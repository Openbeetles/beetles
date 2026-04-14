use crate::documents::{OFFICE_METADATA_DOCUMENTS_BASE_URL, OFFICE_METADATA_DOCUMENTS_USERNAME};
use crate::mail::{
    OFFICE_METADATA_MAIL_IMAP_HOST, OFFICE_METADATA_MAIL_SMTP_HOST, OFFICE_METADATA_MAIL_USERNAME,
};
use serde::{Deserialize, Serialize};

use super::{
    OfficeAccount, OfficeAccountRuntimeStatus, OfficeCapability, OfficeCredential,
    OFFICE_METADATA_CALENDAR_ID,
};

const OFFICE_METADATA_CALENDAR_USERNAME_FIELD: &str = "calendar_username";
const OFFICE_METADATA_CALENDAR_BASE_URL_FIELD: &str = "calendar_base_url";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigReadiness {
    NeedsCredentialInput,
    ReadyForProbe,
    ProbeUnavailable,
    Ready,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigNextAction {
    DraftCredentials,
    Probe,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountAssessment {
    pub account_key: String,
    pub provider_kind: String,
    pub enabled_capabilities: Vec<OfficeCapability>,
    pub credential_present: bool,
    pub credential_configured: bool,
    pub probe_supported: bool,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    pub readiness: OfficeConfigReadiness,
    pub next_action: OfficeConfigNextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<OfficeAccountRuntimeStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigAssessment {
    #[serde(default)]
    pub accounts: Vec<OfficeAccountAssessment>,
}

pub fn assess_office_account(
    account: &OfficeAccount,
    credential: Option<&OfficeCredential>,
    runtime_status: Option<&OfficeAccountRuntimeStatus>,
    probe_supported: bool,
) -> OfficeAccountAssessment {
    let missing_fields = collect_missing_fields(account, credential);
    let readiness = if missing_fields.is_empty() {
        if runtime_status
            .as_ref()
            .is_some_and(|status| status.probe_ok)
        {
            OfficeConfigReadiness::Ready
        } else if probe_supported {
            OfficeConfigReadiness::ReadyForProbe
        } else {
            OfficeConfigReadiness::ProbeUnavailable
        }
    } else {
        OfficeConfigReadiness::NeedsCredentialInput
    };
    let next_action = match readiness {
        OfficeConfigReadiness::NeedsCredentialInput => OfficeConfigNextAction::DraftCredentials,
        OfficeConfigReadiness::ReadyForProbe => OfficeConfigNextAction::Probe,
        OfficeConfigReadiness::ProbeUnavailable | OfficeConfigReadiness::Ready => {
            OfficeConfigNextAction::None
        }
    };
    OfficeAccountAssessment {
        account_key: account.account_key.clone(),
        provider_kind: account.provider_kind.clone(),
        enabled_capabilities: account.enabled_capabilities.clone(),
        credential_present: credential.is_some(),
        credential_configured: missing_fields.is_empty(),
        probe_supported,
        missing_fields,
        readiness,
        next_action,
        runtime_status: runtime_status.cloned(),
    }
}

fn collect_missing_fields(
    account: &OfficeAccount,
    credential: Option<&OfficeCredential>,
) -> Vec<String> {
    let mut missing = std::collections::BTreeSet::new();
    let access_token = credential
        .map(|item| item.access_token.trim())
        .unwrap_or_default();
    let external_account_id = account.external_account_id.trim();
    let metadata_value = |key: &str| {
        credential
            .and_then(|item| item.metadata_value(key))
            .map(str::trim)
            .unwrap_or_default()
    };

    match account.provider_kind.as_str() {
        "imap_smtp" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_MAIL_USERNAME).is_empty()
            {
                missing.insert("mail_username".to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_IMAP_HOST,
                metadata_value(OFFICE_METADATA_MAIL_IMAP_HOST),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_SMTP_HOST,
                metadata_value(OFFICE_METADATA_MAIL_SMTP_HOST),
            );
        }
        "webdav" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_DOCUMENTS_USERNAME).is_empty()
            {
                missing.insert(OFFICE_METADATA_DOCUMENTS_USERNAME.to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                metadata_value(OFFICE_METADATA_DOCUMENTS_BASE_URL),
            );
        }
        "caldav" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_CALENDAR_USERNAME_FIELD).is_empty()
            {
                missing.insert(OFFICE_METADATA_CALENDAR_USERNAME_FIELD.to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL_FIELD),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_ID,
                metadata_value(OFFICE_METADATA_CALENDAR_ID),
            );
        }
        _ => {
            if account
                .enabled_capabilities
                .iter()
                .any(|capability| *capability != OfficeCapability::ContactsDirectory)
            {
                push_missing_if_blank(&mut missing, "access_token", access_token);
            }
        }
    }

    missing.into_iter().collect()
}

fn push_missing_if_blank(
    missing: &mut std::collections::BTreeSet<String>,
    field: &str,
    value: &str,
) {
    if value.trim().is_empty() {
        missing.insert(field.to_string());
    }
}
