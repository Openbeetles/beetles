use crate::error::{Error, Result};
use crate::office::{
    normalize_google_api_base_url, normalize_microsoft_graph_base_url,
    OfficeAuthorityBackedCredentialStoreCore, OfficeAuthoritySource, OfficeCapability,
    OfficeCredential, OfficeService, GOOGLE_CALENDAR_DEFAULT_BASE_URL,
    MICROSOFT_GRAPH_DEFAULT_BASE_URL, OFFICE_METADATA_CALENDAR_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const OFFICE_METADATA_CALENDAR_USERNAME: &str = "calendar_username";
pub const OFFICE_METADATA_CALENDAR_BASE_URL: &str = "calendar_base_url";
pub const OFFICE_METADATA_CALENDAR_ROOT_PATH: &str = "calendar_root_path";
pub const OFFICE_METADATA_CALENDAR_APP_ID: &str = "calendar_app_id";
pub const OFFICE_METADATA_CALENDAR_CORP_ID: &str = "calendar_corp_id";
pub const FEISHU_CALENDAR_DEFAULT_BASE_URL: &str = "https://open.feishu.cn";
pub const GOOGLE_DEFAULT_CALENDAR_ID: &str = "primary";
pub const MICROSOFT365_DEFAULT_CALENDAR_ID: &str = "primary";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarProviderCredential {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub calendar_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub root_path: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub token_endpoint: String,
    #[serde(default)]
    pub expires_at_unix_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarProviderCredentialStatus {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub calendar_id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub root_path: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub has_refresh_token: bool,
    #[serde(default)]
    pub expires_at_unix_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

pub trait CalendarProviderCredentialStore: Send + Sync {
    fn get(&self, account_key: &str) -> Result<Option<CalendarProviderCredential>>;
    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>>;
    fn set(&self, credential: &CalendarProviderCredential) -> Result<()>;
    fn clear(&self, account_key: &str) -> Result<()>;
    fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>>;
}

impl CalendarProviderCredential {
    pub fn status(&self) -> CalendarProviderCredentialStatus {
        let transport_configured = match self.provider.as_str() {
            "caldav" => !self.username.trim().is_empty() && !self.base_url.trim().is_empty(),
            "feishu_calendar" | "wecom_calendar" => {
                !self.app_id.trim().is_empty()
                    && !self.base_url.trim().is_empty()
                    && !self.calendar_id.trim().is_empty()
            }
            "microsoft365_calendar" => {
                !self.access_token.trim().is_empty()
                    && !self.base_url.trim().is_empty()
                    && !self.calendar_id.trim().is_empty()
            }
            "google_calendar" => {
                !self.access_token.trim().is_empty()
                    && !self.base_url.trim().is_empty()
                    && !self.calendar_id.trim().is_empty()
            }
            _ => true,
        };
        CalendarProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            calendar_id: self.calendar_id.clone(),
            username: self.username.clone(),
            app_id: self.app_id.clone(),
            base_url: self.base_url.clone(),
            root_path: self.root_path.clone(),
            configured: !self.access_token.trim().is_empty() && transport_configured,
            has_refresh_token: !self.refresh_token.trim().is_empty(),
            expires_at_unix_secs: self.expires_at_unix_secs,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedCalendarProviderCredentialStore {
    core: OfficeAuthorityBackedCredentialStoreCore,
}

impl OfficeBackedCalendarProviderCredentialStore {
    pub fn new(office: OfficeService) -> Self {
        Self {
            core: OfficeAuthorityBackedCredentialStoreCore::new(office),
        }
    }

    pub fn with_authority(authority: Arc<dyn OfficeAuthoritySource + Send + Sync>) -> Self {
        Self {
            core: OfficeAuthorityBackedCredentialStoreCore::with_authority(authority),
        }
    }
}

impl CalendarProviderCredentialStore for OfficeBackedCalendarProviderCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<CalendarProviderCredential>> {
        self.core.get_for_capability(
            OfficeCapability::Calendar,
            account_key,
            |account, credential| Ok(calendar_credential_from_office(account, credential)),
        )
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.core
            .find_account_keys_by_provider(OfficeCapability::Calendar, provider)
    }

    fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
        let office = self.core.load_office()?;
        let account = office.account(&credential.account_key).ok_or_else(|| {
            Error::config(
                "calendar_provider",
                format!(
                    "calendar credential account '{}' is not registered",
                    credential.account_key
                ),
            )
        })?;
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::Calendar)
        {
            return Err(Error::config(
                "calendar_provider",
                format!(
                    "account '{}' does not enable calendar capability",
                    credential.account_key
                ),
            ));
        }
        if !credential.provider.trim().is_empty() && credential.provider != account.provider_kind {
            return Err(Error::config(
                "calendar_provider",
                format!(
                    "account '{}' is registered for provider '{}', not '{}'",
                    credential.account_key, account.provider_kind, credential.provider
                ),
            ));
        }
        let mut metadata = BTreeMap::new();
        if !credential.calendar_id.trim().is_empty() {
            metadata.insert(
                OFFICE_METADATA_CALENDAR_ID.to_string(),
                credential.calendar_id.trim().to_string(),
            );
        }
        if !credential.username.trim().is_empty() {
            metadata.insert(
                OFFICE_METADATA_CALENDAR_USERNAME.to_string(),
                credential.username.trim().to_string(),
            );
        }
        if !credential.app_id.trim().is_empty() {
            metadata.insert(
                OFFICE_METADATA_CALENDAR_APP_ID.to_string(),
                credential.app_id.trim().to_string(),
            );
        }
        if !credential.base_url.trim().is_empty() {
            metadata.insert(
                OFFICE_METADATA_CALENDAR_BASE_URL.to_string(),
                credential.base_url.trim().trim_end_matches('/').to_string(),
            );
        }
        if !credential.root_path.trim().is_empty() {
            metadata.insert(
                OFFICE_METADATA_CALENDAR_ROOT_PATH.to_string(),
                credential.root_path.trim().to_string(),
            );
        }
        office.set_credential(&OfficeCredential {
            account_key: credential.account_key.clone(),
            access_token: credential.access_token.clone(),
            refresh_token: credential.refresh_token.clone(),
            token_endpoint: credential.token_endpoint.clone(),
            expires_at_unix_secs: credential.expires_at_unix_secs,
            updated_at: credential.updated_at,
            metadata,
        })
    }

    fn clear(&self, account_key: &str) -> Result<()> {
        self.core.load_office()?.clear_credential(account_key)
    }

    fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>> {
        self.core
            .list_statuses_for_capability(OfficeCapability::Calendar, |account, credential| {
                Ok(calendar_credential_from_office(account, credential).status())
            })
    }
}

pub(crate) fn calendar_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> CalendarProviderCredential {
    let calendar_id = credential
        .metadata_value(OFFICE_METADATA_CALENDAR_ID)
        .unwrap_or(if account.provider_kind == "microsoft365_calendar" {
            MICROSOFT365_DEFAULT_CALENDAR_ID
        } else if account.provider_kind == "google_calendar" {
            GOOGLE_DEFAULT_CALENDAR_ID
        } else {
            ""
        })
        .to_string();
    if account.provider_kind == "google_calendar" {
        let base_url = normalize_google_api_base_url(
            credential
                .metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL)
                .unwrap_or(GOOGLE_CALENDAR_DEFAULT_BASE_URL),
            GOOGLE_CALENDAR_DEFAULT_BASE_URL,
        );
        return CalendarProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            calendar_id,
            username: String::new(),
            app_id: String::new(),
            base_url,
            root_path: String::new(),
            access_token: credential.access_token,
            refresh_token: credential.refresh_token,
            token_endpoint: credential.token_endpoint,
            expires_at_unix_secs: credential.expires_at_unix_secs,
            updated_at: credential.updated_at,
        };
    }
    if account.provider_kind == "microsoft365_calendar" {
        let base_url = normalize_microsoft_graph_base_url(
            credential
                .metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL)
                .unwrap_or(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
        );
        return CalendarProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            calendar_id,
            username: String::new(),
            app_id: String::new(),
            base_url,
            root_path: String::new(),
            access_token: credential.access_token,
            refresh_token: credential.refresh_token,
            token_endpoint: credential.token_endpoint,
            expires_at_unix_secs: credential.expires_at_unix_secs,
            updated_at: credential.updated_at,
        };
    }
    if matches!(
        account.provider_kind.as_str(),
        "feishu_calendar" | "wecom_calendar"
    ) {
        let app_id_key = if account.provider_kind == "wecom_calendar" {
            OFFICE_METADATA_CALENDAR_CORP_ID
        } else {
            OFFICE_METADATA_CALENDAR_APP_ID
        };
        let default_base_url = if account.provider_kind == "wecom_calendar" {
            crate::office::WECOM_DEFAULT_BASE_URL
        } else {
            FEISHU_CALENDAR_DEFAULT_BASE_URL
        };
        let app_id = credential
            .metadata_value(app_id_key)
            .unwrap_or_default()
            .trim()
            .to_string();
        let base_url = credential
            .metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL)
            .unwrap_or(default_base_url)
            .trim()
            .trim_end_matches('/')
            .to_string();
        return CalendarProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            calendar_id,
            username: String::new(),
            app_id,
            base_url,
            root_path: String::new(),
            access_token: credential.access_token,
            refresh_token: credential.refresh_token,
            token_endpoint: credential.token_endpoint,
            expires_at_unix_secs: credential.expires_at_unix_secs,
            updated_at: credential.updated_at,
        };
    }
    let username = credential
        .metadata_value(OFFICE_METADATA_CALENDAR_USERNAME)
        .unwrap_or(account.external_account_id.as_str())
        .trim()
        .to_string();
    let base_url = credential
        .metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();
    let root_path = credential
        .metadata_value(OFFICE_METADATA_CALENDAR_ROOT_PATH)
        .unwrap_or_default()
        .trim()
        .to_string();
    CalendarProviderCredential {
        account_key: credential.account_key,
        provider: account.provider_kind,
        account_id: account.external_account_id,
        account_label: account.account_label,
        calendar_id,
        username,
        app_id: String::new(),
        base_url,
        root_path,
        access_token: credential.access_token,
        refresh_token: credential.refresh_token,
        token_endpoint: credential.token_endpoint,
        expires_at_unix_secs: credential.expires_at_unix_secs,
        updated_at: credential.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccount, OfficeAccountIdentityClass, OfficeCapability};

    #[test]
    fn calendar_credential_from_office_maps_caldav_transport_metadata() {
        let credential = calendar_credential_from_office(
            OfficeAccount {
                account_key: "calendar-work".to_string(),
                provider_kind: "caldav".to_string(),
                external_account_id: "work@example.com".to_string(),
                account_label: "Work Calendar".to_string(),
                identity_class: OfficeAccountIdentityClass::Work,
                enabled_capabilities: vec![OfficeCapability::Calendar],
            },
            OfficeCredential {
                account_key: "calendar-work".to_string(),
                access_token: "app-password".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 42,
                metadata: [
                    ("calendar_id".to_string(), "team".to_string()),
                    ("calendar_username".to_string(), "caldav-user".to_string()),
                    (
                        "calendar_base_url".to_string(),
                        "https://dav.example.com/remote.php/dav/calendars".to_string(),
                    ),
                    ("calendar_root_path".to_string(), "/work".to_string()),
                ]
                .into_iter()
                .collect(),
            },
        );

        let value = serde_json::to_value(&credential).expect("serialize calendar credential");
        assert_eq!(value["calendar_id"], "team");
        assert_eq!(value["username"], "caldav-user");
        assert_eq!(
            value["base_url"],
            "https://dav.example.com/remote.php/dav/calendars"
        );
        assert_eq!(value["root_path"], "/work");
    }

    #[test]
    fn calendar_credential_from_office_maps_feishu_calendar_metadata() {
        let credential = calendar_credential_from_office(
            OfficeAccount {
                account_key: "calendar-feishu".to_string(),
                provider_kind: "feishu_calendar".to_string(),
                external_account_id: "work-calendar".to_string(),
                account_label: "Feishu Calendar".to_string(),
                identity_class: OfficeAccountIdentityClass::Work,
                enabled_capabilities: vec![OfficeCapability::Calendar],
            },
            OfficeCredential {
                account_key: "calendar-feishu".to_string(),
                access_token: "app-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 42,
                metadata: [
                    (
                        OFFICE_METADATA_CALENDAR_ID.to_string(),
                        "cal_a1b2".to_string(),
                    ),
                    (
                        OFFICE_METADATA_CALENDAR_APP_ID.to_string(),
                        "cli_calendar".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            },
        );

        assert_eq!(credential.provider, "feishu_calendar");
        assert_eq!(credential.calendar_id, "cal_a1b2");
        assert_eq!(credential.app_id, "cli_calendar");
        assert_eq!(credential.base_url, FEISHU_CALENDAR_DEFAULT_BASE_URL);
        assert_eq!(credential.username, "");
    }

    #[test]
    fn calendar_credential_from_office_maps_wecom_calendar_metadata() {
        let credential = calendar_credential_from_office(
            OfficeAccount {
                account_key: "calendar-wecom".to_string(),
                provider_kind: "wecom_calendar".to_string(),
                external_account_id: "calendar-admin".to_string(),
                account_label: "WeCom Calendar".to_string(),
                identity_class: OfficeAccountIdentityClass::Work,
                enabled_capabilities: vec![OfficeCapability::Calendar],
            },
            OfficeCredential {
                account_key: "calendar-wecom".to_string(),
                access_token: "corp-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 42,
                metadata: [
                    ("calendar_id".to_string(), "cal-wecom-1".to_string()),
                    ("calendar_corp_id".to_string(), "wwcorp123".to_string()),
                ]
                .into_iter()
                .collect(),
            },
        );

        assert_eq!(credential.provider, "wecom_calendar");
        assert_eq!(credential.calendar_id, "cal-wecom-1");
        assert_eq!(credential.app_id, "wwcorp123");
        assert_eq!(credential.base_url, crate::office::WECOM_DEFAULT_BASE_URL);
        assert_eq!(credential.username, "");
    }

    #[test]
    fn calendar_credential_from_office_maps_microsoft_calendar_metadata() {
        let credential = calendar_credential_from_office(
            OfficeAccount {
                account_key: "calendar-ms".to_string(),
                provider_kind: "microsoft365_calendar".to_string(),
                external_account_id: "alice@contoso.com".to_string(),
                account_label: "Microsoft Calendar".to_string(),
                identity_class: OfficeAccountIdentityClass::Work,
                enabled_capabilities: vec![OfficeCapability::Calendar],
            },
            OfficeCredential {
                account_key: "calendar-ms".to_string(),
                access_token: "ms-token".to_string(),
                refresh_token: "refresh".to_string(),
                token_endpoint: "https://login.microsoftonline.com/common/oauth2/v2.0/token"
                    .to_string(),
                expires_at_unix_secs: 0,
                updated_at: 42,
                metadata: std::collections::BTreeMap::new(),
            },
        );

        assert_eq!(credential.provider, "microsoft365_calendar");
        assert_eq!(credential.calendar_id, MICROSOFT365_DEFAULT_CALENDAR_ID);
        assert_eq!(credential.base_url, MICROSOFT_GRAPH_DEFAULT_BASE_URL);
        assert_eq!(credential.app_id, "");
    }

    #[test]
    fn calendar_credential_from_office_maps_google_calendar_metadata() {
        let credential = calendar_credential_from_office(
            OfficeAccount {
                account_key: "calendar-google".to_string(),
                provider_kind: "google_calendar".to_string(),
                external_account_id: "alice@gmail.com".to_string(),
                account_label: "Google Calendar".to_string(),
                identity_class: OfficeAccountIdentityClass::Personal,
                enabled_capabilities: vec![OfficeCapability::Calendar],
            },
            OfficeCredential {
                account_key: "calendar-google".to_string(),
                access_token: "google-token".to_string(),
                refresh_token: "refresh".to_string(),
                token_endpoint: "https://oauth2.googleapis.com/token".to_string(),
                expires_at_unix_secs: 0,
                updated_at: 42,
                metadata: std::collections::BTreeMap::new(),
            },
        );

        assert_eq!(credential.provider, "google_calendar");
        assert_eq!(credential.calendar_id, GOOGLE_DEFAULT_CALENDAR_ID);
        assert_eq!(credential.base_url, GOOGLE_CALENDAR_DEFAULT_BASE_URL);
        assert_eq!(credential.app_id, "");
    }
}
