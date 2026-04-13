//! Shared documents domain: provider credentials, readable content, and service routing.

mod content;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod credentials;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod provider;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod providers;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod service;

use serde::{Deserialize, Serialize};

pub use content::{
    build_search_snippet, contains_query_text, decode_readable_document,
    decode_searchable_document_text, detect_document_kind, DecodedReadableDocument,
    EMPTY_DOCUMENT_WARNING,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use credentials::{
    DocumentsProviderCredential, DocumentsProviderCredentialStatus,
    DocumentsProviderCredentialStore, OfficeBackedDocumentsProviderCredentialStore,
    OFFICE_METADATA_DOCUMENTS_BASE_URL, OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
    OFFICE_METADATA_DOCUMENTS_USERNAME,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use provider::{DocumentsOperation, DocumentsProvider, DocumentsProviderRegistry};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use service::DocumentsService;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsQuery {
    #[serde(default)]
    pub path: String,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsSearchQuery {
    #[serde(default)]
    pub path: String,
    pub query: String,
    pub limit: usize,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub max_read_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsEntry {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsReadResult {
    pub entry: DocumentsEntry,
    pub content: String,
    pub truncated: bool,
    pub raw_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsSearchHit {
    pub entry: DocumentsEntry,
    pub match_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
