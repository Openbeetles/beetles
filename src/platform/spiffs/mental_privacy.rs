//! SPIFFS implementation of mental privacy state store.

use crate::error::Result;
use crate::memory::{MentalPrivacyState, MentalPrivacyStore, REL_PATH_MENTAL_PRIVACY_STATES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_MENTAL_PRIVACY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredMentalPrivacyState(MentalPrivacyState);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_MENTAL_PRIVACY_STATES)
}

pub struct SpiffsMentalPrivacyStore {
    store: ChatScopedCachedJsonMapStore<StoredMentalPrivacyState>,
}

impl SpiffsMentalPrivacyStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "mental_privacy_cache_lock",
                "mental_privacy_cache",
                "mental_privacy_persist",
                MAX_MENTAL_PRIVACY_CHATS,
            ),
        }
    }
}

impl Default for SpiffsMentalPrivacyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MentalPrivacyStore for SpiffsMentalPrivacyStore {
    fn get(&self, chat_id: &str) -> Result<Option<MentalPrivacyState>> {
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|state| state.0))
    }

    fn set(&self, chat_id: &str, state: &MentalPrivacyState) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredMentalPrivacyState(state.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
