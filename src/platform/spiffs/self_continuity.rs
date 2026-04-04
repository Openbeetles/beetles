//! SPIFFS 实现的 Self Continuity 存储。单文件 memory/self_continuities.json。

use crate::error::Result;
use crate::memory::{SelfContinuity, SelfContinuityStore, REL_PATH_SELF_CONTINUITIES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_SELF_CONTINUITY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredSelfContinuity(SelfContinuity);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_SELF_CONTINUITIES)
}

pub struct SpiffsSelfContinuityStore {
    store: ChatScopedCachedJsonMapStore<StoredSelfContinuity>,
}

impl SpiffsSelfContinuityStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "self_continuity_cache_lock",
                "self_continuity_cache",
                "self_continuity_persist",
                MAX_SELF_CONTINUITY_CHATS,
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
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|continuity| continuity.0))
    }

    fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredSelfContinuity(continuity.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
