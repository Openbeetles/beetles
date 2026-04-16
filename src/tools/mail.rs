//! Mail tool: provider-backed office mail access over shared office authority.

use crate::contacts_directory::{
    ContactsDirectoryEmailResolution, ContactsDirectoryService, ContactsDirectoryStore,
};
use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailProviderCredentialStatus, MailProviderCredentialStore,
    MailProviderRegistry, MailQuery, MailSearchQuery, MailSendRequest, MailService,
};
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

pub struct MailTool {
    service: MailService,
    contacts_directory: Option<ContactsDirectoryService>,
}

#[derive(Serialize)]
struct MailProviderStatusResponse {
    op: &'static str,
    registered_remote_providers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_mail_account_key: Option<String>,
    configured_providers: Vec<MailProviderCredentialStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_assessments: Vec<OfficeAccountAssessment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_diagnostics: Vec<OfficeAccountDiagnostic>,
    account_statuses: Vec<MailAccountStatus>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    office_runtime_statuses: Vec<OfficeAccountRuntimeStatus>,
}

#[derive(Serialize)]
struct MailListResponse {
    op: &'static str,
    provider: String,
    count: usize,
    items: Vec<MailMessageSummary>,
}

#[derive(Serialize)]
struct MailSearchResponse {
    op: &'static str,
    provider: String,
    query: String,
    count: usize,
    items: Vec<MailMessageSummary>,
}

#[derive(Serialize)]
struct MailGetResponse {
    op: &'static str,
    provider: String,
    message: MailMessage,
}

#[derive(Serialize)]
struct MailMutationResponse {
    op: &'static str,
    ok: bool,
    provider: String,
    message: MailMessageSummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contacts: Vec<MailResolvedContact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    office_runtime_status: Option<OfficeAccountRuntimeStatus>,
}

#[derive(Serialize)]
struct MailAccountStatus {
    account_key: String,
    provider: String,
    configured: bool,
    send_supported: bool,
    draft_supported: bool,
    send_ready: bool,
    last_error: String,
    last_activity_kind: String,
    last_activity_ok: bool,
    last_activity_at_unix_secs: u64,
}

#[derive(Serialize)]
struct MailResolvedContact {
    field: &'static str,
    query: String,
    contact_id: String,
    display_name: String,
    email: String,
    match_reason: String,
    score: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity_class: Option<String>,
}

impl MailTool {
    pub fn new(credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>) -> Self {
        Self::with_runtime(credential_store, MailProviderRegistry::new(), None, None)
    }

    pub fn with_providers(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
    ) -> Self {
        Self::with_runtime(credential_store, providers, None, None)
    }

    pub fn with_office_service(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    pub fn with_office_authority(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        Self::with_runtime(credential_store, providers, Some(office_authority), None)
    }

    pub fn with_office_service_and_contacts(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: OfficeService,
        contacts_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
    ) -> Self {
        Self::with_office_authority_and_contacts(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_store,
        )
    }

    pub fn with_office_authority_and_contacts(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
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
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
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
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
        contacts_directory: Option<ContactsDirectoryService>,
    ) -> Self {
        Self {
            service: MailService::with_office_authority(
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
            stage: "tool_mail",
            op,
            provider,
            account_key,
            capability: OfficeCapability::Mail,
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
        let obj = parse_tool_args(args, "tool_mail")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_mail", "missing op"))?;
        let preferred_identity_class =
            parse_preferred_identity_class(&obj, "preferred_identity_class", "tool_mail")?;
        match op {
            "provider_status" => {
                let registered_remote_providers = self
                    .service
                    .provider_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let configured_providers = self.service.list_provider_statuses()?;
                let office_runtime_statuses = self.service.office_runtime_statuses()?;
                let account_assessments = self.service.office_account_assessments()?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailProviderStatusResponse {
                        op: "provider_status",
                        registered_remote_providers,
                        default_mail_account_key: self.service.office_default_account_key()?,
                        account_diagnostics: build_account_diagnostics(&account_assessments),
                        account_assessments,
                        account_statuses: build_mail_account_statuses(
                            &self.service,
                            &configured_providers,
                            &office_runtime_statuses,
                        ),
                        configured_providers,
                        office_runtime_statuses,
                    },
                )?))
            }
            "list" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self.service.resolve_provider_name_with_identity(
                    requested_provider.as_deref(),
                    preferred_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "list",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                let items = match self.service.list_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    preferred_identity_class,
                    MailQuery {
                        mailbox: optional_str(&obj, "mailbox"),
                        unread_only: obj
                            .get("unread_only")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        received_after_unix_secs: parse_optional_u64(
                            obj.get("received_after_unix_secs"),
                            "received_after_unix_secs",
                        )?,
                        limit: obj.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize,
                    }
                    .with_limit_clamped(),
                ) {
                    Ok(items) => items,
                    Err(error) => {
                        return self.office_operation_failure(
                            "list",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailListResponse {
                        op: "list",
                        provider,
                        count: items.len(),
                        items,
                    },
                )?))
            }
            "search" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self.service.resolve_provider_name_with_identity(
                    requested_provider.as_deref(),
                    preferred_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "search",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                let query = required_str(&obj, "query")?.trim().to_string();
                if query.is_empty() {
                    return Err(Error::config("tool_mail", "query must not be empty"));
                }
                let items = match self.service.search_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    preferred_identity_class,
                    MailSearchQuery {
                        mailbox: optional_str(&obj, "mailbox"),
                        query: query.clone(),
                        unread_only: obj
                            .get("unread_only")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        received_after_unix_secs: parse_optional_u64(
                            obj.get("received_after_unix_secs"),
                            "received_after_unix_secs",
                        )?,
                        limit: obj.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize,
                    }
                    .with_limit_clamped(),
                ) {
                    Ok(items) => items,
                    Err(error) => {
                        return self.office_operation_failure(
                            "search",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailSearchResponse {
                        op: "search",
                        provider,
                        query,
                        count: items.len(),
                        items,
                    },
                )?))
            }
            "get" => {
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let provider = match self.service.resolve_provider_name_with_identity(
                    requested_provider.as_deref(),
                    preferred_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "get",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                let id = required_str(&obj, "id")?;
                let message = match self.service.get_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    preferred_identity_class,
                    id,
                ) {
                    Ok(Some(message)) => message,
                    Ok(None) => return Err(Error::config("tool_mail", "message not found")),
                    Err(error) => {
                        return self.office_operation_failure(
                            "get",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            preferred_identity_class,
                            &error,
                        )
                    }
                };
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailGetResponse {
                        op: "get",
                        provider,
                        message,
                    },
                )?))
            }
            "send" => {
                require_confirm(&obj, "send")?;
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (
                    to,
                    cc,
                    bcc,
                    mut resolved_contacts,
                    lookup_identity_hint,
                    lookup_provider_hint,
                ) = self.resolve_compose_recipients(&obj)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_ops(
                                provider_hint,
                                effective_identity_class,
                                &[crate::mail::MailOperation::Send],
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contacts_identity(
                    &mut resolved_contacts,
                    effective_identity_class,
                );
                if to.is_empty() && cc.is_empty() && bcc.is_empty() {
                    return Err(Error::config(
                        "tool_mail",
                        "send requires at least one recipient in to, cc, bcc, or *_lookup",
                    ));
                }
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "send",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let message = match self.service.send_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    &MailSendRequest {
                        subject: required_str(&obj, "subject")?.to_string(),
                        text_body: required_str(&obj, "text_body")?.to_string(),
                        to,
                        cc,
                        bcc,
                        in_reply_to: String::new(),
                        references: String::new(),
                    },
                ) {
                    Ok(message) => message,
                    Err(error) => {
                        return self.office_operation_failure(
                            "send",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let office_runtime_status =
                    self.service.office_runtime_status(&message.account_key)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailMutationResponse {
                        op: "send",
                        ok: true,
                        provider: message.provider.clone(),
                        message,
                        resolved_contacts,
                        office_runtime_status,
                    },
                )?))
            }
            "draft" => {
                require_confirm(&obj, "draft")?;
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (
                    to,
                    cc,
                    bcc,
                    mut resolved_contacts,
                    lookup_identity_hint,
                    lookup_provider_hint,
                ) = self.resolve_compose_recipients(&obj)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                let derived_provider_hint =
                    if requested_provider.is_none() && requested_account_key.is_none() {
                        lookup_provider_hint.as_deref().filter(|provider_hint| {
                            self.service.provider_is_routable_for_ops(
                                provider_hint,
                                effective_identity_class,
                                &[crate::mail::MailOperation::Draft],
                            )
                        })
                    } else {
                        None
                    };
                let effective_provider_hint =
                    requested_provider.as_deref().or(derived_provider_hint);
                annotate_resolved_contacts_identity(
                    &mut resolved_contacts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    effective_provider_hint,
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "draft",
                            effective_provider_hint,
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let message = match self.service.draft_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    &MailSendRequest {
                        subject: optional_str(&obj, "subject"),
                        text_body: optional_str(&obj, "text_body"),
                        to,
                        cc,
                        bcc,
                        in_reply_to: String::new(),
                        references: String::new(),
                    },
                ) {
                    Ok(message) => message,
                    Err(error) => {
                        return self.office_operation_failure(
                            "draft",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let office_runtime_status =
                    self.service.office_runtime_status(&message.account_key)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailMutationResponse {
                        op: "draft",
                        ok: true,
                        provider: message.provider.clone(),
                        message,
                        resolved_contacts,
                        office_runtime_status,
                    },
                )?))
            }
            "reply" => {
                require_confirm(&obj, "reply")?;
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (
                    to,
                    cc,
                    bcc,
                    mut resolved_contacts,
                    lookup_identity_hint,
                    _lookup_provider_hint,
                ) = self.resolve_compose_recipients(&obj)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                annotate_resolved_contacts_identity(
                    &mut resolved_contacts,
                    effective_identity_class,
                );
                let provider = match self.service.resolve_provider_name_with_identity(
                    requested_provider.as_deref(),
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "reply",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let message = match self.service.reply_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    required_str(&obj, "id")?,
                    &MailSendRequest {
                        subject: optional_str(&obj, "subject"),
                        text_body: required_str(&obj, "text_body")?.to_string(),
                        to,
                        cc,
                        bcc,
                        in_reply_to: String::new(),
                        references: String::new(),
                    },
                ) {
                    Ok(message) => message,
                    Err(error) => {
                        return self.office_operation_failure(
                            "reply",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let office_runtime_status =
                    self.service.office_runtime_status(&message.account_key)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailMutationResponse {
                        op: "reply",
                        ok: true,
                        provider: message.provider.clone(),
                        message,
                        resolved_contacts,
                        office_runtime_status,
                    },
                )?))
            }
            "forward" => {
                require_confirm(&obj, "forward")?;
                let requested_provider = parse_provider(&obj);
                let requested_account_key = parse_account_key(&obj);
                let (
                    to,
                    cc,
                    bcc,
                    mut resolved_contacts,
                    lookup_identity_hint,
                    _lookup_provider_hint,
                ) = self.resolve_compose_recipients(&obj)?;
                let effective_identity_class = preferred_identity_class.or(lookup_identity_hint);
                annotate_resolved_contacts_identity(
                    &mut resolved_contacts,
                    effective_identity_class,
                );
                if to.is_empty() && cc.is_empty() && bcc.is_empty() {
                    return Err(Error::config(
                        "tool_mail",
                        "forward requires at least one recipient in to, cc, bcc, or *_lookup",
                    ));
                }
                let provider = match self.service.resolve_provider_name_with_identity(
                    requested_provider.as_deref(),
                    effective_identity_class,
                ) {
                    Ok(provider) => provider,
                    Err(error) => {
                        return self.office_operation_failure(
                            "forward",
                            requested_provider.as_deref(),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let message = match self.service.forward_with_identity(
                    &provider,
                    requested_account_key.as_deref(),
                    effective_identity_class,
                    required_str(&obj, "id")?,
                    &MailSendRequest {
                        subject: optional_str(&obj, "subject"),
                        text_body: optional_str(&obj, "text_body"),
                        to,
                        cc,
                        bcc,
                        in_reply_to: String::new(),
                        references: String::new(),
                    },
                ) {
                    Ok(message) => message,
                    Err(error) => {
                        return self.office_operation_failure(
                            "forward",
                            Some(provider.as_str()),
                            requested_account_key.as_deref(),
                            effective_identity_class,
                            &error,
                        )
                    }
                };
                let office_runtime_status =
                    self.service.office_runtime_status(&message.account_key)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_mail",
                    &MailMutationResponse {
                        op: "forward",
                        ok: true,
                        provider: message.provider.clone(),
                        message,
                        resolved_contacts,
                        office_runtime_status,
                    },
                )?))
            }
            _ => Err(Error::config("tool_mail", format!("unknown op '{}'", op))),
        }
    }
}

impl Tool for MailTool {
    fn name(&self) -> &'static str {
        "mail"
    }

    fn description(&self) -> &'static str {
        "Access office mail through shared account authority. Ops: provider_status, list, search, get, send, draft, reply, forward. Provider can be omitted when office mail defaults or a single configured provider make routing unambiguous."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: provider_status|list|search|get|send|draft|reply|forward"},"provider":{"type":"string","description":"Optional mail provider. Omit only when office defaults or a single configured provider make routing unambiguous."},"account_key":{"type":"string","description":"Optional explicit office mail account key."},"mailbox":{"type":"string","description":"Mailbox to query for list or search. Defaults to provider mailbox."},"query":{"type":"string","description":"Required free-text search query for search."},"unread_only":{"type":"boolean","description":"Whether list or search should only include unread mail."},"received_after_unix_secs":{"type":"integer","description":"Optional lower bound for received time."},"limit":{"type":"integer","description":"List or search limit, default 10, max 50."},"id":{"type":"string","description":"Message ID or provider UID for get, reply, or forward."},"subject":{"type":"string","description":"Mail subject for send, draft, or optional override on reply/forward."},"text_body":{"type":"string","description":"Mail body or note body for send, draft, reply, or forward."},"to":{"type":"array","items":{"type":"string"},"description":"Primary recipient email addresses."},"cc":{"type":"array","items":{"type":"string"},"description":"CC recipient email addresses."},"bcc":{"type":"array","items":{"type":"string"},"description":"BCC recipient email addresses."},"to_lookup":{"type":"array","items":{"type":"string"},"description":"Primary recipient contact queries resolved through contacts_directory."},"cc_lookup":{"type":"array","items":{"type":"string"},"description":"CC recipient contact queries resolved through contacts_directory."},"bcc_lookup":{"type":"array","items":{"type":"string"},"description":"BCC recipient contact queries resolved through contacts_directory."},"confirm":{"type":"boolean","description":"Must be true for send, draft, reply, and forward."}},"required":["op"]}"#
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
            .with_effect_class(ToolEffectClass::VisibleOutbound)
            .with_risk_level(ToolRiskLevel::High)
            .with_approval_mode(ToolApprovalMode::ExplicitIntent)
            .with_rollback_kind(ToolRollbackKind::Irreversible)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_mail_governance")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("provider_status");
        let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
        Ok(match op {
            "send" | "draft" | "reply" | "forward" => self
                .metadata()
                .default_execution_shape(match op {
                    "send" => "mail_send",
                    "draft" => "mail_draft",
                    "reply" => "mail_reply",
                    _ => "mail_forward",
                })
                .with_approval_granted(confirm),
            "provider_status" | "list" | "search" | "get" => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_approval_granted(true)
                .with_rollback_kind(ToolRollbackKind::None),
            _ => self.metadata().default_execution_shape(op),
        })
    }

    fn requires_network_for(&self, args: &str) -> Result<bool> {
        let obj = parse_tool_args(args, "tool_mail_network")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("provider_status");
        Ok(matches!(
            op,
            "list" | "search" | "get" | "send" | "draft" | "reply" | "forward"
        ))
    }
}

type ComposeRecipients = (
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<MailResolvedContact>,
    Option<OfficeAccountIdentityClass>,
    Option<String>,
);

type RecipientQueryResolution = (
    Vec<String>,
    Vec<MailResolvedContact>,
    Option<OfficeAccountIdentityClass>,
    Option<String>,
);

impl MailTool {
    fn resolve_compose_recipients(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<ComposeRecipients> {
        let (to_lookup, to_lookup_resolved, to_identity_hint, to_provider_hint) =
            self.resolve_recipient_queries(obj, "to_lookup", "to")?;
        let (cc_lookup, cc_lookup_resolved, cc_identity_hint, cc_provider_hint) =
            self.resolve_recipient_queries(obj, "cc_lookup", "cc")?;
        let (bcc_lookup, bcc_lookup_resolved, bcc_identity_hint, bcc_provider_hint) =
            self.resolve_recipient_queries(obj, "bcc_lookup", "bcc")?;
        Ok((
            merge_recipients(parse_recipients(obj, "to")?, to_lookup),
            merge_recipients(parse_recipients(obj, "cc")?, cc_lookup),
            merge_recipients(parse_recipients(obj, "bcc")?, bcc_lookup),
            [to_lookup_resolved, cc_lookup_resolved, bcc_lookup_resolved]
                .into_iter()
                .flatten()
                .collect(),
            coalesce_identity_hints([to_identity_hint, cc_identity_hint, bcc_identity_hint]),
            coalesce_provider_hints([to_provider_hint, cc_provider_hint, bcc_provider_hint]),
        ))
    }

    fn resolve_recipient_queries(
        &self,
        obj: &serde_json::Map<String, Value>,
        field: &str,
        surface: &'static str,
    ) -> Result<RecipientQueryResolution> {
        let queries = parse_recipients(obj, field)?;
        if queries.is_empty() {
            return Ok((Vec::new(), Vec::new(), None, None));
        }
        let Some(directory) = self.contacts_directory.as_ref() else {
            return Err(Error::config(
                "tool_mail",
                format!("{field} requires contacts_directory support"),
            ));
        };
        let mut emails = Vec::with_capacity(queries.len());
        let mut resolved = Vec::with_capacity(queries.len());
        let mut identity_hints = Vec::with_capacity(queries.len());
        let mut provider_hints = Vec::with_capacity(queries.len());
        for query in queries {
            let resolution = directory.resolve_primary_email(&query)?;
            identity_hints.push(
                self.service
                    .office_identity_class_for_account(resolution.account_key.as_deref())?,
            );
            provider_hints.push(mail_provider_hint_from_contact_resolution(&resolution));
            emails.push(resolution.email.clone());
            resolved.push(mail_resolved_contact(surface, resolution));
        }
        Ok((
            emails,
            resolved,
            coalesce_identity_hints(identity_hints),
            coalesce_provider_hints(provider_hints),
        ))
    }
}

fn build_mail_account_statuses(
    service: &MailService,
    configured_providers: &[MailProviderCredentialStatus],
    office_runtime_statuses: &[OfficeAccountRuntimeStatus],
) -> Vec<MailAccountStatus> {
    let runtime_by_account = office_runtime_statuses
        .iter()
        .map(|status| (status.account_key.as_str(), status))
        .collect::<std::collections::BTreeMap<_, _>>();
    configured_providers
        .iter()
        .map(|status| {
            let runtime = runtime_by_account.get(status.account_key.as_str()).copied();
            let send_supported =
                service.provider_supports(&status.provider, crate::mail::MailOperation::Send);
            let draft_supported =
                service.provider_supports(&status.provider, crate::mail::MailOperation::Draft);
            let activity_supports_send = runtime.is_some_and(|runtime| {
                runtime.last_activity_ok
                    && matches!(
                        runtime.last_activity_kind.as_str(),
                        "mail_send" | "mail_reply" | "mail_forward"
                    )
            });
            MailAccountStatus {
                account_key: status.account_key.clone(),
                provider: status.provider.clone(),
                configured: status.configured,
                send_supported,
                draft_supported,
                send_ready: status.configured
                    && send_supported
                    && runtime.is_some_and(|runtime| runtime.probe_ok || activity_supports_send),
                last_error: runtime
                    .map(|item| item.last_error.clone())
                    .unwrap_or_default(),
                last_activity_kind: runtime
                    .map(|item| item.last_activity_kind.clone())
                    .unwrap_or_default(),
                last_activity_ok: runtime.is_some_and(|item| item.last_activity_ok),
                last_activity_at_unix_secs: runtime
                    .map(|item| item.last_activity_at_unix_secs)
                    .unwrap_or(0),
            }
        })
        .collect()
}

trait MailQueryExt {
    fn with_limit_clamped(self) -> Self;
}

impl MailQueryExt for MailQuery {
    fn with_limit_clamped(mut self) -> Self {
        self.limit = self.limit.clamp(1, 50);
        self
    }
}

impl MailQueryExt for MailSearchQuery {
    fn with_limit_clamped(mut self) -> Self {
        self.limit = self.limit.clamp(1, 50);
        self
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
        .ok_or_else(|| Error::config("tool_mail", format!("missing {}", field)))
}

fn optional_str(obj: &serde_json::Map<String, Value>, field: &str) -> String {
    obj.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn parse_optional_u64(value: Option<&Value>, field: &str) -> Result<Option<u64>> {
    match value {
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| Error::config("tool_mail", format!("{} must be non-negative", field))),
        Some(_) => Err(Error::config(
            "tool_mail",
            format!("{} must be an integer", field),
        )),
        None => Ok(None),
    }
}

fn parse_recipients(obj: &serde_json::Map<String, Value>, field: &str) -> Result<Vec<String>> {
    let Some(value) = obj.get(field) else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| Error::config("tool_mail", format!("{} must be an array", field)))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    Error::config(
                        "tool_mail",
                        format!("{} items must be non-empty strings", field),
                    )
                })
        })
        .collect()
}

fn merge_recipients(mut direct: Vec<String>, resolved: Vec<String>) -> Vec<String> {
    for email in resolved {
        if !direct.iter().any(|item| item.eq_ignore_ascii_case(&email)) {
            direct.push(email);
        }
    }
    direct
}

fn mail_resolved_contact(
    field: &'static str,
    resolution: ContactsDirectoryEmailResolution,
) -> MailResolvedContact {
    let ContactsDirectoryEmailResolution {
        query,
        contact_id,
        display_name,
        email,
        match_reason,
        score,
        provider,
        account_key,
        ..
    } = resolution;
    MailResolvedContact {
        field,
        query,
        contact_id,
        display_name,
        email,
        match_reason,
        score,
        provider,
        account_key,
        identity_class: None,
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

fn mail_provider_hint_from_contact_resolution(
    resolution: &ContactsDirectoryEmailResolution,
) -> Option<String> {
    match resolution.provider.as_deref() {
        Some("feishu_contacts_directory") => Some("feishu_mail".to_string()),
        Some("wecom_contacts_directory") => Some("wecom_mail".to_string()),
        _ => None,
    }
}

fn annotate_resolved_contacts_identity(
    resolved_contacts: &mut [MailResolvedContact],
    identity_class: Option<OfficeAccountIdentityClass>,
) {
    let identity_class = identity_class.map(identity_class_label);
    for item in resolved_contacts {
        item.identity_class = identity_class.clone();
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

fn require_confirm(obj: &serde_json::Map<String, Value>, op: &str) -> Result<()> {
    if obj.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
        Ok(())
    } else {
        Err(Error::config(
            "tool_mail",
            format!("{} requires confirm=true", op),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{save_office_accounts_segment, ConfigFileStore};
    use crate::contacts_directory::{
        ContactEntry, ContactsDirectoryProvider, ContactsDirectoryProviderCredential,
        ContactsDirectoryProviderRegistry, ContactsDirectoryService, ContactsDirectoryStore,
        OfficeBackedContactsDirectoryProviderCredentialStore, StateFsContactsDirectoryStore,
        OFFICE_METADATA_CONTACTS_APP_ID,
    };
    use crate::mail::{
        MailOperation, MailProvider, MailProviderCredential,
        OfficeBackedMailProviderCredentialStore,
    };
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, ReloadingOfficeAuthoritySource,
    };
    use crate::platform::StateFs;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<HashMap<String, MailProviderCredential>>,
    }

    impl MailProviderCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<MailProviderCredential>> {
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

        fn list_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .map(MailProviderCredential::status)
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
    struct StubRuntimeStatusStore {
        items: Mutex<HashMap<String, OfficeAccountRuntimeStatus>>,
    }

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(status.account_key.clone(), status.clone());
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
            Ok(self.files.lock().unwrap().get(rel_path).cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files.lock().unwrap().remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct MemoryConfigFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl ConfigFileStore for MemoryConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    struct StubProvider {
        provider_name: &'static str,
        display_name: &'static str,
        sent_requests: Mutex<Vec<MailSendRequest>>,
        drafted_requests: Mutex<Vec<MailSendRequest>>,
    }

    impl Default for StubProvider {
        fn default() -> Self {
            Self {
                provider_name: "imap_smtp",
                display_name: "IMAP/SMTP",
                sent_requests: Mutex::new(Vec::new()),
                drafted_requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl MailProvider for StubProvider {
        fn provider_name(&self) -> &'static str {
            self.provider_name
        }

        fn display_name(&self) -> &'static str {
            self.display_name
        }

        fn supports(&self, _op: MailOperation) -> bool {
            true
        }

        fn list_messages(
            &self,
            credential: &MailProviderCredential,
            query: MailQuery,
        ) -> Result<Vec<MailMessageSummary>> {
            Ok(vec![MailMessageSummary {
                id: format!("list-{}", credential.account_key),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: if query.mailbox.is_empty() {
                    credential.imap_mailbox.clone()
                } else {
                    query.mailbox
                },
                subject: "subject".to_string(),
                from: credential.from_address.clone(),
                to: vec![credential.account_id.clone()],
                preview: "preview".to_string(),
                unread: true,
                received_at_unix_secs: 1,
            }])
        }

        fn search_messages(
            &self,
            credential: &MailProviderCredential,
            query: MailSearchQuery,
        ) -> Result<Vec<MailMessageSummary>> {
            Ok(vec![MailMessageSummary {
                id: format!("search-{}", credential.account_key),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: if query.mailbox.is_empty() {
                    credential.imap_mailbox.clone()
                } else {
                    query.mailbox
                },
                subject: format!("match {}", query.query.trim()),
                from: credential.from_address.clone(),
                to: vec![credential.account_id.clone()],
                preview: "search preview".to_string(),
                unread: true,
                received_at_unix_secs: 1,
            }])
        }

        fn get_message(
            &self,
            credential: &MailProviderCredential,
            id: &str,
        ) -> Result<Option<MailMessage>> {
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
                    received_at_unix_secs: 2,
                },
                text_body: "body".to_string(),
                message_id: "<msg-1@example.com>".to_string(),
                reply_to: vec!["reply@example.com".to_string()],
                references: "<root@example.com>".to_string(),
            }))
        }

        fn send_message(
            &self,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
            self.sent_requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(request.clone());
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
                received_at_unix_secs: 3,
            })
        }

        fn draft_message(
            &self,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
            self.drafted_requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(request.clone());
            Ok(MailMessageSummary {
                id: "draft-1".to_string(),
                provider: credential.provider.clone(),
                account_key: credential.account_key.clone(),
                mailbox: credential.draft_mailbox.clone(),
                subject: request.subject.clone(),
                from: credential.from_address.clone(),
                to: request.to.clone(),
                preview: request.text_body.clone(),
                unread: false,
                received_at_unix_secs: 3,
            })
        }
    }

    fn build_tool() -> (MailTool, Arc<StubProvider>, Arc<StubRuntimeStatusStore>) {
        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, label, account_id) in [
            ("mail-work", "Work", "work@example.com"),
            ("mail-personal", "Personal", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "imap_smtp".to_string(),
                        account_id: account_id.to_string(),
                        account_label: label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: label.to_string(),
                    },
                );
        }

        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider.clone());

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
        binding.set_default_account(OfficeCapability::Mail, "mail-work".to_string());
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        runtime_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "mail-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 7,
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 8,
            })
            .expect("seed runtime status");
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for account_key in ["mail-work", "mail-personal"] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
                    access_token: "secret".to_string(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 1,
                    metadata: [
                        (
                            crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                            "imap.example.com".to_string(),
                        ),
                        (
                            crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                            "smtp.example.com".to_string(),
                        ),
                    ]
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
            runtime_store.clone(),
        );

        let contacts_store = Arc::new(StateFsContactsDirectoryStore::new(Arc::new(
            MemoryStateFs::default(),
        )));
        contacts_store
            .upsert(&crate::contacts_directory::ContactEntry {
                id: "alice-zhang".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@example.com".to_string()],
                aliases: vec!["阿丽丝".to_string()],
                organization: "Beetle".to_string(),
                updated_at_unix_secs: 1,
                notes: String::new(),
            })
            .expect("seed contacts directory");

        (
            MailTool::with_office_service_and_contacts(
                credential_store,
                providers,
                office_service,
                contacts_store,
            ),
            provider,
            runtime_store,
        )
    }

    fn save_mail_accounts(
        config_file_store: &dyn ConfigFileStore,
        default_account_key: &str,
    ) -> Result<()> {
        save_office_accounts_segment(
            config_file_store,
            &format!(
                r#"{{
                    "registry": {{
                        "accounts": {{
                            "mail-work": {{
                                "account_key": "mail-work",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "work@example.com",
                                "account_label": "Work",
                                "identity_class": "work",
                                "enabled_capabilities": ["mail"]
                            }},
                            "mail-personal": {{
                                "account_key": "mail-personal",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "personal@example.com",
                                "account_label": "Personal",
                                "identity_class": "personal",
                                "enabled_capabilities": ["mail"]
                            }}
                        }}
                    }},
                    "binding": {{
                        "capability_defaults": {{
                            "mail": "{default_account_key}"
                        }}
                    }},
                    "policy": {{}}
                }}"#
            ),
        )
    }

    fn build_office_backed_tool_without_credentials() -> MailTool {
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider);

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Mail, "mail-work".to_string());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore::default()),
        );

        MailTool::with_office_service(
            Arc::new(OfficeBackedMailProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        )
    }

    fn build_office_backed_tool_with_ambiguous_accounts() -> MailTool {
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider);

        let mut registry = OfficeAccountRegistry::new();
        for (account_key, account_label, external_account_id, identity_class) in [
            (
                "mail-work",
                "Work",
                "work@example.com",
                OfficeAccountIdentityClass::Work,
            ),
            (
                "mail-personal",
                "Personal",
                "personal@example.com",
                OfficeAccountIdentityClass::Personal,
            ),
        ] {
            registry.insert(OfficeAccount {
                account_key: account_key.to_string(),
                provider_kind: "imap_smtp".to_string(),
                external_account_id: external_account_id.to_string(),
                account_label: account_label.to_string(),
                identity_class,
                enabled_capabilities: vec![OfficeCapability::Mail],
            });
        }
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for account_key in ["mail-work", "mail-personal"] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
                    access_token: "secret".to_string(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 1,
                    metadata: [
                        (
                            crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                            "imap.example.com".to_string(),
                        ),
                        (
                            crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                            "smtp.example.com".to_string(),
                        ),
                    ]
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
            Arc::new(StubRuntimeStatusStore::default()),
        );

        MailTool::with_office_service(
            Arc::new(OfficeBackedMailProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        )
    }

    #[test]
    fn mail_tool_provider_status_reports_defaults_and_runtime() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .expect("provider status");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["registered_remote_providers"][0], "imap_smtp");
        assert_eq!(payload["default_mail_account_key"], "mail-work");
        assert_eq!(payload["configured_providers"][0]["provider"], "imap_smtp");
        assert_eq!(payload["office_runtime_statuses"][0]["probe_ok"], true);
        let account_statuses = payload["account_statuses"]
            .as_array()
            .expect("account statuses array");
        let work_status = account_statuses
            .iter()
            .find(|item| item["account_key"] == "mail-work")
            .expect("mail-work status");
        assert_eq!(work_status["send_ready"], true);
        let assessments = payload["account_assessments"]
            .as_array()
            .expect("account assessments array");
        let work_assessment = assessments
            .iter()
            .find(|item| item["account_key"] == "mail-work")
            .expect("mail-work assessment");
        assert_eq!(work_assessment["readiness"], "ready");
        assert_eq!(work_assessment["next_action"], "none");
        assert_eq!(work_assessment["probe_supported"], true);
        let diagnostics = payload["account_diagnostics"]
            .as_array()
            .expect("account diagnostics array");
        let work_diagnostic = diagnostics
            .iter()
            .find(|item| item["account_key"] == "mail-work")
            .expect("mail-work diagnostic");
        assert_eq!(work_diagnostic["diagnosis_kind"], "ready");
        assert_eq!(work_diagnostic["recommended_action"], "none");
    }

    #[test]
    fn mail_tool_list_routes_via_office_default_when_provider_is_omitted() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"list"}"#, &mut ctx)
            .expect("list mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["provider"], "imap_smtp");
        assert_eq!(payload["items"][0]["account_key"], "mail-work");
    }

    #[test]
    fn mail_tool_search_returns_results_and_uses_office_default() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"search","query":"hello project","limit":5}"#,
                &mut ctx,
            )
            .expect("search mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["op"], "search");
        assert_eq!(payload["provider"], "imap_smtp");
        assert_eq!(payload["query"], "hello project");
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["items"][0]["account_key"], "mail-work");
    }

    #[test]
    fn mail_tool_list_prefers_identity_class_over_default_account() {
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider);

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, label, account_id) in [
            ("mail-work", "Work", "work@example.com"),
            ("mail-personal", "Personal", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "imap_smtp".to_string(),
                        account_id: account_id.to_string(),
                        account_label: label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: label.to_string(),
                    },
                );
        }

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
        binding.set_default_account(OfficeCapability::Mail, "mail-personal".to_string());
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for account_key in ["mail-work", "mail-personal"] {
            office_credential_store
                .set(&OfficeCredential {
                    account_key: account_key.to_string(),
                    access_token: "secret".to_string(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 1,
                    metadata: [
                        (
                            crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                            "imap.example.com".to_string(),
                        ),
                        (
                            crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                            "smtp.example.com".to_string(),
                        ),
                    ]
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
            Arc::new(StubRuntimeStatusStore::default()),
        );
        let tool = MailTool::with_office_service(
            Arc::new(OfficeBackedMailProviderCredentialStore::new(
                office_service.clone(),
            )),
            providers,
            office_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"list","provider":"imap_smtp","preferred_identity_class":"work"}"#,
                &mut ctx,
            )
            .expect("list mail via preferred identity");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["items"][0]["account_key"], "mail-work");
    }

    #[test]
    fn mail_tool_get_returns_message_body() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"get","provider":"imap_smtp","id":"42"}"#, &mut ctx)
            .expect("get mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["summary"]["id"], "42");
        assert_eq!(payload["message"]["text_body"], "body");
    }

    #[test]
    fn mail_tool_list_updates_runtime_status() {
        let (tool, _provider, runtime_store) = build_tool();
        let mut ctx = DummyCtx;

        tool.execute(r#"{"op":"list"}"#, &mut ctx)
            .expect("list mail");

        let runtime = runtime_store
            .get("mail-work")
            .expect("load runtime")
            .expect("runtime status");
        assert_eq!(runtime.last_activity_kind, "mail_list");
        assert!(runtime.last_activity_ok);
        assert!(runtime.probe_ok);
    }

    #[test]
    fn mail_tool_search_updates_runtime_status() {
        let (tool, _provider, runtime_store) = build_tool();
        let mut ctx = DummyCtx;

        tool.execute(r#"{"op":"search","query":"hello"}"#, &mut ctx)
            .expect("search mail");

        let runtime = runtime_store
            .get("mail-work")
            .expect("load runtime")
            .expect("runtime status");
        assert_eq!(runtime.last_activity_kind, "mail_search");
        assert!(runtime.last_activity_ok);
        assert!(runtime.probe_ok);
    }

    #[test]
    fn mail_tool_get_updates_runtime_status() {
        let (tool, _provider, runtime_store) = build_tool();
        let mut ctx = DummyCtx;

        tool.execute(r#"{"op":"get","provider":"imap_smtp","id":"42"}"#, &mut ctx)
            .expect("get mail");

        let runtime = runtime_store
            .get("mail-work")
            .expect("load runtime")
            .expect("runtime status");
        assert_eq!(runtime.last_activity_kind, "mail_get");
        assert!(runtime.last_activity_ok);
        assert!(runtime.probe_ok);
    }

    #[test]
    fn mail_tool_send_requires_confirm_and_returns_summary() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let error = tool
            .execute(
                r#"{"op":"send","subject":"Hi","text_body":"Body","to":["a@example.com"]}"#,
                &mut ctx,
            )
            .expect_err("send without confirm should fail");
        assert!(error.to_string().contains("confirm=true"));

        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hi","text_body":"Body","to":["a@example.com"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["message"]["mailbox"], "Sent");
        assert_eq!(payload["message"]["subject"], "Hi");
    }

    #[test]
    fn mail_tool_send_resolves_lookup_recipients_via_contacts_directory() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;

        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hi","text_body":"Body","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send mail with lookup");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["to"][0], "alice@example.com");
        assert_eq!(payload["resolved_contacts"][0]["field"], "to");
        assert_eq!(payload["resolved_contacts"][0]["contact_id"], "alice-zhang");
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

    #[test]
    fn mail_tool_send_resolves_lookup_recipients_via_remote_contacts_directory() {
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider.clone());

        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .items
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(
                "mail-work".to_string(),
                MailProviderCredential {
                    account_key: "mail-work".to_string(),
                    provider: "imap_smtp".to_string(),
                    account_id: "work@example.com".to_string(),
                    account_label: "Work".to_string(),
                    username: "work@example.com".to_string(),
                    corp_id: String::new(),
                    secret: "secret".to_string(),
                    base_url: String::new(),
                    imap_host: "imap.example.com".to_string(),
                    imap_port: 993,
                    imap_mailbox: "INBOX".to_string(),
                    draft_mailbox: "Drafts".to_string(),
                    imap_tls: true,
                    smtp_host: "smtp.example.com".to_string(),
                    smtp_port: 465,
                    smtp_tls: true,
                    from_address: "work@example.com".to_string(),
                    from_name: "Work".to_string(),
                },
            );

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
            account_key: "contacts-feishu".to_string(),
            provider_kind: "feishu_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Mail, "mail-work".to_string());
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        office_credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed mail office credential");
        office_credential_store
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
            .expect("seed contacts office credential");
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            runtime_store,
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
                id: "ou_alice".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@beetle.cn".to_string()],
                aliases: vec!["阿丽丝".to_string()],
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
        let tool = MailTool::with_office_authority_and_contacts_service(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hello","text_body":"Need sync","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send via remote contacts directory");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["to"][0], "alice@beetle.cn");
        assert_eq!(payload["resolved_contacts"][0]["contact_id"], "ou_alice");
    }

    #[test]
    fn mail_tool_send_remote_work_contact_prefers_matching_mail_identity_over_default_account() {
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider.clone());

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, account_label, account_id) in [
            ("mail-work", "Work", "work@example.com"),
            ("mail-personal", "Personal", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "imap_smtp".to_string(),
                        account_id: account_id.to_string(),
                        account_label: account_label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: account_label.to_string(),
                    },
                );
        }

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
        registry.insert(OfficeAccount {
            account_key: "contacts-feishu".to_string(),
            provider_kind: "feishu_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        });

        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Mail, "mail-personal".to_string());
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for (account_key, access_token, metadata) in [
            (
                "mail-work",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            (
                "mail-personal",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "personal@example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
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
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            runtime_store,
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
                id: "ou_alice".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@beetle.cn".to_string()],
                aliases: vec!["阿丽丝".to_string()],
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
        let tool = MailTool::with_office_authority_and_contacts_service(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hello","text_body":"Need sync","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send via remote contacts directory with work hint");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["account_key"], "mail-work");
        assert_eq!(payload["message"]["from"], "work@example.com");
        assert_eq!(
            payload["resolved_contacts"][0]["account_key"],
            "contacts-feishu"
        );
        assert_eq!(payload["resolved_contacts"][0]["identity_class"], "work");
    }

    #[test]
    fn mail_tool_send_remote_contact_prefers_matching_mail_provider_over_default_work_provider() {
        let imap_provider = Arc::new(StubProvider::default());
        let feishu_provider = Arc::new(StubProvider {
            provider_name: "feishu_mail",
            display_name: "Feishu Mail",
            ..StubProvider::default()
        });
        let mut providers = MailProviderRegistry::new();
        providers.register(imap_provider);
        providers.register(feishu_provider.clone());

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, provider_name, account_label, account_id) in [
            (
                "mail-work-imap",
                "imap_smtp",
                "Work IMAP",
                "work@example.com",
            ),
            (
                "mail-work-feishu",
                "feishu_mail",
                "Work Feishu",
                "work@feishu.cn",
            ),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: provider_name.to_string(),
                        account_id: account_id.to_string(),
                        account_label: account_label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: account_label.to_string(),
                    },
                );
        }

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work-imap".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work IMAP".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        registry.insert(OfficeAccount {
            account_key: "mail-work-feishu".to_string(),
            provider_kind: "feishu_mail".to_string(),
            external_account_id: "work@feishu.cn".to_string(),
            account_label: "Work Feishu".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
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
        binding.set_default_account(OfficeCapability::Mail, "mail-work-imap".to_string());
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for (account_key, access_token, metadata) in [
            (
                "mail-work-imap",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            (
                "mail-work-feishu",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@feishu.cn".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.feishu.cn".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.feishu.cn".to_string(),
                    ),
                ]
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
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            runtime_store,
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
                id: "ou_alice".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@beetle.cn".to_string()],
                aliases: vec!["阿丽丝".to_string()],
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
        let tool = MailTool::with_office_authority_and_contacts_service(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hello","text_body":"Need sync","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send via remote contacts directory with provider hint");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");

        assert_eq!(payload["provider"], "feishu_mail");
        assert_eq!(payload["message"]["account_key"], "mail-work-feishu");
        assert_eq!(payload["message"]["provider"], "feishu_mail");
        assert_eq!(payload["message"]["from"], "work@feishu.cn");
        assert_eq!(
            payload["resolved_contacts"][0]["provider"],
            "feishu_contacts_directory"
        );
        assert_eq!(
            payload["resolved_contacts"][0]["account_key"],
            "contacts-feishu"
        );
        assert_eq!(payload["resolved_contacts"][0]["identity_class"], "work");

        let sent_requests = feishu_provider
            .sent_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(sent_requests.len(), 1);
        assert_eq!(sent_requests[0].to, vec!["alice@beetle.cn".to_string()]);
    }

    #[test]
    fn mail_tool_send_remote_contact_provider_hint_resolves_without_mail_default_binding() {
        let imap_provider = Arc::new(StubProvider::default());
        let feishu_provider = Arc::new(StubProvider {
            provider_name: "feishu_mail",
            display_name: "Feishu Mail",
            ..StubProvider::default()
        });
        let mut providers = MailProviderRegistry::new();
        providers.register(imap_provider);
        providers.register(feishu_provider.clone());

        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, provider_name, account_label, account_id) in [
            (
                "mail-work-imap",
                "imap_smtp",
                "Work IMAP",
                "work@example.com",
            ),
            (
                "mail-work-feishu",
                "feishu_mail",
                "Work Feishu",
                "work@feishu.cn",
            ),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: provider_name.to_string(),
                        account_id: account_id.to_string(),
                        account_label: account_label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: account_label.to_string(),
                    },
                );
        }

        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work-imap".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work IMAP".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        registry.insert(OfficeAccount {
            account_key: "mail-work-feishu".to_string(),
            provider_kind: "feishu_mail".to_string(),
            external_account_id: "work@feishu.cn".to_string(),
            account_label: "Work Feishu".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
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
        binding.set_default_account(
            OfficeCapability::ContactsDirectory,
            "contacts-feishu".to_string(),
        );
        let office_credential_store = Arc::new(StubOfficeCredentialStore::default());
        for (account_key, access_token, metadata) in [
            (
                "mail-work-imap",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            (
                "mail-work-feishu",
                "secret",
                [
                    (
                        crate::mail::OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@feishu.cn".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.feishu.cn".to_string(),
                    ),
                    (
                        crate::mail::OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.feishu.cn".to_string(),
                    ),
                ]
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
        let runtime_store = Arc::new(StubRuntimeStatusStore::default());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            office_credential_store,
            runtime_store,
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
                id: "ou_alice".to_string(),
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@beetle.cn".to_string()],
                aliases: vec!["阿丽丝".to_string()],
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
        let tool = MailTool::with_office_authority_and_contacts_service(
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
            contacts_service,
        );

        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"send","subject":"Hello","text_body":"Need sync","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("send via remote contacts directory without mail default");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");

        assert_eq!(payload["provider"], "feishu_mail");
        assert_eq!(payload["message"]["account_key"], "mail-work-feishu");
        assert_eq!(payload["message"]["provider"], "feishu_mail");
        assert_eq!(payload["message"]["from"], "work@feishu.cn");
    }

    #[test]
    fn mail_tool_reply_uses_original_message_and_updates_runtime_status() {
        let (tool, provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"reply","id":"42","text_body":"Thanks for the update.","confirm":true}"#,
                &mut ctx,
            )
            .expect("reply mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["subject"], "Re: hello");
        assert_eq!(
            payload["office_runtime_status"]["last_activity_kind"],
            "mail_reply"
        );
        let requests = provider
            .sent_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].to, vec!["reply@example.com".to_string()]);
        assert_eq!(requests[0].in_reply_to, "<msg-1@example.com>");
        assert!(requests[0].text_body.contains("Thanks for the update."));
        assert!(requests[0].text_body.contains("body"));
    }

    #[test]
    fn mail_tool_forward_resolves_lookup_recipients() {
        let (tool, provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"forward","id":"42","text_body":"FYI","to_lookup":["Alice Zhang"],"confirm":true}"#,
                &mut ctx,
            )
            .expect("forward mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["subject"], "Fwd: hello");
        assert_eq!(payload["message"]["to"][0], "alice@example.com");
        assert_eq!(payload["resolved_contacts"][0]["field"], "to");
        let requests = provider
            .sent_requests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        assert_eq!(requests.len(), 1);
        assert!(requests[0].text_body.contains("FYI"));
        assert!(requests[0].text_body.contains("body"));
    }

    #[test]
    fn mail_tool_draft_returns_remote_draft_summary() {
        let (tool, _provider, _runtime_store) = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(
                r#"{"op":"draft","subject":"Draft note","text_body":"Work in progress","confirm":true}"#,
                &mut ctx,
            )
            .expect("save draft");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["mailbox"], "Drafts");
        assert_eq!(
            payload["office_runtime_status"]["last_activity_kind"],
            "mail_draft"
        );
    }

    #[test]
    fn mail_tool_reloads_office_default_after_accounts_commit() {
        let credential_store = Arc::new(StubCredentialStore::default());
        for (account_key, label, account_id) in [
            ("mail-work", "Work", "work@example.com"),
            ("mail-personal", "Personal", "personal@example.com"),
        ] {
            credential_store
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(
                    account_key.to_string(),
                    MailProviderCredential {
                        account_key: account_key.to_string(),
                        provider: "imap_smtp".to_string(),
                        account_id: account_id.to_string(),
                        account_label: label.to_string(),
                        username: account_id.to_string(),
                        corp_id: String::new(),
                        secret: "secret".to_string(),
                        base_url: String::new(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        draft_mailbox: "Drafts".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: label.to_string(),
                    },
                );
        }

        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        save_mail_accounts(config_file_store.as_ref(), "mail-work").expect("seed accounts");
        let office_source = Arc::new(ReloadingOfficeAuthoritySource::new(
            config_file_store.clone(),
            Arc::new(StubOfficeCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore::default()),
        ));
        let provider = Arc::new(StubProvider::default());
        let mut providers = MailProviderRegistry::new();
        providers.register(provider);
        let contacts_store = Arc::new(StateFsContactsDirectoryStore::new(Arc::new(
            MemoryStateFs::default(),
        )));
        let tool = MailTool::with_office_authority_and_contacts(
            credential_store,
            providers,
            office_source,
            contacts_store,
        );

        let mut ctx = DummyCtx;
        let first = tool
            .execute(r#"{"op":"list"}"#, &mut ctx)
            .expect("first list");
        let first: Value = serde_json::from_str(&first).expect("valid first list");
        assert_eq!(first["items"][0]["account_key"], "mail-work");

        save_mail_accounts(config_file_store.as_ref(), "mail-personal").expect("update accounts");

        let second = tool
            .execute(r#"{"op":"list"}"#, &mut ctx)
            .expect("second list");
        let second: Value = serde_json::from_str(&second).expect("valid second list");
        assert_eq!(second["items"][0]["account_key"], "mail-personal");

        let status = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .expect("provider status");
        let status: Value = serde_json::from_str(&status).expect("valid status");
        assert_eq!(status["default_mail_account_key"], "mail-personal");
    }

    #[test]
    fn mail_tool_list_returns_structured_office_failure_when_default_account_has_no_credential() {
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
        assert_eq!(payload["office_assessment"]["capability"], "mail");
        assert_eq!(
            payload["office_assessment"]["default_account_key"],
            "mail-work"
        );
        assert_eq!(
            payload["office_assessment"]["account_assessments"][0]["account_key"],
            "mail-work"
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
            "mail-work"
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
    fn mail_tool_list_returns_structured_office_failure_when_mail_accounts_are_ambiguous() {
        let tool = build_office_backed_tool_with_ambiguous_accounts();
        let mut ctx = DummyCtx;

        let outcome = tool
            .execute_outcome(r#"{"op":"list","provider":"imap_smtp"}"#, &mut ctx)
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
        assert_eq!(payload["office_assessment"]["capability"], "mail");
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["status"],
            "ambiguous"
        );
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["candidate_accounts"][0]["account_key"],
            "mail-personal"
        );
        assert_eq!(
            payload["office_assessment"]["resolve_hint"]["candidate_accounts"][1]["account_key"],
            "mail-work"
        );
        assert!(payload["error"]
            .as_str()
            .expect("error string")
            .contains("multiple configured accounts"));
    }
}
