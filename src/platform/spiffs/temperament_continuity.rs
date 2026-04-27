//! SPIFFS 实现的 Temperament Continuity 存储。单文件 memory/temperament_continuities.json。

use crate::error::Result;
use crate::memory::{
    TemperamentContinuity, TemperamentContinuityStore, REL_PATH_TEMPERAMENT_CONTINUITIES,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_TEMPERAMENT_CONTINUITY_SCOPES: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTemperamentContinuity(TemperamentContinuity);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TEMPERAMENT_CONTINUITIES)
}

pub struct SpiffsTemperamentContinuityStore {
    store: ChatScopedCachedJsonMapStore<StoredTemperamentContinuity>,
}

impl SpiffsTemperamentContinuityStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "temperament_continuity_cache_lock",
                "temperament_continuity_cache",
                "temperament_continuity_persist",
                MAX_TEMPERAMENT_CONTINUITY_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsTemperamentContinuityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TemperamentContinuityStore for SpiffsTemperamentContinuityStore {
    fn get(&self, scope_id: &str) -> Result<Option<TemperamentContinuity>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|continuity| continuity.0))
    }

    fn set(&self, scope_id: &str, continuity: &TemperamentContinuity) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredTemperamentContinuity(continuity.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
