use crate::error::{Error, Result};
use crate::office::{
    OfficeAuthoritySource, OfficeCapability, OfficeCredential, OfficeService,
    SnapshotOfficeAuthoritySource,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const OFFICE_METADATA_DOCUMENTS_USERNAME: &str = "documents_username";
pub const OFFICE_METADATA_DOCUMENTS_BASE_URL: &str = "documents_base_url";
pub const OFFICE_METADATA_DOCUMENTS_ROOT_PATH: &str = "documents_root_path";
pub const OFFICE_METADATA_DOCUMENTS_APP_ID: &str = "documents_app_id";
pub const OFFICE_METADATA_DOCUMENTS_CORP_ID: &str = "documents_corp_id";
pub const OFFICE_METADATA_DOCUMENTS_SPACE_ID: &str = "documents_space_id";
pub const FEISHU_DOCUMENTS_DEFAULT_BASE_URL: &str = "https://open.feishu.cn";

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
    pub app_id: String,
    #[serde(default)]
    pub space_id: String,
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
        let configured = match self.provider.as_str() {
            "feishu_documents" => {
                !self.app_id.trim().is_empty()
                    && !self.secret.trim().is_empty()
                    && !self.root_path.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
            "wecom_documents" => {
                !self.app_id.trim().is_empty()
                    && !self.secret.trim().is_empty()
                    && !self.space_id.trim().is_empty()
                    && !self.root_path.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
            _ => {
                !self.username.trim().is_empty()
                    && !self.secret.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
        };
        DocumentsProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            configured,
            root_path: self.root_path.clone(),
            base_url: self.base_url.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedDocumentsProviderCredentialStore {
    authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
}

impl OfficeBackedDocumentsProviderCredentialStore {
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

impl DocumentsProviderCredentialStore for OfficeBackedDocumentsProviderCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<DocumentsProviderCredential>> {
        let office = self.load_office()?;
        let Some(account) = office.account(account_key) else {
            return Ok(None);
        };
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::Documents)
        {
            return Ok(None);
        }
        let Some(credential) = office.credential(account_key)? else {
            return Ok(None);
        };
        Ok(Some(documents_credential_from_office(account, credential)?))
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        let office = self.load_office()?;
        let mut keys = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::Documents) {
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

    fn list_statuses(&self) -> Result<Vec<DocumentsProviderCredentialStatus>> {
        let office = self.load_office()?;
        let mut statuses = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::Documents) {
            let Some(credential) = office.credential(&account.account_key)? else {
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
    if account.provider_kind == "feishu_documents" {
        let app_id = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_APP_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let base_url = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_BASE_URL)
            .unwrap_or(FEISHU_DOCUMENTS_DEFAULT_BASE_URL)
            .trim()
            .trim_end_matches('/')
            .to_string();
        let root_path = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_ROOT_PATH)
            .unwrap_or_default()
            .trim()
            .to_string();
        if app_id.is_empty() {
            return Err(Error::config(
                "documents_provider_credential",
                "documents_app_id must not be empty",
            ));
        }
        if root_path.is_empty() {
            return Err(Error::config(
                "documents_provider_credential",
                "documents_root_path must not be empty",
            ));
        }
        return Ok(DocumentsProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            username: String::new(),
            secret: credential.access_token,
            app_id,
            space_id: String::new(),
            base_url,
            root_path,
        });
    }

    if account.provider_kind == "wecom_documents" {
        let app_id = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_CORP_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let space_id = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_SPACE_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let base_url = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_BASE_URL)
            .unwrap_or(crate::office::WECOM_DEFAULT_BASE_URL)
            .trim()
            .trim_end_matches('/')
            .to_string();
        let root_path = credential
            .metadata_value(OFFICE_METADATA_DOCUMENTS_ROOT_PATH)
            .unwrap_or_default()
            .trim()
            .to_string();
        if app_id.is_empty() {
            return Err(Error::config(
                "documents_provider_credential",
                "documents_corp_id must not be empty",
            ));
        }
        if space_id.is_empty() {
            return Err(Error::config(
                "documents_provider_credential",
                "documents_space_id must not be empty",
            ));
        }
        if root_path.is_empty() {
            return Err(Error::config(
                "documents_provider_credential",
                "documents_root_path must not be empty",
            ));
        }
        return Ok(DocumentsProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            username: String::new(),
            secret: credential.access_token,
            app_id,
            space_id,
            base_url,
            root_path,
        });
    }

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
        app_id: String::new(),
        space_id: String::new(),
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
        fn get(
            &self,
            _account_key: &str,
        ) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
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

    #[test]
    fn office_backed_store_adapts_feishu_documents_metadata() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-feishu".to_string(),
            provider_kind: "feishu_documents".to_string(),
            external_account_id: "tenant-docs".to_string(),
            account_label: "Feishu Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let credential_store = std::sync::Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-feishu".to_string(),
                access_token: "app-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    (
                        OFFICE_METADATA_DOCUMENTS_APP_ID.to_string(),
                        "cli_a1b2c3".to_string(),
                    ),
                    (
                        OFFICE_METADATA_DOCUMENTS_ROOT_PATH.to_string(),
                        "fldcn-root".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed feishu office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            credential_store,
            std::sync::Arc::new(StubRuntimeStatusStore),
        );
        let store = OfficeBackedDocumentsProviderCredentialStore::new(office);
        let credential = store
            .get("docs-feishu")
            .expect("store get")
            .expect("credential present");
        assert_eq!(credential.provider, "feishu_documents");
        assert_eq!(credential.app_id, "cli_a1b2c3");
        assert_eq!(credential.root_path, "fldcn-root");
        assert_eq!(credential.base_url, "https://open.feishu.cn");
    }

    #[test]
    fn office_backed_store_adapts_wecom_documents_metadata() {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "docs-wecom".to_string(),
            provider_kind: "wecom_documents".to_string(),
            external_account_id: String::new(),
            account_label: "WeCom Docs".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Documents],
        });
        let credential_store = std::sync::Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-wecom".to_string(),
                access_token: "corp-secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [
                    ("documents_corp_id".to_string(), "wwcorp123".to_string()),
                    ("documents_space_id".to_string(), "space-1".to_string()),
                    (
                        OFFICE_METADATA_DOCUMENTS_ROOT_PATH.to_string(),
                        "folder-root".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed wecom office credential");
        let office = OfficeService::new(
            registry,
            OfficeCapabilityBinding::default(),
            OfficeSelectionPolicy::default(),
            credential_store,
            std::sync::Arc::new(StubRuntimeStatusStore),
        );
        let store = OfficeBackedDocumentsProviderCredentialStore::new(office);
        let credential = store
            .get("docs-wecom")
            .expect("store get")
            .expect("credential present");
        assert_eq!(credential.provider, "wecom_documents");
        assert_eq!(credential.app_id, "wwcorp123");
        assert_eq!(credential.root_path, "folder-root");
        assert_eq!(credential.base_url, crate::office::WECOM_DEFAULT_BASE_URL);
        assert_eq!(
            serde_json::to_value(&credential).expect("serialize")["space_id"],
            "space-1"
        );
    }
}
