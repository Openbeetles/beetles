use crate::error::{Error, Result};
use crate::mail::{
    OFFICE_METADATA_MAIL_FROM_ADDRESS, OFFICE_METADATA_MAIL_IMAP_HOST,
    OFFICE_METADATA_MAIL_IMAP_PORT, OFFICE_METADATA_MAIL_IMAP_TLS, OFFICE_METADATA_MAIL_SMTP_HOST,
    OFFICE_METADATA_MAIL_SMTP_PORT, OFFICE_METADATA_MAIL_SMTP_TLS, OFFICE_METADATA_MAIL_USERNAME,
};
use crate::office::{
    infer_single_capability_for_provider, office_provider_schema, OfficeAccountConfigSaveRequest,
    OfficeAccountIdentityClass, OfficeAccountOnboardingRequest, OfficeCapability,
    OfficeProviderFieldLocation,
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub(crate) fn parse_public_account_upsert_request_value(
    obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<OfficeAccountOnboardingRequest> {
    reject_legacy_public_account_keys(obj, stage)?;
    normalize_public_account_upsert_request(obj, stage)
}

fn normalize_public_account_upsert_request(
    obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<OfficeAccountOnboardingRequest> {
    let capability_hint = obj
        .get("capability")
        .map(|value| parse_capability_value(value, stage))
        .transpose()?;
    let provider_kind =
        public_provider_kind(obj).or_else(|| infer_provider_kind_from_sources(&[Some(obj)]));
    let capability_hint = capability_hint.or_else(|| {
        provider_kind
            .as_deref()
            .and_then(infer_single_capability_for_provider)
    });
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
    );
    let account_label =
        preferred_string_from_sources(&[Some(obj)], &["account_label", "display_name", "label"])
            .or_else(|| external_account_id.clone());
    let identity_class = identity_class_from_sources(&[Some(obj)], stage)?;

    let mut request = OfficeAccountOnboardingRequest {
        provider_kind,
        capability: capability_hint,
        external_account_id,
        account_label,
        identity_class,
        config: None,
    };
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
    request: &mut OfficeAccountOnboardingRequest,
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
    merge_top_level_provider_field_aliases(
        &mut fields,
        request.provider_kind.as_deref().unwrap_or_default(),
        source_obj,
        stage,
    )?;
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

fn merge_top_level_provider_field_aliases(
    fields: &mut BTreeMap<String, String>,
    provider_kind: &str,
    source_obj: &Map<String, Value>,
    stage: &'static str,
) -> Result<()> {
    let Some(schema) = office_provider_schema(provider_kind) else {
        return Ok(());
    };
    for field in &schema.fields {
        if field.location == OfficeProviderFieldLocation::ExternalAccountId
            || fields.contains_key(&field.key)
        {
            continue;
        }
        let Some(value) = source_obj.get(&field.key) else {
            continue;
        };
        if !has_non_empty_scalar(Some(value)) {
            continue;
        }
        let value = json_scalar_to_string(value, &field.key, stage)?;
        if !value.trim().is_empty() {
            fields.insert(field.key.clone(), value);
        }
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

#[cfg(test)]
mod tests {
    use super::parse_public_account_upsert_request_value;
    use crate::office::WECOM_DEFAULT_BASE_URL;
    use serde_json::json;

    #[test]
    fn public_contract_normalizes_top_level_provider_metadata_fields() {
        let body = json!({
            "account_id": "wecom-docs",
            "identity_class": "work",
            "access_token": "corp-secret",
            "documents_corp_id": "wwcorp",
            "documents_space_id": "space-1",
            "documents_root_path": "/shared/docs",
            "documents_base_url": WECOM_DEFAULT_BASE_URL
        });
        let obj = body.as_object().expect("object");

        let request = parse_public_account_upsert_request_value(obj, "public_contract_test")
            .expect("normalize onboarding request");

        assert_eq!(request.provider_kind.as_deref(), Some("wecom_documents"));
        assert_eq!(
            request.capability,
            Some(crate::office::OfficeCapability::Documents)
        );
        assert_eq!(request.external_account_id.as_deref(), Some("wecom-docs"));
        assert_eq!(request.account_label.as_deref(), Some("wecom-docs"));
        let config = request.config.expect("config");
        assert_eq!(
            config.fields.get("documents_corp_id").map(String::as_str),
            Some("wwcorp")
        );
        assert_eq!(
            config.fields.get("documents_space_id").map(String::as_str),
            Some("space-1")
        );
        assert_eq!(
            config.fields.get("documents_root_path").map(String::as_str),
            Some("/shared/docs")
        );
        assert_eq!(
            config.fields.get("documents_base_url").map(String::as_str),
            Some(WECOM_DEFAULT_BASE_URL)
        );
    }
}
