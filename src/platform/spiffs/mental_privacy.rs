//! SPIFFS implementation of mental privacy state store.

use crate::error::Result;
use crate::memory::{MentalPrivacyState, MentalPrivacyStore, REL_PATH_MENTAL_PRIVACY_STATES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_MENTAL_PRIVACY_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredMentalPrivacyState(MentalPrivacyState);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_MENTAL_PRIVACY_STATES)
}

pub struct SpiffsMentalPrivacyStore {
    store: CachedJsonFileStore<HashMap<String, StoredMentalPrivacyState>>,
}

impl SpiffsMentalPrivacyStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "mental_privacy_cache_lock",
                "mental_privacy_cache",
                "mental_privacy_persist",
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
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|state| state.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, state: &MentalPrivacyState) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next = StoredMentalPrivacyState(state.clone());
            if map.get(chat_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_MENTAL_PRIVACY_CHATS {
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
