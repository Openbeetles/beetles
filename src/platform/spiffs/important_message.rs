//! SPIFFS 实现的重要消息偏移存储。单文件 memory/important_message.json，单 chat 单 offset。
//! ImportantMessageStore implementation; single file, one chat's offset at a time.

use crate::error::{Error, Result};
use crate::memory::{ImportantMessageStore, REL_PATH_IMPORTANT_MESSAGE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_IMPORTANT_MESSAGE_CHATS: usize = 32;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_IMPORTANT_MESSAGE)
}

/// 兼容旧格式：单 chat 单 offset。
#[derive(Serialize, Deserialize)]
struct LegacyImportantMessageState {
    chat_id: String,
    offset_from_end: u32,
}

/// 单文件缓存；按 chat_id 保存待保留的重要消息偏移。
pub struct SpiffsImportantMessageStore {
    cache: Mutex<Option<HashMap<String, u32>>>,
}

impl SpiffsImportantMessageStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, u32> {
        let path = full_path();
        let buf = match read_file(&path) {
            Ok(buf) => buf,
            Err(_) => return HashMap::new(),
        };
        if buf.len() <= 2 {
            return HashMap::new();
        }
        if let Ok(map) = serde_json::from_slice::<HashMap<String, u32>>(&buf) {
            return map;
        }
        if let Ok(legacy) = serde_json::from_slice::<LegacyImportantMessageState>(&buf) {
            let mut map = HashMap::with_capacity(1);
            map.insert(legacy.chat_id, legacy.offset_from_end);
            return map;
        }
        HashMap::new()
    }

    fn with_map_mut<R>(&self, f: impl FnOnce(&mut HashMap<String, u32>) -> Result<R>) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("important_message_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("important_message_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, u32>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("important_message_set", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsImportantMessageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportantMessageStore for SpiffsImportantMessageStore {
    fn set_important_offset_from_end(&self, chat_id: &str, offset_from_end: u32) -> Result<()> {
        self.with_map_mut(|map| {
            if !map.contains_key(chat_id) && map.len() >= MAX_IMPORTANT_MESSAGE_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), offset_from_end);
            Self::persist(map)
        })
    }

    fn get_important_offset(&self, chat_id: &str) -> Result<Option<u32>> {
        self.with_map_mut(|map| Ok(map.get(chat_id).copied()))
    }

    fn clear_important(&self, chat_id: &str) -> Result<()> {
        self.with_map_mut(|map| {
            if map.remove(chat_id).is_some() {
                Self::persist(map)?;
            }
            Ok(())
        })
    }
}
