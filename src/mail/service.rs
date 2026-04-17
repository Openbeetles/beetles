use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailProviderCredentialStatus, MailProviderCredentialStore, MailProviderRegistry, MailQuery,
    MailSearchQuery, MailSendRequest,
};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountIdentityClass, OfficeAccountRuntimeStatus,
    OfficeAuthoritySource, OfficeCapability, OfficeCapabilityRuntime, OfficeHttpClient,
    OfficeResolveResult, OfficeService, SnapshotOfficeAuthoritySource, UnavailableOfficeHttpClient,
};
use std::sync::Arc;

pub struct MailService {
    credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
    providers: MailProviderRegistry,
    office_runtime: OfficeCapabilityRuntime,
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
            office_runtime: OfficeCapabilityRuntime::new(
                OfficeCapability::Mail,
                "mail_provider",
                "mail",
                "mail_runtime",
                office_authority,
            ),
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        self.resolve_provider_name_with_identity(provider, None)
    }

    pub fn resolve_provider_name_with_identity(
        &self,
        provider: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<String> {
        self.office_runtime.resolve_provider_name(
            provider,
            preferred_identity_class,
            self.credential_store.as_ref(),
        )
    }

    pub fn list_provider_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>> {
        self.credential_store.list_statuses()
    }

    pub fn office_default_account_key(&self) -> Result<Option<String>> {
        self.office_runtime.default_account_key()
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
        self.office_runtime
            .resolve_hint(provider, account_key, preferred_identity_class)
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        self.office_runtime.runtime_statuses()
    }

    pub fn office_runtime_status(
        &self,
        account_key: &str,
    ) -> Result<Option<OfficeAccountRuntimeStatus>> {
        self.office_runtime.runtime_status(account_key)
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        self.office_runtime
            .account_assessments(|provider_kind| self.providers.get(provider_kind).is_some())
    }

    pub fn office_identity_class_for_account(
        &self,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeAccountIdentityClass>> {
        self.office_runtime.identity_class_for_account(account_key)
    }

    pub fn provider_supports(&self, provider: &str, op: MailOperation) -> bool {
        self.providers
            .get(provider)
            .is_some_and(|provider_impl| provider_impl.supports(op))
    }

    pub fn provider_is_routable_for_ops(
        &self,
        provider: &str,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        ops: &[MailOperation],
    ) -> bool {
        self.resolve_remote_for_ops(provider, None, preferred_identity_class, ops)
            .is_ok()
    }

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.list_with_http(&mut unavailable_http, provider, account_key, query)
    }

    pub fn list_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.list_with_http_and_identity(http, provider, account_key, None, query)
    }

    pub fn list_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.list_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            query,
        )
    }

    pub fn list_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let (provider_impl, credential) = self.resolve_remote_with_identity(
            provider,
            account_key,
            preferred_identity_class,
            MailOperation::List,
        )?;
        let result = provider_impl.list_messages(http, &credential, query);
        self.record_runtime_activity(&credential.account_key, "mail_list", result.as_ref().err());
        result
    }

    pub fn search(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.search_with_http(&mut unavailable_http, provider, account_key, query)
    }

    pub fn search_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        self.search_with_http_and_identity(http, provider, account_key, None, query)
    }

    pub fn search_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.search_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            query,
        )
    }

    pub fn search_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let (provider_impl, credential) = self.resolve_remote_with_identity(
            provider,
            account_key,
            preferred_identity_class,
            MailOperation::Search,
        )?;
        let result = provider_impl.search_messages(http, &credential, query);
        self.record_runtime_activity(
            &credential.account_key,
            "mail_search",
            result.as_ref().err(),
        );
        result
    }

    pub fn get(
        &self,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.get_with_http(&mut unavailable_http, provider, account_key, id)
    }

    pub fn get_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        self.get_with_http_and_identity(http, provider, account_key, None, id)
    }

    pub fn get_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.get_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            id,
        )
    }

    pub fn get_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        id: &str,
    ) -> Result<Option<MailMessage>> {
        let (provider_impl, credential) = self.resolve_remote_with_identity(
            provider,
            account_key,
            preferred_identity_class,
            MailOperation::Get,
        )?;
        let result = provider_impl.get_message(http, &credential, id);
        self.record_runtime_activity(&credential.account_key, "mail_get", result.as_ref().err());
        result
    }

    pub fn send(
        &self,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.send_with_http(&mut unavailable_http, provider, account_key, request)
    }

    pub fn send_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.send_with_http_and_identity(http, provider, account_key, None, request)
    }

    pub fn send_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.send_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            request,
        )
    }

    pub fn send_with_http_and_identity(
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
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.draft_with_http(&mut unavailable_http, provider, account_key, request)
    }

    pub fn draft_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.draft_with_http_and_identity(http, provider, account_key, None, request)
    }

    pub fn draft_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.draft_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            request,
        )
    }

    pub fn draft_with_http_and_identity(
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
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.reply_with_http(
            &mut unavailable_http,
            provider,
            account_key,
            original_id,
            request,
        )
    }

    pub fn reply_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.reply_with_http_and_identity(http, provider, account_key, None, original_id, request)
    }

    pub fn reply_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.reply_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            original_id,
            request,
        )
    }

    pub fn reply_with_http_and_identity(
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
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.forward_with_http(
            &mut unavailable_http,
            provider,
            account_key,
            original_id,
            request,
        )
    }

    pub fn forward_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        self.forward_with_http_and_identity(http, provider, account_key, None, original_id, request)
    }

    pub fn forward_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        original_id: &str,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.forward_with_http_and_identity(
            &mut unavailable_http,
            provider,
            account_key,
            preferred_identity_class,
            original_id,
            request,
        )
    }

    pub fn forward_with_http_and_identity(
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

    fn resolve_remote_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        op: MailOperation,
    ) -> Result<(Arc<dyn MailProvider>, MailProviderCredential)> {
        self.resolve_remote_for_ops(provider, account_key, preferred_identity_class, &[op])
    }

    fn resolve_remote_for_ops(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
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
        let account_key = self.resolve_account_key_with_identity(
            provider,
            account_key,
            preferred_identity_class,
        )?;
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
        http: &mut dyn OfficeHttpClient,
        route: MutatingActionRoute<'_>,
        execute: F,
    ) -> Result<MailMessageSummary>
    where
        F: FnOnce(
            &mut dyn OfficeHttpClient,
            Arc<dyn MailProvider>,
            &MailProviderCredential,
        ) -> Result<MailMessageSummary>,
    {
        let (provider_impl, credential) = self.resolve_remote_for_ops(
            route.provider,
            route.account_key,
            route.preferred_identity_class,
            route.ops,
        )?;
        let result = execute(http, provider_impl, &credential);
        self.record_runtime_activity(
            &credential.account_key,
            route.activity_kind,
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
        self.office_runtime
            .record_runtime_activity(account_key, activity_kind, error);
    }

    fn resolve_account_key_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<String> {
        self.office_runtime.resolve_account_key(
            provider,
            account_key,
            preferred_identity_class,
            self.credential_store.as_ref(),
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
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            credential_store.clone(),
            Arc::new(StubRuntimeStatusStore::default()),
        );
        let mail_credentials: Arc<dyn MailProviderCredentialStore + Send + Sync> =
            Arc::new(OfficeBackedMailProviderCredentialStore::new(office.clone()));
        let mut providers = MailProviderRegistry::new();
        providers.register(Arc::new(StubProvider::default()));
        let service = MailService::with_office_service(mail_credentials, providers, Some(office));

        let error = service
            .list(
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
