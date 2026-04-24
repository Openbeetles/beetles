use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, write_json_file};

pub(crate) struct StoreOp<R> {
    pub(crate) result: R,
    pub(crate) dirty: bool,
}

impl<R> StoreOp<R> {
    pub(crate) fn clean(result: R) -> Self {
        Self {
            result,
            dirty: false,
        }
    }

    pub(crate) fn dirty(result: R) -> Self {
        Self {
            result,
            dirty: true,
        }
    }

    pub(crate) fn with_dirty(result: R, dirty: bool) -> Self {
        Self { result, dirty }
    }
}

pub(crate) struct CachedJsonFileStore<T> {
    cache: Mutex<Option<T>>,
    path_fn: fn() -> PathBuf,
    load_fn: fn(&PathBuf, &'static str) -> Result<T>,
    stage_cache_lock: &'static str,
    stage_cache: &'static str,
    stage_persist: &'static str,
}

impl<T> CachedJsonFileStore<T>
where
    T: Default + Serialize + DeserializeOwned,
{
    pub(crate) fn new(
        path_fn: fn() -> PathBuf,
        load_fn: fn(&PathBuf, &'static str) -> Result<T>,
        stage_cache_lock: &'static str,
        stage_cache: &'static str,
        stage_persist: &'static str,
    ) -> Self {
        Self {
            cache: Mutex::new(None),
            path_fn,
            load_fn,
            stage_cache_lock,
            stage_cache,
            stage_persist,
        }
    }

    pub(crate) fn with_cached_mut<R>(
        &self,
        f: impl FnOnce(&mut T) -> Result<StoreOp<R>>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config(self.stage_cache_lock, e.to_string()))?;
        if guard.is_none() {
            *guard = Some(self.load_from_disk()?);
        }
        let value = guard
            .as_mut()
            .ok_or_else(|| Error::config(self.stage_cache, "cache not initialized"))?;
        let op = f(value)?;
        if op.dirty {
            self.persist(value)?;
        }
        Ok(op.result)
    }

    fn load_from_disk(&self) -> Result<T> {
        (self.load_fn)(&(self.path_fn)(), self.stage_cache)
    }

    fn persist(&self, value: &T) -> Result<()> {
        let json = serde_json::to_vec(value)
            .map_err(|e| Error::config(self.stage_persist, e.to_string()))?;
        write_json_file((self.path_fn)(), &json)
    }
}

pub(crate) fn load_json_or_default<T>(path: &PathBuf, stage: &'static str) -> Result<T>
where
    T: Default + DeserializeOwned,
{
    match read_file(path) {
        Ok(buf) if buf.len() > 2 => {
            serde_json::from_slice(&buf).map_err(|error| Error::config(stage, error.to_string()))
        }
        Ok(_) => Ok(T::default()),
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(T::default())
        }
        Err(error) => Err(error.with_stage(stage)),
    }
}

pub(crate) struct ChatScopedCachedJsonMapStore<V>
where
    V: Clone + PartialEq + Serialize + DeserializeOwned,
{
    store: CachedJsonFileStore<HashMap<String, V>>,
    max_entries: usize,
}

impl<V> ChatScopedCachedJsonMapStore<V>
where
    V: Clone + PartialEq + Serialize + DeserializeOwned,
{
    pub(crate) fn new(
        path_fn: fn() -> PathBuf,
        stage_cache_lock: &'static str,
        stage_cache: &'static str,
        stage_persist: &'static str,
        max_entries: usize,
    ) -> Self {
        Self {
            store: CachedJsonFileStore::new(
                path_fn,
                load_json_or_default,
                stage_cache_lock,
                stage_cache,
                stage_persist,
            ),
            max_entries,
        }
    }

    #[inline(never)]
    pub(crate) fn get_cloned(&self, chat_id: &str) -> Result<Option<V>> {
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.get(chat_id).cloned())))
    }

    #[inline(never)]
    pub(crate) fn set_owned(&self, chat_id: &str, next: V) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= self.max_entries {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next);
            Ok(StoreOp::dirty(()))
        })
    }

    #[inline(never)]
    pub(crate) fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.remove(chat_id).is_some() {
                return Ok(StoreOp::dirty(()));
            }
            Ok(StoreOp::clean(()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_store_path() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("beetle-cached-json-{unique}"));
            std::fs::create_dir_all(&root).unwrap();
            root.join("cached.json")
        })
        .clone()
    }

    fn reset_test_store(path: &Path) {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    #[test]
    fn corrupt_cached_json_does_not_downgrade_into_empty_writeback() {
        let path = test_store_path();
        reset_test_store(&path);
        super::super::write_file(&path, br#"{"chat-1": }"#).unwrap();

        let store = ChatScopedCachedJsonMapStore::<u32>::new(
            test_store_path,
            "cached_json_test_lock",
            "cached_json_test_cache",
            "cached_json_test_persist",
            8,
        );

        let load_error = store
            .get_cloned("chat-1")
            .expect_err("corrupt cache must error");
        assert_eq!(load_error.stage(), "cached_json_test_cache");

        let set_error = store
            .set_owned("chat-2", 7)
            .expect_err("mutating a corrupt cache must fail closed");
        assert_eq!(set_error.stage(), "cached_json_test_cache");

        let bytes = std::fs::read(&path).expect("read original bytes");
        assert_eq!(bytes, br#"{"chat-1": }"#);
    }
}
