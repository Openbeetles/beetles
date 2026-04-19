use crate::error::{Error, Result};
use crate::office::{
    parse_public_account_upsert_request_value, OfficeAccountOnboardingDisposition,
    OfficeCapability, OfficeConfigAssessment, OfficeConfigManagementService, OfficeProviderSchema,
    OfficeResolveRequest,
};
use crate::tools::{
    http_bridge::ToolContextHttpClient, office_args::parse_identity_class_value, parse_tool_args,
    serialize_tool_output, Tool, ToolApprovalMode, ToolContext, ToolEffectClass,
    ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
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

impl OfficeConfigTool {
    pub fn new(service: OfficeConfigManagementService) -> Self {
        Self { service }
    }
}

impl Tool for OfficeConfigTool {
    fn name(&self) -> &'static str {
        "office_config"
    }

    fn description(&self) -> &'static str {
        "Configure, reconfigure, inspect, resolve, revoke, or probe shared office accounts for mail, calendar, documents, and contacts. Use this for account onboarding, provider schema lookup, account resolution, and account repair."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: provider_schema|resolve_account|apply_account"},"capability":{"type":"string","description":"Optional office capability hint: mail|calendar|documents|contacts_directory"},"provider":{"type":"string","description":"Optional provider hint when the user explicitly chose a provider family."},"identity_class":{"type":"string","description":"Required for apply_account. Account identity class: work|personal|family|shared|other."},"account_label":{"type":"string","description":"Optional human-readable account label."},"display_name":{"type":"string","description":"Optional display name or label hint."},"external_account_id":{"type":"string","description":"Optional explicit external account identity when it is not obvious from email/account_id/username."},"email":{"type":"string","description":"Email address for mail-style providers."},"account_id":{"type":"string","description":"Account identifier for providers that use account IDs instead of email."},"username":{"type":"string","description":"Username for providers that use usernames instead of email."},"password":{"type":"string","description":"Password or app password for password-style authentication."},"access_token":{"type":"string","description":"Access token, app secret, or other token-style credential."},"refresh_token":{"type":"string","description":"Optional refresh token when the provider supports it."},"token_endpoint":{"type":"string","description":"Optional token endpoint override for providers that need it."},"imap_host":{"type":"string","description":"IMAP server hostname for IMAP/SMTP providers."},"imap_port":{"type":"integer","description":"Optional IMAP port override."},"imap_tls":{"type":"boolean","description":"Optional IMAP TLS override."},"smtp_host":{"type":"string","description":"SMTP server hostname for IMAP/SMTP providers."},"smtp_port":{"type":"integer","description":"Optional SMTP port override."},"smtp_tls":{"type":"boolean","description":"Optional SMTP TLS override."},"metadata":{"type":"object","description":"Optional provider-specific factual fields such as corp_id, app_id, calendar_id, root_path, or space_id when the provider needs them."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_office_config")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_office_config", "missing op"))?;
        match op {
            "inspect" => serialize_tool_output(
                "tool_office_config",
                &OfficeConfigResponse {
                    op: "inspect",
                    ok: true,
                    payload: self.service.inspect()?,
                },
            ),
            "assess" => {
                let payload: OfficeConfigAssessment = self
                    .service
                    .assess(obj.get("account_key").and_then(Value::as_str))?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "assess",
                        ok: true,
                        payload: AssessmentResponse {
                            count: payload.accounts.len(),
                            accounts: payload.accounts,
                        },
                    },
                )
            }
            "provider_schema" => {
                let capability = obj
                    .get("capability")
                    .map(parse_capability_value)
                    .transpose()?;
                let preferred_provider_kind = tool_facing_provider_kind(&obj);
                let providers = self
                    .service
                    .provider_schemas(preferred_provider_kind.as_deref(), capability)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "provider_schema",
                        ok: true,
                        payload: ProviderSchemaResponse {
                            count: providers.len(),
                            providers,
                        },
                    },
                )
            }
            "resolve_account" => {
                let capability =
                    parse_capability_value(obj.get("capability").ok_or_else(|| {
                        Error::config("tool_office_config", "missing capability")
                    })?)?;
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
                serialize_tool_output(
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
                )
            }
            "apply_account" => {
                let request =
                    parse_public_account_upsert_request_value(&obj, "tool_office_config")?;
                let mut http = ToolContextHttpClient::new(ctx);
                let result = self.service.apply_account_with_http(&mut http, &request)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "apply_account",
                        ok: matches!(
                            result.disposition,
                            OfficeAccountOnboardingDisposition::Applied
                        ),
                        payload: result,
                    },
                )
            }
            "revoke" => {
                require_confirm(&obj, "revoke")?;
                let account_key = obj
                    .get("account_key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_office_config", "missing account_key"))?;
                let clear_runtime_status = obj
                    .get("clear_runtime_status")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                self.service.revoke(account_key, clear_runtime_status)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "revoke",
                        ok: true,
                        payload: RevokeResponse {
                            account_key,
                            cleared_runtime_status: clear_runtime_status,
                        },
                    },
                )
            }
            "probe" => {
                let account_key = obj
                    .get("account_key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_office_config", "missing account_key"))?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "probe",
                        ok: true,
                        payload: {
                            let mut http = ToolContextHttpClient::new(ctx);
                            self.service.probe_with_http(&mut http, account_key)?
                        },
                    },
                )
            }
            _ => Err(Error::config(
                "tool_office_config",
                format!("unknown op '{}'", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_effect_class(ToolEffectClass::ConfigWrite)
            .with_risk_level(ToolRiskLevel::High)
            .with_rollback_kind(ToolRollbackKind::ConfigRestore)
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
            r#"{"op":"resolve_account","capability":"mail"}"#,
            r#"{"op":"apply_account"}"#,
        ]
    }
}

fn require_confirm(obj: &serde_json::Map<String, Value>, op: &str) -> Result<()> {
    if obj.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
        Ok(())
    } else {
        Err(Error::config(
            "tool_office_config",
            format!("{op} requires confirm=true"),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFileStore;
    use crate::office::{
        OfficeAccountRuntimeStatus, OfficeCredential, OfficeCredentialStore,
        OfficeProbeDisposition, OfficeRuntimeStatusStore,
    };
    use crate::tools::{ToolApprovalMode, ToolEffectClass, ToolRiskLevel, ToolRollbackKind};
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
        let error = fixture
            .tool
            .execute(r#"{"op":"revoke","account_key":"mail-work"}"#, &mut ctx)
            .expect_err("revoke without confirm must fail");
        assert!(error.to_string().contains("revoke requires confirm=true"));
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
}
