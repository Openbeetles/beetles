//! SPIFFS 实现的 Self Model 存储。单文件 memory/self_models.json。

use crate::error::Result;
use crate::memory::{REL_PATH_SELF_MODELS, SelfModel, SelfModelStore};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_SELF_MODEL_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSelfModel(SelfModel);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SELF_MODELS)
}

pub struct SpiffsSelfModelStore {
    store: ChatScopedCachedJsonMapStore<StoredSelfModel>,
}

impl SpiffsSelfModelStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "self_model_cache_lock",
                "self_model_cache",
                "self_model_persist",
                MAX_SELF_MODEL_CHATS,
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
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|model| model.0))
    }

    fn set(&self, chat_id: &str, model: &SelfModel) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredSelfModel(model.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
