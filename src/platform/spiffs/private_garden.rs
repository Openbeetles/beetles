//! SPIFFS 实现的私有花园。索引单文件，正文分 chat 目录保存。

use crate::error::{Error, Result};
use crate::memory::{
    build_private_garden_preview, normalize_private_garden_doc_path, PrivateGardenDoc,
    PrivateGardenDocRecord, PrivateGardenStore, REL_PATH_PRIVATE_GARDEN_DIR,
    REL_PATH_PRIVATE_GARDEN_INDEX,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::{read_file, remove_file, state_path_join, write_file};

const MAX_PRIVATE_GARDEN_CHATS: usize = 32;
const MAX_PRIVATE_GARDEN_DOCS_PER_CHAT: usize = 16;
const MAX_PRIVATE_GARDEN_DOC_BYTES: usize = 8 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
struct StoredGardenIndex {
    chats: HashMap<String, StoredGardenChat>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct StoredGardenChat {
    docs: Vec<PrivateGardenDocRecord>,
}

fn full_index_path() -> PathBuf {
    state_path_join(REL_PATH_PRIVATE_GARDEN_INDEX)
}

fn fnv1a64_hash(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn chat_dir_name(chat_id: &str) -> String {
    if !chat_id.is_empty()
        && chat_id.len() <= 20
        && chat_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return chat_id.to_string();
    }
    format!("{:016x}", fnv1a64_hash(chat_id))
}

fn chat_doc_rel_path(chat_id: &str, doc_path: &str) -> String {
    format!(
        "{}/{}/{}",
        REL_PATH_PRIVATE_GARDEN_DIR,
        chat_dir_name(chat_id),
        doc_path
    )
}

pub struct SpiffsPrivateGardenStore {
    index: CachedJsonFileStore<StoredGardenIndex>,
}

impl SpiffsPrivateGardenStore {
    pub fn new() -> Self {
        Self {
            index: CachedJsonFileStore::new(
                full_index_path,
                load_json_or_default,
                "private_garden_cache_lock",
                "private_garden_cache",
                "private_garden_persist",
            ),
        }
    }
}

impl Default for SpiffsPrivateGardenStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivateGardenStore for SpiffsPrivateGardenStore {
    fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
        self.index.with_cached_mut(|index| {
            let docs = index
                .chats
                .get(chat_id)
                .map(|chat| {
                    let mut docs = chat.docs.clone();
                    docs.sort_by(|a, b| {
                        b.updated_at
                            .cmp(&a.updated_at)
                            .then_with(|| a.path.cmp(&b.path))
                    });
                    docs.truncate(limit);
                    docs
                })
                .unwrap_or_default();
            Ok(StoreOp::clean(docs))
        })
    }

    fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
        let doc_path = normalize_private_garden_doc_path(doc_path)?;
        self.index.with_cached_mut(|index| {
            let Some(chat) = index.chats.get(chat_id) else {
                return Ok(StoreOp::clean(None));
            };
            let Some(record) = chat.docs.iter().find(|doc| doc.path == doc_path) else {
                return Ok(StoreOp::clean(None));
            };
            let rel_path = chat_doc_rel_path(chat_id, &doc_path);
            let buf = match read_file(state_path_join(&rel_path)) {
                Ok(buf) => buf,
                Err(Error::Other { .. }) | Err(Error::Io { .. }) => {
                    return Ok(StoreOp::clean(None));
                }
                Err(error) => return Err(error),
            };
            let content = String::from_utf8(buf).map_err(|_| {
                Error::config("private_garden_read", "stored document is not valid UTF-8")
            })?;
            Ok(StoreOp::clean(Some(PrivateGardenDoc {
                path: doc_path,
                content,
                updated_at: record.updated_at,
                revision: record.revision,
            })))
        })
    }

    fn write(
        &self,
        chat_id: &str,
        doc_path: &str,
        content: &str,
        now_secs: u64,
    ) -> Result<PrivateGardenDocRecord> {
        let doc_path = normalize_private_garden_doc_path(doc_path)?;
        if content.as_bytes().len() > MAX_PRIVATE_GARDEN_DOC_BYTES {
            return Err(Error::config(
                "private_garden_write",
                format!("content exceeds {} bytes", MAX_PRIVATE_GARDEN_DOC_BYTES),
            ));
        }
        self.index.with_cached_mut(|index| {
            if !index.chats.contains_key(chat_id) && index.chats.len() >= MAX_PRIVATE_GARDEN_CHATS {
                if let Some(evicted_chat_id) = index.chats.keys().next().cloned() {
                    if let Some(evicted_chat) = index.chats.remove(&evicted_chat_id) {
                        for doc in evicted_chat.docs {
                            let _ = remove_file(state_path_join(chat_doc_rel_path(
                                &evicted_chat_id,
                                &doc.path,
                            )));
                        }
                    }
                }
            }

            let chat = index.chats.entry(chat_id.to_string()).or_default();
            let existing_index = chat.docs.iter().position(|doc| doc.path == doc_path);
            let revision = existing_index
                .and_then(|idx| chat.docs.get(idx).map(|doc| doc.revision.saturating_add(1)))
                .unwrap_or(1);
            let next = PrivateGardenDocRecord {
                path: doc_path.clone(),
                updated_at: now_secs,
                revision,
                bytes: content.len(),
                preview: build_private_garden_preview(content),
            };

            if existing_index.is_none() && chat.docs.len() >= MAX_PRIVATE_GARDEN_DOCS_PER_CHAT {
                if let Some((evict_idx, evicted)) = chat
                    .docs
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        a.updated_at
                            .cmp(&b.updated_at)
                            .then_with(|| a.path.cmp(&b.path))
                    })
                    .map(|(idx, doc)| (idx, doc.clone()))
                {
                    chat.docs.remove(evict_idx);
                    let _ = remove_file(state_path_join(chat_doc_rel_path(chat_id, &evicted.path)));
                }
            }

            let rel_path = chat_doc_rel_path(chat_id, &doc_path);
            write_file(state_path_join(&rel_path), content.as_bytes())?;
            if let Some(idx) = existing_index {
                chat.docs[idx] = next.clone();
            } else {
                chat.docs.push(next.clone());
            }
            Ok(StoreOp::dirty(next))
        })
    }

    fn delete(&self, chat_id: &str, doc_path: &str) -> Result<bool> {
        let doc_path = normalize_private_garden_doc_path(doc_path)?;
        self.index.with_cached_mut(|index| {
            let Some(chat) = index.chats.get_mut(chat_id) else {
                return Ok(StoreOp::clean(false));
            };
            let Some(idx) = chat.docs.iter().position(|doc| doc.path == doc_path) else {
                return Ok(StoreOp::clean(false));
            };
            let removed = chat.docs.remove(idx);
            let remove_chat_entry = chat.docs.is_empty();
            let _ = remove_file(state_path_join(chat_doc_rel_path(chat_id, &removed.path)));
            if remove_chat_entry {
                index.chats.remove(chat_id);
            }
            Ok(StoreOp::dirty(true))
        })
    }
}
