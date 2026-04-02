//! SPIFFS 实现的最近一轮执行账本存储。单文件 memory/turn_ledgers.json。

use crate::error::Result;
use crate::memory::{TurnLedger, TurnLedgerStore, REL_PATH_TURN_LEDGERS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_TURN_LEDGER_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedger(TurnLedger);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TURN_LEDGERS)
}

pub struct SpiffsTurnLedgerStore {
    store: CachedJsonFileStore<HashMap<String, StoredTurnLedger>>,
}

impl SpiffsTurnLedgerStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path(),
                load_json_or_default,
                "turn_ledger_cache_lock",
                "turn_ledger_cache",
                "turn_ledger_persist",
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
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|ledger| ledger.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredTurnLedger(ledger.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_TURN_LEDGER_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next);
            Ok(StoreOp::dirty(()))
        })
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.remove(chat_id).is_some() {
                return Ok(StoreOp::dirty(()));
            }
            Ok(StoreOp::clean(()))
        })
    }
}
