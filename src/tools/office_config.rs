use crate::config::OfficeAccountsSegment;
use crate::error::{Error, Result};
use crate::mail::{
    OFFICE_METADATA_MAIL_FROM_ADDRESS, OFFICE_METADATA_MAIL_IMAP_HOST,
    OFFICE_METADATA_MAIL_IMAP_PORT, OFFICE_METADATA_MAIL_IMAP_TLS, OFFICE_METADATA_MAIL_SMTP_HOST,
    OFFICE_METADATA_MAIL_SMTP_PORT, OFFICE_METADATA_MAIL_SMTP_TLS, OFFICE_METADATA_MAIL_USERNAME,
};
use crate::office::{
    OfficeAccount, OfficeAccountDraftRequest, OfficeAccountIdentityClass, OfficeCapability,
    OfficeConfigAssessment, OfficeConfigManagementService, OfficeCredential,
    OfficeCredentialDraftRequest, OfficeCredentialsSegment, OfficeProviderSchema,
    OfficeResolveRequest,
};
use crate::tools::{
    office_args::parse_identity_class_value, parse_tool_args, serialize_tool_output, Tool,
    ToolApprovalMode, ToolContext, ToolEffectClass, ToolExecutionShape, ToolMetadata,
    ToolRiskLevel, ToolRollbackKind,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

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
struct ValidationResponse<'a> {
    target: &'a str,
    valid: bool,
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
        "Manage office authority through structured operations. Ops: inspect, assess, provider_schema, resolve_account, draft_accounts, draft_credentials, validate_accounts, validate_credentials, commit_accounts, commit_credentials, revoke, probe."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: inspect|assess|provider_schema|resolve_account|draft_accounts|draft_credentials|validate_accounts|validate_credentials|commit_accounts|commit_credentials|revoke|probe"},"capability":{"type":"string","description":"Office capability: mail|calendar|documents|contacts_directory"},"provider_kind":{"type":"string","description":"Optional provider kind for provider_schema"},"preferred_account_key":{"type":"string","description":"Optional explicit account preference for resolve_account"},"preferred_identity_class":{"type":"string","description":"Optional identity class for resolve_account: work|personal|family|shared|other"},"account":{"type":"object","description":"OfficeAccount payload for draft_accounts"},"set_defaults":{"type":"array","items":{"type":"string"},"description":"Capabilities that should default to account.account_key"},"clear_defaults":{"type":"array","items":{"type":"string"},"description":"Capabilities whose default binding should be cleared when pointing at account.account_key"},"policy_patch":{"type":"object","description":"Optional OfficePolicyPatch payload for draft_accounts"},"credential":{"type":"object","description":"OfficeCredential payload for draft_credentials"},"segment":{"type":"object","description":"OfficeAccountsSegment or OfficeCredentialsSegment payload for validate/commit ops"},"account_key":{"type":"string","description":"Optional account key for assess, revoke, or probe"},"clear_runtime_status":{"type":"boolean","description":"Whether revoke should also clear runtime status; default true"},"confirm":{"type":"boolean","description":"Required for commit_* and revoke"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
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
            "draft_accounts" => {
                let request = parse_account_draft_request(&obj)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "draft_accounts",
                        ok: true,
                        payload: self.service.draft_accounts(&request)?,
                    },
                )
            }
            "draft_credentials" => {
                let request = parse_credential_draft_request(&obj)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "draft_credentials",
                        ok: true,
                        payload: self.service.draft_credentials(&request)?,
                    },
                )
            }
            "validate_accounts" => {
                let segment = parse_accounts_segment(&obj)?;
                self.service.validate_accounts(&segment)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "validate_accounts",
                        ok: true,
                        payload: ValidationResponse {
                            target: "accounts",
                            valid: true,
                        },
                    },
                )
            }
            "validate_credentials" => {
                let segment = parse_credentials_segment(&obj)?;
                self.service.validate_credentials(&segment)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "validate_credentials",
                        ok: true,
                        payload: ValidationResponse {
                            target: "credentials",
                            valid: true,
                        },
                    },
                )
            }
            "commit_accounts" => {
                require_confirm(&obj, "commit_accounts")?;
                let segment = parse_accounts_segment(&obj)?;
                self.service.commit_accounts(&segment)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "commit_accounts",
                        ok: true,
                        payload: ValidationResponse {
                            target: "accounts",
                            valid: true,
                        },
                    },
                )
            }
            "commit_credentials" => {
                require_confirm(&obj, "commit_credentials")?;
                let segment = parse_credentials_segment(&obj)?;
                self.service.commit_credentials(&segment)?;
                serialize_tool_output(
                    "tool_office_config",
                    &OfficeConfigResponse {
                        op: "commit_credentials",
                        ok: true,
                        payload: ValidationResponse {
                            target: "credentials",
                            valid: true,
                        },
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
                        payload: self.service.probe(account_key)?,
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
            "commit_accounts" | "commit_credentials" | "revoke" => self
                .metadata()
                .default_execution_shape(op)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm),
            _ => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_rollback_kind(ToolRollbackKind::None),
        })
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

fn parse_accounts_segment(obj: &serde_json::Map<String, Value>) -> Result<OfficeAccountsSegment> {
    serde_json::from_value(
        obj.get("segment")
            .cloned()
            .ok_or_else(|| Error::config("tool_office_config", "missing segment"))?,
    )
    .map_err(|error| Error::config("tool_office_config", error.to_string()))
}

fn parse_credentials_segment(
    obj: &serde_json::Map<String, Value>,
) -> Result<OfficeCredentialsSegment> {
    serde_json::from_value(
        obj.get("segment")
            .cloned()
            .ok_or_else(|| Error::config("tool_office_config", "missing segment"))?,
    )
    .map_err(|error| Error::config("tool_office_config", error.to_string()))
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

fn parse_account_draft_request(obj: &Map<String, Value>) -> Result<OfficeAccountDraftRequest> {
    match serde_json::from_value::<OfficeAccountDraftRequest>(Value::Object(obj.clone())) {
        Ok(request) => apply_tool_facing_account_aliases(request, obj),
        Err(_) => normalize_tool_facing_account_draft_request(obj),
    }
}

fn normalize_tool_facing_account_draft_request(
    obj: &Map<String, Value>,
) -> Result<OfficeAccountDraftRequest> {
    let account_obj = obj
        .get("account")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::config("tool_office_config", "missing account"))?;
    let capability_hint = obj
        .get("capability")
        .map(parse_capability_value)
        .transpose()?;
    let account_key = required_string(account_obj, "account_key")?;
    let provider_kind = tool_facing_account_provider_kind(account_obj, obj)
        .ok_or_else(|| Error::config("tool_office_config", "missing provider_kind"))?;
    let external_account_id = preferred_string(
        account_obj,
        &["external_account_id", "email", "account_id", "username"],
    )
    .unwrap_or_default();
    let account_label = preferred_string(account_obj, &["account_label", "display_name", "label"])
        .unwrap_or_default();
    let identity_class = account_obj
        .get("identity_class")
        .map(|value| parse_identity_class_value(value, "identity_class", "tool_office_config"))
        .transpose()?
        .unwrap_or(OfficeAccountIdentityClass::Other);
    let enabled_capabilities = parse_capability_list_with_hint(
        account_obj,
        &["enabled_capabilities", "capabilities"],
        capability_hint,
    )?;
    Ok(OfficeAccountDraftRequest {
        account: OfficeAccount {
            account_key,
            provider_kind,
            external_account_id,
            account_label,
            identity_class,
            enabled_capabilities,
        },
        set_defaults: parse_capability_array(obj, "set_defaults")?,
        clear_defaults: parse_capability_array(obj, "clear_defaults")?,
        policy_patch: obj
            .get("policy_patch")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| Error::config("tool_office_config", error.to_string()))?,
    })
}

fn parse_credential_draft_request(
    obj: &Map<String, Value>,
) -> Result<OfficeCredentialDraftRequest> {
    match serde_json::from_value::<OfficeCredentialDraftRequest>(Value::Object(obj.clone())) {
        Ok(request) => apply_tool_facing_credential_aliases(request, obj),
        Err(_) => normalize_tool_facing_credential_draft_request(obj),
    }
}

fn apply_tool_facing_account_aliases(
    mut request: OfficeAccountDraftRequest,
    obj: &Map<String, Value>,
) -> Result<OfficeAccountDraftRequest> {
    let Some(account_obj) = obj.get("account").and_then(Value::as_object) else {
        return Ok(request);
    };
    if request.account.external_account_id.trim().is_empty() {
        request.account.external_account_id = preferred_string(
            account_obj,
            &["external_account_id", "email", "account_id", "username"],
        )
        .unwrap_or_default();
    }
    if request.account.account_label.trim().is_empty() {
        request.account.account_label =
            preferred_string(account_obj, &["account_label", "display_name", "label"])
                .unwrap_or_default();
    }
    request.account.provider_kind = tool_facing_account_provider_kind(account_obj, obj)
        .unwrap_or_else(|| normalize_office_provider_kind(&request.account.provider_kind));
    if request.account.enabled_capabilities.is_empty() {
        request.account.enabled_capabilities = parse_capability_list_with_hint(
            account_obj,
            &["enabled_capabilities", "capabilities"],
            obj.get("capability")
                .map(parse_capability_value)
                .transpose()?,
        )?;
    }
    Ok(request)
}

fn apply_tool_facing_credential_aliases(
    mut request: OfficeCredentialDraftRequest,
    obj: &Map<String, Value>,
) -> Result<OfficeCredentialDraftRequest> {
    let Some(credential_obj) = obj.get("credential").and_then(Value::as_object) else {
        return Ok(request);
    };
    if request.credential.access_token.trim().is_empty() {
        request.credential.access_token =
            preferred_string(credential_obj, &["access_token", "password"]).unwrap_or_default();
    }
    if request.credential.refresh_token.trim().is_empty() {
        request.credential.refresh_token =
            optional_string(credential_obj, "refresh_token").unwrap_or_default();
    }
    if request.credential.token_endpoint.trim().is_empty() {
        request.credential.token_endpoint =
            optional_string(credential_obj, "token_endpoint").unwrap_or_default();
    }
    if request.credential.expires_at_unix_secs == 0 {
        request.credential.expires_at_unix_secs =
            optional_u64(credential_obj, "expires_at_unix_secs")?.unwrap_or_default();
    }
    if request.credential.updated_at == 0 {
        request.credential.updated_at =
            optional_u64(credential_obj, "updated_at")?.unwrap_or_default();
    }
    insert_metadata_alias(
        &mut request.credential.metadata,
        credential_obj,
        OFFICE_METADATA_MAIL_USERNAME,
        &["mail_username", "email", "username"],
    );
    insert_metadata_alias(
        &mut request.credential.metadata,
        credential_obj,
        OFFICE_METADATA_MAIL_FROM_ADDRESS,
        &["mail_from_address", "email", "from_address"],
    );
    insert_metadata_value(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_IMAP_HOST,
        optional_string(credential_obj, "imap_host"),
    );
    insert_metadata_u64(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_IMAP_PORT,
        optional_u64(credential_obj, "imap_port")?,
    );
    insert_metadata_bool(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_IMAP_TLS,
        optional_bool(credential_obj, "imap_tls")?,
    );
    insert_metadata_value(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_SMTP_HOST,
        optional_string(credential_obj, "smtp_host"),
    );
    insert_metadata_u64(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_SMTP_PORT,
        optional_u64(credential_obj, "smtp_port")?,
    );
    insert_metadata_bool(
        &mut request.credential.metadata,
        OFFICE_METADATA_MAIL_SMTP_TLS,
        optional_bool(credential_obj, "smtp_tls")?,
    );
    Ok(request)
}

fn normalize_tool_facing_credential_draft_request(
    obj: &Map<String, Value>,
) -> Result<OfficeCredentialDraftRequest> {
    let credential_obj = obj
        .get("credential")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::config("tool_office_config", "missing credential"))?;
    let account_key = required_string(credential_obj, "account_key")?;
    let access_token =
        preferred_string(credential_obj, &["access_token", "password"]).unwrap_or_default();
    let refresh_token = optional_string(credential_obj, "refresh_token").unwrap_or_default();
    let token_endpoint = optional_string(credential_obj, "token_endpoint").unwrap_or_default();
    let expires_at_unix_secs =
        optional_u64(credential_obj, "expires_at_unix_secs")?.unwrap_or_default();
    let updated_at = optional_u64(credential_obj, "updated_at")?.unwrap_or_default();
    let mut metadata = credential_obj
        .get("metadata")
        .cloned()
        .map(serde_json::from_value::<BTreeMap<String, String>>)
        .transpose()
        .map_err(|error| Error::config("tool_office_config", error.to_string()))?
        .unwrap_or_default();
    insert_metadata_alias(
        &mut metadata,
        credential_obj,
        OFFICE_METADATA_MAIL_USERNAME,
        &["mail_username", "email", "username"],
    );
    insert_metadata_alias(
        &mut metadata,
        credential_obj,
        OFFICE_METADATA_MAIL_FROM_ADDRESS,
        &["mail_from_address", "email", "from_address"],
    );
    insert_metadata_value(
        &mut metadata,
        OFFICE_METADATA_MAIL_IMAP_HOST,
        optional_string(credential_obj, "imap_host"),
    );
    insert_metadata_u64(
        &mut metadata,
        OFFICE_METADATA_MAIL_IMAP_PORT,
        optional_u64(credential_obj, "imap_port")?,
    );
    insert_metadata_bool(
        &mut metadata,
        OFFICE_METADATA_MAIL_IMAP_TLS,
        optional_bool(credential_obj, "imap_tls")?,
    );
    insert_metadata_value(
        &mut metadata,
        OFFICE_METADATA_MAIL_SMTP_HOST,
        optional_string(credential_obj, "smtp_host"),
    );
    insert_metadata_u64(
        &mut metadata,
        OFFICE_METADATA_MAIL_SMTP_PORT,
        optional_u64(credential_obj, "smtp_port")?,
    );
    insert_metadata_bool(
        &mut metadata,
        OFFICE_METADATA_MAIL_SMTP_TLS,
        optional_bool(credential_obj, "smtp_tls")?,
    );
    Ok(OfficeCredentialDraftRequest {
        credential: OfficeCredential {
            account_key,
            access_token,
            refresh_token,
            token_endpoint,
            expires_at_unix_secs,
            updated_at,
            metadata,
        },
    })
}

fn parse_capability_array(obj: &Map<String, Value>, field: &str) -> Result<Vec<OfficeCapability>> {
    obj.get(field)
        .map(|value| parse_capability_values(value, field))
        .transpose()
        .map(|value| value.unwrap_or_default())
}

fn parse_capability_list_with_hint(
    obj: &Map<String, Value>,
    fields: &[&str],
    hint: Option<OfficeCapability>,
) -> Result<Vec<OfficeCapability>> {
    for field in fields {
        if let Some(value) = obj.get(*field) {
            return parse_capability_values(value, field);
        }
    }
    Ok(hint.map(|value| vec![value]).unwrap_or_default())
}

fn parse_capability_values(value: &Value, field: &str) -> Result<Vec<OfficeCapability>> {
    let items = value
        .as_array()
        .ok_or_else(|| Error::config("tool_office_config", format!("{field} must be an array")))?;
    items
        .iter()
        .map(parse_capability_value)
        .collect::<Result<Vec<_>>>()
}

fn required_string(obj: &Map<String, Value>, field: &str) -> Result<String> {
    optional_string(obj, field)
        .ok_or_else(|| Error::config("tool_office_config", format!("missing {field}")))
}

fn preferred_string(obj: &Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| optional_string(obj, field))
}

fn optional_string(obj: &Map<String, Value>, field: &str) -> Option<String> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_u64(obj: &Map<String, Value>, field: &str) -> Result<Option<u64>> {
    match obj.get(field) {
        None => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(|| {
            Error::config(
                "tool_office_config",
                format!("{field} must be a non-negative integer"),
            )
        }),
        Some(Value::String(raw)) => raw.trim().parse::<u64>().map(Some).map_err(|_| {
            Error::config(
                "tool_office_config",
                format!("{field} must be a non-negative integer"),
            )
        }),
        Some(_) => Err(Error::config(
            "tool_office_config",
            format!("{field} must be an integer"),
        )),
    }
}

fn optional_bool(obj: &Map<String, Value>, field: &str) -> Result<Option<bool>> {
    match obj.get(field) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(Value::String(raw)) => match raw.trim() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(Error::config(
                "tool_office_config",
                format!("{field} must be true or false"),
            )),
        },
        Some(_) => Err(Error::config(
            "tool_office_config",
            format!("{field} must be a boolean"),
        )),
    }
}

fn insert_metadata_alias(
    metadata: &mut BTreeMap<String, String>,
    obj: &Map<String, Value>,
    metadata_key: &str,
    aliases: &[&str],
) {
    if metadata.contains_key(metadata_key) {
        return;
    }
    if let Some(value) = preferred_string(obj, aliases) {
        metadata.insert(metadata_key.to_string(), value);
    }
}

fn insert_metadata_value(
    metadata: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<String>,
) {
    if metadata.contains_key(key) {
        return;
    }
    if let Some(value) = value {
        metadata.insert(key.to_string(), value);
    }
}

fn insert_metadata_u64(metadata: &mut BTreeMap<String, String>, key: &str, value: Option<u64>) {
    if metadata.contains_key(key) {
        return;
    }
    if let Some(value) = value {
        metadata.insert(key.to_string(), value.to_string());
    }
}

fn insert_metadata_bool(metadata: &mut BTreeMap<String, String>, key: &str, value: Option<bool>) {
    if metadata.contains_key(key) {
        return;
    }
    if let Some(value) = value {
        metadata.insert(key.to_string(), value.to_string());
    }
}

fn tool_facing_provider_kind(obj: &Map<String, Value>) -> Option<String> {
    preferred_string(obj, &["provider_kind", "provider"])
        .map(|raw| normalize_office_provider_kind(raw.as_str()))
}

fn tool_facing_account_provider_kind(
    account_obj: &Map<String, Value>,
    obj: &Map<String, Value>,
) -> Option<String> {
    preferred_string(account_obj, &["provider_kind", "provider"])
        .or_else(|| preferred_string(obj, &["provider_kind", "provider"]))
        .map(|raw| normalize_office_provider_kind(raw.as_str()))
}

fn normalize_office_provider_kind(raw: &str) -> String {
    match raw.trim() {
        "qq" | "qqmail" | "qq_mail" => "imap_smtp".to_string(),
        other => other.to_string(),
    }
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
        config_file_store: Arc<MemoryConfigFileStore>,
        credential_store: Arc<MemoryCredentialStore>,
        runtime_status_store: Arc<MemoryRuntimeStatusStore>,
    }

    fn build_fixture() -> ToolFixture {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        let credential_store = Arc::new(MemoryCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let tool = OfficeConfigTool::new(OfficeConfigManagementService::new(
            config_file_store.clone(),
            credential_store.clone(),
            runtime_status_store.clone(),
        ));
        ToolFixture {
            tool,
            config_file_store,
            credential_store,
            runtime_status_store,
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
    fn draft_accounts_returns_preview_segment() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_accounts",
                "account":{
                    "account_key":"mail-work",
                    "provider_kind":"imap_smtp",
                    "external_account_id":"work@example.com",
                    "account_label":"Work",
                    "identity_class":"work",
                    "enabled_capabilities":["mail"]
                },
                "set_defaults":["mail"]
            }"#,
                &mut ctx,
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            payload["payload"]["binding"]["capability_defaults"]["mail"],
            "mail-work"
        );
    }

    #[test]
    fn draft_accounts_accepts_tool_facing_mail_account_shape() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_accounts",
                "capability":"mail",
                "account":{
                    "account_key":"qq_675778650",
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "provider_kind":"qq",
                    "capabilities":["mail"]
                },
                "set_defaults":["mail"]
            }"#,
                &mut ctx,
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let account = &payload["payload"]["registry"]["accounts"]["qq_675778650"];
        assert_eq!(account["provider_kind"], "imap_smtp");
        assert_eq!(account["account_label"], "QQ邮箱");
        assert_eq!(account["external_account_id"], "675778650@qq.com");
        assert_eq!(account["identity_class"], "other");
        assert_eq!(account["enabled_capabilities"][0], "mail");
    }

    #[test]
    fn draft_accounts_accepts_nested_provider_alias_when_provider_kind_missing() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_accounts",
                "capability":"mail",
                "account":{
                    "account_key":"qq_675778650",
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "provider":"qqmail",
                    "capabilities":["mail"]
                },
                "set_defaults":["mail"]
            }"#,
                &mut ctx,
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let account = &payload["payload"]["registry"]["accounts"]["qq_675778650"];
        assert_eq!(account["provider_kind"], "imap_smtp");
        assert_eq!(account["account_label"], "QQ邮箱");
        assert_eq!(account["external_account_id"], "675778650@qq.com");
    }

    #[test]
    fn draft_accounts_accepts_top_level_provider_kind_when_nested_field_is_missing() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_accounts",
                "capability":"mail",
                "provider_kind":"qq",
                "account":{
                    "account_key":"qq_675778650",
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "capabilities":["mail"]
                },
                "set_defaults":["mail"]
            }"#,
                &mut ctx,
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let account = &payload["payload"]["registry"]["accounts"]["qq_675778650"];
        assert_eq!(account["provider_kind"], "imap_smtp");
        assert_eq!(account["account_label"], "QQ邮箱");
        assert_eq!(account["external_account_id"], "675778650@qq.com");
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
    fn draft_credentials_accepts_tool_facing_imap_smtp_shape_after_account_commit() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let account_payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_accounts",
                "capability":"mail",
                "account":{
                    "account_key":"qq_675778650",
                    "display_name":"QQ邮箱",
                    "email":"675778650@qq.com",
                    "provider_kind":"imap_smtp",
                    "capabilities":["mail"]
                }
            }"#,
                &mut ctx,
            )
            .unwrap();
        let account_payload: Value = serde_json::from_str(&account_payload).unwrap();
        fixture
            .tool
            .execute(
                &json!({
                    "op":"commit_accounts",
                    "segment": account_payload["payload"].clone(),
                    "confirm": true
                })
                .to_string(),
                &mut ctx,
            )
            .unwrap();
        let payload = fixture
            .tool
            .execute(
                r#"{
                "op":"draft_credentials",
                "credential":{
                    "account_key":"qq_675778650",
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
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        let credential = &payload["payload"]["items"][0];
        assert_eq!(credential["account_key"], "qq_675778650");
        assert_eq!(credential["access_token"], "hqvqcibpdvqgbdba");
        assert_eq!(credential["metadata"]["mail_username"], "675778650@qq.com");
        assert_eq!(
            credential["metadata"]["mail_from_address"],
            "675778650@qq.com"
        );
        assert_eq!(credential["metadata"]["mail_imap_host"], "imap.qq.com");
        assert_eq!(credential["metadata"]["mail_imap_port"], "993");
        assert_eq!(credential["metadata"]["mail_smtp_host"], "smtp.qq.com");
        assert_eq!(credential["metadata"]["mail_smtp_port"], "465");
        assert_eq!(credential["metadata"]["mail_imap_tls"], "true");
        assert_eq!(credential["metadata"]["mail_smtp_tls"], "true");
    }

    #[test]
    fn commit_accounts_requires_confirm_and_explicit_intent() {
        let fixture = build_fixture();
        let shape = fixture
            .tool
            .execution_shape(r#"{"op":"commit_accounts"}"#)
            .expect("shape");
        assert_eq!(shape.operation, "commit_accounts");
        assert_eq!(shape.effect_class, ToolEffectClass::ConfigWrite);
        assert_eq!(shape.risk_level, ToolRiskLevel::High);
        assert_eq!(shape.approval_mode, ToolApprovalMode::ExplicitIntent);
        assert!(!shape.approval_granted);
        assert_eq!(shape.rollback_kind, ToolRollbackKind::ConfigRestore);

        let mut ctx = DummyCtx;
        let error = fixture
            .tool
            .execute(r#"{"op":"commit_accounts","segment":{}}"#, &mut ctx)
            .expect_err("commit without confirm must fail");
        assert!(error
            .to_string()
            .contains("commit_accounts requires confirm=true"));
    }

    #[test]
    fn resolve_account_returns_selected_binding() {
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
                "binding":{"capability_defaults":{"mail":"mail-work"}},
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
        assert_eq!(payload["payload"]["selection_reason"], "capability_default");
    }

    #[test]
    fn commit_accounts_persists_and_probe_reports_missing_credential() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        fixture
            .tool
            .execute(
                &json!({
                    "op": "commit_accounts",
                    "segment": {
                        "registry": {
                            "accounts": {
                                "mail-work": {
                                    "account_key": "mail-work",
                                    "provider_kind": "imap_smtp",
                                    "external_account_id": "work@example.com",
                                    "account_label": "Work",
                                    "identity_class": "work",
                                    "enabled_capabilities": ["mail"]
                                }
                            }
                        },
                        "binding": {
                            "capability_defaults": {
                                "mail": "mail-work"
                            }
                        },
                        "policy": {}
                    },
                    "confirm": true
                })
                .to_string(),
                &mut ctx,
            )
            .expect("commit accounts");

        let stored = crate::config::get_office_accounts_segment(fixture.config_file_store.as_ref())
            .expect("stored accounts");
        let stored: Value = serde_json::from_str(&stored).expect("stored json");
        assert_eq!(
            stored["binding"]["capability_defaults"]["mail"],
            "mail-work"
        );

        let payload = fixture
            .tool
            .execute(r#"{"op":"probe","account_key":"mail-work"}"#, &mut ctx)
            .expect("probe");
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(
            payload["payload"]["disposition"],
            json!(OfficeProbeDisposition::MissingCredential)
        );
        assert_eq!(payload["payload"]["reason"], "credential_missing");
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
                "binding": {},
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
            "needs_credential_input"
        );
        assert_eq!(
            payload["payload"]["accounts"][0]["next_action"],
            "draft_credentials"
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
    fn draft_credentials_normalizes_wecom_documents_defaults_and_trims_values() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        crate::config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry":{"accounts":{
                    "docs-wecom":{
                        "account_key":"docs-wecom",
                        "provider_kind":"wecom_documents",
                        "external_account_id":"",
                        "account_label":"WeCom Docs",
                        "identity_class":"work",
                        "enabled_capabilities":["documents"]
                    }
                }},
                "binding":{},
                "policy":{}
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
            .execute(
                r#"{
                    "op":"draft_credentials",
                    "credential":{
                        "account_key":"docs-wecom",
                        "access_token":"  corp-secret  ",
                        "metadata":{
                            "documents_corp_id":"  wwcorp  ",
                            "documents_space_id":"  space-1  ",
                            "documents_root_path":"  /shared/docs  ",
                            "documents_base_url":"   "
                        }
                    }
                }"#,
                &mut ctx,
            )
            .expect("draft credentials");
        let payload: Value = serde_json::from_str(&payload).expect("valid json");
        let credential = &payload["payload"]["items"][0];
        assert_eq!(credential["access_token"], "corp-secret");
        assert_eq!(credential["metadata"]["documents_corp_id"], "wwcorp");
        assert_eq!(credential["metadata"]["documents_space_id"], "space-1");
        assert_eq!(
            credential["metadata"]["documents_root_path"],
            "/shared/docs"
        );
        assert_eq!(
            credential["metadata"]["documents_base_url"],
            crate::office::WECOM_DEFAULT_BASE_URL
        );
    }

    #[test]
    fn validate_credentials_rejects_unknown_provider_metadata_key() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        crate::config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry":{"accounts":{
                    "docs-wecom":{
                        "account_key":"docs-wecom",
                        "provider_kind":"wecom_documents",
                        "external_account_id":"",
                        "account_label":"WeCom Docs",
                        "identity_class":"work",
                        "enabled_capabilities":["documents"]
                    }
                }},
                "binding":{},
                "policy":{}
            }"#,
        )
        .expect("seed accounts");
        let tool = OfficeConfigTool::new(OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        ));
        let mut ctx = DummyCtx;
        let error = tool
            .execute(
                r#"{
                    "op":"validate_credentials",
                    "segment":{
                        "items":[
                            {
                                "account_key":"docs-wecom",
                                "access_token":"corp-secret",
                                "metadata":{
                                    "documents_corp_id":"wwcorp",
                                    "documents_space_id":"space-1",
                                    "documents_root_path":"/shared/docs",
                                    "documents_extra":"oops"
                                }
                            }
                        ]
                    }
                }"#,
                &mut ctx,
            )
            .expect_err("unexpected metadata key must fail");
        assert!(error.to_string().contains("documents_extra"));
    }

    #[test]
    fn validate_credentials_rejects_unknown_account_key() {
        let fixture = build_fixture();
        let mut ctx = DummyCtx;
        let error = fixture
            .tool
            .execute(
                r#"{
                    "op":"validate_credentials",
                    "segment":{
                        "items":[
                            {
                                "account_key":"missing-account",
                                "access_token":"secret"
                            }
                        ]
                    }
                }"#,
                &mut ctx,
            )
            .expect_err("unknown account must fail");
        assert!(error.to_string().contains("missing-account"));
    }

    #[test]
    fn validate_credentials_rejects_missing_conditional_transport_fields() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        crate::config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry":{"accounts":{
                    "mail-work":{
                        "account_key":"mail-work",
                        "provider_kind":"imap_smtp",
                        "external_account_id":"",
                        "account_label":"Work",
                        "identity_class":"work",
                        "enabled_capabilities":["mail"]
                    }
                }},
                "binding":{},
                "policy":{}
            }"#,
        )
        .expect("seed accounts");
        let tool = OfficeConfigTool::new(OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        ));
        let mut ctx = DummyCtx;
        let error = tool
            .execute(
                r#"{
                    "op":"validate_credentials",
                    "segment":{
                        "items":[
                            {
                                "account_key":"mail-work",
                                "access_token":"secret",
                                "metadata":{
                                    "mail_imap_host":"imap.example.com",
                                    "mail_smtp_host":"smtp.example.com"
                                }
                            }
                        ]
                    }
                }"#,
                &mut ctx,
            )
            .expect_err("conditional missing fields must fail");
        assert!(error.to_string().contains("mail_username"));
    }

    #[test]
    fn commit_credentials_persists_normalized_schema_defaults() {
        let fixture = build_fixture();
        crate::config::save_office_accounts_segment(
            fixture.config_file_store.as_ref(),
            r#"{
                "registry":{"accounts":{
                    "docs-wecom":{
                        "account_key":"docs-wecom",
                        "provider_kind":"wecom_documents",
                        "external_account_id":"",
                        "account_label":"WeCom Docs",
                        "identity_class":"work",
                        "enabled_capabilities":["documents"]
                    }
                }},
                "binding":{},
                "policy":{}
            }"#,
        )
        .expect("seed accounts");
        let mut ctx = DummyCtx;
        fixture
            .tool
            .execute(
                r#"{
                    "op":"commit_credentials",
                    "segment":{
                        "items":[
                            {
                                "account_key":"docs-wecom",
                                "access_token":"  corp-secret  ",
                                "metadata":{
                                    "documents_corp_id":"  wwcorp  ",
                                    "documents_space_id":"  space-1  ",
                                    "documents_root_path":"  /shared/docs  ",
                                    "documents_base_url":""
                                }
                            }
                        ]
                    },
                    "confirm":true
                }"#,
                &mut ctx,
            )
            .expect("commit credentials");

        let credential = fixture
            .credential_store
            .get("docs-wecom")
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
}
