//! SPIFFS 实现的执行状态存储。单文件 memory/execution_states.json。

use crate::error::Result;
use crate::memory::{ExecutionState, ExecutionStateStore, REL_PATH_EXECUTION_STATES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_EXECUTION_STATE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredExecutionState(ExecutionState);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_EXECUTION_STATES)
}

pub struct SpiffsExecutionStateStore {
    store: ChatScopedCachedJsonMapStore<StoredExecutionState>,
}

impl SpiffsExecutionStateStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "execution_state_cache_lock",
                "execution_state_cache",
                "execution_state_persist",
                MAX_EXECUTION_STATE_CHATS,
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
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|state| state.0))
    }

    fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredExecutionState(state.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
