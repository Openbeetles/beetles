//! storage / state-root backed office credential store.

use crate::error::{Error, Result};
use crate::office::{
    OfficeCredential, OfficeCredentialStore, OfficeCredentialsSegment, REL_PATH_OFFICE_CREDENTIALS,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::{read_file, state_path_join, write_json_file};

const MAX_OFFICE_CREDENTIALS: usize = 16;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_OFFICE_CREDENTIALS)
}

pub struct StorageOfficeCredentialStore {
    cache: Mutex<Option<BTreeMap<String, OfficeCredential>>>,
    path: PathBuf,
}

impl StorageOfficeCredentialStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
            path: full_path(),
        }
    }

    #[cfg(test)]
    fn new_for_test_path(path: PathBuf) -> Self {
        Self {
            cache: Mutex::new(None),
            path,
        }
    }

    fn load_from_disk(path: &Path) -> Result<BTreeMap<String, OfficeCredential>> {
        let bytes = match read_file(path) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => return Ok(BTreeMap::new()),
            Err(error) if error.stage() == "storage_read" => return Ok(BTreeMap::new()),
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
            *guard = Some(Self::load_from_disk(&self.path)?);
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("office_credentials_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(path: &Path, map: &BTreeMap<String, OfficeCredential>) -> Result<()> {
        let segment = OfficeCredentialsSegment {
            items: map.values().cloned().collect(),
        };
        let json = serde_json::to_vec(&segment)
            .map_err(|error| Error::config("office_credentials_persist", error.to_string()))?;
        write_json_file(path, &json)
    }
}

impl Default for StorageOfficeCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OfficeCredentialStore for StorageOfficeCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
        self.with_map_mut(|map| Ok(map.get(account_key).cloned()))
    }

    fn list(&self) -> Result<Vec<OfficeCredential>> {
        self.with_map_mut(|map| Ok(map.values().cloned().collect()))
    }

    fn set(&self, credential: &OfficeCredential) -> Result<()> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("office_credentials_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_from_disk(&self.path)?);
        }
        let current = guard
            .as_ref()
            .ok_or_else(|| Error::config("office_credentials_cache", "cache not initialized"))?;
        let mut next_map = current.clone();
        {
            let account_key = credential.account_key.trim().to_string();
            if account_key.is_empty() {
                return Err(Error::config(
                    "office_credentials_set",
                    "account_key must not be empty",
                ));
            }
            if !next_map.contains_key(&account_key) && next_map.len() >= MAX_OFFICE_CREDENTIALS {
                return Err(Error::config(
                    "office_credentials_set",
                    format!(
                        "office credential limit {} exceeded",
                        MAX_OFFICE_CREDENTIALS
                    ),
                ));
            }
            let mut next = credential.clone();
            next.account_key = account_key.clone();
            next_map.insert(account_key, next);
        }
        Self::persist(&self.path, &next_map)?;
        *guard = Some(next_map);
        Ok(())
    }

    fn clear(&self, account_key: &str) -> Result<()> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("office_credentials_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_from_disk(&self.path)?);
        }
        let current = guard
            .as_ref()
            .ok_or_else(|| Error::config("office_credentials_cache", "cache not initialized"))?;
        if !current.contains_key(account_key) {
            return Ok(());
        }
        let mut next_map = current.clone();
        next_map.remove(account_key);
        Self::persist(&self.path, &next_map)?;
        *guard = Some(next_map);
        Ok(())
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
            return Err(format!(
                "duplicate office credential account_key '{}'",
                account_key
            ));
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_credential_path(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "beetle-office-credentials-{}-{}-{id}.json",
            std::process::id(),
            name
        ))
    }

    fn cleanup_path(path: &Path) {
        if path.is_dir() {
            std::fs::remove_dir_all(path).expect("remove office credential test dir");
        } else {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn set_rejects_empty_account_key() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_credential_path("empty-key");
        let store = StorageOfficeCredentialStore::new_for_test_path(path.clone());
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
        cleanup_path(&path);
    }

    #[test]
    fn set_roundtrips_metadata() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_credential_path("roundtrip");
        let store = StorageOfficeCredentialStore::new_for_test_path(path.clone());
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
        cleanup_path(&path);
    }

    #[test]
    fn set_persist_failure_does_not_mutate_cached_credentials() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_credential_path("persist-failure");
        cleanup_path(&path);
        std::fs::create_dir_all(&path).expect("create blocking office credential dir");
        let store = StorageOfficeCredentialStore::new_for_test_path(path.clone());

        let error = store
            .set(&OfficeCredential {
                account_key: "calendar-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect_err("persist failure should be returned");

        assert_eq!(error.stage(), "atomic_write");
        assert!(store.get("calendar-work").expect("cache read").is_none());
        std::fs::remove_dir_all(&path).expect("cleanup blocking office credential dir");
    }
}
