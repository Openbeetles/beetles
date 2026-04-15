use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailProviderCredentialStatus, MailProviderCredentialStore, MailProviderRegistry, MailQuery,
    MailSearchQuery, MailSendRequest,
};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability,
    OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::util::current_unix_secs;
use std::sync::Arc;

pub struct MailService {
    credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
    providers: MailProviderRegistry,
    office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
}

impl MailService {
    pub fn new(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
    ) -> Self {
        Self::with_office_authority(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self::with_office_authority(
            credential_store,
            providers,
            office_service.map(|office| {
                Arc::new(SnapshotOfficeAuthoritySource::new(office))
                    as Arc<dyn OfficeAuthoritySource + Send + Sync>
            }),
        )
    }

    pub fn with_office_authority(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            credential_store,
            providers,
            office_authority,
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(provider.to_string());
        }
        if let Some(office_service) = self.load_office_service()? {
            if let Some(account_key) = office_service.default_account_key(OfficeCapability::Mail) {
                let credential = self.credential_store.get(&account_key)?.ok_or_else(|| {
                    Error::config(
                        "mail_provider",
                        format!(
                            "office-selected mail account '{}' has no configured credential",
                            account_key
                        ),
                    )
                })?;
                return Ok(credential.provider);
            }
        }
        let mut providers = self
            .list_provider_statuses()?
            .into_iter()
            .map(|status| status.provider)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        match providers.len() {
            0 => Err(Error::config(
                "mail_provider",
                "no configured mail provider is available",
            )),
            1 => Ok(providers.remove(0)),
            _ => Err(Error::config(
                "mail_provider",
                "multiple configured mail providers are available; provider is required",
            )),
        }
    }

    pub fn list_provider_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>> {
        self.credential_store.list_statuses()
    }

    pub fn office_default_account_key(&self) -> Result<Option<String>> {
        Ok(self
            .load_office_service()?
            .and_then(|service| service.default_account_key(OfficeCapability::Mail)))
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        let mail_accounts = service
            .accounts_for_capability(OfficeCapability::Mail)
            .into_iter()
            .map(|account| account.account_key)
            .collect::<std::collections::BTreeSet<_>>();
        Ok(service
            .list_runtime_statuses()?
            .into_iter()
            .filter(|status| mail_accounts.contains(&status.account_key))
            .collect())
    }

    pub fn office_runtime_status(
        &self,
        account_key: &str,
    ) -> Result<Option<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(None);
        };
        service.runtime_status(account_key)
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        service.assess_capability_accounts(OfficeCapability::Mail, |provider_kind| {
            self.providers.get(provider_kind).is_some()
        })
    }

    pub fn provider_supports(&self, provider: &str, op: MailOperation) -> bool {
        self.providers
            .get(provider)
            .is_some_and(|provider_impl| provider_impl.supports(op))
    }

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, MailOperation::List)?;
        provider_impl.list_messages(&credential, query)
    }

    pub fn search(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, MailOperation::Search)?;
        provider_impl.search_messages(&credential, query)
    }

    pub fn get(
        &self,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, MailOperation::Get)?;
        provider_impl.get_message(&credential, id)
    }

    pub fn send(
        &self,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            provider,
            account_key,
            &[MailOperation::Send],
            "mail_send",
            |provider_impl, credential| provider_impl.send_message(credential, request),
        )
    }

    pub fn draft(
        &self,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            provider,
            account_key,
            &[MailOperation::Draft],
            "mail_draft",
            |provider_impl, credential| provider_impl.draft_message(credential, request),
        )
    }

    pub fn reply(
        &self,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            provider,
            account_key,
            &[MailOperation::Get, MailOperation::Send],
            "mail_reply",
            |provider_impl, credential| {
                let original = provider_impl
                    .get_message(credential, original_id)?
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
                provider_impl.send_message(credential, &composed)
            },
        )
    }

    pub fn forward(
        &self,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.execute_mutating_action(
            provider,
            account_key,
            &[MailOperation::Get, MailOperation::Send],
            "mail_forward",
            |provider_impl, credential| {
                let original = provider_impl
                    .get_message(credential, original_id)?
                    .ok_or_else(|| Error::config("mail_forward", "original message not found"))?;
                let mut composed = request.clone();
                composed.subject =
                    compose_forward_subject(&request.subject, &original.summary.subject);
                composed.text_body = render_forward_body(&request.text_body, &original);
                provider_impl.send_message(credential, &composed)
            },
        )
    }

    fn resolve_remote(
        &self,
        provider: &str,
        account_key: Option<&str>,
        op: MailOperation,
    ) -> Result<(Arc<dyn MailProvider>, MailProviderCredential)> {
        self.resolve_remote_for_ops(provider, account_key, &[op])
    }

    fn resolve_remote_for_ops(
        &self,
        provider: &str,
        account_key: Option<&str>,
        ops: &[MailOperation],
    ) -> Result<(Arc<dyn MailProvider>, MailProviderCredential)> {
        let provider_impl = self.providers.get(provider).ok_or_else(|| {
            Error::config(
                "mail_provider",
                format!("provider '{}' is not registered", provider),
            )
        })?;
        for op in ops {
            if !provider_impl.supports(*op) {
                return Err(Error::config(
                    "mail_provider",
                    format!("provider '{}' does not support {:?}", provider, op),
                ));
            }
        }
        let account_key = self.resolve_account_key(provider, account_key)?;
        let credential = self.credential_store.get(&account_key)?.ok_or_else(|| {
            Error::config(
                "mail_provider",
                format!(
                    "provider '{}' has no configured credential for account '{}'",
                    provider, account_key
                ),
            )
        })?;
        if credential.provider != provider {
            return Err(Error::config(
                "mail_provider",
                format!(
                    "account '{}' is configured for provider '{}', not '{}'",
                    account_key, credential.provider, provider
                ),
            ));
        }
        Ok((provider_impl, credential))
    }

    fn execute_mutating_action<F>(
        &self,
        provider: &str,
        account_key: Option<&str>,
        ops: &[MailOperation],
        activity_kind: &'static str,
        execute: F,
    ) -> Result<MailMessageSummary>
    where
        F: FnOnce(Arc<dyn MailProvider>, &MailProviderCredential) -> Result<MailMessageSummary>,
    {
        let (provider_impl, credential) =
            self.resolve_remote_for_ops(provider, account_key, ops)?;
        let result = execute(provider_impl, &credential);
        self.record_runtime_activity(
            &credential.account_key,
            activity_kind,
            result.as_ref().err(),
        );
        result
    }

    fn record_runtime_activity(
        &self,
        account_key: &str,
        activity_kind: &'static str,
        error: Option<&Error>,
    ) {
        let Some(office_service) = self.load_office_service().unwrap_or_else(|load_error| {
            log::warn!(
                "[mail_runtime] failed to load office authority for {}: {}",
                account_key,
                load_error
            );
            None
        }) else {
            return;
        };
        let now = current_unix_secs();
        let mut status = match office_service.runtime_status(account_key) {
            Ok(Some(status)) => status,
            Ok(None) => OfficeAccountRuntimeStatus {
                account_key: account_key.to_string(),
                ..OfficeAccountRuntimeStatus::default()
            },
            Err(load_error) => {
                log::warn!(
                    "[mail_runtime] failed to load runtime status for {}: {}",
                    account_key,
                    load_error
                );
                return;
            }
        };
        status.account_key = account_key.to_string();
        status.last_activity_kind = activity_kind.to_string();
        status.last_activity_ok = error.is_none();
        status.last_activity_at_unix_secs = now;
        status.updated_at = now;
        if let Some(error) = error {
            status.last_error = error.to_string();
        } else {
            status.last_error.clear();
            status.probe_ok = true;
        }
        if let Err(store_error) = office_service.set_runtime_status(&status) {
            log::warn!(
                "[mail_runtime] failed to persist runtime status for {}: {}",
                account_key,
                store_error
            );
        }
    }

    fn resolve_account_key(&self, provider: &str, account_key: Option<&str>) -> Result<String> {
        if let Some(account_key) = account_key.filter(|value| !value.trim().is_empty()) {
            return Ok(account_key.to_string());
        }
        if let Some(office_service) = self.load_office_service()? {
            if let Some(account_key) = resolve_office_default_account_key(
                &office_service,
                self.credential_store.as_ref(),
                provider,
            )? {
                return Ok(account_key);
            }
        }
        let mut keys = self
            .credential_store
            .find_account_keys_by_provider(provider)?;
        keys.sort();
        match keys.len() {
            0 => Err(Error::config(
                "mail_provider",
                format!("provider '{}' has no configured credential", provider),
            )),
            1 => Ok(keys.remove(0)),
            _ => Err(Error::config(
                "mail_provider",
                format!(
                    "provider '{}' has multiple configured accounts; account_key is required",
                    provider
                ),
            )),
        }
    }

    fn load_office_service(&self) -> Result<Option<OfficeService>> {
        self.office_authority
            .as_ref()
            .map(|authority| authority.load())
            .transpose()
    }
}

fn resolve_office_default_account_key(
    office_service: &OfficeService,
    credential_store: &(dyn MailProviderCredentialStore + Send + Sync),
    provider: &str,
) -> Result<Option<String>> {
    let Some(account_key) = office_service.default_account_key(OfficeCapability::Mail) else {
        return Ok(None);
    };
    let Some(credential) = credential_store.get(&account_key)? else {
        return Err(Error::config(
            "mail_provider",
            format!(
                "office-selected mail account '{}' has no configured credential",
                account_key
            ),
        ));
    };
    if credential.provider == provider {
        Ok(Some(account_key))
    } else {
        Ok(None)
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
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy,
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
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy {
                global_default_account_key: "mail-work".to_string(),
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
    fn mail_service_uses_office_default_account_resolution() {
        let (service, _provider, _runtime_store) = build_service();
        let items = service
            .list(
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
    fn mail_service_search_uses_office_default_account_resolution() {
        let (service, _provider, _runtime_store) = build_service();
        let items = service
            .search(
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
    fn mail_service_infers_provider_from_office_default_account() {
        let (service, _provider, _runtime_store) = build_service();
        let provider = service
            .resolve_provider_name(None)
            .expect("resolve provider name");
        assert_eq!(provider, "imap_smtp");
    }

    #[test]
    fn mail_service_reply_records_runtime_activity() {
        let (service, provider, runtime_store) = build_service();
        let summary = service
            .reply(
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
        let summary = service
            .draft(
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
