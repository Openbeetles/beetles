#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::contacts_directory::credentials::contacts_credential_from_office;
use crate::contacts_directory::{
    normalize_contact_entry, ContactEntry, ContactsDirectoryProvider,
    ContactsDirectoryProviderCredential,
};
use crate::error::{Error, Result};
use crate::office::{
    build_microsoft_graph_url, request_microsoft_graph_json_ureq, OfficeAccount,
    OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use serde::Deserialize;

pub struct Microsoft365ContactsDirectoryProvider;

impl ContactsDirectoryProvider for Microsoft365ContactsDirectoryProvider {
    fn provider_name(&self) -> &'static str {
        "microsoft365_contacts_directory"
    }

    fn display_name(&self) -> &'static str {
        "Microsoft 365 People Directory"
    }

    fn lookup_contacts(
        &self,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        validate_microsoft365_credential(credential)?;
        let client = Microsoft365ContactsClient::new(credential)?;
        client.lookup_contacts(query, limit)
    }
}

pub struct Microsoft365ContactsDirectoryOfficeProbeAdapter;

impl OfficeProbeAdapter for Microsoft365ContactsDirectoryOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "microsoft365_contacts_directory"
    }

    fn probe(
        &self,
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
                });
            }
        };
        if validate_microsoft365_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "contacts_transport_config_missing".to_string(),
            });
        }
        let client = Microsoft365ContactsClient::new(&adapted)?;
        client.list_directory_users(Some(""), 1)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "microsoft365_contacts_users_ok".to_string(),
        })
    }
}

struct Microsoft365ContactsClient<'a> {
    credential: &'a ContactsDirectoryProviderCredential,
}

impl<'a> Microsoft365ContactsClient<'a> {
    fn new(credential: &'a ContactsDirectoryProviderCredential) -> Result<Self> {
        validate_microsoft365_credential(credential)?;
        Ok(Self { credential })
    }

    fn lookup_contacts(&self, query: &str, limit: usize) -> Result<Vec<ContactEntry>> {
        let limit = limit.clamp(1, 50);
        let users = self.list_directory_users(Some(query.trim()), limit)?;
        let normalized_query = query.trim().to_ascii_lowercase();
        let mut contacts = Vec::new();
        for user in users {
            if let Some(contact) = user.to_contact_entry()? {
                if normalized_query.is_empty()
                    || microsoft_contact_matches_query(&contact, &normalized_query)
                {
                    contacts.push(contact);
                }
            }
            if contacts.len() >= limit {
                break;
            }
        }
        Ok(contacts)
    }

    fn list_directory_users(
        &self,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MicrosoftGraphUser>> {
        let normalized_query = query
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let mut query_pairs = vec![
            ("$top", limit.clamp(1, 50).to_string()),
            (
                "$select",
                "id,displayName,mail,userPrincipalName,jobTitle,department,companyName".to_string(),
            ),
        ];
        if let Some(query) = normalized_query.as_deref() {
            query_pairs.push(("$search", format!("\"{}\"", render_directory_search(query))));
        }
        let url = build_microsoft_graph_url(&self.credential.base_url, "/users", &query_pairs);
        let mut request = ureq::get(&url).set("Authorization", &self.authorization_header());
        if normalized_query.is_some() {
            request = request.set("ConsistencyLevel", "eventual");
        }
        let payload: MicrosoftGraphUsersCollection =
            request_microsoft_graph_json_ureq("microsoft365_contacts_lookup", request.call())?;
        Ok(payload.value)
    }

    fn authorization_header(&self) -> String {
        format!("Bearer {}", self.credential.secret)
    }
}

type MicrosoftGraphUsersCollection = crate::office::MicrosoftGraphCollection<MicrosoftGraphUser>;

#[derive(Debug, Default, Deserialize)]
struct MicrosoftGraphUser {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "displayName")]
    display_name: String,
    #[serde(default)]
    mail: String,
    #[serde(default, rename = "userPrincipalName")]
    user_principal_name: String,
    #[serde(default, rename = "jobTitle")]
    job_title: String,
    #[serde(default)]
    department: String,
    #[serde(default, rename = "companyName")]
    company_name: String,
}

impl MicrosoftGraphUser {
    fn to_contact_entry(&self) -> Result<Option<ContactEntry>> {
        let id = self.id.trim();
        if id.is_empty() {
            return Ok(None);
        }
        let mut emails = Vec::new();
        for candidate in [&self.mail, &self.user_principal_name] {
            let email = candidate.trim().to_ascii_lowercase();
            if !email.is_empty() && !emails.iter().any(|existing| existing == &email) {
                emails.push(email);
            }
        }
        let mut aliases = Vec::new();
        let principal = self.user_principal_name.trim();
        if !principal.is_empty() {
            aliases.push(principal.to_string());
            if let Some((alias, _)) = principal.split_once('@') {
                let alias = alias.trim();
                if !alias.is_empty() && !aliases.iter().any(|existing| existing == alias) {
                    aliases.push(alias.to_string());
                }
            }
        }
        let organization = if !self.department.trim().is_empty() {
            self.department.trim().to_string()
        } else {
            self.company_name.trim().to_string()
        };
        Ok(Some(normalize_contact_entry(ContactEntry {
            id: id.to_string(),
            display_name: self.display_name.trim().to_string(),
            emails,
            aliases,
            organization,
            notes: self.job_title.trim().to_string(),
            updated_at_unix_secs: crate::util::current_unix_secs(),
        })?))
    }
}

fn validate_microsoft365_credential(
    credential: &ContactsDirectoryProviderCredential,
) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_contacts_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "microsoft365_contacts_provider",
            "contacts_base_url must not be empty",
        ));
    }
    Ok(())
}

fn render_directory_search(query: &str) -> String {
    let sanitized = query.replace(['"', '\''], " ").trim().to_string();
    if sanitized.is_empty() {
        "displayName:".to_string()
    } else {
        format!("displayName:{sanitized} OR mail:{sanitized} OR userPrincipalName:{sanitized}")
    }
}

fn microsoft_contact_matches_query(contact: &ContactEntry, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    if contact.display_name.to_ascii_lowercase().contains(query) {
        return true;
    }
    if contact.organization.to_ascii_lowercase().contains(query) {
        return true;
    }
    if contact.notes.to_ascii_lowercase().contains(query) {
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
    use crate::office::{OfficeAccountIdentityClass, OfficeCapability, OfficeCredential};

    #[test]
    fn microsoft365_contacts_provider_reports_lookup_support() {
        let provider = Microsoft365ContactsDirectoryProvider;
        assert_eq!(provider.provider_name(), "microsoft365_contacts_directory");
        assert_eq!(provider.display_name(), "Microsoft 365 People Directory");
        assert!(provider.supports(ContactsDirectoryOperation::Lookup));
    }

    #[test]
    fn microsoft365_contacts_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = Microsoft365ContactsDirectoryOfficeProbeAdapter;
        let result = adapter
            .probe(
                &crate::office::OfficeAccount {
                    account_key: "contacts-ms".to_string(),
                    provider_kind: "microsoft365_contacts_directory".to_string(),
                    external_account_id: "alice@contoso.com".to_string(),
                    account_label: "Microsoft People".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
                },
                &OfficeCredential {
                    account_key: "contacts-ms".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
        assert_eq!(result.provider_kind, "microsoft365_contacts_directory");
    }

    #[test]
    fn microsoft_graph_user_maps_to_contact_entry() {
        let entry = MicrosoftGraphUser {
            id: "user-1".to_string(),
            display_name: "Alice Chen".to_string(),
            mail: "alice@contoso.com".to_string(),
            user_principal_name: "alice@contoso.com".to_string(),
            job_title: "PM".to_string(),
            department: "Beetle Team".to_string(),
            company_name: "Contoso".to_string(),
        }
        .to_contact_entry()
        .expect("contact")
        .expect("entry");
        assert_eq!(entry.id, "user-1");
        assert_eq!(entry.display_name, "Alice Chen");
        assert_eq!(entry.organization, "Beetle Team");
        assert!(entry.aliases.iter().any(|alias| alias == "alice"));
        assert!(entry
            .emails
            .iter()
            .any(|email| email == "alice@contoso.com"));
    }
}
