//! storage 实现的 Inner Life 存储。单文件 memory/inner_life.json。

use crate::error::Result;
use crate::memory::{InnerLife, InnerLifeStore, REL_PATH_INNER_LIFE};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_INNER_LIFE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredInnerLife(InnerLife);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_INNER_LIFE)
}

pub struct StorageInnerLifeStore {
    store: ChatScopedCachedJsonMapStore<StoredInnerLife>,
}

impl StorageInnerLifeStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "inner_life_cache_lock",
                "inner_life_cache",
                "inner_life_persist",
                MAX_INNER_LIFE_CHATS,
            ),
        }
    }
}

impl Default for StorageInnerLifeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerLifeStore for StorageInnerLifeStore {
    fn get(&self, chat_id: &str) -> Result<Option<InnerLife>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|inner_life| inner_life.0))
    }

    fn set(&self, chat_id: &str, inner_life: &InnerLife) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredInnerLife(inner_life.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
