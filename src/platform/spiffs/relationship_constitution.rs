//! SPIFFS implementation of Relationship Constitution store. Single-file JSON map.

use crate::error::Result;
use crate::memory::{
    RelationshipConstitution, RelationshipConstitutionStore, REL_PATH_RELATIONSHIP_CONSTITUTIONS,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_RELATIONSHIP_CONSTITUTION_SCOPES: usize = 16;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredRelationshipConstitution(RelationshipConstitution);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_RELATIONSHIP_CONSTITUTIONS)
}

pub struct SpiffsRelationshipConstitutionStore {
    store: ChatScopedCachedJsonMapStore<StoredRelationshipConstitution>,
}

impl SpiffsRelationshipConstitutionStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "relationship_constitution_cache_lock",
                "relationship_constitution_cache",
                "relationship_constitution_persist",
                MAX_RELATIONSHIP_CONSTITUTION_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsRelationshipConstitutionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RelationshipConstitutionStore for SpiffsRelationshipConstitutionStore {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipConstitution>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|constitution| constitution.0))
    }

    fn set(&self, scope_id: &str, constitution: &RelationshipConstitution) -> Result<()> {
        self.store.set_owned(
            scope_id,
            StoredRelationshipConstitution(constitution.clone()),
        )
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
