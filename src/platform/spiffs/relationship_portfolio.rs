//! SPIFFS implementation of Relationship Portfolio store. Single-file JSON map.

use crate::error::Result;
use crate::memory::{
    REL_PATH_RELATIONSHIP_PORTFOLIOS, RelationshipPortfolio, RelationshipPortfolioStore,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::ChatScopedCachedJsonMapStore;
use super::state_path_join;

const MAX_RELATIONSHIP_PORTFOLIO_SCOPES: usize = 4;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredRelationshipPortfolio(RelationshipPortfolio);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_RELATIONSHIP_PORTFOLIOS)
}

pub struct SpiffsRelationshipPortfolioStore {
    store: ChatScopedCachedJsonMapStore<StoredRelationshipPortfolio>,
}

impl SpiffsRelationshipPortfolioStore {
    pub fn new() -> Self {
        Self {
            store: ChatScopedCachedJsonMapStore::new(
                full_path,
                "relationship_portfolio_cache_lock",
                "relationship_portfolio_cache",
                "relationship_portfolio_persist",
                MAX_RELATIONSHIP_PORTFOLIO_SCOPES,
            ),
        }
    }
}

impl Default for SpiffsRelationshipPortfolioStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RelationshipPortfolioStore for SpiffsRelationshipPortfolioStore {
    fn get(&self, scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
        self.store
            .get_cloned(scope_id)
            .map(|value| value.map(|portfolio| portfolio.0))
    }

    fn set(&self, scope_id: &str, portfolio: &RelationshipPortfolio) -> Result<()> {
        self.store
            .set_owned(scope_id, StoredRelationshipPortfolio(portfolio.clone()))
    }

    fn clear(&self, scope_id: &str) -> Result<()> {
        self.store.clear(scope_id)
    }
}
