use crate::error::{Error, Result};
use crate::office::{
    OfficeCapability, OfficeCredential, OfficeService, OFFICE_METADATA_CALENDAR_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const OFFICE_METADATA_CALENDAR_USERNAME: &str = "calendar_username";
pub const OFFICE_METADATA_CALENDAR_BASE_URL: &str = "calendar_base_url";
pub const OFFICE_METADATA_CALENDAR_ROOT_PATH: &str = "calendar_root_path";

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
        let transport_configured = if self.provider == "caldav" {
            !self.username.trim().is_empty() && !self.base_url.trim().is_empty()
        } else {
            true
        };
        CalendarProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            calendar_id: self.calendar_id.clone(),
            username: self.username.clone(),
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
    office: OfficeService,
}

impl OfficeBackedCalendarProviderCredentialStore {
    pub fn new(office: OfficeService) -> Self {
        Self { office }
    }
}

impl CalendarProviderCredentialStore for OfficeBackedCalendarProviderCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<CalendarProviderCredential>> {
        let Some(account) = self.office.account(account_key) else {
            return Ok(None);
        };
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::Calendar)
        {
            return Ok(None);
        }
        let Some(credential) = self.office.credential(account_key)? else {
            return Ok(None);
        };
        Ok(Some(calendar_credential_from_office(account, credential)))
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        for account in self.office.accounts_for_capability(OfficeCapability::Calendar) {
            if account.provider_kind != provider {
                continue;
            }
            if self.office.credential(&account.account_key)?.is_some() {
                keys.push(account.account_key);
            }
        }
        keys.sort();
        Ok(keys)
    }

    fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
        let account = self.office.account(&credential.account_key).ok_or_else(|| {
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
        self.office.set_credential(&OfficeCredential {
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
        self.office.clear_credential(account_key)
    }

    fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>> {
        let mut statuses = Vec::new();
        for account in self.office.accounts_for_capability(OfficeCapability::Calendar) {
            let Some(credential) = self.office.credential(&account.account_key)? else {
                continue;
            };
            statuses.push(calendar_credential_from_office(account, credential).status());
        }
        statuses.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.account_key.cmp(&right.account_key))
        });
        Ok(statuses)
    }
}

pub(crate) fn calendar_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> CalendarProviderCredential {
    let calendar_id = credential
        .metadata_value(OFFICE_METADATA_CALENDAR_ID)
        .unwrap_or_default()
        .to_string();
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
                    (
                        "calendar_username".to_string(),
                        "caldav-user".to_string(),
                    ),
                    (
                        "calendar_base_url".to_string(),
                        "https://dav.example.com/remote.php/dav/calendars".to_string(),
                    ),
                    (
                        "calendar_root_path".to_string(),
                        "/work".to_string(),
                    ),
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
}
