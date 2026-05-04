//! storage 实现的结构化长期记忆存储。单文件 `memory/long_term_memories.json`。
//! File-backed structured long-term memory store over storage/state root.

use crate::error::{Error, Result};
use crate::memory::{
    canonicalize_long_term_memory_entry, compare_long_term_memory_query_results,
    govern_long_term_memory_entries, long_term_memory_entry_from_draft,
    long_term_memory_matches_query, merge_long_term_memory_entry,
    score_long_term_memory_recall_breakdown, LongTermMemoryDraft, LongTermMemoryEntry,
    LongTermMemoryQuery, LongTermMemorySlot, LongTermMemoryStore, MAX_LONG_TERM_MEMORY_ITEMS,
    REL_PATH_LONG_TERM_MEMORIES,
};
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_json_file};

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_LONG_TERM_MEMORIES)
}

pub struct StorageLongTermMemoryStore {
    cache: Mutex<Option<Vec<LongTermMemoryEntry>>>,
    path_fn: fn() -> PathBuf,
}

impl StorageLongTermMemoryStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            path_fn: full_path,
        }
    }

    #[cfg(test)]
    fn with_path_fn(path_fn: fn() -> PathBuf) -> Self {
        Self {
            cache: Mutex::new(None),
            path_fn,
        }
    }

    fn load_entries_from_disk(&self) -> Result<Vec<LongTermMemoryEntry>> {
        match read_file((self.path_fn)()) {
            Ok(buf) => {
                if buf.len() <= 2 {
                    Ok(Vec::new())
                } else {
                    serde_json::from_slice::<Vec<LongTermMemoryEntry>>(&buf)
                        .map_err(|error| Error::config("long_term_memory_load", error.to_string()))
                        .map(|entries| {
                            entries
                                .into_iter()
                                .filter_map(canonicalize_long_term_memory_entry)
                                .collect()
                        })
                }
            }
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.with_stage("long_term_memory_load")),
        }
    }

    fn with_entries_mut<R>(
        &self,
        f: impl FnOnce(&mut Vec<LongTermMemoryEntry>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("long_term_memory_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(self.load_entries_from_disk()?);
        }
        let entries = guard
            .as_mut()
            .ok_or_else(|| Error::config("long_term_memory_cache", "cache not initialized"))?;
        if govern_long_term_memory_entries(entries, crate::util::current_unix_secs()) {
            self.persist(entries)?;
        }
        f(entries)
    }

    fn with_entries<R>(&self, f: impl FnOnce(&[LongTermMemoryEntry]) -> Result<R>) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("long_term_memory_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(self.load_entries_from_disk()?);
        }
        let entries = guard
            .as_ref()
            .ok_or_else(|| Error::config("long_term_memory_cache", "cache not initialized"))?;
        f(entries)
    }

    fn persist(&self, entries: &[LongTermMemoryEntry]) -> Result<()> {
        let json = serde_json::to_vec(entries)
            .map_err(|e| Error::config("long_term_memory_persist", e.to_string()))?;
        write_json_file((self.path_fn)(), &json)
    }
}

impl Default for StorageLongTermMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LongTermMemoryStore for StorageLongTermMemoryStore {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize> {
        self.with_entries_mut(|entries| {
            let mut changed = false;
            let mut changed_count = 0usize;
            for draft in drafts {
                let Some(normalized) = draft.normalized() else {
                    continue;
                };
                let Some(id) = normalized.stable_id() else {
                    continue;
                };
                if let Some(existing) = entries.iter_mut().find(|entry| entry.id == id) {
                    if merge_long_term_memory_entry(existing, &normalized, now_secs) {
                        changed = true;
                        changed_count += 1;
                    }
                    continue;
                }

                let Some(entry) = long_term_memory_entry_from_draft(&normalized, id, now_secs)
                else {
                    continue;
                };
                entries.push(entry);
                changed = true;
                changed_count += 1;
            }

            changed |= govern_long_term_memory_entries(entries, now_secs);

            if changed {
                entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                self.persist(entries)?;
            }
            Ok(changed_count)
        })
    }

    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>> {
        let limit = limit.clamp(1, MAX_LONG_TERM_MEMORY_ITEMS);
        let now_secs = crate::util::current_unix_secs();
        self.with_entries(|entries| {
            let mut scored: Vec<(u32, u32, String)> = entries
                .iter()
                .filter_map(|entry| {
                    let breakdown = score_long_term_memory_recall_breakdown(
                        query,
                        source_chat_id,
                        now_secs,
                        entry,
                    );
                    (breakdown.total_score > 0).then(|| {
                        (
                            breakdown.total_score,
                            breakdown.semantic_score,
                            entry.id.clone(),
                        )
                    })
                })
                .collect();
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0).then_with(|| {
                    b.1.cmp(&a.1).then_with(|| {
                        let left = entries
                            .iter()
                            .find(|entry| entry.id == a.2)
                            .map(|entry| entry.updated_at)
                            .unwrap_or(0);
                        let right = entries
                            .iter()
                            .find(|entry| entry.id == b.2)
                            .map(|entry| entry.updated_at)
                            .unwrap_or(0);
                        right.cmp(&left)
                    })
                })
            });
            scored.truncate(limit);
            let selected_ids: Vec<String> = scored.into_iter().map(|(_, _, id)| id).collect();
            let mut out = Vec::with_capacity(selected_ids.len());
            for selected_id in selected_ids {
                if let Some(entry) = entries.iter().find(|entry| entry.id == selected_id) {
                    out.push(entry.clone());
                }
            }
            Ok(out)
        })
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        self.with_entries(|entries| Ok(entries.iter().find(|entry| entry.id == id).cloned()))
    }

    fn get_slot(&self, slot: &LongTermMemorySlot) -> Result<Option<LongTermMemoryEntry>> {
        let Some(id) = slot.stable_id() else {
            return Ok(None);
        };
        self.get(&id)
    }

    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        let normalized = query.normalized();
        let now_secs = crate::util::current_unix_secs();
        self.with_entries(|entries| {
            let mut out = Vec::with_capacity(entries.len().min(normalized.limit));
            for entry in entries.iter() {
                if !long_term_memory_matches_query(entry, &normalized, now_secs) {
                    continue;
                }
                out.push(entry.clone());
            }
            out.sort_by(|left, right| {
                compare_long_term_memory_query_results(left, right, &normalized)
            });
            out.truncate(normalized.limit);
            Ok(out)
        })
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let limit = limit.clamp(1, MAX_LONG_TERM_MEMORY_ITEMS);
        self.with_entries(|entries| {
            let mut out = entries.to_vec();
            out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            out.truncate(limit);
            Ok(out)
        })
    }

    fn delete(&self, id: &str) -> Result<bool> {
        self.with_entries_mut(|entries| {
            let before = entries.len();
            entries.retain(|entry| entry.id != id);
            let removed = before != entries.len();
            if removed {
                self.persist(entries)?;
            }
            Ok(removed)
        })
    }

    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
        let Some(id) = slot.stable_id() else {
            return Ok(false);
        };
        self.delete(&id)
    }

    fn count(&self) -> Result<usize> {
        self.with_entries(|entries| Ok(entries.len()))
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
            let root = std::env::temp_dir().join(format!("beetle-long-term-memory-{unique}"));
            std::fs::create_dir_all(&root).unwrap();
            root.join("long_term_memories.json")
        })
        .clone()
    }

    fn read_paths_test_store_path() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("beetle-long-term-memory-read-{unique}"));
            std::fs::create_dir_all(&root).unwrap();
            root.join("long_term_memories.json")
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
    fn corrupt_long_term_memory_file_fails_closed_without_overwrite() {
        let path = test_store_path();
        reset_test_store(&path);
        super::super::write_file(&path, br#"{"entries": }"#).unwrap();

        let store = StorageLongTermMemoryStore::with_path_fn(test_store_path);
        let load_error = store.list(8).expect_err("corrupt memory file must error");
        assert_eq!(load_error.stage(), "long_term_memory_load");

        let bytes = std::fs::read(&path).expect("read original bytes");
        assert_eq!(bytes, br#"{"entries": }"#);
    }

    #[test]
    fn read_paths_do_not_touch_or_rewrite_long_term_memory_file() {
        let path = read_paths_test_store_path();
        reset_test_store(&path);
        let store = StorageLongTermMemoryStore::with_path_fn(read_paths_test_store_path);
        store
            .upsert_many(
                &[LongTermMemoryDraft {
                    kind: crate::memory::LongTermMemoryKind::Fact,
                    topic: "rust".to_string(),
                    content: "Rust is used in Beetle runtime governance.".to_string(),
                    keywords: vec!["rust".to_string(), "runtime".to_string()],
                    source_chat_id: None,
                    source_type: None,
                    source_scope: None,
                    confidence: None,
                    freshness: None,
                    stale_hint: None,
                    supporting_citations: Vec::new(),
                    evidence_count: None,
                    observed_at: None,
                    last_confirmed_at: None,
                    source_revision: None,
                }],
                1,
            )
            .unwrap();
        let before = std::fs::read(&path).expect("read memory file before query");

        let recalled = store.recall("rust runtime", None, 4).unwrap();
        assert_eq!(recalled.len(), 1);
        let id = recalled[0].id.clone();
        assert!(store.get(&id).unwrap().is_some());
        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.list(8).unwrap().len(), 1);
        assert_eq!(
            store
                .query(&LongTermMemoryQuery {
                    kind: Some(crate::memory::LongTermMemoryKind::Fact),
                    topic: Some("rust".to_string()),
                    source_scope: None,
                    source_chat_id: None,
                    freshness: None,
                    include_stale: true,
                    limit: 8,
                })
                .unwrap()
                .len(),
            1
        );

        let after = std::fs::read(&path).expect("read memory file after query");
        assert_eq!(
            after, before,
            "long-term memory read paths must not persist last_used/governance from hot routes or turn prepare"
        );
    }
}
