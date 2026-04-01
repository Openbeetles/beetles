//! SPIFFS 实现的执行状态存储。单文件 memory/execution_states.json。

use crate::error::{Error, Result};
use crate::memory::{ExecutionState, ExecutionStateStore, REL_PATH_EXECUTION_STATES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_EXECUTION_STATE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredExecutionState(ExecutionState);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_EXECUTION_STATES)
}

pub struct SpiffsExecutionStateStore {
    cache: Mutex<Option<HashMap<String, StoredExecutionState>>>,
}

impl SpiffsExecutionStateStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, StoredExecutionState> {
        match read_file(full_path()) {
            Ok(buf) if buf.len() > 2 => serde_json::from_slice(&buf).unwrap_or_default(),
            _ => HashMap::new(),
        }
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, StoredExecutionState>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("execution_state_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("execution_state_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, StoredExecutionState>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("execution_state_set", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsExecutionStateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionStateStore for SpiffsExecutionStateStore {
    fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
        self.with_map_mut(|map| Ok(map.get(chat_id).map(|state| state.0.clone())))
    }

    fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
        self.with_map_mut(|map| {
            let next_state = StoredExecutionState(state.clone());
            if map.get(chat_id) == Some(&next_state) {
                return Ok(());
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_EXECUTION_STATE_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next_state);
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
