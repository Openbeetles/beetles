#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::documents::credentials::documents_credential_from_office;
use crate::documents::{
    build_search_snippet, contains_query_text, decode_readable_document,
    decode_searchable_document_text, DocumentsEntry, DocumentsOperation, DocumentsProvider,
    DocumentsProviderCredential, DocumentsQuery, DocumentsReadResult, DocumentsSearchHit,
    DocumentsSearchQuery,
};
use crate::error::{Error, Result};
use crate::office::{OfficeAccount, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult};
use base64::Engine;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::VecDeque;
use std::io::Read;

const MAX_SEARCH_SCAN_ENTRIES: usize = 64;
const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;

pub struct WebDavProvider;

impl DocumentsProvider for WebDavProvider {
    fn provider_name(&self) -> &'static str {
        "webdav"
    }

    fn display_name(&self) -> &'static str {
        "WebDAV"
    }

    fn supports(&self, _op: DocumentsOperation) -> bool {
        true
    }

    fn list_entries(
        &self,
        credential: &DocumentsProviderCredential,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        validate_webdav_credential(credential)?;
        let mut entries = propfind_entries(credential, &query.path, 1)?;
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
        validate_webdav_credential(credential)?;
        let normalized_path = normalize_relative_path(path);
        if normalized_path.is_empty() {
            return Err(Error::config(
                "webdav_provider",
                "read path must not be empty",
            ));
        }
        let raw = http_get_bytes(credential, &normalized_path)?;
        let decoded = decode_readable_document(
            &normalized_path,
            &raw,
            max_chars.max(1),
            "webdav_read_document",
        )?;
        Ok(DocumentsReadResult {
            entry: DocumentsEntry {
                path: normalized_path.clone(),
                name: file_name_from_path(&normalized_path),
                kind: decoded.kind,
                is_dir: false,
                content_type: None,
                size_bytes: Some(raw.len() as u64),
            },
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
        validate_webdav_credential(credential)?;
        let mut queue = VecDeque::from([normalize_relative_path(&query.path)]);
        let mut hits = Vec::new();
        let mut scanned_entries = 0usize;
        let max_read_bytes = if query.max_read_bytes == 0 {
            MAX_SEARCH_READ_BYTES
        } else {
            query.max_read_bytes.min(MAX_SEARCH_READ_BYTES)
        };

        while let Some(dir) = queue.pop_front() {
            if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                break;
            }
            let entries = propfind_entries(credential, &dir, 1)?;
            for entry in entries {
                scanned_entries += 1;
                let path_hit = contains_query_text(&entry.path, &query.query, query.case_sensitive)
                    || contains_query_text(&entry.name, &query.query, query.case_sensitive);
                if entry.is_dir {
                    if path_hit {
                        hits.push(DocumentsSearchHit {
                            entry: entry.clone(),
                            match_kind: "path".to_string(),
                            snippet: None,
                            warning: None,
                        });
                    }
                    queue.push_back(entry.path.clone());
                    if scanned_entries >= MAX_SEARCH_SCAN_ENTRIES || hits.len() >= query.limit {
                        break;
                    }
                    continue;
                }

                let mut content_match = None;
                let mut warning = None;
                if entry.size_bytes.unwrap_or(0) as usize <= max_read_bytes {
                    let raw = http_get_bytes(credential, &entry.path)?;
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

pub struct WebDavOfficeProbeAdapter;

impl OfficeProbeAdapter for WebDavOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "webdav"
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
        if validate_webdav_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "documents_transport_config_missing".to_string(),
            });
        }
        propfind_entries(&adapted, "", 0)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "webdav_propfind_ok".to_string(),
        })
    }
}

fn validate_webdav_credential(credential: &DocumentsProviderCredential) -> Result<()> {
    if credential.username.trim().is_empty() {
        return Err(Error::config(
            "webdav_provider",
            "username must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config("webdav_provider", "secret must not be empty"));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "webdav_provider",
            "base_url must not be empty",
        ));
    }
    Ok(())
}

fn propfind_entries(
    credential: &DocumentsProviderCredential,
    path: &str,
    depth: u8,
) -> Result<Vec<DocumentsEntry>> {
    let target_path = normalize_relative_path(path);
    let url = build_request_url(credential, &target_path, true);
    let request_body = r#"<?xml version="1.0" encoding="utf-8"?><propfind xmlns="DAV:"><prop><displayname/><getcontentlength/><getcontenttype/><resourcetype/></prop></propfind>"#;
    let response = ureq::request("PROPFIND", &url)
        .set("Depth", &depth.to_string())
        .set("Content-Type", "application/xml; charset=utf-8")
        .set("Authorization", &basic_auth_header(credential))
        .send_string(request_body);
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _response)) => {
            return Err(Error::config(
                "webdav_propfind",
                format!("propfind failed with status {}", status),
            ))
        }
        Err(error) => {
            return Err(Error::config("webdav_propfind", error.to_string()));
        }
    };
    let status = response.status();
    if status != 207 && !(200..300).contains(&status) {
        return Err(Error::config(
            "webdav_propfind",
            format!("propfind failed with status {}", status),
        ));
    }
    let body = response
        .into_string()
        .map_err(|error| Error::config("webdav_propfind_read", error.to_string()))?;
    let request_directory = normalize_relative_path(path);
    parse_propfind_response(credential, &request_directory, &body)
}

fn http_get_bytes(credential: &DocumentsProviderCredential, path: &str) -> Result<Vec<u8>> {
    let url = build_request_url(credential, path, false);
    let response = ureq::get(&url)
        .set("Authorization", &basic_auth_header(credential))
        .call();
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(status, _response)) => {
            return Err(Error::config(
                "webdav_get",
                format!("get failed with status {}", status),
            ))
        }
        Err(error) => return Err(Error::config("webdav_get", error.to_string())),
    };
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| Error::config("webdav_get_read", error.to_string()))?;
    Ok(bytes)
}

fn basic_auth_header(credential: &DocumentsProviderCredential) -> String {
    let token = format!("{}:{}", credential.username, credential.secret);
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(token)
    )
}

fn build_request_url(
    credential: &DocumentsProviderCredential,
    relative_path: &str,
    prefer_directory: bool,
) -> String {
    let mut segments = Vec::new();
    if !credential.root_path.trim().is_empty() {
        segments.extend(
            credential
                .root_path
                .trim_matches('/')
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string),
        );
    }
    if !relative_path.trim().is_empty() {
        segments.extend(
            relative_path
                .trim_matches('/')
                .split('/')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string),
        );
    }
    let encoded_path = if segments.is_empty() {
        String::new()
    } else {
        segments
            .into_iter()
            .map(|segment| urlencoding::encode(&segment).into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    let mut url = credential.base_url.trim_end_matches('/').to_string();
    if !encoded_path.is_empty() {
        url.push('/');
        url.push_str(&encoded_path);
    }
    if prefer_directory && !url.ends_with('/') {
        url.push('/');
    }
    url
}

fn parse_propfind_response(
    credential: &DocumentsProviderCredential,
    request_directory: &str,
    xml: &str,
) -> Result<Vec<DocumentsEntry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut current = RawResponse::default();
    let mut responses = Vec::new();
    let mut tag_stack: Vec<String> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = local_name(event.name().as_ref()).to_string();
                if name == "response" {
                    current = RawResponse::default();
                } else if name == "collection" {
                    current.is_dir = true;
                }
                tag_stack.push(name);
            }
            Ok(Event::Empty(event)) => {
                if local_name(event.name().as_ref()) == "collection" {
                    current.is_dir = true;
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(tag) = tag_stack.last() {
                    let value = text
                        .decode()
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    match tag.as_str() {
                        "href" => current.href = value,
                        "displayname" => current.display_name = value,
                        "getcontentlength" => current.size_bytes = value.parse::<u64>().ok(),
                        "getcontenttype" => {
                            current.content_type = (!value.trim().is_empty()).then_some(value)
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let name = local_name(event.name().as_ref()).to_string();
                if name == "response" {
                    responses.push(current.clone());
                    current = RawResponse::default();
                }
                tag_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(Error::config("webdav_propfind_parse", error.to_string())),
            _ => {}
        }
        buf.clear();
    }

    let mut entries = Vec::new();
    for response in responses {
        let Some(path) = relative_path_from_href(credential, &response.href) else {
            continue;
        };
        if path == request_directory || (path.is_empty() && request_directory.is_empty()) {
            continue;
        }
        let normalized_path = normalize_relative_path(&path);
        entries.push(DocumentsEntry {
            name: if response.display_name.trim().is_empty() {
                file_name_from_path(&normalized_path)
            } else {
                response.display_name
            },
            kind: if response.is_dir {
                "directory".to_string()
            } else if let Some(content_type) = response.content_type.as_ref() {
                if content_type.contains("json") {
                    "json".to_string()
                } else if content_type.contains("html") {
                    "html".to_string()
                } else if content_type.contains("pdf") {
                    "pdf".to_string()
                } else {
                    "file".to_string()
                }
            } else {
                "file".to_string()
            },
            path: normalized_path,
            is_dir: response.is_dir,
            content_type: response.content_type,
            size_bytes: response.size_bytes,
        });
    }
    Ok(entries)
}

fn relative_path_from_href(credential: &DocumentsProviderCredential, href: &str) -> Option<String> {
    let href_path = strip_url_origin(href);
    let base_path = strip_url_origin(&credential.base_url);
    let mut relative = href_path
        .strip_prefix(base_path.as_str())
        .unwrap_or(&href_path);
    let root_prefix = normalize_root_prefix(&credential.root_path);
    if !root_prefix.is_empty() {
        relative = relative
            .strip_prefix(root_prefix.as_str())
            .unwrap_or(relative);
    }
    let trimmed = relative.trim_matches('/');
    Some(
        urlencoding::decode(trimmed)
            .unwrap_or(std::borrow::Cow::Borrowed(trimmed))
            .into_owned(),
    )
}

fn normalize_root_prefix(root_path: &str) -> String {
    if root_path.trim().is_empty() {
        String::new()
    } else {
        format!("/{}", root_path.trim_matches('/'))
    }
}

fn normalize_relative_path(path: &str) -> String {
    path.trim().trim_matches('/').to_string()
}

fn strip_url_origin(url: &str) -> String {
    let without_origin = if let Some(scheme_pos) = url.find("://") {
        let rest = &url[scheme_pos + 3..];
        match rest.find('/') {
            Some(path_pos) => &rest[path_pos..],
            None => "/",
        }
    } else {
        url
    };
    without_origin.trim_end_matches('/').to_string()
}

fn file_name_from_path(path: &str) -> String {
    path.rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn local_name(name: &[u8]) -> &str {
    let raw = std::str::from_utf8(name).unwrap_or_default();
    raw.rsplit(':').next().unwrap_or(raw)
}

fn search_score(match_kind: &str) -> u8 {
    match match_kind {
        "path+content" => 3,
        "path" => 2,
        _ => 1,
    }
}

#[derive(Clone, Debug, Default)]
struct RawResponse {
    href: String,
    display_name: String,
    is_dir: bool,
    content_type: Option<String>,
    size_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> DocumentsProviderCredential {
        DocumentsProviderCredential {
            account_key: "docs".to_string(),
            provider: "webdav".to_string(),
            account_id: "user".to_string(),
            account_label: "Docs".to_string(),
            username: "user".to_string(),
            secret: "secret".to_string(),
            app_id: String::new(),
            base_url: "https://dav.example.com/remote.php/dav/files/user".to_string(),
            root_path: "/Workspace".to_string(),
        }
    }

    #[test]
    fn validate_webdav_credential_accepts_complete_transport_shape() {
        validate_webdav_credential(&credential()).expect("valid credential");
    }

    #[test]
    fn build_request_url_joins_root_and_relative_path() {
        let url = build_request_url(&credential(), "Reports/Quarter 1.txt", false);
        assert_eq!(
            url,
            "https://dav.example.com/remote.php/dav/files/user/Workspace/Reports/Quarter%201.txt"
        );
    }

    #[test]
    fn parse_propfind_response_maps_entries() {
        let xml = r#"<?xml version="1.0"?>
        <d:multistatus xmlns:d="DAV:">
          <d:response>
            <d:href>/remote.php/dav/files/user/Workspace/Reports/</d:href>
            <d:propstat><d:prop><d:displayname>Reports</d:displayname><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
          </d:response>
          <d:response>
            <d:href>/remote.php/dav/files/user/Workspace/Reports/report.txt</d:href>
            <d:propstat><d:prop><d:displayname>report.txt</d:displayname><d:getcontentlength>12</d:getcontentlength><d:getcontenttype>text/plain</d:getcontenttype></d:prop></d:propstat>
          </d:response>
        </d:multistatus>"#;
        let entries = parse_propfind_response(&credential(), "", xml).expect("parse xml");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.path == "Reports"));
        assert!(entries
            .iter()
            .any(|entry| entry.path == "Reports/report.txt"));
    }

    #[test]
    fn probe_adapter_reports_missing_transport_shape_before_network() {
        let account = OfficeAccount {
            account_key: "docs".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "user".to_string(),
            account_label: "Docs".to_string(),
            identity_class: crate::office::OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![crate::office::OfficeCapability::Documents],
        };
        let credential = crate::office::OfficeCredential {
            account_key: "docs".to_string(),
            access_token: "secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let result = WebDavOfficeProbeAdapter
            .probe(&account, &credential)
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
    }
}
