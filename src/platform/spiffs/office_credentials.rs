//! SPIFFS / state-root backed office credential store.

use crate::error::{Error, Result};
use crate::office::{
    OfficeCredential, OfficeCredentialStore, OfficeCredentialsSegment, REL_PATH_OFFICE_CREDENTIALS,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_OFFICE_CREDENTIALS: usize = 16;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_OFFICE_CREDENTIALS)
}

pub struct SpiffsOfficeCredentialStore {
    cache: Mutex<Option<BTreeMap<String, OfficeCredential>>>,
}

impl SpiffsOfficeCredentialStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_from_disk() -> Result<BTreeMap<String, OfficeCredential>> {
        let bytes = match read_file(full_path()) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => return Ok(BTreeMap::new()),
            Err(error) if error.stage() == "spiffs_read" => return Ok(BTreeMap::new()),
            Err(error) => return Err(error),
        };
        let segment: OfficeCredentialsSegment = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config("office_credentials_load", error.to_string()))?;
        segment_to_map(segment).map_err(|error| Error::config("office_credentials_load", error))
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, OfficeCredential>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("office_credentials_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_from_disk()?);
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("office_credentials_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &BTreeMap<String, OfficeCredential>) -> Result<()> {
        let segment = OfficeCredentialsSegment {
            items: map.values().cloned().collect(),
        };
        let json = serde_json::to_vec(&segment)
            .map_err(|error| Error::config("office_credentials_persist", error.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsOfficeCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OfficeCredentialStore for SpiffsOfficeCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
        self.with_map_mut(|map| Ok(map.get(account_key).cloned()))
    }

    fn list(&self) -> Result<Vec<OfficeCredential>> {
        self.with_map_mut(|map| Ok(map.values().cloned().collect()))
    }

    fn set(&self, credential: &OfficeCredential) -> Result<()> {
        self.with_map_mut(|map| {
            let account_key = credential.account_key.trim().to_string();
            if account_key.is_empty() {
                return Err(Error::config(
                    "office_credentials_set",
                    "account_key must not be empty",
                ));
            }
            if !map.contains_key(&account_key) && map.len() >= MAX_OFFICE_CREDENTIALS {
                return Err(Error::config(
                    "office_credentials_set",
                    format!("office credential limit {} exceeded", MAX_OFFICE_CREDENTIALS),
                ));
            }
            let mut next = credential.clone();
            next.account_key = account_key.clone();
            map.insert(account_key, next);
            Self::persist(map)
        })
    }

    fn clear(&self, account_key: &str) -> Result<()> {
        self.with_map_mut(|map| {
            if map.remove(account_key).is_some() {
                Self::persist(map)?;
            }
            Ok(())
        })
    }
}

fn segment_to_map(
    segment: OfficeCredentialsSegment,
) -> std::result::Result<BTreeMap<String, OfficeCredential>, String> {
    let mut map = BTreeMap::new();
    for credential in segment.items {
        let account_key = credential.account_key.trim().to_string();
        if account_key.is_empty() {
            return Err("account_key must not be empty".to_string());
        }
        if map.contains_key(&account_key) {
            return Err(format!("duplicate office credential account_key '{}'", account_key));
        }
        let mut next = credential;
        next.account_key = account_key.clone();
        map.insert(account_key, next);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::OFFICE_METADATA_CALENDAR_ID;

    #[test]
    fn set_rejects_empty_account_key() {
        let store = SpiffsOfficeCredentialStore::new();
        let error = store
            .set(&OfficeCredential {
                account_key: String::new(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect_err("empty account_key must fail");
        assert!(error.to_string().contains("account_key must not be empty"));
    }

    #[test]
    fn set_roundtrips_metadata() {
        let store = SpiffsOfficeCredentialStore::new();
        store.clear("calendar-work").ok();
        store
            .set(&OfficeCredential {
                account_key: "calendar-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: "refresh".to_string(),
                token_endpoint: "https://example.com/token".to_string(),
                expires_at_unix_secs: 12,
                updated_at: 34,
                metadata: BTreeMap::from([(
                    OFFICE_METADATA_CALENDAR_ID.to_string(),
                    "primary".to_string(),
                )]),
            })
            .expect("set credential");
        let saved = store
            .get("calendar-work")
            .expect("read credential")
            .expect("credential exists");
        assert_eq!(
            saved.metadata_value(OFFICE_METADATA_CALENDAR_ID),
            Some("primary")
        );
        store.clear("calendar-work").expect("cleanup");
    }
}
