//! storage / state-root backed continuity capsule store.

use crate::error::{Error, Result};
use crate::memory::{
    apply_continuity_capsule_drafts, canonicalize_continuity_capsule, ContinuityCapsule,
    ContinuityCapsuleDraft, ContinuityCapsuleStore, ContinuityCapsuleWriteOutcome,
    MAX_CONTINUITY_CAPSULES, REL_PATH_CONTINUITY_CAPSULES,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{read_file, state_path_join, write_json_file};

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_CONTINUITY_CAPSULES)
}

pub struct StorageContinuityCapsuleStore {
    cache: Mutex<Option<Vec<ContinuityCapsule>>>,
    path_fn: Arc<dyn Fn() -> PathBuf + Send + Sync>,
}

impl StorageContinuityCapsuleStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            path_fn: Arc::new(full_path),
        }
    }

    #[cfg(test)]
    fn with_path(path: PathBuf) -> Self {
        Self {
            cache: Mutex::new(None),
            path_fn: Arc::new(move || path.clone()),
        }
    }

    fn load_entries_from_disk(&self) -> Result<Vec<ContinuityCapsule>> {
        match read_file((self.path_fn.as_ref())()) {
            Ok(buf) => {
                if buf.len() <= 2 {
                    Ok(Vec::new())
                } else {
                    serde_json::from_slice::<Vec<ContinuityCapsule>>(&buf)
                        .map_err(|error| {
                            Error::config("continuity_capsule_load", error.to_string())
                        })
                        .map(|entries| {
                            entries
                                .into_iter()
                                .filter_map(canonicalize_continuity_capsule)
                                .collect()
                        })
                }
            }
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.with_stage("continuity_capsule_load")),
        }
    }

    fn with_entries_mut<R>(
        &self,
        f: impl FnOnce(&mut Vec<ContinuityCapsule>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("continuity_capsule_cache_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(self.load_entries_from_disk()?);
        }
        let entries = guard
            .as_mut()
            .ok_or_else(|| Error::config("continuity_capsule_cache", "cache not initialized"))?;
        f(entries)
    }

    fn persist(&self, entries: &[ContinuityCapsule]) -> Result<()> {
        let json = serde_json::to_vec(entries)
            .map_err(|error| Error::config("continuity_capsule_persist", error.to_string()))?;
        write_json_file((self.path_fn.as_ref())(), &json)
    }
}

impl Default for StorageContinuityCapsuleStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ContinuityCapsuleStore for StorageContinuityCapsuleStore {
    fn upsert_many(
        &self,
        drafts: &[ContinuityCapsuleDraft],
        now_secs: u64,
    ) -> Result<ContinuityCapsuleWriteOutcome> {
        self.with_entries_mut(|entries| {
            let outcome = apply_continuity_capsule_drafts(entries, drafts, now_secs);
            if outcome.upserted > 0 || outcome.superseded > 0 {
                self.persist(entries)?;
            }
            Ok(outcome)
        })
    }

    fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
        self.with_entries_mut(|entries| {
            Ok(entries
                .iter()
                .find(|entry| entry.capsule_id == capsule_id)
                .cloned())
        })
    }

    fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
        let limit = limit.clamp(1, MAX_CONTINUITY_CAPSULES);
        self.with_entries_mut(|entries| {
            let mut out = entries.clone();
            out.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            out.truncate(limit);
            Ok(out)
        })
    }

    fn count(&self) -> Result<usize> {
        self.with_entries_mut(|entries| Ok(entries.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_store_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("beetle-continuity-capsule-{label}-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        root.join("continuity_capsules.json")
    }

    fn reset_test_store(path: &Path) {
        let _ = std::fs::remove_file(path);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }

    #[test]
    fn store_roundtrip_persists_capsules() {
        let path = unique_test_store_path("roundtrip");
        reset_test_store(&path);
        let store = StorageContinuityCapsuleStore::with_path(path.clone());
        let outcome = store
            .upsert_many(
                &[ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    topic: "memory enhancement".to_string(),
                    summary: "close P4-B1".to_string(),
                    ..Default::default()
                }],
                42,
            )
            .unwrap();
        assert_eq!(outcome.upserted, 1);
        let items = store.list(8).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].topic, "memory enhancement");
        let store_reload = StorageContinuityCapsuleStore::with_path(path.clone());
        let reloaded = store_reload.list(8).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].summary, "close P4-B1");
    }

    #[test]
    fn corrupt_capsule_file_fails_closed_without_overwrite() {
        let path = unique_test_store_path("corrupt");
        reset_test_store(&path);
        super::super::write_file(&path, br#"{"capsules": }"#).unwrap();

        let store = StorageContinuityCapsuleStore::with_path(path.clone());
        let load_error = store.list(8).expect_err("corrupt capsule file must error");
        assert_eq!(load_error.stage(), "continuity_capsule_load");

        let bytes = std::fs::read(&path).expect("read original bytes");
        assert_eq!(bytes, br#"{"capsules": }"#);
    }
}
