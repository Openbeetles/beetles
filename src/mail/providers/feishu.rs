#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailQuery, MailSearchQuery, MailSendRequest,
};
use crate::office::{OfficeHttpClient, OfficeProbeAdapter, OfficeProbeResult};

use super::imap_smtp::{ImapSmtpOfficeProbeAdapter, ImapSmtpProvider};

pub struct FeishuMailProvider;

impl MailProvider for FeishuMailProvider {
    fn provider_name(&self) -> &'static str {
        "feishu_mail"
    }

    fn display_name(&self) -> &'static str {
        "Feishu Mail"
    }

    fn supports(&self, _op: MailOperation) -> bool {
        true
    }

    fn list_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let backend = ImapSmtpProvider;
        backend
            .list_messages(http, credential, query)
            .map_err(remap_transport_error)
    }

    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let backend = ImapSmtpProvider;
        backend
            .search_messages(http, credential, query)
            .map_err(remap_transport_error)
    }

    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let backend = ImapSmtpProvider;
        backend
            .get_message(http, credential, id)
            .map_err(remap_transport_error)
    }

    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let backend = ImapSmtpProvider;
        backend
            .send_message(http, credential, request)
            .map_err(remap_transport_error)
    }

    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let backend = ImapSmtpProvider;
        backend
            .draft_message(http, credential, request)
            .map_err(remap_transport_error)
    }
}

pub struct FeishuMailOfficeProbeAdapter;

impl OfficeProbeAdapter for FeishuMailOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "feishu_mail"
    }

    fn probe(
        &self,
        http: &mut dyn OfficeHttpClient,
        account: &crate::office::OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let backend = ImapSmtpOfficeProbeAdapter;
        backend
            .probe(http, account, credential)
            .map(|mut result| {
                if result.reason == "imap_login_ok" {
                    result.reason = "feishu_mail_login_ok".to_string();
                }
                result
            })
            .map_err(remap_transport_error)
    }
}

fn remap_transport_error(error: Error) -> Error {
    match error.stage() {
        "imap_smtp_provider" => error.with_stage("feishu_mail_provider"),
        "imap_smtp_runtime" => error.with_stage("feishu_mail_runtime"),
        "imap_smtp_probe_select" => error.with_stage("feishu_mail_probe_select"),
        "imap_smtp_probe_logout" => error.with_stage("feishu_mail_probe_logout"),
        "imap_smtp_select" => error.with_stage("feishu_mail_select"),
        "imap_smtp_search" => error.with_stage("feishu_mail_search"),
        "imap_smtp_logout" => error.with_stage("feishu_mail_logout"),
        "imap_smtp_fetch" => error.with_stage("feishu_mail_fetch"),
        "imap_smtp_parse" => error.with_stage("feishu_mail_parse"),
        "imap_smtp_append_draft" => error.with_stage("feishu_mail_append_draft"),
        "imap_smtp_tcp_connect" => error.with_stage("feishu_mail_tcp_connect"),
        "imap_smtp_sni" => error.with_stage("feishu_mail_sni"),
        "imap_smtp_tls_connect" => error.with_stage("feishu_mail_tls_connect"),
        "imap_smtp_login" => error.with_stage("feishu_mail_login"),
        "imap_smtp_smtp_transport" => error.with_stage("feishu_mail_smtp_transport"),
        "imap_smtp_smtp_send" => error.with_stage("feishu_mail_smtp_send"),
        "imap_smtp_smtp_message" => error.with_stage("feishu_mail_smtp_message"),
        "imap_smtp_mailbox" => error.with_stage("feishu_mail_mailbox"),
        _ => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::MailOperation;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeCapability, OfficeCredential,
        OfficeProbeDisposition,
    };

    #[test]
    fn feishu_mail_provider_reports_transport_capabilities() {
        let provider = FeishuMailProvider;
        assert_eq!(provider.provider_name(), "feishu_mail");
        assert_eq!(provider.display_name(), "Feishu Mail");
        assert!(provider.supports(MailOperation::List));
        assert!(provider.supports(MailOperation::Search));
        assert!(provider.supports(MailOperation::Get));
        assert!(provider.supports(MailOperation::Send));
        assert!(provider.supports(MailOperation::Draft));
    }

    #[test]
    fn feishu_mail_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = FeishuMailOfficeProbeAdapter;
        let mut credential = OfficeCredential {
            account_key: "mail-feishu".to_string(),
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
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "mail-feishu".to_string(),
                    provider_kind: "feishu_mail".to_string(),
                    external_account_id: "work@example.com".to_string(),
                    account_label: "Feishu Mail".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
                &credential,
            )
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
        assert_eq!(result.provider_kind, "feishu_mail");
        assert_eq!(result.reason, "mail_transport_config_missing");
    }
}
