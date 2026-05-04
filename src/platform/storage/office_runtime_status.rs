//! storage / state-root backed office runtime status store.

use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountRuntimeStatus, OfficeAccountStatusSummary, OfficeRuntimeStatusStore,
    REL_PATH_OFFICE_RUNTIME_STATUS,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_json_file};

const MAX_OFFICE_RUNTIME_STATUSES: usize = 32;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_OFFICE_RUNTIME_STATUS)
}

pub struct StorageOfficeRuntimeStatusStore {
    cache: Mutex<Option<BTreeMap<String, OfficeAccountRuntimeStatus>>>,
}

impl StorageOfficeRuntimeStatusStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_from_disk() -> Result<BTreeMap<String, OfficeAccountRuntimeStatus>> {
        let bytes = match read_file(full_path()) {
            Ok(bytes) if !bytes.is_empty() => bytes,
            Ok(_) => return Ok(BTreeMap::new()),
            Err(error) if error.stage() == "storage_read" => return Ok(BTreeMap::new()),
            Err(error) => return Err(error),
        };
        let summary: OfficeAccountStatusSummary = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config("office_runtime_status_load", error.to_string()))?;
        let mut map = BTreeMap::new();
        for status in summary.items {
            let account_key = status.account_key.trim().to_string();
            if account_key.is_empty() {
                return Err(Error::config(
                    "office_runtime_status_load",
                    "account_key must not be empty",
                ));
            }
            if map.contains_key(&account_key) {
                return Err(Error::config(
                    "office_runtime_status_load",
                    format!("duplicate office runtime status '{}'", account_key),
                ));
            }
            let mut next = status;
            next.account_key = account_key.clone();
            map.insert(account_key, next);
        }
        Ok(map)
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, OfficeAccountRuntimeStatus>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("office_runtime_status_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_from_disk()?);
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("office_runtime_status_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &BTreeMap<String, OfficeAccountRuntimeStatus>) -> Result<()> {
        let summary = OfficeAccountStatusSummary {
            items: map.values().cloned().collect(),
        };
        let json = serde_json::to_vec(&summary)
            .map_err(|error| Error::config("office_runtime_status_persist", error.to_string()))?;
        write_json_file(full_path(), &json)
    }
}

impl Default for StorageOfficeRuntimeStatusStore {
    fn default() -> Self {
        Self::new()
    }
}

impl OfficeRuntimeStatusStore for StorageOfficeRuntimeStatusStore {
    fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
        self.with_map_mut(|map| Ok(map.get(account_key).cloned()))
    }

    fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        self.with_map_mut(|map| Ok(map.values().cloned().collect()))
    }

    fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
        self.with_map_mut(|map| {
            let account_key = status.account_key.trim().to_string();
            if account_key.is_empty() {
                return Err(Error::config(
                    "office_runtime_status_set",
                    "account_key must not be empty",
                ));
            }
            if !map.contains_key(&account_key) && map.len() >= MAX_OFFICE_RUNTIME_STATUSES {
                return Err(Error::config(
                    "office_runtime_status_set",
                    format!(
                        "office runtime status limit {} exceeded",
                        MAX_OFFICE_RUNTIME_STATUSES
                    ),
                ));
            }
            let mut next = status.clone();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_status_roundtrips() {
        let store = StorageOfficeRuntimeStatusStore::new();
        store.clear("calendar-work").ok();
        store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "calendar-work".to_string(),
                probe_ok: false,
                last_error: "token_expired".to_string(),
                last_probe_at_unix_secs: 12,
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 34,
            })
            .expect("set runtime status");
        let saved = store
            .get("calendar-work")
            .expect("get runtime status")
            .expect("runtime status exists");
        assert_eq!(saved.last_error, "token_expired");
        assert!(!saved.probe_ok);
        store.clear("calendar-work").expect("cleanup");
    }
}
