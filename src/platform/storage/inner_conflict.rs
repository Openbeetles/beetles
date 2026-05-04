//! storage 实现的 Inner Conflict 存储。单文件 memory/inner_conflicts.json。

use crate::error::Result;
use crate::memory::{InnerConflict, InnerConflictStore, REL_PATH_INNER_CONFLICTS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_INNER_CONFLICT_SCOPES: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredInnerConflict(InnerConflict);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_INNER_CONFLICTS)
}

pub struct StorageInnerConflictStore {
    store: ChatScopedCachedJsonMapStore<StoredInnerConflict>,
}

impl StorageInnerConflictStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "inner_conflict_cache_lock",
                "inner_conflict_cache",
                "inner_conflict_persist",
                MAX_INNER_CONFLICT_SCOPES,
            ),
        }
    }
}

impl Default for StorageInnerConflictStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerConflictStore for StorageInnerConflictStore {
    fn get(&self, scope_id: &str) -> Result<Option<InnerConflict>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|conflict| conflict.0))
    }

    fn set(&self, scope_id: &str, conflict: &InnerConflict) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredInnerConflict(conflict.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
