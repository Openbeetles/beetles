//! SPIFFS 实现的最近一轮执行账本存储。单文件 memory/turn_ledgers.json。

use crate::error::Result;
use crate::memory::{REL_PATH_TURN_LEDGERS, TurnLedger, TurnLedgerStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_TURN_LEDGER_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedger(TurnLedger);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TURN_LEDGERS)
}

pub struct SpiffsTurnLedgerStore {
    store: ChatScopedCachedJsonMapStore<StoredTurnLedger>,
}

impl SpiffsTurnLedgerStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "turn_ledger_cache_lock",
                "turn_ledger_cache",
                "turn_ledger_persist",
                MAX_TURN_LEDGER_CHATS,
            ),
        }
    }
}

impl Default for SpiffsTurnLedgerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnLedgerStore for SpiffsTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|ledger| ledger.0))
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredTurnLedger(ledger.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
