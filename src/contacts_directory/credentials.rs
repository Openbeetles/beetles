use crate::error::{Error, Result};
use crate::office::{
    OfficeAuthoritySource, OfficeCapability, OfficeCredential, OfficeService,
    SnapshotOfficeAuthoritySource,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const OFFICE_METADATA_CONTACTS_APP_ID: &str = "contacts_app_id";
pub const OFFICE_METADATA_CONTACTS_BASE_URL: &str = "contacts_base_url";
pub const FEISHU_CONTACTS_DEFAULT_BASE_URL: &str = "https://open.feishu.cn";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryProviderCredential {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub secret: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryProviderCredentialStatus {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub base_url: String,
}

pub trait ContactsDirectoryProviderCredentialStore: Send + Sync {
    fn get(&self, account_key: &str) -> Result<Option<ContactsDirectoryProviderCredential>>;
    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>>;
    fn list_statuses(&self) -> Result<Vec<ContactsDirectoryProviderCredentialStatus>>;
}

impl ContactsDirectoryProviderCredential {
    pub fn status(&self) -> ContactsDirectoryProviderCredentialStatus {
        ContactsDirectoryProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            configured: !self.app_id.trim().is_empty()
                && !self.secret.trim().is_empty()
                && !self.base_url.trim().is_empty(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedContactsDirectoryProviderCredentialStore {
    authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
}

impl OfficeBackedContactsDirectoryProviderCredentialStore {
    pub fn new(office: OfficeService) -> Self {
        Self::with_authority(Arc::new(SnapshotOfficeAuthoritySource::new(office)))
    }

    pub fn with_authority(authority: Arc<dyn OfficeAuthoritySource + Send + Sync>) -> Self {
        Self { authority }
    }

    fn load_office(&self) -> Result<OfficeService> {
        self.authority.load()
    }
}

impl ContactsDirectoryProviderCredentialStore
    for OfficeBackedContactsDirectoryProviderCredentialStore
{
    fn get(&self, account_key: &str) -> Result<Option<ContactsDirectoryProviderCredential>> {
        let office = self.load_office()?;
        let Some(account) = office.account(account_key) else {
            return Ok(None);
        };
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::ContactsDirectory)
        {
            return Ok(None);
        }
        let Some(credential) = office.credential(account_key)? else {
            return Ok(None);
        };
        Ok(Some(contacts_credential_from_office(account, credential)?))
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        let office = self.load_office()?;
        let mut keys = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::ContactsDirectory) {
            if account.provider_kind != provider {
                continue;
            }
            if office.credential(&account.account_key)?.is_some() {
                keys.push(account.account_key);
            }
        }
        keys.sort();
        Ok(keys)
    }

    fn list_statuses(&self) -> Result<Vec<ContactsDirectoryProviderCredentialStatus>> {
        let office = self.load_office()?;
        let mut statuses = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::ContactsDirectory) {
            let Some(credential) = office.credential(&account.account_key)? else {
                continue;
            };
            statuses.push(contacts_credential_from_office(account, credential)?.status());
        }
        statuses.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.account_key.cmp(&right.account_key))
        });
        Ok(statuses)
    }
}

pub(crate) fn contacts_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> Result<ContactsDirectoryProviderCredential> {
    let app_id = credential
        .metadata_value(OFFICE_METADATA_CONTACTS_APP_ID)
        .unwrap_or_default()
        .trim()
        .to_string();
    let base_url = credential
        .metadata_value(OFFICE_METADATA_CONTACTS_BASE_URL)
        .unwrap_or(FEISHU_CONTACTS_DEFAULT_BASE_URL)
        .trim()
        .trim_end_matches('/')
        .to_string();
    if app_id.is_empty() {
        return Err(Error::config(
            "contacts_directory_provider_credential",
            "contacts_app_id must not be empty",
        ));
    }
    Ok(ContactsDirectoryProviderCredential {
        account_key: credential.account_key,
        provider: account.provider_kind,
        account_id: account.external_account_id,
        account_label: account.account_label,
        app_id,
        base_url,
        secret: credential.access_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccount, OfficeAccountIdentityClass, OfficeCapability};

    #[test]
    fn contacts_credential_from_office_maps_feishu_contacts_metadata() {
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
            updated_at: 1,
            metadata: [(
                OFFICE_METADATA_CONTACTS_APP_ID.to_string(),
                "cli_contacts".to_string(),
            )]
            .into_iter()
            .collect(),
        };

        let adapted = contacts_credential_from_office(account, credential).expect("adapted");
        assert_eq!(adapted.provider, "feishu_contacts_directory");
        assert_eq!(adapted.app_id, "cli_contacts");
        assert_eq!(adapted.base_url, FEISHU_CONTACTS_DEFAULT_BASE_URL);
        assert_eq!(adapted.secret, "app-secret");
    }
}
