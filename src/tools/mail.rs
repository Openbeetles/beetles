//! Mail tool: provider-backed office mail access over shared office authority.

use crate::contacts_directory::{
    ContactsDirectoryEmailResolution, ContactsDirectoryService, ContactsDirectoryStore,
};
use crate::error::{Error, Result};
use crate::mail::{
    MailMessage, MailMessageSummary, MailProviderCredentialStatus, MailProviderCredentialStore,
    MailProviderRegistry, MailQuery, MailSendRequest, MailService,
};
use crate::office::{OfficeAccountRuntimeStatus, OfficeService};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolContext, ToolEffectClass,
    ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
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
struct MailGetResponse {
    op: &'static str,
    provider: String,
    message: MailMessage,
}

#[derive(Serialize)]
struct MailSendResponse {
    op: &'static str,
    ok: bool,
    provider: String,
    message: MailMessageSummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    resolved_contacts: Vec<MailResolvedContact>,
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
        Self::with_runtime(credential_store, providers, Some(office_service), None)
    }

    pub fn with_office_service_and_contacts(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: OfficeService,
        contacts_store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
    ) -> Self {
        Self::with_runtime(
            credential_store,
            providers,
            Some(office_service),
            Some(ContactsDirectoryService::new(contacts_store)),
        )
    }

    fn with_runtime(
        credential_store: Arc<dyn MailProviderCredentialStore + Send + Sync>,
        providers: MailProviderRegistry,
        office_service: Option<OfficeService>,
        contacts_directory: Option<ContactsDirectoryService>,
    ) -> Self {
        Self {
            service: MailService::with_office_service(credential_store, providers, office_service),
            contacts_directory,
        }
    }
}

impl Tool for MailTool {
    fn name(&self) -> &'static str {
        "mail"
    }

    fn description(&self) -> &'static str {
        "Access office mail through shared account authority. Ops: provider_status, list, get, send. Provider can be omitted when office mail defaults or a single configured provider make routing unambiguous."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: provider_status|list|get|send"},"provider":{"type":"string","description":"Optional mail provider. Omit only when office defaults or a single configured provider make routing unambiguous."},"account_key":{"type":"string","description":"Optional explicit office mail account key."},"mailbox":{"type":"string","description":"Mailbox to query for list. Defaults to provider mailbox."},"unread_only":{"type":"boolean","description":"Whether list should only include unread mail."},"received_after_unix_secs":{"type":"integer","description":"Optional lower bound for received time."},"limit":{"type":"integer","description":"List limit, default 10, max 50."},"id":{"type":"string","description":"Message ID or provider UID for get."},"subject":{"type":"string","description":"Mail subject for send."},"text_body":{"type":"string","description":"Mail body for send."},"to":{"type":"array","items":{"type":"string"},"description":"Primary recipient email addresses for send."},"cc":{"type":"array","items":{"type":"string"},"description":"CC recipient email addresses for send."},"bcc":{"type":"array","items":{"type":"string"},"description":"BCC recipient email addresses for send."},"to_lookup":{"type":"array","items":{"type":"string"},"description":"Primary recipient contact queries resolved through contacts_directory."},"cc_lookup":{"type":"array","items":{"type":"string"},"description":"CC recipient contact queries resolved through contacts_directory."},"bcc_lookup":{"type":"array","items":{"type":"string"},"description":"BCC recipient contact queries resolved through contacts_directory."},"confirm":{"type":"boolean","description":"Must be true for send."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_mail")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_mail", "missing op"))?;
        match op {
            "provider_status" => {
                let registered_remote_providers = self
                    .service
                    .provider_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let configured_providers = self.service.list_provider_statuses()?;
                serialize_tool_output(
                    "tool_mail",
                    &MailProviderStatusResponse {
                        op: "provider_status",
                        registered_remote_providers,
                        default_mail_account_key: self.service.office_default_account_key(),
                        configured_providers,
                        office_runtime_statuses: self.service.office_runtime_statuses()?,
                    },
                )
            }
            "list" => {
                let provider = self
                    .service
                    .resolve_provider_name(parse_provider(&obj).as_deref())?;
                let items = self.service.list(
                    &provider,
                    parse_account_key(&obj).as_deref(),
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
                )?;
                serialize_tool_output(
                    "tool_mail",
                    &MailListResponse {
                        op: "list",
                        provider,
                        count: items.len(),
                        items,
                    },
                )
            }
            "get" => {
                let provider = self
                    .service
                    .resolve_provider_name(parse_provider(&obj).as_deref())?;
                let id = required_str(&obj, "id")?;
                let message = self
                    .service
                    .get(&provider, parse_account_key(&obj).as_deref(), id)?
                    .ok_or_else(|| Error::config("tool_mail", "message not found"))?;
                serialize_tool_output(
                    "tool_mail",
                    &MailGetResponse {
                        op: "get",
                        provider,
                        message,
                    },
                )
            }
            "send" => {
                require_confirm(&obj, "send")?;
                let provider = self
                    .service
                    .resolve_provider_name(parse_provider(&obj).as_deref())?;
                let (to_lookup, to_lookup_resolved) =
                    self.resolve_recipient_queries(&obj, "to_lookup", "to")?;
                let (cc_lookup, cc_lookup_resolved) =
                    self.resolve_recipient_queries(&obj, "cc_lookup", "cc")?;
                let (bcc_lookup, bcc_lookup_resolved) =
                    self.resolve_recipient_queries(&obj, "bcc_lookup", "bcc")?;
                let to = merge_recipients(parse_recipients(&obj, "to")?, to_lookup);
                let cc = merge_recipients(parse_recipients(&obj, "cc")?, cc_lookup);
                let bcc = merge_recipients(parse_recipients(&obj, "bcc")?, bcc_lookup);
                if to.is_empty() && cc.is_empty() && bcc.is_empty() {
                    return Err(Error::config(
                        "tool_mail",
                        "send requires at least one recipient in to, cc, bcc, or *_lookup",
                    ));
                }
                let message = self.service.send(
                    &provider,
                    parse_account_key(&obj).as_deref(),
                    &MailSendRequest {
                        subject: required_str(&obj, "subject")?.to_string(),
                        text_body: required_str(&obj, "text_body")?.to_string(),
                        to,
                        cc,
                        bcc,
                    },
                )?;
                serialize_tool_output(
                    "tool_mail",
                    &MailSendResponse {
                        op: "send",
                        ok: true,
                        provider,
                        message,
                        resolved_contacts: [
                            to_lookup_resolved,
                            cc_lookup_resolved,
                            bcc_lookup_resolved,
                        ]
                        .into_iter()
                        .flatten()
                        .collect(),
                    },
                )
            }
            _ => Err(Error::config("tool_mail", format!("unknown op '{}'", op))),
        }
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
            "send" => self
                .metadata()
                .default_execution_shape("mail_send")
                .with_approval_granted(confirm),
            "provider_status" | "list" | "get" => self
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
        Ok(matches!(op, "list" | "get" | "send"))
    }
}

impl MailTool {
    fn resolve_recipient_queries(
        &self,
        obj: &serde_json::Map<String, Value>,
        field: &str,
        surface: &'static str,
    ) -> Result<(Vec<String>, Vec<MailResolvedContact>)> {
        let queries = parse_recipients(obj, field)?;
        if queries.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        let Some(directory) = self.contacts_directory.as_ref() else {
            return Err(Error::config(
                "tool_mail",
                format!("{field} requires contacts_directory support"),
            ));
        };
        let mut emails = Vec::with_capacity(queries.len());
        let mut resolved = Vec::with_capacity(queries.len());
        for query in queries {
            let resolution = directory.resolve_primary_email(&query)?;
            emails.push(resolution.email.clone());
            resolved.push(mail_resolved_contact(surface, resolution));
        }
        Ok((emails, resolved))
    }
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
    MailResolvedContact {
        field,
        query: resolution.query,
        contact_id: resolution.contact_id,
        display_name: resolution.display_name,
        email: resolution.email,
        match_reason: resolution.match_reason,
        score: resolution.score,
    }
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
    use crate::contacts_directory::{ContactsDirectoryStore, StateFsContactsDirectoryStore};
    use crate::mail::{MailOperation, MailProvider, MailProviderCredential};
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy,
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
    struct StubOfficeCredentialStore;

    impl OfficeCredentialStore for StubOfficeCredentialStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(Vec::new())
        }

        fn set(&self, _credential: &OfficeCredential) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
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
            Ok(vec![OfficeAccountRuntimeStatus {
                account_key: "mail-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 7,
                updated_at: 8,
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

    struct StubProvider;

    impl MailProvider for StubProvider {
        fn provider_name(&self) -> &'static str {
            "imap_smtp"
        }

        fn display_name(&self) -> &'static str {
            "IMAP/SMTP"
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
            }))
        }

        fn send_message(
            &self,
            credential: &MailProviderCredential,
            request: &MailSendRequest,
        ) -> Result<MailMessageSummary> {
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
    }

    fn build_tool() -> MailTool {
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
                        secret: "secret".to_string(),
                        imap_host: "imap.example.com".to_string(),
                        imap_port: 993,
                        imap_mailbox: "INBOX".to_string(),
                        imap_tls: true,
                        smtp_host: "smtp.example.com".to_string(),
                        smtp_port: 465,
                        smtp_tls: true,
                        from_address: account_id.to_string(),
                        from_name: label.to_string(),
                    },
                );
        }

        let mut providers = MailProviderRegistry::new();
        providers.register(Arc::new(StubProvider));

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
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore),
            Arc::new(StubRuntimeStatusStore),
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

        MailTool::with_office_service_and_contacts(
            credential_store,
            providers,
            office_service,
            contacts_store,
        )
    }

    #[test]
    fn mail_tool_provider_status_reports_defaults_and_runtime() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .expect("provider status");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["registered_remote_providers"][0], "imap_smtp");
        assert_eq!(payload["default_mail_account_key"], "mail-work");
        assert_eq!(payload["configured_providers"][0]["provider"], "imap_smtp");
        assert_eq!(payload["office_runtime_statuses"][0]["probe_ok"], true);
    }

    #[test]
    fn mail_tool_list_routes_via_office_default_when_provider_is_omitted() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"list"}"#, &mut ctx)
            .expect("list mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["provider"], "imap_smtp");
        assert_eq!(payload["items"][0]["account_key"], "mail-work");
    }

    #[test]
    fn mail_tool_get_returns_message_body() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"get","provider":"imap_smtp","id":"42"}"#, &mut ctx)
            .expect("get mail");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["message"]["summary"]["id"], "42");
        assert_eq!(payload["message"]["text_body"], "body");
    }

    #[test]
    fn mail_tool_send_requires_confirm_and_returns_summary() {
        let tool = build_tool();
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
        let tool = build_tool();
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
}
