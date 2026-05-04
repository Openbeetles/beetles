//! storage 实现的 World Sense 存储。单文件 memory/world_sense.json。

use crate::error::Result;
use crate::memory::{WorldSense, WorldSenseStore, REL_PATH_WORLD_SENSE};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_WORLD_SENSE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredWorldSense(WorldSense);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_WORLD_SENSE)
}

pub struct StorageWorldSenseStore {
    store: ChatScopedCachedJsonMapStore<StoredWorldSense>,
}

impl StorageWorldSenseStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "world_sense_cache_lock",
                "world_sense_cache",
                "world_sense_persist",
                MAX_WORLD_SENSE_CHATS,
            ),
        }
    }
}

impl Default for StorageWorldSenseStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldSenseStore for StorageWorldSenseStore {
    fn get(&self, chat_id: &str) -> Result<Option<WorldSense>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|world_sense| world_sense.0))
    }

    fn set(&self, chat_id: &str, world_sense: &WorldSense) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredWorldSense(world_sense.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
