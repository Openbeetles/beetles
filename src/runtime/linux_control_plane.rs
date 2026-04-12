//! Linux supervisor-owned control plane.

use crate::bus::new_inbound_channel;
use crate::channel_capability::build_channel_capability_registry;
use crate::channels::QqMsgIdCache;
use crate::config::AppConfig;
use crate::error::{Error, Result};
use crate::platform::http_server::common::CORS_HEADERS;
use crate::platform::http_server::handlers::{ControlPlaneRouteContract, HandlerContext};
use crate::platform::http_server::linux_runtime::{self, LinuxHttpServerSpec};
use crate::platform::http_server::router::{
    self, IncomingRequest, OutgoingResponse, RestartAction, RouterEnv,
};
use crate::platform::Platform;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};

const LINUX_HTTP_WORKERS: usize = 4;

pub fn spawn(platform: Arc<dyn Platform>) -> Result<crate::util::TaskHandle> {
    crate::util::spawn_guarded_with_profile_handle(
        "linux_control_plane",
        crate::util::STACK_CHANNEL_SENDER,
        Some(crate::util::SpawnCore::Core0),
        crate::util::HttpThreadRole::Background,
        move || {
            if let Err(error) = run(platform) {
                log::error!("[linux_control_plane] server exited: {}", error);
                crate::state::set_last_error(&error);
            }
        },
    )
    .map_err(|e| Error::io("linux_control_plane_spawn", e))
}

fn run(platform: Arc<dyn Platform>) -> Result<()> {
    crate::orchestrator::register_memory_snapshot_provider(Arc::new({
        let platform = Arc::clone(&platform);
        move || platform.memory_snapshot()
    }));
    crate::orchestrator::init();
    if let Err(error) = crate::platform::csrf::init() {
        return Err(Error::config("linux_control_plane_csrf", error.to_string()));
    }
    let soul_kernel_report = crate::runtime::ensure_platform_soul_kernel_recovery(
        platform.as_ref(),
        crate::util::current_unix_secs(),
    );
    if soul_kernel_report.restore_attempted {
        log::info!(
            "[linux_control_plane] soul_kernel recovery action={:?} restored_snapshots={} restored_layers={} degraded_after={}",
            soul_kernel_report.action,
            soul_kernel_report.restored_snapshots,
            soul_kernel_report.restored_layers.len(),
            soul_kernel_report.status_after.degraded,
        );
    } else {
        log::info!(
            "[linux_control_plane] soul_kernel ready={} safe_mode_readable={} degraded={}",
            soul_kernel_report.status_after.minimum_viable,
            soul_kernel_report.status_after.safe_mode_minimum_readable,
            soul_kernel_report.status_after.degraded,
        );
    }

    let config = crate::bootstrap::load_config(&platform);
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
        Arc::clone(&skill_meta_store),
        Arc::clone(&skill_storage),
        8192,
    ));
    let (tool_registry, _) = crate::build_default_registry(
        config.as_ref(),
        crate::DefaultRegistryDeps {
            platform: Arc::clone(&platform),
            remind_at_store: platform.remind_at_store(),
            session_store: platform.session_store(),
            memory_store: platform.memory_store(),
            long_term_memory_store: platform.long_term_memory_store(),
            turn_ledger_store: platform.turn_ledger_store(),
            private_garden_store: platform.private_garden_store(),
            config_store: platform.config_store(),
        },
    );
    let tool_registry = Arc::new(tool_registry);
    let channel_capability_registry =
        Arc::new(build_channel_capability_registry(config.as_ref(), false));
    let ctx = Arc::new(
        crate::platform::http_server::handlers::build_runtime_handler_context(
            Arc::clone(&platform),
            tool_registry,
            channel_capability_registry,
            Arc::new(AtomicUsize::new(0)),
            Arc::new(AtomicUsize::new(0)),
            platform.memory_store(),
            platform.session_store(),
            None,
            skill_prompt_cache,
            Arc::new(RwLock::new((*config).clone())),
            config.llm_stream,
            ControlPlaneRouteContract::SUPERVISOR_MINIMAL,
        ),
    );
    let router_env = Arc::new(build_router_env(config.as_ref()));
    let _active_guard = crate::runtime::ConfigPlaneGuard::enter();
    let listen =
        std::env::var("BEETLE_CONFIG_HTTP_LISTEN").unwrap_or_else(|_| "0.0.0.0:80".to_string());
    linux_runtime::run_linux_http_server(
        LinuxHttpServerSpec {
            log_tag: "linux_control_plane",
            listen_stage: "linux_control_plane_listen",
            listen_log: format!(
                "[linux_control_plane] listening on {} (supervisor-owned control plane)",
                listen
            ),
            listen_addr: listen,
            worker_name_prefix: "linux_control_plane_worker_",
            worker_count: LINUX_HTTP_WORKERS,
        },
        move |incoming| {
            dispatch(&ctx, router_env.as_ref(), incoming).unwrap_or_else(|error| {
                log::warn!("[linux_control_plane] dispatch failed: {}", error);
                OutgoingResponse::json(
                    500,
                    "Internal Server Error",
                    CORS_HEADERS,
                    br#"{"error":"internal error"}"#.to_vec(),
                )
            })
        },
        move |_path, restart| {
            if restart != RestartAction::After300Ms {
                return;
            }
            crate::util::spawn_guarded_with_profile(
                "linux_control_plane_restart_defer",
                crate::util::STACK_RESTART_DEFER,
                Some(crate::util::SpawnCore::Core0),
                crate::util::HttpThreadRole::Background,
                move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    if let Err(error) = crate::runtime::linux_supervisor::request_restart() {
                        log::warn!(
                            "[linux_control_plane] failed to request restart after response: {}",
                            error
                        );
                    }
                },
            );
        },
    )
}

fn dispatch(
    ctx: &HandlerContext,
    router_env: &RouterEnv,
    incoming: IncomingRequest,
) -> Result<OutgoingResponse> {
    let path = incoming.uri.split('?').next().unwrap_or("/");
    if !ctx.route_contract.inbound_webhooks_enabled && supervisor_blocks_webhook_route(path) {
        return Ok(OutgoingResponse::json(
            404,
            "Not Found",
            CORS_HEADERS,
            br#"{"error":"not found"}"#.to_vec(),
        ));
    }
    router::dispatch(ctx, router_env, incoming)
}

fn build_router_env(config: &AppConfig) -> RouterEnv {
    let (inbound_tx, _inbound_rx, _inbound_depth) =
        new_inbound_channel(crate::constants::DEFAULT_CAPACITY);
    let qq_msg_id_cache: QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
    RouterEnv::new(
        inbound_tx,
        qq_msg_id_cache,
        false,
        config.qq_channel_app_id.clone(),
        config.qq_channel_secret.clone(),
    )
}

fn supervisor_blocks_webhook_route(path: &str) -> bool {
    matches!(
        path,
        "/api/webhook"
            | "/api/feishu/event"
            | "/api/dingtalk/webhook"
            | "/api/wecom/webhook"
            | "/api/webhook/qq"
    )
}

#[cfg(test)]
mod tests {
    use super::{build_router_env, dispatch};
    use crate::config::AppConfig;
    use crate::error::Result;
    use crate::platform::http_server::handlers::{ControlPlaneRouteContract, HandlerContext};
    use crate::platform::http_server::router::IncomingRequest;
    use crate::platform::{ConfigStore, Platform};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct TestConfigStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl ConfigStore for TestConfigStore {
        fn read_string(&self, key: &str) -> Result<Option<String>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(key)
                .cloned())
        }

        fn write_string(&self, key: &str, value: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(key.to_string(), value.to_string());
            Ok(())
        }

        fn erase_keys(&self, keys: &[&str]) -> Result<()> {
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

    fn build_test_context() -> (HandlerContext, super::RouterEnv) {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let config_store: Arc<dyn ConfigStore + Send + Sync> = Arc::new(TestConfigStore::default());
        let skill_storage = platform.skill_storage();
        assert!(crate::platform::pairing::set_code(config_store.as_ref(), "123456",).unwrap());

        let ctx = crate::platform::http_server::handlers::build_test_handler_context(
            config.clone(),
            platform,
            config_store,
            skill_storage,
            ControlPlaneRouteContract::SUPERVISOR_MINIMAL,
            "linux",
        );
        let router_env = build_router_env(&config);
        (ctx, router_env)
    }

    fn request(method: &str, uri: &str) -> IncomingRequest {
        IncomingRequest {
            method: method.to_string(),
            uri: uri.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn authed_request(method: &str, uri: &str, body: &str) -> IncomingRequest {
        crate::platform::csrf::init().expect("init csrf");
        let csrf = crate::platform::csrf::get_token().expect("csrf token");
        IncomingRequest {
            method: method.to_string(),
            uri: uri.to_string(),
            headers: vec![
                ("X-Pairing-Code".to_string(), "123456".to_string()),
                ("X-CSRF-Token".to_string(), csrf),
                ("Content-Type".to_string(), "application/json".to_string()),
            ],
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn supervisor_root_inventory_exposes_shared_tools_and_skills_routes() {
        let (ctx, router_env) = build_test_context();
        let out = dispatch(&ctx, &router_env, request("GET", "/")).expect("dispatch /");
        assert_eq!(out.status, 200);

        let body = String::from_utf8(out.body).expect("utf8 body");
        let parsed: Value = serde_json::from_str(&body).expect("root json");
        let endpoints = parsed
            .get("endpoints")
            .and_then(Value::as_array)
            .expect("endpoints array");
        let endpoint_values: Vec<&str> = endpoints.iter().filter_map(Value::as_str).collect();

        assert_eq!(parsed.get("name").and_then(Value::as_str), Some("beetle"));
        assert!(endpoint_values.contains(&"GET /api/tools"));
        assert!(endpoint_values.contains(&"GET /api/skills"));
        assert!(!endpoint_values.contains(&"POST /api/webhook"));
        assert!(!endpoint_values.contains(&"POST /api/webhook/qq"));
    }

    #[test]
    fn supervisor_control_plane_dispatches_tools_and_skills_routes() {
        let (ctx, router_env) = build_test_context();

        let tools = dispatch(&ctx, &router_env, request("GET", "/api/tools")).expect("tools");
        assert_eq!(tools.status, 200);

        let skills = dispatch(&ctx, &router_env, request("GET", "/api/skills")).expect("skills");
        assert_eq!(skills.status, 200);
    }

    #[test]
    fn build_test_context_keeps_activation_isolated_between_contexts() {
        let (ctx_a, router_env_a) = build_test_context();
        let (ctx_b, router_env_b) = build_test_context();

        crate::platform::pairing::clear_code(ctx_a.config_store.as_ref()).unwrap();

        let blocked = dispatch(&ctx_a, &router_env_a, request("GET", "/api/tools")).expect("ctx_a");
        assert_eq!(blocked.status, 401);

        let allowed = dispatch(&ctx_b, &router_env_b, request("GET", "/api/tools")).expect("ctx_b");
        assert_eq!(allowed.status, 200);
    }

    #[test]
    fn supervisor_control_plane_keeps_webhook_ingress_unavailable() {
        let (ctx, router_env) = build_test_context();
        let out =
            dispatch(&ctx, &router_env, request("POST", "/api/webhook")).expect("webhook dispatch");
        assert_eq!(out.status, 404);
    }

    #[test]
    fn supervisor_control_plane_persists_operator_maintenance_requests() {
        let _guard = crate::runtime::operator_maintenance_test_guard();
        let (ctx, router_env) = build_test_context();
        let state_root = crate::platform::state_mount_path();
        if state_root.is_file() {
            let _ = std::fs::remove_file(&state_root);
        }
        std::fs::create_dir_all(&state_root).expect("create state root");
        let maintenance_root = state_root.join("runtime/operator_maintenance");
        let _ = std::fs::remove_dir_all(&maintenance_root);
        let out = dispatch(
            &ctx,
            &router_env,
            authed_request(
                "POST",
                "/api/memory/maintenance",
                r#"{"action":"replay_recovery"}"#,
            ),
        )
        .expect("maintenance dispatch");
        assert_eq!(out.status, 202);

        let parsed: Value = serde_json::from_slice(&out.body).expect("response json");
        assert_eq!(parsed["accepted"], true);
        assert_eq!(parsed["delivery"], "persisted_bridge");
        let _ = std::fs::remove_dir_all(maintenance_root);
    }
}
