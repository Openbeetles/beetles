use crate::documents::{
    DocumentsEntry, DocumentsProviderCredential, DocumentsQuery, DocumentsReadResult,
    DocumentsSearchHit, DocumentsSearchQuery,
};
use crate::error::Result;
use crate::office::office_refactor_helpers::define_provider_registry;
use crate::office::OfficeHttpClient;

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
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>>;
    fn read_document(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult>;
    fn search_documents(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>>;
}

define_provider_registry!(
    DocumentsProviderRegistry,
    crate::documents::DocumentsProvider
);
