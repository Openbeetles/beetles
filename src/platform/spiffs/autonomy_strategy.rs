//! SPIFFS 实现的 Autonomy Strategy 存储。单文件 memory/autonomy_strategies.json。

use crate::error::Result;
use crate::memory::{AutonomyStrategy, AutonomyStrategyStore, REL_PATH_AUTONOMY_STRATEGIES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_AUTONOMY_STRATEGY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredAutonomyStrategy(AutonomyStrategy);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_AUTONOMY_STRATEGIES)
}

pub struct SpiffsAutonomyStrategyStore {
    store: CachedJsonFileStore<HashMap<String, StoredAutonomyStrategy>>,
}

impl SpiffsAutonomyStrategyStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "autonomy_strategy_cache_lock",
                "autonomy_strategy_cache",
                "autonomy_strategy_persist",
            ),
        }
    }
}

impl Default for SpiffsAutonomyStrategyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AutonomyStrategyStore for SpiffsAutonomyStrategyStore {
    fn get(&self, chat_id: &str) -> Result<Option<AutonomyStrategy>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|strategy| strategy.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, strategy: &AutonomyStrategy) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredAutonomyStrategy(strategy.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_AUTONOMY_STRATEGY_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next);
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
