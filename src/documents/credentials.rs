use crate::error::{Error, Result};
use crate::office::{OfficeCapability, OfficeCredential, OfficeService};
use serde::{Deserialize, Serialize};

pub const OFFICE_METADATA_DOCUMENTS_USERNAME: &str = "documents_username";
pub const OFFICE_METADATA_DOCUMENTS_BASE_URL: &str = "documents_base_url";
pub const OFFICE_METADATA_DOCUMENTS_ROOT_PATH: &str = "documents_root_path";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsProviderCredential {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub root_path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentsProviderCredentialStatus {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub root_path: String,
    #[serde(default)]
    pub base_url: String,
}

pub trait DocumentsProviderCredentialStore: Send + Sync {
    fn get(&self, account_key: &str) -> Result<Option<DocumentsProviderCredential>>;
    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>>;
    fn list_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>>;
}

impl DocumentsProviderCredential {
    pub fn status(&self) -> DocumentsProviderCredentialStatus {
        DocumentsProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            configured: !self.username.trim().is_empty()
                && !self.secret.trim().is_empty()
                && !self.base_url.trim().is_empty(),
            root_path: self.root_path.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedDocumentsProviderCredentialStore {
    office: OfficeService,
}

impl OfficeBackedDocumentsProviderCredentialStore {
    pub fn new(office: OfficeService) -> Self {
        Self { office }
    }
}

impl DocumentsProviderCredentialStore for OfficeBackedDocumentsProviderCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<DocumentsProviderCredential>> {
        let Some(account) = self.office.account(account_key) else {
            return Ok(None);
        };
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::Documents)
        {
            return Ok(None);
        }
        let Some(credential) = self.office.credential(account_key)? else {
            return Ok(None);
        };
        Ok(Some(documents_credential_from_office(account, credential)?))
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        let mut keys = Vec::new();
        for account in self.office.accounts_for_capability(OfficeCapability::Documents) {
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

    fn list_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>> {
        let mut statuses = Vec::new();
        for account in self.office.accounts_for_capability(OfficeCapability::Documents) {
            let Some(credential) = self.office.credential(&account.account_key)? else {
                continue;
            };
            statuses.push(documents_credential_from_office(account, credential)?.status());
        }
        statuses.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.account_key.cmp(&right.account_key))
        });
        Ok(statuses)
    }
}

pub(crate) fn documents_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> Result<DocumentsProviderCredential> {
    let username = credential
        .metadata_value(OFFICE_METADATA_DOCUMENTS_USERNAME)
        .unwrap_or(account.external_account_id.as_str())
        .trim()
        .to_string();
    let base_url = credential
        .metadata_value(OFFICE_METADATA_DOCUMENTS_BASE_URL)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();
    let root_path = credential
        .metadata_value(OFFICE_METADATA_DOCUMENTS_ROOT_PATH)
        .unwrap_or_default()
        .trim()
        .to_string();
    if base_url.is_empty() {
        return Err(Error::config(
            "documents_provider_credential",
            "documents_base_url must not be empty",
        ));
    }
    Ok(DocumentsProviderCredential {
        account_key: credential.account_key,
        provider: account.provider_kind,
        account_id: account.external_account_id,
        account_label: account.account_label,
        username,
        secret: credential.access_token,
        base_url,
        root_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy,
    };
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, _account_key: &str) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(None)
        }
        fn list(&self) -> Result<Vec<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }
        fn set(&self, _status: &crate::office::OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    fn build_office() -> (OfficeService, std::sync::Arc<StubCredentialStore>) {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-work".to_string(),
            provider_kind: "webdav".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let credential_store = std::sync::Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "app-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    (
                        OFFICE_METADATA_DOCUMENTS_USERNAME.to_string(),
                        "user@example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                        "https://dav.example.com/remote.php/dav/files/user".to_string(),
                    ),
                    (
                        OFFICE_METADATA_DOCUMENTS_ROOT_PATH.to_string(),
                        "/Workspace".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            credential_store.clone(),
            std::sync::Arc::new(StubRuntimeStatusStore),
        );
        (office, credential_store)
    }

    #[test]
    fn office_backed_store_adapts_documents_metadata() {
        let (office, _) = build_office();
        let store = OfficeBackedDocumentsProviderCredentialStore::new(office);
        let credential = store
            .get("docs-work")
            .expect("store get")
            .expect("credential present");
        assert_eq!(credential.provider, "webdav");
        assert_eq!(credential.username, "user@example.com");
        assert_eq!(credential.root_path, "/Workspace");
    }
}
