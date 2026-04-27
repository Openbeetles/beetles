//! SPIFFS 实现的 Felt Significance 存储。单文件 memory/felt_significances.json。

use crate::error::Result;
use crate::memory::{FeltSignificance, FeltSignificanceStore, REL_PATH_FELT_SIGNIFICANCES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_FELT_SIGNIFICANCE_SCOPES: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredFeltSignificance(FeltSignificance);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_FELT_SIGNIFICANCES)
}

pub struct SpiffsFeltSignificanceStore {
    store: ChatScopedCachedJsonMapStore<StoredFeltSignificance>,
}

impl SpiffsFeltSignificanceStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "felt_significance_cache_lock",
                "felt_significance_cache",
                "felt_significance_persist",
                MAX_FELT_SIGNIFICANCE_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsFeltSignificanceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl FeltSignificanceStore for SpiffsFeltSignificanceStore {
    fn get(&self, scope_id: &str) -> Result<Option<FeltSignificance>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|significance| significance.0))
    }

    fn set(&self, scope_id: &str, significance: &FeltSignificance) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredFeltSignificance(significance.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
