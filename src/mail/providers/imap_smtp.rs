#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::error::{Error, Result};
use crate::mail::{
    credentials::mail_credential_from_office, MailMessage, MailMessageSummary, MailOperation,
    MailProvider, MailProviderCredential, MailQuery, MailSearchQuery, MailSendRequest,
};
use crate::office::{OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult};
use crate::util::{current_unix_secs, truncate_content_to_max};
use async_imap::types::Fetch;
use futures::TryStreamExt;
use lettre::message::header;
use lettre::message::{Mailbox, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use mail_parser::MessageParser;
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::DnsName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::runtime::Builder;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

type ImapSession = async_imap::Session<TlsStream<TcpStream>>;

pub struct ImapSmtpProvider;

impl MailProvider for ImapSmtpProvider {
    fn provider_name(&self) -> &'static str {
        "imap_smtp"
    }

    fn display_name(&self) -> &'static str {
        "IMAP/SMTP"
    }

    fn supports(&self, _op: MailOperation) -> bool {
        true
    }

    fn list_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        validate_imap_smtp_credential(credential)?;
        build_mail_runtime()?.block_on(async_list_messages(credential, query))
    }

    fn search_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        validate_imap_smtp_credential(credential)?;
        build_mail_runtime()?.block_on(async_search_messages(credential, query))
    }

    fn get_message(
        &self,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        validate_imap_smtp_credential(credential)?;
        build_mail_runtime()?.block_on(async_get_message(credential, id))
    }

    fn send_message(
        &self,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        validate_imap_smtp_credential(credential)?;
        send_via_smtp(credential, request)
    }

    fn draft_message(
        &self,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        validate_imap_smtp_credential(credential)?;
        build_mail_runtime()?.block_on(save_draft_via_imap(credential, request))
    }
}

pub struct ImapSmtpOfficeProbeAdapter;

impl OfficeProbeAdapter for ImapSmtpOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "imap_smtp"
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
        if validate_imap_smtp_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "mail_transport_config_missing".to_string(),
            });
        }
        build_mail_runtime()?.block_on(async_probe(&adapted))?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "imap_login_ok".to_string(),
        })
    }
}

fn validate_imap_smtp_credential(credential: &MailProviderCredential) -> Result<()> {
    if credential.username.trim().is_empty() {
        return Err(Error::config(
            "imap_smtp_provider",
            "username must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "imap_smtp_provider",
            "secret must not be empty",
        ));
    }
    if credential.imap_host.trim().is_empty() {
        return Err(Error::config(
            "imap_smtp_provider",
            "imap_host must not be empty",
        ));
    }
    if credential.smtp_host.trim().is_empty() {
        return Err(Error::config(
            "imap_smtp_provider",
            "smtp_host must not be empty",
        ));
    }
    if credential.imap_port == 0 {
        return Err(Error::config("imap_smtp_provider", "imap_port must be > 0"));
    }
    if credential.smtp_port == 0 {
        return Err(Error::config("imap_smtp_provider", "smtp_port must be > 0"));
    }
    if credential.from_address.trim().is_empty() {
        return Err(Error::config(
            "imap_smtp_provider",
            "from_address must not be empty",
        ));
    }
    Ok(())
}

pub(crate) fn render_mail_preview(text: &str) -> String {
    truncate_content_to_max(&text.split_whitespace().collect::<Vec<_>>().join(" "), 160).to_string()
}

fn build_mail_runtime() -> Result<tokio::runtime::Runtime> {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::config("imap_smtp_runtime", error.to_string()))
}

async fn async_probe(credential: &MailProviderCredential) -> Result<()> {
    let mut session = connect_imap(credential).await?;
    session
        .select(&credential.imap_mailbox)
        .await
        .map_err(|error| Error::config("imap_smtp_probe_select", error.to_string()))?;
    session
        .logout()
        .await
        .map_err(|error| Error::config("imap_smtp_probe_logout", error.to_string()))?;
    Ok(())
}

async fn async_list_messages(
    credential: &MailProviderCredential,
    query: MailQuery,
) -> Result<Vec<MailMessageSummary>> {
    let mut session = connect_imap(credential).await?;
    let mailbox = if query.mailbox.trim().is_empty() {
        credential.imap_mailbox.as_str()
    } else {
        query.mailbox.as_str()
    };
    session
        .select(mailbox)
        .await
        .map_err(|error| Error::config("imap_smtp_select", error.to_string()))?;
    let criteria = if query.unread_only { "UNSEEN" } else { "ALL" };
    let uids = session
        .uid_search(criteria)
        .await
        .map_err(|error| Error::config("imap_smtp_search", error.to_string()))?;
    let items = fetch_message_summaries_for_uids(
        &mut session,
        credential,
        mailbox,
        uids.into_iter().collect(),
        query.received_after_unix_secs,
        query.limit,
    )
    .await?;
    session
        .logout()
        .await
        .map_err(|error| Error::config("imap_smtp_logout", error.to_string()))?;
    Ok(items)
}

async fn async_search_messages(
    credential: &MailProviderCredential,
    query: MailSearchQuery,
) -> Result<Vec<MailMessageSummary>> {
    let mut session = connect_imap(credential).await?;
    let mailbox = if query.mailbox.trim().is_empty() {
        credential.imap_mailbox.as_str()
    } else {
        query.mailbox.as_str()
    };
    session
        .select(mailbox)
        .await
        .map_err(|error| Error::config("imap_smtp_select", error.to_string()))?;
    let criteria = render_search_criteria(&query)?;
    let uids = session
        .uid_search(criteria)
        .await
        .map_err(|error| Error::config("imap_smtp_search", error.to_string()))?;
    let items = fetch_message_summaries_for_uids(
        &mut session,
        credential,
        mailbox,
        uids.into_iter().collect(),
        query.received_after_unix_secs,
        query.limit,
    )
    .await?;
    session
        .logout()
        .await
        .map_err(|error| Error::config("imap_smtp_logout", error.to_string()))?;
    Ok(items)
}

async fn async_get_message(
    credential: &MailProviderCredential,
    id: &str,
) -> Result<Option<MailMessage>> {
    let mut session = connect_imap(credential).await?;
    session
        .select(&credential.imap_mailbox)
        .await
        .map_err(|error| Error::config("imap_smtp_select", error.to_string()))?;
    let fetches = session
        .uid_fetch(id, "RFC822")
        .await
        .map_err(|error| Error::config("imap_smtp_fetch", error.to_string()))?;
    let fetches: Vec<Fetch> = fetches
        .try_collect()
        .await
        .map_err(|error| Error::config("imap_smtp_fetch", error.to_string()))?;
    let result = if let Some(fetch) = fetches.into_iter().next() {
        Some(message_from_fetch(
            credential,
            &credential.imap_mailbox,
            fetch,
        )?)
    } else {
        None
    };
    session
        .logout()
        .await
        .map_err(|error| Error::config("imap_smtp_logout", error.to_string()))?;
    Ok(result)
}

async fn fetch_message_summaries_for_uids(
    session: &mut ImapSession,
    credential: &MailProviderCredential,
    mailbox: &str,
    mut uids: Vec<u32>,
    received_after_unix_secs: Option<u64>,
    limit: usize,
) -> Result<Vec<MailMessageSummary>> {
    uids.sort_unstable_by(|left, right| right.cmp(left));
    if uids.len() > limit {
        uids.truncate(limit);
    }
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let uid_set = uids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let fetches = session
        .uid_fetch(&uid_set, "RFC822")
        .await
        .map_err(|error| Error::config("imap_smtp_fetch", error.to_string()))?;
    let fetches: Vec<Fetch> = fetches
        .try_collect()
        .await
        .map_err(|error| Error::config("imap_smtp_fetch", error.to_string()))?;
    let mut items = fetches
        .into_iter()
        .map(|fetch| summarize_fetch(credential, mailbox, fetch))
        .collect::<Result<Vec<_>>>()?;
    if let Some(received_after) = received_after_unix_secs {
        items.retain(|item| {
            item.received_at_unix_secs == 0 || item.received_at_unix_secs >= received_after
        });
    }
    items.sort_by(|left, right| {
        right
            .received_at_unix_secs
            .cmp(&left.received_at_unix_secs)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(items)
}

fn render_search_criteria(query: &MailSearchQuery) -> Result<String> {
    let needle = query.query.trim();
    if needle.is_empty() {
        return Err(Error::config("imap_smtp_search", "query must not be empty"));
    }
    let base = if query.unread_only { "UNSEEN" } else { "ALL" };
    Ok(format!(
        r#"{base} TEXT "{}""#,
        escape_imap_search_string(needle)
    ))
}

fn escape_imap_search_string(value: &str) -> String {
    value.replace('\\', r"\\").replace('"', r#"\""#)
}

async fn save_draft_via_imap(
    credential: &MailProviderCredential,
    request: &MailSendRequest,
) -> Result<MailMessageSummary> {
    let message = build_rfc822_message(credential, request)?;
    let mut session = connect_imap(credential).await?;
    session
        .append(
            &credential.draft_mailbox,
            Some(r"(\Draft)"),
            None,
            message.formatted(),
        )
        .await
        .map_err(|error| Error::config("imap_smtp_append_draft", error.to_string()))?;
    session
        .logout()
        .await
        .map_err(|error| Error::config("imap_smtp_logout", error.to_string()))?;
    Ok(MailMessageSummary {
        id: format!("draft-{}", current_unix_secs()),
        provider: credential.provider.clone(),
        account_key: credential.account_key.clone(),
        mailbox: credential.draft_mailbox.clone(),
        subject: request.subject.clone(),
        from: credential.from_address.clone(),
        to: request.to.clone(),
        preview: render_mail_preview(&request.text_body),
        unread: false,
        received_at_unix_secs: current_unix_secs(),
    })
}

async fn connect_imap(credential: &MailProviderCredential) -> Result<ImapSession> {
    let addr = format!("{}:{}", credential.imap_host, credential.imap_port);
    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|error| Error::config("imap_smtp_tcp_connect", error.to_string()))?;
    let certs = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };
    let config = ClientConfig::builder()
        .with_root_certificates(certs)
        .with_no_client_auth();
    let tls = TlsConnector::from(Arc::new(config));
    let sni = DnsName::try_from(credential.imap_host.clone())
        .map_err(|error| Error::config("imap_smtp_sni", error.to_string()))?;
    let stream = tls
        .connect(sni.into(), tcp)
        .await
        .map_err(|error| Error::config("imap_smtp_tls_connect", error.to_string()))?;
    let client = async_imap::Client::new(stream);
    client
        .login(&credential.username, &credential.secret)
        .await
        .map_err(|(error, _)| Error::config("imap_smtp_login", error.to_string()))
}

fn summarize_fetch(
    credential: &MailProviderCredential,
    mailbox: &str,
    fetch: Fetch,
) -> Result<MailMessageSummary> {
    Ok(message_from_fetch(credential, mailbox, fetch)?.summary)
}

fn message_from_fetch(
    credential: &MailProviderCredential,
    mailbox: &str,
    fetch: Fetch,
) -> Result<MailMessage> {
    let uid = fetch
        .uid
        .ok_or_else(|| Error::config("imap_smtp_fetch", "message uid missing"))?;
    let raw = fetch
        .body()
        .ok_or_else(|| Error::config("imap_smtp_fetch", "message body missing"))?;
    let parsed = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| Error::config("imap_smtp_parse", "failed to parse RFC822 message"))?;
    let text_body = extract_text(&parsed);
    Ok(MailMessage {
        summary: MailMessageSummary {
            id: uid.to_string(),
            provider: credential.provider.clone(),
            account_key: credential.account_key.clone(),
            mailbox: mailbox.to_string(),
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

pub(crate) fn extract_first_address(addresses: Option<&mail_parser::Address<'_>>) -> String {
    match addresses {
        Some(mail_parser::Address::List(items)) => items
            .iter()
            .find_map(|entry| entry.address.as_deref())
            .unwrap_or_default()
            .to_string(),
        Some(mail_parser::Address::Group(groups)) => groups
            .iter()
            .flat_map(|group| group.addresses.iter())
            .find_map(|entry| entry.address.as_deref())
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    }
}

pub(crate) fn extract_addresses(addresses: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    match addresses {
        Some(mail_parser::Address::List(items)) => items
            .iter()
            .filter_map(|entry| entry.address.as_deref().map(str::to_string))
            .collect(),
        Some(mail_parser::Address::Group(groups)) => groups
            .iter()
            .flat_map(|group| group.addresses.iter())
            .filter_map(|entry| entry.address.as_deref().map(str::to_string))
            .collect(),
        None => Vec::new(),
    }
}

pub(crate) fn extract_text(parsed: &mail_parser::Message<'_>) -> String {
    if let Some(text) = parsed.body_text(0) {
        return text.to_string();
    }
    if let Some(html) = parsed.body_html(0) {
        return render_mail_preview(html.as_ref());
    }
    String::new()
}

fn send_via_smtp(
    credential: &MailProviderCredential,
    request: &MailSendRequest,
) -> Result<MailMessageSummary> {
    let message = build_rfc822_message(credential, request)?;
    let credentials = Credentials::new(credential.username.clone(), credential.secret.clone());
    let builder = if credential.smtp_tls {
        if credential.smtp_port == 587 {
            SmtpTransport::starttls_relay(&credential.smtp_host)
                .map_err(|error| Error::config("imap_smtp_smtp_transport", error.to_string()))?
        } else {
            SmtpTransport::relay(&credential.smtp_host)
                .map_err(|error| Error::config("imap_smtp_smtp_transport", error.to_string()))?
        }
    } else {
        SmtpTransport::builder_dangerous(&credential.smtp_host)
    };
    let transport = builder
        .port(credential.smtp_port)
        .credentials(credentials)
        .build();
    transport
        .send(&message)
        .map_err(|error| Error::config("imap_smtp_smtp_send", error.to_string()))?;
    Ok(MailMessageSummary {
        id: format!("smtp-{}", current_unix_secs()),
        provider: credential.provider.clone(),
        account_key: credential.account_key.clone(),
        mailbox: "Sent".to_string(),
        subject: request.subject.clone(),
        from: credential.from_address.clone(),
        to: request.to.clone(),
        preview: render_mail_preview(&request.text_body),
        unread: false,
        received_at_unix_secs: current_unix_secs(),
    })
}

fn build_rfc822_message(
    credential: &MailProviderCredential,
    request: &MailSendRequest,
) -> Result<Message> {
    let from = parse_mailbox(&credential.from_address, Some(&credential.from_name))?;
    let mut builder = Message::builder()
        .from(from)
        .subject(request.subject.clone());
    for address in &request.to {
        builder = builder.to(parse_mailbox(address, None)?);
    }
    for address in &request.cc {
        builder = builder.cc(parse_mailbox(address, None)?);
    }
    for address in &request.bcc {
        builder = builder.bcc(parse_mailbox(address, None)?);
    }
    if !request.in_reply_to.trim().is_empty() {
        builder = builder.header(header::InReplyTo::from(request.in_reply_to.clone()));
    }
    if !request.references.trim().is_empty() {
        builder = builder.header(header::References::from(request.references.clone()));
    }
    builder
        .singlepart(SinglePart::plain(request.text_body.clone()))
        .map_err(|error| Error::config("imap_smtp_smtp_message", error.to_string()))
}

fn parse_mailbox(address: &str, display_name: Option<&str>) -> Result<Mailbox> {
    let value = match display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(name) => format!("{name} <{}>", address.trim()),
        None => address.trim().to_string(),
    };
    value
        .parse::<Mailbox>()
        .map_err(|error| Error::config("imap_smtp_mailbox", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> MailProviderCredential {
        MailProviderCredential {
            account_key: "mail-work".to_string(),
            provider: "imap_smtp".to_string(),
            account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            username: "work@example.com".to_string(),
            corp_id: String::new(),
            secret: "secret".to_string(),
            base_url: String::new(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_mailbox: "INBOX".to_string(),
            draft_mailbox: "Drafts".to_string(),
            imap_tls: true,
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_tls: true,
            from_address: "work@example.com".to_string(),
            from_name: "Work".to_string(),
        }
    }

    #[test]
    fn validate_imap_smtp_credential_rejects_missing_hosts() {
        let mut credential = fixture();
        credential.imap_host.clear();
        let error = validate_imap_smtp_credential(&credential).expect_err("missing host");
        assert!(error.to_string().contains("imap_host"));
    }

    #[test]
    fn validate_imap_smtp_credential_accepts_complete_transport_shape() {
        validate_imap_smtp_credential(&fixture()).expect("valid transport shape");
    }

    #[test]
    fn render_mail_preview_collapses_whitespace() {
        assert_eq!(
            render_mail_preview("hello\n\nworld   beetle"),
            "hello world beetle"
        );
    }

    #[test]
    fn render_search_criteria_keeps_unseen_and_escapes_quotes() {
        let criteria = render_search_criteria(&MailSearchQuery {
            mailbox: "INBOX".to_string(),
            query: r#"hello "project""#.to_string(),
            unread_only: true,
            received_after_unix_secs: None,
            limit: 10,
        })
        .expect("criteria");
        assert_eq!(criteria, r#"UNSEEN TEXT "hello \"project\"""#);
    }

    #[test]
    fn probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = ImapSmtpOfficeProbeAdapter;
        let mut credential = crate::office::OfficeCredential {
            account_key: "mail-work".to_string(),
            access_token: "secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        credential
            .metadata
            .insert("mail_username".to_string(), "work@example.com".to_string());
        let result = adapter
            .probe(
                &crate::office::OfficeAccount {
                    account_key: "mail-work".to_string(),
                    provider_kind: "imap_smtp".to_string(),
                    external_account_id: "work@example.com".to_string(),
                    account_label: "Work".to_string(),
                    identity_class: crate::office::OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![crate::office::OfficeCapability::Mail],
                },
                &credential,
            )
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
        assert_eq!(result.reason, "mail_transport_config_missing");
    }
}
