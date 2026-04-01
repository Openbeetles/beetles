//! SPIFFS 实现的会话摘要存储。单文件 memory/session_summaries.json，chat 数上界 32。
//! SessionSummaryStore implementation; single JSON file, chat_id -> { summary, last_summary_at_count }.

use crate::constants::SESSION_SUMMARY_MAX_LEN;
use crate::error::{Error, Result};
use crate::memory::{SessionSummaryStore, REL_PATH_SESSION_SUMMARIES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_SESSION_SUMMARY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SummaryEntry {
    summary: String,
    last_summary_at_count: usize,
}

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SESSION_SUMMARIES)
}

fn truncate_summary(s: &str) -> String {
    if s.chars().count() <= SESSION_SUMMARY_MAX_LEN {
        s.to_string()
    } else {
        s.chars().take(SESSION_SUMMARY_MAX_LEN).collect::<String>()
    }
}

pub struct SpiffsSessionSummaryStore {
    cache: Mutex<Option<HashMap<String, SummaryEntry>>>,
}

impl SpiffsSessionSummaryStore {
    pub fn new() -> Self {
        SpiffsSessionSummaryStore {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, SummaryEntry> {
        let path = full_path();
        match read_file(&path) {
            Ok(buf) => {
                if buf.len() <= 2 {
                    HashMap::new()
                } else {
                    serde_json::from_slice(&buf).unwrap_or_default()
                }
            }
            Err(_) => HashMap::new(),
        }
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, SummaryEntry>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("session_summary_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("session_summary_cache", "cache not initialized"))?;
        f(map)
    }
}

impl Default for SpiffsSessionSummaryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionSummaryStore for SpiffsSessionSummaryStore {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        self.with_map_mut(|map| Ok(map.get(chat_id).map(|e| e.summary.clone())))
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        self.set_with_count(chat_id, summary, 0)
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.with_map_mut(|map| {
            let next_entry = SummaryEntry {
                summary: truncate_summary(summary),
                last_summary_at_count: message_count,
            };
            if map.get(chat_id) == Some(&next_entry) {
                return Ok(());
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_SESSION_SUMMARY_CHATS {
                let key_to_remove = map.keys().next().cloned();
                if let Some(k) = key_to_remove {
                    map.remove(&k);
                }
            }
            map.insert(chat_id.to_string(), next_entry);
            let json = serde_json::to_vec(map)
                .map_err(|e| Error::config("session_summary_set", e.to_string()))?;
            write_file(full_path(), &json)?;
            Ok(())
        })
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        self.with_map_mut(|map| {
            Ok(map
                .get(chat_id)
                .map(|e| (e.summary.clone(), e.last_summary_at_count)))
        })
    }
}
