use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountIdentityClass, OfficeAccountRuntimeStatus,
    OfficeAuthoritySource, OfficeCapability, OfficeResolveRequest, OfficeResolveResult,
    OfficeService,
};
use crate::util::current_unix_secs;
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct OfficeCapabilityRuntime {
    capability: OfficeCapability,
    config_stage: &'static str,
    selected_account_label: &'static str,
    runtime_log_tag: &'static str,
    office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
}

pub(crate) trait OfficeCapabilityCredentialDirectory: Send + Sync {
    fn provider_for_account(&self, account_key: &str) -> Result<Option<String>>;
    fn configured_provider_names(&self) -> Result<Vec<String>>;
    fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OfficeSelectedRoute {
    pub provider: String,
    pub account_key: String,
}

enum OfficeSelectionAmbiguity<'a> {
    Ignore,
    ErrorForProvider(&'a str),
}

impl OfficeCapabilityRuntime {
    pub(crate) fn new(
        capability: OfficeCapability,
        config_stage: &'static str,
        selected_account_label: &'static str,
        runtime_log_tag: &'static str,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            capability,
            config_stage,
            selected_account_label,
            runtime_log_tag,
            office_authority,
        }
    }

    pub(crate) fn selected_route<C>(
        &self,
        preferred_provider_kind: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        credentials: &C,
    ) -> Result<Option<OfficeSelectedRoute>>
    where
        C: OfficeCapabilityCredentialDirectory + ?Sized,
    {
        self.office_selected_route(
            preferred_provider_kind,
            preferred_identity_class,
            credentials,
            OfficeSelectionAmbiguity::Ignore,
        )
    }

    pub(crate) fn selected_provider_name<C>(
        &self,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        credentials: &C,
    ) -> Result<Option<String>>
    where
        C: OfficeCapabilityCredentialDirectory + ?Sized,
    {
        Ok(self
            .selected_route(None, preferred_identity_class, credentials)?
            .map(|route| route.provider))
    }

    pub(crate) fn resolve_provider_name<C>(
        &self,
        provider: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        credentials: &C,
    ) -> Result<String>
    where
        C: OfficeCapabilityCredentialDirectory + ?Sized,
    {
        if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(provider.to_string());
        }
        if let Some(provider) =
            self.selected_provider_name(preferred_identity_class, credentials)?
        {
            return Ok(provider);
        }
        let mut providers = credentials.configured_provider_names()?;
        providers.sort();
        match providers.len() {
            0 => Err(Error::config(
                self.config_stage,
                format!(
                    "no configured {} provider is available",
                    self.capability_label()
                ),
            )),
            1 => Ok(providers.remove(0)),
            _ => Err(Error::config(
                self.config_stage,
                format!(
                    "multiple configured {} providers are available; provider is required",
                    self.capability_label()
                ),
            )),
        }
    }

    pub(crate) fn default_account_key(&self) -> Result<Option<String>> {
        Ok(self
            .load_service()?
            .and_then(|service| service.default_account_key(self.capability)))
    }

    pub(crate) fn resolve_hint(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<Option<OfficeResolveResult>> {
        if account_key.is_some_and(|value| !value.trim().is_empty()) {
            return Ok(None);
        }
        let Some(service) = self.load_service()? else {
            return Ok(None);
        };
        match service.resolve(&OfficeResolveRequest {
            capability: self.capability,
            preferred_account_key: None,
            preferred_provider_kind: provider
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            preferred_identity_class,
            historical_account_key: None,
        }) {
            OfficeResolveResult::Selected(_) => Ok(None),
            other => Ok(Some(other)),
        }
    }

    pub(crate) fn runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_service()? else {
            return Ok(Vec::new());
        };
        let accounts = service
            .accounts_for_capability(self.capability)
            .into_iter()
            .map(|account| account.account_key)
            .collect::<BTreeSet<_>>();
        Ok(service
            .list_runtime_statuses()?
            .into_iter()
            .filter(|status| accounts.contains(&status.account_key))
            .collect())
    }

    pub(crate) fn runtime_status(
        &self,
        account_key: &str,
    ) -> Result<Option<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_service()? else {
            return Ok(None);
        };
        service.runtime_status(account_key)
    }

    pub(crate) fn account_assessments<F>(
        &self,
        probe_supported_for_provider: F,
    ) -> Result<Vec<OfficeAccountAssessment>>
    where
        F: FnMut(&str) -> bool,
    {
        let Some(service) = self.load_service()? else {
            return Ok(Vec::new());
        };
        service.assess_capability_accounts(self.capability, probe_supported_for_provider)
    }

    pub(crate) fn identity_class_for_account(
        &self,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeAccountIdentityClass>> {
        let Some(account_key) = account_key.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let Some(service) = self.load_service()? else {
            return Ok(None);
        };
        Ok(service
            .account(account_key)
            .map(|account| account.identity_class))
    }

    pub(crate) fn resolve_account_key<C>(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        credentials: &C,
    ) -> Result<String>
    where
        C: OfficeCapabilityCredentialDirectory + ?Sized,
    {
        if let Some(account_key) = account_key.filter(|value| !value.trim().is_empty()) {
            return Ok(account_key.to_string());
        }
        if let Some(route) = self.office_selected_route(
            Some(provider),
            preferred_identity_class,
            credentials,
            OfficeSelectionAmbiguity::ErrorForProvider(provider),
        )? {
            return Ok(route.account_key);
        }
        let mut keys = credentials.account_keys_for_provider(provider)?;
        keys.sort();
        match keys.len() {
            0 => Err(Error::config(
                self.config_stage,
                format!("provider '{}' has no configured credential", provider),
            )),
            1 => Ok(keys.remove(0)),
            _ => Err(Error::config(
                self.config_stage,
                format!(
                    "provider '{}' has multiple configured accounts; account_key is required",
                    provider
                ),
            )),
        }
    }

    pub(crate) fn record_runtime_activity(
        &self,
        account_key: &str,
        activity_kind: &'static str,
        error: Option<&Error>,
    ) {
        let Some(service) = self.load_service().unwrap_or_else(|load_error| {
            log::warn!(
                "[{}] failed to load office authority for {}: {}",
                self.runtime_log_tag,
                account_key,
                load_error
            );
            None
        }) else {
            return;
        };
        let now = current_unix_secs();
        let mut status = match service.runtime_status(account_key) {
            Ok(Some(status)) => status,
            Ok(None) => OfficeAccountRuntimeStatus {
                account_key: account_key.to_string(),
                ..OfficeAccountRuntimeStatus::default()
            },
            Err(load_error) => {
                log::warn!(
                    "[{}] failed to load runtime status for {}: {}",
                    self.runtime_log_tag,
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
        if let Err(store_error) = service.set_runtime_status(&status) {
            log::warn!(
                "[{}] failed to persist runtime status for {}: {}",
                self.runtime_log_tag,
                account_key,
                store_error
            );
        }
    }

    fn load_service(&self) -> Result<Option<OfficeService>> {
        self.office_authority
            .as_ref()
            .map(|authority| authority.load())
            .transpose()
    }

    fn capability_label(&self) -> &'static str {
        match self.capability {
            OfficeCapability::Mail => "mail",
            OfficeCapability::Calendar => "calendar",
            OfficeCapability::Documents => "documents",
            OfficeCapability::ContactsDirectory => "contacts directory",
        }
    }

    fn office_selected_route<C>(
        &self,
        preferred_provider_kind: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        credentials: &C,
        ambiguity: OfficeSelectionAmbiguity<'_>,
    ) -> Result<Option<OfficeSelectedRoute>>
    where
        C: OfficeCapabilityCredentialDirectory + ?Sized,
    {
        let Some(service) = self.load_service()? else {
            return Ok(None);
        };
        match service.resolve(&OfficeResolveRequest {
            capability: self.capability,
            preferred_account_key: None,
            preferred_provider_kind: preferred_provider_kind.map(str::to_string),
            preferred_identity_class,
            historical_account_key: None,
        }) {
            OfficeResolveResult::Selected(selection) => {
                let account_key = selection.account_key;
                let provider =
                    credentials
                        .provider_for_account(&account_key)?
                        .ok_or_else(|| {
                            Error::config(
                                self.config_stage,
                                format!(
                                    "office-selected {} account '{}' has no configured credential",
                                    self.selected_account_label, account_key
                                ),
                            )
                        })?;
                if preferred_provider_kind.is_some_and(|expected| provider != expected) {
                    return Ok(None);
                }
                Ok(Some(OfficeSelectedRoute {
                    provider,
                    account_key,
                }))
            }
            OfficeResolveResult::Ambiguous(ambiguity_result) => match ambiguity {
                OfficeSelectionAmbiguity::Ignore => Ok(None),
                OfficeSelectionAmbiguity::ErrorForProvider(provider) => {
                    let candidate_accounts = ambiguity_result
                        .candidate_accounts
                        .iter()
                        .map(|candidate| {
                            format!("{} ({})", candidate.account_key, candidate.account_label)
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    Err(Error::config(
                        self.config_stage,
                        format!(
                            "provider '{}' has multiple configured accounts; candidate accounts: {}",
                            provider, candidate_accounts
                        ),
                    ))
                }
            },
            OfficeResolveResult::Missing(_) => Ok(None),
        }
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl OfficeCapabilityCredentialDirectory
    for dyn crate::mail::MailProviderCredentialStore + Send + Sync
{
    fn provider_for_account(&self, account_key: &str) -> Result<Option<String>> {
        Ok(self.get(account_key)?.map(|credential| credential.provider))
    }

    fn configured_provider_names(&self) -> Result<Vec<String>> {
        Ok(self
            .list_statuses()?
            .into_iter()
            .map(|status| status.provider)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.find_account_keys_by_provider(provider)
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl OfficeCapabilityCredentialDirectory
    for dyn crate::documents::DocumentsProviderCredentialStore + Send + Sync
{
    fn provider_for_account(&self, account_key: &str) -> Result<Option<String>> {
        Ok(self.get(account_key)?.map(|credential| credential.provider))
    }

    fn configured_provider_names(&self) -> Result<Vec<String>> {
        Ok(self
            .list_statuses()?
            .into_iter()
            .map(|status| status.provider)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.find_account_keys_by_provider(provider)
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl OfficeCapabilityCredentialDirectory
    for dyn crate::calendar::CalendarProviderCredentialStore + Send + Sync
{
    fn provider_for_account(&self, account_key: &str) -> Result<Option<String>> {
        Ok(self.get(account_key)?.map(|credential| credential.provider))
    }

    fn configured_provider_names(&self) -> Result<Vec<String>> {
        Ok(self
            .list_statuses()?
            .into_iter()
            .map(|status| status.provider)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.find_account_keys_by_provider(provider)
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl OfficeCapabilityCredentialDirectory
    for dyn crate::contacts_directory::ContactsDirectoryProviderCredentialStore + Send + Sync
{
    fn provider_for_account(&self, account_key: &str) -> Result<Option<String>> {
        Ok(self.get(account_key)?.map(|credential| credential.provider))
    }

    fn configured_provider_names(&self) -> Result<Vec<String>> {
        Ok(self
            .list_statuses()?
            .into_iter()
            .map(|status| status.provider)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect())
    }

    fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.find_account_keys_by_provider(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::{OfficeCapabilityCredentialDirectory, OfficeCapabilityRuntime};
    use crate::error::Result;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, OfficeService, SnapshotOfficeAuthoritySource,
    };
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(
            &self,
            _account_key: &str,
        ) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }

        fn set(&self, _status: &crate::office::OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    struct StubDirectory {
        provider_by_account: BTreeMap<String, String>,
        providers: Vec<String>,
        keys_by_provider: BTreeMap<String, Vec<String>>,
    }

    impl OfficeCapabilityCredentialDirectory for StubDirectory {
        fn provider_for_account(&self, account_key: &str) -> Result<Option<String>> {
            Ok(self.provider_by_account.get(account_key).cloned())
        }

        fn configured_provider_names(&self) -> Result<Vec<String>> {
            Ok(self.providers.clone())
        }

        fn account_keys_for_provider(&self, provider: &str) -> Result<Vec<String>> {
            Ok(self
                .keys_by_provider
                .get(provider)
                .cloned()
                .unwrap_or_default())
        }
    }

    fn runtime_with_default_mail_account(
        ask_when_ambiguous: bool,
        default_account_key: Option<&str>,
    ) -> OfficeCapabilityRuntime {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        registry.insert(OfficeAccount {
            account_key: "mail-personal".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let mut binding = OfficeCapabilityBinding::default();
        if let Some(default_account_key) = default_account_key {
            binding.set_default_account(OfficeCapability::Mail, default_account_key.to_string());
        }
        let office = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy {
                global_default_account_key: String::new(),
                ask_when_ambiguous,
                preferred_identity_class: None,
            },
            Arc::new(StubCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore),
        );
        OfficeCapabilityRuntime::new(
            OfficeCapability::Mail,
            "mail_provider",
            "mail",
            "mail_runtime",
            Some(Arc::new(SnapshotOfficeAuthoritySource::new(office))),
        )
    }

    #[test]
    fn selected_provider_name_returns_office_selected_provider() {
        let runtime = runtime_with_default_mail_account(false, Some("mail-work"));
        let directory = StubDirectory {
            provider_by_account: [("mail-work".to_string(), "imap_smtp".to_string())]
                .into_iter()
                .collect(),
            providers: vec!["imap_smtp".to_string()],
            keys_by_provider: [(
                "imap_smtp".to_string(),
                vec!["mail-work".to_string(), "mail-personal".to_string()],
            )]
            .into_iter()
            .collect(),
        };

        let provider = runtime
            .selected_provider_name(None, &directory)
            .expect("selected provider");
        assert_eq!(provider.as_deref(), Some("imap_smtp"));
    }

    #[test]
    fn resolve_account_key_reports_ambiguous_candidates_from_office_selection() {
        let runtime = runtime_with_default_mail_account(true, None);
        let directory = StubDirectory {
            provider_by_account: BTreeMap::new(),
            providers: vec!["imap_smtp".to_string()],
            keys_by_provider: [(
                "imap_smtp".to_string(),
                vec!["mail-work".to_string(), "mail-personal".to_string()],
            )]
            .into_iter()
            .collect(),
        };

        let error = runtime
            .resolve_account_key("imap_smtp", None, None, &directory)
            .expect_err("ambiguous office selection should error");
        let message = error.to_string();
        assert!(message.contains("mail-work"));
        assert!(message.contains("mail-personal"));
    }
}
