use crate::documents::{
    DocumentsEntry, DocumentsOperation, DocumentsProvider, DocumentsProviderCredential,
    DocumentsProviderCredentialStatus, DocumentsProviderCredentialStore, DocumentsProviderRegistry,
    DocumentsQuery, DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability,
    OfficeResolveAmbiguity, OfficeResolveAmbiguityReason, OfficeResolveCandidate,
    OfficeResolveRequest, OfficeResolveResult, OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::util::current_unix_secs;
use std::sync::Arc;

pub struct DocumentsService {
    credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
    providers: DocumentsProviderRegistry,
    office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
}

impl DocumentsService {
    pub fn new(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
    ) -> Self {
        Self::with_office_authority(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
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
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
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
            if let Some(account_key) =
                office_service.default_account_key(OfficeCapability::Documents)
            {
                let credential = self.credential_store.get(&account_key)?.ok_or_else(|| {
                    Error::config(
                        "documents_provider",
                        format!(
                            "office-selected documents account '{}' has no configured credential",
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
                "documents_provider",
                "no configured documents provider is available",
            )),
            1 => Ok(providers.remove(0)),
            _ => Err(Error::config(
                "documents_provider",
                "multiple configured documents providers are available; provider is required",
            )),
        }
    }

    pub fn list_provider_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>> {
        self.credential_store.list_statuses()
    }

    pub fn office_default_account_key(&self) -> Result<Option<String>> {
        Ok(self
            .load_office_service()?
            .and_then(|service| service.default_account_key(OfficeCapability::Documents)))
    }

    pub fn office_resolve_hint(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeResolveResult>> {
        if account_key.is_some_and(|value| !value.trim().is_empty()) {
            return Ok(None);
        }
        let Some(office_service) = self.load_office_service()? else {
            return Ok(None);
        };
        let provider = provider.map(str::trim).filter(|value| !value.is_empty());
        if let Some(provider) = provider {
            let candidates = office_service
                .accounts_for_capability(OfficeCapability::Documents)
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
        match office_service.resolve(&OfficeResolveRequest {
            capability: OfficeCapability::Documents,
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
            .accounts_for_capability(OfficeCapability::Documents)
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
        service.assess_capability_accounts(OfficeCapability::Documents, |provider_kind| {
            self.providers.get(provider_kind).is_some()
        })
    }

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, DocumentsOperation::List)?;
        let result = provider_impl.list_entries(&credential, query);
        self.record_runtime_activity(
            &credential.account_key,
            "documents_list",
            result.as_ref().err(),
        );
        result
    }

    pub fn read(
        &self,
        provider: &str,
        account_key: Option<&str>,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, DocumentsOperation::Read)?;
        let result = provider_impl.read_document(&credential, path, max_chars);
        self.record_runtime_activity(
            &credential.account_key,
            "documents_read",
            result.as_ref().err(),
        );
        result
    }

    pub fn search(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, DocumentsOperation::Search)?;
        let result = provider_impl.search_documents(&credential, query);
        self.record_runtime_activity(
            &credential.account_key,
            "documents_search",
            result.as_ref().err(),
        );
        result
    }

    fn resolve_remote(
        &self,
        provider: &str,
        account_key: Option<&str>,
        op: DocumentsOperation,
    ) -> Result<(Arc<dyn DocumentsProvider>, DocumentsProviderCredential)> {
        let provider_impl = self.providers.get(provider).ok_or_else(|| {
            Error::config(
                "documents_provider",
                format!("provider '{}' is not registered", provider),
            )
        })?;
        if !provider_impl.supports(op) {
            return Err(Error::config(
                "documents_provider",
                format!("provider '{}' does not support {:?}", provider, op),
            ));
        }
        let account_key = self.resolve_account_key(provider, account_key)?;
        let credential = self.credential_store.get(&account_key)?.ok_or_else(|| {
            Error::config(
                "documents_provider",
                format!(
                    "provider '{}' has no configured credential for account '{}'",
                    provider, account_key
                ),
            )
        })?;
        if credential.provider != provider {
            return Err(Error::config(
                "documents_provider",
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
                "documents_provider",
                format!("provider '{}' has no configured credential", provider),
            )),
            1 => Ok(keys.remove(0)),
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
                        "documents_provider",
                        format!(
                            "provider '{}' has multiple configured accounts; candidate accounts: {}",
                            provider, candidate_accounts
                        ),
                    ));
                }
                Err(Error::config(
                    "documents_provider",
                    format!(
                        "provider '{}' has multiple configured accounts; account_key is required",
                        provider
                    ),
                ))
            }
        }
    }

    fn load_office_service(&self) -> Result<Option<OfficeService>> {
        self.office_authority
            .as_ref()
            .map(|authority| authority.load())
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
                "[documents_runtime] failed to load office authority for {}: {}",
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
                    "[documents_runtime] failed to load runtime status for {}: {}",
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
                "[documents_runtime] failed to persist runtime status for {}: {}",
                account_key,
                store_error
            );
        }
    }
}

fn resolve_office_default_account_key(
    office_service: &OfficeService,
    credential_store: &(dyn DocumentsProviderCredentialStore + Send + Sync),
    provider: &str,
) -> Result<Option<String>> {
    let Some(account_key) = office_service.default_account_key(OfficeCapability::Documents) else {
        return Ok(None);
    };
    let Some(credential) = credential_store.get(&account_key)? else {
        return Err(Error::config(
            "documents_provider",
            format!(
                "office-selected documents account '{}' has no configured credential",
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
    use crate::documents::{
        OfficeBackedDocumentsProviderCredentialStore, OFFICE_METADATA_DOCUMENTS_BASE_URL,
        OFFICE_METADATA_DOCUMENTS_USERNAME,
    };
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy,
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
    struct MemoryRuntimeStatusStore {
        items: Mutex<BTreeMap<String, OfficeAccountRuntimeStatus>>,
    }

    impl OfficeRuntimeStatusStore for MemoryRuntimeStatusStore {
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

    struct StubProvider;

    impl DocumentsProvider for StubProvider {
        fn provider_name(&self) -> &'static str {
            "webdav"
        }
        fn display_name(&self) -> &'static str {
            "WebDAV"
        }
        fn supports(&self, _op: DocumentsOperation) -> bool {
            true
        }
        fn list_entries(
            &self,
            credential: &DocumentsProviderCredential,
            query: DocumentsQuery,
        ) -> Result<Vec<DocumentsEntry>> {
            Ok(vec![DocumentsEntry {
                path: if query.path.is_empty() {
                    "report.txt".to_string()
                } else {
                    format!("{}/report.txt", query.path.trim_matches('/'))
                },
                name: credential.account_label.clone(),
                kind: "text".to_string(),
                is_dir: false,
                content_type: Some("text/plain".to_string()),
                size_bytes: Some(12),
            }])
        }
        fn read_document(
            &self,
            credential: &DocumentsProviderCredential,
            path: &str,
            _max_chars: usize,
        ) -> Result<DocumentsReadResult> {
            Ok(DocumentsReadResult {
                entry: DocumentsEntry {
                    path: path.to_string(),
                    name: credential.account_label.clone(),
                    kind: "text".to_string(),
                    is_dir: false,
                    content_type: Some("text/plain".to_string()),
                    size_bytes: Some(4),
                },
                content: "body".to_string(),
                truncated: false,
                raw_bytes: 4,
                warning: None,
            })
        }
        fn search_documents(
            &self,
            credential: &DocumentsProviderCredential,
            query: DocumentsSearchQuery,
        ) -> Result<Vec<DocumentsSearchHit>> {
            Ok(vec![DocumentsSearchHit {
                entry: DocumentsEntry {
                    path: if query.path.is_empty() {
                        "report.txt".to_string()
                    } else {
                        format!("{}/report.txt", query.path.trim_matches('/'))
                    },
                    name: credential.account_label.clone(),
                    kind: "text".to_string(),
                    is_dir: false,
                    content_type: Some("text/plain".to_string()),
                    size_bytes: Some(12),
                },
                match_kind: "content".to_string(),
                snippet: Some(query.query),
                warning: None,
            }])
        }
    }

    struct FailingProvider;

    impl DocumentsProvider for FailingProvider {
        fn provider_name(&self) -> &'static str {
            "webdav"
        }

        fn display_name(&self) -> &'static str {
            "WebDAV"
        }

        fn supports(&self, _op: DocumentsOperation) -> bool {
            true
        }

        fn list_entries(
            &self,
            _credential: &DocumentsProviderCredential,
            _query: DocumentsQuery,
        ) -> Result<Vec<DocumentsEntry>> {
            Err(Error::config(
                "documents_provider_test",
                "remote list failed",
            ))
        }

        fn read_document(
            &self,
            _credential: &DocumentsProviderCredential,
            _path: &str,
            _max_chars: usize,
        ) -> Result<DocumentsReadResult> {
            Err(Error::config(
                "documents_provider_test",
                "remote read failed",
            ))
        }

        fn search_documents(
            &self,
            _credential: &DocumentsProviderCredential,
            _query: DocumentsSearchQuery,
        ) -> Result<Vec<DocumentsSearchHit>> {
            Err(Error::config(
                "documents_provider_test",
                "remote search failed",
            ))
        }
    }

    fn build_service() -> DocumentsService {
        build_service_with_runtime_store(Arc::new(MemoryRuntimeStatusStore::default())).0
    }

    fn build_service_with_runtime_store(
        runtime_status_store: Arc<MemoryRuntimeStatusStore>,
    ) -> (DocumentsService, Arc<MemoryRuntimeStatusStore>) {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    (
                        OFFICE_METADATA_DOCUMENTS_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                        "https://dav.example.com/root".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy {
                global_default_account_key: "docs-work".to_string(),
                ask_when_ambiguous: false,
                preferred_identity_class: None,
            },
            credential_store.clone(),
            runtime_status_store.clone(),
        );
        let docs_credentials: Arc<dyn DocumentsProviderCredentialStore + Send + Sync> = Arc::new(
            OfficeBackedDocumentsProviderCredentialStore::new(office.clone()),
        );
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));
        (
            DocumentsService::with_office_service(docs_credentials, providers, Some(office)),
            runtime_status_store,
        )
    }

    fn build_failing_service_with_runtime_store(
        runtime_status_store: Arc<MemoryRuntimeStatusStore>,
    ) -> (DocumentsService, Arc<MemoryRuntimeStatusStore>) {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    (
                        OFFICE_METADATA_DOCUMENTS_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                        "https://dav.example.com/root".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy {
                global_default_account_key: "docs-work".to_string(),
                ask_when_ambiguous: false,
                preferred_identity_class: None,
            },
            credential_store.clone(),
            runtime_status_store.clone(),
        );
        let docs_credentials: Arc<dyn DocumentsProviderCredentialStore + Send + Sync> = Arc::new(
            OfficeBackedDocumentsProviderCredentialStore::new(office.clone()),
        );
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(FailingProvider));
        (
            DocumentsService::with_office_service(docs_credentials, providers, Some(office)),
            runtime_status_store,
        )
    }

    #[test]
    fn documents_service_uses_office_default_account_resolution() {
        let service = build_service();
        let items = service
            .list(
                "webdav",
                None,
                DocumentsQuery {
                    path: String::new(),
                    limit: 10,
                },
            )
            .expect("list documents");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "report.txt");
    }

    #[test]
    fn documents_service_records_runtime_activity_for_remote_operations() {
        let (service, runtime_status_store) =
            build_service_with_runtime_store(Arc::new(MemoryRuntimeStatusStore::default()));

        let _ = service
            .list(
                "webdav",
                None,
                DocumentsQuery {
                    path: String::new(),
                    limit: 10,
                },
            )
            .expect("list documents");
        let status = runtime_status_store
            .get("docs-work")
            .expect("load runtime after list")
            .expect("runtime status after list");
        assert_eq!(status.last_activity_kind, "documents_list");
        assert!(status.last_activity_ok);
        assert!(status.last_error.is_empty());

        let _ = service
            .read("webdav", None, "report.txt", 1_000)
            .expect("read document");
        let status = runtime_status_store
            .get("docs-work")
            .expect("load runtime after read")
            .expect("runtime status after read");
        assert_eq!(status.last_activity_kind, "documents_read");
        assert!(status.last_activity_ok);
        assert!(status.last_error.is_empty());

        let _ = service
            .search(
                "webdav",
                None,
                DocumentsSearchQuery {
                    path: String::new(),
                    query: "body".to_string(),
                    limit: 10,
                    case_sensitive: false,
                    max_read_bytes: 1_024,
                },
            )
            .expect("search documents");
        let status = runtime_status_store
            .get("docs-work")
            .expect("load runtime after search")
            .expect("runtime status after search");
        assert_eq!(status.last_activity_kind, "documents_search");
        assert!(status.last_activity_ok);
        assert!(status.last_error.is_empty());
    }

    #[test]
    fn documents_service_records_runtime_failure_for_remote_operations() {
        let (service, runtime_status_store) =
            build_failing_service_with_runtime_store(Arc::new(MemoryRuntimeStatusStore::default()));

        let error = service
            .read("webdav", None, "report.txt", 1_000)
            .expect_err("failing provider should surface error");
        assert!(error.to_string().contains("remote read failed"));

        let status = runtime_status_store
            .get("docs-work")
            .expect("load runtime after failure")
            .expect("runtime status after failure");
        assert_eq!(status.last_activity_kind, "documents_read");
        assert!(!status.last_activity_ok);
        assert!(status.last_error.contains("remote read failed"));
    }
}
