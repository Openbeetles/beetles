use crate::contacts_directory::{
    clamp_contacts_directory_limit, normalize_contact_entry, normalize_match_key,
    slugify_contact_id, ContactEntry, ContactsDirectoryEmailResolution, ContactsDirectoryLookupHit,
    ContactsDirectoryStatus, ContactsDirectoryStore, ContactsDirectoryUpsertResult,
};
use crate::error::{Error, Result};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability,
    OfficeResolveAmbiguity, OfficeResolveAmbiguityReason, OfficeResolveCandidate,
    OfficeResolveRequest, OfficeResolveResult, OfficeService, SnapshotOfficeAuthoritySource,
};
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct ContactsDirectoryService {
    local_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    remote: Option<RemoteContactsDirectoryRuntime>,
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
#[derive(Clone)]
struct RemoteContactsDirectoryRuntime {
    credential_store:
        Arc<dyn crate::contacts_directory::ContactsDirectoryProviderCredentialStore + Send + Sync>,
    providers: crate::contacts_directory::ContactsDirectoryProviderRegistry,
    office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
}

impl ContactsDirectoryService {
    pub fn new(store: Arc<dyn ContactsDirectoryStore + Send + Sync>) -> Self {
        Self {
            local_store: store,
            #[cfg(all(
                feature = "capability_office",
                not(any(target_arch = "xtensa", target_arch = "riscv32"))
            ))]
            remote: None,
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_service(
        local_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
        credential_store: Arc<
            dyn crate::contacts_directory::ContactsDirectoryProviderCredentialStore + Send + Sync,
        >,
        providers: crate::contacts_directory::ContactsDirectoryProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            local_store,
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_authority(
        local_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
        credential_store: Arc<
            dyn crate::contacts_directory::ContactsDirectoryProviderCredentialStore + Send + Sync,
        >,
        providers: crate::contacts_directory::ContactsDirectoryProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        Self {
            local_store,
            remote: Some(RemoteContactsDirectoryRuntime {
                credential_store,
                providers,
                office_authority,
            }),
        }
    }

    pub fn status(&self) -> Result<ContactsDirectoryStatus> {
        let items = self.local_store.list()?;
        Ok(ContactsDirectoryStatus {
            total_contacts: items.len(),
            contacts_with_email: items.iter().filter(|item| !item.emails.is_empty()).count(),
            contacts_with_alias: items.iter().filter(|item| !item.aliases.is_empty()).count(),
            latest_updated_at_unix_secs: items
                .iter()
                .map(|item| item.updated_at_unix_secs)
                .max()
                .unwrap_or(0),
        })
    }

    pub fn list(&self, limit: Option<usize>) -> Result<Vec<ContactEntry>> {
        let limit = clamp_contacts_directory_limit(limit);
        let mut items = self.local_store.list()?;
        items.sort_by_key(|item| {
            (
                normalize_match_key(&item.display_name),
                Reverse(item.updated_at_unix_secs),
                item.id.clone(),
            )
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn lookup(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ContactsDirectoryLookupHit>> {
        self.lookup_with_route(query, limit, None, None)
    }

    pub fn resolve_primary_email(&self, query: &str) -> Result<ContactsDirectoryEmailResolution> {
        self.resolve_primary_email_with_route(query, None, None)
    }

    pub fn upsert(&self, mut contact: ContactEntry) -> Result<ContactsDirectoryUpsertResult> {
        let existing = self.local_store.list()?;
        let existing_ids = existing
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        let created = if contact.id.trim().is_empty() {
            let matching_id = infer_existing_id_from_email(&existing, &contact);
            match matching_id {
                Some(id) => {
                    contact.id = id;
                    false
                }
                None => {
                    contact.id = allocate_contact_id(&contact, &existing_ids);
                    true
                }
            }
        } else {
            let normalized_id = slugify_contact_id(contact.id.trim());
            let exists = existing.iter().any(|item| item.id == normalized_id);
            contact.id = normalized_id;
            !exists
        };

        contact = normalize_contact_entry(contact)?;
        if contact.updated_at_unix_secs == 0 {
            contact.updated_at_unix_secs = crate::util::current_unix_secs();
        }
        self.local_store.upsert(&contact)?;
        Ok(ContactsDirectoryUpsertResult { created, contact })
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let id = slugify_contact_id(id);
        if id.is_empty() {
            return Err(Error::config(
                "contacts_directory_delete",
                "id must not be empty",
            ));
        }
        self.local_store.delete(&id)
    }

    pub fn lookup_with_route(
        &self,
        query: &str,
        limit: Option<usize>,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Vec<ContactsDirectoryLookupHit>> {
        let query = normalize_match_key(query);
        if query.is_empty() {
            return Err(Error::config(
                "contacts_directory_lookup",
                "query must not be empty",
            ));
        }

        let limit = clamp_contacts_directory_limit(limit);
        let mut hits = self
            .local_store
            .list()?
            .into_iter()
            .filter_map(|contact| score_contact_match(&contact, &query, None, None))
            .collect::<Vec<_>>();
        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            hits.extend(self.remote_lookup_hits(&query, limit, provider, account_key)?);
        }
        hits.sort_by_key(|hit| {
            (
                Reverse(hit.score),
                source_priority(&hit.source_kind),
                normalize_match_key(&hit.contact.display_name),
                hit.contact.id.clone(),
            )
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn resolve_primary_email_with_route(
        &self,
        query: &str,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<ContactsDirectoryEmailResolution> {
        let mut hits = self
            .lookup_with_route(query, Some(5), provider, account_key)?
            .into_iter()
            .filter(|hit| !hit.contact.emails.is_empty())
            .collect::<Vec<_>>();
        if hits.is_empty() {
            return Err(Error::config(
                "contacts_directory_email_resolve",
                format!("no contact with email matches query '{query}'"),
            ));
        }
        let best = hits.remove(0);
        if hits
            .first()
            .is_some_and(|candidate| candidate.score == best.score)
        {
            return Err(Error::config(
                "contacts_directory_email_resolve",
                format!("contact query '{query}' is ambiguous"),
            ));
        }
        Ok(ContactsDirectoryEmailResolution {
            query: query.to_string(),
            contact_id: best.contact.id,
            display_name: best.contact.display_name,
            email: best.contact.emails.into_iter().next().unwrap_or_default(),
            match_reason: best.match_reason,
            score: best.score,
            source_kind: best.source_kind,
            provider: best.provider,
            account_key: best.account_key,
        })
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
impl ContactsDirectoryService {
    pub fn provider_names(&self) -> Vec<&'static str> {
        self.remote
            .as_ref()
            .map(|remote| remote.providers.names())
            .unwrap_or_default()
    }

    pub fn list_provider_statuses(
        &self,
    ) -> Result<Vec<crate::contacts_directory::ContactsDirectoryProviderCredentialStatus>> {
        match self.remote.as_ref() {
            Some(remote) => remote.credential_store.list_statuses(),
            None => Ok(Vec::new()),
        }
    }

    pub fn office_default_account_key(&self) -> Result<Option<String>> {
        Ok(self
            .load_office_service()?
            .and_then(|service| service.default_account_key(OfficeCapability::ContactsDirectory)))
    }

    pub fn office_resolve_hint(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeResolveResult>> {
        if account_key.is_some_and(|value| !value.trim().is_empty()) {
            return Ok(None);
        }
        let Some(service) = self.load_office_service()? else {
            return Ok(None);
        };
        let provider = provider.map(str::trim).filter(|value| !value.is_empty());
        if let Some(provider) = provider {
            let candidates = service
                .accounts_for_capability(OfficeCapability::ContactsDirectory)
                .into_iter()
                .filter(|account| account.provider_kind == provider)
                .map(|account| OfficeResolveCandidate::from_account(&account))
                .collect::<Vec<_>>();
            if candidates.len() > 1 {
                return Ok(Some(OfficeResolveResult::Ambiguous(
                    OfficeResolveAmbiguity {
                        reason: OfficeResolveAmbiguityReason::MultipleMatchingAccounts,
                        candidate_accounts: candidates,
                    },
                )));
            }
        }
        match service.resolve(&OfficeResolveRequest {
            capability: OfficeCapability::ContactsDirectory,
            preferred_account_key: None,
            preferred_identity_class: None,
        }) {
            OfficeResolveResult::Selected(_) => Ok(None),
            other => Ok(Some(other)),
        }
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        let accounts = service
            .accounts_for_capability(OfficeCapability::ContactsDirectory)
            .into_iter()
            .map(|account| account.account_key)
            .collect::<std::collections::BTreeSet<_>>();
        Ok(service
            .list_runtime_statuses()?
            .into_iter()
            .filter(|status| accounts.contains(&status.account_key))
            .collect())
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        service.assess_capability_accounts(OfficeCapability::ContactsDirectory, |provider_kind| {
            self.provider_names()
                .iter()
                .any(|name| name == &provider_kind)
        })
    }

    fn remote_lookup_hits(
        &self,
        query: &str,
        limit: usize,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Vec<ContactsDirectoryLookupHit>> {
        let Some(remote) = self.remote.as_ref() else {
            return Ok(Vec::new());
        };
        let route = if provider.is_some() || account_key.is_some() {
            self.resolve_remote_route(provider, account_key)?
        } else {
            match self.resolve_default_remote_route()? {
                Some(route) => route,
                None => return Ok(Vec::new()),
            }
        };
        let provider_impl = remote.providers.get(&route.provider).ok_or_else(|| {
            Error::config(
                "contacts_directory_lookup",
                format!("provider '{}' is not registered", route.provider),
            )
        })?;
        let credential = remote
            .credential_store
            .get(&route.account_key)?
            .ok_or_else(|| {
                Error::config(
                    "contacts_directory_lookup",
                    format!(
                        "provider '{}' has no configured credential for account '{}'",
                        route.provider, route.account_key
                    ),
                )
            })?;
        let result = provider_impl.lookup_contacts(&credential, query, limit);
        self.record_runtime_activity(&route.account_key, "contacts_lookup", result.as_ref().err());
        Ok(result?
            .into_iter()
            .filter_map(|contact| {
                score_contact_match(
                    &contact,
                    query,
                    Some(route.provider.as_str()),
                    Some(route.account_key.as_str()),
                )
            })
            .collect())
    }

    fn resolve_default_remote_route(&self) -> Result<Option<ContactsLookupRoute>> {
        let Some(remote) = self.remote.as_ref() else {
            return Ok(None);
        };
        if let Some(office_service) = self.load_office_service()? {
            if let Some(account_key) =
                office_service.default_account_key(OfficeCapability::ContactsDirectory)
            {
                let credential = remote.credential_store.get(&account_key)?.ok_or_else(|| {
                    Error::config(
                        "contacts_directory_lookup",
                        format!(
                            "office-selected contacts account '{}' has no configured credential",
                            account_key
                        ),
                    )
                })?;
                return Ok(Some(ContactsLookupRoute {
                    provider: credential.provider,
                    account_key,
                }));
            }
        }
        let mut statuses = remote.credential_store.list_statuses()?;
        statuses.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.account_key.cmp(&right.account_key))
        });
        if statuses.len() == 1 {
            let status = statuses.remove(0);
            return Ok(Some(ContactsLookupRoute {
                provider: status.provider,
                account_key: status.account_key,
            }));
        }
        Ok(None)
    }

    fn resolve_remote_route(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<ContactsLookupRoute> {
        let Some(remote) = self.remote.as_ref() else {
            return Err(Error::config(
                "contacts_directory_lookup",
                "remote contacts providers are not configured",
            ));
        };
        if let Some(account_key) = account_key.filter(|value| !value.trim().is_empty()) {
            let credential = remote
                .credential_store
                .get(account_key)?
                .ok_or_else(|| Error::config("contacts_directory_lookup", "unknown account_key"))?;
            if let Some(provider) = provider.filter(|value| !value.trim().is_empty()) {
                if credential.provider != provider {
                    return Err(Error::config(
                        "contacts_directory_lookup",
                        format!(
                            "account '{}' is configured for provider '{}', not '{}'",
                            account_key, credential.provider, provider
                        ),
                    ));
                }
            }
            return Ok(ContactsLookupRoute {
                provider: credential.provider,
                account_key: account_key.to_string(),
            });
        }
        let provider = provider
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::config(
                    "contacts_directory_lookup",
                    "provider or account_key is required for explicit remote lookup",
                )
            })?;
        let mut keys = remote
            .credential_store
            .find_account_keys_by_provider(provider)?;
        keys.sort();
        match keys.len() {
            0 => Err(Error::config(
                "contacts_directory_lookup",
                format!("provider '{}' has no configured credential", provider),
            )),
            1 => Ok(ContactsLookupRoute {
                provider: provider.to_string(),
                account_key: keys.remove(0),
            }),
            _ => {
                if let Some(OfficeResolveResult::Ambiguous(ambiguity)) =
                    self.office_resolve_hint(Some(provider), None)?
                {
                    let candidate_accounts = ambiguity
                        .candidate_accounts
                        .iter()
                        .map(|candidate| {
                            format!("{} ({})", candidate.account_key, candidate.account_label)
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Error::config(
                        "contacts_directory_lookup",
                        format!(
                            "provider '{}' has multiple configured accounts; candidate accounts: {}",
                            provider, candidate_accounts
                        ),
                    ));
                }
                Err(Error::config(
                    "contacts_directory_lookup",
                    format!(
                        "provider '{}' has multiple configured accounts; account_key is required",
                        provider
                    ),
                ))
            }
        }
    }

    fn load_office_service(&self) -> Result<Option<OfficeService>> {
        self.remote
            .as_ref()
            .map(|remote| remote.office_authority.load())
            .transpose()
    }

    fn record_runtime_activity(
        &self,
        account_key: &str,
        activity_kind: &'static str,
        error: Option<&Error>,
    ) {
        let Some(office_service) = self.load_office_service().unwrap_or_else(|load_error| {
            log::warn!(
                "[contacts_runtime] failed to load office authority for {}: {}",
                account_key,
                load_error
            );
            None
        }) else {
            return;
        };
        let now = crate::util::current_unix_secs();
        let mut status = match office_service.runtime_status(account_key) {
            Ok(Some(status)) => status,
            Ok(None) => OfficeAccountRuntimeStatus {
                account_key: account_key.to_string(),
                ..OfficeAccountRuntimeStatus::default()
            },
            Err(load_error) => {
                log::warn!(
                    "[contacts_runtime] failed to load runtime status for {}: {}",
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
                "[contacts_runtime] failed to persist runtime status for {}: {}",
                account_key,
                store_error
            );
        }
    }
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
struct ContactsLookupRoute {
    provider: String,
    account_key: String,
}

fn infer_existing_id_from_email(
    existing: &[ContactEntry],
    contact: &ContactEntry,
) -> Option<String> {
    let primary_email = contact
        .emails
        .first()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())?;
    let mut matches = existing
        .iter()
        .filter(|item| item.emails.iter().any(|email| email == &primary_email));
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first.id.clone())
    }
}

fn allocate_contact_id(contact: &ContactEntry, existing_ids: &BTreeSet<String>) -> String {
    let seed = contact
        .display_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base = slugify_contact_id(if seed.is_empty() {
        contact
            .emails
            .first()
            .map(String::as_str)
            .unwrap_or("contact")
    } else {
        &seed
    });
    if !existing_ids.contains(&base) {
        return base;
    }
    for idx in 2..=9999 {
        let candidate = format!("{base}-{idx}");
        if !existing_ids.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", crate::util::current_unix_secs())
}

fn score_contact_match(
    contact: &ContactEntry,
    query: &str,
    provider: Option<&str>,
    account_key: Option<&str>,
) -> Option<ContactsDirectoryLookupHit> {
    let mut best_score = 0u32;
    let mut best_reason = None::<&str>;

    let name_key = normalize_match_key(&contact.display_name);
    let organization_key = normalize_match_key(&contact.organization);

    update_best_match(
        query,
        &name_key,
        "display_name_exact",
        "display_name_prefix",
        "display_name_contains",
        &mut best_score,
        &mut best_reason,
        420,
        340,
        260,
    );

    for alias in &contact.aliases {
        let alias_key = normalize_match_key(alias);
        update_best_match(
            query,
            &alias_key,
            "alias_exact",
            "alias_prefix",
            "alias_contains",
            &mut best_score,
            &mut best_reason,
            390,
            320,
            240,
        );
    }

    for email in &contact.emails {
        let email_key = normalize_match_key(email);
        update_best_match(
            query,
            &email_key,
            "email_exact",
            "email_prefix",
            "email_contains",
            &mut best_score,
            &mut best_reason,
            520,
            460,
            360,
        );
    }

    if !organization_key.is_empty() && organization_key.contains(query) && best_score < 180 {
        best_score = 180;
        best_reason = Some("organization_contains");
    }

    best_reason.map(|reason| ContactsDirectoryLookupHit {
        contact: contact.clone(),
        match_reason: reason.to_string(),
        score: best_score,
        source_kind: if provider.is_some() {
            "office".to_string()
        } else {
            "local".to_string()
        },
        provider: provider.map(str::to_string),
        account_key: account_key.map(str::to_string),
    })
}

fn source_priority(source_kind: &str) -> u8 {
    match source_kind {
        "local" => 0,
        _ => 1,
    }
}

#[allow(clippy::too_many_arguments)]
fn update_best_match(
    query: &str,
    value: &str,
    exact_reason: &'static str,
    prefix_reason: &'static str,
    contains_reason: &'static str,
    best_score: &mut u32,
    best_reason: &mut Option<&'static str>,
    exact_score: u32,
    prefix_score: u32,
    contains_score: u32,
) {
    if value.is_empty() {
        return;
    }
    let (score, reason) = if value == query {
        (exact_score, exact_reason)
    } else if value.starts_with(query) {
        (prefix_score, prefix_reason)
    } else if value.contains(query) {
        (contains_score, contains_reason)
    } else {
        return;
    };
    if score > *best_score {
        *best_score = score;
        *best_reason = Some(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::ContactsDirectoryService;
    use crate::contacts_directory::{
        ContactEntry, ContactsDirectoryProvider, ContactsDirectoryProviderCredential,
        ContactsDirectoryProviderRegistry, ContactsDirectoryStore,
        OfficeBackedContactsDirectoryProviderCredentialStore, OFFICE_METADATA_CONTACTS_APP_ID,
    };
    use crate::error::Result;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, OfficeService,
    };
    use std::collections::BTreeMap;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryStore {
        items: Mutex<HashMap<String, ContactEntry>>,
    }

    impl ContactsDirectoryStore for MemoryStore {
        fn list(&self) -> Result<Vec<ContactEntry>> {
            Ok(self.items.lock().unwrap().values().cloned().collect())
        }

        fn get(&self, id: &str) -> Result<Option<ContactEntry>> {
            Ok(self.items.lock().unwrap().get(id).cloned())
        }

        fn upsert(&self, contact: &ContactEntry) -> Result<()> {
            self.items
                .lock()
                .unwrap()
                .insert(contact.id.clone(), contact.clone());
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self.items.lock().unwrap().remove(id).is_some())
        }
    }

    fn build_service() -> ContactsDirectoryService {
        let store = Arc::new(MemoryStore::default());
        let service = ContactsDirectoryService::new(store);
        service
            .upsert(ContactEntry {
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@example.com".to_string()],
                aliases: vec!["阿丽丝".to_string()],
                organization: "Beetle".to_string(),
                ..ContactEntry::default()
            })
            .expect("seed alice");
        service
            .upsert(ContactEntry {
                display_name: "Bob Li".to_string(),
                emails: vec!["bob@example.com".to_string()],
                organization: "Other".to_string(),
                ..ContactEntry::default()
            })
            .expect("seed bob");
        service
    }

    #[derive(Default)]
    struct StubOfficeCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubOfficeCredentialStore {
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

    struct StubRemoteProvider {
        contacts: Vec<ContactEntry>,
    }

    impl ContactsDirectoryProvider for StubRemoteProvider {
        fn provider_name(&self) -> &'static str {
            "feishu_contacts_directory"
        }

        fn display_name(&self) -> &'static str {
            "Feishu Contacts Directory"
        }

        fn lookup_contacts(
            &self,
            _credential: &ContactsDirectoryProviderCredential,
            _query: &str,
            _limit: usize,
        ) -> Result<Vec<ContactEntry>> {
            Ok(self.contacts.clone())
        }
    }

    fn build_remote_office_service() -> OfficeService {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "contacts-feishu".to_string(),
            provider_kind: "feishu_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let credential_store = Arc::new(StubOfficeCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "contacts-feishu".to_string(),
                access_token: "app-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [(
                    OFFICE_METADATA_CONTACTS_APP_ID.to_string(),
                    "cli_contacts".to_string(),
                )]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            credential_store,
            Arc::new(StubRuntimeStatusStore),
        )
    }

    #[test]
    fn lookup_prefers_exact_email_matches() {
        let service = build_service();
        let hits = service
            .lookup("alice@example.com", Some(5))
            .expect("lookup contacts");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].match_reason, "email_exact");
        assert_eq!(hits[0].contact.display_name, "Alice Zhang");
    }

    #[test]
    fn lookup_matches_aliases() {
        let service = build_service();
        let hits = service.lookup("阿丽", Some(5)).expect("lookup by alias");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].contact.display_name, "Alice Zhang");
    }

    #[test]
    fn resolve_primary_email_returns_best_unique_match() {
        let service = build_service();
        let resolution = service
            .resolve_primary_email("alice@example.com")
            .expect("resolve email");

        assert_eq!(resolution.contact_id, "alice-zhang");
        assert_eq!(resolution.email, "alice@example.com");
    }

    #[test]
    fn upsert_reuses_existing_id_for_unique_email() {
        let service = build_service();
        let alice = service
            .lookup("alice@example.com", Some(1))
            .unwrap()
            .remove(0);
        let updated = service
            .upsert(ContactEntry {
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@example.com".to_string()],
                notes: "friend".to_string(),
                ..ContactEntry::default()
            })
            .expect("update alice");

        assert!(!updated.created);
        assert_eq!(updated.contact.id, alice.contact.id);
    }

    #[test]
    fn lookup_uses_office_default_feishu_contacts_when_local_store_has_no_match() {
        let local_store = Arc::new(MemoryStore::default());
        let office_service = build_remote_office_service();
        let credential_store = Arc::new(OfficeBackedContactsDirectoryProviderCredentialStore::new(
            office_service.clone(),
        ));
        let mut providers = ContactsDirectoryProviderRegistry::new();
        providers.register(Arc::new(StubRemoteProvider {
            contacts: vec![ContactEntry {
                id: "ou_alice".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@beetle.cn".to_string()],
                aliases: vec!["阿丽丝".to_string()],
                organization: "Beetle".to_string(),
                notes: String::new(),
                updated_at_unix_secs: 1,
            }],
        }));
        let service = ContactsDirectoryService::with_office_service(
            local_store,
            credential_store,
            providers,
            office_service,
        );

        let hits = service
            .lookup("Alice Zhang", Some(5))
            .expect("lookup remote contact");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].contact.id, "ou_alice");
        assert_eq!(hits[0].source_kind, "office");
        assert_eq!(
            hits[0].provider.as_deref(),
            Some("feishu_contacts_directory")
        );
        assert_eq!(hits[0].account_key.as_deref(), Some("contacts-feishu"));

        let resolution = service
            .resolve_primary_email("Alice Zhang")
            .expect("resolve remote email");
        assert_eq!(resolution.contact_id, "ou_alice");
        assert_eq!(resolution.email, "alice@beetle.cn");
        assert_eq!(resolution.source_kind, "office");
        assert_eq!(
            resolution.provider.as_deref(),
            Some("feishu_contacts_directory")
        );
    }
}
