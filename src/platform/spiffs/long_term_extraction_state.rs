//! SPIFFS 实现的长期记忆提取状态存储。单文件 memory/long_term_extraction_states.json。
//! File-backed long-term memory extraction state store.

use crate::error::Result;
use crate::memory::{
    LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
    REL_PATH_LONG_TERM_EXTRACTION_STATES,
};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_LONG_TERM_EXTRACTION_STATE_CHATS: usize = 64;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_LONG_TERM_EXTRACTION_STATES)
}

pub struct SpiffsLongTermMemoryExtractionStateStore {
    store: CachedJsonFileStore<HashMap<String, LongTermMemoryExtractionState>>,
}

impl SpiffsLongTermMemoryExtractionStateStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "long_term_extraction_state_cache_lock",
                "long_term_extraction_state_cache",
                "long_term_extraction_state_persist",
            ),
        }
    }
}

impl Default for SpiffsLongTermMemoryExtractionStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LongTermMemoryExtractionStateStore for SpiffsLongTermMemoryExtractionStateStore {
    fn get(&self, chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.get(chat_id).cloned())))
    }

    fn set(&self, chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.get(chat_id) == Some(state) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_LONG_TERM_EXTRACTION_STATE_CHATS {
                let key_to_remove = map.keys().next().cloned();
                if let Some(key) = key_to_remove {
                    map.remove(&key);
                }
            }
            map.insert(chat_id.to_string(), state.clone());
            Ok(StoreOp::dirty(()))
        })
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.remove(chat_id).is_some() {
                return Ok(StoreOp::dirty(()));
            }
            Ok(StoreOp::clean(()))
        })
    }
}
