use crate::office::OfficeAccountIdentityClass;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeSelectionPolicy {
    #[serde(default)]
    pub global_default_account_key: String,
    #[serde(default)]
    pub ask_when_ambiguous: bool,
    #[serde(default)]
    pub preferred_identity_class: Option<OfficeAccountIdentityClass>,
}
