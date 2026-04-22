use crate::error::{Error, Result};
use crate::office::{
    office_config_op_doctrines, office_tool_doctrine, parse_public_account_upsert_request_value,
    OfficeAccountOnboardingDisposition, OfficeCapability, OfficeConfigAssessment,
    OfficeConfigManagementService, OfficeProviderSchema, OfficeResolveRequest,
};
use crate::tools::{
    http_bridge::ToolContextHttpClient, office_args::parse_identity_class_value, parse_tool_args,
    serialize_tool_output, Tool, ToolApprovalMode, ToolClarificationField, ToolClarificationOption,
    ToolContext, ToolEffectClass, ToolExecutionBlocker, ToolExecutionBlockerKind,
    ToolExecutionOutcome, ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
};
use serde::Serialize;
use serde_json::Value;

pub struct OfficeConfigTool {
    service: OfficeConfigManagementService,
}

#[derive(Serialize)]
struct OfficeConfigResponse<T: Serialize> {
    op: &'static str,
    ok: bool,
    payload: T,
}

#[derive(Serialize)]
struct RevokeResponse<'a> {
    account_key: &'a str,
    cleared_runtime_status: bool,
}

#[derive(Serialize)]
struct AssessmentResponse {
    count: usize,
    accounts: Vec<crate::office::OfficeAccountAssessment>,
}

#[derive(Serialize)]
struct ProviderSchemaResponse {
    count: usize,
    providers: Vec<OfficeProviderSchema>,
}

#[derive(Serialize)]
struct OfficeInspectCredentialsResponse {
    items: Vec<crate::office::OfficeCredentialStatus>,
}

#[derive(Serialize)]
struct OfficeInspectSnapshotResponse {
    accounts: crate::config::OfficeAccountsSegment,
    credentials: OfficeInspectCredentialsResponse,
    summary: crate::office::OfficeAuthoritySummary,
}

impl OfficeConfigTool {
    pub fn new(service: OfficeConfigManagementService) -> Self {
        Self { service }
    }
}

fn public_inspect_snapshot(
    snapshot: crate::office::OfficeConfigSnapshot,
) -> OfficeInspectSnapshotResponse {
    let mut credential_statuses = snapshot
        .credentials
        .items
        .into_iter()
        .map(|credential| credential.status())
        .collect::<Vec<_>>();
    credential_statuses.sort_by(|left, right| left.account_key.cmp(&right.account_key));
    OfficeInspectSnapshotResponse {
        accounts: snapshot.accounts,
        credentials: OfficeInspectCredentialsResponse {
            items: credential_statuses,
        },
        summary: snapshot.summary,
    }
}

fn onboarding_blocker_kind(
    disposition: OfficeAccountOnboardingDisposition,
) -> Option<ToolExecutionBlockerKind> {
    match disposition {
        OfficeAccountOnboardingDisposition::Applied => None,
        OfficeAccountOnboardingDisposition::NeedsUserFacts => {
            Some(ToolExecutionBlockerKind::NeedsUserFacts)
        }
        OfficeAccountOnboardingDisposition::ProbeFailed => {
            Some(ToolExecutionBlockerKind::ProbeFailed)
        }
        OfficeAccountOnboardingDisposition::Unsupported => {
            Some(ToolExecutionBlockerKind::Unsupported)
        }
    }
}

fn clarification_field_from_office_schema(
    field: &crate::office::OfficeConfigCreateFieldSchema,
) -> ToolClarificationField {
    ToolClarificationField {
        key: field.key.clone(),
        label: field.label.clone(),
        description: field.description.clone(),
        required: field.required,
        secret: field.secret,
        multiple: field.multiple,
        options: field
            .options
            .iter()
            .map(|option| ToolClarificationOption {
                value: option.value.clone(),
                label: option.label.clone(),
            })
            .collect(),
    }
}

fn onboarding_blocker_summary(
    result: &crate::office::OfficeAccountOnboardingResult,
    _locale: crate::i18n::Locale,
) -> String {
    match result.disposition {
        OfficeAccountOnboardingDisposition::NeedsUserFacts => {
            let field_text = if result.missing_fields.is_empty() {
                "required facts are still missing".to_string()
            } else {
                result.missing_fields.join(", ")
            };
            format!("Account onboarding is blocked: {field_text}")
        }
        OfficeAccountOnboardingDisposition::ProbeFailed => {
            let reason = result
                .error_message
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(result.reason.as_str());
            format!("Account onboarding probe failed: {reason}")
        }
        OfficeAccountOnboardingDisposition::Unsupported => {
            let provider = result
                .provider_kind
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(result.reason.as_str());
            format!("This account onboarding path is not supported: {provider}")
        }
        OfficeAccountOnboardingDisposition::Applied => String::new(),
    }
}

impl Tool for OfficeConfigTool {
    fn name(&self) -> &'static str {
        "office_config"
    }

    fn description(&self) -> &'static str {
        office_tool_doctrine(self.name())
            .map(|doctrine| doctrine.description)
            .unwrap_or(
                "Configure, reconfigure, and repair shared office accounts for mail, calendar, documents, and contacts.",
            )
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation. Mainline path: provider_schema -> apply_account -> resolve_account (when routing is ambiguous). Repair/advanced paths: probe|revoke|inspect|assess."},"capability":{"type":"string","description":"Optional office capability hint: mail|calendar|documents|contacts_directory"},"provider":{"type":"string","description":"Optional provider hint when the user explicitly chose a provider family."},"identity_class":{"type":"string","description":"Required for apply_account. Account identity class: work|personal|family|shared|other."},"account_label":{"type":"string","description":"Optional human-readable account label."},"display_name":{"type":"string","description":"Optional display name or label hint."},"external_account_id":{"type":"string","description":"Optional explicit external account identity when it is not obvious from email/account_id/username."},"email":{"type":"string","description":"Email address for mail-style providers."},"account_id":{"type":"string","description":"Account identifier for providers that use account IDs instead of email."},"username":{"type":"string","description":"Username for providers that use usernames instead of email."},"password":{"type":"string","description":"Password or app password for password-style authentication."},"access_token":{"type":"string","description":"Access token, app secret, or other token-style credential."},"refresh_token":{"type":"string","description":"Optional refresh token when the provider supports it."},"token_endpoint":{"type":"string","description":"Optional token endpoint override for providers that need it."},"imap_host":{"type":"string","description":"IMAP server hostname for IMAP/SMTP providers."},"imap_port":{"type":"integer","description":"Optional IMAP port override."},"imap_tls":{"type":"boolean","description":"Optional IMAP TLS override."},"smtp_host":{"type":"string","description":"SMTP server hostname for IMAP/SMTP providers."},"smtp_port":{"type":"integer","description":"Optional SMTP port override."},"smtp_tls":{"type":"boolean","description":"Optional SMTP TLS override."},"metadata":{"type":"object","description":"Optional provider-specific factual fields such as corp_id, app_id, calendar_id, root_path, or space_id when the provider needs them."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        self.execute_outcome(args, ctx)
            .map(|outcome| outcome.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_office_config")?;
        let Some(op) = obj
            .get("op")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return office_config_op_choice_outcome(None);
        };
        match op {
            "inspect" => Ok(ToolExecutionOutcome::text(serialize_tool_output(
                "tool_office_config",
                &OfficeConfigResponse {
                    op: "inspect",
                    ok: true,
                    payload: public_inspect_snapshot(self.service.inspect()?),
                },
            )?)),
            "assess" => {
                let payload: OfficeConfigAssessment = self
                    .service
                    .assess(obj.get("account_key").and_then(Value::as_str))?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "assess",
                        ok: true,
                        payload: AssessmentResponse {
                            count: payload.accounts.len(),
                            accounts: payload.accounts,
                        },
                    },
                )?))
            }
            "provider_schema" => {
                let capability = match obj.get("capability") {
                    Some(value) => Some(match parse_capability_choice(value) {
                        Ok(capability) => capability,
                        Err(outcome) => return Ok(*outcome),
                    }),
                    None => None,
                };
                let preferred_provider_kind = tool_facing_provider_kind(&obj);
                let providers = self
                    .service
                    .provider_schemas(preferred_provider_kind.as_deref(), capability)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "provider_schema",
                        ok: true,
                        payload: ProviderSchemaResponse {
                            count: providers.len(),
                            providers,
                        },
                    },
                )?))
            }
            "resolve_account" => {
                let Some(capability_value) = obj.get("capability") else {
                    return missing_capability_outcome();
                };
                let capability = match parse_capability_choice(capability_value) {
                    Ok(capability) => capability,
                    Err(outcome) => return Ok(*outcome),
                };
                let preferred_identity_class = obj
                    .get("preferred_identity_class")
                    .map(|value| {
                        parse_identity_class_value(
                            value,
                            "preferred_identity_class",
                            "tool_office_config",
                        )
                    })
                    .transpose()?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "resolve_account",
                        ok: true,
                        payload: self.service.resolve_account(&OfficeResolveRequest {
                            capability,
                            preferred_account_key: obj
                                .get("preferred_account_key")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            preferred_provider_kind: tool_facing_provider_kind(&obj),
                            preferred_identity_class,
                            historical_account_key: None,
                        })?,
                    },
                )?))
            }
            "apply_account" => {
                let request =
                    parse_public_account_upsert_request_value(&obj, "tool_office_config")?;
                let mut http = ToolContextHttpClient::new(ctx);
                let result = self.service.apply_account_with_http(&mut http, &request)?;
                let content = serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "apply_account",
                        ok: matches!(
                            result.disposition,
                            OfficeAccountOnboardingDisposition::Applied
                        ),
                        payload: result.clone(),
                    },
                )?;
                let outcome = ToolExecutionOutcome::text(content);
                let Some(blocker_kind) = onboarding_blocker_kind(result.disposition) else {
                    return Ok(outcome);
                };
                Ok(outcome.with_blocker(ToolExecutionBlocker {
                    kind: blocker_kind,
                    summary: onboarding_blocker_summary(&result, ctx.user_locale()),
                    missing_fields: result.missing_fields.clone(),
                    clarification_fields: result
                        .missing_field_details
                        .iter()
                        .map(clarification_field_from_office_schema)
                        .collect(),
                }))
            }
            "revoke" => {
                if !obj.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
                    return revoke_confirmation_outcome();
                }
                let account_key = obj
                    .get("account_key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_office_config", "missing account_key"))?;
                let clear_runtime_status = obj
                    .get("clear_runtime_status")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.service.revoke(account_key, clear_runtime_status)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "revoke",
                        ok: true,
                        payload: RevokeResponse {
                            account_key,
                            cleared_runtime_status: clear_runtime_status,
                        },
                    },
                )?))
            }
            "probe" => {
                let account_key = obj
                    .get("account_key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_office_config", "missing account_key"))?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "probe",
                        ok: true,
                        payload: {
                            let mut http = ToolContextHttpClient::new(ctx);
                            self.service.probe_with_http(&mut http, account_key)?
                        },
                    },
                )?))
            }
            _ => office_config_op_choice_outcome(Some(op)),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_effect_class(ToolEffectClass::ConfigWrite)
            .with_risk_level(ToolRiskLevel::High)
            .with_rollback_kind(ToolRollbackKind::ConfigRestore)
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_office_config_governance")?;
        let op = obj.get("op").and_then(Value::as_str).unwrap_or("inspect");
        let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
        Ok(match op {
            "revoke" => self
                .metadata()
                .default_execution_shape(op)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm),
            "apply_account" => self.metadata().default_execution_shape(op),
            _ => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_rollback_kind(ToolRollbackKind::None),
        })
    }

    fn governance_examples(&self) -> &'static [&'static str] {
        &[
            r#"{"op":"provider_schema","capability":"mail"}"#,
            r#"{"op":"apply_account"}"#,
            r#"{"op":"resolve_account","capability":"mail"}"#,
        ]
    }

    fn requires_network_for(&self, args: &str) -> Result<bool> {
        let obj = parse_tool_args(args, "tool_office_config_network")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("inspect")
            .trim()
            .to_ascii_lowercase();
        Ok(matches!(op.as_str(), "apply_account" | "probe"))
    }
}

fn parse_capability_value(value: &Value) -> Result<OfficeCapability> {
    let raw = value
        .as_str()
        .ok_or_else(|| Error::config("tool_office_config", "capability must be a string"))?;
    match raw {
        "mail" => Ok(OfficeCapability::Mail),
        "calendar" => Ok(OfficeCapability::Calendar),
        "documents" => Ok(OfficeCapability::Documents),
        "contacts_directory" => Ok(OfficeCapability::ContactsDirectory),
        _ => Err(Error::config(
            "tool_office_config",
            format!("unsupported capability '{}'", raw),
        )),
    }
}

fn parse_capability_choice(
    value: &Value,
) -> std::result::Result<OfficeCapability, Box<ToolExecutionOutcome>> {
    parse_capability_value(value).map_err(|_| Box::new(invalid_capability_outcome()))
}

fn tool_facing_provider_kind(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("provider")
        .or_else(|| obj.get("provider_kind"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| match value {
            "qq" | "qqmail" | "qq_mail" => "imap_smtp".to_string(),
            other => other.to_string(),
        })
}

fn office_config_op_choice_outcome(op: Option<&str>) -> Result<ToolExecutionOutcome> {
    let options = office_config_op_doctrines()
        .iter()
        .map(|doctrine| ToolClarificationOption {
            value: doctrine.op.to_string(),
            label: doctrine.op.to_string(),
        })
        .collect::<Vec<_>>();
    Ok(ToolExecutionOutcome::text(
        serde_json::json!({
            "op": op,
            "ok": false,
            "warning": "office_config: choose a supported operation",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "An operation is still required before office_config can continue.",
        vec!["op".to_string()],
        vec![ToolClarificationField {
            key: "op".to_string(),
            label: "Operation".to_string(),
            description: "Choose the office account operation to perform.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options,
        }],
    )))
}

fn missing_capability_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        serde_json::json!({
            "op": "resolve_account",
            "ok": false,
            "warning": "office_config: missing capability",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "A capability is still required before resolve_account can continue.",
        vec!["capability".to_string()],
        vec![ToolClarificationField {
            key: "capability".to_string(),
            label: "Capability".to_string(),
            description: "Choose which office capability account to resolve.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![
                ToolClarificationOption {
                    value: "mail".to_string(),
                    label: "mail".to_string(),
                },
                ToolClarificationOption {
                    value: "calendar".to_string(),
                    label: "calendar".to_string(),
                },
                ToolClarificationOption {
                    value: "documents".to_string(),
                    label: "documents".to_string(),
                },
                ToolClarificationOption {
                    value: "contacts_directory".to_string(),
                    label: "contacts_directory".to_string(),
                },
            ],
        }],
    )))
}

fn invalid_capability_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "warning": "office_config: capability must be one of mail, calendar, documents, contacts_directory",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "A supported office capability is still required before this tool can continue.",
        vec!["capability".to_string()],
        vec![ToolClarificationField {
            key: "capability".to_string(),
            label: "Capability".to_string(),
            description: "Choose a supported office capability.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![
                ToolClarificationOption { value: "mail".to_string(), label: "mail".to_string() },
                ToolClarificationOption { value: "calendar".to_string(), label: "calendar".to_string() },
                ToolClarificationOption { value: "documents".to_string(), label: "documents".to_string() },
                ToolClarificationOption { value: "contacts_directory".to_string(), label: "contacts_directory".to_string() },
            ],
        }],
    ))
}

fn revoke_confirmation_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        serde_json::json!({
            "op": "revoke",
            "ok": false,
            "warning": "office_config: revoke requires confirm=true",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_confirmation(
        "Explicit confirmation is still required before revoking this account.",
        vec![ToolClarificationField {
            key: "confirm".to_string(),
            label: "Confirm revoke".to_string(),
            description: "Set confirm=true to revoke the account.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![ToolClarificationOption {
                value: "true".to_string(),
                label: "true".to_string(),
            }],
        }],
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFileStore;
    use crate::office::{
        OfficeAccountRuntimeStatus, OfficeCredential, OfficeCredentialStore,
        OfficeProbeDisposition, OfficeRuntimeStatusStore,
    };
    use crate::tools::{
        ToolApprovalMode, ToolEffectClass, ToolExecutionBlockerKind, ToolRiskLevel,
        ToolRollbackKind,
    };
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Arc, Mutex};

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("tool_office_config_test", "network unused"))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("tool_office_config_test", "network unused"))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    struct MemoryConfigFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemoryConfigFileStore {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }
    }

    impl ConfigFileStore for MemoryConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(rel_path)
                .cloned())
        }
        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }
        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for MemoryCredentialStore {
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

    struct ToolFixture {
        tool: OfficeConfigTool,
        credential_store: Arc<MemoryCredentialStore>,
        runtime_status_store: Arc<MemoryRuntimeStatusStore>,
    }

    fn build_fixture() -> ToolFixture {
        build_fixture_with_probe_adapters(Vec::new())
    }

    fn build_fixture_with_probe_adapters(
        probe_adapters: Vec<Arc<dyn crate::office::OfficeProbeAdapter + Send + Sync>>,
    ) -> ToolFixture {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        let credential_store = Arc::new(MemoryCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let tool = OfficeConfigTool::new(
            OfficeConfigManagementService::new(
                config_file_store.clone(),
                credential_store.clone(),
                runtime_status_store.clone(),
            )
            .with_probe_adapters(probe_adapters),
        );
        ToolFixture {
            tool,
            credential_store,
            runtime_status_store,
        }
    }

    #[derive(Clone)]
    struct ReadyProbeAdapter {
        provider_kind: &'static str,
        reason: &'static str,
    }

    impl crate::office::OfficeProbeAdapter for ReadyProbeAdapter {
        fn provider_kind(&self) -> &'static str {
            self.provider_kind
        }

        fn probe(
            &self,
            _http: &mut dyn crate::office::OfficeHttpClient,
            account: &crate::office::OfficeAccount,
            _credential: &crate::office::OfficeCredential,
        ) -> Result<crate::office::OfficeProbeResult> {
            Ok(crate::office::OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: true,
                disposition: OfficeProbeDisposition::Ready,
                reason: self.reason.to_string(),
            })
        }
    }

    #[test]
    fn inspect_returns_snapshot() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(r#"{"op":"inspect"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["op"], "inspect");
        assert!(payload["payload"]["summary"]["accounts"].is_array());
    }

    #[test]
    fn inspect_redacts_raw_credentials_and_returns_status_only() {
        let fixture = build_fixture();
        fixture
            .credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "secret-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                token_endpoint: "https://example.com/token".to_string(),
                expires_at_unix_secs: 42,
                updated_at: 7,
                metadata: BTreeMap::from([("tenant".to_string(), "alpha".to_string())]),
            })
            .expect("seed credential");
        let mut ctx = DummyCtx;

        let payload = fixture
            .tool
            .execute(r#"{"op":"inspect"}"#, &mut ctx)
            .expect("inspect");
        let payload: Value = serde_json::from_str(&payload).expect("valid inspect json");
        let credential = &payload["payload"]["credentials"]["items"][0];

        assert_eq!(credential["account_key"], "mail-work");
        assert_eq!(credential["configured"], true);
        assert_eq!(credential["has_refresh_token"], true);
        assert_eq!(credential["expires_at_unix_secs"], 42);
        assert_eq!(credential["updated_at"], 7);
        assert!(credential.get("access_token").is_none());
        assert!(credential.get("refresh_token").is_none());
        assert!(credential.get("token_endpoint").is_none());
        assert!(credential.get("metadata").is_none());
    }

    #[test]
    fn provider_schema_accepts_top_level_provider_alias() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(r#"{"op":"provider_schema","provider":"qqmail"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["payload"]["count"], 1);
        assert_eq!(
            payload["payload"]["providers"][0]["provider_kind"],
            "imap_smtp"
        );
    }

    #[test]
    fn apply_account_rejects_legacy_nested_account_and_credential_shape() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let error = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "capability":"mail",
                "account":{
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "identity_class":"other",
                    "provider_kind":"qq",
                    "capabilities":["mail"]
                },
                "credential":{
                    "password":"hqvqcibpdvqgbdba",
                    "email":"675778650@qq.com",
                    "imap_host":"imap.qq.com",
                    "imap_port":993,
                    "smtp_host":"smtp.qq.com",
                    "smtp_port":465,
                    "imap_tls":true,
                    "smtp_tls":true
                }
            }"#,
                &mut ctx,
            )
            .expect_err("legacy nested public payload should be rejected");
        assert!(error
            .to_string()
            .contains("legacy public account wrappers are not supported"));
    }

    #[test]
    fn apply_account_rejects_legacy_nested_config_shape() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let error = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "capability":"mail",
                "provider_kind":"qq",
                "identity_class":"other",
                "config":{
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "password":"hqvqcibpdvqgbdba",
                    "imap_host":"imap.qq.com",
                    "imap_port":993,
                    "smtp_host":"smtp.qq.com",
                    "smtp_port":465,
                    "imap_tls":true,
                    "smtp_tls":true
                }
            }"#,
                &mut ctx,
            )
            .expect_err("legacy nested config payload should be rejected");
        assert!(error
            .to_string()
            .contains("legacy public account wrappers are not supported"));
    }

    #[test]
    fn apply_account_public_flat_shape_infers_provider_and_capability_from_transport_facts() {
        let fixture = build_fixture_with_probe_adapters(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "imap_smtp",
            reason: "imap_login_ok",
        })]);
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "identity_class":"other",
                "email":"675778650@qq.com",
                "password":"hqvqcibpdvqgbdba",
                "imap_host":"imap.qq.com",
                "smtp_host":"smtp.qq.com"
            }"#,
                &mut ctx,
            )
            .expect("provider/capability should be inferred");
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["payload"]["provider_kind"], "imap_smtp");
        assert_eq!(
            payload["payload"]["account"]["account"]["enabled_capabilities"],
            json!(["mail"])
        );
    }

    #[test]
    fn apply_account_rejects_legacy_nested_credential_shape() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let error = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "identity_class":"other",
                "credential":{
                    "email":"675778650@qq.com",
                    "password":"hqvqcibpdvqgbdba",
                    "imap_host":"imap.qq.com",
                    "smtp_host":"smtp.qq.com"
                }
            }"#,
                &mut ctx,
            )
            .expect_err("legacy nested credential payload should be rejected");
        assert!(error
            .to_string()
            .contains("legacy public account wrappers are not supported"));
    }

    #[test]
    fn apply_account_public_flat_shape_returns_structured_missing_user_facts() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "capability":"mail",
                "provider_kind":"qq",
                "display_name":"QQ邮箱",
                "password":"hqvqcibpdvqgbdba",
                "imap_host":"imap.qq.com",
                "smtp_host":"smtp.qq.com"
            }"#,
                &mut ctx,
            )
            .expect("missing user facts should return structured payload");
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["payload"]["disposition"], "needs_user_facts");
        assert!(payload["payload"]["missing_fields"]
            .as_array()
            .expect("missing fields")
            .iter()
            .any(|item| item == "mail_username"));
    }

    #[test]
    fn apply_account_public_shape_reports_missing_identity_class_without_persisting() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"apply_account",
                "provider_kind":"imap_smtp",
                "capability":"mail",
                "email":"work@example.com",
                "password":"secret",
                "imap_host":"imap.example.com",
                "smtp_host":"smtp.example.com"
            }"#,
                &mut ctx,
            )
            .expect("missing identity_class should return structured blocker");
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["payload"]["disposition"], "needs_user_facts");
        assert!(payload["payload"]["missing_fields"]
            .as_array()
            .expect("missing fields")
            .iter()
            .any(|item| item == "identity_class"));
        assert!(fixture
            .credential_store
            .list()
            .expect("credentials")
            .is_empty());
    }

    #[test]
    fn apply_account_execute_outcome_reports_structured_blocker_for_missing_user_facts() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(
                r#"{
                "op":"apply_account",
                "provider_kind":"imap_smtp",
                "capability":"mail",
                "email":"work@example.com",
                "password":"secret",
                "imap_host":"imap.example.com",
                "smtp_host":"smtp.example.com"
            }"#,
                &mut ctx,
            )
            .expect("missing identity_class should return structured blocker outcome");
        let blocker = outcome
            .blocker
            .as_ref()
            .expect("tool outcome should carry blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker
            .missing_fields
            .iter()
            .any(|item| item == "identity_class"));
        assert!(blocker
            .clarification_fields
            .iter()
            .any(|field| field.key == "identity_class"));
        assert!(blocker.summary.contains("identity_class"));
        assert!(!outcome.is_success());
    }

    #[test]
    fn apply_account_is_high_risk_config_write_without_extra_confirm() {
        let fixture = build_fixture();
        let shape = fixture
            .tool
            .execution_shape(r#"{"op":"apply_account"}"#)
            .expect("shape");
        assert_eq!(shape.operation, "apply_account");
        assert_eq!(shape.effect_class, ToolEffectClass::ConfigWrite);
        assert_eq!(shape.risk_level, ToolRiskLevel::High);
        assert_eq!(shape.approval_mode, ToolApprovalMode::Automatic);
        assert!(shape.approval_granted);
        assert_eq!(shape.rollback_kind, ToolRollbackKind::ConfigRestore);
    }

    #[test]
    fn office_config_declares_dynamic_network_usage_by_operation() {
        let fixture = build_fixture();

        assert!(fixture.tool.requires_network());
        assert!(!fixture
            .tool
            .requires_network_for(r#"{"op":"inspect"}"#)
            .expect("inspect network classification"));
        assert!(fixture
            .tool
            .requires_network_for(r#"{"op":"apply_account"}"#)
            .expect("apply_account network classification"));
        assert!(fixture
            .tool
            .requires_network_for(r#"{"op":"probe","account_key":"mail-work"}"#)
            .expect("probe network classification"));
    }

    #[test]
    fn resolve_account_returns_sole_candidate_selection() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        crate::config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry":{"accounts":{
                    "mail-work":{
                        "account_key":"mail-work",
                        "provider_kind":"imap_smtp",
                        "external_account_id":"work@example.com",
                        "account_label":"Work",
                        "identity_class":"work",
                        "enabled_capabilities":["mail"]
                    }
                }},
                "policy":{}
            }"#,
        )
        .unwrap();
        let tool = OfficeConfigTool::new(OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        ));
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"resolve_account","capability":"mail"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["payload"]["status"], "selected");
        assert_eq!(payload["payload"]["account_key"], "mail-work");
        assert_eq!(payload["payload"]["selection_reason"], "sole_candidate");
    }

    #[test]
    fn apply_account_applies_atomically_and_persists_ready_runtime_state() {
        let fixture = build_fixture_with_probe_adapters(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "imap_smtp",
            reason: "imap_login_ok",
        })]);
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                &json!({
                    "op": "apply_account",
                    "capability": "mail",
                    "provider_kind": "imap_smtp",
                    "email": "work@example.com",
                    "display_name": "Work",
                    "identity_class": "work",
                    "access_token": "secret-token",
                    "imap_host": "imap.example.com",
                    "smtp_host": "smtp.example.com"
                })
                .to_string(),
                &mut ctx,
            )
            .expect("apply account");
        let payload: Value = serde_json::from_str(&payload).expect("parse apply payload");
        assert_eq!(payload["ok"], true);
        assert_eq!(payload["payload"]["disposition"], "applied");
        let account_key = payload["payload"]["account"]["account"]["account_key"]
            .as_str()
            .expect("account key");
        let credential = fixture
            .credential_store
            .get(account_key)
            .expect("credential lookup")
            .expect("credential persisted");
        assert_eq!(credential.access_token, "secret-token");
        let runtime = fixture
            .runtime_status_store
            .get(account_key)
            .expect("runtime lookup")
            .expect("runtime status persisted");
        assert!(runtime.probe_ok);
        assert!(runtime.last_error.is_empty());
    }

    #[test]
    fn revoke_requires_confirm_and_explicit_intent() {
        let fixture = build_fixture();
        let shape = fixture
            .tool
            .execution_shape(r#"{"op":"revoke"}"#)
            .expect("shape");
        assert_eq!(shape.operation, "revoke");
        assert_eq!(shape.effect_class, ToolEffectClass::ConfigWrite);
        assert_eq!(shape.risk_level, ToolRiskLevel::High);
        assert_eq!(shape.approval_mode, ToolApprovalMode::ExplicitIntent);
        assert!(!shape.approval_granted);
        assert_eq!(shape.rollback_kind, ToolRollbackKind::ConfigRestore);

        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(r#"{"op":"revoke","account_key":"mail-work"}"#, &mut ctx)
            .expect("revoke without confirm should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsConfirmation);
    }

    #[test]
    fn missing_op_returns_choice_blocker() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(r#"{}"#, &mut ctx)
            .expect("missing op should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn unknown_op_returns_choice_blocker() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(r#"{"op":"merge_account"}"#, &mut ctx)
            .expect("unknown op should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn resolve_account_missing_capability_returns_facts_blocker() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(r#"{"op":"resolve_account"}"#, &mut ctx)
            .expect("missing capability should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker
            .missing_fields
            .iter()
            .any(|item| item == "capability"));
    }

    #[test]
    fn resolve_account_invalid_capability_returns_choice_blocker() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(
                r#"{"op":"resolve_account","capability":"mailbox"}"#,
                &mut ctx,
            )
            .expect("invalid capability should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn revoke_without_confirm_returns_confirmation_blocker() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let outcome = fixture
            .tool
            .execute_outcome(r#"{"op":"revoke","account_key":"mail-work"}"#, &mut ctx)
            .expect("missing confirm should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsConfirmation);
    }

    #[test]
    fn assess_reports_missing_fields_and_next_action() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        crate::config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry": {
                    "accounts": {
                        "mail-work": {
                            "account_key": "mail-work",
                            "provider_kind": "imap_smtp",
                            "external_account_id": "",
                            "account_label": "Work",
                            "identity_class": "work",
                            "enabled_capabilities": ["mail"]
                        }
                    }
                },
                "policy": {}
            }"#,
        )
        .expect("seed accounts");
        let tool = OfficeConfigTool::new(OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        ));
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"assess","account_key":"mail-work"}"#, &mut ctx)
            .expect("assess");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(
            payload["payload"]["accounts"][0]["account_key"],
            "mail-work"
        );
        assert_eq!(
            payload["payload"]["accounts"][0]["readiness"],
            "needs_configuration"
        );
        assert_eq!(
            payload["payload"]["accounts"][0]["next_action"],
            "configure_account"
        );
        assert!(payload["payload"]["accounts"][0]["missing_fields"]
            .as_array()
            .expect("missing fields array")
            .iter()
            .any(|item| item == "mail_imap_host"));
        assert!(payload["payload"]["accounts"][0]["missing_field_details"]
            .as_array()
            .expect("missing field details array")
            .iter()
            .any(|item| {
                item["key"] == "mail_imap_host"
                    && item["label"] == "IMAP host"
                    && item["required"] == true
            }));
    }

    #[test]
    fn revoke_clears_credential_and_runtime_status() {
        let fixture = build_fixture();
        fixture
            .credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect("seed credential");
        fixture
            .runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "mail-work".to_string(),
                probe_ok: false,
                last_error: "auth_failed".to_string(),
                last_probe_at_unix_secs: 1,
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 1,
            })
            .expect("seed runtime status");

        let mut ctx = DummyCtx;
        fixture
            .tool
            .execute(
                r#"{"op":"revoke","account_key":"mail-work","confirm":true}"#,
                &mut ctx,
            )
            .expect("revoke");

        assert!(fixture
            .credential_store
            .get("mail-work")
            .expect("credential lookup")
            .is_none());
        assert!(fixture
            .runtime_status_store
            .get("mail-work")
            .expect("runtime lookup")
            .is_none());
    }

    #[test]
    fn provider_schema_reports_wecom_documents_onboarding_contract() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{"op":"provider_schema","provider_kind":"wecom_documents"}"#,
                &mut ctx,
            )
            .expect("provider schema");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["op"], "provider_schema");
        assert_eq!(payload["payload"]["count"], 1);
        assert_eq!(
            payload["payload"]["providers"][0]["provider_kind"],
            "wecom_documents"
        );
        assert!(payload["payload"]["providers"][0]["fields"]
            .as_array()
            .expect("fields array")
            .iter()
            .any(|item| {
                item["key"] == "documents_space_id"
                    && item["location"] == "metadata"
                    && item["value_kind"] == "identifier"
                    && item["required"] == true
            }));
        assert!(payload["payload"]["providers"][0]["fields"]
            .as_array()
            .expect("fields array")
            .iter()
            .any(|item| {
                item["key"] == "documents_base_url"
                    && item["required"] == false
                    && item["default_value"] == crate::office::WECOM_DEFAULT_BASE_URL
            }));
    }

    #[test]
    fn apply_account_normalizes_wecom_documents_defaults_and_trims_values() {
        let fixture = build_fixture_with_probe_adapters(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "wecom_documents",
            reason: "wecom_documents_ok",
        })]);
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                    "op":"apply_account",
                    "capability":"documents",
                    "provider_kind":"wecom_documents",
                    "identity_class":"work",
                    "account_label":"WeCom Docs",
                    "account_id":"docs-wecom",
                    "access_token":"  corp-secret  ",
                    "metadata":{
                        "documents_corp_id":"  wwcorp  ",
                        "documents_space_id":"  space-1  ",
                        "documents_root_path":"  /shared/docs  ",
                        "documents_base_url":"   "
                    }
                }"#,
                &mut ctx,
            )
            .expect("apply account");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        let account_key = payload["payload"]["account"]["account"]["account_key"]
            .as_str()
            .expect("account key");
        let credential = fixture
            .credential_store
            .get(account_key)
            .expect("credential lookup")
            .expect("credential persisted");
        assert_eq!(credential.access_token, "corp-secret");
        assert_eq!(
            credential
                .metadata
                .get("documents_base_url")
                .map(String::as_str),
            Some(crate::office::WECOM_DEFAULT_BASE_URL)
        );
        assert_eq!(
            credential
                .metadata
                .get("documents_root_path")
                .map(String::as_str),
            Some("/shared/docs")
        );
    }

    #[test]
    fn apply_account_infers_wecom_documents_provider_and_capability_from_unique_metadata() {
        let fixture = build_fixture_with_probe_adapters(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "wecom_documents",
            reason: "wecom_documents_ok",
        })]);
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                    "op":"apply_account",
                    "account_id":"wecom-docs",
                    "identity_class":"other",
                    "display_name":"WeCom Docs",
                    "access_token":"corp-secret",
                    "metadata":{
                        "documents_corp_id":"wwcorp",
                        "documents_space_id":"space-1",
                        "documents_root_path":"/shared/docs"
                    }
                }"#,
                &mut ctx,
            )
            .expect("provider/capability should be inferred");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(payload["payload"]["provider_kind"], "wecom_documents");
        assert_eq!(
            payload["payload"]["account"]["account"]["enabled_capabilities"],
            json!(["documents"])
        );
    }

    #[test]
    fn apply_account_accepts_top_level_provider_metadata_without_metadata_wrapper() {
        let fixture = build_fixture_with_probe_adapters(vec![Arc::new(ReadyProbeAdapter {
            provider_kind: "wecom_documents",
            reason: "wecom_documents_ok",
        })]);
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                    "op":"apply_account",
                    "account_id":"wecom-docs",
                    "identity_class":"work",
                    "access_token":"corp-secret",
                    "documents_corp_id":"wwcorp",
                    "documents_space_id":"space-1",
                    "documents_root_path":"/shared/docs"
                }"#,
                &mut ctx,
            )
            .expect("top-level provider facts should normalize");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        let account_key = payload["payload"]["account"]["account"]["account_key"]
            .as_str()
            .expect("account key");
        let credential = fixture
            .credential_store
            .get(account_key)
            .expect("credential lookup")
            .expect("credential persisted");
        assert_eq!(
            credential
                .metadata
                .get("documents_corp_id")
                .map(String::as_str),
            Some("wwcorp")
        );
        assert_eq!(
            credential
                .metadata
                .get("documents_space_id")
                .map(String::as_str),
            Some("space-1")
        );
        assert_eq!(
            credential
                .metadata
                .get("documents_root_path")
                .map(String::as_str),
            Some("/shared/docs")
        );
    }

    #[test]
    fn office_config_public_schema_hides_internal_contract_fields() {
        let fixture = build_fixture();
        let schema = fixture.tool.schema();

        for hidden in [
            "preferred_account_key",
            "preferred_identity_class",
            "set_defaults",
            "clear_defaults",
            "policy_patch",
            "\"account\":{",
            "\"config\":{",
            "\"credential\":{",
            "\"account_key\":",
            "clear_runtime_status",
            "\"confirm\":",
        ] {
            assert!(
                !schema.contains(hidden),
                "public office_config schema should not expose internal field marker {hidden}: {schema}"
            );
        }

        for visible in [
            "\"identity_class\":",
            "\"email\":",
            "\"account_id\":",
            "\"username\":",
            "\"password\":",
            "\"access_token\":",
            "\"imap_host\":",
            "\"smtp_host\":",
        ] {
            assert!(
                schema.contains(visible),
                "public office_config schema should describe visible onboarding field marker {visible}: {schema}"
            );
        }
    }

    #[test]
    fn office_config_description_keeps_full_capability_but_highlights_mainline_ops() {
        let fixture = build_fixture();
        let description = fixture.tool.description();

        assert!(
            description.contains("provider_schema"),
            "office_config description should explicitly call out provider_schema as part of the mainline path: {description}"
        );
        assert!(
            description.contains("apply_account"),
            "office_config description should explicitly call out apply_account as part of the mainline path: {description}"
        );
        assert!(
            description.contains("repair") || description.contains("advanced"),
            "office_config description should preserve repair/advanced management depth: {description}"
        );
    }

    #[test]
    fn office_config_choice_outcome_orders_mainline_ops_before_repair_ops() {
        let outcome = office_config_op_choice_outcome(None).expect("choice outcome");
        let blocker = outcome.blocker.expect("blocker");
        let op_field = blocker
            .clarification_fields
            .into_iter()
            .find(|field| field.key == "op")
            .expect("op field");
        let values = op_field
            .options
            .into_iter()
            .map(|option| option.value)
            .collect::<Vec<_>>();

        assert_eq!(
            values,
            vec![
                "provider_schema".to_string(),
                "apply_account".to_string(),
                "resolve_account".to_string(),
                "probe".to_string(),
                "revoke".to_string(),
                "inspect".to_string(),
                "assess".to_string(),
            ],
            "office_config should list mainline ops before repair/advanced ops"
        );
    }
}
