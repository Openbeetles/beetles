use crate::error::{Error, Result};
use crate::mail::{
    OFFICE_METADATA_MAIL_FROM_ADDRESS, OFFICE_METADATA_MAIL_IMAP_HOST,
    OFFICE_METADATA_MAIL_IMAP_PORT, OFFICE_METADATA_MAIL_IMAP_TLS, OFFICE_METADATA_MAIL_SMTP_HOST,
    OFFICE_METADATA_MAIL_SMTP_PORT, OFFICE_METADATA_MAIL_SMTP_TLS, OFFICE_METADATA_MAIL_USERNAME,
};
use crate::office::{
    office_provider_schema, OfficeAccountConfigSaveRequest, OfficeAccountIdentityClass,
    OfficeAccountRecordInput, OfficeAccountUpsertRequest, OfficeCapability,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(crate) fn parse_public_account_upsert_request_value(
    obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<OfficeAccountUpsertRequest> {
    reject_legacy_public_account_keys(obj, stage)?;
    normalize_public_account_upsert_request(obj, stage)
}

fn normalize_public_account_upsert_request(
    obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<OfficeAccountUpsertRequest> {
    let capability_hint = obj
        .get("capability")
        .map(|value| parse_capability_value(value, stage))
        .transpose()?;
    let provider_kind = public_provider_kind(obj)
        .or_else(|| infer_provider_kind_from_sources(&[Some(obj)]))
        .ok_or_else(|| Error::config(stage, "missing provider_kind"))?;
    let enabled_capabilities = capability_hint
        .or_else(|| infer_single_capability_for_provider(&provider_kind))
        .map(|value| vec![value])
        .unwrap_or_default();
    if enabled_capabilities.is_empty() {
        return Err(Error::config(stage, "missing capability"));
    }

    let external_account_id = preferred_string_from_sources(
        &[Some(obj)],
        &[
            "external_account_id",
            "email",
            "account_id",
            "username",
            "mail_username",
            "from_address",
            "mail_from_address",
        ],
    )
    .ok_or_else(|| Error::config(stage, "missing external account identity"))?;
    let account_label =
        preferred_string_from_sources(&[Some(obj)], &["account_label", "display_name", "label"])
            .unwrap_or_else(|| external_account_id.clone());
    let identity_class = identity_class_from_sources(&[Some(obj)], stage)?
        .ok_or_else(|| Error::config(stage, "missing identity_class"))?;

    let mut request = OfficeAccountUpsertRequest {
        account: OfficeAccountRecordInput {
            account_key: String::new(),
            provider_kind,
            external_account_id,
            account_label,
            identity_class,
            enabled_capabilities,
        },
        policy_patch: None,
        config: None,
    };
    validate_public_account_request(&request, true, stage)?;
    merge_public_config_object_aliases(&mut request, obj, stage)?;
    if request
        .config
        .as_ref()
        .is_some_and(|config| config.fields.is_empty() && config.clear_fields.is_empty())
    {
        request.config = None;
    }
    Ok(request)
}

fn merge_public_config_object_aliases(
    request: &mut OfficeAccountUpsertRequest,
    source_obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<()> {
    let mut fields = BTreeMap::new();
    merge_config_field_alias(
        &mut fields,
        "access_token",
        preferred_string(source_obj, &["access_token", "password"]),
    );
    merge_config_field_alias(
        &mut fields,
        "refresh_token",
        optional_string(source_obj, "refresh_token"),
    );
    merge_config_field_alias(
        &mut fields,
        "token_endpoint",
        optional_string(source_obj, "token_endpoint"),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_USERNAME,
        optional_string(source_obj, "mail_username")
            .or_else(|| optional_string(source_obj, "username")),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_FROM_ADDRESS,
        optional_string(source_obj, "mail_from_address")
            .or_else(|| optional_string(source_obj, "from_address")),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_IMAP_HOST,
        optional_string(source_obj, "imap_host"),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_IMAP_PORT,
        optional_u64(source_obj, "imap_port", stage)?.map(|value| value.to_string()),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_IMAP_TLS,
        optional_bool(source_obj, "imap_tls", stage)?.map(|value| value.to_string()),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_SMTP_HOST,
        optional_string(source_obj, "smtp_host"),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_SMTP_PORT,
        optional_u64(source_obj, "smtp_port", stage)?.map(|value| value.to_string()),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_SMTP_TLS,
        optional_bool(source_obj, "smtp_tls", stage)?.map(|value| value.to_string()),
    );
    merge_metadata_object_aliases(&mut fields, source_obj, stage)?;
    if fields.is_empty() {
        return Ok(());
    }

    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_USERNAME,
        preferred_string(source_obj, &["mail_username", "email", "username"]),
    );
    merge_config_field_alias(
        &mut fields,
        OFFICE_METADATA_MAIL_FROM_ADDRESS,
        preferred_string(source_obj, &["mail_from_address", "email", "from_address"]),
    );

    let config = request
        .config
        .get_or_insert_with(OfficeAccountConfigSaveRequest::default);
    for (key, value) in fields {
        config.fields.entry(key).or_insert(value);
    }
    Ok(())
}

fn merge_config_field_alias(
    fields: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<String>,
) {
    if fields.contains_key(key) {
        return;
    }
    if let Some(value) = value {
        fields.insert(key.to_string(), value);
    }
}

fn merge_metadata_object_aliases(
    fields: &mut BTreeMap<String, String>,
    source_obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<()> {
    let Some(metadata_obj) = source_obj.get("metadata").and_then(Value::as_object) else {
        return Ok(());
    };
    for (key, value) in metadata_obj {
        if fields.contains_key(key) {
            continue;
        }
        let value = json_scalar_to_string(value, key, stage)?;
        if !value.trim().is_empty() {
            fields.insert(key.clone(), value);
        }
    }
    Ok(())
}

fn json_scalar_to_string(value: &Value, field: &str, stage: &'static str) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.trim().to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(Error::config(
            stage,
            format!("{field} metadata value must be string/number/boolean"),
        )),
    }
}

fn parse_capability_value(value: &Value, stage: &'static str) -> Result<OfficeCapability> {
    let raw = value
        .as_str()
        .ok_or_else(|| Error::config(stage, "capability must be a string"))?;
    match raw {
        "mail" => Ok(OfficeCapability::Mail),
        "calendar" => Ok(OfficeCapability::Calendar),
        "documents" => Ok(OfficeCapability::Documents),
        "contacts_directory" => Ok(OfficeCapability::ContactsDirectory),
        _ => Err(Error::config(
            stage,
            format!("unsupported capability '{raw}'"),
        )),
    }
}

fn preferred_string(obj: &Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| optional_string(obj, field))
}

fn preferred_string_from_sources(
    sources: &[Option<&Map<String, Value>>],
    fields: &[&str],
) -> Option<String> {
    sources
        .iter()
        .flatten()
        .find_map(|source| preferred_string(source, fields))
}

fn optional_string(obj: &Map<String, Value>, field: &str) -> Option<String> {
    obj.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn optional_u64(obj: &Map<String, Value>, field: &str, stage: &'static str) -> Result<Option<u64>> {
    match obj.get(field) {
        None => Ok(None),
        Some(Value::Number(number)) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| Error::config(stage, format!("{field} must be a non-negative integer"))),
        Some(Value::String(raw)) => {
            raw.trim().parse::<u64>().map(Some).map_err(|_| {
                Error::config(stage, format!("{field} must be a non-negative integer"))
            })
        }
        Some(_) => Err(Error::config(stage, format!("{field} must be an integer"))),
    }
}

fn optional_bool(
    obj: &Map<String, Value>,
    field: &str,
    stage: &'static str,
) -> Result<Option<bool>> {
    match obj.get(field) {
        None => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(Value::String(raw)) => match raw.trim() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            _ => Err(Error::config(
                stage,
                format!("{field} must be true or false"),
            )),
        },
        Some(_) => Err(Error::config(stage, format!("{field} must be a boolean"))),
    }
}

fn public_provider_kind(obj: &Map<String, Value>) -> Option<String> {
    preferred_string(obj, &["provider_kind", "provider"])
        .map(|raw| normalize_office_provider_kind(raw.as_str()))
}

fn normalize_office_provider_kind(raw: &str) -> String {
    match raw.trim() {
        "qq" | "qqmail" | "qq_mail" => "imap_smtp".to_string(),
        other => other.to_string(),
    }
}

fn reject_legacy_public_account_keys(obj: &Map<String, Value>, stage: &'static str) -> Result<()> {
    for key in [
        "account",
        "config",
        "credential",
        "account_key",
        "enabled_capabilities",
        "capabilities",
        "set_defaults",
        "clear_defaults",
        "policy_patch",
    ] {
        if obj.contains_key(key) {
            return Err(Error::config(
                stage,
                format!(
                    "legacy public account wrappers are not supported: remove '{}'",
                    key
                ),
            ));
        }
    }
    Ok(())
}

fn infer_provider_kind_from_sources(sources: &[Option<&Map<String, Value>>]) -> Option<String> {
    if source_has_any_key(
        sources,
        &[
            "documents_space_id",
            "documents_app_id",
            "documents_corp_id",
            "contacts_app_id",
            "contacts_corp_id",
            "calendar_app_id",
            "calendar_corp_id",
            "mail_corp_id",
        ],
    ) {
        if source_has_any_key(sources, &["documents_space_id", "documents_corp_id"]) {
            return Some("wecom_documents".to_string());
        }
        if source_has_any_key(sources, &["documents_app_id"]) {
            return Some("feishu_documents".to_string());
        }
        if source_has_any_key(sources, &["contacts_app_id"]) {
            return Some("feishu_contacts_directory".to_string());
        }
        if source_has_any_key(sources, &["contacts_corp_id"]) {
            return Some("wecom_contacts_directory".to_string());
        }
        if source_has_any_key(sources, &["calendar_app_id"]) {
            return Some("feishu_calendar".to_string());
        }
        if source_has_any_key(sources, &["calendar_corp_id"]) {
            return Some("wecom_calendar".to_string());
        }
        if source_has_any_key(sources, &["mail_corp_id"]) {
            return Some("wecom_mail".to_string());
        }
    }

    if source_has_any_key(
        sources,
        &[
            "imap_host",
            "smtp_host",
            OFFICE_METADATA_MAIL_IMAP_HOST,
            OFFICE_METADATA_MAIL_SMTP_HOST,
            "imap_port",
            "smtp_port",
            "imap_tls",
            "smtp_tls",
            "mail_imap_mailbox",
            "mail_draft_mailbox",
        ],
    ) {
        return Some("imap_smtp".to_string());
    }

    None
}

fn source_has_any_key(sources: &[Option<&Map<String, Value>>], keys: &[&str]) -> bool {
    sources.iter().flatten().any(|source| {
        let metadata_obj = source.get("metadata").and_then(Value::as_object);
        keys.iter().any(|key| {
            has_non_empty_scalar(source.get(*key))
                || metadata_obj.is_some_and(|metadata| has_non_empty_scalar(metadata.get(*key)))
        })
    })
}

fn infer_single_capability_for_provider(provider_kind: &str) -> Option<OfficeCapability> {
    let schema = office_provider_schema(provider_kind)?;
    match schema.capabilities.as_slice() {
        [capability] => Some(*capability),
        _ => None,
    }
}

fn has_non_empty_scalar(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => !value.trim().is_empty(),
        Some(Value::Number(_)) | Some(Value::Bool(_)) => true,
        _ => false,
    }
}

fn identity_class_from_sources(
    sources: &[Option<&Map<String, Value>>],
    stage: &'static str,
) -> Result<Option<OfficeAccountIdentityClass>> {
    for source in sources.iter().flatten() {
        if let Some(value) = source.get("identity_class") {
            return parse_identity_class_value(value, "identity_class", stage).map(Some);
        }
    }
    Ok(None)
}

fn parse_identity_class_value(
    value: &Value,
    field: &str,
    stage: &'static str,
) -> Result<OfficeAccountIdentityClass> {
    let raw = value
        .as_str()
        .ok_or_else(|| Error::config(stage, format!("{field} must be a string")))?;
    match raw {
        "work" => Ok(OfficeAccountIdentityClass::Work),
        "personal" => Ok(OfficeAccountIdentityClass::Personal),
        "family" => Ok(OfficeAccountIdentityClass::Family),
        "shared" => Ok(OfficeAccountIdentityClass::Shared),
        "other" => Ok(OfficeAccountIdentityClass::Other),
        _ => Err(Error::config(stage, format!("unsupported {field} '{raw}'"))),
    }
}

fn validate_public_account_request(
    request: &OfficeAccountUpsertRequest,
    require_external_account_identity: bool,
    stage: &'static str,
) -> Result<()> {
    if request.account.provider_kind.trim().is_empty() {
        return Err(Error::config(stage, "missing provider_kind"));
    }
    if request.account.enabled_capabilities.is_empty() {
        return Err(Error::config(stage, "missing capability"));
    }
    if require_external_account_identity && request.account.external_account_id.trim().is_empty() {
        return Err(Error::config(stage, "missing external account identity"));
    }
    Ok(())
}
