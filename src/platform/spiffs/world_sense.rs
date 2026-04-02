//! SPIFFS 实现的 World Sense 存储。单文件 memory/world_sense.json。

use crate::error::Result;
use crate::memory::{WorldSense, WorldSenseStore, REL_PATH_WORLD_SENSE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_WORLD_SENSE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredWorldSense(WorldSense);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_WORLD_SENSE)
}

pub struct SpiffsWorldSenseStore {
    store: CachedJsonFileStore<HashMap<String, StoredWorldSense>>,
}

impl SpiffsWorldSenseStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "world_sense_cache_lock",
                "world_sense_cache",
                "world_sense_persist",
            ),
        }
    }
}

impl Default for SpiffsWorldSenseStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldSenseStore for SpiffsWorldSenseStore {
    fn get(&self, chat_id: &str) -> Result<Option<WorldSense>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|world_sense| world_sense.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, world_sense: &WorldSense) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredWorldSense(world_sense.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_WORLD_SENSE_CHATS {
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
