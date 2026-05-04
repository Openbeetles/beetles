//! storage 实现的 Autonomy Strategy 存储。单文件 memory/autonomy_strategies.json。

use crate::error::Result;
use crate::memory::{AutonomyStrategy, AutonomyStrategyStore, REL_PATH_AUTONOMY_STRATEGIES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_AUTONOMY_STRATEGY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredAutonomyStrategy(AutonomyStrategy);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_AUTONOMY_STRATEGIES)
}

pub struct StorageAutonomyStrategyStore {
    store: ChatScopedCachedJsonMapStore<StoredAutonomyStrategy>,
}

impl StorageAutonomyStrategyStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "autonomy_strategy_cache_lock",
                "autonomy_strategy_cache",
                "autonomy_strategy_persist",
                MAX_AUTONOMY_STRATEGY_CHATS,
            ),
        }
    }
}

impl Default for StorageAutonomyStrategyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl AutonomyStrategyStore for StorageAutonomyStrategyStore {
    fn get(&self, chat_id: &str) -> Result<Option<AutonomyStrategy>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|strategy| strategy.0))
    }

    fn set(&self, chat_id: &str, strategy: &AutonomyStrategy) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredAutonomyStrategy(strategy.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
