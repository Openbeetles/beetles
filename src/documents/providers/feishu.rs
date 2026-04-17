#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::documents::credentials::documents_credential_from_office;
use crate::documents::{
    build_search_snippet, contains_query_text, documents_bounded_read_bytes,
    documents_search_match_kind, documents_search_match_score, merge_document_warning,
    DocumentsEntry, DocumentsOperation, DocumentsProvider, DocumentsProviderCredential,
    DocumentsQuery, DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery,
    DOCUMENTS_SEARCH_MATCH_PATH, EMPTY_DOCUMENT_WARNING, PARTIAL_DOCUMENT_READ_WARNING,
};
use crate::error::{Error, Result};
use crate::office::{
    read_bounded_http_bytes, OfficeAccount, OfficeHttpClient, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const MAX_SEARCH_SCAN_ENTRIES: usize = 64;
const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;
const FEISHU_LIST_PAGE_SIZE: usize = 200;

pub struct FeishuDocumentsProvider;

impl DocumentsProvider for FeishuDocumentsProvider {
    fn provider_name(&self) -> &'static str {
        "feishu_documents"
    }

    fn display_name(&self) -> &'static str {
        "Feishu Documents"
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
        let client = FeishuDocumentsClient::new(http, credential)?;
        let normalized_path = normalize_relative_path(&query.path);
        let folder = client.resolve_folder(http, &normalized_path)?;
        let mut entries = client.list_folder_entries(http, &folder.token, &folder.path)?;
        entries.sort_by(|left, right| {
            right
                .is_dir
                .cmp(&left.is_dir)
                .then_with(|| left.path.cmp(&right.path))
        });
        if entries.len() > query.limit {
            entries.truncate(query.limit);
        }
        Ok(entries)
    }

    fn read_document(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &DocumentsProviderCredential,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        let client = FeishuDocumentsClient::new(http, credential)?;
        let resolved = client.resolve_entry(http, path)?;
        if resolved.item.is_dir() {
            return Err(Error::config(
                "feishu_documents_read",
                format!("'{}' is a folder, not a document", resolved.path),
            ));
        }
        let transport_limit = documents_bounded_read_bytes(max_chars);
        let read = client.read_supported_document(http, &resolved.item, transport_limit)?;
        let trimmed = normalize_document_text(&read.content);
        let (content, truncated) = truncate_chars(&trimmed, max_chars.max(1));
        Ok(DocumentsReadResult {
            entry: resolved.item.to_documents_entry(&resolved.parent_path),
            content,
            truncated: truncated || read.truncated,
            raw_bytes: trimmed.len(),
            warning: merge_document_warning(
                trimmed
                    .is_empty()
                    .then(|| EMPTY_DOCUMENT_WARNING.to_string()),
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
        let client = FeishuDocumentsClient::new(http, credential)?;
        let normalized_path = normalize_relative_path(&query.path);
        let start_folder = client.resolve_folder(http, &normalized_path)?;
        let mut queue = VecDeque::from([start_folder]);
        let mut hits = Vec::new();
        let mut scanned_entries = 0usize;
        let max_read_bytes = if query.max_read_bytes == 0 {
            MAX_SEARCH_READ_BYTES
        } else {
            query.max_read_bytes.min(MAX_SEARCH_READ_BYTES)
        };

        while let Some(folder) = queue.pop_front() {
            if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                break;
            }
            let entries = client.list_folder_items(http, &folder.token)?;
            for item in entries {
                let entry = item.to_documents_entry(&folder.path);
                scanned_entries += 1;
                let path_hit = contains_query_text(&entry.path, &query.query, query.case_sensitive)
                    || contains_query_text(&entry.name, &query.query, query.case_sensitive);
                if item.is_dir() {
                    if path_hit {
                        hits.push(DocumentsSearchHit {
                            entry: entry.clone(),
                            match_kind: DOCUMENTS_SEARCH_MATCH_PATH.to_string(),
                            snippet: None,
                            warning: None,
                        });
                    }
                    queue.push_back(ResolvedFolder {
                        token: item.token.clone(),
                        path: entry.path.clone(),
                    });
                    if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                        break;
                    }
                    continue;
                }

                let mut content_match = None;
                let mut warning = None;
                if item.supports_raw_read() {
                    let read = client.read_supported_document(http, &item, max_read_bytes)?;
                    let normalized = normalize_document_text(&read.content);
                    if contains_query_text(&normalized, &query.query, query.case_sensitive) {
                        content_match = Some(build_search_snippet(
                            &normalized,
                            &query.query,
                            query.case_sensitive,
                        ));
                    }
                    if read.truncated {
                        warning = Some(PARTIAL_DOCUMENT_READ_WARNING.to_string());
                    }
                }

                if let Some(match_kind) =
                    documents_search_match_kind(path_hit, content_match.is_some())
                {
                    hits.push(DocumentsSearchHit {
                        entry: entry.clone(),
                        match_kind: match_kind.to_string(),
                        snippet: content_match,
                        warning,
                    });
                }

                if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                    break;
                }
            }
        }

        hits.sort_by(|left, right| {
            documents_search_match_score(&right.match_kind)
                .cmp(&documents_search_match_score(&left.match_kind))
                .then_with(|| left.entry.path.cmp(&right.entry.path))
        });
        if hits.len() > query.limit {
            hits.truncate(query.limit);
        }
        Ok(hits)
    }
}

pub struct FeishuDocumentsOfficeProbeAdapter;

impl OfficeProbeAdapter for FeishuDocumentsOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "feishu_documents"
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
        if validate_feishu_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "documents_transport_config_missing".to_string(),
            });
        }
        let client = FeishuDocumentsClient::new(http, &adapted)?;
        client.list_folder_items(http, &adapted.root_path)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "feishu_drive_list_ok".to_string(),
        })
    }
}

#[derive(Clone)]
struct FeishuDocumentsClient<'a> {
    credential: &'a DocumentsProviderCredential,
    tenant_access_token: String,
}

struct FeishuRawDocumentRead {
    content: String,
    truncated: bool,
}

#[derive(Clone, Debug)]
struct ResolvedFolder {
    token: String,
    path: String,
}

#[derive(Clone, Debug)]
struct ResolvedEntry {
    item: FeishuDriveFile,
    path: String,
    parent_path: String,
}

impl<'a> FeishuDocumentsClient<'a> {
    fn new(
        http: &mut dyn OfficeHttpClient,
        credential: &'a DocumentsProviderCredential,
    ) -> Result<Self> {
        validate_feishu_credential(credential)?;
        Ok(Self {
            credential,
            tenant_access_token: fetch_tenant_access_token(http, credential)?,
        })
    }

    fn resolve_folder(
        &self,
        http: &mut dyn OfficeHttpClient,
        path: &str,
    ) -> Result<ResolvedFolder> {
        let normalized = normalize_relative_path(path);
        if normalized.is_empty() {
            return Ok(ResolvedFolder {
                token: self.credential.root_path.clone(),
                path: String::new(),
            });
        }
        let mut current = ResolvedFolder {
            token: self.credential.root_path.clone(),
            path: String::new(),
        };
        for component in normalized.split('/') {
            let item = self
                .list_folder_items(http, &current.token)?
                .into_iter()
                .find(|item| item.is_dir() && item.name == component)
                .ok_or_else(|| {
                    Error::config(
                        "feishu_documents_path",
                        format!(
                            "folder '{}' was not found under '{}'",
                            component, current.path
                        ),
                    )
                })?;
            current = ResolvedFolder {
                token: item.token,
                path: join_relative_path(&current.path, component),
            };
        }
        Ok(current)
    }

    fn resolve_entry(&self, http: &mut dyn OfficeHttpClient, path: &str) -> Result<ResolvedEntry> {
        let normalized = normalize_relative_path(path);
        if normalized.is_empty() {
            return Err(Error::config(
                "feishu_documents_path",
                "read path must not be empty",
            ));
        }
        let mut segments = normalized.split('/').collect::<Vec<_>>();
        let leaf = segments.pop().unwrap_or_default();
        let parent_path = segments.join("/");
        let folder = self.resolve_folder(http, &parent_path)?;
        let item = self
            .list_folder_items(http, &folder.token)?
            .into_iter()
            .find(|item| item.name == leaf)
            .ok_or_else(|| {
                Error::config(
                    "feishu_documents_path",
                    format!("'{}' was not found", normalized),
                )
            })?;
        Ok(ResolvedEntry {
            path: join_relative_path(&parent_path, leaf),
            item,
            parent_path,
        })
    }

    fn list_folder_entries(
        &self,
        http: &mut dyn OfficeHttpClient,
        folder_token: &str,
        folder_path: &str,
    ) -> Result<Vec<DocumentsEntry>> {
        Ok(self
            .list_folder_items(http, folder_token)?
            .into_iter()
            .map(|item| item.to_documents_entry(folder_path))
            .collect())
    }

    fn list_folder_items(
        &self,
        http: &mut dyn OfficeHttpClient,
        folder_token: &str,
    ) -> Result<Vec<FeishuDriveFile>> {
        let mut files = Vec::new();
        let mut next_page_token = None::<String>;
        loop {
            let data = self
                .api_get::<FeishuDriveListResponse>(
                    http,
                    "feishu_documents_list",
                    "/open-apis/drive/v1/files",
                    &[
                        ("folder_token", folder_token.to_string()),
                        ("page_size", FEISHU_LIST_PAGE_SIZE.to_string()),
                        ("page_token", next_page_token.clone().unwrap_or_default()),
                    ],
                )?
                .require_data("feishu_documents_list")?;
            files.extend(data.files);
            if !data.has_more.unwrap_or(false)
                || data.next_page_token.as_deref().unwrap_or("").is_empty()
            {
                break;
            }
            next_page_token = data.next_page_token;
        }
        Ok(files)
    }

    fn read_supported_document(
        &self,
        http: &mut dyn OfficeHttpClient,
        item: &FeishuDriveFile,
        max_response_bytes: usize,
    ) -> Result<FeishuRawDocumentRead> {
        match item.file_type.as_str() {
            "docx" => self.read_docx_raw_content(http, &item.token, max_response_bytes),
            other => Err(Error::config(
                "feishu_documents_read",
                format!("file type '{}' is not readable yet", other),
            )),
        }
    }

    fn read_docx_raw_content(
        &self,
        http: &mut dyn OfficeHttpClient,
        document_id: &str,
        max_response_bytes: usize,
    ) -> Result<FeishuRawDocumentRead> {
        let url = build_api_url(
            &self.credential.base_url,
            &format!("/open-apis/docx/v1/documents/{document_id}/raw_content"),
            &[],
        );
        let auth = format!("Bearer {}", self.tenant_access_token);
        let bounded = read_bounded_http_bytes(
            http,
            "feishu_documents_read",
            &url,
            &[("Authorization", auth.as_str())],
            max_response_bytes.max(1),
        )?;
        let data: FeishuDocxRawContentResponse = serde_json::from_slice(&bounded.bytes).map_err(|error| {
            if bounded.truncated {
                Error::config(
                    "feishu_documents_read",
                    format!(
                        "document exceeded bounded read budget of {} bytes; increase max_read_bytes to inspect more",
                        max_response_bytes.max(1)
                    ),
                )
            } else {
                Error::config("feishu_documents_read", error.to_string())
            }
        })?;
        let data = data.require_data("feishu_documents_read")?;
        Ok(FeishuRawDocumentRead {
            content: data.content.unwrap_or_default(),
            truncated: bounded.truncated,
        })
    }

    fn api_get<T: DeserializeOwned>(
        &self,
        http: &mut dyn OfficeHttpClient,
        stage: &'static str,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let url = build_api_url(&self.credential.base_url, path, params);
        let auth = format!("Bearer {}", self.tenant_access_token);
        request_feishu_json(
            http,
            stage,
            "GET",
            &url,
            &[("Authorization", auth.as_str())],
            None,
        )
    }
}

#[derive(Deserialize)]
struct FeishuTokenResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    tenant_access_token: String,
}

#[derive(Deserialize)]
struct FeishuApiEnvelope<T> {
    code: i32,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

impl<T> FeishuApiEnvelope<T> {
    fn require_data(self, stage: &'static str) -> Result<T> {
        if self.code != 0 {
            return Err(Error::config(
                stage,
                if self.msg.trim().is_empty() {
                    format!("Feishu API failed with code {}", self.code)
                } else {
                    self.msg
                },
            ));
        }
        self.data
            .ok_or_else(|| Error::config(stage, "Feishu API returned no data"))
    }
}

#[derive(Clone, Debug, Deserialize)]
struct FeishuDriveListData {
    #[serde(default)]
    files: Vec<FeishuDriveFile>,
    #[serde(default)]
    next_page_token: Option<String>,
    #[serde(default)]
    has_more: Option<bool>,
}

type FeishuDriveListResponse = FeishuApiEnvelope<FeishuDriveListData>;

#[derive(Clone, Debug, Deserialize)]
struct FeishuDriveFile {
    token: String,
    name: String,
    #[serde(rename = "type")]
    file_type: String,
    #[serde(default)]
    size: Option<serde_json::Value>,
}

impl FeishuDriveFile {
    fn is_dir(&self) -> bool {
        self.file_type == "folder"
    }

    fn supports_raw_read(&self) -> bool {
        self.file_type == "docx"
    }

    fn to_documents_entry(&self, parent_path: &str) -> DocumentsEntry {
        DocumentsEntry {
            path: join_relative_path(parent_path, &self.name),
            name: self.name.clone(),
            kind: if self.is_dir() {
                "folder".to_string()
            } else {
                self.file_type.clone()
            },
            is_dir: self.is_dir(),
            content_type: None,
            size_bytes: parse_size_bytes(self.size.as_ref()),
        }
    }
}

#[derive(Deserialize)]
struct FeishuDocxRawContentData {
    #[serde(default)]
    content: Option<String>,
}

type FeishuDocxRawContentResponse = FeishuApiEnvelope<FeishuDocxRawContentData>;

#[derive(Serialize)]
struct FeishuTenantAccessTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

fn validate_feishu_credential(credential: &DocumentsProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "feishu_documents_provider",
            "app_id must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "feishu_documents_provider",
            "secret must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "feishu_documents_provider",
            "base_url must not be empty",
        ));
    }
    if credential.root_path.trim().is_empty() {
        return Err(Error::config(
            "feishu_documents_provider",
            "root_path must not be empty",
        ));
    }
    Ok(())
}

fn fetch_tenant_access_token(
    http: &mut dyn OfficeHttpClient,
    credential: &DocumentsProviderCredential,
) -> Result<String> {
    let url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        credential.base_url.trim_end_matches('/')
    );
    let body = serde_json::to_string(&FeishuTenantAccessTokenRequest {
        app_id: credential.app_id.as_str(),
        app_secret: credential.secret.as_str(),
    })
    .map_err(|error| Error::config("feishu_documents_auth", error.to_string()))?;
    let payload: FeishuTokenResponse = request_feishu_json(
        http,
        "feishu_documents_auth",
        "POST",
        &url,
        &[("Content-Type", "application/json")],
        Some(body.as_bytes()),
    )?;
    if payload.code != 0 {
        return Err(Error::config(
            "feishu_documents_auth",
            if payload.msg.trim().is_empty() {
                format!("Feishu auth failed with code {}", payload.code)
            } else {
                payload.msg
            },
        ));
    }
    if payload.tenant_access_token.trim().is_empty() {
        return Err(Error::config(
            "feishu_documents_auth",
            "tenant_access_token must not be empty",
        ));
    }
    Ok(payload.tenant_access_token)
}

fn request_feishu_json<T: DeserializeOwned>(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<T> {
    let (status, response_body) = http.request_with_headers(method, url, headers, body)?;
    if !(200..300).contains(&status) {
        return Err(Error::http(stage, status));
    }
    serde_json::from_slice(response_body.as_slice())
        .map_err(|error| Error::config(stage, error.to_string()))
}

fn build_api_url(base_url: &str, path: &str, params: &[(&str, String)]) -> String {
    let mut url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let query = params
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| format!("{key}={}", urlencoding::encode(value)))
        .collect::<Vec<_>>();
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    url
}

fn normalize_relative_path(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn join_relative_path(parent: &str, name: &str) -> String {
    let normalized_parent = normalize_relative_path(parent);
    let normalized_name = name.trim_matches('/').trim();
    if normalized_parent.is_empty() {
        normalized_name.to_string()
    } else if normalized_name.is_empty() {
        normalized_parent
    } else {
        format!("{normalized_parent}/{normalized_name}")
    }
}

fn normalize_document_text(content: &str) -> String {
    content
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), false);
    }
    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("\n\n... [truncated] ...");
    (out, true)
}

fn parse_size_bytes(value: Option<&serde_json::Value>) -> Option<u64> {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_u64(),
        Some(serde_json::Value::String(text)) => text.parse::<u64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tiny_http::{Method, Response, Server, StatusCode};

    fn credential(base_url: &str) -> DocumentsProviderCredential {
        DocumentsProviderCredential {
            account_key: "docs-feishu".to_string(),
            provider: "feishu_documents".to_string(),
            account_id: "tenant-docs".to_string(),
            account_label: "Feishu Docs".to_string(),
            username: String::new(),
            secret: "app-secret".to_string(),
            app_id: "cli_a1b2c3".to_string(),
            space_id: String::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            root_path: "fldcn-root".to_string(),
        }
    }

    fn spawn_feishu_stub_server() -> (String, thread::JoinHandle<()>) {
        let server = Server::http("127.0.0.1:0").expect("bind stub server");
        let base_url = format!("http://{}", server.server_addr());
        let handle = thread::spawn(move || {
            for _ in 0..10 {
                let mut request = match server.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                };
                let url = request.url().to_string();
                let method = request.method().clone();
                let method_for_panic = format!("{:?}", method);
                let mut body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut body)
                    .expect("read request body");
                let response_body = match (method, url.as_str()) {
                    (Method::Post, "/open-apis/auth/v3/tenant_access_token/internal") => {
                        assert!(body.contains("\"app_id\":\"cli_a1b2c3\""));
                        assert!(body.contains("\"app_secret\":\"app-secret\""));
                        r#"{"code":0,"msg":"ok","tenant_access_token":"tenant-token"}"#
                    }
                    (Method::Get, path)
                        if path
                            .starts_with("/open-apis/drive/v1/files?folder_token=fldcn-root") =>
                    {
                        r#"{"code":0,"data":{"files":[{"token":"fldcn-reports","name":"Reports","type":"folder","url":"https://example.com/reports"},{"token":"doccn-quarterly","name":"Quarterly Plan","type":"docx","url":"https://example.com/docx/quarterly"}]}}"#
                    }
                    (Method::Get, path)
                        if path.starts_with(
                            "/open-apis/drive/v1/files?folder_token=fldcn-reports",
                        ) =>
                    {
                        r#"{"code":0,"data":{"files":[{"token":"doccn-review","name":"Q1 Review","type":"docx","url":"https://example.com/docx/review"}]}}"#
                    }
                    (Method::Get, "/open-apis/docx/v1/documents/doccn-quarterly/raw_content") => {
                        r#"{"code":0,"data":{"content":"Quarterly plan summary\nAction: review launch checklist"}}"#
                    }
                    (Method::Get, "/open-apis/docx/v1/documents/doccn-review/raw_content") => {
                        r#"{"code":0,"data":{"content":"Q1 Review notes\nCalendar bridge shipped\nDocuments summary landed"}}"#
                    }
                    _ => panic!("unexpected request: {} {}", method_for_panic, url),
                };
                let response = Response::from_string(response_body)
                    .with_status_code(StatusCode(200))
                    .with_header(
                        tiny_http::Header::from_bytes(
                            b"Content-Type" as &[u8],
                            b"application/json" as &[u8],
                        )
                        .expect("json header"),
                    );
                request.respond(response).expect("write response");
            }
        });
        (base_url, handle)
    }

    #[test]
    fn feishu_documents_provider_reports_supported_operations() {
        let provider = FeishuDocumentsProvider;
        assert!(provider.supports(DocumentsOperation::List));
        assert!(provider.supports(DocumentsOperation::Read));
        assert!(provider.supports(DocumentsOperation::Search));
    }

    #[test]
    fn feishu_documents_provider_lists_reads_and_searches_shared_folder() {
        let (base_url, handle) = spawn_feishu_stub_server();
        let provider = FeishuDocumentsProvider;
        let credential = credential(&base_url);
        let mut http = crate::platform::EspHttpClient::new().expect("http client");

        let entries = provider
            .list_entries(
                &mut http,
                &credential,
                DocumentsQuery {
                    path: String::new(),
                    limit: 10,
                },
            )
            .expect("list root entries");
        assert_eq!(entries[0].path, "Reports");
        assert_eq!(entries[1].path, "Quarterly Plan");
        assert_eq!(entries[1].kind, "docx");

        let document = provider
            .read_document(&mut http, &credential, "Quarterly Plan", 10_000)
            .expect("read docx content");
        assert!(document.content.contains("Quarterly plan summary"));
        assert_eq!(document.entry.kind, "docx");

        let hits = provider
            .search_documents(
                &mut http,
                &credential,
                DocumentsSearchQuery {
                    path: String::new(),
                    query: "calendar".to_string(),
                    limit: 10,
                    case_sensitive: false,
                    max_read_bytes: 32 * 1024,
                },
            )
            .expect("search shared folder");
        assert_eq!(hits[0].entry.path, "Reports/Q1 Review");
        assert_eq!(hits[0].match_kind, "content");
        assert!(hits[0]
            .snippet
            .as_deref()
            .expect("snippet")
            .contains("Calendar bridge shipped"));

        handle.join().expect("stub server exits cleanly");
    }
}
