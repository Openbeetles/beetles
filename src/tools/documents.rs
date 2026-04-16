//! Documents tool: office-routed document libraries backed by shared office authority.

use crate::contacts_directory::{
    ContactEntry, ContactsDirectoryLookupHit, ContactsDirectoryService, ContactsDirectoryStore,
};
use crate::documents::{
    summarize_document_read_result, DocumentsEntry, DocumentsOperation,
    DocumentsProviderCredentialStatus, DocumentsProviderCredentialStore, DocumentsProviderRegistry,
    DocumentsQuery, DocumentsReadResult, DocumentsSearchHit, DocumentsSearchQuery,
    DocumentsService, DocumentsSummaryResult,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountIdentityClass, OfficeAccountRuntimeStatus,
    OfficeAuthoritySource, OfficeCapability, OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::tools::{
    office_args::parse_preferred_identity_class,
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
    contacts_directory: Option<ContactsDirectoryService>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contexts: Vec<DocumentsResolvedContext>,
}

#[derive(Serialize)]
struct DocumentsReadResponse {
    op: &'static str,
    provider: String,
    document: DocumentsReadResult,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contexts: Vec<DocumentsResolvedContext>,
}

#[derive(Serialize)]
struct DocumentsSummaryResponse {
    op: &'static str,
    provider: String,
    summary: DocumentsSummaryResult,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contexts: Vec<DocumentsResolvedContext>,
}

#[derive(Serialize)]
struct DocumentsSearchResponse {
    op: &'static str,
    provider: String,
    count: usize,
    hits: Vec<DocumentsSearchHit>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contexts: Vec<DocumentsResolvedContext>,
}

#[derive(Serialize)]
struct DocumentsResolvedContext {
    query: String,
    contact_id: String,
    display_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    organization: String,
    match_reason: String,
    score: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_class: Option<String>,
}

impl DocumentsTool {
    pub fn new(credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>) -> Self {
        Self::with_runtime(
            credential_store,
            DocumentsProviderRegistry::new(),
            None,
            None,
        )
    }

    pub fn with_providers(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
    ) -> Self {
        Self::with_runtime(credential_store, providers, None, None)
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
        Self::with_runtime(credential_store, providers, Some(office_authority), None)
    }

    pub fn with_office_authority_and_contacts(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
        contacts_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
    ) -> Self {
        Self::with_runtime(
            credential_store,
            providers,
            Some(office_authority),
            Some(ContactsDirectoryService::new(contacts_store)),
        )
    }

    pub fn with_office_authority_and_contacts_service(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
        contacts_directory: ContactsDirectoryService,
    ) -> Self {
        Self::with_runtime(
            credential_store,
            providers,
            Some(office_authority),
            Some(contacts_directory),
        )
    }

    fn with_runtime(
        credential_store: Arc<dyn DocumentsProviderCredentialStore + Send + Sync>,
        providers: DocumentsProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
        contacts_directory: Option<ContactsDirectoryService>,
    ) -> Self {
        Self {
            service: DocumentsService::with_office_authority(
                credential_store,
                providers,
                office_authority,
            ),
            contacts_directory,
        }
    }

    fn office_operation_failure(
        &self,
        op: &str,
        provider: Option<&str>,
        account_key: Option<&str>,
        preferred_identity_class: Option<crate::office::OfficeAccountIdentityClass>,
        error: &Error,
    ) -> Result<ToolExecutionOutcome> {
        build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_documents",
            op,
            provider,
            account_key,
            capability: OfficeCapability::Documents,
            default_account_key: self.service.office_default_account_key()?,
            resolve_hint: self.service.office_resolve_hint_with_identity(
                provider,
                account_key,
                preferred_identity_class,
            )?,
            account_assessments: self.service.office_account_assessments()?,
            error,
        })
    }

    fn execute_impl(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_documents")?;
        let preferred_identity_class =
            parse_preferred_identity_class(&obj, "preferred_identity_class", "tool_documents")?;
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
                let (mut resolved_contexts, lookup_identity_hint, lookup_provider_hint) =
                    self.resolve_lookup_context(&obj, preferred_identity_class)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_op(
                                provider_hint,
                                effective_identity_class,
                                DocumentsOperation::List,
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contexts_identity(
                    &mut resolved_contexts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "list",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let items = match self.service.list_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
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
                            effective_identity_class,
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
                        resolved_contexts,
                    },
                )?))
            }
            "read" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (mut resolved_contexts, lookup_identity_hint, lookup_provider_hint) =
                    self.resolve_lookup_context(&obj, preferred_identity_class)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_op(
                                provider_hint,
                                effective_identity_class,
                                DocumentsOperation::Read,
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contexts_identity(
                    &mut resolved_contexts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "read",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let document = match self.service.read_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    required_str(&obj, "path")?,
                    parse_max_chars(obj.get("max_chars"))?,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        return self.office_operation_failure(
                            "read",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
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
                        resolved_contexts,
                    },
                )?))
            }
            "summarize" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (mut resolved_contexts, lookup_identity_hint, lookup_provider_hint) =
                    self.resolve_lookup_context(&obj, preferred_identity_class)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_op(
                                provider_hint,
                                effective_identity_class,
                                DocumentsOperation::Read,
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contexts_identity(
                    &mut resolved_contexts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "summarize",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let document = match self.service.read_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    required_str(&obj, "path")?,
                    parse_max_chars(obj.get("max_chars"))?,
                ) {
                    Ok(document) => document,
                    Err(error) => {
                        return self.office_operation_failure(
                            "summarize",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
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
                        resolved_contexts,
                    },
                )?))
            }
            "search" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (mut resolved_contexts, lookup_identity_hint, lookup_provider_hint) =
                    self.resolve_lookup_context(&obj, preferred_identity_class)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_op(
                                provider_hint,
                                effective_identity_class,
                                DocumentsOperation::Search,
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contexts_identity(
                    &mut resolved_contexts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "search",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let hits = match self.service.search_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
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
                            effective_identity_class,
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
                        resolved_contexts,
                    },
                )?))
            }
            _ => Err(Error::config(
                "tool_documents",
                format!("unknown op '{}'", op),
            )),
        }
    }

    fn resolve_lookup_context(
        &self,
        obj: &serde_json::Map<String, Value>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<DocumentsLookupContext> {
        let queries = parse_string_list(obj, "context_lookup", "tool_documents")?;
        if queries.is_empty() {
            return Ok((Vec::new(), None, None));
        }
        let Some(directory) = self.contacts_directory.as_ref() else {
            return Err(Error::config(
                "tool_documents",
                "context_lookup requires contacts_directory support",
            ));
        };
        let mut resolved = Vec::with_capacity(queries.len());
        let mut identity_hints = Vec::with_capacity(queries.len());
        let mut provider_hints = Vec::with_capacity(queries.len());
        for query in queries {
            let hit = directory.resolve_lookup_hit_with_route_and_identity(
                &query,
                None,
                None,
                preferred_identity_class,
            )?;
            identity_hints.push(
                self.service
                    .office_identity_class_for_account(hit.account_key.as_deref())?,
            );
            provider_hints.push(documents_provider_hint_from_lookup_hit(&hit));
            resolved.push(documents_resolved_context(query, hit));
        }
        Ok((
            resolved,
            coalesce_identity_hints(identity_hints),
            coalesce_provider_hints(provider_hints),
        ))
    }
}

type DocumentsLookupContext = (
    Vec<DocumentsResolvedContext>,
    Option<OfficeAccountIdentityClass>,
    Option<String>,
);

impl Tool for DocumentsTool {
    fn name(&self) -> &'static str {
        "documents"
    }

    fn description(&self) -> &'static str {
        "Access office document libraries through shared account authority. Ops: provider_status, list, read, summarize, search. Provider can be omitted when office documents defaults, identity hints, or people/team context make routing unambiguous."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: provider_status|list|read|summarize|search"},"provider":{"type":"string","description":"Optional documents provider. Omit only when office defaults, identity hints, or people/team context make routing unambiguous."},"account_key":{"type":"string","description":"Optional explicit office documents account key."},"preferred_identity_class":{"type":"string","description":"Optional identity class preference when routing through office authority: work|personal|family|shared|other."},"context_lookup":{"type":"array","items":{"type":"string"},"description":"Optional people or organization queries resolved through contacts_directory to guide documents routing."},"path":{"type":"string","description":"Optional directory or file path inside the provider root."},"focus":{"type":"string","description":"Optional phrase to emphasize in summarize output."},"limit":{"type":"integer","description":"List/search limit, default 10, max 50."},"max_chars":{"type":"integer","description":"Maximum characters to return for read or summarize, default 16000, max 50000."},"query":{"type":"string","description":"Search phrase for search."},"case_sensitive":{"type":"boolean","description":"Whether search matching is case-sensitive."},"max_read_bytes":{"type":"integer","description":"Maximum bytes to read per file during content search, default 262144."}},"required":["op"]}"#
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

fn documents_resolved_context(
    query: String,
    hit: ContactsDirectoryLookupHit,
) -> DocumentsResolvedContext {
    let ContactEntry {
        id,
        display_name,
        organization,
        ..
    } = hit.contact;
    DocumentsResolvedContext {
        query,
        contact_id: id,
        display_name,
        organization,
        match_reason: hit.match_reason,
        score: hit.score,
        provider: hit.provider,
        account_key: hit.account_key,
        identity_class: None,
    }
}

fn documents_provider_hint_from_lookup_hit(hit: &ContactsDirectoryLookupHit) -> Option<String> {
    match hit.provider.as_deref() {
        Some("feishu_contacts_directory") => Some("feishu_documents".to_string()),
        Some("wecom_contacts_directory") => Some("wecom_documents".to_string()),
        Some("microsoft365_contacts_directory") => Some("microsoft365_documents".to_string()),
        _ => None,
    }
}

fn coalesce_identity_hints<I>(hints: I) -> Option<OfficeAccountIdentityClass>
where
    I: IntoIterator<Item = Option<OfficeAccountIdentityClass>>,
{
    let mut selected = None;
    for hint in hints.into_iter().flatten() {
        match selected {
            Some(existing) if existing != hint => return None,
            Some(_) => {}
            None => selected = Some(hint),
        }
    }
    selected
}

fn coalesce_provider_hints<I>(hints: I) -> Option<String>
where
    I: IntoIterator<Item = Option<String>>,
{
    let mut selected = None::<String>;
    for hint in hints.into_iter().flatten() {
        match selected.as_deref() {
            Some(existing) if existing != hint => return None,
            Some(_) => {}
            None => selected = Some(hint),
        }
    }
    selected
}

fn annotate_resolved_contexts_identity(
    resolved_contexts: &mut [DocumentsResolvedContext],
    identity_class: Option<OfficeAccountIdentityClass>,
) {
    let identity_class = identity_class.map(identity_class_label);
    for item in resolved_contexts {
        item.identity_class = identity_class.clone();
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

fn parse_string_list(
    obj: &serde_json::Map<String, Value>,
    field: &'static str,
    stage: &'static str,
) -> Result<Vec<String>> {
    let Some(value) = obj.get(field) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| Error::config(stage, format!("{} must be an array", field)))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    Error::config(stage, format!("{} items must be non-empty strings", field))
                })
        })
        .collect()
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

fn identity_class_label(identity_class: OfficeAccountIdentityClass) -> String {
    match identity_class {
        OfficeAccountIdentityClass::Work => "work",
        OfficeAccountIdentityClass::Personal => "personal",
        OfficeAccountIdentityClass::Family => "family",
        OfficeAccountIdentityClass::Shared => "shared",
        OfficeAccountIdentityClass::Other => "other",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contacts_directory::{
        ContactEntry, ContactsDirectoryProvider, ContactsDirectoryProviderCredential,
        ContactsDirectoryProviderRegistry, ContactsDirectoryService,
        OfficeBackedContactsDirectoryProviderCredentialStore, StateFsContactsDirectoryStore,
        OFFICE_METADATA_CONTACTS_APP_ID,
    };
    use crate::documents::{
        DocumentsOperation, DocumentsProvider, DocumentsProviderCredential,
        OfficeBackedDocumentsProviderCredentialStore,
    };
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, OfficeService, SnapshotOfficeAuthoritySource,
    };
    use crate::platform::StateFs;
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

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
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

    struct StubRemoteContactsProvider {
        contacts: Vec<ContactEntry>,
    }

    impl ContactsDirectoryProvider for StubRemoteContactsProvider {
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

    #[test]
    fn documents_tool_list_prefers_identity_class_over_default_account() {
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, account_label, external_account_id) in [
            ("docs-work", "Work Docs", "work@example.com"),
            ("docs-personal", "Personal Docs", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    DocumentsProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "webdav".to_string(),
                        account_id: external_account_id.to_string(),
                        account_label: account_label.to_string(),
                        username: external_account_id.to_string(),
                        secret: "secret".to_string(),
                        app_id: String::new(),
                        space_id: String::new(),
                        base_url: "https://dav.example.com/root".to_string(),
                        root_path: "/Workspace".to_string(),
                    },
                );
        }

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        registry.insert(OfficeAccount {
            account_key: "docs-personal".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Documents, "docs-personal".to_string());
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for account_key in ["docs-work", "docs-personal"] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
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
        }
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            Arc::new(StubRuntimeStatusStore),
        );
        let tool = DocumentsTool::with_office_service(
            Arc::new(OfficeBackedDocumentsProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"list","provider":"webdav","preferred_identity_class":"work"}"#,
                &mut ctx,
            )
            .expect("list documents via preferred identity");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["items"][0]["name"], "Work Docs");
    }

    #[test]
    fn documents_tool_list_returns_resolve_hint_when_accounts_are_ambiguous() {
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

        let mut registry = OfficeAccountRegistry::new();
        for (account_key, external_account_id, account_label, identity_class) in [
            (
                "docs-work",
                "work@example.com",
                "Work Docs",
                OfficeAccountIdentityClass::Work,
            ),
            (
                "docs-personal",
                "personal@example.com",
                "Personal Docs",
                OfficeAccountIdentityClass::Personal,
            ),
        ] {
            registry.insert(OfficeAccount {
                account_key: account_key.to_string(),
                provider_kind: "webdav".to_string(),
                external_account_id: external_account_id.to_string(),
                account_label: account_label.to_string(),
                identity_class,
                enabled_capabilities: vec![OfficeCapability::Documents],
            });
        }
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for account_key in ["docs-work", "docs-personal"] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
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
        }
        let office_service = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            office_credential_store,
            Arc::new(StubRuntimeStatusStore),
        );

        let tool = DocumentsTool::with_office_service(
            Arc::new(OfficeBackedDocumentsProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        );
        let mut ctx = DummyCtx;

        let outcome = tool
            .execute_outcome(r#"{"op":"list","provider":"webdav"}"#, &mut ctx)
            .expect("structured ambiguity outcome");
        assert_eq!(
            outcome.failure_kind,
            Some(crate::tools::ToolExecutionFailureKind::Capability)
        );

        let payload: Value =
            serde_json::from_str(&outcome.content).expect("valid failure response json");
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["status"],
            "ambiguous"
        );
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["candidate_accounts"][0]["account_key"],
            "docs-personal"
        );
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["candidate_accounts"][1]["account_key"],
            "docs-work"
        );
        assert!(payload["error"]
            .as_str()
            .expect("error string")
            .contains("candidate accounts"));
    }

    #[test]
    fn documents_tool_context_lookup_prefers_matching_documents_identity_over_default_account() {
        let mut providers = DocumentsProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, account_label, external_account_id) in [
            ("docs-work", "Work Docs", "work@example.com"),
            ("docs-personal", "Personal Docs", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    DocumentsProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "webdav".to_string(),
                        account_id: external_account_id.to_string(),
                        account_label: account_label.to_string(),
                        username: external_account_id.to_string(),
                        secret: "secret".to_string(),
                        app_id: String::new(),
                        space_id: String::new(),
                        base_url: "https://dav.example.com/root".to_string(),
                        root_path: "/Workspace".to_string(),
                    },
                );
        }

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        registry.insert(OfficeAccount {
            account_key: "docs-personal".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        registry.insert(OfficeAccount {
            account_key: "contacts-feishu".to_string(),
            provider_kind: "feishu_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        });

        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Documents, "docs-personal".to_string());
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for (account_key, access_token, metadata) in [
            (
                "docs-work",
                "secret",
                [(
                    crate::documents::OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                    "https://dav.example.com/root".to_string(),
                )]
                .into_iter()
                .collect(),
            ),
            (
                "docs-personal",
                "secret",
                [(
                    crate::documents::OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                    "https://dav.example.com/root".to_string(),
                )]
                .into_iter()
                .collect(),
            ),
            (
                "contacts-feishu",
                "app-secret",
                [(
                    OFFICE_METADATA_CONTACTS_APP_ID.to_string(),
                    "cli_contacts".to_string(),
                )]
                .into_iter()
                .collect(),
            ),
        ] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
                    access_token: access_token.to_string(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 1,
                    metadata,
                })
                .expect("seed office credential");
        }
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            Arc::new(StubRuntimeStatusStore),
        );
        let contacts_store = Arc::new(StateFsContactsDirectoryStore::new(Arc::new(
            MemoryStateFs::default(),
        )));
        let contacts_credentials = Arc::new(
            OfficeBackedContactsDirectoryProviderCredentialStore::new(office_service.clone()),
        );
        let mut contacts_providers = ContactsDirectoryProviderRegistry::new();
        contacts_providers.register(Arc::new(StubRemoteContactsProvider {
            contacts: vec![ContactEntry {
                id: "ou_beetle".to_string(),
                display_name: "Beetle Team".to_string(),
                emails: vec!["team@beetle.cn".to_string()],
                aliases: vec!["甲壳虫团队".to_string()],
                organization: "Beetle".to_string(),
                notes: String::new(),
                updated_at_unix_secs: 1,
            }],
        }));
        let contacts_service = ContactsDirectoryService::with_office_service(
            contacts_store,
            contacts_credentials,
            contacts_providers,
            office_service.clone(),
        );
        let tool = DocumentsTool::with_office_authority_and_contacts_service(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"list","context_lookup":["Beetle Team"]}"#,
                &mut ctx,
            )
            .expect("list documents via remote contacts context");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["items"][0]["name"], "Work Docs");
        assert_eq!(
            payload["resolved_contexts"][0]["account_key"],
            "contacts-feishu"
        );
        assert_eq!(payload["resolved_contexts"][0]["identity_class"], "work");
    }
}
