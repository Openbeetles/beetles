use crate::contacts_directory::{
    normalize_contact_entry, ContactEntry, ContactsDirectorySegment, ContactsDirectoryStore,
    MAX_CONTACTS_DIRECTORY_ENTRIES, REL_PATH_CONTACTS_DIRECTORY,
};
use crate::error::{Error, Result};
use crate::StateFs;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub struct StateFsContactsDirectoryStore {
    state_fs: Arc<dyn StateFs + Send + Sync>,
    cache: Mutex<Option<BTreeMap<String, ContactEntry>>>,
}

impl StateFsContactsDirectoryStore {
    pub fn new(state_fs: Arc<dyn StateFs + Send + Sync>) -> Self {
        Self {
            state_fs,
            cache: Mutex::new(None),
        }
    }

    fn load_from_disk(&self) -> Result<BTreeMap<String, ContactEntry>> {
        let bytes = match self.state_fs.read(REL_PATH_CONTACTS_DIRECTORY) {
            Ok(Some(bytes)) if !bytes.is_empty() => bytes,
            Ok(Some(_)) | Ok(None) => return Ok(BTreeMap::new()),
            Err(error) => return Err(error),
        };
        let segment: ContactsDirectorySegment = serde_json::from_slice(&bytes)
            .map_err(|error| Error::config("contacts_directory_load", error.to_string()))?;
        segment_to_map(segment).map_err(|error| Error::config("contacts_directory_load", error))
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut BTreeMap<String, ContactEntry>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|error| Error::config("contacts_directory_lock", error.to_string()))?;
        if guard.is_none() {
            *guard = Some(self.load_from_disk()?);
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("contacts_directory_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(&self, map: &BTreeMap<String, ContactEntry>) -> Result<()> {
        let segment = ContactsDirectorySegment {
            items: map.values().cloned().collect(),
        };
        let json = serde_json::to_vec(&segment)
            .map_err(|error| Error::config("contacts_directory_persist", error.to_string()))?;
        self.state_fs.write(REL_PATH_CONTACTS_DIRECTORY, &json)
    }
}

impl ContactsDirectoryStore for StateFsContactsDirectoryStore {
    fn list(&self) -> Result<Vec<ContactEntry>> {
        self.with_map_mut(|map| Ok(map.values().cloned().collect()))
    }

    fn get(&self, id: &str) -> Result<Option<ContactEntry>> {
        self.with_map_mut(|map| Ok(map.get(id).cloned()))
    }

    fn upsert(&self, contact: &ContactEntry) -> Result<()> {
        self.with_map_mut(|map| {
            if !map.contains_key(&contact.id) && map.len() >= MAX_CONTACTS_DIRECTORY_ENTRIES {
                return Err(Error::config(
                    "contacts_directory_upsert",
                    format!(
                        "contacts directory limit {} exceeded",
                        MAX_CONTACTS_DIRECTORY_ENTRIES
                    ),
                ));
            }
            let normalized = normalize_contact_entry(contact.clone())?;
            if normalized.id.is_empty() {
                return Err(Error::config(
                    "contacts_directory_upsert",
                    "contact id must not be empty",
                ));
            }
            map.insert(normalized.id.clone(), normalized);
            self.persist(map)
        })
    }

    fn delete(&self, id: &str) -> Result<bool> {
        self.with_map_mut(|map| {
            let removed = map.remove(id).is_some();
            if removed {
                self.persist(map)?;
            }
            Ok(removed)
        })
    }
}

fn segment_to_map(
    segment: ContactsDirectorySegment,
) -> std::result::Result<BTreeMap<String, ContactEntry>, String> {
    if segment.items.len() > MAX_CONTACTS_DIRECTORY_ENTRIES {
        return Err(format!(
            "contacts directory limit {} exceeded",
            MAX_CONTACTS_DIRECTORY_ENTRIES
        ));
    }
    let mut map = BTreeMap::new();
    for item in segment.items {
        let normalized = normalize_contact_entry(item).map_err(|error| error.to_string())?;
        if normalized.id.is_empty() {
            return Err("contact id must not be empty".to_string());
        }
        if map.contains_key(&normalized.id) {
            return Err(format!("duplicate contact id '{}'", normalized.id));
        }
        map.insert(normalized.id.clone(), normalized);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::StateFsContactsDirectoryStore;
    use crate::contacts_directory::{
        ContactEntry, ContactsDirectoryStore, REL_PATH_CONTACTS_DIRECTORY,
    };
    use crate::error::Result;
    use crate::StateFs;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.files.lock().unwrap().get(rel_path).cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files.lock().unwrap().remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn state_fs_store_roundtrips_contact_entries() {
        let fs = Arc::new(MemoryStateFs::default());
        let store = StateFsContactsDirectoryStore::new(fs.clone());
        store
            .upsert(&ContactEntry {
                id: "alice".to_string(),
                display_name: "Alice".to_string(),
                emails: vec!["alice@example.com".to_string()],
                ..ContactEntry::default()
            })
            .expect("save contact");

        let saved = store.get("alice").expect("read").expect("contact exists");
        assert_eq!(saved.display_name, "Alice");
        assert!(fs
            .read(REL_PATH_CONTACTS_DIRECTORY)
            .expect("state read")
            .is_some());
    }

    #[test]
    fn delete_returns_false_for_missing_contact() {
        let store = StateFsContactsDirectoryStore::new(Arc::new(MemoryStateFs::default()));
        assert!(!store.delete("missing").expect("delete missing"));
    }
}
