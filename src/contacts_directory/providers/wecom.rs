#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::contacts_directory::credentials::contacts_credential_from_office;
use crate::contacts_directory::{
    normalize_contact_entry, ContactEntry, ContactsDirectoryProvider,
    ContactsDirectoryProviderCredential,
};
use crate::error::{Error, Result};
use crate::office::{
    fetch_wecom_access_token, request_wecom_json, OfficeAccount, OfficeHttpClient,
    OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult, WecomApiEnvelope,
    WecomAuthCredential,
};
use serde::Deserialize;

const WECOM_ROOT_DEPARTMENT_ID: u64 = 1;

pub struct WecomContactsDirectoryProvider;

impl ContactsDirectoryProvider for WecomContactsDirectoryProvider {
    fn provider_name(&self) -> &'static str {
        "wecom_contacts_directory"
    }

    fn display_name(&self) -> &'static str {
        "WeCom Contacts Directory"
    }

    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        validate_wecom_credential(credential)?;
        let client = WecomContactsClient::new(http, credential)?;
        client.lookup_contacts(http, query, limit)
    }
}

pub struct WecomContactsDirectoryOfficeProbeAdapter;

impl OfficeProbeAdapter for WecomContactsDirectoryOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "wecom_contacts_directory"
    }

    fn probe(
        &self,
        http: &mut dyn OfficeHttpClient,
        account: &OfficeAccount,
        credential: &crate::office::OfficeCredential,
    ) -> Result<OfficeProbeResult> {
        let adapted = match contacts_credential_from_office(account.clone(), credential.clone()) {
            Ok(adapted) => adapted,
            Err(_) => {
                return Ok(OfficeProbeResult {
                    account_key: account.account_key.clone(),
                    provider_kind: account.provider_kind.clone(),
                    configured: false,
                    disposition: OfficeProbeDisposition::MissingCredential,
                    reason: "contacts_transport_config_missing".to_string(),
                })
            }
        };
        if validate_wecom_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "contacts_transport_config_missing".to_string(),
            });
        }
        let client = WecomContactsClient::new(http, &adapted)?;
        client.list_users(http)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "wecom_contacts_users_ok".to_string(),
        })
    }
}

struct WecomContactsClient {
    base_url: String,
    access_token: String,
}

impl WecomContactsClient {
    fn new(
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
    ) -> Result<Self> {
        let access_token = fetch_wecom_access_token(
            http,
            "wecom_contacts_auth",
            WecomAuthCredential {
                corp_id: credential.app_id.as_str(),
                corp_secret: credential.secret.as_str(),
                base_url: credential.base_url.as_str(),
            },
        )?;
        Ok(Self {
            base_url: credential.base_url.trim_end_matches('/').to_string(),
            access_token,
        })
    }

    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        let query = query.trim().to_ascii_lowercase();
        let limit = limit.clamp(1, 50);
        let mut out = Vec::new();
        for user in self.list_users(http)? {
            let contact = user.to_contact_entry()?;
            if query.is_empty() || matches_query(&contact, &query) {
                out.push(contact);
            }
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    fn list_users(&self, http: &mut dyn OfficeHttpClient) -> Result<Vec<WecomUser>> {
        let url = format!(
            "{}/cgi-bin/user/list?access_token={}&department_id={}&fetch_child=1",
            self.base_url,
            urlencoding::encode(&self.access_token),
            WECOM_ROOT_DEPARTMENT_ID
        );
        let payload: WecomApiEnvelope<WecomUsersPayload> =
            request_wecom_json(http, "wecom_contacts_lookup", "GET", &url, &[], None)?;
        Ok(payload.require_ok("wecom_contacts_lookup")?.userlist)
    }
}

#[derive(Debug, Default, Deserialize)]
struct WecomUsersPayload {
    #[serde(default)]
    userlist: Vec<WecomUser>,
}

#[derive(Debug, Default, Deserialize)]
struct WecomUser {
    #[serde(default)]
    userid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    biz_mail: String,
    #[serde(default)]
    position: String,
}

impl WecomUser {
    fn to_contact_entry(&self) -> Result<ContactEntry> {
        let mut aliases = Vec::new();
        if !self.alias.trim().is_empty() {
            aliases.push(self.alias.trim().to_string());
        }
        if !self.userid.trim().is_empty() {
            aliases.push(self.userid.trim().to_string());
        }
        let mut emails = Vec::new();
        for email in [&self.biz_mail, &self.email] {
            let normalized = email.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !emails.iter().any(|item| item == &normalized) {
                emails.push(normalized);
            }
        }
        normalize_contact_entry(ContactEntry {
            id: self.userid.trim().to_string(),
            display_name: self.name.trim().to_string(),
            emails,
            aliases,
            organization: String::new(),
            notes: self.position.trim().to_string(),
            updated_at_unix_secs: crate::util::current_unix_secs(),
        })
    }
}

fn validate_wecom_credential(credential: &ContactsDirectoryProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "wecom_contacts_provider",
            "contacts_corp_id must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "wecom_contacts_provider",
            "secret must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "wecom_contacts_provider",
            "contacts_base_url must not be empty",
        ));
    }
    Ok(())
}

fn matches_query(contact: &ContactEntry, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    if contact.display_name.to_ascii_lowercase().contains(query) {
        return true;
    }
    if contact
        .emails
        .iter()
        .any(|email| email.to_ascii_lowercase().contains(query))
    {
        return true;
    }
    contact
        .aliases
        .iter()
        .any(|alias| alias.to_ascii_lowercase().contains(query))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contacts_directory::ContactsDirectoryOperation;

    #[test]
    fn wecom_contacts_provider_reports_lookup_support() {
        let provider = WecomContactsDirectoryProvider;
        assert_eq!(provider.provider_name(), "wecom_contacts_directory");
        assert_eq!(provider.display_name(), "WeCom Contacts Directory");
        assert!(provider.supports(ContactsDirectoryOperation::Lookup));
    }

    #[test]
    fn wecom_user_maps_to_contact_entry() {
        let entry = WecomUser {
            userid: "zhangsan".to_string(),
            name: "张三".to_string(),
            alias: "zs".to_string(),
            email: "zhangsan@example.com".to_string(),
            biz_mail: "zhangsan@corp.example.com".to_string(),
            position: "销售经理".to_string(),
        }
        .to_contact_entry()
        .expect("contact entry");
        assert_eq!(entry.id, "zhangsan");
        assert_eq!(entry.display_name, "张三");
        assert_eq!(entry.emails[0], "zhangsan@corp.example.com");
        assert!(entry.aliases.iter().any(|item| item == "zs"));
    }

    #[test]
    fn wecom_contacts_provider_validates_required_fields() {
        let error = validate_wecom_credential(&ContactsDirectoryProviderCredential {
            account_key: "contacts-wecom".to_string(),
            provider: "wecom_contacts_directory".to_string(),
            account_id: String::new(),
            account_label: "WeCom Contacts".to_string(),
            app_id: String::new(),
            base_url: crate::office::WECOM_DEFAULT_BASE_URL.to_string(),
            secret: "corp-secret".to_string(),
        })
        .expect_err("missing corp id");
        assert_eq!(error.stage(), "wecom_contacts_provider");
    }
}
