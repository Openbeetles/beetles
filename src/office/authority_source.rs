use crate::config::{self, ConfigFileStore};
use crate::error::{Error, Result};
use crate::office::{OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeService};
use std::sync::Arc;

pub trait OfficeAuthoritySource: Send + Sync {
    fn load(&self) -> Result<OfficeService>;
}

#[derive(Clone)]
pub struct SnapshotOfficeAuthoritySource {
    office: OfficeService,
}

impl SnapshotOfficeAuthoritySource {
    pub fn new(office: OfficeService) -> Self {
        Self { office }
    }
}

impl OfficeAuthoritySource for SnapshotOfficeAuthoritySource {
    fn load(&self) -> Result<OfficeService> {
        Ok(self.office.clone())
    }
}

#[derive(Clone)]
pub struct ReloadingOfficeAuthoritySource {
    config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
    credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
    runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
}

impl ReloadingOfficeAuthoritySource {
    pub fn new(
        config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
        credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
        runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
    ) -> Self {
        Self {
            config_file_store,
            credential_store,
            runtime_status_store,
        }
    }
}

impl OfficeAuthoritySource for ReloadingOfficeAuthoritySource {
    fn load(&self) -> Result<OfficeService> {
        let json = config::get_office_accounts_segment(self.config_file_store.as_ref())?;
        let segment: crate::config::OfficeAccountsSegment = serde_json::from_str(&json)
            .map_err(|error| Error::config("office_authority_load_accounts", error.to_string()))?;
        Ok(OfficeService::new(
            segment.registry,
            segment.binding,
            segment.policy,
            Arc::clone(&self.credential_store),
            Arc::clone(&self.runtime_status_store),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        OfficeAuthoritySource, ReloadingOfficeAuthoritySource, SnapshotOfficeAuthoritySource,
    };
    use crate::config::{save_office_accounts_segment, ConfigFileStore};
    use crate::error::Result;
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, OfficeService,
    };
    use std::collections::{BTreeMap, HashMap};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryConfigFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl ConfigFileStore for MemoryConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
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

    fn office_with_default(default_account_key: &str) -> OfficeService {
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        registry.insert(OfficeAccount {
            account_key: "mail-personal".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Mail, default_account_key.to_string());
        OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore),
        )
    }

    fn save_mail_accounts(
        config_file_store: &dyn ConfigFileStore,
        default_account_key: &str,
    ) -> Result<()> {
        save_office_accounts_segment(
            config_file_store,
            &format!(
                r#"{{
                    "registry": {{
                        "accounts": {{
                            "mail-work": {{
                                "account_key": "mail-work",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "work@example.com",
                                "account_label": "Work",
                                "identity_class": "work",
                                "enabled_capabilities": ["mail"]
                            }},
                            "mail-personal": {{
                                "account_key": "mail-personal",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "personal@example.com",
                                "account_label": "Personal",
                                "identity_class": "personal",
                                "enabled_capabilities": ["mail"]
                            }}
                        }}
                    }},
                    "binding": {{
                        "capability_defaults": {{
                            "mail": "{default_account_key}"
                        }}
                    }},
                    "policy": {{}}
                }}"#
            ),
        )
    }

    #[test]
    fn snapshot_source_keeps_original_authority_snapshot() {
        let source = SnapshotOfficeAuthoritySource::new(office_with_default("mail-work"));
        let office = source.load().expect("load authority");
        assert_eq!(
            office.default_account_key(OfficeCapability::Mail),
            Some("mail-work".to_string())
        );
    }

    #[test]
    fn reloading_source_reads_latest_accounts_segment() {
        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        save_mail_accounts(config_file_store.as_ref(), "mail-work").expect("seed accounts");
        let source = ReloadingOfficeAuthoritySource::new(
            config_file_store.clone(),
            Arc::new(StubCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore),
        );

        let first = source.load().expect("load first authority");
        assert_eq!(
            first.default_account_key(OfficeCapability::Mail),
            Some("mail-work".to_string())
        );

        save_mail_accounts(config_file_store.as_ref(), "mail-personal").expect("update accounts");

        let second = source.load().expect("load second authority");
        assert_eq!(
            second.default_account_key(OfficeCapability::Mail),
            Some("mail-personal".to_string())
        );
    }
}
