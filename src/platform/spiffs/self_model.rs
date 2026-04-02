//! SPIFFS 实现的 Self Model 存储。单文件 memory/self_models.json。

use crate::error::Result;
use crate::memory::{SelfModel, SelfModelStore, REL_PATH_SELF_MODELS};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_SELF_MODEL_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSelfModel(SelfModel);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SELF_MODELS)
}

pub struct SpiffsSelfModelStore {
    store: CachedJsonFileStore<HashMap<String, StoredSelfModel>>,
}

impl SpiffsSelfModelStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "self_model_cache_lock",
                "self_model_cache",
                "self_model_persist",
            ),
        }
    }
}

impl Default for SpiffsSelfModelStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfModelStore for SpiffsSelfModelStore {
    fn get(&self, chat_id: &str) -> Result<Option<SelfModel>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|model| model.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, model: &SelfModel) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next_model = StoredSelfModel(model.clone());
            if map.get(chat_id) == Some(&next_model) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_SELF_MODEL_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next_model);
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
