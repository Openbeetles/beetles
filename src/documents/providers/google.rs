#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::documents::credentials::documents_credential_from_office;
use crate::documents::{
    build_search_snippet, contains_query_text, decode_readable_document,
    decode_searchable_document_text, documents_bounded_read_bytes, documents_search_match_kind,
    merge_document_warning, DocumentsEntry, DocumentsOperation, DocumentsProvider,
    DocumentsProviderCredential, DocumentsQuery, DocumentsReadResult, DocumentsSearchHit,
    DocumentsSearchQuery, PARTIAL_DOCUMENT_READ_WARNING,
};
use crate::error::{Error, Result};
use crate::office::{
    build_google_api_url, read_bounded_http_bytes, request_google_api_json, OfficeAccount,
    OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use serde::Deserialize;
use std::collections::VecDeque;

const MAX_SEARCH_SCAN_ENTRIES: usize = 64;
const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;
const GOOGLE_FOLDER_MIME: &str = "application/vnd.google-apps.folder";

pub struct GoogleDocumentsProvider;

impl DocumentsProvider for GoogleDocumentsProvider {
    fn provider_name(&self) -> &'static str {
        "google_documents"
    }

    fn display_name(&self) -> &'static str {
        "Google Documents"
    }

    fn supports(&self, op: DocumentsOperation) -> bool {
        matches!(
            op,
            DocumentsOperation::List | DocumentsOperation::Read | DocumentsOperation::Search
        )
    }

    fn list_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        let client = GoogleDocumentsClient::new(credential)?;
        let items = client.list_entries(http, &query.path)?;
        Ok(items.into_iter().take(query.limit).collect())
    }

    fn read_document(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        let client = GoogleDocumentsClient::new(credential)?;
        let item = client.resolve_item(http, path)?;
        if item.is_dir() {
            return Err(Error::config(
                "google_documents_read",
                format!("'{}' is a folder, not a document", item.relative_path),
            ));
        }
        let transport_limit = documents_bounded_read_bytes(max_chars);
        let read = client.read_item_bytes(http, &item, transport_limit)?;
        let decoded =
            decode_readable_document(&item.relative_path, &read.bytes, max_chars.max(1), "google_documents_read")
                .map_err(|error| {
                    if read.truncated {
                        Error::config(
                            "google_documents_read",
                            format!(
                                "document exceeded bounded read budget of {} bytes; increase max_chars to read more",
                                transport_limit
                            ),
                        )
                    } else {
                        error
                    }
                })?;
        Ok(DocumentsReadResult {
            entry: item.to_documents_entry(),
            content: decoded.content,
            truncated: decoded.truncated || read.truncated,
            raw_bytes: decoded.raw_bytes,
            warning: merge_document_warning(
                decoded.warning,
                read.truncated.then_some(PARTIAL_DOCUMENT_READ_WARNING),
            ),
        })
    }

    fn search_documents(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        let client = GoogleDocumentsClient::new(credential)?;
        let search_hits = client.search_entries(http, &query.query, query.limit.max(20))?;
        let mut queue = search_hits
            .into_iter()
            .filter(|item| {
                path_is_within_scope(&item.relative_path, &normalize_relative_path(&query.path))
            })
            .collect::<VecDeque<_>>();
        let mut hits = Vec::new();
        let mut scanned = 0usize;
        let max_read_bytes = if query.max_read_bytes == 0 {
            MAX_SEARCH_READ_BYTES
        } else {
            query.max_read_bytes.min(MAX_SEARCH_READ_BYTES)
        };
        while let Some(item) = queue.pop_front() {
            if scanned >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                break;
            }
            scanned += 1;
            let entry = item.to_documents_entry();
            let path_hit = contains_query_text(&entry.path, &query.query, query.case_sensitive)
                || contains_query_text(&entry.name, &query.query, query.case_sensitive);
            let mut snippet = None;
            let mut warning = None;
            let mut content_hit = false;
            if !item.is_dir()
                && item
                    .size_bytes
                    .map(|size| size as usize <= max_read_bytes)
                    .unwrap_or(true)
            {
                let raw = client.read_item_bytes(http, &item, max_read_bytes)?;
                if let Some(text) = decode_searchable_document_text(&entry.path, &raw.bytes) {
                    if contains_query_text(&text, &query.query, query.case_sensitive) {
                        content_hit = true;
                        snippet = Some(build_search_snippet(
                            &text,
                            &query.query,
                            query.case_sensitive,
                        ));
                    }
                }
                if raw.truncated {
                    warning = Some(PARTIAL_DOCUMENT_READ_WARNING.to_string());
                }
            } else if !item.is_dir() {
                warning = Some("content not searched because the file is too large".to_string());
            }
            if let Some(match_kind) = documents_search_match_kind(path_hit, content_hit) {
                hits.push(DocumentsSearchHit {
                    entry,
                    match_kind: match_kind.to_string(),
                    snippet,
                    warning,
                });
            }
        }
        if hits.len() > query.limit {
            hits.truncate(query.limit);
        }
        Ok(hits)
    }
}

pub struct GoogleDocumentsOfficeProbeAdapter;

impl OfficeProbeAdapter for GoogleDocumentsOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "google_documents"
    }

    fn probe(
        &self,
        http: &mut dyn OfficeHttpClient,
        account: &OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = match documents_credential_from_office(account.clone(), credential.clone()) {
            Ok(adapted) => adapted,
            Err(_) => {
                return Ok(OfficeProbeResult {
                    account_key: account.account_key.clone(),
                    provider_kind: account.provider_kind.clone(),
                    configured: false,
                    disposition: OfficeProbeDisposition::MissingCredential,
                    reason: "documents_transport_config_missing".to_string(),
                })
            }
        };
        if validate_google_documents_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "documents_transport_config_missing".to_string(),
            });
        }
        let client = GoogleDocumentsClient::new(&adapted)?;
        client.list_entries(http, "")?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "google_drive_list_ok".to_string(),
        })
    }
}

struct GoogleDocumentsClient<'a> {
    credential: &'a DocumentsProviderCredential,
}

#[derive(Clone, Debug)]
struct GoogleDriveItem {
    id: String,
    name: String,
    relative_path: String,
    mime_type: String,
    size_bytes: Option<u64>,
}

impl GoogleDocumentsClient<'_> {
    fn new(credential: &DocumentsProviderCredential) -> Result<GoogleDocumentsClient<'_>> {
        validate_google_documents_credential(credential)?;
        Ok(GoogleDocumentsClient { credential })
    }

    fn list_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        relative_path: &str,
    ) -> Result<Vec<DocumentsEntry>> {
        let parent_id = self.resolve_parent_folder_id(http, relative_path)?;
        let q = format!("'{}' in parents and trashed=false", parent_id);
        let auth = self.auth_header();
        let payload: GoogleDriveFiles = request_google_api_json(
            http,
            "google_documents_list",
            "GET",
            &self.endpoint(
                "/files",
                &[
                    ("q", q),
                    ("pageSize", "200".to_string()),
                    ("fields", "files(id,name,mimeType,size,parents)".to_string()),
                    ("includeItemsFromAllDrives", "true".to_string()),
                    ("supportsAllDrives", "true".to_string()),
                    ("driveId", self.credential.space_id.trim().to_string()),
                    (
                        "corpora",
                        if self.credential.space_id.trim().is_empty() {
                            "".to_string()
                        } else {
                            "drive".to_string()
                        },
                    ),
                ],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        let current_parent = normalize_relative_path(relative_path);
        Ok(payload
            .files
            .into_iter()
            .map(|item| {
                GoogleDriveItem::from_graph_item(item, &current_parent).to_documents_entry()
            })
            .collect())
    }

    fn resolve_item(
        &self,
        http: &mut dyn OfficeHttpClient,
        relative_path: &str,
    ) -> Result<GoogleDriveItem> {
        let request_path = scoped_relative_path(&self.credential.root_path, relative_path);
        if request_path.is_empty() {
            return Ok(GoogleDriveItem {
                id: "root".to_string(),
                name: root_entry_name(&self.credential.root_path),
                relative_path: String::new(),
                mime_type: GOOGLE_FOLDER_MIME.to_string(),
                size_bytes: None,
            });
        }
        let mut parent_id = self.root_parent_id();
        let mut current_parent = String::new();
        let segments = request_path
            .split('/')
            .filter(|segment| !segment.trim().is_empty())
            .collect::<Vec<_>>();
        for (index, segment) in segments.iter().enumerate() {
            let q = format!(
                "'{}' in parents and trashed=false and name='{}'",
                parent_id,
                escape_drive_query(segment)
            );
            let auth = self.auth_header();
            let payload: GoogleDriveFiles = request_google_api_json(
                http,
                "google_documents_resolve",
                "GET",
                &self.endpoint(
                    "/files",
                    &[
                        ("q", q),
                        ("pageSize", "2".to_string()),
                        ("fields", "files(id,name,mimeType,size,parents)".to_string()),
                        ("includeItemsFromAllDrives", "true".to_string()),
                        ("supportsAllDrives", "true".to_string()),
                        ("driveId", self.credential.space_id.trim().to_string()),
                        (
                            "corpora",
                            if self.credential.space_id.trim().is_empty() {
                                "".to_string()
                            } else {
                                "drive".to_string()
                            },
                        ),
                    ],
                ),
                &[("Authorization", auth.as_str())],
                None,
            )?;
            let item = payload.files.into_iter().next().ok_or_else(|| {
                Error::config(
                    "google_documents_resolve",
                    format!("path '{}' not found", relative_path),
                )
            })?;
            let relative_parent = current_parent.clone();
            let item = GoogleDriveItem::from_graph_item(item, &relative_parent);
            if index + 1 == segments.len() {
                return Ok(item);
            }
            if item.is_dir() {
                parent_id = item.id.clone();
                current_parent = item.relative_path.clone();
            } else {
                return Err(Error::config(
                    "google_documents_resolve",
                    format!("'{}' is not a folder", item.relative_path),
                ));
            }
        }
        Err(Error::config(
            "google_documents_resolve",
            "path must not be empty",
        ))
    }

    fn read_item_bytes(
        &self,
        http: &mut dyn OfficeHttpClient,
        item: &GoogleDriveItem,
        max_bytes: usize,
    ) -> Result<crate::office::OfficeBoundedBytes> {
        let endpoint = if item.mime_type.starts_with("application/vnd.google-apps") {
            self.endpoint(
                &format!("/files/{}/export", urlencoding::encode(&item.id)),
                &[("mimeType", "text/plain".to_string())],
            )
        } else {
            self.endpoint(
                &format!("/files/{}", urlencoding::encode(&item.id)),
                &[("alt", "media".to_string())],
            )
        };
        let auth = self.auth_header();
        read_bounded_http_bytes(
            http,
            "google_documents_read",
            &endpoint,
            &[("Authorization", auth.as_str())],
            max_bytes,
        )
    }

    fn search_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &str,
        limit: usize,
    ) -> Result<Vec<GoogleDriveItem>> {
        let escaped = escape_drive_query(query.trim());
        let q = if escaped.is_empty() {
            "trashed=false".to_string()
        } else {
            format!(
                "trashed=false and (name contains '{}' or fullText contains '{}')",
                escaped, escaped
            )
        };
        let auth = self.auth_header();
        let payload: GoogleDriveFiles = request_google_api_json(
            http,
            "google_documents_search",
            "GET",
            &self.endpoint(
                "/files",
                &[
                    ("q", q),
                    ("pageSize", limit.clamp(1, 100).to_string()),
                    ("fields", "files(id,name,mimeType,size,parents)".to_string()),
                    ("includeItemsFromAllDrives", "true".to_string()),
                    ("supportsAllDrives", "true".to_string()),
                    ("driveId", self.credential.space_id.trim().to_string()),
                    (
                        "corpora",
                        if self.credential.space_id.trim().is_empty() {
                            "".to_string()
                        } else {
                            "drive".to_string()
                        },
                    ),
                ],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        Ok(payload
            .files
            .into_iter()
            .map(|item| {
                let parent_path = item.parents.first().cloned().unwrap_or_default();
                GoogleDriveItem::from_graph_item(item, &parent_path)
            })
            .collect())
    }

    fn resolve_parent_folder_id(
        &self,
        http: &mut dyn OfficeHttpClient,
        relative_path: &str,
    ) -> Result<String> {
        let request_path = scoped_relative_path(&self.credential.root_path, relative_path);
        if request_path.is_empty() {
            return Ok(self.root_parent_id());
        }
        self.resolve_item(http, relative_path).map(|item| item.id)
    }

    fn root_parent_id(&self) -> String {
        if self.credential.space_id.trim().is_empty() {
            "root".to_string()
        } else {
            self.credential.space_id.trim().to_string()
        }
    }

    fn endpoint(&self, path: &str, query: &[(&str, String)]) -> String {
        build_google_api_url(&self.credential.base_url, path, query)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.credential.secret)
    }
}

#[derive(Debug, Default, Deserialize)]
struct GoogleDriveFiles {
    #[serde(default)]
    files: Vec<GoogleDriveFile>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleDriveFile {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "mimeType")]
    mime_type: String,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    parents: Vec<String>,
}

impl GoogleDriveItem {
    fn from_graph_item(item: GoogleDriveFile, parent_path: &str) -> Self {
        let parent_path = normalize_relative_path(parent_path);
        let relative_path = if parent_path.is_empty() {
            item.name.clone()
        } else {
            format!("{}/{}", parent_path, item.name)
        };
        Self {
            id: item.id,
            name: item.name,
            relative_path,
            mime_type: item.mime_type,
            size_bytes: item.size.and_then(|value| value.parse::<u64>().ok()),
        }
    }

    fn is_dir(&self) -> bool {
        self.mime_type == GOOGLE_FOLDER_MIME
    }

    fn to_documents_entry(&self) -> DocumentsEntry {
        DocumentsEntry {
            path: self.relative_path.clone(),
            name: self.name.clone(),
            kind: if self.is_dir() {
                "folder".to_string()
            } else {
                self.mime_type.clone()
            },
            is_dir: self.is_dir(),
            content_type: (!self.is_dir()).then(|| self.mime_type.clone()),
            size_bytes: self.size_bytes,
        }
    }
}

fn validate_google_documents_credential(credential: &DocumentsProviderCredential) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "google_documents_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "google_documents_provider",
            "documents_base_url must not be empty",
        ));
    }
    if credential.root_path.trim().is_empty() {
        return Err(Error::config(
            "google_documents_provider",
            "documents_root_path must not be empty",
        ));
    }
    Ok(())
}

fn scoped_relative_path(root_path: &str, relative_path: &str) -> String {
    let root = normalize_relative_path(root_path);
    let child = normalize_relative_path(relative_path);
    match (root.is_empty(), child.is_empty()) {
        (true, true) => String::new(),
        (true, false) => child,
        (false, true) => root,
        (false, false) => format!("{root}/{child}"),
    }
}

fn normalize_relative_path(value: &str) -> String {
    value
        .trim()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.trim().is_empty())
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("/")
}

fn path_is_within_scope(path: &str, scope: &str) -> bool {
    scope.is_empty() || path == scope || path.starts_with(&format!("{scope}/"))
}

fn root_entry_name(root_path: &str) -> String {
    normalize_relative_path(root_path)
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or("root")
        .to_string()
}

fn escape_drive_query(query: &str) -> String {
    query.trim().replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccountIdentityClass, OfficeCapability, OfficeCredential};

    #[test]
    fn google_documents_provider_reports_drive_capabilities() {
        let provider = GoogleDocumentsProvider;
        assert_eq!(provider.provider_name(), "google_documents");
        assert!(provider.supports(DocumentsOperation::Read));
        assert!(provider.supports(DocumentsOperation::Search));
    }

    #[test]
    fn google_documents_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = GoogleDocumentsOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "google-docs".to_string(),
                    provider_kind: "google_documents".to_string(),
                    external_account_id: "alice@gmail.com".to_string(),
                    account_label: "Google Docs".to_string(),
                    identity_class: OfficeAccountIdentityClass::Personal,
                    enabled_capabilities: vec![OfficeCapability::Documents],
                },
                &OfficeCredential {
                    account_key: "google-docs".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(result.provider_kind, "google_documents");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
    }
}
