use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailOperation, MailProvider, MailProviderCredential,
    MailProviderCredentialStatus, MailProviderCredentialStore, MailProviderRegistry, MailQuery,
    MailSendRequest,
};
use crate::office::{OfficeAccountRuntimeStatus, OfficeCapability, OfficeService};
use std::sync::Arc;

pub struct MailService {
    credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
    providers: MailProviderRegistry,
    office_service: Option<OfficeService>,
}

impl MailService {
    pub fn new(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
    ) -> Self {
        Self::with_office_service(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self { credential_store, providers, office_service }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(provider.to_string());
        }
        if let Some(office_service) = self.office_service.as_ref() {
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

    pub fn office_default_account_key(&self) -> Option<String> {
        self.office_service
            .as_ref()
            .and_then(|service| service.default_account_key(OfficeCapability::Mail))
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.office_service.as_ref() else {
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

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>> {
        let (provider_impl, credential) = self.resolve_remote(provider, account_key, MailOperation::List)?;
        provider_impl.list_messages(&credential, query)
    }

    pub fn get(&self, provider: &str, account_key: Option<&str>, id: &str) -> Result<Option<MailMessage>> {
        let (provider_impl, credential) = self.resolve_remote(provider, account_key, MailOperation::Get)?;
        provider_impl.get_message(&credential, id)
    }

    pub fn send(
        &self,
        provider: &str,
        account_key: Option<&str>,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary> {
        let (provider_impl, credential) = self.resolve_remote(provider, account_key, MailOperation::Send)?;
        provider_impl.send_message(&credential, request)
    }

    fn resolve_remote(
        &self,
        provider: &str,
        account_key: Option<&str>,
        op: MailOperation,
    ) -> Result<(Arc<dyn MailProvider>, MailProviderCredential)> {
        let provider_impl = self.providers.get(provider).ok_or_else(|| {
            Error::config("mail_provider", format!("provider '{}' is not registered", provider))
        })?;
        if !provider_impl.supports(op) {
            return Err(Error::config(
                "mail_provider",
                format!("provider '{}' does not support {:?}", provider, op),
            ));
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

    fn resolve_account_key(&self, provider: &str, account_key: Option<&str>) -> Result<String> {
        if let Some(account_key) = account_key.filter(|value| !value.trim().is_empty()) {
            return Ok(account_key.to_string());
        }
        if let Some(office_service) = self.office_service.as_ref() {
            if let Some(account_key) =
                resolve_office_default_account_key(office_service, self.credential_store.as_ref(), provider)?
            {
                return Ok(account_key);
            }
        }
        let mut keys = self.credential_store.find_account_keys_by_provider(provider)?;
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
            Ok(self.items.lock().unwrap_or_else(|e| e.into_inner()).get(account_key).cloned())
        }
        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self.items.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect())
        }
        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items.lock().unwrap_or_else(|e| e.into_inner()).insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }
        fn clear(&self, account_key: &str) -> Result<()> {
            self.items.lock().unwrap_or_else(|e| e.into_inner()).remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> { Ok(None) }
        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> { Ok(Vec::new()) }
        fn set(&self, _status: &OfficeAccountRuntimeStatus) -> Result<()> { Ok(()) }
        fn clear(&self, _account_key: &str) -> Result<()> { Ok(()) }
    }

    struct StubProvider;

    impl MailProvider for StubProvider {
        fn provider_name(&self) -> &'static str { "imap_smtp" }
        fn display_name(&self) -> &'static str { "IMAP/SMTP" }
        fn supports(&self, _op: MailOperation) -> bool { true }
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
        fn get_message(&self, credential: &MailProviderCredential, id: &str) -> Result<Option<MailMessage>> {
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
            }))
        }
        fn send_message(
            &self,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
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
    }

    fn build_service() -> MailService {
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
                    ("mail_from_address".to_string(), "work@example.com".to_string()),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy {
                global_default_account_key: "mail-work".to_string(),
                ask_when_ambiguous: false,
                preferred_identity_class: None,
            },
            credential_store.clone(),
            Arc::new(StubRuntimeStatusStore),
        );
        let mail_credentials: Arc<dyn MailProviderCredentialStore + Send + Sync> =
            Arc::new(OfficeBackedMailProviderCredentialStore::new(office.clone()));
        let mut providers = MailProviderRegistry::new();
        providers.register(Arc::new(StubProvider));
        MailService::with_office_service(mail_credentials, providers, Some(office))
    }

    #[test]
    fn mail_service_uses_office_default_account_resolution() {
        let service = build_service();
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
    fn mail_service_provider_status_comes_from_credential_store() {
        let service = build_service();
        let statuses = service.list_provider_statuses().expect("provider statuses");
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].provider, "imap_smtp");
        assert_eq!(statuses[0].from_address, "work@example.com");
    }

    #[test]
    fn mail_service_infers_provider_from_office_default_account() {
        let service = build_service();
        let provider = service
            .resolve_provider_name(None)
            .expect("resolve provider name");
        assert_eq!(provider, "imap_smtp");
    }
}
