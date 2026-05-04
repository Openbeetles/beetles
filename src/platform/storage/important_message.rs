//! storage 实现的重要消息偏移存储。单文件 memory/important_message.json，单 chat 单 offset。
//! ImportantMessageStore implementation; single file, one chat's offset at a time.

use crate::error::Result;
use crate::memory::{ImportantMessageStore, REL_PATH_IMPORTANT_MESSAGE};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{CachedJsonFileStore, StoreOp};
use super::{read_file, state_path_join};

const MAX_IMPORTANT_MESSAGE_CHATS: usize = 32;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_IMPORTANT_MESSAGE)
}

/// 单文件缓存；按 chat_id 保存待保留的重要消息偏移。
pub struct StorageImportantMessageStore {
    store: CachedJsonFileStore<HashMap<String, u32>>,
}

impl StorageImportantMessageStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                Self::load_map_from_disk,
                "important_message_cache_lock",
                "important_message_cache",
                "important_message_persist",
            ),
        }
    }

    fn load_map_from_disk(path: &PathBuf, stage: &'static str) -> Result<HashMap<String, u32>> {
        let buf = match read_file(path) {
            Ok(buf) => buf,
            Err(crate::error::Error::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                return Ok(HashMap::new());
            }
            Err(error) => return Err(error.with_stage(stage)),
        };
        if buf.len() <= 2 {
            return Ok(HashMap::new());
        }
        if let Ok(map) = serde_json::from_slice::<HashMap<String, u32>>(&buf) {
            return Ok(map);
        }
        #[derive(Deserialize)]
        struct LegacyImportantMessage {
            #[serde(rename = "chat_id")]
            _chat_id: String,
            #[serde(rename = "offset_from_end")]
            _offset_from_end: u32,
        }
        if serde_json::from_slice::<LegacyImportantMessage>(&buf).is_ok() {
            return Ok(HashMap::new());
        }
        Err(crate::error::Error::config(
            stage,
            "invalid important message cache json",
        ))
    }
}

impl Default for StorageImportantMessageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportantMessageStore for StorageImportantMessageStore {
    fn set_important_offset_from_end(&self, chat_id: &str, offset_from_end: u32) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.get(chat_id).copied() == Some(offset_from_end) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_IMPORTANT_MESSAGE_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), offset_from_end);
            Ok(StoreOp::dirty(()))
        })
    }

    fn get_important_offset(&self, chat_id: &str) -> Result<Option<u32>> {
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.get(chat_id).copied())))
    }

    fn clear_important(&self, chat_id: &str) -> Result<()> {
        self.store.with_cached_mut(|map| {
            if map.remove(chat_id).is_some() {
                return Ok(StoreOp::dirty(()));
            }
            Ok(StoreOp::clean(()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{full_path, StorageImportantMessageStore};
    use crate::memory::ImportantMessageStore;

    #[test]
    fn store_ignores_legacy_single_chat_payload() {
        let chat_id = format!(
            "legacy-important-message-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let store = StorageImportantMessageStore::new();
        store.clear_important(&chat_id).unwrap();

        let payload = format!(r#"{{"chat_id":"{chat_id}","offset_from_end":7}}"#).into_bytes();
        super::super::write_file(full_path(), &payload).unwrap();

        let store = StorageImportantMessageStore::new();
        let loaded = store.get_important_offset(&chat_id).unwrap();
        assert_eq!(loaded, None);

        let _ = super::super::remove_file(full_path());
        store.clear_important(&chat_id).unwrap();
    }
}
