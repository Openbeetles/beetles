//! SPIFFS 实现的长期记忆提取状态存储。单文件 memory/long_term_extraction_states.json。
//! File-backed long-term memory extraction state store.

use crate::error::{Error, Result};
use crate::memory::{
    LongTermMemoryExtractionState, LongTermMemoryExtractionStateStore,
    REL_PATH_LONG_TERM_EXTRACTION_STATES,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_LONG_TERM_EXTRACTION_STATE_CHATS: usize = 64;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_LONG_TERM_EXTRACTION_STATES)
}

pub struct SpiffsLongTermMemoryExtractionStateStore {
    cache: Mutex<Option<HashMap<String, LongTermMemoryExtractionState>>>,
}

impl SpiffsLongTermMemoryExtractionStateStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, LongTermMemoryExtractionState> {
        match read_file(full_path()) {
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
        f: impl FnOnce(&mut HashMap<String, LongTermMemoryExtractionState>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("long_term_extraction_state_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard.as_mut().ok_or_else(|| {
            Error::config("long_term_extraction_state_cache", "cache not initialized")
        })?;
        f(map)
    }

    fn persist(map: &HashMap<String, LongTermMemoryExtractionState>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("long_term_extraction_state_persist", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsLongTermMemoryExtractionStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl LongTermMemoryExtractionStateStore for SpiffsLongTermMemoryExtractionStateStore {
    fn get(&self, chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
        self.with_map_mut(|map| Ok(map.get(chat_id).cloned()))
    }

    fn set(&self, chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
        self.with_map_mut(|map| {
            if !map.contains_key(chat_id) && map.len() >= MAX_LONG_TERM_EXTRACTION_STATE_CHATS {
                let key_to_remove = map.keys().next().cloned();
                if let Some(key) = key_to_remove {
                    map.remove(&key);
                }
            }
            map.insert(chat_id.to_string(), state.clone());
            Self::persist(map)
        })
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.with_map_mut(|map| {
            if map.remove(chat_id).is_some() {
                Self::persist(map)?;
            }
            Ok(())
        })
    }
}
