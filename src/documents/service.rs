use crate::documents::{
    DocumentsEntry, DocumentsOperation, DocumentsProviderCredentialStatus,
    DocumentsProviderCredentialStore, DocumentsProviderRegistry, DocumentsQuery,
    DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery,
};
use crate::error::Result;
use crate::office::{
    office_authority_from_service, run_with_unavailable_office_http, OfficeAccountAssessment,
    OfficeAccountIdentityClass, OfficeAccountRuntimeStatus, OfficeAuthoritySource,
    OfficeCapabilityRuntime, OfficeCapabilityServiceCore, OfficeHttpClient, OfficeResolveResult,
    OfficeService,
};
use std::sync::Arc;

type DocumentsRemoteCore = OfficeCapabilityServiceCore<
    DocumentsProviderRegistry,
    dyn DocumentsProviderCredentialStore + Send + Sync,
>;

pub struct DocumentsService {
    core: DocumentsRemoteCore,
}

impl DocumentsService {
    pub fn new(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
    ) -> Self {
        Self::build(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self::build(
            credential_store,
            providers,
            office_authority_from_service(office_service),
        )
    }

    pub fn with_office_authority(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self::build(credential_store, providers, office_authority)
    }

    fn build(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            core: OfficeCapabilityServiceCore::new(
                providers,
                credential_store,
                OfficeCapabilityRuntime::new(
                    crate::office::OfficeCapability::Documents,
                    "documents_provider",
                    "documents",
                    "documents_runtime",
                    office_authority,
                ),
                "documents_provider",
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

    pub fn list_provider_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>> {
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

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        self.core.account_assessments()
    }

    pub fn office_identity_class_for_account(
        &self,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeAccountIdentityClass>> {
        self.core.identity_class_for_account(account_key)
    }

    pub fn provider_supports(&self, provider: &str, op: DocumentsOperation) -> bool {
        self.core.provider_supports(provider, op)
    }

    pub fn provider_is_routable_for_op(
        &self,
        provider: &str,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        op: DocumentsOperation,
    ) -> bool {
        self.core
            .provider_is_routable_for_ops(provider, preferred_identity_class, &[op])
    }

    pub fn list(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        run_with_unavailable_office_http(|http| {
            self.list_with_http(http, provider, account_key, query)
        })
    }

    pub fn list_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        self.list_with_http_and_identity(http, provider, account_key, None, query)
    }

    pub fn list_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        run_with_unavailable_office_http(|http| {
            self.list_with_http_and_identity(
                http,
                provider,
                account_key,
                preferred_identity_class,
                query,
            )
        })
    }

    pub fn list_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: DocumentsQuery,
    ) -> Result<Vec<DocumentsEntry>> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[DocumentsOperation::List],
            "documents_list",
            |provider_impl, credential| provider_impl.list_entries(http, credential, query),
        )
    }

    pub fn read(
        &self,
        provider: &str,
        account_key: Option<&str>,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        run_with_unavailable_office_http(|http| {
            self.read_with_http(http, provider, account_key, path, max_chars)
        })
    }

    pub fn read_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        self.read_with_http_and_identity(http, provider, account_key, None, path, max_chars)
    }

    pub fn read_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        run_with_unavailable_office_http(|http| {
            self.read_with_http_and_identity(
                http,
                provider,
                account_key,
                preferred_identity_class,
                path,
                max_chars,
            )
        })
    }

    pub fn read_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        path: &str,
        max_chars: usize,
    ) -> Result<DocumentsReadResult> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[DocumentsOperation::Read],
            "documents_read",
            |provider_impl, credential| {
                provider_impl.read_document(http, credential, path, max_chars)
            },
        )
    }

    pub fn search(
        &self,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        run_with_unavailable_office_http(|http| {
            self.search_with_http(http, provider, account_key, query)
        })
    }

    pub fn search_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        self.search_with_http_and_identity(http, provider, account_key, None, query)
    }

    pub fn search_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        run_with_unavailable_office_http(|http| {
            self.search_with_http_and_identity(
                http,
                provider,
                account_key,
                preferred_identity_class,
                query,
            )
        })
    }

    pub fn search_with_http_and_identity(
        &self,
        http: &mut dyn OfficeHttpClient,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: DocumentsSearchQuery,
    ) -> Result<Vec<DocumentsSearchHit>> {
        self.core.run_remote_operation(
            provider,
            account_key,
            preferred_identity_class,
            &[DocumentsOperation::Search],
            "documents_search",
            |provider_impl, credential| provider_impl.search_documents(http, credential, query),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{
        DocumentsProvider, DocumentsProviderCredential,
        OfficeBackedDocumentsProviderCredentialStore, OFFICE_METADATA_DOCUMENTS_BASE_URL,
        OFFICE_METADATA_DOCUMENTS_USERNAME,
    };
    use crate::error::Error;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeSelectionPolicy,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            _http: &mut dyn crate::office::OfficeHttpClient,
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
            OfficeSelectionPolicy {
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
            OfficeSelectionPolicy {
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
    fn documents_service_uses_office_resolution_when_provider_has_single_candidate() {
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
