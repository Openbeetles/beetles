#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::error::{Error, Result};
use crate::mail::credentials::mail_credential_from_office;
use crate::mail::DEFAULT_MAILBOX;
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailQuery, MailSearchQuery, MailSendRequest,
};
use crate::office::{
    build_google_api_url, request_google_api_json, OfficeHttpClient, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult,
};
use crate::util::current_unix_secs;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

const GOOGLE_MAIL_DEFAULT_MAILBOX: &str = "INBOX";
const GOOGLE_MAIL_DRAFT_MAILBOX: &str = "DRAFT";

pub struct GoogleMailProvider;

impl MailProvider for GoogleMailProvider {
    fn provider_name(&self) -> &'static str {
        "google_mail"
    }

    fn display_name(&self) -> &'static str {
        "Google Mail"
    }

    fn supports(&self, op: MailOperation) -> bool {
        matches!(
            op,
            MailOperation::List
                | MailOperation::Search
                | MailOperation::Get
                | MailOperation::Send
                | MailOperation::Draft
        )
    }

    fn list_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let client = GoogleMailClient::new(credential)?;
        client.list_messages(http, &query)
    }

    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let client = GoogleMailClient::new(credential)?;
        client.search_messages(http, &query)
    }

    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let client = GoogleMailClient::new(credential)?;
        client.get_message(http, id)
    }

    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let client = GoogleMailClient::new(credential)?;
        client.send_message(http, request)
    }

    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let client = GoogleMailClient::new(credential)?;
        client.draft_message(http, request)
    }
}

pub struct GoogleMailOfficeProbeAdapter;

impl OfficeProbeAdapter for GoogleMailOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "google_mail"
    }

    fn probe(
        &self,
        http: &mut dyn OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = match mail_credential_from_office(account.clone(), credential.clone()) {
            Ok(adapted) => adapted,
            Err(_) => {
                return Ok(OfficeProbeResult {
                    account_key: account.account_key.clone(),
                    provider_kind: account.provider_kind.clone(),
                    configured: false,
                    disposition: OfficeProbeDisposition::MissingCredential,
                    reason: "mail_transport_config_missing".to_string(),
                });
            }
        };
        if validate_google_mail_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "mail_transport_config_missing".to_string(),
            });
        }
        let client = GoogleMailClient::new(&adapted)?;
        client.fetch_sender_profile(http)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "google_mail_profile_ok".to_string(),
        })
    }
}

struct GoogleMailClient<'a> {
    credential: &'a MailProviderCredential,
}

impl<'a> GoogleMailClient<'a> {
    fn new(credential: &'a MailProviderCredential) -> Result<Self> {
        validate_google_mail_credential(credential)?;
        Ok(Self { credential })
    }

    fn list_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let q = render_mail_query(
            query.mailbox.as_str(),
            query.unread_only,
            query.received_after_unix_secs,
        );
        let auth = self.auth_header();
        let payload: GoogleMessagesList = request_google_api_json(
            http,
            "google_mail_list",
            "GET",
            &self.endpoint(
                "/users/me/messages",
                &[
                    ("maxResults", query.limit.clamp(1, 50).to_string()),
                    ("q", q),
                ],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        self.hydrate_summaries(
            http,
            payload.messages,
            normalized_mailbox(query.mailbox.as_str()),
        )
    }

    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let scoped_query = render_search_query(query);
        let auth = self.auth_header();
        let payload: GoogleMessagesList = request_google_api_json(
            http,
            "google_mail_search",
            "GET",
            &self.endpoint(
                "/users/me/messages",
                &[
                    ("maxResults", query.limit.clamp(1, 50).to_string()),
                    ("q", scoped_query),
                ],
            ),
            &[("Authorization", auth.as_str())],
            None,
        )?;
        self.hydrate_summaries(
            http,
            payload.messages,
            normalized_mailbox(query.mailbox.as_str()),
        )
    }

    fn hydrate_summaries(
        &self,
        http: &mut dyn OfficeHttpClient,
        messages: Vec<GoogleMessageRef>,
        mailbox: &str,
    ) -> Result<Vec<MailMessageSummary>> {
        let mut items = Vec::new();
        for message in messages {
            if let Some(full) = self.fetch_message(http, &message.id, false)? {
                items.push(full.into_summary(self.credential, mailbox));
            }
        }
        Ok(items)
    }

    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        self.fetch_message(http, id, true)
            .map(|item| item.map(|message| message.into_message(self.credential, DEFAULT_MAILBOX)))
    }

    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let raw = render_raw_message(self.credential, request)?;
        let auth = self.auth_header();
        let body = json!({ "raw": raw }).to_string();
        let response: GoogleMessageSent = request_google_api_json(
            http,
            "google_mail_send",
            "POST",
            &self.endpoint("/users/me/messages/send", &[]),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        )?;
        self.build_mutation_summary(http, response.id, request, false)
    }

    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let raw = render_raw_message(self.credential, request)?;
        let auth = self.auth_header();
        let body = json!({ "message": { "raw": raw } }).to_string();
        let response: GoogleDraftCreated = request_google_api_json(
            http,
            "google_mail_draft",
            "POST",
            &self.endpoint("/users/me/drafts", &[]),
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        )?;
        self.build_mutation_summary(http, response.message.id, request, true)
    }

    fn build_mutation_summary(
        &self,
        http: &mut dyn OfficeHttpClient,
        id: String,
        request: &MailSendRequest,
        is_draft: bool,
    ) -> Result<MailMessageSummary> {
        let sender = self.fetch_sender_profile(http).unwrap_or_default();
        Ok(MailMessageSummary {
            id,
            provider: self.credential.provider.clone(),
            account_key: self.credential.account_key.clone(),
            mailbox: if is_draft {
                GOOGLE_MAIL_DRAFT_MAILBOX.to_string()
            } else {
                GOOGLE_MAIL_DEFAULT_MAILBOX.to_string()
            },
            subject: request.subject.trim().to_string(),
            from: sender.email,
            to: request.to.clone(),
            preview: request.text_body.chars().take(160).collect(),
            unread: false,
            received_at_unix_secs: current_unix_secs(),
        })
    }

    fn fetch_sender_profile(&self, http: &mut dyn OfficeHttpClient) -> Result<GoogleProfile> {
        let auth = self.auth_header();
        request_google_api_json(
            http,
            "google_mail_profile",
            "GET",
            &self.endpoint("/users/me/profile", &[]),
            &[("Authorization", auth.as_str())],
            None,
        )
    }

    fn fetch_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        id: &str,
        include_body: bool,
    ) -> Result<Option<GoogleMessage>> {
        let format = if include_body { "full" } else { "metadata" };
        let auth = self.auth_header();
        let (status, body) = http.get_with_headers(
            &self.endpoint(
                &format!("/users/me/messages/{}", urlencoding::encode(id)),
                &[
                    ("format", format.to_string()),
                    (
                        "metadataHeaders",
                        "Subject,From,To,Cc,Bcc,Date,Message-ID,In-Reply-To,References".to_string(),
                    ),
                ],
            ),
            &[("Authorization", auth.as_str())],
        )?;
        match status {
            404 => Ok(None),
            200..=299 => {
                crate::office::parse_google_api_json("google_mail_get", status, body).map(Some)
            }
            _ => crate::office::parse_google_api_json::<serde_json::Value>(
                "google_mail_get",
                status,
                body,
            )
            .map(|_| None),
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
struct GoogleMessagesList {
    #[serde(default)]
    messages: Vec<GoogleMessageRef>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleMessageRef {
    #[serde(default)]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleDraftCreated {
    #[serde(default)]
    message: GoogleMessageRef,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleMessageSent {
    #[serde(default)]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleProfile {
    #[serde(default, rename = "emailAddress")]
    email: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleMessage {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "internalDate")]
    internal_date: String,
    #[serde(default, rename = "labelIds")]
    label_ids: Vec<String>,
    #[serde(default)]
    snippet: String,
    #[serde(default)]
    payload: GoogleMessagePayload,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleMessagePayload {
    #[serde(default)]
    headers: Vec<GoogleMessageHeader>,
    #[serde(default)]
    body: GoogleBodyData,
    #[serde(default)]
    parts: Vec<GoogleMessagePayload>,
    #[serde(default, rename = "mimeType")]
    mime_type: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleMessageHeader {
    #[serde(default)]
    name: String,
    #[serde(default)]
    value: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleBodyData {
    #[serde(default)]
    data: String,
}

impl GoogleMessage {
    fn into_summary(
        self,
        credential: &MailProviderCredential,
        mailbox: &str,
    ) -> MailMessageSummary {
        MailMessageSummary {
            id: self.id,
            provider: credential.provider.clone(),
            account_key: credential.account_key.clone(),
            mailbox: mailbox.to_string(),
            subject: header_value(&self.payload.headers, "Subject"),
            from: header_value(&self.payload.headers, "From"),
            to: split_recipients(&header_value(&self.payload.headers, "To")),
            preview: self.snippet,
            unread: self.label_ids.iter().any(|item| item == "UNREAD"),
            received_at_unix_secs: self.internal_date.parse::<u64>().unwrap_or_default() / 1000,
        }
    }

    fn into_message(self, credential: &MailProviderCredential, mailbox: &str) -> MailMessage {
        let message_id = header_value(&self.payload.headers, "Message-ID");
        let reply_to = split_recipients(&header_value(&self.payload.headers, "In-Reply-To"));
        let references = header_value(&self.payload.headers, "References");
        let text_body = extract_text_body(&self.payload).unwrap_or_default();
        MailMessage {
            summary: self.into_summary(credential, mailbox),
            text_body,
            message_id,
            reply_to,
            references,
        }
    }
}

fn validate_google_mail_credential(credential: &MailProviderCredential) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "google_mail_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "google_mail_provider",
            "mail_base_url must not be empty",
        ));
    }
    Ok(())
}

fn normalized_mailbox(mailbox: &str) -> &str {
    if mailbox.trim().is_empty() {
        GOOGLE_MAIL_DEFAULT_MAILBOX
    } else {
        mailbox.trim()
    }
}

fn render_mail_query(
    mailbox: &str,
    unread_only: bool,
    received_after_unix_secs: Option<u64>,
) -> String {
    let mut parts = Vec::new();
    let mailbox = mailbox.trim();
    if !mailbox.is_empty() {
        match mailbox.to_ascii_lowercase().as_str() {
            "inbox" => parts.push("in:inbox".to_string()),
            "draft" | "drafts" => parts.push("in:drafts".to_string()),
            other => parts.push(format!("label:{other}")),
        }
    }
    if unread_only {
        parts.push("is:unread".to_string());
    }
    if let Some(unix_secs) = received_after_unix_secs {
        parts.push(format!("after:{unix_secs}"));
    }
    parts.join(" ")
}

fn render_search_query(query: &MailSearchQuery) -> String {
    let mut parts = Vec::new();
    if !query.query.trim().is_empty() {
        parts.push(query.query.trim().to_string());
    }
    let scoped = render_mail_query(
        query.mailbox.as_str(),
        query.unread_only,
        query.received_after_unix_secs,
    );
    if !scoped.is_empty() {
        parts.push(scoped);
    }
    parts.join(" ")
}

fn render_raw_message(
    credential: &MailProviderCredential,
    request: &MailSendRequest,
) -> Result<String> {
    let from_address = credential
        .from_address
        .trim()
        .if_empty_then(|| credential.account_id.trim())
        .ok_or_else(|| Error::config("google_mail_send", "from address must not be empty"))?;
    let mut lines = vec![
        format!(
            "From: {}",
            render_mailbox(from_address, credential.from_name.trim())
        ),
        format!("To: {}", request.to.join(", ")),
    ];
    if !request.cc.is_empty() {
        lines.push(format!("Cc: {}", request.cc.join(", ")));
    }
    if !request.bcc.is_empty() {
        lines.push(format!("Bcc: {}", request.bcc.join(", ")));
    }
    lines.push(format!("Subject: {}", request.subject.trim()));
    lines.push("MIME-Version: 1.0".to_string());
    lines.push("Content-Type: text/plain; charset=UTF-8".to_string());
    if !request.in_reply_to.trim().is_empty() {
        lines.push(format!("In-Reply-To: {}", request.in_reply_to.trim()));
    }
    if !request.references.trim().is_empty() {
        lines.push(format!("References: {}", request.references.trim()));
    }
    lines.push(String::new());
    lines.push(request.text_body.clone());
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(lines.join("\r\n").as_bytes()))
}

fn render_mailbox(address: &str, name: &str) -> String {
    if name.trim().is_empty() {
        address.to_string()
    } else {
        format!("{} <{}>", name.trim(), address)
    }
}

fn header_value(headers: &[GoogleMessageHeader], name: &str) -> String {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .map(|header| header.value.trim().to_string())
        .unwrap_or_default()
}

fn split_recipients(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn decode_base64url(data: &str) -> Option<Vec<u8>> {
    if data.trim().is_empty() {
        return None;
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.as_bytes())
        .ok()
}

fn extract_text_body(payload: &GoogleMessagePayload) -> Option<String> {
    if payload.mime_type.eq_ignore_ascii_case("text/plain") || payload.parts.is_empty() {
        if let Some(bytes) = decode_base64url(&payload.body.data) {
            if let Ok(text) = String::from_utf8(bytes) {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }
    payload.parts.iter().find_map(extract_text_body)
}

trait IfEmptyThen<'a> {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str>;
}

impl<'a> IfEmptyThen<'a> for &'a str {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str> {
        if self.trim().is_empty() {
            let value = fallback();
            (!value.trim().is_empty()).then_some(value)
        } else {
            Some(self)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeCapability, OfficeCredential,
    };

    #[test]
    fn google_mail_provider_reports_gmail_capabilities() {
        let provider = GoogleMailProvider;
        assert_eq!(provider.provider_name(), "google_mail");
        assert!(provider.supports(MailOperation::Send));
        assert!(provider.supports(MailOperation::Draft));
    }

    #[test]
    fn google_mail_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = GoogleMailOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "google-mail".to_string(),
                    provider_kind: "google_mail".to_string(),
                    external_account_id: "alice@gmail.com".to_string(),
                    account_label: "Google Mail".to_string(),
                    identity_class: OfficeAccountIdentityClass::Personal,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
                &OfficeCredential {
                    account_key: "google-mail".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(result.provider_kind, "google_mail");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
    }
}
