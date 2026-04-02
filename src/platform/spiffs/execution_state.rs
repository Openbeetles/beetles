//! SPIFFS 实现的执行状态存储。单文件 memory/execution_states.json。

use crate::error::Result;
use crate::memory::{ExecutionState, ExecutionStateStore, REL_PATH_EXECUTION_STATES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_EXECUTION_STATE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredExecutionState(ExecutionState);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_EXECUTION_STATES)
}

pub struct SpiffsExecutionStateStore {
    store: CachedJsonFileStore<HashMap<String, StoredExecutionState>>,
}

impl SpiffsExecutionStateStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "execution_state_cache_lock",
                "execution_state_cache",
                "execution_state_persist",
            ),
        }
    }
}

impl Default for SpiffsExecutionStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionStateStore for SpiffsExecutionStateStore {
    fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|state| state.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next_state = StoredExecutionState(state.clone());
            if map.get(chat_id) == Some(&next_state) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_EXECUTION_STATE_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next_state);
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
