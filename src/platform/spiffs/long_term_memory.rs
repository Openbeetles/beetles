//! SPIFFS 实现的结构化长期记忆存储。单文件 `memory/long_term_memories.json`。
//! File-backed structured long-term memory store over SPIFFS/state root.

use crate::error::{Error, Result};
use crate::memory::{
    canonicalize_long_term_memory_entry, compare_long_term_memory_query_results,
    govern_long_term_memory_entries, long_term_memory_entry_from_draft,
    long_term_memory_matches_query, merge_long_term_memory_entry, score_long_term_memory_recall,
    touch_long_term_memory_usage, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryQuery,
    LongTermMemorySlot, LongTermMemoryStore, MAX_LONG_TERM_MEMORY_ITEMS,
    REL_PATH_LONG_TERM_MEMORIES,
};
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_LONG_TERM_MEMORIES)
}

pub struct SpiffsLongTermMemoryStore {
    cache: Mutex<Option<Vec<LongTermMemoryEntry>>>,
}

impl SpiffsLongTermMemoryStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_entries_from_disk() -> Vec<LongTermMemoryEntry> {
        match read_file(full_path()) {
            Ok(buf) => {
                if buf.len() <= 2 {
                    Vec::new()
                } else {
                    serde_json::from_slice::<Vec<LongTermMemoryEntry>>(&buf)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(canonicalize_long_term_memory_entry)
                        .collect()
                }
            }
            Err(_) => Vec::new(),
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
            *guard = Some(Self::load_entries_from_disk());
        }
        let entries = guard
            .as_mut()
            .ok_or_else(|| Error::config("long_term_memory_cache", "cache not initialized"))?;
        if govern_long_term_memory_entries(entries, crate::util::current_unix_secs()) {
            Self::persist(entries)?;
        }
        f(entries)
    }

    fn persist(entries: &[LongTermMemoryEntry]) -> Result<()> {
        let json = serde_json::to_vec(entries)
            .map_err(|e| Error::config("long_term_memory_persist", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsLongTermMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LongTermMemoryStore for SpiffsLongTermMemoryStore {
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
                Self::persist(entries)?;
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
        self.with_entries_mut(|entries| {
            let mut scored: Vec<(u32, String)> = entries
                .iter()
                .filter_map(|entry| {
                    let score =
                        score_long_term_memory_recall(query, source_chat_id, now_secs, entry);
                    (score > 0).then(|| (score, entry.id.clone()))
                })
                .collect();
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0).then_with(|| {
                    let left = entries
                        .iter()
                        .find(|entry| entry.id == a.1)
                        .map(|entry| entry.updated_at)
                        .unwrap_or(0);
                    let right = entries
                        .iter()
                        .find(|entry| entry.id == b.1)
                        .map(|entry| entry.updated_at)
                        .unwrap_or(0);
                    right.cmp(&left)
                })
            });
            scored.truncate(limit);
            let mut touched = false;
            let selected_ids: Vec<String> = scored.into_iter().map(|(_, id)| id).collect();
            let mut out = Vec::with_capacity(selected_ids.len());
            for selected_id in selected_ids {
                if let Some(entry) = entries.iter_mut().find(|entry| entry.id == selected_id) {
                    touched |= touch_long_term_memory_usage(entry, now_secs);
                    out.push(entry.clone());
                }
            }
            if touched {
                Self::persist(entries)?;
            }
            Ok(out)
        })
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        let now_secs = crate::util::current_unix_secs();
        self.with_entries_mut(|entries| {
            let mut touched = false;
            let item = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .map(|entry| {
                    touched = touch_long_term_memory_usage(entry, now_secs);
                    entry.clone()
                });
            if touched {
                Self::persist(entries)?;
            }
            Ok(item)
        })
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
        self.with_entries_mut(|entries| {
            let mut touched = false;
            let mut out = Vec::with_capacity(entries.len().min(normalized.limit));
            for entry in entries.iter_mut() {
                if !long_term_memory_matches_query(entry, &normalized, now_secs) {
                    continue;
                }
                touched |= touch_long_term_memory_usage(entry, now_secs);
                out.push(entry.clone());
            }
            out.sort_by(|left, right| {
                compare_long_term_memory_query_results(left, right, &normalized)
            });
            out.truncate(normalized.limit);
            if touched {
                Self::persist(entries)?;
            }
            Ok(out)
        })
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let limit = limit.clamp(1, MAX_LONG_TERM_MEMORY_ITEMS);
        self.with_entries_mut(|entries| {
            let mut out = entries.clone();
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
                Self::persist(entries)?;
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
        self.with_entries_mut(|entries| Ok(entries.len()))
    }
}
