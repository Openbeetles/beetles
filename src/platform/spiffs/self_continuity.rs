//! SPIFFS 实现的 Self Continuity 存储。单文件 memory/self_continuities.json。

use crate::error::Result;
use crate::memory::{SelfContinuity, SelfContinuityStore, REL_PATH_SELF_CONTINUITIES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_SELF_CONTINUITY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSelfContinuity(SelfContinuity);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SELF_CONTINUITIES)
}

pub struct SpiffsSelfContinuityStore {
    store: CachedJsonFileStore<HashMap<String, StoredSelfContinuity>>,
}

impl SpiffsSelfContinuityStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "self_continuity_cache_lock",
                "self_continuity_cache",
                "self_continuity_persist",
            ),
        }
    }
}

impl Default for SpiffsSelfContinuityStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfContinuityStore for SpiffsSelfContinuityStore {
    fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|continuity| continuity.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredSelfContinuity(continuity.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_SELF_CONTINUITY_CHATS {
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
