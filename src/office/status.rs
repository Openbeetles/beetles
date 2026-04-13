use serde::{Deserialize, Serialize};

pub const REL_PATH_OFFICE_RUNTIME_STATUS: &str = "runtime/office_runtime_status.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountRuntimeStatus {
    pub account_key: String,
    #[serde(default)]
    pub probe_ok: bool,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_probe_at_unix_secs: u64,
    #[serde(default)]
    pub last_activity_kind: String,
    #[serde(default)]
    pub last_activity_ok: bool,
    #[serde(default)]
    pub last_activity_at_unix_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountStatusSummary {
    #[serde(default)]
    pub items: Vec<OfficeAccountRuntimeStatus>,
}

pub trait OfficeRuntimeStatusStore: Send + Sync {
    fn get(&self, account_key: &str) -> crate::error::Result<Option<OfficeAccountRuntimeStatus>>;
    fn list(&self) -> crate::error::Result<Vec<OfficeAccountRuntimeStatus>>;
    fn set(&self, status: &OfficeAccountRuntimeStatus) -> crate::error::Result<()>;
    fn clear(&self, account_key: &str) -> crate::error::Result<()>;
}
