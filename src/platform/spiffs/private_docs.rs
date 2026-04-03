//! SPIFFS 实现的私有文档工作区存储。单文件 memory/private_doc_workspaces.json。

use crate::error::Result;
use crate::memory::{PrivateDocStore, PrivateDocWorkspace, REL_PATH_PRIVATE_DOC_WORKSPACES};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_PRIVATE_DOC_CHATS: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredPrivateDocWorkspace(PrivateDocWorkspace);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_PRIVATE_DOC_WORKSPACES)
}

pub struct SpiffsPrivateDocStore {
    store: ChatScopedCachedJsonMapStore<StoredPrivateDocWorkspace>,
}

impl SpiffsPrivateDocStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "private_doc_cache_lock",
                "private_doc_cache",
                "private_doc_persist",
                MAX_PRIVATE_DOC_CHATS,
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
        self.store
            .get_cloned(chat_id)
            .map(|value| value.map(|workspace| workspace.0))
    }

    fn set(&self, chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
        self.store
            .set_owned(chat_id, StoredPrivateDocWorkspace(workspace.clone()))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.store.clear(chat_id)
    }
}
