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
    decode_searchable_document_text, detect_document_kind, summarize_document_read_result,
    DecodedReadableDocument, EMPTY_DOCUMENT_WARNING,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use credentials::{
    DocumentsProviderCredential, DocumentsProviderCredentialStatus,
    DocumentsProviderCredentialStore, OfficeBackedDocumentsProviderCredentialStore,
    FEISHU_DOCUMENTS_DEFAULT_BASE_URL, OFFICE_METADATA_DOCUMENTS_APP_ID,
    OFFICE_METADATA_DOCUMENTS_BASE_URL, OFFICE_METADATA_DOCUMENTS_CORP_ID,
    OFFICE_METADATA_DOCUMENTS_DRIVE_ID, OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
    OFFICE_METADATA_DOCUMENTS_SPACE_ID, OFFICE_METADATA_DOCUMENTS_USERNAME,
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
pub struct DocumentsSummaryResult {
    pub entry: DocumentsEntry,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub focus: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_points: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_items: Vec<String>,
    pub handoff: DocumentsSummaryHandoff,
    #[serde(default)]
    pub truncated_source: bool,
    #[serde(default)]
    pub raw_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsSummaryHandoff {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_candidates: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mail_brief: String,
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

pub const DOCUMENTS_SEARCH_MATCH_PATH: &str = "path";
pub const DOCUMENTS_SEARCH_MATCH_CONTENT: &str = "content";
pub const DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT: &str = "path+content";

pub fn documents_search_match_kind(path_hit: bool, content_hit: bool) -> Option<&'static str> {
    match (path_hit, content_hit) {
        (true, true) => Some(DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT),
        (true, false) => Some(DOCUMENTS_SEARCH_MATCH_PATH),
        (false, true) => Some(DOCUMENTS_SEARCH_MATCH_CONTENT),
        (false, false) => None,
    }
}

pub fn documents_search_match_score(match_kind: &str) -> u8 {
    match match_kind {
        DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT => 3,
        DOCUMENTS_SEARCH_MATCH_PATH => 2,
        DOCUMENTS_SEARCH_MATCH_CONTENT => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        documents_search_match_kind, documents_search_match_score, DOCUMENTS_SEARCH_MATCH_CONTENT,
        DOCUMENTS_SEARCH_MATCH_PATH, DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT,
    };

    #[test]
    fn documents_search_match_kind_covers_all_hit_shapes() {
        assert_eq!(documents_search_match_kind(false, false), None);
        assert_eq!(
            documents_search_match_kind(true, false),
            Some(DOCUMENTS_SEARCH_MATCH_PATH)
        );
        assert_eq!(
            documents_search_match_kind(false, true),
            Some(DOCUMENTS_SEARCH_MATCH_CONTENT)
        );
        assert_eq!(
            documents_search_match_kind(true, true),
            Some(DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT)
        );
    }

    #[test]
    fn documents_search_match_score_is_consistent_with_match_specificity() {
        assert_eq!(
            documents_search_match_score(DOCUMENTS_SEARCH_MATCH_PATH_AND_CONTENT),
            3
        );
        assert_eq!(documents_search_match_score(DOCUMENTS_SEARCH_MATCH_PATH), 2);
        assert_eq!(
            documents_search_match_score(DOCUMENTS_SEARCH_MATCH_CONTENT),
            1
        );
        assert_eq!(documents_search_match_score("unknown"), 0);
    }
}
