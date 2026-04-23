use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailProviderCredentialStatus, MailProviderCredentialStore, MailProviderRegistry, MailQuery,
    MailSearchQuery, MailSendRequest,
};
use crate::office::{
    office_authority_from_service, OfficeAccountAssessment, OfficeAccountIdentityClass,
    OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability, OfficeCapabilityRuntime,
    OfficeCapabilityServiceCore, OfficeHttpClient, OfficeResolveResult, OfficeService,
};
use std::sync::Arc;

type MailRemoteCore = OfficeCapabilityServiceCore<
    MailProviderRegistry,
    dyn MailProviderCredentialStore + Send + Sync,
>;

pub struct MailService {
    core: MailRemoteCore,
}

struct MutatingActionRoute<'a> {
    provider: &'a str,
    account_key: Option<&'a str>,
    preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ops: &'a [MailOperation],
    activity_kind: &'static str,
}

impl MailService {
    pub fn new(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
    ) -> Self {
        Self::build(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self::build(
            credential_store,
            providers,
            office_authority_from_service(office_service),
        )
    }

    pub fn with_office_authority(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self::build(credential_store, providers, office_authority)
    }

    fn build(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            core: OfficeCapabilityServiceCore::new(
                providers,
                credential_store,
                OfficeCapabilityRuntime::new(
                    OfficeCapability::Mail,
                    "mail_provider",
                    "mail",
                    "mail_runtime",
                    office_authority,
                ),
                "mail_provider",
            ),
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.core.provider_names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        self.resolve_provider_name_with_identity(provider, None)
    }

    pub fn resolve_provider_name_with_identity(
        &self,
        provider: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<String> {
        self.core
            .resolve_provider_name(provider, preferred_identity_class)
    }

    pub fn list_provider_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>> {
        self.core.list_provider_statuses()
    }

    pub fn office_resolve_hint(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeResolveResult>> {
        self.office_resolve_hint_with_identity(provider, account_key, None)
    }

    pub fn office_resolve_hint_with_identity(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<Option<OfficeResolveResult>> {
        self.core
            .resolve_hint(provider, account_key, preferred_identity_class)
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        self.core.runtime_statuses()
    }

    pub fn office_runtime_status(
        &self,
        account_key: &str,
    ) -> Result<Option<OfficeAccountRuntimeStatus>> {
        self.core.runtime_status(account_key)
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        self.core.account_assessments()
    }

    pub fn office_identity_class_for_account(
        &self,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeAccountIdentityClass>> {
        self.core.identity_class_for_account(account_key)
    }

    pub fn provider_supports(&self, provider: &str, op: MailOperation) -> bool {
        self.core.provider_supports(provider, op)
    }

    pub fn provider_is_routable_for_ops(
        &self,
        provider: &str,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        ops: &[MailOperation],
    ) -> bool {
        self.core
            .provider_is_routable_for_ops(provider, preferred_identity_class, ops)
    }

    pub fn list(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.list_with_identity(http, provider, account_key, None, query)
    }

    pub fn list_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[MailOperation::List],
            "mail_list",
            |provider_impl, credential| provider_impl.list_messages(http, credential, query),
        )
    }

    pub fn search(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.search_with_identity(http, provider, account_key, None, query)
    }

    pub fn search_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[MailOperation::Search],
            "mail_search",
            |provider_impl, credential| provider_impl.search_messages(http, credential, query),
        )
    }

    pub fn get(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        self.get_with_identity(http, provider, account_key, None, id)
    }

    pub fn get_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[MailOperation::Get],
            "mail_get",
            |provider_impl, credential| provider_impl.get_message(http, credential, id),
        )
    }

    pub fn send(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.send_with_identity(http, provider, account_key, None, request)
    }

    pub fn send_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            http,
            MutatingActionRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[MailOperation::Send],
                activity_kind: "mail_send",
            },
            |http, provider_impl, credential| provider_impl.send_message(http, credential, request),
        )
    }

    pub fn draft(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.draft_with_identity(http, provider, account_key, None, request)
    }

    pub fn draft_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            http,
            MutatingActionRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[MailOperation::Draft],
                activity_kind: "mail_draft",
            },
            |http, provider_impl, credential| {
                provider_impl.draft_message(http, credential, request)
            },
        )
    }

    pub fn reply(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.reply_with_identity(http, provider, account_key, None, original_id, request)
    }

    pub fn reply_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            http,
            MutatingActionRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[MailOperation::Get, MailOperation::Send],
                activity_kind: "mail_reply",
            },
            |http, provider_impl, credential| {
                let original = provider_impl
                    .get_message(http, credential, original_id)?
                    .ok_or_else(|| Error::config("mail_reply", "original message not found"))?;
                let mut composed = request.clone();
                composed.subject =
                    compose_reply_subject(&request.subject, &original.summary.subject);
                composed.text_body = render_reply_body(&request.text_body, &original);
                composed.to =
                    merge_unique_recipients(reply_recipients(&original), composed.to.clone());
                composed.in_reply_to = original.message_id.clone();
                composed.references =
                    compose_references(&original.references, &original.message_id);
                provider_impl.send_message(http, credential, &composed)
            },
        )
    }

    pub fn forward(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.forward_with_identity(http, provider, account_key, None, original_id, request)
    }

    pub fn forward_with_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            http,
            MutatingActionRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[MailOperation::Get, MailOperation::Send],
                activity_kind: "mail_forward",
            },
            |http, provider_impl, credential| {
                let original = provider_impl
                    .get_message(http, credential, original_id)?
                    .ok_or_else(|| Error::config("mail_forward", "original message not found"))?;
                let mut composed = request.clone();
                composed.subject =
                    compose_forward_subject(&request.subject, &original.summary.subject);
                composed.text_body = render_forward_body(&request.text_body, &original);
                provider_impl.send_message(http, credential, &composed)
            },
        )
    }

    fn execute_mutating_action<F>(
        &self,
        http: &mut dyn OfficeHttpClient,
        route: MutatingActionRoute<'_>,
        execute: F,
    ) -> Result<MailMessageSummary>
    where
        F: FnOnce(
            &mut dyn OfficeHttpClient,
            &Arc<dyn MailProvider>,
            &MailProviderCredential,
        ) -> Result<MailMessageSummary>,
    {
        self.core.run_remote_operation(
            route.provider,
            route.account_key,
            route.preferred_identity_class,
            route.ops,
            route.activity_kind,
            |provider_impl, credential| execute(http, provider_impl, credential),
        )
    }
}

fn compose_reply_subject(subject_override: &str, original_subject: &str) -> String {
    if !subject_override.trim().is_empty() {
        subject_override.trim().to_string()
    } else if original_subject.trim_start().starts_with("Re:") {
        original_subject.trim().to_string()
    } else {
        format!("Re: {}", original_subject.trim())
    }
}

fn compose_forward_subject(subject_override: &str, original_subject: &str) -> String {
    if !subject_override.trim().is_empty() {
        subject_override.trim().to_string()
    } else if original_subject.trim_start().starts_with("Fwd:") {
        original_subject.trim().to_string()
    } else {
        format!("Fwd: {}", original_subject.trim())
    }
}

fn render_reply_body(user_text: &str, original: &MailMessage) -> String {
    format!(
        "{}\n\nOn message {} from {}, subject '{}':\n{}",
        user_text.trim(),
        original.summary.id,
        original.summary.from,
        original.summary.subject,
        original.text_body.trim()
    )
}

fn render_forward_body(user_text: &str, original: &MailMessage) -> String {
    let prefix = user_text.trim();
    if prefix.is_empty() {
        format!(
            "Forwarded message from {} with subject '{}':\n{}",
            original.summary.from,
            original.summary.subject,
            original.text_body.trim()
        )
    } else {
        format!(
            "{}\n\nForwarded message from {} with subject '{}':\n{}",
            prefix,
            original.summary.from,
            original.summary.subject,
            original.text_body.trim()
        )
    }
}

fn reply_recipients(original: &MailMessage) -> Vec<String> {
    let preferred = if original.reply_to.is_empty() {
        vec![original.summary.from.clone()]
    } else {
        original.reply_to.clone()
    };
    preferred
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn compose_references(existing: &str, message_id: &str) -> String {
    let mut refs = existing
        .split_whitespace()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !message_id.trim().is_empty() && !refs.iter().any(|value| value == message_id) {
        refs.push(message_id.trim().to_string());
    }
    refs.join(" ")
}

fn merge_unique_recipients(primary: Vec<String>, extras: Vec<String>) -> Vec<String> {
    let mut merged = primary;
    for recipient in extras {
        if !merged
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(recipient.as_str()))
        {
            merged.push(recipient);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::mail::OfficeBackedMailProviderCredentialStore;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeSelectionPolicy,
        UnavailableOfficeHttpClient,
    };
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }
        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }
        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }
        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRuntimeStatusStore {
        items: Mutex<BTreeMap<String, OfficeAccountRuntimeStatus>>,
    }

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }
        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }
        fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(status.account_key.clone(), status.clone());
            Ok(())
        }
        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubProvider {
        sent_requests: Mutex<Vec<MailSendRequest>>,
        drafted_requests: Mutex<Vec<MailSendRequest>>,
    }

    impl MailProvider for StubProvider {
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
            _http: &mut dyn crate::office::OfficeHttpClient,
            credential: &MailProviderCredential,
            query: MailQuery,
        ) -> Result<Vec<MailMessageSummary>> {
            Ok(vec![MailMessageSummary {
                id: "msg-1".to_string(),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: query.mailbox,
                subject: "hello".to_string(),
                from: credential.from_address.clone(),
                to: vec![credential.account_id.clone()],
                preview: "preview".to_string(),
                unread: true,
                received_at_unix_secs: 1,
            }])
        }
        fn search_messages(
            &self,
            _http: &mut dyn crate::office::OfficeHttpClient,
            credential: &MailProviderCredential,
            query: MailSearchQuery,
        ) -> Result<Vec<MailMessageSummary>> {
            Ok(vec![MailMessageSummary {
                id: "search-1".to_string(),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: if query.mailbox.is_empty() {
                    credential.imap_mailbox.clone()
                } else {
                    query.mailbox
                },
                subject: format!("matched {}", query.query.trim()),
                from: credential.from_address.clone(),
                to: vec![credential.account_id.clone()],
                preview: "search preview".to_string(),
                unread: true,
                received_at_unix_secs: 1,
            }])
        }
        fn get_message(
            &self,
            _http: &mut dyn crate::office::OfficeHttpClient,
            credential: &MailProviderCredential,
            id: &str,
        ) -> Result<Option<MailMessage>> {
            Ok(Some(MailMessage {
                summary: MailMessageSummary {
                    id: id.to_string(),
                    provider: credential.provider.clone(),
                    account_key: credential.account_key.clone(),
                    mailbox: credential.imap_mailbox.clone(),
                    subject: "hello".to_string(),
                    from: credential.from_address.clone(),
                    to: vec![credential.account_id.clone()],
                    preview: "preview".to_string(),
                    unread: false,
                    received_at_unix_secs: 1,
                },
                text_body: "body".to_string(),
                message_id: "<msg-1@example.com>".to_string(),
                reply_to: vec!["reply@example.com".to_string()],
                references: "<root@example.com>".to_string(),
            }))
        }
        fn send_message(
            &self,
            _http: &mut dyn crate::office::OfficeHttpClient,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
            self.sent_requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request.clone());
            Ok(MailMessageSummary {
                id: "sent-1".to_string(),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: "Sent".to_string(),
                subject: request.subject.clone(),
                from: credential.from_address.clone(),
                to: request.to.clone(),
                preview: request.text_body.clone(),
                unread: false,
                received_at_unix_secs: 2,
            })
        }

        fn draft_message(
            &self,
            _http: &mut dyn crate::office::OfficeHttpClient,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
            self.drafted_requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(request.clone());
            Ok(MailMessageSummary {
                id: "draft-1".to_string(),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: credential.draft_mailbox.clone(),
                subject: request.subject.clone(),
                from: credential.from_address.clone(),
                to: request.to.clone(),
                preview: request.text_body.clone(),
                unread: false,
                received_at_unix_secs: 2,
            })
        }
    }

    fn unavailable_http() -> UnavailableOfficeHttpClient {
        UnavailableOfficeHttpClient
    }

    fn build_service() -> (MailService, Arc<StubProvider>, Arc<StubRuntimeStatusStore>) {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    ("mail_username".to_string(), "work@example.com".to_string()),
                    ("mail_imap_host".to_string(), "imap.example.com".to_string()),
                    ("mail_smtp_host".to_string(), "smtp.example.com".to_string()),
                    (
                        "mail_from_address".to_string(),
                        "work@example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        let office = OfficeService::new(
            registry,
            OfficeSelectionPolicy {
                ask_when_ambiguous: false,
                preferred_identity_class: None,
            },
            credential_store.clone(),
            runtime_store.clone(),
        );
        let mail_credentials: Arc<dyn MailProviderCredentialStore + Send + Sync> =
            Arc::new(OfficeBackedMailProviderCredentialStore::new(office.clone()));
        let mut providers = MailProviderRegistry::new();
        let provider = Arc::new(StubProvider::default());
        providers.register(provider.clone());
        (
            MailService::with_office_service(mail_credentials, providers, Some(office)),
            provider,
            runtime_store,
        )
    }

    #[test]
    fn mail_service_uses_office_resolution_when_provider_has_single_candidate() {
        let (service, _provider, _runtime_store) = build_service();
        let mut http = unavailable_http();
        let items = service
            .list(
                &mut http,
                "imap_smtp",
                None,
                MailQuery {
                    mailbox: "INBOX".to_string(),
                    unread_only: true,
                    received_after_unix_secs: None,
                    limit: 10,
                },
            )
            .expect("list mail");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].account_key, "mail-work");
    }

    #[test]
    fn mail_service_search_uses_office_resolution_when_provider_has_single_candidate() {
        let (service, _provider, _runtime_store) = build_service();
        let mut http = unavailable_http();
        let items = service
            .search(
                &mut http,
                "imap_smtp",
                None,
                MailSearchQuery {
                    mailbox: "INBOX".to_string(),
                    query: "hello project".to_string(),
                    unread_only: true,
                    received_after_unix_secs: None,
                    limit: 10,
                },
            )
            .expect("search mail");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].account_key, "mail-work");
        assert_eq!(items[0].subject, "matched hello project");
    }

    #[test]
    fn mail_service_provider_status_comes_from_credential_store() {
        let (service, _provider, _runtime_store) = build_service();
        let statuses = service.list_provider_statuses().expect("provider statuses");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].provider, "imap_smtp");
        assert_eq!(statuses[0].from_address, "work@example.com");
    }

    #[test]
    fn mail_service_infers_provider_from_office_selected_route() {
        let (service, _provider, _runtime_store) = build_service();
        let provider = service
            .resolve_provider_name(None)
            .expect("resolve provider name");
        assert_eq!(provider, "imap_smtp");
    }

    #[test]
    fn mail_service_reports_ambiguous_office_accounts_with_candidate_labels() {
        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, account_id) in [
            ("mail-work", "work@example.com"),
            ("mail-personal", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    OfficeCredential {
                        account_key: account_key.to_string(),
                        access_token: "secret".to_string(),
                        refresh_token: String::new(),
                        token_endpoint: String::new(),
                        expires_at_unix_secs: 0,
                        updated_at: 1,
                        metadata: [
                            ("mail_username".to_string(), account_id.to_string()),
                            ("mail_from_address".to_string(), account_id.to_string()),
                            ("mail_imap_host".to_string(), "imap.example.com".to_string()),
                            ("mail_smtp_host".to_string(), "smtp.example.com".to_string()),
                        ]
                        .into_iter()
                        .collect(),
                    },
                );
        }

        let mut registry = OfficeAccountRegistry::new();
        for (account_key, label, account_id, identity_class) in [
            (
                "mail-work",
                "Work",
                "work@example.com",
                OfficeAccountIdentityClass::Work,
            ),
            (
                "mail-personal",
                "Personal",
                "personal@example.com",
                OfficeAccountIdentityClass::Personal,
            ),
        ] {
            registry.insert(OfficeAccount {
                account_key: account_key.to_string(),
                provider_kind: "imap_smtp".to_string(),
                external_account_id: account_id.to_string(),
                account_label: label.to_string(),
                identity_class,
                enabled_capabilities: vec![OfficeCapability::Mail],
            });
        }

        let office = OfficeService::new(
            registry,
            OfficeSelectionPolicy::default(),
            credential_store.clone(),
            Arc::new(StubRuntimeStatusStore::default()),
        );
        let mail_credentials: Arc<dyn MailProviderCredentialStore + Send + Sync> =
            Arc::new(OfficeBackedMailProviderCredentialStore::new(office.clone()));
        let mut providers = MailProviderRegistry::new();
        providers.register(Arc::new(StubProvider::default()));
        let service = MailService::with_office_service(mail_credentials, providers, Some(office));
        let mut http = unavailable_http();

        let error = service
            .list(
                &mut http,
                "imap_smtp",
                None,
                MailQuery {
                    mailbox: "INBOX".to_string(),
                    unread_only: false,
                    received_after_unix_secs: None,
                    limit: 10,
                },
            )
            .expect_err("ambiguous office accounts should fail");
        let message = error.to_string();
        assert!(message.contains("multiple configured accounts"));
        assert!(message.contains("mail-work"));
        assert!(message.contains("mail-personal"));
    }

    #[test]
    fn mail_service_reply_records_runtime_activity() {
        let (service, provider, runtime_store) = build_service();
        let mut http = unavailable_http();
        let summary = service
            .reply(
                &mut http,
                "imap_smtp",
                None,
                "42",
                &MailSendRequest {
                    subject: String::new(),
                    text_body: "Thanks".to_string(),
                    to: Vec::new(),
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    in_reply_to: String::new(),
                    references: String::new(),
                },
            )
            .expect("reply");
        assert_eq!(summary.subject, "Re: hello");
        let requests = provider
            .sent_requests
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(requests[0].to, vec!["reply@example.com".to_string()]);
        assert_eq!(requests[0].in_reply_to, "<msg-1@example.com>");
        assert_eq!(
            requests[0].references,
            "<root@example.com> <msg-1@example.com>"
        );
        let status = runtime_store
            .get("mail-work")
            .expect("runtime status")
            .expect("status exists");
        assert_eq!(status.last_activity_kind, "mail_reply");
        assert!(status.last_activity_ok);
    }

    #[test]
    fn mail_service_draft_records_runtime_activity() {
        let (service, provider, runtime_store) = build_service();
        let mut http = unavailable_http();
        let summary = service
            .draft(
                &mut http,
                "imap_smtp",
                None,
                &MailSendRequest {
                    subject: "Draft".to_string(),
                    text_body: "Body".to_string(),
                    to: Vec::new(),
                    cc: Vec::new(),
                    bcc: Vec::new(),
                    in_reply_to: String::new(),
                    references: String::new(),
                },
            )
            .expect("draft");
        assert_eq!(summary.mailbox, "Drafts");
        assert_eq!(
            provider
                .drafted_requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            1
        );
        let status = runtime_store
            .get("mail-work")
            .expect("runtime status")
            .expect("status exists");
        assert_eq!(status.last_activity_kind, "mail_draft");
        assert!(status.last_activity_ok);
    }
}
