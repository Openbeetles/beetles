//! SPIFFS 实现的结构化长期记忆存储。单文件 `memory/long_term_memories.json`。
//! File-backed structured long-term memory store over SPIFFS/state root.

use crate::error::{Error, Result};
use crate::memory::{
    score_long_term_memory_recall, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryStore,
    MAX_LONG_TERM_MEMORY_ITEMS, REL_PATH_LONG_TERM_MEMORIES,
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
                    serde_json::from_slice(&buf).unwrap_or_default()
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
            for draft in drafts {
                let Some(normalized) = draft.normalized() else {
                    continue;
                };
                let Some(id) = normalized.stable_id() else {
                    continue;
                };
                if let Some(existing) = entries.iter_mut().find(|entry| entry.id == id) {
                    let mut merged_keywords = existing.keywords.clone();
                    for keyword in normalized.keywords {
                        if merged_keywords.iter().any(|item| item == &keyword) {
                            continue;
                        }
                        merged_keywords.push(keyword);
                    }
                    merged_keywords.truncate(crate::memory::MAX_LONG_TERM_MEMORY_KEYWORDS);
                    if existing.keywords != merged_keywords {
                        existing.keywords = merged_keywords;
                        changed = true;
                    }
                    if existing.source_chat_id.is_none() && normalized.source_chat_id.is_some() {
                        existing.source_chat_id = normalized.source_chat_id.clone();
                        changed = true;
                    }
                    continue;
                }

                entries.push(LongTermMemoryEntry {
                    id,
                    kind: normalized.kind,
                    content: normalized.content,
                    keywords: normalized.keywords,
                    source_chat_id: normalized.source_chat_id,
                    created_at: now_secs,
                });
                changed = true;
            }

            if entries.len() > MAX_LONG_TERM_MEMORY_ITEMS {
                entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                entries.truncate(MAX_LONG_TERM_MEMORY_ITEMS);
                changed = true;
            }

            if changed {
                entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                Self::persist(entries)?;
            }
            Ok(entries.len())
        })
    }

    fn recall(&self, query: &str, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let limit = limit.clamp(
            crate::memory::DEFAULT_LONG_TERM_MEMORY_RECALL_LIMIT,
            MAX_LONG_TERM_MEMORY_ITEMS,
        );
        self.with_entries_mut(|entries| {
            let mut scored: Vec<(u32, LongTermMemoryEntry)> = entries
                .iter()
                .filter_map(|entry| {
                    let score = score_long_term_memory_recall(query, entry);
                    (score > 0).then(|| (score, entry.clone()))
                })
                .collect();
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| b.1.created_at.cmp(&a.1.created_at))
            });
            scored.truncate(limit);
            Ok(scored.into_iter().map(|(_, entry)| entry).collect())
        })
    }

    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
        self.with_entries_mut(|entries| Ok(entries.iter().find(|entry| entry.id == id).cloned()))
    }

    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
        let limit = limit.clamp(1, MAX_LONG_TERM_MEMORY_ITEMS);
        self.with_entries_mut(|entries| {
            let mut out = entries.clone();
            out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
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

    fn count(&self) -> Result<usize> {
        self.with_entries_mut(|entries| Ok(entries.len()))
    }
}
