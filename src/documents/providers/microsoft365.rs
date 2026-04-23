#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::documents::credentials::documents_credential_from_office;
use crate::documents::{
    decode_readable_document, decode_searchable_document_text, documents_bounded_read_bytes,
    merge_document_warning, search_read_budget, search_scoped_candidates, DocumentsEntry,
    DocumentsOperation, DocumentsProvider, DocumentsProviderCredential, DocumentsQuery,
    DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery, SearchLimits, SearchReadOutcome,
    MAX_SEARCH_SCAN_ENTRIES, PARTIAL_DOCUMENT_READ_WARNING,
};
use crate::error::{Error, Result};
use crate::office::{
    build_microsoft_graph_url, read_bounded_http_bytes, request_microsoft_graph_json,
    OfficeAccount, OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use serde::Deserialize;
pub struct Microsoft365DocumentsProvider;

impl DocumentsProvider for Microsoft365DocumentsProvider {
    fn provider_name(&self) -> &'static str {
        "microsoft365_documents"
    }

    fn display_name(&self) -> &'static str {
        "Microsoft 365 Documents"
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
        let client = Microsoft365DocumentsClient::new(credential)?;
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
        let client = Microsoft365DocumentsClient::new(credential)?;
        let item = client.resolve_item(http, path)?;
        if item.is_dir() {
            return Err(Error::config(
                "microsoft365_documents_read",
                format!("'{}' is a folder, not a document", item.relative_path),
            ));
        }
        let transport_limit = documents_bounded_read_bytes(max_chars);
        let raw = client.read_item_bytes(http, &item, transport_limit)?;
        let decoded = decode_readable_document(
            &item.relative_path,
            &raw.bytes,
            max_chars.max(1),
            "microsoft365_documents_read",
        )?;
        Ok(DocumentsReadResult {
            entry: item.to_documents_entry(),
            content: decoded.content,
            truncated: decoded.truncated || raw.truncated,
            raw_bytes: decoded.raw_bytes,
            warning: merge_document_warning(
                decoded.warning,
                raw.truncated.then_some(PARTIAL_DOCUMENT_READ_WARNING),
            ),
        })
    }

    fn search_documents(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        let client = Microsoft365DocumentsClient::new(credential)?;
        let search_hits = client.search_entries(http, &query.query, query.limit.max(20))?;
        let max_read_bytes = search_read_budget(query.max_read_bytes);
        search_scoped_candidates(
            http,
            &query,
            &query.path,
            search_hits,
            SearchLimits::new(MAX_SEARCH_SCAN_ENTRIES, max_read_bytes),
            |item| item.to_documents_entry(),
            |http, item, entry, read_limit| {
                let raw = client.read_item_bytes(http, item, read_limit)?;
                Ok(SearchReadOutcome::new(
                    decode_searchable_document_text(&entry.path, &raw.bytes),
                    raw.truncated,
                ))
            },
        )
    }
}

pub struct Microsoft365DocumentsOfficeProbeAdapter;

impl OfficeProbeAdapter for Microsoft365DocumentsOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "microsoft365_documents"
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
        if validate_microsoft365_documents_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "documents_transport_config_missing".to_string(),
            });
        }
        let client = Microsoft365DocumentsClient::new(&adapted)?;
        client.list_entries(http, "")?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "microsoft365_drive_list_ok".to_string(),
        })
    }
}

struct Microsoft365DocumentsClient<'a> {
    credential: &'a DocumentsProviderCredential,
}

#[derive(Clone, Debug)]
struct MicrosoftDriveItem {
    name: String,
    relative_path: String,
    is_folder: bool,
    content_type: Option<String>,
    size_bytes: Option<u64>,
}

impl Microsoft365DocumentsClient<'_> {
    fn new(credential: &DocumentsProviderCredential) -> Result<Microsoft365DocumentsClient<'_>> {
        validate_microsoft365_documents_credential(credential)?;
        Ok(Microsoft365DocumentsClient { credential })
    }

    fn list_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        relative_path: &str,
    ) -> Result<Vec<DocumentsEntry>> {
        let request_path = scoped_relative_path(&self.credential.root_path, relative_path);
        let path = if request_path.is_empty() {
            format!("{}/children", self.drive_root_path())
        } else {
            format!(
                "{}:/{}:/children",
                self.drive_root_path(),
                encode_path_segments(&request_path)
            )
        };
        let auth = self.auth_header();
        let payload: MicrosoftGraphDriveCollection = request_microsoft_graph_json(
            http,
            "microsoft365_documents_list",
            "GET",
            &self.endpoint(&path, &[("$top", "200".to_string())]),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        let current_parent = normalize_relative_path(relative_path);
        Ok(payload
            .value
            .into_iter()
            .map(|item| {
                MicrosoftDriveItem::from_graph_item(item, &current_parent).to_documents_entry()
            })
            .collect())
    }

    fn resolve_item(
        &self,
        http: &mut dyn OfficeHttpClient,
        relative_path: &str,
    ) -> Result<MicrosoftDriveItem> {
        let request_path = scoped_relative_path(&self.credential.root_path, relative_path);
        if request_path.is_empty() {
            return Ok(MicrosoftDriveItem {
                name: root_entry_name(&self.credential.root_path),
                relative_path: String::new(),
                is_folder: true,
                content_type: None,
                size_bytes: None,
            });
        }
        let path = format!(
            "{}:/{}",
            self.drive_root_path(),
            encode_path_segments(&request_path)
        );
        let auth = self.auth_header();
        let item: MicrosoftGraphDriveItem = request_microsoft_graph_json(
            http,
            "microsoft365_documents_resolve",
            "GET",
            &self.endpoint(
                &path,
                &[(
                    "$select",
                    "id,name,folder,file,size,parentReference,webUrl".to_string(),
                )],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        Ok(MicrosoftDriveItem::from_graph_item(
            item,
            &parent_relative_path(relative_path),
        ))
    }

    fn read_item_bytes(
        &self,
        http: &mut dyn OfficeHttpClient,
        item: &MicrosoftDriveItem,
        max_bytes: usize,
    ) -> Result<crate::office::OfficeBoundedBytes> {
        let request_path = scoped_relative_path(&self.credential.root_path, &item.relative_path);
        let path = format!(
            "{}:/{}:/content",
            self.drive_root_path(),
            encode_path_segments(&request_path)
        );
        let auth = self.auth_header();
        read_bounded_http_bytes(
            http,
            "microsoft365_documents_read",
            &self.endpoint(&path, &[]),
            &[("Authorization", auth.as_str())],
            max_bytes,
        )
    }

    fn search_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MicrosoftDriveItem>> {
        let auth = self.auth_header();
        let payload: MicrosoftGraphDriveCollection = request_microsoft_graph_json(
            http,
            "microsoft365_documents_search",
            "GET",
            &self.endpoint(
                &format!(
                    "{}/search(q='{}')",
                    self.drive_root_path(),
                    query.replace(['\'', '"'], " ").trim()
                ),
                &[
                    ("$top", limit.clamp(1, 50).to_string()),
                    (
                        "$select",
                        "id,name,folder,file,size,parentReference,webUrl".to_string(),
                    ),
                ],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        Ok(payload
            .value
            .into_iter()
            .map(|item| MicrosoftDriveItem::from_graph_item(item, ""))
            .collect())
    }

    fn drive_root_path(&self) -> String {
        if self.credential.space_id.trim().is_empty() {
            "/me/drive/root".to_string()
        } else {
            format!(
                "/drives/{}/root",
                urlencoding::encode(self.credential.space_id.trim())
            )
        }
    }

    fn endpoint(&self, path: &str, query: &[(&str, String)]) -> String {
        build_microsoft_graph_url(&self.credential.base_url, path, query)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.credential.secret)
    }
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphDriveCollection {
    #[serde(default)]
    value: Vec<MicrosoftGraphDriveItem>,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphDriveItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    folder: Option<serde_json::Value>,
    #[serde(default)]
    file: Option<MicrosoftGraphFileFacet>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default, rename = "parentReference")]
    parent_reference: Option<MicrosoftGraphParentReference>,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphFileFacet {
    #[serde(default, rename = "mimeType")]
    mime_type: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphParentReference {
    #[serde(default)]
    path: String,
}

impl MicrosoftDriveItem {
    fn from_graph_item(item: MicrosoftGraphDriveItem, current_parent: &str) -> Self {
        let normalized_parent = if current_parent.trim().is_empty() {
            graph_parent_relative_path(item.parent_reference.as_ref()).unwrap_or_default()
        } else {
            normalize_relative_path(current_parent)
        };
        let relative_path = if normalized_parent.is_empty() {
            normalize_relative_path(&item.name)
        } else {
            format!("{normalized_parent}/{}", item.name.trim())
        };
        Self {
            name: item.name.trim().to_string(),
            relative_path,
            is_folder: item.folder.is_some(),
            content_type: item.file.and_then(|file| {
                let mime = file.mime_type.trim().to_string();
                (!mime.is_empty()).then_some(mime)
            }),
            size_bytes: item.size,
        }
    }

    fn is_dir(&self) -> bool {
        self.is_folder
    }

    fn to_documents_entry(&self) -> DocumentsEntry {
        DocumentsEntry {
            path: self.relative_path.clone(),
            name: self.name.clone(),
            kind: if self.is_folder {
                "folder".to_string()
            } else {
                self.content_type.as_deref().unwrap_or("file").to_string()
            },
            is_dir: self.is_folder,
            content_type: self.content_type.clone(),
            size_bytes: self.size_bytes,
        }
    }
}

fn graph_parent_relative_path(
    parent_reference: Option<&MicrosoftGraphParentReference>,
) -> Option<String> {
    let raw = parent_reference?.path.trim();
    let (_, suffix) = raw.split_once("/root:")?;
    Some(normalize_relative_path(suffix))
}

fn validate_microsoft365_documents_credential(
    credential: &DocumentsProviderCredential,
) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_documents_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_documents_provider",
            "documents_base_url must not be empty",
        ));
    }
    if credential.root_path.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_documents_provider",
            "documents_root_path must not be empty",
        ));
    }
    Ok(())
}

fn scoped_relative_path(root_path: &str, relative_path: &str) -> String {
    let root = normalize_relative_path(root_path);
    let relative = normalize_relative_path(relative_path);
    if root.is_empty() {
        relative
    } else if relative.is_empty() {
        root
    } else {
        format!("{root}/{relative}")
    }
}

fn normalize_relative_path(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn parent_relative_path(path: &str) -> String {
    let normalized = normalize_relative_path(path);
    normalized
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn encode_path_segments(path: &str) -> String {
    normalize_relative_path(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(urlencoding::encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn root_entry_name(root_path: &str) -> String {
    let normalized = normalize_relative_path(root_path);
    normalized
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("root")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccountIdentityClass, OfficeCapability, OfficeCredential};

    #[test]
    fn microsoft365_documents_provider_reports_graph_capabilities() {
        let provider = Microsoft365DocumentsProvider;
        assert_eq!(provider.provider_name(), "microsoft365_documents");
        assert!(provider.supports(DocumentsOperation::List));
        assert!(provider.supports(DocumentsOperation::Read));
        assert!(provider.supports(DocumentsOperation::Search));
    }

    #[test]
    fn microsoft365_documents_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = Microsoft365DocumentsOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "docs-ms".to_string(),
                    provider_kind: "microsoft365_documents".to_string(),
                    external_account_id: "alice@contoso.com".to_string(),
                    account_label: "Microsoft Docs".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Documents],
                },
                &OfficeCredential {
                    account_key: "docs-ms".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
        assert_eq!(result.provider_kind, "microsoft365_documents");
    }

    #[test]
    fn scoped_relative_path_joins_root_and_child_paths() {
        assert_eq!(
            scoped_relative_path("/Shared", "Plans/Q2"),
            "Shared/Plans/Q2"
        );
        assert_eq!(scoped_relative_path("/", "Plans/Q2"), "Plans/Q2");
    }
}
