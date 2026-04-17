#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::contacts_directory::credentials::contacts_credential_from_office;
use crate::contacts_directory::{
    normalize_contact_entry, ContactEntry, ContactsDirectoryProvider,
    ContactsDirectoryProviderCredential,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeAccount, OfficeHttpClient, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const FEISHU_CONTACTS_PAGE_SIZE: usize = 50;
const FEISHU_CONTACTS_MAX_PAGES: usize = 4;

pub struct FeishuContactsDirectoryProvider;

impl ContactsDirectoryProvider for FeishuContactsDirectoryProvider {
    fn provider_name(&self) -> &'static str {
        "feishu_contacts_directory"
    }

    fn display_name(&self) -> &'static str {
        "Feishu Contacts Directory"
    }

    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        validate_feishu_credential(credential)?;
        let client = FeishuContactsClient::new(http, credential)?;
        client.lookup_contacts(http, query, limit)
    }
}

pub struct FeishuContactsDirectoryOfficeProbeAdapter;

impl OfficeProbeAdapter for FeishuContactsDirectoryOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "feishu_contacts_directory"
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
        if validate_feishu_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "contacts_transport_config_missing".to_string(),
            });
        }
        let client = FeishuContactsClient::new(http, &adapted)?;
        client.list_users_page(http, None)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "feishu_contacts_users_ok".to_string(),
        })
    }
}

struct FeishuContactsClient {
    base_url: String,
    tenant_access_token: String,
}

impl FeishuContactsClient {
    fn new(
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
    ) -> Result<Self> {
        let tenant_access_token = fetch_tenant_access_token(http, credential)?;
        Ok(Self {
            base_url: credential.base_url.trim_end_matches('/').to_string(),
            tenant_access_token,
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
        let mut page_token = None::<String>;
        for _ in 0..FEISHU_CONTACTS_MAX_PAGES {
            let page = self.list_users_page(http, page_token.as_deref())?;
            for user in page.items {
                if let Some(contact) = user.to_contact_entry()? {
                    if query.is_empty() || contact_matches_query(&contact, &query) {
                        out.push(contact);
                    }
                }
                if out.len() >= limit {
                    out.truncate(limit);
                    return Ok(out);
                }
            }
            if !page.has_more || page.page_token.trim().is_empty() {
                break;
            }
            page_token = Some(page.page_token);
        }
        Ok(out)
    }

    fn list_users_page(
        &self,
        http: &mut dyn OfficeHttpClient,
        page_token: Option<&str>,
    ) -> Result<FeishuUsersPage> {
        let mut url = format!(
            "{}/open-apis/contact/v3/users?page_size={}&user_id_type=open_id",
            self.base_url, FEISHU_CONTACTS_PAGE_SIZE
        );
        if let Some(page_token) = page_token.filter(|value| !value.trim().is_empty()) {
            url.push_str("&page_token=");
            url.push_str(page_token);
        }
        let auth = format!("Bearer {}", self.tenant_access_token);
        let payload: FeishuEnvelope<FeishuUsersData> = request_feishu_json(
            http,
            "feishu_contacts_lookup",
            "GET",
            &url,
            &[("Authorization", auth.as_str())],
            None,
        )?;
        let data = payload.require_data("feishu_contacts_lookup")?;
        Ok(FeishuUsersPage {
            has_more: data.has_more,
            page_token: data.page_token.unwrap_or_default(),
            items: data.items,
        })
    }
}

struct FeishuUsersPage {
    has_more: bool,
    page_token: String,
    items: Vec<FeishuUser>,
}

#[derive(Debug, Deserialize)]
struct FeishuEnvelope<T> {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<T>,
    #[serde(default)]
    tenant_access_token: Option<String>,
}

impl<T> FeishuEnvelope<T> {
    fn require_ok(&self, stage: &'static str) -> Result<()> {
        if self.code != 0 {
            return Err(Error::config(
                stage,
                format!("feishu returned code {}: {}", self.code, self.msg),
            ));
        }
        Ok(())
    }

    fn require_data(self, stage: &'static str) -> Result<T> {
        self.require_ok(stage)?;
        self.data
            .ok_or_else(|| Error::config(stage, "missing response data"))
    }
}

#[derive(Debug, Default, Deserialize)]
struct FeishuUsersData {
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    page_token: Option<String>,
    #[serde(default)]
    items: Vec<FeishuUser>,
}

#[derive(Debug, Deserialize)]
struct FeishuUser {
    #[serde(default)]
    open_id: String,
    #[serde(default)]
    user_id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    en_name: String,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    enterprise_email: String,
    #[serde(default)]
    job_title: String,
}

impl FeishuUser {
    fn to_contact_entry(&self) -> Result<Option<ContactEntry>> {
        let id = if !self.open_id.trim().is_empty() {
            self.open_id.trim().to_string()
        } else {
            self.user_id.trim().to_string()
        };
        if id.is_empty() {
            return Ok(None);
        }
        let mut aliases = Vec::new();
        if !self.en_name.trim().is_empty() {
            aliases.push(self.en_name.trim().to_string());
        }
        if !self.nickname.trim().is_empty() {
            aliases.push(self.nickname.trim().to_string());
        }
        let mut emails = Vec::new();
        for email in [&self.enterprise_email, &self.email] {
            let normalized = email.trim().to_ascii_lowercase();
            if !normalized.is_empty() && !emails.iter().any(|item| item == &normalized) {
                emails.push(normalized);
            }
        }
        Ok(Some(normalize_contact_entry(ContactEntry {
            id,
            display_name: self.name.trim().to_string(),
            emails,
            aliases,
            organization: String::new(),
            notes: self.job_title.trim().to_string(),
            updated_at_unix_secs: 0,
        })?))
    }
}

#[derive(Debug, Serialize)]
struct FeishuTenantTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

fn validate_feishu_credential(credential: &ContactsDirectoryProviderCredential) -> Result<()> {
    if credential.app_id.trim().is_empty() {
        return Err(Error::config(
            "feishu_contacts_provider",
            "contacts_app_id must not be empty",
        ));
    }
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "feishu_contacts_provider",
            "access_token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "feishu_contacts_provider",
            "contacts_base_url must not be empty",
        ));
    }
    Ok(())
}

fn fetch_tenant_access_token(
    http: &mut dyn OfficeHttpClient,
    credential: &ContactsDirectoryProviderCredential,
) -> Result<String> {
    validate_feishu_credential(credential)?;
    let body = serde_json::to_vec(&FeishuTenantTokenRequest {
        app_id: credential.app_id.as_str(),
        app_secret: credential.secret.as_str(),
    })
    .map_err(|error| Error::config("feishu_contacts_auth", error.to_string()))?;
    let url = format!(
        "{}/open-apis/auth/v3/tenant_access_token/internal",
        credential.base_url.trim_end_matches('/')
    );
    let payload: FeishuEnvelope<serde_json::Value> = request_feishu_json(
        http,
        "feishu_contacts_auth",
        "POST",
        &url,
        &[("Content-Type", "application/json")],
        Some(body.as_slice()),
    )?;
    payload.require_ok("feishu_contacts_auth")?;
    payload
        .tenant_access_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::config("feishu_contacts_auth", "missing tenant_access_token"))
}

fn request_feishu_json<T: DeserializeOwned>(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<T> {
    let (status, body) = http.request_with_headers(method, url, headers, body)?;
    if !(200..300).contains(&status) {
        return Err(Error::config(
            stage,
            format!(
                "http {}: {}",
                status,
                String::from_utf8_lossy(body.as_slice()).trim()
            ),
        ));
    }
    serde_json::from_slice(body.as_slice()).map_err(|error| Error::config(stage, error.to_string()))
}

fn contact_matches_query(contact: &ContactEntry, query: &str) -> bool {
    normalized_contact_fields(contact)
        .into_iter()
        .any(|field| field.contains(query))
}

fn normalized_contact_fields(contact: &ContactEntry) -> Vec<String> {
    let mut fields = Vec::new();
    if !contact.display_name.trim().is_empty() {
        fields.push(contact.display_name.trim().to_ascii_lowercase());
    }
    for alias in &contact.aliases {
        if !alias.trim().is_empty() {
            fields.push(alias.trim().to_ascii_lowercase());
        }
    }
    for email in &contact.emails {
        if !email.trim().is_empty() {
            fields.push(email.trim().to_ascii_lowercase());
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> ContactsDirectoryProviderCredential {
        ContactsDirectoryProviderCredential {
            account_key: "contacts-feishu".to_string(),
            provider: "feishu_contacts_directory".to_string(),
            account_id: String::new(),
            account_label: "Feishu Contacts".to_string(),
            app_id: "cli_contacts".to_string(),
            base_url: "https://open.feishu.cn".to_string(),
            secret: "app-secret".to_string(),
        }
    }

    #[test]
    fn feishu_contacts_provider_reports_lookup_support() {
        let provider = FeishuContactsDirectoryProvider;
        assert!(provider.supports(crate::contacts_directory::ContactsDirectoryOperation::Lookup));
    }

    #[test]
    fn feishu_user_maps_to_contact_entry() {
        let user = FeishuUser {
            open_id: "ou_alice".to_string(),
            user_id: String::new(),
            name: "Alice Zhang".to_string(),
            en_name: "Alice".to_string(),
            nickname: "阿丽丝".to_string(),
            email: "alice@example.com".to_string(),
            enterprise_email: "alice@beetle.cn".to_string(),
            job_title: "PM".to_string(),
        };

        let contact = user
            .to_contact_entry()
            .expect("contact entry")
            .expect("mapped");
        assert_eq!(contact.id, "ou_alice");
        assert_eq!(contact.display_name, "Alice Zhang");
        assert_eq!(contact.emails[0], "alice@beetle.cn");
        assert!(contact.aliases.iter().any(|alias| alias == "Alice"));
    }

    #[test]
    fn feishu_contacts_provider_validates_required_fields() {
        let mut credential = credential();
        credential.app_id.clear();
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let error = FeishuContactsDirectoryProvider
            .lookup_contacts(&mut http, &credential, "alice", 5)
            .expect_err("missing app id must fail");
        assert_eq!(error.stage(), "feishu_contacts_provider");
    }
}
