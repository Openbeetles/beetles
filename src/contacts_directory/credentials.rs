use crate::error::{Error, Result};
use crate::office::{
    normalize_google_api_base_url, normalize_microsoft_graph_base_url,
    OfficeAuthorityBackedCredentialStoreCore, OfficeAuthoritySource, OfficeCapability,
    OfficeCredential, OfficeService, GOOGLE_PEOPLE_DEFAULT_BASE_URL,
    MICROSOFT_GRAPH_DEFAULT_BASE_URL,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const OFFICE_METADATA_CONTACTS_APP_ID: &str = "contacts_app_id";
pub const OFFICE_METADATA_CONTACTS_BASE_URL: &str = "contacts_base_url";
pub const OFFICE_METADATA_CONTACTS_CORP_ID: &str = "contacts_corp_id";
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
        let configured = match self.provider.as_str() {
            "wecom_contacts_directory" => {
                !self.app_id.trim().is_empty()
                    && !self.secret.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
            "microsoft365_contacts_directory" => {
                !self.secret.trim().is_empty() && !self.base_url.trim().is_empty()
            }
            "google_contacts_directory" => {
                !self.secret.trim().is_empty() && !self.base_url.trim().is_empty()
            }
            _ => {
                !self.app_id.trim().is_empty()
                    && !self.secret.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
        };
        ContactsDirectoryProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            configured,
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedContactsDirectoryProviderCredentialStore {
    core: OfficeAuthorityBackedCredentialStoreCore,
}

impl OfficeBackedContactsDirectoryProviderCredentialStore {
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

impl ContactsDirectoryProviderCredentialStore
    for OfficeBackedContactsDirectoryProviderCredentialStore
{
    fn get(&self, account_key: &str) -> Result<Option<ContactsDirectoryProviderCredential>> {
        self.core.get_for_capability(
            OfficeCapability::ContactsDirectory,
            account_key,
            contacts_credential_from_office,
        )
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        self.core
            .find_account_keys_by_provider(OfficeCapability::ContactsDirectory, provider)
    }

    fn list_statuses(&self) -> Result<Vec<ContactsDirectoryProviderCredentialStatus>> {
        self.core.list_statuses_for_capability(
            OfficeCapability::ContactsDirectory,
            |account, credential| Ok(contacts_credential_from_office(account, credential)?.status()),
        )
    }
}

pub(crate) fn contacts_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> Result<ContactsDirectoryProviderCredential> {
    if account.provider_kind == "google_contacts_directory" {
        let base_url = normalize_google_api_base_url(
            credential
                .metadata_value(OFFICE_METADATA_CONTACTS_BASE_URL)
                .unwrap_or(GOOGLE_PEOPLE_DEFAULT_BASE_URL),
            GOOGLE_PEOPLE_DEFAULT_BASE_URL,
        );
        return Ok(ContactsDirectoryProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            app_id: String::new(),
            base_url,
            secret: credential.access_token,
        });
    }

    if account.provider_kind == "microsoft365_contacts_directory" {
        let base_url = normalize_microsoft_graph_base_url(
            credential
                .metadata_value(OFFICE_METADATA_CONTACTS_BASE_URL)
                .unwrap_or(MICROSOFT_GRAPH_DEFAULT_BASE_URL),
        );
        return Ok(ContactsDirectoryProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            app_id: String::new(),
            base_url,
            secret: credential.access_token,
        });
    }

    if account.provider_kind == "wecom_contacts_directory" {
        let corp_id = credential
            .metadata_value(OFFICE_METADATA_CONTACTS_CORP_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let base_url = credential
            .metadata_value(OFFICE_METADATA_CONTACTS_BASE_URL)
            .unwrap_or(crate::office::WECOM_DEFAULT_BASE_URL)
            .trim()
            .trim_end_matches('/')
            .to_string();
        if corp_id.is_empty() {
            return Err(Error::config(
                "contacts_directory_provider_credential",
                "contacts_corp_id must not be empty",
            ));
        }
        return Ok(ContactsDirectoryProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            app_id: corp_id,
            base_url,
            secret: credential.access_token,
        });
    }

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

    #[test]
    fn contacts_credential_from_office_maps_wecom_contacts_metadata() {
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
            updated_at: 1,
            metadata: [(
                OFFICE_METADATA_CONTACTS_CORP_ID.to_string(),
                "wwcorp123".to_string(),
            )]
            .into_iter()
            .collect(),
        };

        let adapted = contacts_credential_from_office(account, credential).expect("adapted");
        assert_eq!(adapted.provider, "wecom_contacts_directory");
        assert_eq!(adapted.app_id, "wwcorp123");
        assert_eq!(adapted.base_url, crate::office::WECOM_DEFAULT_BASE_URL);
        assert_eq!(adapted.secret, "corp-secret");
    }

    #[test]
    fn contacts_credential_from_office_maps_microsoft_contacts_metadata() {
        let account = OfficeAccount {
            account_key: "contacts-ms".to_string(),
            provider_kind: "microsoft365_contacts_directory".to_string(),
            external_account_id: "alice@contoso.com".to_string(),
            account_label: "Microsoft Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        };
        let credential = OfficeCredential {
            account_key: "contacts-ms".to_string(),
            access_token: "graph-token".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 1,
            metadata: std::collections::BTreeMap::new(),
        };

        let adapted = contacts_credential_from_office(account, credential).expect("adapted");
        assert_eq!(adapted.provider, "microsoft365_contacts_directory");
        assert_eq!(adapted.base_url, MICROSOFT_GRAPH_DEFAULT_BASE_URL);
        assert_eq!(adapted.app_id, "");
        assert_eq!(adapted.secret, "graph-token");
    }

    #[test]
    fn contacts_credential_from_office_maps_google_contacts_metadata() {
        let account = OfficeAccount {
            account_key: "contacts-google".to_string(),
            provider_kind: "google_contacts_directory".to_string(),
            external_account_id: "alice@gmail.com".to_string(),
            account_label: "Google Contacts".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
        };
        let credential = OfficeCredential {
            account_key: "contacts-google".to_string(),
            access_token: "google-people-token".to_string(),
            refresh_token: String::new(),
            token_endpoint: String::new(),
            expires_at_unix_secs: 0,
            updated_at: 1,
            metadata: std::collections::BTreeMap::new(),
        };

        let adapted = contacts_credential_from_office(account, credential).expect("adapted");
        assert_eq!(adapted.provider, "google_contacts_directory");
        assert_eq!(adapted.base_url, GOOGLE_PEOPLE_DEFAULT_BASE_URL);
        assert_eq!(adapted.app_id, "");
        assert_eq!(adapted.secret, "google-people-token");
    }
}
