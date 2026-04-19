use crate::error::Result;
#[cfg(feature = "capability_office")]
use crate::office::{assess_office_account, OfficeAccountAssessment};
use crate::office::{
    OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeAccountRuntimeStatus,
    OfficeCapability, OfficeCredential, OfficeCredentialStatus, OfficeCredentialStore,
    OfficeResolveRequest, OfficeResolveResult, OfficeResolver, OfficeRuntimeStatusStore,
    OfficeSelectionPolicy,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountAuthorityStatus {
    pub account_key: String,
    pub provider_kind: String,
    pub external_account_id: String,
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    pub enabled_capabilities: Vec<OfficeCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_status: Option<OfficeCredentialStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<OfficeAccountRuntimeStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAuthoritySummary {
    pub policy: OfficeSelectionPolicy,
    pub accounts: Vec<OfficeAccountAuthorityStatus>,
}

#[derive(Clone)]
pub struct OfficeService {
    registry: OfficeAccountRegistry,
    policy: OfficeSelectionPolicy,
    credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
    runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
}

impl OfficeService {
    pub fn new(
        registry: OfficeAccountRegistry,
        policy: OfficeSelectionPolicy,
        credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
        runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            policy,
            credential_store,
            runtime_status_store,
        }
    }

    pub fn resolve(&self, request: &OfficeResolveRequest) -> OfficeResolveResult {
        let mut request = request.clone();
        if request.historical_account_key.is_none() {
            request.historical_account_key =
                self.historical_account_key_for_request(&request).unwrap_or_else(|error| {
                    log::warn!(
                        "[office_service] failed to derive historical account preference for {:?}: {}",
                        request.capability,
                        error
                    );
                    None
                });
        }
        OfficeResolver::resolve(&self.registry, &self.policy, &request)
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

    #[cfg(feature = "capability_office")]
    pub fn assess_account(
        &self,
        account_key: &str,
        probe_supported: bool,
    ) -> Result<Option<OfficeAccountAssessment>> {
        let Some(account) = self.account(account_key) else {
            return Ok(None);
        };
        let credential = self.credential(account_key)?;
        let runtime_status = self.runtime_status(account_key)?;
        Ok(Some(assess_office_account(
            &account,
            credential.as_ref(),
            runtime_status.as_ref(),
            probe_supported,
        )))
    }

    #[cfg(feature = "capability_office")]
    pub fn assess_capability_accounts<F>(
        &self,
        capability: OfficeCapability,
        mut probe_supported_for_provider: F,
    ) -> Result<Vec<OfficeAccountAssessment>>
    where
        F: FnMut(&str) -> bool,
    {
        let mut items = self
            .accounts_for_capability(capability)
            .into_iter()
            .map(|account| {
                self.assess_account(
                    &account.account_key,
                    probe_supported_for_provider(account.provider_kind.as_str()),
                )
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(items)
    }

    #[cfg(feature = "capability_office")]
    pub fn assess_all_accounts<F>(
        &self,
        mut probe_supported_for_provider: F,
    ) -> Result<Vec<OfficeAccountAssessment>>
    where
        F: FnMut(&str) -> bool,
    {
        let mut items = self
            .registry
            .all_accounts()
            .into_iter()
            .map(|account| {
                self.assess_account(
                    &account.account_key,
                    probe_supported_for_provider(account.provider_kind.as_str()),
                )
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(items)
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
                credential_status: credentials.get(&account.account_key).cloned(),
                runtime_status: runtime_statuses.get(&account.account_key).cloned(),
            })
            .collect::<Vec<_>>();
        Ok(OfficeAuthoritySummary {
            policy: self.policy.clone(),
            accounts,
        })
    }

    fn historical_account_key_for_request(
        &self,
        request: &OfficeResolveRequest,
    ) -> Result<Option<String>> {
        if request
            .preferred_account_key
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(None);
        }

        let provider_filter = request
            .preferred_provider_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let mut ranked = self
            .accounts_for_capability(request.capability)
            .into_iter()
            .filter(|account| {
                provider_filter.is_none_or(|provider| account.provider_kind == provider)
                    && request
                        .preferred_identity_class
                        .is_none_or(|identity| account.identity_class == identity)
            })
            .filter_map(|account| {
                let runtime_status = self.runtime_status(&account.account_key).ok().flatten()?;
                if !runtime_status.last_activity_ok
                    || runtime_status.last_activity_at_unix_secs == 0
                    || !activity_kind_matches_capability(
                        runtime_status.last_activity_kind.as_str(),
                        request.capability,
                    )
                {
                    return None;
                }
                Some((
                    runtime_status.last_activity_at_unix_secs,
                    account.account_key,
                ))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        let Some((best_at, best_account_key)) = ranked.first() else {
            return Ok(None);
        };
        if ranked
            .iter()
            .skip(1)
            .any(|(timestamp, _)| *timestamp == *best_at)
        {
            return Ok(None);
        }
        Ok(Some(best_account_key.clone()))
    }
}

fn activity_kind_matches_capability(activity_kind: &str, capability: OfficeCapability) -> bool {
    match capability {
        OfficeCapability::Mail => activity_kind.starts_with("mail_"),
        OfficeCapability::Calendar => activity_kind.starts_with("calendar_"),
        OfficeCapability::Documents => activity_kind.starts_with("documents_"),
        OfficeCapability::ContactsDirectory => activity_kind.starts_with("contacts_"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::office::OfficeRuntimeStatusStore;
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
    fn summary_joins_runtime_and_credentials_without_legacy_default_metadata() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(work_account());
        registry.insert(personal_account());
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
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 77,
            })
            .expect("set runtime status");

        let service = OfficeService::new(
            registry,
            OfficeSelectionPolicy::default(),
            credential_store,
            runtime_status_store,
        );

        let summary = service.summary().expect("summary");
        let summary_json = serde_json::to_value(&summary).expect("summary json");
        let work = summary
            .accounts
            .iter()
            .find(|item| item.account_key == "calendar-work")
            .expect("work account");
        assert!(summary_json.get("defaults").is_none());
        assert!(summary_json["accounts"][0]
            .get("selected_for_capabilities")
            .is_none());
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
    fn resolve_reports_ambiguity_when_multiple_accounts_share_capability() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(work_account());
        registry.insert(personal_account());

        let service = OfficeService::new(
            registry,
            OfficeSelectionPolicy::default(),
            Arc::new(StubCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore::default()),
        );

        assert_eq!(
            service.resolve(&OfficeResolveRequest {
                capability: OfficeCapability::Calendar,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            }),
            OfficeResolveResult::Ambiguous(crate::office::OfficeResolveAmbiguity {
                reason: crate::office::OfficeResolveAmbiguityReason::MultipleMatchingAccounts,
                candidate_accounts: vec![
                    crate::office::OfficeResolveCandidate {
                        account_key: "calendar-personal".to_string(),
                        provider_kind: "google_calendar".to_string(),
                        account_label: "私人日历".to_string(),
                        identity_class: OfficeAccountIdentityClass::Personal,
                    },
                    crate::office::OfficeResolveCandidate {
                        account_key: "calendar-work".to_string(),
                        provider_kind: "google_calendar".to_string(),
                        account_label: "工作日历".to_string(),
                        identity_class: OfficeAccountIdentityClass::Work,
                    },
                ],
            })
        );
    }

    #[test]
    fn resolve_prefers_historical_successful_activity_before_ambiguity() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(work_account());
        registry.insert(personal_account());

        let credential_store = Arc::new(StubCredentialStore::default());
        for account_key in ["calendar-work", "calendar-personal"] {
            credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
                    access_token: "secret".to_string(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 1,
                    metadata: BTreeMap::new(),
                })
                .expect("seed office credential");
        }

        let runtime_status_store = Arc::new(StubRuntimeStatusStore::default());
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "calendar-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 10,
                last_activity_kind: "calendar_create".to_string(),
                last_activity_ok: true,
                last_activity_at_unix_secs: 200,
                updated_at: 200,
            })
            .expect("set work runtime status");
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "calendar-personal".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 11,
                last_activity_kind: "calendar_create".to_string(),
                last_activity_ok: true,
                last_activity_at_unix_secs: 100,
                updated_at: 100,
            })
            .expect("set personal runtime status");

        let service = OfficeService::new(
            registry,
            OfficeSelectionPolicy {
                ask_when_ambiguous: true,
                preferred_identity_class: None,
            },
            credential_store,
            runtime_status_store,
        );

        assert_eq!(
            service.resolve(&OfficeResolveRequest {
                capability: OfficeCapability::Calendar,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            }),
            OfficeResolveResult::Selected(crate::office::OfficeResolveSelection {
                account_key: "calendar-work".to_string(),
                selection_reason:
                    crate::office::OfficeResolveSelectionReason::HistoricalSuccessfulActivity,
            })
        );
    }
}
