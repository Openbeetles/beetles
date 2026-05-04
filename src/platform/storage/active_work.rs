//! storage-backed active foreground work store.
//! 基于 storage 的前台主工作存储。

use crate::agent::{ActiveWorkRecord, ActiveWorkStore, REL_PATH_ACTIVE_WORKS};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_ACTIVE_WORK_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredActiveWork(ActiveWorkRecord);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_ACTIVE_WORKS)
}

pub struct StorageActiveWorkStore {
    store: ChatScopedCachedJsonMapStore<StoredActiveWork>,
}

impl StorageActiveWorkStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "active_work_cache_lock",
                "active_work_cache",
                "active_work_persist",
                MAX_ACTIVE_WORK_CHATS,
            ),
        }
    }
}

impl Default for StorageActiveWorkStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveWorkStore for StorageActiveWorkStore {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|record| record.0))
    }

    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredActiveWork(record.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
