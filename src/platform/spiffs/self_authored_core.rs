//! SPIFFS implementation of Self-Authored Core store. Single-file JSON map.

use crate::error::Result;
use crate::memory::{
    REL_PATH_SELF_AUTHORED_CORES, SelfAuthoredCore, SelfAuthoredCoreStore,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_SELF_AUTHORED_CORE_SCOPES: usize = 8;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSelfAuthoredCore(SelfAuthoredCore);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SELF_AUTHORED_CORES)
}

pub struct SpiffsSelfAuthoredCoreStore {
    store: ChatScopedCachedJsonMapStore<StoredSelfAuthoredCore>,
}

impl SpiffsSelfAuthoredCoreStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "self_authored_core_cache_lock",
                "self_authored_core_cache",
                "self_authored_core_persist",
                MAX_SELF_AUTHORED_CORE_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsSelfAuthoredCoreStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfAuthoredCoreStore for SpiffsSelfAuthoredCoreStore {
    fn get(&self, scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|core| core.0))
    }

    fn set(&self, scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredSelfAuthoredCore(core.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
