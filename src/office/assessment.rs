use crate::calendar::{OFFICE_METADATA_CALENDAR_APP_ID, OFFICE_METADATA_CALENDAR_CORP_ID};
use crate::contacts_directory::OFFICE_METADATA_CONTACTS_APP_ID;
use crate::contacts_directory::OFFICE_METADATA_CONTACTS_CORP_ID;
use crate::documents::{
    OFFICE_METADATA_DOCUMENTS_APP_ID, OFFICE_METADATA_DOCUMENTS_BASE_URL,
    OFFICE_METADATA_DOCUMENTS_CORP_ID, OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
    OFFICE_METADATA_DOCUMENTS_SPACE_ID, OFFICE_METADATA_DOCUMENTS_USERNAME,
};
use crate::mail::{
    OFFICE_METADATA_MAIL_BASE_URL, OFFICE_METADATA_MAIL_CORP_ID, OFFICE_METADATA_MAIL_IMAP_HOST,
    OFFICE_METADATA_MAIL_SMTP_HOST, OFFICE_METADATA_MAIL_USERNAME,
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
        "imap_smtp" | "feishu_mail" => {
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
        "wecom_mail" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_CORP_ID,
                metadata_value(OFFICE_METADATA_MAIL_CORP_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_BASE_URL,
                metadata_value(OFFICE_METADATA_MAIL_BASE_URL),
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
        "feishu_documents" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_APP_ID,
                metadata_value(OFFICE_METADATA_DOCUMENTS_APP_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                metadata_value(OFFICE_METADATA_DOCUMENTS_ROOT_PATH),
            );
        }
        "wecom_documents" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_CORP_ID,
                metadata_value(OFFICE_METADATA_DOCUMENTS_CORP_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_SPACE_ID,
                metadata_value(OFFICE_METADATA_DOCUMENTS_SPACE_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_ROOT_PATH,
                metadata_value(OFFICE_METADATA_DOCUMENTS_ROOT_PATH),
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
        "feishu_calendar" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_APP_ID,
                metadata_value(OFFICE_METADATA_CALENDAR_APP_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_ID,
                metadata_value(OFFICE_METADATA_CALENDAR_ID),
            );
        }
        "wecom_calendar" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_CORP_ID,
                metadata_value(OFFICE_METADATA_CALENDAR_CORP_ID),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_ID,
                metadata_value(OFFICE_METADATA_CALENDAR_ID),
            );
        }
        "feishu_contacts_directory" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CONTACTS_APP_ID,
                metadata_value(OFFICE_METADATA_CONTACTS_APP_ID),
            );
        }
        "wecom_contacts_directory" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CONTACTS_CORP_ID,
                metadata_value(OFFICE_METADATA_CONTACTS_CORP_ID),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::OFFICE_METADATA_DOCUMENTS_ROOT_PATH;
    use crate::office::OfficeAccountIdentityClass;

    #[test]
    fn assess_office_account_marks_feishu_documents_missing_app_id_and_root_path() {
        let account = OfficeAccount {
            account_key: "docs-feishu".to_string(),
            provider_kind: "feishu_documents".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        };
        let credential = OfficeCredential {
            account_key: "docs-feishu".to_string(),
            access_token: "app-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"documents_app_id".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&OFFICE_METADATA_DOCUMENTS_ROOT_PATH.to_string()));
    }

    #[test]
    fn assess_office_account_marks_wecom_documents_missing_corp_space_and_root() {
        let account = OfficeAccount {
            account_key: "docs-wecom".to_string(),
            provider_kind: "wecom_documents".to_string(),
            external_account_id: String::new(),
            account_label: "WeCom Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        };
        let credential = OfficeCredential {
            account_key: "docs-wecom".to_string(),
            access_token: "corp-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"documents_corp_id".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"documents_space_id".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&OFFICE_METADATA_DOCUMENTS_ROOT_PATH.to_string()));
    }

    #[test]
    fn assess_office_account_marks_feishu_calendar_missing_app_id_and_calendar_id() {
        let account = OfficeAccount {
            account_key: "calendar-feishu".to_string(),
            provider_kind: "feishu_calendar".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Calendar".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        };
        let credential = OfficeCredential {
            account_key: "calendar-feishu".to_string(),
            access_token: "app-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"calendar_app_id".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"calendar_id".to_string()));
    }

    #[test]
    fn assess_office_account_marks_wecom_calendar_missing_corp_id_and_calendar_id() {
        let account = OfficeAccount {
            account_key: "calendar-wecom".to_string(),
            provider_kind: "wecom_calendar".to_string(),
            external_account_id: String::new(),
            account_label: "WeCom Calendar".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        };
        let credential = OfficeCredential {
            account_key: "calendar-wecom".to_string(),
            access_token: "corp-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"calendar_corp_id".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"calendar_id".to_string()));
    }

    #[test]
    fn assess_office_account_marks_feishu_contacts_missing_app_id() {
        let account = OfficeAccount {
            account_key: "contacts-feishu".to_string(),
            provider_kind: "feishu_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        };
        let credential = OfficeCredential {
            account_key: "contacts-feishu".to_string(),
            access_token: "app-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"contacts_app_id".to_string()));
    }

    #[test]
    fn assess_office_account_marks_wecom_contacts_missing_corp_id() {
        let account = OfficeAccount {
            account_key: "contacts-wecom".to_string(),
            provider_kind: "wecom_contacts_directory".to_string(),
            external_account_id: String::new(),
            account_label: "WeCom Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        };
        let credential = OfficeCredential {
            account_key: "contacts-wecom".to_string(),
            access_token: "corp-secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&"contacts_corp_id".to_string()));
    }

    #[test]
    fn assess_office_account_marks_feishu_mail_missing_transport_fields() {
        let account = OfficeAccount {
            account_key: "mail-feishu".to_string(),
            provider_kind: "feishu_mail".to_string(),
            external_account_id: String::new(),
            account_label: "Feishu Mail".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        };
        let credential = OfficeCredential {
            account_key: "mail-feishu".to_string(),
            access_token: "secret".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 0,
            metadata: std::collections::BTreeMap::new(),
        };
        let assessment = assess_office_account(&account, Some(&credential), None, true);
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert!(assessment
            .missing_fields
            .contains(&OFFICE_METADATA_MAIL_USERNAME.to_string()));
        assert!(assessment
            .missing_fields
            .contains(&OFFICE_METADATA_MAIL_IMAP_HOST.to_string()));
        assert!(assessment
            .missing_fields
            .contains(&OFFICE_METADATA_MAIL_SMTP_HOST.to_string()));
    }
}
