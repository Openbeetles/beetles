use crate::documents::{
    DocumentsEntry, DocumentsOperation, DocumentsProvider, DocumentsProviderCredential,
    DocumentsProviderCredentialStatus, DocumentsProviderCredentialStore, DocumentsProviderRegistry,
    DocumentsQuery, DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery,
};
use crate::error::{Error, Result};
use crate::office::{OfficeAccountRuntimeStatus, OfficeCapability, OfficeService};
use std::sync::Arc;

pub struct DocumentsService {
    credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
    providers: DocumentsProviderRegistry,
    office_service: Option<OfficeService>,
}

impl DocumentsService {
    pub fn new(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
    ) -> Self {
        Self::with_office_service(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self {
            credential_store,
            providers,
            office_service,
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(provider.to_string());
        }
        if let Some(office_service) = self.office_service.as_ref() {
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

    pub fn office_default_account_key(&self) -> Option<String> {
        self.office_service
            .as_ref()
            .and_then(|service| service.default_account_key(OfficeCapability::Documents))
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.office_service.as_ref() else {
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

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, DocumentsOperation::List)?;
        provider_impl.list_entries(&credential, query)
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
        provider_impl.read_document(&credential, path, max_chars)
    }

    pub fn search(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, DocumentsOperation::Search)?;
        provider_impl.search_documents(&credential, query)
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
        if let Some(office_service) = self.office_service.as_ref() {
            if let Some(account_key) = resolve_office_default_account_key(
                office_service,
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
            _ => Err(Error::config(
                "documents_provider",
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
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }
        fn set(&self, _status: &OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _account_key: &str) -> Result<()> {
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

    fn build_service() -> DocumentsService {
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
            Arc::new(StubRuntimeStatusStore),
        );
        let docs_credentials: Arc<dyn DocumentsProviderCredentialStore + Send + Sync> = Arc::new(
            OfficeBackedDocumentsProviderCredentialStore::new(office.clone()),
        );
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));
        DocumentsService::with_office_service(docs_credentials, providers, Some(office))
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
}
