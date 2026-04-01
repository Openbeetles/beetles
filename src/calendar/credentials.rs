use crate::error::Result;
use serde::{Deserialize, Serialize};

pub const REL_PATH_CALENDAR_PROVIDER_CREDENTIALS: &str = "config/calendar_credentials.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarProviderCredential {
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub calendar_id: String,
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
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub calendar_id: String,
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
    fn get(&self, provider: &str) -> Result<Option<CalendarProviderCredential>>;
    fn set(&self, credential: &CalendarProviderCredential) -> Result<()>;
    fn clear(&self, provider: &str) -> Result<()>;
    fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>>;
}

impl CalendarProviderCredential {
    pub fn status(&self) -> CalendarProviderCredentialStatus {
        CalendarProviderCredentialStatus {
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            calendar_id: self.calendar_id.clone(),
            configured: !self.access_token.trim().is_empty(),
            has_refresh_token: !self.refresh_token.trim().is_empty(),
            expires_at_unix_secs: self.expires_at_unix_secs,
            updated_at: self.updated_at,
        }
    }
}
