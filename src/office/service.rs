use crate::error::Result;
use crate::office::{
    OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeAccountRuntimeStatus,
    OfficeCapability, OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStatus,
    OfficeCredentialStore, OfficeResolveRequest, OfficeResolveResult, OfficeResolver,
    OfficeRuntimeStatusStore, OfficeSelectionPolicy,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCapabilityDefault {
    pub capability: OfficeCapability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_key: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountAuthorityStatus {
    pub account_key: String,
    pub provider_kind: String,
    pub external_account_id: String,
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    pub enabled_capabilities: Vec<OfficeCapability>,
    pub selected_for_capabilities: Vec<OfficeCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<OfficeCredentialStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<OfficeAccountRuntimeStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAuthoritySummary {
    pub policy: OfficeSelectionPolicy,
    pub defaults: Vec<OfficeCapabilityDefault>,
    pub accounts: Vec<OfficeAccountAuthorityStatus>,
}

#[derive(Clone)]
pub struct OfficeService {
    registry: OfficeAccountRegistry,
    binding: OfficeCapabilityBinding,
    policy: OfficeSelectionPolicy,
    credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
    runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
}

impl OfficeService {
    pub fn new(
        registry: OfficeAccountRegistry,
        binding: OfficeCapabilityBinding,
        policy: OfficeSelectionPolicy,
        credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
        runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            binding,
            policy,
            credential_store,
            runtime_status_store,
        }
    }

    pub fn resolve(&self, request: &OfficeResolveRequest) -> OfficeResolveResult {
        OfficeResolver::resolve(&self.registry, &self.binding, &self.policy, request)
    }

    pub fn default_account_key(&self, capability: OfficeCapability) -> Option<String> {
        match self.resolve(&OfficeResolveRequest {
            capability,
            preferred_account_key: None,
            preferred_identity_class: None,
        }) {
            OfficeResolveResult::Selected(selection) => Some(selection.account_key),
            OfficeResolveResult::Ambiguous | OfficeResolveResult::Missing => None,
        }
    }

    pub fn account(&self, account_key: &str) -> Option<OfficeAccount> {
        self.registry.get(account_key).cloned()
    }

    pub fn accounts_for_capability(&self, capability: OfficeCapability) -> Vec<OfficeAccount> {
        let mut accounts = self
            .registry
            .accounts_for_capability(capability)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        accounts.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        accounts
    }

    pub fn credential(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
        self.credential_store.get(account_key)
    }

    pub fn list_credentials(&self) -> Result<Vec<OfficeCredential>> {
        let mut items = self.credential_store.list()?;
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(items)
    }

    pub fn set_credential(&self, credential: &OfficeCredential) -> Result<()> {
        self.credential_store.set(credential)
    }

    pub fn clear_credential(&self, account_key: &str) -> Result<()> {
        self.credential_store.clear(account_key)
    }

    pub fn runtime_status(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
        self.runtime_status_store.get(account_key)
    }

    pub fn list_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let mut items = self.runtime_status_store.list()?;
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(items)
    }

    pub fn set_runtime_status(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
        self.runtime_status_store.set(status)
    }

    pub fn clear_runtime_status(&self, account_key: &str) -> Result<()> {
        self.runtime_status_store.clear(account_key)
    }

    pub fn summary(&self) -> Result<OfficeAuthoritySummary> {
        let credentials = self
            .list_credentials()?
            .into_iter()
            .map(|credential| (credential.account_key.clone(), credential.status()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let runtime_statuses = self
            .list_runtime_statuses()?
            .into_iter()
            .map(|status| (status.account_key.clone(), status))
            .collect::<std::collections::BTreeMap<_, _>>();
        let defaults = OfficeCapability::all()
            .into_iter()
            .map(|capability| OfficeCapabilityDefault {
                capability,
                account_key: self.default_account_key(capability),
            })
            .collect::<Vec<_>>();
        let selected = defaults
            .iter()
            .filter_map(|item| {
                item.account_key
                    .as_ref()
                    .map(|account_key| (account_key.clone(), item.capability))
            })
            .fold(
                std::collections::BTreeMap::<String, Vec<OfficeCapability>>::new(),
                |mut acc, (account_key, capability)| {
                    acc.entry(account_key).or_default().push(capability);
                    acc
                },
            );
        let accounts = self
            .registry
            .all_accounts()
            .into_iter()
            .map(|account| OfficeAccountAuthorityStatus {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                external_account_id: account.external_account_id.clone(),
                account_label: account.account_label.clone(),
                identity_class: account.identity_class,
                enabled_capabilities: account.enabled_capabilities.clone(),
                selected_for_capabilities: selected
                    .get(&account.account_key)
                    .cloned()
                    .unwrap_or_default(),
                credential_status: credentials.get(&account.account_key).cloned(),
                runtime_status: runtime_statuses.get(&account.account_key).cloned(),
            })
            .collect::<Vec<_>>();
        Ok(OfficeAuthoritySummary {
            policy: self.policy.clone(),
            defaults,
            accounts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::office::{OfficeCapabilityBinding, OfficeRuntimeStatusStore};
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

    fn work_account() -> OfficeAccount {
        OfficeAccount {
            account_key: "calendar-work".to_string(),
            provider_kind: "google_calendar".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "工作日历".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar, OfficeCapability::Mail],
        }
    }

    fn personal_account() -> OfficeAccount {
        OfficeAccount {
            account_key: "calendar-personal".to_string(),
            provider_kind: "google_calendar".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "私人日历".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        }
    }

    #[test]
    fn summary_marks_selected_account_and_joins_runtime_and_credentials() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(work_account());
        registry.insert(personal_account());
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "calendar-work".to_string());
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "calendar-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: "refresh".to_string(),
                token_endpoint: "https://example.com/token".to_string(),
                expires_at_unix_secs: 99,
                updated_at: 88,
                metadata: BTreeMap::from([(
                    crate::office::OFFICE_METADATA_CALENDAR_ID.to_string(),
                    "primary".to_string(),
                )]),
            })
            .expect("set credential");
        let runtime_status_store = Arc::new(StubRuntimeStatusStore::default());
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "calendar-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 77,
                updated_at: 77,
            })
            .expect("set runtime status");

        let service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            credential_store,
            runtime_status_store,
        );

        let summary = service.summary().expect("summary");
        let work = summary
            .accounts
            .iter()
            .find(|item| item.account_key == "calendar-work")
            .expect("work account");
        assert_eq!(
            summary
                .defaults
                .iter()
                .find(|item| item.capability == OfficeCapability::Calendar)
                .and_then(|item| item.account_key.as_deref()),
            Some("calendar-work")
        );
        assert_eq!(
            work.selected_for_capabilities,
            vec![OfficeCapability::Mail, OfficeCapability::Calendar]
        );
        assert_eq!(
            work.credential_status.as_ref().map(|item| item.configured),
            Some(true)
        );
        assert_eq!(
            work.runtime_status.as_ref().map(|item| item.probe_ok),
            Some(true)
        );
    }

    #[test]
    fn default_account_key_returns_none_when_resolution_is_ambiguous() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(work_account());
        registry.insert(personal_account());

        let service = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            Arc::new(StubCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore::default()),
        );

        assert_eq!(service.default_account_key(OfficeCapability::Calendar), None);
    }
}
