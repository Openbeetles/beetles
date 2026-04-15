use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum OfficeCapability {
    Mail,
    Calendar,
    Documents,
    ContactsDirectory,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum OfficeAccountIdentityClass {
    Work,
    Personal,
    Family,
    Shared,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccount {
    pub account_key: String,
    pub provider_kind: String,
    #[serde(default)]
    pub external_account_id: String,
    #[serde(default)]
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    #[serde(default)]
    pub enabled_capabilities: Vec<OfficeCapability>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountRegistry {
    #[serde(default)]
    accounts: BTreeMap<String, OfficeAccount>,
}

impl OfficeAccountRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, account: OfficeAccount) {
        self.accounts.insert(account.account_key.clone(), account);
    }

    pub fn get(&self, account_key: &str) -> Option<&OfficeAccount> {
        self.accounts.get(account_key)
    }

    pub fn remove(&mut self, account_key: &str) -> Option<OfficeAccount> {
        self.accounts.remove(account_key)
    }

    pub fn all_accounts(&self) -> Vec<&OfficeAccount> {
        self.accounts.values().collect()
    }

    pub fn accounts_for_capability(&self, capability: OfficeCapability) -> Vec<&OfficeAccount> {
        self.accounts
            .values()
            .filter(|account| account.enabled_capabilities.contains(&capability))
            .collect()
    }
}

impl OfficeCapability {
    pub const fn all() -> [OfficeCapability; 4] {
        [
            OfficeCapability::Mail,
            OfficeCapability::Calendar,
            OfficeCapability::Documents,
            OfficeCapability::ContactsDirectory,
        ]
    }
}
