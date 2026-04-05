//! SPIFFS implementation of Relationship Topology store. Single-file JSON map.

use crate::error::Result;
use crate::memory::{
    RelationshipTopology, RelationshipTopologyStore, REL_PATH_RELATIONSHIP_TOPOLOGIES,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_RELATIONSHIP_TOPOLOGY_SCOPES: usize = 4;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredRelationshipTopology(RelationshipTopology);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_RELATIONSHIP_TOPOLOGIES)
}

pub struct SpiffsRelationshipTopologyStore {
    store: ChatScopedCachedJsonMapStore<StoredRelationshipTopology>,
}

impl SpiffsRelationshipTopologyStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "relationship_topology_cache_lock",
                "relationship_topology_cache",
                "relationship_topology_persist",
                MAX_RELATIONSHIP_TOPOLOGY_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsRelationshipTopologyStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RelationshipTopologyStore for SpiffsRelationshipTopologyStore {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|topology| topology.0))
    }

    fn set(&self, scope_id: &str, topology: &RelationshipTopology) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredRelationshipTopology(topology.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
