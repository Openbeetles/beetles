//! Shared mail domain: provider credentials, messages, and service routing.

mod credentials;
mod provider;
pub mod providers;
mod service;

use serde::{Deserialize, Serialize};

pub use credentials::{
    MailProviderCredential, MailProviderCredentialStatus, MailProviderCredentialStore,
    OfficeBackedMailProviderCredentialStore, OFFICE_METADATA_MAIL_DRAFT_MAILBOX,
    OFFICE_METADATA_MAIL_FROM_ADDRESS, OFFICE_METADATA_MAIL_FROM_NAME,
    OFFICE_METADATA_MAIL_IMAP_HOST, OFFICE_METADATA_MAIL_IMAP_MAILBOX,
    OFFICE_METADATA_MAIL_IMAP_PORT, OFFICE_METADATA_MAIL_IMAP_TLS, OFFICE_METADATA_MAIL_SMTP_HOST,
    OFFICE_METADATA_MAIL_SMTP_PORT, OFFICE_METADATA_MAIL_SMTP_TLS, OFFICE_METADATA_MAIL_USERNAME,
};
pub use provider::{MailOperation, MailProvider, MailProviderRegistry};
pub use service::MailService;

pub const DEFAULT_MAILBOX: &str = "INBOX";
pub const DEFAULT_DRAFT_MAILBOX: &str = "Drafts";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailMessageSummary {
    pub id: String,
    pub provider: String,
    pub account_key: String,
    pub mailbox: String,
    pub subject: String,
    pub from: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub received_at_unix_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailMessage {
    pub summary: MailMessageSummary,
    #[serde(default)]
    pub text_body: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub reply_to: Vec<String>,
    #[serde(default)]
    pub references: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailQuery {
    #[serde(default)]
    pub mailbox: String,
    #[serde(default)]
    pub unread_only: bool,
    #[serde(default)]
    pub received_after_unix_secs: Option<u64>,
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailSendRequest {
    pub subject: String,
    pub text_body: String,
    #[serde(default)]
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub references: String,
}
