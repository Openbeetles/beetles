//! SPIFFS / state-root backed calendar provider credential store.

use crate::calendar::{
    CalendarProviderCredential, CalendarProviderCredentialStatus, CalendarProviderCredentialStore,
    REL_PATH_CALENDAR_PROVIDER_CREDENTIALS,
};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_CALENDAR_PROVIDER_CREDENTIALS: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
struct StoredCalendarProviderCredential(CalendarProviderCredential);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_CALENDAR_PROVIDER_CREDENTIALS)
}

pub struct SpiffsCalendarProviderCredentialStore {
    cache: Mutex<Option<HashMap<String, StoredCalendarProviderCredential>>>,
}

impl SpiffsCalendarProviderCredentialStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, StoredCalendarProviderCredential> {
        match read_file(full_path()) {
            Ok(buf) if buf.len() > 2 => serde_json::from_slice(&buf).unwrap_or_default(),
            _ => HashMap::new(),
        }
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, StoredCalendarProviderCredential>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("calendar_credential_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("calendar_credential_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, StoredCalendarProviderCredential>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("calendar_credential_set", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsCalendarProviderCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CalendarProviderCredentialStore for SpiffsCalendarProviderCredentialStore {
    fn get(&self, provider: &str) -> Result<Option<CalendarProviderCredential>> {
        self.with_map_mut(|map| Ok(map.get(provider).map(|entry| entry.0.clone())))
    }

    fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
        self.with_map_mut(|map| {
            if !map.contains_key(&credential.provider)
                && map.len() >= MAX_CALENDAR_PROVIDER_CREDENTIALS
            {
                let oldest = map
                    .iter()
                    .min_by_key(|(_, entry)| entry.0.updated_at)
                    .map(|(provider, _)| provider.clone());
                if let Some(provider) = oldest {
                    map.remove(&provider);
                }
            }
            map.insert(
                credential.provider.clone(),
                StoredCalendarProviderCredential(credential.clone()),
            );
            Self::persist(map)
        })
    }

    fn clear(&self, provider: &str) -> Result<()> {
        self.with_map_mut(|map| {
            if map.remove(provider).is_some() {
                Self::persist(map)?;
            }
            Ok(())
        })
    }

    fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>> {
        self.with_map_mut(|map| {
            let mut statuses = map
                .values()
                .map(|entry| entry.0.status())
                .collect::<Vec<_>>();
            statuses.sort_by(|left, right| left.provider.cmp(&right.provider));
            Ok(statuses)
        })
    }
}
