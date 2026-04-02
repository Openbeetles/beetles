//! SPIFFS 实现的私有文档工作区存储。单文件 memory/private_doc_workspaces.json。

use crate::error::Result;
use crate::memory::{PrivateDocStore, PrivateDocWorkspace, REL_PATH_PRIVATE_DOC_WORKSPACES};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_PRIVATE_DOC_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredPrivateDocWorkspace(PrivateDocWorkspace);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_PRIVATE_DOC_WORKSPACES)
}

pub struct SpiffsPrivateDocStore {
    store: CachedJsonFileStore<HashMap<String, StoredPrivateDocWorkspace>>,
}

impl SpiffsPrivateDocStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "private_doc_cache_lock",
                "private_doc_cache",
                "private_doc_persist",
            ),
        }
    }
}

impl Default for SpiffsPrivateDocStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivateDocStore for SpiffsPrivateDocStore {
    fn get(&self, chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.get(chat_id).map(|workspace| workspace.0.clone()),
            ))
        })
    }

    fn set(&self, chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let next_workspace = StoredPrivateDocWorkspace(workspace.clone());
            if map.get(chat_id) == Some(&next_workspace) {
                return Ok(StoreOp::clean(()));
            }
            if !map.contains_key(chat_id) && map.len() >= MAX_PRIVATE_DOC_CHATS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(chat_id.to_string(), next_workspace);
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
