//! Documents tool: office-routed document libraries backed by shared office authority.

use crate::documents::{
    summarize_document_read_result, DocumentsEntry, DocumentsProviderCredentialStatus,
    DocumentsProviderCredentialStore, DocumentsProviderRegistry, DocumentsQuery,
    DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery, DocumentsService,
    DocumentsSummaryResult,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability,
    OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::tools::{
    office_diagnostics::{build_account_diagnostics, OfficeAccountDiagnostic},
    office_failure::{build_office_operation_failure_outcome, OfficeOperationFailureInput},
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolContext, ToolEffectClass,
    ToolExecutionOutcome, ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

const DEFAULT_READ_MAX_CHARS: usize = 16_000;
const MAX_READ_MAX_CHARS: usize = 50_000;
const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 50;
const DEFAULT_SEARCH_MAX_READ_BYTES: usize = 256 * 1024;

pub struct DocumentsTool {
    service: DocumentsService,
}

#[derive(Serialize)]
struct DocumentsProviderStatusResponse {
    op: &'static str,
    registered_remote_providers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_documents_account_key: Option<String>,
    configured_providers: Vec<DocumentsProviderCredentialStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_assessments: Vec<OfficeAccountAssessment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_diagnostics: Vec<OfficeAccountDiagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    office_runtime_statuses: Vec<OfficeAccountRuntimeStatus>,
}

#[derive(Serialize)]
struct DocumentsListResponse {
    op: &'static str,
    provider: String,
    count: usize,
    items: Vec<DocumentsEntry>,
}

#[derive(Serialize)]
struct DocumentsReadResponse {
    op: &'static str,
    provider: String,
    document: DocumentsReadResult,
}

#[derive(Serialize)]
struct DocumentsSummaryResponse {
    op: &'static str,
    provider: String,
    summary: DocumentsSummaryResult,
}

#[derive(Serialize)]
struct DocumentsSearchResponse {
    op: &'static str,
    provider: String,
    count: usize,
    hits: Vec<DocumentsSearchHit>,
}

impl DocumentsTool {
    pub fn new(credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>) -> Self {
        Self::with_runtime(credential_store, DocumentsProviderRegistry::new(), None)
    }

    pub fn with_providers(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
    ) -> Self {
        Self::with_runtime(credential_store, providers, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    pub fn with_office_authority(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        Self::with_runtime(credential_store, providers, Some(office_authority))
    }

    fn with_runtime(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            service: DocumentsService::with_office_authority(
                credential_store,
                providers,
                office_authority,
            ),
        }
    }

    fn office_operation_failure(
        &self,
        op: &str,
        provider: Option<&str>,
        account_key: Option<&str>,
        error: &Error,
    ) -> Result<ToolExecutionOutcome> {
        build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_documents",
            op,
            provider,
            account_key,
            capability: OfficeCapability::Documents,
            default_account_key: self.service.office_default_account_key()?,
            account_assessments: self.service.office_account_assessments()?,
            error,
        })
    }

    fn execute_impl(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_documents")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_documents", "missing op"))?;
        match op {
            "provider_status" => {
                let registered_remote_providers = self
                    .service
                    .provider_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let account_assessments = self.service.office_account_assessments()?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_documents",
                    &DocumentsProviderStatusResponse {
                        op: "provider_status",
                        registered_remote_providers,
                        default_documents_account_key: self.service.office_default_account_key()?,
                        configured_providers: self.service.list_provider_statuses()?,
                        account_diagnostics: build_account_diagnostics(&account_assessments),
                        account_assessments,
                        office_runtime_statuses: self.service.office_runtime_statuses()?,
                    },
                )?))
            }
            "list" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self
                    .service
                    .resolve_provider_name(requested_provider.as_deref())
                {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "list",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                let items = match self.service.list(
                    &provider,
                    requested_account_key.as_deref(),
                    DocumentsQuery {
                        path: optional_str(&obj, "path"),
                        limit: parse_limit(obj.get("limit")),
                    },
                ) {
                    Ok(items) => items,
                    Err(error) => {
                        return self.office_operation_failure(
                            "list",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_documents",
                    &DocumentsListResponse {
                        op: "list",
                        provider,
                        count: items.len(),
                        items,
                    },
                )?))
            }
            "read" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self
                    .service
                    .resolve_provider_name(requested_provider.as_deref())
                {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "read",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                let document = match self.service.read(
                    &provider,
                    requested_account_key.as_deref(),
                    required_str(&obj, "path")?,
                    parse_max_chars(obj.get("max_chars"))?,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        return self.office_operation_failure(
                            "read",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_documents",
                    &DocumentsReadResponse {
                        op: "read",
                        provider,
                        document,
                    },
                )?))
            }
            "summarize" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self
                    .service
                    .resolve_provider_name(requested_provider.as_deref())
                {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "summarize",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                let document = match self.service.read(
                    &provider,
                    requested_account_key.as_deref(),
                    required_str(&obj, "path")?,
                    parse_max_chars(obj.get("max_chars"))?,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        return self.office_operation_failure(
                            "summarize",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                let summary = summarize_document_read_result(
                    &document,
                    obj.get("focus").and_then(Value::as_str),
                );
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_documents",
                    &DocumentsSummaryResponse {
                        op: "summarize",
                        provider,
                        summary,
                    },
                )?))
            }
            "search" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self
                    .service
                    .resolve_provider_name(requested_provider.as_deref())
                {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "search",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                let hits = match self.service.search(
                    &provider,
                    requested_account_key.as_deref(),
                    DocumentsSearchQuery {
                        path: optional_str(&obj, "path"),
                        query: required_str(&obj, "query")?.to_string(),
                        limit: parse_limit(obj.get("limit")),
                        case_sensitive: obj
                            .get("case_sensitive")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        max_read_bytes: parse_max_read_bytes(obj.get("max_read_bytes"))?,
                    },
                ) {
                    Ok(hits) => hits,
                    Err(error) => {
                        return self.office_operation_failure(
                            "search",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_documents",
                    &DocumentsSearchResponse {
                        op: "search",
                        provider,
                        count: hits.len(),
                        hits,
                    },
                )?))
            }
            _ => Err(Error::config(
                "tool_documents",
                format!("unknown op '{}'", op),
            )),
        }
    }
}

impl Tool for DocumentsTool {
    fn name(&self) -> &'static str {
        "documents"
    }

    fn description(&self) -> &'static str {
        "Access office document libraries through shared account authority. Ops: provider_status, list, read, summarize, search. Provider can be omitted when office documents defaults or a single configured provider make routing unambiguous."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: provider_status|list|read|summarize|search"},"provider":{"type":"string","description":"Optional documents provider. Omit only when office defaults or a single configured provider make routing unambiguous."},"account_key":{"type":"string","description":"Optional explicit office documents account key."},"path":{"type":"string","description":"Optional directory or file path inside the provider root."},"focus":{"type":"string","description":"Optional phrase to emphasize in summarize output."},"limit":{"type":"integer","description":"List/search limit, default 10, max 50."},"max_chars":{"type":"integer","description":"Maximum characters to return for read or summarize, default 16000, max 50000."},"query":{"type":"string","description":"Search phrase for search."},"case_sensitive":{"type":"boolean","description":"Whether search matching is case-sensitive."},"max_read_bytes":{"type":"integer","description":"Maximum bytes to read per file during content search, default 262144."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        Ok(self.execute_impl(args, ctx)?.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        self.execute_impl(args, ctx)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_effect_class(ToolEffectClass::ReadOnly)
            .with_risk_level(ToolRiskLevel::Low)
            .with_approval_mode(ToolApprovalMode::Automatic)
            .with_rollback_kind(ToolRollbackKind::None)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_documents_governance")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("provider_status");
        Ok(self
            .metadata()
            .default_execution_shape(op)
            .with_approval_granted(true))
    }

    fn requires_network_for(&self, args: &str) -> Result<bool> {
        let obj = parse_tool_args(args, "tool_documents_network")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("provider_status");
        Ok(matches!(op, "list" | "read" | "summarize" | "search"))
    }
}

fn parse_provider(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_account_key(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("account_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn required_str<'a>(obj: &'a serde_json::Map<String, Value>, field: &str) -> Result<&'a str> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::config("tool_documents", format!("missing {}", field)))
}

fn optional_str(obj: &serde_json::Map<String, Value>, field: &str) -> String {
    obj.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn parse_limit(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, MAX_LIMIT as u64) as usize)
        .unwrap_or(DEFAULT_LIMIT)
}

fn parse_max_chars(value: Option<&Value>) -> Result<usize> {
    match value {
        Some(Value::Number(number)) => number
            .as_u64()
            .map(|value| value.clamp(1, MAX_READ_MAX_CHARS as u64) as usize)
            .ok_or_else(|| Error::config("tool_documents", "max_chars must be a positive integer")),
        Some(_) => Err(Error::config(
            "tool_documents",
            "max_chars must be an integer",
        )),
        None => Ok(DEFAULT_READ_MAX_CHARS),
    }
}

fn parse_max_read_bytes(value: Option<&Value>) -> Result<usize> {
    match value {
        Some(Value::Number(number)) => number
            .as_u64()
            .map(|value| value.max(1) as usize)
            .ok_or_else(|| {
                Error::config(
                    "tool_documents",
                    "max_read_bytes must be a positive integer",
                )
            }),
        Some(_) => Err(Error::config(
            "tool_documents",
            "max_read_bytes must be an integer",
        )),
        None => Ok(DEFAULT_SEARCH_MAX_READ_BYTES),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::{
        DocumentsOperation, DocumentsProvider, DocumentsProviderCredential,
        OfficeBackedDocumentsProviderCredentialStore,
    };
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy,
    };
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<HashMap<String, DocumentsProviderCredential>>,
    }

    impl DocumentsProviderCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<DocumentsProviderCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .filter(|credential| credential.provider == provider)
                .map(|credential| credential.account_key.clone())
                .collect())
        }

        fn list_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .map(DocumentsProviderCredential::status)
                .collect())
        }
    }

    #[derive(Default)]
    struct StubOfficeCredentialStore {
        items: Mutex<HashMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubOfficeCredentialStore {
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
        fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(
                (account_key == "docs-work").then_some(OfficeAccountRuntimeStatus {
                    account_key: "docs-work".to_string(),
                    probe_ok: true,
                    last_error: String::new(),
                    last_probe_at_unix_secs: 11,
                    last_activity_kind: String::new(),
                    last_activity_ok: false,
                    last_activity_at_unix_secs: 0,
                    updated_at: 12,
                }),
            )
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(vec![OfficeAccountRuntimeStatus {
                account_key: "docs-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 11,
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 12,
            }])
        }

        fn set(&self, _status: &OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
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
                    "Reports/q1.txt".to_string()
                } else {
                    format!("{}/q1.txt", query.path.trim_matches('/'))
                },
                name: credential.account_label.clone(),
                kind: "text".to_string(),
                is_dir: false,
                content_type: Some("text/plain".to_string()),
                size_bytes: Some(24),
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
                    name: path
                        .rsplit('/')
                        .next()
                        .filter(|value| !value.is_empty())
                        .unwrap_or(credential.account_label.as_str())
                        .to_string(),
                    kind: "text".to_string(),
                    is_dir: false,
                    content_type: Some("text/plain".to_string()),
                    size_bytes: Some(24),
                },
                content: if path.ends_with("quarterly-plan.txt") {
                    "Q1 Review\n- Calendar bridge shipped to remote office calendar\n- Documents summary should feed weekly updates\nAction: send summary to finance\nTODO: create follow-up task for customer review".to_string()
                } else {
                    "quarterly summary".to_string()
                },
                truncated: false,
                raw_bytes: 24,
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
                        "Reports/q1.txt".to_string()
                    } else {
                        format!("{}/q1.txt", query.path.trim_matches('/'))
                    },
                    name: credential.account_label.clone(),
                    kind: "text".to_string(),
                    is_dir: false,
                    content_type: Some("text/plain".to_string()),
                    size_bytes: Some(24),
                },
                match_kind: "content".to_string(),
                snippet: Some(format!("match: {}", query.query)),
                warning: None,
            }])
        }
    }

    fn build_tool() -> DocumentsTool {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                "docs-work".to_string(),
                DocumentsProviderCredential {
                    account_key: "docs-work".to_string(),
                    provider: "webdav".to_string(),
                    account_id: "work@example.com".to_string(),
                    account_label: "Work Docs".to_string(),
                    username: "work@example.com".to_string(),
                    secret: "secret".to_string(),
                    app_id: String::new(),
                    space_id: String::new(),
                    base_url: "https://dav.example.com/root".to_string(),
                    root_path: "/Workspace".to_string(),
                },
            );

        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Documents, "docs-work".to_string());
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        office_credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [(
                    crate::documents::OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                    "https://dav.example.com/root".to_string(),
                )]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            Arc::new(StubRuntimeStatusStore),
        );

        DocumentsTool::with_office_service(credential_store, providers, office_service)
    }

    fn build_office_backed_tool_without_credentials() -> DocumentsTool {
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Documents, "docs-work".to_string());
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            Arc::new(StubRuntimeStatusStore),
        );

        DocumentsTool::with_office_service(
            Arc::new(OfficeBackedDocumentsProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        )
    }

    #[test]
    fn documents_tool_provider_status_reports_defaults_and_runtime() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .expect("provider status");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["registered_remote_providers"][0], "webdav");
        assert_eq!(payload["default_documents_account_key"], "docs-work");
        assert_eq!(payload["configured_providers"][0]["provider"], "webdav");
        assert_eq!(payload["office_runtime_statuses"][0]["probe_ok"], true);
        assert_eq!(
            payload["account_assessments"][0]["account_key"],
            "docs-work"
        );
        assert_eq!(payload["account_assessments"][0]["readiness"], "ready");
        assert_eq!(payload["account_assessments"][0]["next_action"], "none");
        assert_eq!(payload["account_assessments"][0]["probe_supported"], true);
        assert_eq!(
            payload["account_diagnostics"][0]["account_key"],
            "docs-work"
        );
        assert_eq!(payload["account_diagnostics"][0]["diagnosis_kind"], "ready");
        assert_eq!(
            payload["account_diagnostics"][0]["recommended_action"],
            "none"
        );
    }

    #[test]
    fn documents_tool_routes_list_read_and_search_via_office_default() {
        let tool = build_tool();
        let mut ctx = DummyCtx;

        let list = tool
            .execute(r#"{"op":"list","path":"Reports"}"#, &mut ctx)
            .expect("list documents");
        let list: Value = serde_json::from_str(&list).expect("valid list json");
        assert_eq!(list["provider"], "webdav");
        assert_eq!(list["items"][0]["path"], "Reports/q1.txt");

        let read = tool
            .execute(r#"{"op":"read","path":"Reports/q1.txt"}"#, &mut ctx)
            .expect("read document");
        let read: Value = serde_json::from_str(&read).expect("valid read json");
        assert_eq!(read["document"]["content"], "quarterly summary");

        let search = tool
            .execute(r#"{"op":"search","query":"quarterly"}"#, &mut ctx)
            .expect("search documents");
        let search: Value = serde_json::from_str(&search).expect("valid search json");
        assert_eq!(search["hits"][0]["match_kind"], "content");
        assert_eq!(search["hits"][0]["snippet"], "match: quarterly");
    }

    #[test]
    fn documents_tool_summarize_returns_structured_summary_and_handoff() {
        let tool = build_tool();
        let mut ctx = DummyCtx;

        let payload = tool
            .execute(
                r#"{"op":"summarize","path":"Reports/quarterly-plan.txt","focus":"summary"}"#,
                &mut ctx,
            )
            .expect("summarize document");
        let payload: Value = serde_json::from_str(&payload).expect("valid summarize json");
        assert_eq!(payload["provider"], "webdav");
        assert_eq!(payload["summary"]["focus"], "summary");
        assert_eq!(
            payload["summary"]["summary"],
            "Documents summary should feed weekly updates"
        );
        assert_eq!(
            payload["summary"]["key_points"][0],
            "Documents summary should feed weekly updates"
        );
        assert_eq!(
            payload["summary"]["action_items"][0],
            "send summary to finance"
        );
        assert_eq!(
            payload["summary"]["handoff"]["task_candidates"][1],
            "create follow-up task for customer review"
        );
        assert!(payload["summary"]["handoff"]["mail_brief"]
            .as_str()
            .expect("mail brief")
            .contains("Q1 Review"));
    }

    #[test]
    fn documents_tool_list_returns_structured_office_failure_when_default_account_has_no_credential(
    ) {
        let tool = build_office_backed_tool_without_credentials();
        let mut ctx = DummyCtx;

        let outcome = tool
            .execute_outcome(r#"{"op":"list"}"#, &mut ctx)
            .expect("structured failure outcome");
        assert_eq!(
            outcome.failure_kind,
            Some(crate::tools::ToolExecutionFailureKind::Capability)
        );

        let payload: Value =
            serde_json::from_str(&outcome.content).expect("valid failure response json");
        assert_eq!(payload["op"], "list");
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["failure_kind"], "capability");
        assert_eq!(payload["office_assessment"]["capability"], "documents");
        assert_eq!(
            payload["office_assessment"]["default_account_key"],
            "docs-work"
        );
        assert_eq!(
            payload["office_assessment"]["account_assessments"][0]["account_key"],
            "docs-work"
        );
        assert_eq!(
            payload["office_assessment"]["account_assessments"][0]["readiness"],
            "needs_credential_input"
        );
        assert_eq!(
            payload["office_assessment"]["account_assessments"][0]["next_action"],
            "draft_credentials"
        );
        assert_eq!(
            payload["office_assessment"]["account_diagnostics"][0]["account_key"],
            "docs-work"
        );
        assert_eq!(
            payload["office_assessment"]["account_diagnostics"][0]["diagnosis_kind"],
            "needs_credential_input"
        );
        assert_eq!(
            payload["office_assessment"]["account_diagnostics"][0]["recommended_action"],
            "draft_credentials"
        );
        assert!(payload["error"]
            .as_str()
            .expect("error string")
            .contains("no configured credential"));
    }
}
