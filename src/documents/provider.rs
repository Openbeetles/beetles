use crate::documents::{
    DocumentsEntry, DocumentsQuery, DocumentsReadResult, DocumentsSearchHit,
    DocumentsSearchQuery, DocumentsProviderCredential,
};
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentsOperation {
    List,
    Read,
    Search,
}

pub trait DocumentsProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn supports(&self, op: DocumentsOperation) -> bool;
    fn list_entries(
        &self,
        credential: &DocumentsProviderCredential,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>>;
    fn read_document(
        &self,
        credential: &DocumentsProviderCredential,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult>;
    fn search_documents(
        &self,
        credential: &DocumentsProviderCredential,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>>;
}

#[derive(Default)]
pub struct DocumentsProviderRegistry {
    providers: HashMap<&'static str, Arc<dyn DocumentsProvider>>,
}

impl DocumentsProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn DocumentsProvider>) {
        self.providers.insert(provider.provider_name(), provider);
    }

    pub fn get(&self, provider: &str) -> Option<Arc<dyn DocumentsProvider>> {
        self.providers.get(provider).cloned()
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names = self.providers.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }
}
