//! storage 实现的长期记忆提取状态存储。单文件 memory/long_term_extraction_states.json。
//! File-backed long-term memory extraction state store.

use crate::error::Result;
use crate::memory::{
    LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
    REL_PATH_LONG_TERM_EXTRACTION_STATES,
};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_LONG_TERM_EXTRACTION_STATE_CHATS: usize = 64;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_LONG_TERM_EXTRACTION_STATES)
}

pub struct StorageLongTermMemoryExtractionStateStore {
    store: ChatScopedCachedJsonMapStore<LongTermMemoryExtractionState>,
}

impl StorageLongTermMemoryExtractionStateStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "long_term_extraction_state_cache_lock",
                "long_term_extraction_state_cache",
                "long_term_extraction_state_persist",
                MAX_LONG_TERM_EXTRACTION_STATE_CHATS,
            ),
        }
    }
}

impl Default for StorageLongTermMemoryExtractionStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LongTermMemoryExtractionStateStore for StorageLongTermMemoryExtractionStateStore {
    fn get(&self, chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
        self.store.get_cloned(chat_id)
    }

    fn set(&self, chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
        self.store.set_owned(chat_id, state.clone())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
