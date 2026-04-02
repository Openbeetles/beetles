//! SPIFFS 实现的 Inner Life 存储。单文件 memory/inner_life.json。

use crate::error::Result;
use crate::memory::{InnerLife, InnerLifeStore, REL_PATH_INNER_LIFE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_INNER_LIFE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredInnerLife(InnerLife);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_INNER_LIFE)
}

pub struct SpiffsInnerLifeStore {
    store: CachedJsonFileStore<HashMap<String, StoredInnerLife>>,
}

impl SpiffsInnerLifeStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "inner_life_cache_lock",
                "inner_life_cache",
                "inner_life_persist",
            ),
        }
    }
}

impl Default for SpiffsInnerLifeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerLifeStore for SpiffsInnerLifeStore {
    fn get(&self, chat_id: &str) -> Result<Option<InnerLife>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|inner_life| inner_life.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, inner_life: &InnerLife) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredInnerLife(inner_life.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_INNER_LIFE_CHATS {
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
