#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::documents::credentials::documents_credential_from_office;
use crate::documents::{
    build_search_snippet, contains_query_text, decode_readable_document,
    decode_searchable_document_text, DocumentsEntry, DocumentsOperation, DocumentsProvider,
    DocumentsProviderCredential, DocumentsQuery, DocumentsReadResult, DocumentsSearchHit,
    DocumentsSearchQuery,
};
use crate::error::{Error, Result};
use crate::office::{
    fetch_wecom_access_token_ureq, request_wecom_json_ureq, OfficeAccount, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult, WecomApiEnvelope, WecomAuthCredential,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::VecDeque;
use std::io::Read;

const MAX_SEARCH_SCAN_ENTRIES: usize = 64;
const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;
const WECOM_LIST_LIMIT: usize = 100;

pub struct WecomDocumentsProvider;

impl DocumentsProvider for WecomDocumentsProvider {
    fn provider_name(&self) -> &'static str {
        "wecom_documents"
    }

    fn display_name(&self) -> &'static str {
        "WeCom Documents"
    }

    fn supports(&self, op: DocumentsOperation) -> bool {
        matches!(
            op,
            DocumentsOperation::List | DocumentsOperation::Read | DocumentsOperation::Search
        )
    }

    fn list_entries(
        &self,
        credential: &DocumentsProviderCredential,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        let client = WecomDocumentsClient::new(credential)?;
        let folder = client.resolve_folder(&normalize_relative_path(&query.path))?;
        let mut entries = client.list_folder_entries(&folder.file_id, &folder.path)?;
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
        credential: &DocumentsProviderCredential,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        let client = WecomDocumentsClient::new(credential)?;
        let resolved = client.resolve_entry(path)?;
        if resolved.item.is_dir() {
            return Err(Error::config(
                "wecom_documents_read",
                format!("'{}' is a folder, not a document", resolved.path),
            ));
        }
        let raw = client.download_file(&resolved.item)?;
        let decoded = decode_readable_document(
            &resolved.path,
            &raw,
            max_chars.max(1),
            "wecom_documents_read",
        )?;
        Ok(DocumentsReadResult {
            entry: resolved.item.to_documents_entry(&resolved.parent_path),
            content: decoded.content,
            truncated: decoded.truncated,
            raw_bytes: decoded.raw_bytes,
            warning: decoded.warning,
        })
    }

    fn search_documents(
        &self,
        credential: &DocumentsProviderCredential,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        let client = WecomDocumentsClient::new(credential)?;
        let start_folder = client.resolve_folder(&normalize_relative_path(&query.path))?;
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
            let items = client.list_folder_items(&folder.file_id)?;
            for item in items {
                let entry = item.to_documents_entry(&folder.path);
                scanned_entries += 1;
                let path_hit = contains_query_text(&entry.path, &query.query, query.case_sensitive)
                    || contains_query_text(&entry.name, &query.query, query.case_sensitive);
                if item.is_dir() {
                    if path_hit {
                        hits.push(DocumentsSearchHit {
                            entry: entry.clone(),
                            match_kind: "path".to_string(),
                            snippet: None,
                            warning: None,
                        });
                    }
                    queue.push_back(ResolvedFolder {
                        file_id: item.file_id.clone(),
                        path: entry.path.clone(),
                    });
                    if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                        break;
                    }
                    continue;
                }

                let mut content_match = None;
                let mut warning = None;
                if item.size_bytes() as usize <= max_read_bytes {
                    let raw = client.download_file(&item)?;
                    if let Some(text) = decode_searchable_document_text(&entry.path, &raw) {
                        if contains_query_text(&text, &query.query, query.case_sensitive) {
                            content_match = Some(build_search_snippet(
                                &text,
                                &query.query,
                                query.case_sensitive,
                            ));
                        }
                    }
                } else {
                    warning =
                        Some("content not searched because the file is too large".to_string());
                }

                if path_hit || content_match.is_some() {
                    hits.push(DocumentsSearchHit {
                        entry: entry.clone(),
                        match_kind: match (path_hit, content_match.is_some()) {
                            (true, true) => "path+content",
                            (true, false) => "path",
                            (false, true) => "content",
                            (false, false) => unreachable!(),
                        }
                        .to_string(),
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
            search_score(&right.match_kind)
                .cmp(&search_score(&left.match_kind))
                .then_with(|| left.entry.path.cmp(&right.entry.path))
        });
        if hits.len() > query.limit {
            hits.truncate(query.limit);
        }
        Ok(hits)
    }
}

pub struct WecomDocumentsOfficeProbeAdapter;

impl OfficeProbeAdapter for WecomDocumentsOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "wecom_documents"
    }

    fn probe(
        &self,
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
        if validate_wecom_documents_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "documents_transport_config_missing".to_string(),
            });
        }
        let client = WecomDocumentsClient::new(&adapted)?;
        client.list_folder_items(&adapted.root_path)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "wecom_wedrive_list_ok".to_string(),
        })
    }
}

#[derive(Clone)]
struct WecomDocumentsClient<'a> {
    credential: &'a DocumentsProviderCredential,
    access_token: String,
}

#[derive(Clone, Debug)]
struct ResolvedFolder {
    file_id: String,
    path: String,
}

#[derive(Clone, Debug)]
struct ResolvedEntry {
    item: WecomDriveFile,
    path: String,
    parent_path: String,
}

impl<'a> WecomDocumentsClient<'a> {
    fn new(credential: &'a DocumentsProviderCredential) -> Result<Self> {
        validate_wecom_documents_credential(credential)?;
        let access_token = fetch_wecom_access_token_ureq(
            "wecom_documents_auth",
            WecomAuthCredential {
                corp_id: credential.app_id.as_str(),
                corp_secret: credential.secret.as_str(),
                base_url: credential.base_url.as_str(),
            },
        )?;
        Ok(Self {
            credential,
            access_token,
        })
    }

    fn resolve_folder(&self, path: &str) -> Result<ResolvedFolder> {
        let normalized = normalize_relative_path(path);
        if normalized.is_empty() {
            return Ok(ResolvedFolder {
                file_id: self.credential.root_path.clone(),
                path: String::new(),
            });
        }
        let mut current = ResolvedFolder {
            file_id: self.credential.root_path.clone(),
            path: String::new(),
        };
        for component in normalized.split('/') {
            let item = self
                .list_folder_items(&current.file_id)?
                .into_iter()
                .find(|item| item.is_dir() && item.file_name == component)
                .ok_or_else(|| {
                    Error::config(
                        "wecom_documents_path",
                        format!(
                            "folder '{}' was not found under '{}'",
                            component, current.path
                        ),
                    )
                })?;
            current = ResolvedFolder {
                file_id: item.file_id,
                path: join_relative_path(&current.path, component),
            };
        }
        Ok(current)
    }

    fn resolve_entry(&self, path: &str) -> Result<ResolvedEntry> {
        let normalized = normalize_relative_path(path);
        if normalized.is_empty() {
            return Err(Error::config(
                "wecom_documents_path",
                "read path must not be empty",
            ));
        }
        let mut segments = normalized.split('/').collect::<Vec<_>>();
        let leaf = segments.pop().unwrap_or_default();
        let parent_path = segments.join("/");
        let folder = self.resolve_folder(&parent_path)?;
        let item = self
            .list_folder_items(&folder.file_id)?
            .into_iter()
            .find(|item| item.file_name == leaf)
            .ok_or_else(|| {
                Error::config(
                    "wecom_documents_path",
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
        folder_id: &str,
        parent_path: &str,
    ) -> Result<Vec<DocumentsEntry>> {
        self.list_folder_items(folder_id)?
            .into_iter()
            .map(|item| Ok(item.to_documents_entry(parent_path)))
            .collect()
    }

    fn list_folder_items(&self, folder_id: &str) -> Result<Vec<WecomDriveFile>> {
        let mut start = 0usize;
        let mut out = Vec::new();
        loop {
            let payload: WecomApiEnvelope<WecomFileListPayload> = request_wecom_json_ureq(
                "wecom_documents_list",
                ureq::post(&self.endpoint("/cgi-bin/wedrive/file_list"))
                    .set("Content-Type", "application/json")
                    .send_string(
                        &json!({
                            "spaceid": self.credential.space_id,
                            "fatherid": folder_id,
                            "sort_type": 1,
                            "start": start,
                            "limit": WECOM_LIST_LIMIT
                        })
                        .to_string(),
                    ),
            )?;
            let data = payload.require_ok("wecom_documents_list")?;
            out.extend(data.file_list.item);
            if !data.has_more {
                break;
            }
            let Some(next_start) = data.next_start.parse::<usize>().ok() else {
                break;
            };
            if next_start <= start {
                break;
            }
            start = next_start;
        }
        Ok(out)
    }

    fn download_file(&self, item: &WecomDriveFile) -> Result<Vec<u8>> {
        let payload: WecomApiEnvelope<WecomFileDownloadPayload> = request_wecom_json_ureq(
            "wecom_documents_download",
            ureq::post(&self.endpoint("/cgi-bin/wedrive/file_download"))
                .set("Content-Type", "application/json")
                .send_string(
                    &json!({
                        "spaceid": self.credential.space_id,
                        "fileid": item.file_id,
                    })
                    .to_string(),
                ),
        )?;
        let data = payload.require_ok("wecom_documents_download")?;
        if data.download_url.trim().is_empty() {
            return Err(Error::config(
                "wecom_documents_download",
                "missing download_url",
            ));
        }
        let response = ureq::get(&data.download_url)
            .set(
                "Cookie",
                &format!("{}={}", data.cookie_name.trim(), data.cookie_value.trim()),
            )
            .call();
        let response = match response {
            Ok(response) => response,
            Err(ureq::Error::Status(status, _)) => {
                return Err(Error::config(
                    "wecom_documents_download",
                    format!("download failed with status {}", status),
                ))
            }
            Err(error) => return Err(Error::config("wecom_documents_download", error.to_string())),
        };
        let mut reader = response.into_reader();
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| Error::config("wecom_documents_download", error.to_string()))?;
        Ok(bytes)
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}{}?access_token={}",
            self.credential.base_url.trim_end_matches('/'),
            path,
            urlencoding::encode(&self.access_token)
        )
    }
}

#[derive(Debug, Default, Deserialize)]
struct WecomFileListPayload {
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    next_start: String,
    #[serde(default)]
    file_list: WecomFileListItems,
}

#[derive(Debug, Default, Deserialize)]
struct WecomFileListItems {
    #[serde(default)]
    item: Vec<WecomDriveFile>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct WecomDriveFile {
    #[serde(default, rename = "fileid")]
    file_id: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    file_type: String,
    #[serde(default)]
    file_size: String,
}

impl WecomDriveFile {
    fn is_dir(&self) -> bool {
        self.file_type.eq_ignore_ascii_case("folder")
    }

    fn size_bytes(&self) -> u64 {
        self.file_size.parse::<u64>().unwrap_or(0)
    }

    fn kind(&self) -> String {
        if self.is_dir() {
            "directory".to_string()
        } else {
            let normalized = self.file_type.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                "file".to_string()
            } else {
                normalized
            }
        }
    }

    fn to_documents_entry(&self, parent_path: &str) -> DocumentsEntry {
        DocumentsEntry {
            path: join_relative_path(parent_path, &self.file_name),
            name: self.file_name.clone(),
            kind: self.kind(),
            is_dir: self.is_dir(),
            content_type: None,
            size_bytes: (!self.is_dir()).then_some(self.size_bytes()),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct WecomFileDownloadPayload {
    #[serde(default)]
    download_url: String,
    #[serde(default)]
    cookie_name: String,
    #[serde(default)]
    cookie_value: String,
}

fn validate_wecom_documents_credential(credential: &DocumentsProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_documents_provider",
            "documents_corp_id must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "wecom_documents_provider",
            "secret must not be empty",
        ));
    }
    if credential.space_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_documents_provider",
            "documents_space_id must not be empty",
        ));
    }
    if credential.root_path.trim().is_empty() {
        return Err(Error::config(
            "wecom_documents_provider",
            "documents_root_path must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "wecom_documents_provider",
            "documents_base_url must not be empty",
        ));
    }
    Ok(())
}

fn normalize_relative_path(path: &str) -> String {
    path.trim().trim_matches('/').to_string()
}

fn join_relative_path(parent: &str, child: &str) -> String {
    let normalized_parent = normalize_relative_path(parent);
    let normalized_child = child.trim().trim_matches('/').to_string();
    match (normalized_parent.is_empty(), normalized_child.is_empty()) {
        (_, true) => normalized_parent,
        (true, false) => normalized_child,
        (false, false) => format!("{normalized_parent}/{normalized_child}"),
    }
}

fn search_score(match_kind: &str) -> u8 {
    match match_kind {
        "path+content" => 3,
        "path" => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tiny_http::{Method, Response, Server, StatusCode};

    fn credential(base_url: &str) -> DocumentsProviderCredential {
        DocumentsProviderCredential {
            account_key: "docs-wecom".to_string(),
            provider: "wecom_documents".to_string(),
            account_id: String::new(),
            account_label: "WeCom Docs".to_string(),
            username: String::new(),
            secret: "corp-secret".to_string(),
            app_id: "wwcorp123".to_string(),
            space_id: "space-1".to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            root_path: "folder-root".to_string(),
        }
    }

    fn json_response(body: String) -> Response<std::io::Cursor<Vec<u8>>> {
        Response::from_string(body)
            .with_status_code(StatusCode(200))
            .with_header(
                tiny_http::Header::from_bytes(
                    b"Content-Type" as &[u8],
                    b"application/json" as &[u8],
                )
                .expect("json header"),
            )
    }

    fn spawn_wecom_stub_server() -> (String, thread::JoinHandle<()>) {
        let server = Server::http("127.0.0.1:0").expect("bind stub server");
        let base_url = format!("http://{}", server.server_addr());
        let download_base = base_url.clone();
        let handle = thread::spawn(move || {
            for _ in 0..13 {
                let mut request = match server.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                };
                let url = request.url().to_string();
                let method = request.method().clone();
                let mut body = String::new();
                request
                    .as_reader()
                    .read_to_string(&mut body)
                    .expect("read request body");
                let response = match (method, url.as_str()) {
                    (Method::Get, path) if path.starts_with("/cgi-bin/gettoken?") => json_response(
                        r#"{"errcode":0,"errmsg":"ok","access_token":"tenant-token"}"#.to_string(),
                    ),
                    (Method::Post, path) if path.starts_with("/cgi-bin/wedrive/file_list?") => {
                        let payload = if body.contains("\"fatherid\":\"folder-root\"") {
                            r#"{"errcode":0,"errmsg":"ok","has_more":false,"next_start":"0","file_list":{"item":[{"fileid":"folder-reports","file_name":"Reports","file_type":"folder","file_size":"0"},{"fileid":"file-quarterly","file_name":"Quarterly Plan.txt","file_type":"txt","file_size":"54"}]}}"#
                        } else if body.contains("\"fatherid\":\"folder-reports\"") {
                            r#"{"errcode":0,"errmsg":"ok","has_more":false,"next_start":"0","file_list":{"item":[{"fileid":"file-review","file_name":"Q1 Review.txt","file_type":"txt","file_size":"62"}]}}"#
                        } else {
                            panic!("unexpected file_list body: {body}");
                        };
                        json_response(payload.to_string())
                    }
                    (Method::Post, path) if path.starts_with("/cgi-bin/wedrive/file_download?") => {
                        let target = if body.contains("\"fileid\":\"file-quarterly\"") {
                            format!("{download_base}/download/file-quarterly")
                        } else if body.contains("\"fileid\":\"file-review\"") {
                            format!("{download_base}/download/file-review")
                        } else {
                            panic!("unexpected file_download body: {body}");
                        };
                        json_response(format!(
                            r#"{{"errcode":0,"errmsg":"ok","download_url":"{target}","cookie_name":"wedrive_session","cookie_value":"cookie-token"}}"#
                        ))
                    }
                    (Method::Get, "/download/file-quarterly") => {
                        assert!(request.headers().iter().any(|header| {
                            header.field.equiv("Cookie")
                                && header
                                    .value
                                    .as_str()
                                    .contains("wedrive_session=cookie-token")
                        }));
                        Response::from_string(
                            "Quarterly plan summary\nAction: review launch checklist",
                        )
                        .with_status_code(StatusCode(200))
                    }
                    (Method::Get, "/download/file-review") => {
                        assert!(request.headers().iter().any(|header| {
                            header.field.equiv("Cookie")
                                && header
                                    .value
                                    .as_str()
                                    .contains("wedrive_session=cookie-token")
                        }));
                        Response::from_string(
                            "Q1 Review notes\nCalendar bridge shipped\nDocuments summary landed",
                        )
                        .with_status_code(StatusCode(200))
                    }
                    (method, path) => panic!("unexpected request: {:?} {}", method, path),
                };
                request.respond(response).expect("write response");
            }
        });
        (base_url, handle)
    }

    #[test]
    fn wecom_documents_provider_reports_supported_operations() {
        let provider = WecomDocumentsProvider;
        assert!(provider.supports(DocumentsOperation::List));
        assert!(provider.supports(DocumentsOperation::Read));
        assert!(provider.supports(DocumentsOperation::Search));
    }

    #[test]
    fn wecom_documents_provider_lists_reads_and_searches_folder() {
        let (base_url, handle) = spawn_wecom_stub_server();
        let provider = WecomDocumentsProvider;
        let credential = credential(&base_url);

        let entries = provider
            .list_entries(
                &credential,
                DocumentsQuery {
                    path: String::new(),
                    limit: 10,
                },
            )
            .expect("list entries");
        assert_eq!(entries[0].path, "Reports");
        assert_eq!(entries[1].path, "Quarterly Plan.txt");

        let document = provider
            .read_document(&credential, "Quarterly Plan.txt", 10_000)
            .expect("read document");
        assert!(document.content.contains("Quarterly plan summary"));
        assert_eq!(document.entry.kind, "txt");

        let hits = provider
            .search_documents(
                &credential,
                DocumentsSearchQuery {
                    path: String::new(),
                    query: "calendar".to_string(),
                    limit: 10,
                    case_sensitive: false,
                    max_read_bytes: 32 * 1024,
                },
            )
            .expect("search documents");
        assert_eq!(hits[0].entry.path, "Reports/Q1 Review.txt");
        assert_eq!(hits[0].match_kind, "content");
        assert!(hits[0]
            .snippet
            .as_deref()
            .expect("snippet")
            .contains("Calendar bridge shipped"));

        handle.join().expect("stub exits");
    }

    #[test]
    fn validate_wecom_documents_credential_rejects_missing_space_id() {
        let error = validate_wecom_documents_credential(&DocumentsProviderCredential {
            space_id: String::new(),
            ..credential(crate::office::WECOM_DEFAULT_BASE_URL)
        })
        .expect_err("missing space id");
        assert_eq!(error.stage(), "wecom_documents_provider");
    }
}
