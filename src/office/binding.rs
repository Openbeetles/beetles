use crate::office::OfficeCapability;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCapabilityBinding {
    #[serde(default)]
    capability_defaults: BTreeMap<OfficeCapability, String>,
}

impl OfficeCapabilityBinding {
    pub fn set_default_account(&mut self, capability: OfficeCapability, account_key: String) {
        self.capability_defaults.insert(capability, account_key);
    }

    pub fn default_account_for(&self, capability: OfficeCapability) -> Option<&str> {
        self.capability_defaults
            .get(&capability)
            .map(String::as_str)
    }
}
