//! storage 实现的 Outer Voice 存储。单文件 memory/outer_voices.json。

use crate::error::Result;
use crate::memory::{OuterVoice, OuterVoiceStore, REL_PATH_OUTER_VOICES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_OUTER_VOICE_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredOuterVoice(OuterVoice);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_OUTER_VOICES)
}

pub struct StorageOuterVoiceStore {
    store: ChatScopedCachedJsonMapStore<StoredOuterVoice>,
}

impl StorageOuterVoiceStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "outer_voice_cache_lock",
                "outer_voice_cache",
                "outer_voice_persist",
                MAX_OUTER_VOICE_CHATS,
            ),
        }
    }
}

impl Default for StorageOuterVoiceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OuterVoiceStore for StorageOuterVoiceStore {
    fn get(&self, chat_id: &str) -> Result<Option<OuterVoice>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|outer_voice| outer_voice.0))
    }

    fn set(&self, chat_id: &str, outer_voice: &OuterVoice) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredOuterVoice(outer_voice.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
