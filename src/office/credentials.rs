use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const REL_PATH_OFFICE_CREDENTIALS: &str = "config/office_credentials.json";
pub const OFFICE_METADATA_CALENDAR_ID: &str = "calendar_id";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCredential {
    pub account_key: String,
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
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCredentialStatus {
    pub account_key: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub has_refresh_token: bool,
    #[serde(default)]
    pub expires_at_unix_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCredentialsSegment {
    #[serde(default)]
    pub items: Vec<OfficeCredential>,
}

pub trait OfficeCredentialStore: Send + Sync {
    fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>>;
    fn list(&self) -> Result<Vec<OfficeCredential>>;
    fn set(&self, credential: &OfficeCredential) -> Result<()>;
    fn clear(&self, account_key: &str) -> Result<()>;
}

impl OfficeCredential {
    pub fn status(&self) -> OfficeCredentialStatus {
        OfficeCredentialStatus {
            account_key: self.account_key.clone(),
            configured: !self.access_token.trim().is_empty(),
            has_refresh_token: !self.refresh_token.trim().is_empty(),
            expires_at_unix_secs: self.expires_at_unix_secs,
            updated_at: self.updated_at,
        }
    }

    pub fn metadata_value(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(String::as_str)
    }
}
