#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::error::{Error, Result};
use crate::mail::credentials::mail_credential_from_office;
use crate::mail::DEFAULT_MAILBOX;
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailQuery, MailSearchQuery, MailSendRequest,
};
use crate::office::{
    build_microsoft_graph_url, request_microsoft_graph_empty, request_microsoft_graph_json,
    OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use crate::util::{current_unix_secs, epoch_to_ymdhms, parse_iso8601};
use serde::Deserialize;
use serde_json::json;

const MICROSOFT_MAIL_DEFAULT_FOLDER: &str = "inbox";

pub struct Microsoft365MailProvider;

impl MailProvider for Microsoft365MailProvider {
    fn provider_name(&self) -> &'static str {
        "microsoft365_mail"
    }

    fn display_name(&self) -> &'static str {
        "Microsoft 365 Mail"
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
        let client = Microsoft365MailClient::new(credential)?;
        client.list_messages(http, &query)
    }

    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let client = Microsoft365MailClient::new(credential)?;
        client.search_messages(http, &query)
    }

    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let client = Microsoft365MailClient::new(credential)?;
        client.get_message(http, id)
    }

    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let client = Microsoft365MailClient::new(credential)?;
        client.send_message(http, request)
    }

    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let client = Microsoft365MailClient::new(credential)?;
        client.draft_message(http, request)
    }
}

pub struct Microsoft365MailOfficeProbeAdapter;

impl OfficeProbeAdapter for Microsoft365MailOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "microsoft365_mail"
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
                })
            }
        };
        if validate_microsoft365_mail_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "mail_transport_config_missing".to_string(),
            });
        }
        let client = Microsoft365MailClient::new(&adapted)?;
        client.fetch_sender_profile(http)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "microsoft365_mail_profile_ok".to_string(),
        })
    }
}

struct Microsoft365MailClient<'a> {
    credential: &'a MailProviderCredential,
}

impl<'a> Microsoft365MailClient<'a> {
    fn new(credential: &'a MailProviderCredential) -> Result<Self> {
        validate_microsoft365_mail_credential(credential)?;
        Ok(Self { credential })
    }

    fn list_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let endpoint = mailbox_messages_endpoint(&query.mailbox);
        let mut query_pairs = vec![
            ("$top", query.limit.clamp(1, 50).to_string()),
            (
                "$select",
                "id,subject,from,toRecipients,bodyPreview,isRead,receivedDateTime,internetMessageId"
                    .to_string(),
            ),
            ("$orderby", "receivedDateTime desc".to_string()),
        ];
        if let Some(filter) = message_filter(query.unread_only, query.received_after_unix_secs) {
            query_pairs.push(("$filter", filter));
        }
        let url = self.endpoint(&endpoint, &query_pairs);
        let auth = self.auth_header();
        let payload: MicrosoftGraphMailCollection = request_microsoft_graph_json(
            http,
            "microsoft365_mail_list",
            "GET",
            &url,
            &[("Authorization", auth.as_str())],
            None,
        )?;
        payload
            .value
            .into_iter()
            .map(|item| Ok(item.into_summary(self.credential, normalized_mailbox(&query.mailbox))))
            .collect()
    }

    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let endpoint = mailbox_messages_endpoint(&query.mailbox);
        let url = self.endpoint(
            &endpoint,
            &[
                ("$top", query.limit.clamp(1, 50).to_string()),
                (
                    "$select",
                    "id,subject,from,toRecipients,bodyPreview,isRead,receivedDateTime,internetMessageId"
                        .to_string(),
                ),
                ("$search", format!("\"{}\"", query.query.trim())),
            ],
        );
        let auth = self.auth_header();
        let mut payload: MicrosoftGraphMailCollection = request_microsoft_graph_json(
            http,
            "microsoft365_mail_search",
            "GET",
            &url,
            &[
                ("Authorization", auth.as_str()),
                ("ConsistencyLevel", "eventual"),
            ],
            None,
        )?;
        let mailbox = normalized_mailbox(&query.mailbox);
        let mut items = payload
            .value
            .drain(..)
            .map(|item| item.into_summary(self.credential, mailbox))
            .filter(|summary| {
                (!query.unread_only || summary.unread)
                    && query
                        .received_after_unix_secs
                        .map(|min| summary.received_at_unix_secs >= min)
                        .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .received_at_unix_secs
                .cmp(&left.received_at_unix_secs)
                .then_with(|| left.id.cmp(&right.id))
        });
        if items.len() > query.limit {
            items.truncate(query.limit);
        }
        Ok(items)
    }

    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let url = self.endpoint(
            &format!("/me/messages/{}", urlencoding::encode(id)),
            &[(
                "$select",
                "id,subject,from,toRecipients,ccRecipients,bccRecipients,replyTo,body,bodyPreview,isRead,receivedDateTime,internetMessageId,conversationId".to_string(),
            )],
        );
        let auth = self.auth_header();
        let (status, body) = http.get_with_headers(&url, &[("Authorization", auth.as_str())])?;
        match status {
            404 => Ok(None),
            200..=299 => {
                let item: MicrosoftGraphMessage = crate::office::parse_microsoft_graph_json(
                    "microsoft365_mail_get",
                    status,
                    body,
                )?;
                Ok(Some(item.into_message(self.credential, DEFAULT_MAILBOX)?))
            }
            _ => crate::office::parse_microsoft_graph_json::<serde_json::Value>(
                "microsoft365_mail_get",
                status,
                body,
            )
            .map(|_| None),
        }
    }

    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let url = self.endpoint("/me/sendMail", &[]);
        let body = render_send_body(request).to_string();
        let auth = self.auth_header();
        request_microsoft_graph_empty(
            http,
            "microsoft365_mail_send",
            "POST",
            &url,
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        )?;
        let sender = self.fetch_sender_profile(http).unwrap_or_default();
        Ok(MailMessageSummary {
            id: format!("microsoft365-mail-{}", current_unix_secs()),
            provider: self.credential.provider.clone(),
            account_key: self.credential.account_key.clone(),
            mailbox: "sent".to_string(),
            subject: request.subject.clone(),
            from: sender.email,
            to: request.to.clone(),
            preview: render_mail_preview(&request.text_body),
            unread: false,
            received_at_unix_secs: current_unix_secs(),
        })
    }

    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let url = self.endpoint("/me/messages", &[]);
        let body = render_draft_body(request).to_string();
        let auth = self.auth_header();
        let item: MicrosoftGraphMessage = request_microsoft_graph_json(
            http,
            "microsoft365_mail_draft",
            "POST",
            &url,
            &[
                ("Authorization", auth.as_str()),
                ("Content-Type", "application/json"),
            ],
            Some(body.as_bytes()),
        )?;
        Ok(item.into_summary(self.credential, "Drafts"))
    }

    fn fetch_sender_profile(
        &self,
        http: &mut dyn OfficeHttpClient,
    ) -> Result<MicrosoftGraphSenderProfile> {
        let url = self.endpoint("/me", &[("$select", "mail,userPrincipalName".to_string())]);
        let auth = self.auth_header();
        let profile: MicrosoftGraphUserProfile = request_microsoft_graph_json(
            http,
            "microsoft365_mail_profile",
            "GET",
            &url,
            &[("Authorization", auth.as_str())],
            None,
        )?;
        Ok(MicrosoftGraphSenderProfile {
            email: profile
                .mail
                .trim()
                .if_empty_then(|| profile.user_principal_name.trim())
                .unwrap_or(self.credential.from_address.trim())
                .to_string(),
        })
    }

    fn endpoint(&self, path: &str, query: &[(&str, String)]) -> String {
        build_microsoft_graph_url(&self.credential.base_url, path, query)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.credential.secret)
    }
}

#[derive(Default)]
struct MicrosoftGraphSenderProfile {
    email: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphUserProfile {
    #[serde(default)]
    mail: String,
    #[serde(default, rename = "userPrincipalName")]
    user_principal_name: String,
}

type MicrosoftGraphMailCollection = crate::office::MicrosoftGraphCollection<MicrosoftGraphMessage>;

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    from: Option<MicrosoftGraphRecipientWrapper>,
    #[serde(default, rename = "toRecipients")]
    to_recipients: Vec<MicrosoftGraphRecipientWrapper>,
    #[serde(default, rename = "replyTo")]
    reply_to: Vec<MicrosoftGraphRecipientWrapper>,
    #[serde(default, rename = "bodyPreview")]
    body_preview: String,
    #[serde(default)]
    body: Option<MicrosoftGraphItemBody>,
    #[serde(default, rename = "isRead")]
    is_read: bool,
    #[serde(default, rename = "receivedDateTime")]
    received_date_time: String,
    #[serde(default, rename = "internetMessageId")]
    internet_message_id: String,
    #[serde(default, rename = "conversationId")]
    conversation_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphItemBody {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphRecipientWrapper {
    #[serde(default, rename = "emailAddress")]
    email_address: MicrosoftGraphEmailAddress,
}

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphEmailAddress {
    #[serde(default)]
    address: String,
}

impl MicrosoftGraphMessage {
    fn into_summary(
        self,
        credential: &MailProviderCredential,
        mailbox: &str,
    ) -> MailMessageSummary {
        let id = self.id.clone();
        let subject = self.subject.trim().to_string();
        let from = self.sender_address();
        let to = self.to_addresses();
        let preview = self.body_preview.trim().to_string();
        let unread = !self.is_read;
        let received_at_unix_secs = parse_iso8601(&self.received_date_time).unwrap_or_default();
        MailMessageSummary {
            id,
            provider: credential.provider.clone(),
            account_key: credential.account_key.clone(),
            mailbox: mailbox.to_string(),
            subject,
            from,
            to,
            preview,
            unread,
            received_at_unix_secs,
        }
    }

    fn into_message(
        self,
        credential: &MailProviderCredential,
        mailbox: &str,
    ) -> Result<MailMessage> {
        let summary = MailMessageSummary {
            id: self.id.clone(),
            provider: credential.provider.clone(),
            account_key: credential.account_key.clone(),
            mailbox: mailbox.to_string(),
            subject: self.subject.trim().to_string(),
            from: self.sender_address(),
            to: self.to_addresses(),
            preview: self.body_preview.trim().to_string(),
            unread: !self.is_read,
            received_at_unix_secs: parse_iso8601(&self.received_date_time).unwrap_or_default(),
        };
        let references = if self.conversation_id.trim().is_empty() {
            String::new()
        } else {
            self.conversation_id.trim().to_string()
        };
        let text_body = self
            .body
            .as_ref()
            .map(|body| body.content.trim().to_string())
            .filter(|content| !content.is_empty())
            .unwrap_or_else(|| self.body_preview.trim().to_string());
        Ok(MailMessage {
            summary,
            text_body,
            message_id: self.internet_message_id.trim().to_string(),
            reply_to: self.reply_to_addresses(),
            references,
        })
    }

    fn sender_address(&self) -> String {
        self.from
            .as_ref()
            .map(|wrapper| wrapper.email_address.address.trim().to_string())
            .unwrap_or_default()
    }

    fn to_addresses(&self) -> Vec<String> {
        recipient_addresses(&self.to_recipients)
    }

    fn reply_to_addresses(&self) -> Vec<String> {
        recipient_addresses(&self.reply_to)
    }
}

fn recipient_addresses(recipients: &[MicrosoftGraphRecipientWrapper]) -> Vec<String> {
    recipients
        .iter()
        .filter_map(|recipient| {
            let address = recipient.email_address.address.trim();
            (!address.is_empty()).then(|| address.to_string())
        })
        .collect()
}

fn validate_microsoft365_mail_credential(credential: &MailProviderCredential) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_mail_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_mail_provider",
            "mail_base_url must not be empty",
        ));
    }
    Ok(())
}

fn mailbox_messages_endpoint(mailbox: &str) -> String {
    let mailbox = normalized_mailbox(mailbox);
    match mailbox.to_ascii_lowercase().as_str() {
        "inbox" => "/me/mailFolders/inbox/messages".to_string(),
        "drafts" => "/me/mailFolders/drafts/messages".to_string(),
        "sent" | "sentitems" => "/me/mailFolders/sentitems/messages".to_string(),
        other => format!("/me/mailFolders/{}/messages", urlencoding::encode(other)),
    }
}

fn normalized_mailbox(mailbox: &str) -> &str {
    let trimmed = mailbox.trim();
    if trimmed.is_empty() {
        MICROSOFT_MAIL_DEFAULT_FOLDER
    } else {
        trimmed
    }
}

fn message_filter(unread_only: bool, received_after_unix_secs: Option<u64>) -> Option<String> {
    let mut filters = Vec::new();
    if unread_only {
        filters.push("isRead eq false".to_string());
    }
    if let Some(received_after_unix_secs) = received_after_unix_secs {
        filters.push(format!(
            "receivedDateTime ge {}",
            render_graph_datetime(received_after_unix_secs)
        ));
    }
    (!filters.is_empty()).then(|| filters.join(" and "))
}

fn render_send_body(request: &MailSendRequest) -> serde_json::Value {
    json!({
        "message": {
            "subject": request.subject,
            "body": {
                "contentType": "Text",
                "content": request.text_body,
            },
            "toRecipients": render_recipients(&request.to),
            "ccRecipients": render_recipients(&request.cc),
            "bccRecipients": render_recipients(&request.bcc),
            "internetMessageHeaders": render_message_headers(request),
        },
        "saveToSentItems": true,
    })
}

fn render_draft_body(request: &MailSendRequest) -> serde_json::Value {
    json!({
        "subject": request.subject,
        "body": {
            "contentType": "Text",
            "content": request.text_body,
        },
        "toRecipients": render_recipients(&request.to),
        "ccRecipients": render_recipients(&request.cc),
        "bccRecipients": render_recipients(&request.bcc),
        "replyTo": [],
        "internetMessageHeaders": render_message_headers(request),
    })
}

fn render_recipients(addresses: &[String]) -> Vec<serde_json::Value> {
    addresses
        .iter()
        .map(|address| {
            json!({
                "emailAddress": {
                    "address": address,
                }
            })
        })
        .collect()
}

fn render_message_headers(request: &MailSendRequest) -> Vec<serde_json::Value> {
    let mut headers = Vec::new();
    if !request.in_reply_to.trim().is_empty() {
        headers.push(json!({
            "name": "In-Reply-To",
            "value": request.in_reply_to,
        }));
    }
    if !request.references.trim().is_empty() {
        headers.push(json!({
            "name": "References",
            "value": request.references,
        }));
    }
    headers
}

fn render_graph_datetime(unix_secs: u64) -> String {
    let (year, month, day, hour, minute, second) = epoch_to_ymdhms(unix_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn render_mail_preview(body: &str) -> String {
    body.trim().chars().take(120).collect::<String>()
}

trait IfEmptyThen<'a> {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str>;
}

impl<'a> IfEmptyThen<'a> for &'a str {
    fn if_empty_then(self, fallback: impl FnOnce() -> &'a str) -> Option<&'a str> {
        if self.trim().is_empty() {
            let fallback = fallback();
            (!fallback.trim().is_empty()).then_some(fallback)
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
    fn microsoft365_mail_provider_reports_graph_capabilities() {
        let provider = Microsoft365MailProvider;
        assert_eq!(provider.provider_name(), "microsoft365_mail");
        assert!(provider.supports(MailOperation::List));
        assert!(provider.supports(MailOperation::Search));
        assert!(provider.supports(MailOperation::Get));
        assert!(provider.supports(MailOperation::Send));
        assert!(provider.supports(MailOperation::Draft));
    }

    #[test]
    fn microsoft365_mail_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = Microsoft365MailOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "mail-ms".to_string(),
                    provider_kind: "microsoft365_mail".to_string(),
                    external_account_id: "alice@contoso.com".to_string(),
                    account_label: "Microsoft Mail".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
                &OfficeCredential {
                    account_key: "mail-ms".to_string(),
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
        assert_eq!(result.provider_kind, "microsoft365_mail");
    }

    #[test]
    fn message_filter_renders_graph_datetime_and_unread_clause() {
        let rendered = message_filter(true, Some(1_700_000_000)).expect("filter");
        assert!(rendered.contains("isRead eq false"));
        assert!(rendered.contains("receivedDateTime ge 2023-11-14T22:13:20Z"));
    }
}
