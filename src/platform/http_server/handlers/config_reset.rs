//! POST /api/config_reset：需带配对码；成功后恢复默认配置并清除配对码（回到未激活）。

use crate::config;
use crate::platform::http_server::common::ApiResponse;
use crate::platform::pairing;
use crate::platform::ConfigStore;

use super::HandlerContext;

pub fn post(ctx: &HandlerContext) -> Result<ApiResponse, std::io::Error> {
    post_with_hooks(
        ctx.config_store.as_ref(),
        |rel_path| ctx.platform.remove_config_file(rel_path),
        || ctx.reload_config(),
    )
}

const CONFIG_RESET_FILE_PATHS: &[&str] = &[
    "config/skills_meta.json",
    "config/llm.json",
    "config/channels.json",
    "config/accounts.json",
    "config/office_credentials.json",
    "config/hardware.json",
    "config/audio.json",
    "config/display.json",
    "runtime/office_runtime_status.json",
];

fn run_config_reset<F>(
    config_store: &dyn ConfigStore,
    mut remove_config_file: F,
) -> Result<(), std::io::Error>
where
    F: FnMut(&str) -> crate::error::Result<()>,
{
    config::reset_to_defaults(config_store)
        .map_err(|error| std::io::Error::other(format!("reset_to_defaults failed: {error}")))?;
    pairing::clear_code(config_store)
        .map_err(|error| std::io::Error::other(format!("clear_code failed: {error}")))?;
    crate::runtime::sync_pairing_state_from_store(config_store);
    for rel_path in CONFIG_RESET_FILE_PATHS {
        remove_config_file(rel_path).map_err(|error| {
            std::io::Error::other(format!("remove_config_file failed for {rel_path}: {error}"))
        })?;
    }
    Ok(())
}

fn post_with_hooks<F, G>(
    config_store: &dyn ConfigStore,
    mut remove_config_file: F,
    reload_config: G,
) -> Result<ApiResponse, std::io::Error>
where
    F: FnMut(&str) -> crate::error::Result<()>,
    G: FnOnce(),
{
    let result = run_config_reset(config_store, |rel_path| remove_config_file(rel_path));
    reload_config();
    result?;
    Ok(ApiResponse::ok_200_json("{\"ok\":true}"))
}

#[cfg(test)]
mod tests {
    use super::{post_with_hooks, run_config_reset, CONFIG_RESET_FILE_PATHS};
    use crate::platform::ConfigStore;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryConfigStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl MemoryConfigStore {
        fn with_pairing_code(code: &str) -> Self {
            let mut values = HashMap::new();
            values.insert("pairing_code".to_string(), code.to_string());
            Self {
                values: Mutex::new(values),
            }
        }
    }

    impl ConfigStore for MemoryConfigStore {
        fn read_string(&self, key: &str) -> crate::error::Result<Option<String>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(key)
                .cloned())
        }

        fn write_string(&self, key: &str, value: &str) -> crate::error::Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn erase_keys(&self, keys: &[&str]) -> crate::error::Result<()> {
            let mut values = self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            for key in keys {
                values.remove(*key);
            }
            Ok(())
        }
    }

    struct PairingStateRestore {
        pairing_state_known: bool,
        pairing_required: bool,
    }

    impl PairingStateRestore {
        fn capture() -> Self {
            Self {
                pairing_state_known: crate::state::pairing_state_known(),
                pairing_required: crate::state::pairing_required(),
            }
        }
    }

    impl Drop for PairingStateRestore {
        fn drop(&mut self) {
            crate::state::set_pairing_state_known(self.pairing_state_known);
            crate::state::set_pairing_required(self.pairing_required);
        }
    }

    #[test]
    fn run_config_reset_syncs_pairing_state_and_removes_runtime_files() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let _state_guard = crate::state::test_state_guard();
        let _pairing_state = PairingStateRestore::capture();
        crate::state::set_pairing_state_known(false);
        crate::state::set_pairing_required(false);
        let store = MemoryConfigStore::with_pairing_code("123456");
        let removed = Mutex::new(Vec::new());

        run_config_reset(&store, |rel_path| {
            removed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(rel_path.to_string());
            Ok(())
        })
        .expect("reset should succeed");

        assert!(!crate::platform::pairing::code_set(&store));
        assert!(crate::state::pairing_state_known());
        assert!(crate::state::pairing_required());
        assert_eq!(
            removed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            CONFIG_RESET_FILE_PATHS
        );
    }

    #[test]
    fn run_config_reset_reports_cleanup_failures_instead_of_false_success() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let _state_guard = crate::state::test_state_guard();
        let _pairing_state = PairingStateRestore::capture();
        crate::state::set_pairing_state_known(false);
        crate::state::set_pairing_required(false);
        let store = MemoryConfigStore::with_pairing_code("123456");

        let error = run_config_reset(&store, |rel_path| {
            if rel_path == "config/accounts.json" {
                return Err(crate::error::Error::config(
                    "config_reset_test",
                    "synthetic remove failure",
                ));
            }
            Ok(())
        })
        .expect_err("cleanup failure should be surfaced");

        assert!(error.to_string().contains("config/accounts.json"));
        assert!(crate::state::pairing_state_known());
        assert!(crate::state::pairing_required());
    }

    #[test]
    fn post_with_hooks_reloads_cached_config_after_successful_reset() {
        let store = MemoryConfigStore::with_pairing_code("123456");
        let reloaded = AtomicBool::new(false);

        post_with_hooks(
            &store,
            |_| Ok(()),
            || reloaded.store(true, Ordering::Relaxed),
        )
        .expect("post should succeed");

        assert!(reloaded.load(Ordering::Relaxed));
    }

    #[test]
    fn post_with_hooks_reloads_cached_config_after_failed_reset() {
        let store = MemoryConfigStore::with_pairing_code("123456");
        let reloaded = AtomicBool::new(false);

        let result = post_with_hooks(
            &store,
            |rel_path| {
                if rel_path == "config/accounts.json" {
                    return Err(crate::error::Error::config(
                        "config_reset_test",
                        "synthetic remove failure",
                    ));
                }
                Ok(())
            },
            || reloaded.store(true, Ordering::Relaxed),
        );

        let error = match result {
            Ok(_) => panic!("post should surface cleanup failure"),
            Err(error) => error,
        };

        assert!(reloaded.load(Ordering::Relaxed));
        assert!(error.to_string().contains("config/accounts.json"));
    }
}
