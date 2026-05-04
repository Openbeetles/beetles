//! storage implementation of Core Revision Ledger store. Single-file JSON map.

use crate::error::Result;
use crate::memory::{CoreRevisionLedger, CoreRevisionLedgerStore, REL_PATH_CORE_REVISION_LEDGERS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_CORE_REVISION_LEDGER_SCOPES: usize = 8;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredCoreRevisionLedger(CoreRevisionLedger);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_CORE_REVISION_LEDGERS)
}

pub struct StorageCoreRevisionLedgerStore {
    store: ChatScopedCachedJsonMapStore<StoredCoreRevisionLedger>,
}

impl StorageCoreRevisionLedgerStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "core_revision_ledger_cache_lock",
                "core_revision_ledger_cache",
                "core_revision_ledger_persist",
                MAX_CORE_REVISION_LEDGER_SCOPES,
            ),
        }
    }
}

impl Default for StorageCoreRevisionLedgerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CoreRevisionLedgerStore for StorageCoreRevisionLedgerStore {
    fn get(&self, scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|ledger| ledger.0))
    }

    fn set(&self, scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredCoreRevisionLedger(ledger.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
