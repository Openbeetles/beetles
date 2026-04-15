#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::error::{Error, Result};
use crate::mail::credentials::mail_credential_from_office;
use crate::mail::providers::imap_smtp::{
    extract_addresses, extract_first_address, extract_text, render_mail_preview,
};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailQuery, MailSearchQuery, MailSendRequest,
};
use crate::office::{
    fetch_wecom_access_token_ureq, request_wecom_json_ureq, OfficeProbeAdapter,
    OfficeProbeDisposition, OfficeProbeResult, WecomApiEnvelope, WecomAuthCredential,
};
use crate::util::current_unix_secs;
use mail_parser::MessageParser;
use serde::Deserialize;
use serde_json::json;

const WECOM_MAIL_FETCH_LIMIT: usize = 50;
const WECOM_MAIL_LOOKBACK_SECS: u64 = 90 * 24 * 60 * 60;

pub struct WecomMailProvider;

impl MailProvider for WecomMailProvider {
    fn provider_name(&self) -> &'static str {
        "wecom_mail"
    }

    fn display_name(&self) -> &'static str {
        "WeCom Mail"
    }

    fn supports(&self, op: MailOperation) -> bool {
        matches!(
            op,
            MailOperation::List | MailOperation::Search | MailOperation::Get | MailOperation::Send
        )
    }

    fn list_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        if query.unread_only {
            return Err(Error::config(
                "wecom_mail_list",
                "unread_only is not supported by wecom mail",
            ));
        }
        let client = WecomMailClient::new(credential)?;
        client.list_messages(&query)
    }

    fn search_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        if query.unread_only {
            return Err(Error::config(
                "wecom_mail_search",
                "unread_only is not supported by wecom mail",
            ));
        }
        let client = WecomMailClient::new(credential)?;
        client.search_messages(&query)
    }

    fn get_message(
        &self,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let client = WecomMailClient::new(credential)?;
        client.get_message(id)
    }

    fn send_message(
        &self,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let client = WecomMailClient::new(credential)?;
        client.send_message(request)
    }

    fn draft_message(
        &self,
        _credential: &MailProviderCredential,
        _request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        Err(Error::config(
            "wecom_mail_draft",
            "draft is not supported by wecom mail",
        ))
    }
}

pub struct WecomMailOfficeProbeAdapter;

impl OfficeProbeAdapter for WecomMailOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "wecom_mail"
    }

    fn probe(
        &self,
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
        if validate_wecom_mail_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "mail_transport_config_missing".to_string(),
            });
        }
        let client = WecomMailClient::new(&adapted)?;
        client.query_mailbox_address()?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "wecom_mailbox_ok".to_string(),
        })
    }
}

struct WecomMailClient<'a> {
    credential: &'a MailProviderCredential,
    access_token: String,
}

impl<'a> WecomMailClient<'a> {
    fn new(credential: &'a MailProviderCredential) -> Result<Self> {
        validate_wecom_mail_credential(credential)?;
        let access_token = fetch_wecom_access_token_ureq(
            "wecom_mail_auth",
            WecomAuthCredential {
                corp_id: credential.corp_id.as_str(),
                corp_secret: credential.secret.as_str(),
                base_url: credential.base_url.as_str(),
            },
        )?;
        Ok(Self {
            credential,
            access_token,
        })
    }

    fn list_messages(&self, query: &MailQuery) -> Result<Vec<MailMessageSummary>> {
        let ids = self.fetch_mail_ids(query.received_after_unix_secs, query.limit)?;
        ids.into_iter()
            .map(|item| self.read_mail(&item))
            .collect::<Result<Vec<_>>>()
            .map(|items| items.into_iter().map(|item| item.summary).collect())
    }

    fn search_messages(&self, query: &MailSearchQuery) -> Result<Vec<MailMessageSummary>> {
        let ids = self.fetch_mail_ids(query.received_after_unix_secs, query.limit.max(20))?;
        let needle = query.query.trim().to_ascii_lowercase();
        let mut out = Vec::new();
        for id in ids {
            let item = self.read_mail(&id)?;
            let haystack = format!(
                "{}\n{}\n{}\n{}",
                item.summary.subject,
                item.summary.from,
                item.summary.to.join(" "),
                item.text_body
            )
            .to_ascii_lowercase();
            if haystack.contains(&needle) {
                out.push(item.summary);
            }
            if out.len() >= query.limit {
                break;
            }
        }
        Ok(out)
    }

    fn get_message(&self, id: &str) -> Result<Option<MailMessage>> {
        self.read_mail(id).map(Some)
    }

    fn send_message(&self, request: &MailSendRequest) -> Result<MailMessageSummary> {
        let url = self.endpoint("/cgi-bin/exmail/app/compose_send");
        let body = json!({
            "to": {"emails": request.to},
            "cc": {"emails": request.cc},
            "bcc": {"emails": request.bcc},
            "subject": request.subject,
            "content": request.text_body,
            "content_type": "text/plain",
        });
        let payload: WecomApiEnvelope<serde_json::Value> = request_wecom_json_ureq(
            "wecom_mail_send",
            ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
        )?;
        payload.require_ok("wecom_mail_send")?;
        let from = if self.credential.from_address.trim().is_empty() {
            self.query_mailbox_address()?
        } else {
            self.credential.from_address.clone()
        };
        Ok(MailMessageSummary {
            id: format!("wecom-mail-{}", current_unix_secs()),
            provider: self.credential.provider.clone(),
            account_key: self.credential.account_key.clone(),
            mailbox: "sent".to_string(),
            subject: request.subject.clone(),
            from,
            to: request.to.clone(),
            preview: render_mail_preview(&request.text_body),
            unread: false,
            received_at_unix_secs: current_unix_secs(),
        })
    }

    fn fetch_mail_ids(
        &self,
        received_after_unix_secs: Option<u64>,
        limit: usize,
    ) -> Result<Vec<String>> {
        let begin = received_after_unix_secs
            .unwrap_or_else(|| current_unix_secs().saturating_sub(WECOM_MAIL_LOOKBACK_SECS));
        let body = json!({
            "begin_time": begin,
            "end_time": current_unix_secs(),
            "cursor": 0,
            "limit": limit.clamp(1, WECOM_MAIL_FETCH_LIMIT),
        });
        let url = self.endpoint("/cgi-bin/exmail/app/get_mail_list");
        let payload: WecomApiEnvelope<WecomMailListPayload> = request_wecom_json_ureq(
            "wecom_mail_list",
            ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
        )?;
        Ok(payload
            .require_ok("wecom_mail_list")?
            .mail_list
            .into_iter()
            .map(|item| item.mail_id)
            .collect())
    }

    fn read_mail(&self, mail_id: &str) -> Result<MailMessage> {
        let url = self.endpoint("/cgi-bin/exmail/app/read_mail");
        let body = json!({ "mail_id": mail_id });
        let payload: WecomApiEnvelope<WecomReadMailPayload> = request_wecom_json_ureq(
            "wecom_mail_get",
            ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string()),
        )?;
        let data = payload.require_ok("wecom_mail_get")?;
        parse_raw_message(self.credential, mail_id, &data.mail_content)
    }

    fn query_mailbox_address(&self) -> Result<String> {
        let url = self.endpoint("/cgi-bin/exmail/app/get_email_alias");
        let payload: WecomApiEnvelope<WecomMailboxPayload> =
            request_wecom_json_ureq("wecom_mail_probe", ureq::post(&url).call())?;
        let data = payload.require_ok("wecom_mail_probe")?;
        if data.email.trim().is_empty() {
            return Err(Error::config("wecom_mail_probe", "missing mailbox email"));
        }
        Ok(data.email)
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
struct WecomMailListPayload {
    #[serde(default)]
    mail_list: Vec<WecomMailId>,
}

#[derive(Debug, Default, Deserialize)]
struct WecomMailId {
    #[serde(default)]
    mail_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct WecomReadMailPayload {
    #[serde(default)]
    mail_content: String,
}

#[derive(Debug, Default, Deserialize)]
struct WecomMailboxPayload {
    #[serde(default)]
    email: String,
}

fn validate_wecom_mail_credential(credential: &MailProviderCredential) -> Result<()> {
    if credential.corp_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_mail_provider",
            "mail_corp_id must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "wecom_mail_provider",
            "secret must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "wecom_mail_provider",
            "mail_base_url must not be empty",
        ));
    }
    Ok(())
}

fn parse_raw_message(
    credential: &MailProviderCredential,
    id: &str,
    raw: &str,
) -> Result<MailMessage> {
    let parsed = MessageParser::default()
        .parse(raw.as_bytes())
        .ok_or_else(|| Error::config("wecom_mail_parse", "failed to parse mail content"))?;
    let text_body = extract_text(&parsed);
    Ok(MailMessage {
        summary: MailMessageSummary {
            id: id.to_string(),
            provider: credential.provider.clone(),
            account_key: credential.account_key.clone(),
            mailbox: "inbox".to_string(),
            subject: parsed.subject().unwrap_or("(no subject)").to_string(),
            from: extract_first_address(parsed.from()),
            to: extract_addresses(parsed.to()),
            preview: render_mail_preview(&text_body),
            unread: false,
            received_at_unix_secs: 0,
        },
        text_body,
        message_id: parsed.message_id().unwrap_or_default().to_string(),
        reply_to: extract_addresses(parsed.reply_to()),
        references: parsed
            .references()
            .as_text()
            .unwrap_or_default()
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MailProviderCredential {
        MailProviderCredential {
            account_key: "mail-wecom".to_string(),
            provider: "wecom_mail".to_string(),
            account_id: "beetle@corp.example.com".to_string(),
            account_label: "WeCom Mail".to_string(),
            username: String::new(),
            corp_id: "wwcorp123".to_string(),
            secret: "corp-secret".to_string(),
            base_url: crate::office::WECOM_DEFAULT_BASE_URL.to_string(),
            imap_host: String::new(),
            imap_port: 0,
            imap_mailbox: "INBOX".to_string(),
            draft_mailbox: "Drafts".to_string(),
            imap_tls: false,
            smtp_host: String::new(),
            smtp_port: 0,
            smtp_tls: false,
            from_address: "beetle@corp.example.com".to_string(),
            from_name: "Beetle".to_string(),
        }
    }

    #[test]
    fn wecom_mail_provider_reports_supported_operations() {
        let provider = WecomMailProvider;
        assert!(provider.supports(MailOperation::List));
        assert!(provider.supports(MailOperation::Search));
        assert!(provider.supports(MailOperation::Get));
        assert!(provider.supports(MailOperation::Send));
        assert!(!provider.supports(MailOperation::Draft));
    }

    #[test]
    fn validate_wecom_mail_credential_rejects_missing_corp_id() {
        let mut credential = fixture();
        credential.corp_id.clear();
        let error = validate_wecom_mail_credential(&credential).expect_err("missing corp id");
        assert_eq!(error.stage(), "wecom_mail_provider");
    }

    #[test]
    fn parse_raw_message_extracts_basic_mail_fields() {
        let message = parse_raw_message(
            &fixture(),
            "mail-1",
            "From: Alice <alice@example.com>\r\nTo: Bob <bob@example.com>\r\nSubject: Demo\r\nMessage-ID: <msg-1>\r\n\r\nhello beetle",
        )
        .expect("message");
        assert_eq!(message.summary.subject, "Demo");
        assert_eq!(message.summary.from, "alice@example.com");
        assert_eq!(message.summary.to, vec!["bob@example.com".to_string()]);
        assert_eq!(message.text_body, "hello beetle");
    }
}
