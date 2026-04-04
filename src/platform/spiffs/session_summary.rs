//! SPIFFS 实现的会话摘要存储。单文件 memory/session_summaries.json，chat 数上界 32。
//! SessionSummaryStore implementation; single JSON file, chat_id -> { summary, last_summary_at_count }.

use crate::constants::SESSION_SUMMARY_MAX_LEN;
use crate::error::Result;
use crate::memory::{REL_PATH_SESSION_SUMMARIES, SessionSummaryStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

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
    store: ChatScopedCachedJsonMapStore<SummaryEntry>,
}

impl SpiffsSessionSummaryStore {
    pub fn new() -> Self {
        SpiffsSessionSummaryStore {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "session_summary_cache_lock",
                "session_summary_cache",
                "session_summary_persist",
                MAX_SESSION_SUMMARY_CHATS,
            ),
        }
    }
}

impl Default for SpiffsSessionSummaryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionSummaryStore for SpiffsSessionSummaryStore {
    fn get(&self, chat_id: &str) -> Result<Option<String>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|entry| entry.summary))
    }

    fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
        self.set_with_count(chat_id, summary, 0)
    }

    fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
        self.store.set_owned(
            chat_id,
            SummaryEntry {
                summary: truncate_summary(summary),
                last_summary_at_count: message_count,
            },
        )
    }

    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|entry| (entry.summary, entry.last_summary_at_count)))
    }
}
