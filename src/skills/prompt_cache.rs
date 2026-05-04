use crate::platform::{SkillMetaStore, SkillStorage};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct PromptCacheState {
    loaded: bool,
    rendered: String,
}

/// 技能描述字符串缓存，供 system prompt 直接复用，避免在用户消息热路径里重读 storage。
pub struct SkillPromptCache {
    meta_store: Arc<dyn SkillMetaStore + Send + Sync>,
    storage: Arc<dyn SkillStorage + Send + Sync>,
    max_chars: usize,
    state: Mutex<PromptCacheState>,
}

impl SkillPromptCache {
    pub fn new(
        meta_store: Arc<dyn SkillMetaStore + Send + Sync>,
        storage: Arc<dyn SkillStorage + Send + Sync>,
        max_chars: usize,
    ) -> Self {
        Self {
            meta_store,
            storage,
            max_chars,
            state: Mutex::new(PromptCacheState::default()),
        }
    }

    pub fn get(&self) -> String {
        {
            let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.loaded {
                return state.rendered.clone();
            }
        }
        self.refresh()
    }

    pub fn refresh(&self) -> String {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match crate::skills::try_build_skill_descriptions_for_system_prompt(
            self.meta_store.as_ref(),
            self.storage.as_ref(),
            self.max_chars,
        ) {
            Ok(rendered) => {
                state.loaded = true;
                state.rendered = rendered.clone();
                rendered
            }
            Err(error) => {
                log::warn!(
                    "[skills] prompt cache refresh retained last known good render because meta read failed: {}",
                    error
                );
                if state.loaded {
                    state.rendered.clone()
                } else {
                    String::new()
                }
            }
        }
    }

    pub fn invalidate(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.loaded = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct FakeMetaStore {
        reads: AtomicUsize,
        order: Mutex<Vec<String>>,
        disabled: Mutex<Vec<String>>,
        fail_reads: AtomicBool,
    }

    impl FakeMetaStore {
        fn new(order: &[&str], disabled: &[&str]) -> Self {
            Self {
                reads: AtomicUsize::new(0),
                order: Mutex::new(order.iter().map(|value| (*value).to_string()).collect()),
                disabled: Mutex::new(disabled.iter().map(|value| (*value).to_string()).collect()),
                fail_reads: AtomicBool::new(false),
            }
        }
    }

    impl SkillMetaStore for FakeMetaStore {
        fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(crate::error::Error::config(
                    "fake_skill_meta_read",
                    "meta read failed",
                ));
            }
            Ok((
                self.order.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                self.disabled
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            ))
        }

        fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()> {
            *self.order.lock().unwrap_or_else(|e| e.into_inner()) = order.to_vec();
            *self.disabled.lock().unwrap_or_else(|e| e.into_inner()) = disabled.to_vec();
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeSkillStorage {
        list_reads: AtomicUsize,
        file_reads: AtomicUsize,
        names: Mutex<Vec<String>>,
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl FakeSkillStorage {
        fn new(entries: &[(&str, &str)]) -> Self {
            let names: Vec<String> = entries
                .iter()
                .map(|(name, _)| (*name).to_string())
                .collect();
            let files = entries
                .iter()
                .map(|(name, content)| ((*name).to_string(), content.as_bytes().to_vec()))
                .collect();
            Self {
                list_reads: AtomicUsize::new(0),
                file_reads: AtomicUsize::new(0),
                names: Mutex::new(names),
                files: Mutex::new(files),
            }
        }
    }

    impl SkillStorage for FakeSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            self.list_reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.names.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.file_reads.fetch_add(1, Ordering::SeqCst);
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            let mut names = self.names.lock().unwrap_or_else(|e| e.into_inner());
            if !names.iter().any(|value| value == name) {
                names.push(name.to_string());
            }
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.names
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|value| value != name);
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[test]
    fn prompt_cache_uses_cached_render_until_invalidated() {
        let meta = Arc::new(FakeMetaStore::new(&["alpha"], &[]));
        let storage = Arc::new(FakeSkillStorage::new(&[("alpha", "alpha body")]));
        let cache = SkillPromptCache::new(
            Arc::clone(&meta) as Arc<dyn SkillMetaStore + Send + Sync>,
            Arc::clone(&storage) as Arc<dyn SkillStorage + Send + Sync>,
            256,
        );

        let first = cache.get();
        let second = cache.get();

        assert!(first.contains("alpha body"));
        assert_eq!(first, second);
        assert_eq!(meta.reads.load(Ordering::SeqCst), 1);
        assert_eq!(storage.list_reads.load(Ordering::SeqCst), 1);
        assert_eq!(storage.file_reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn prompt_cache_rebuilds_after_invalidate() {
        let meta = Arc::new(FakeMetaStore::new(&["alpha"], &[]));
        let storage = Arc::new(FakeSkillStorage::new(&[("alpha", "alpha body")]));
        let cache = SkillPromptCache::new(
            Arc::clone(&meta) as Arc<dyn SkillMetaStore + Send + Sync>,
            Arc::clone(&storage) as Arc<dyn SkillStorage + Send + Sync>,
            256,
        );

        assert!(cache.get().contains("alpha body"));
        storage.write("alpha", b"updated body").unwrap();
        cache.invalidate();

        let refreshed = cache.get();
        assert!(refreshed.contains("updated body"));
        assert_eq!(storage.list_reads.load(Ordering::SeqCst), 2);
        assert_eq!(storage.file_reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn prompt_cache_retains_last_known_good_render_when_meta_read_fails() {
        let meta = Arc::new(FakeMetaStore::new(&["alpha"], &[]));
        let storage = Arc::new(FakeSkillStorage::new(&[("alpha", "alpha body")]));
        let cache = SkillPromptCache::new(
            Arc::clone(&meta) as Arc<dyn SkillMetaStore + Send + Sync>,
            Arc::clone(&storage) as Arc<dyn SkillStorage + Send + Sync>,
            256,
        );

        let first = cache.get();
        meta.fail_reads.store(true, Ordering::SeqCst);
        storage
            .write("alpha", b"new body that should not leak")
            .unwrap();

        let second = cache.refresh();

        assert_eq!(first, second);
        assert!(second.contains("alpha body"));
        assert!(!second.contains("new body that should not leak"));
    }
}
