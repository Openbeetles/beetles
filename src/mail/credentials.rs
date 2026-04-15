use crate::error::{Error, Result};
use crate::mail::{DEFAULT_DRAFT_MAILBOX, DEFAULT_MAILBOX};
use crate::office::{
    OfficeAuthoritySource, OfficeCapability, OfficeCredential, OfficeService,
    SnapshotOfficeAuthoritySource,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const OFFICE_METADATA_MAIL_USERNAME: &str = "mail_username";
pub const OFFICE_METADATA_MAIL_IMAP_HOST: &str = "mail_imap_host";
pub const OFFICE_METADATA_MAIL_IMAP_PORT: &str = "mail_imap_port";
pub const OFFICE_METADATA_MAIL_IMAP_MAILBOX: &str = "mail_imap_mailbox";
pub const OFFICE_METADATA_MAIL_DRAFT_MAILBOX: &str = "mail_draft_mailbox";
pub const OFFICE_METADATA_MAIL_IMAP_TLS: &str = "mail_imap_tls";
pub const OFFICE_METADATA_MAIL_SMTP_HOST: &str = "mail_smtp_host";
pub const OFFICE_METADATA_MAIL_SMTP_PORT: &str = "mail_smtp_port";
pub const OFFICE_METADATA_MAIL_SMTP_TLS: &str = "mail_smtp_tls";
pub const OFFICE_METADATA_MAIL_FROM_ADDRESS: &str = "mail_from_address";
pub const OFFICE_METADATA_MAIL_FROM_NAME: &str = "mail_from_name";
pub const OFFICE_METADATA_MAIL_CORP_ID: &str = "mail_corp_id";
pub const OFFICE_METADATA_MAIL_BASE_URL: &str = "mail_base_url";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailProviderCredential {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub corp_id: String,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub imap_host: String,
    #[serde(default)]
    pub imap_port: u16,
    #[serde(default)]
    pub imap_mailbox: String,
    #[serde(default)]
    pub draft_mailbox: String,
    #[serde(default)]
    pub imap_tls: bool,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default)]
    pub smtp_port: u16,
    #[serde(default)]
    pub smtp_tls: bool,
    #[serde(default)]
    pub from_address: String,
    #[serde(default)]
    pub from_name: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailProviderCredentialStatus {
    pub account_key: String,
    pub provider: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub mailbox: String,
    #[serde(default)]
    pub from_address: String,
}

pub trait MailProviderCredentialStore: Send + Sync {
    fn get(&self, account_key: &str) -> Result<Option<MailProviderCredential>>;
    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>>;
    fn list_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>>;
}

impl MailProviderCredential {
    pub fn status(&self) -> MailProviderCredentialStatus {
        let configured = match self.provider.as_str() {
            "wecom_mail" => {
                !self.secret.trim().is_empty()
                    && !self.corp_id.trim().is_empty()
                    && !self.base_url.trim().is_empty()
            }
            _ => {
                !self.secret.trim().is_empty()
                    && !self.username.trim().is_empty()
                    && !self.imap_host.trim().is_empty()
                    && !self.smtp_host.trim().is_empty()
            }
        };
        MailProviderCredentialStatus {
            account_key: self.account_key.clone(),
            provider: self.provider.clone(),
            account_id: self.account_id.clone(),
            account_label: self.account_label.clone(),
            configured,
            mailbox: self.imap_mailbox.clone(),
            from_address: self.from_address.clone(),
        }
    }
}

#[derive(Clone)]
pub struct OfficeBackedMailProviderCredentialStore {
    authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
}

impl OfficeBackedMailProviderCredentialStore {
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

impl MailProviderCredentialStore for OfficeBackedMailProviderCredentialStore {
    fn get(&self, account_key: &str) -> Result<Option<MailProviderCredential>> {
        let office = self.load_office()?;
        let Some(account) = office.account(account_key) else {
            return Ok(None);
        };
        if !account
            .enabled_capabilities
            .contains(&OfficeCapability::Mail)
        {
            return Ok(None);
        }
        let Some(credential) = office.credential(account_key)? else {
            return Ok(None);
        };
        Ok(Some(mail_credential_from_office(account, credential)?))
    }

    fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
        let office = self.load_office()?;
        let mut keys = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::Mail) {
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

    fn list_statuses(&self) -> Result<Vec<MailProviderCredentialStatus>> {
        let office = self.load_office()?;
        let mut statuses = Vec::new();
        for account in office.accounts_for_capability(OfficeCapability::Mail) {
            let Some(credential) = office.credential(&account.account_key)? else {
                continue;
            };
            statuses.push(mail_credential_from_office(account, credential)?.status());
        }
        statuses.sort_by(|left, right| {
            left.provider
                .cmp(&right.provider)
                .then_with(|| left.account_key.cmp(&right.account_key))
        });
        Ok(statuses)
    }
}

pub(crate) fn mail_credential_from_office(
    account: crate::office::OfficeAccount,
    credential: OfficeCredential,
) -> Result<MailProviderCredential> {
    if account.provider_kind == "wecom_mail" {
        let corp_id = credential
            .metadata_value(OFFICE_METADATA_MAIL_CORP_ID)
            .unwrap_or_default()
            .trim()
            .to_string();
        let base_url = credential
            .metadata_value(OFFICE_METADATA_MAIL_BASE_URL)
            .unwrap_or(crate::office::WECOM_DEFAULT_BASE_URL)
            .trim()
            .trim_end_matches('/')
            .to_string();
        if corp_id.is_empty() {
            return Err(Error::config(
                "mail_provider_credential",
                "mail_corp_id must not be empty",
            ));
        }
        let from_address = credential
            .metadata_value(OFFICE_METADATA_MAIL_FROM_ADDRESS)
            .unwrap_or(account.external_account_id.as_str())
            .trim()
            .to_string();
        let from_name = credential
            .metadata_value(OFFICE_METADATA_MAIL_FROM_NAME)
            .unwrap_or(account.account_label.as_str())
            .trim()
            .to_string();
        return Ok(MailProviderCredential {
            account_key: credential.account_key,
            provider: account.provider_kind,
            account_id: account.external_account_id,
            account_label: account.account_label,
            username: String::new(),
            corp_id,
            secret: credential.access_token,
            base_url,
            imap_host: String::new(),
            imap_port: 0,
            imap_mailbox: DEFAULT_MAILBOX.to_string(),
            draft_mailbox: DEFAULT_DRAFT_MAILBOX.to_string(),
            imap_tls: false,
            smtp_host: String::new(),
            smtp_port: 0,
            smtp_tls: false,
            from_address,
            from_name,
        });
    }

    let imap_port = parse_mail_port(
        credential.metadata_value(OFFICE_METADATA_MAIL_IMAP_PORT),
        993,
        "mail_provider_credential",
    )?;
    let imap_mailbox =
        mailbox_or_default(credential.metadata_value(OFFICE_METADATA_MAIL_IMAP_MAILBOX));
    let draft_mailbox = mailbox_or_fallback(
        credential.metadata_value(OFFICE_METADATA_MAIL_DRAFT_MAILBOX),
        DEFAULT_DRAFT_MAILBOX,
    );
    let imap_tls = parse_mail_bool(
        credential.metadata_value(OFFICE_METADATA_MAIL_IMAP_TLS),
        true,
    );
    let smtp_port = parse_mail_port(
        credential.metadata_value(OFFICE_METADATA_MAIL_SMTP_PORT),
        465,
        "mail_provider_credential",
    )?;
    let smtp_tls = parse_mail_bool(
        credential.metadata_value(OFFICE_METADATA_MAIL_SMTP_TLS),
        true,
    );
    let username = credential
        .metadata_value(OFFICE_METADATA_MAIL_USERNAME)
        .unwrap_or(account.external_account_id.as_str())
        .trim()
        .to_string();
    let imap_host = credential
        .metadata_value(OFFICE_METADATA_MAIL_IMAP_HOST)
        .unwrap_or_default()
        .trim()
        .to_string();
    let smtp_host = credential
        .metadata_value(OFFICE_METADATA_MAIL_SMTP_HOST)
        .unwrap_or_default()
        .trim()
        .to_string();
    let from_address = credential
        .metadata_value(OFFICE_METADATA_MAIL_FROM_ADDRESS)
        .unwrap_or(username.as_str())
        .trim()
        .to_string();
    let from_name = credential
        .metadata_value(OFFICE_METADATA_MAIL_FROM_NAME)
        .unwrap_or(account.account_label.as_str())
        .trim()
        .to_string();
    Ok(MailProviderCredential {
        account_key: credential.account_key,
        provider: account.provider_kind,
        account_id: account.external_account_id,
        account_label: account.account_label,
        username,
        corp_id: String::new(),
        secret: credential.access_token,
        base_url: String::new(),
        imap_host,
        imap_port,
        imap_mailbox,
        draft_mailbox,
        imap_tls,
        smtp_host,
        smtp_port,
        smtp_tls,
        from_address,
        from_name,
    })
}

fn parse_mail_port(raw: Option<&str>, default_port: u16, stage: &'static str) -> Result<u16> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(raw) => raw
            .parse::<u16>()
            .map_err(|error| Error::config(stage, error.to_string())),
        None => Ok(default_port),
    }
}

fn parse_mail_bool(raw: Option<&str>, default_value: bool) -> bool {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(raw) if raw.eq_ignore_ascii_case("true") || raw == "1" => true,
        Some(raw) if raw.eq_ignore_ascii_case("false") || raw == "0" => false,
        Some(_) => default_value,
        None => default_value,
    }
}

fn mailbox_or_default(value: Option<&str>) -> String {
    mailbox_or_fallback(value, DEFAULT_MAILBOX)
}

fn mailbox_or_fallback(value: Option<&str>, default_mailbox: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_mailbox)
        .to_string()
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
    use std::collections::{BTreeMap, HashMap};
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
            account_key: "mail-work".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        registry.insert(OfficeAccount {
            account_key: "calendar-work".to_string(),
            provider_kind: "caldav".to_string(),
            external_account_id: "calendar@example.com".to_string(),
            account_label: "Calendar".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let credential_store = std::sync::Arc::new(StubCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 10,
                metadata: HashMap::from([
                    (
                        OFFICE_METADATA_MAIL_USERNAME.to_string(),
                        "work@example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_IMAP_PORT.to_string(),
                        "993".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_IMAP_MAILBOX.to_string(),
                        "INBOX".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_SMTP_PORT.to_string(),
                        "465".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_FROM_ADDRESS.to_string(),
                        "work@example.com".to_string(),
                    ),
                ])
                .into_iter()
                .collect(),
            })
            .expect("seed");
        (
            OfficeService::new(
                registry,
                OfficeCapabilityBinding::default(),
                OfficeSelectionPolicy::default(),
                credential_store.clone(),
                std::sync::Arc::new(StubRuntimeStatusStore),
            ),
            credential_store,
        )
    }

    #[test]
    fn office_backed_store_adapts_mail_metadata() {
        let (office, _) = build_office();
        let store = OfficeBackedMailProviderCredentialStore::new(office);

        let credential = store
            .get("mail-work")
            .expect("lookup")
            .expect("mail credential");

        assert_eq!(credential.provider, "imap_smtp");
        assert_eq!(credential.username, "work@example.com");
        assert_eq!(credential.imap_host, "imap.example.com");
        assert_eq!(credential.smtp_host, "smtp.example.com");
        assert_eq!(credential.from_address, "work@example.com");
    }

    #[test]
    fn office_backed_store_hides_non_mail_accounts() {
        let (office, _) = build_office();
        let store = OfficeBackedMailProviderCredentialStore::new(office);

        assert!(store.get("calendar-work").expect("lookup").is_none());
    }
}
