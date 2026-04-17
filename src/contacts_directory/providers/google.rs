#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use crate::contacts_directory::credentials::contacts_credential_from_office;
use crate::contacts_directory::{
    normalize_contact_entry, ContactEntry, ContactsDirectoryProvider,
    ContactsDirectoryProviderCredential,
};
use crate::error::{Error, Result};
use crate::office::{
    build_google_api_url, request_google_api_json, OfficeAccount, OfficeHttpClient,
    OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
use serde::Deserialize;

pub struct GoogleContactsDirectoryProvider;

impl ContactsDirectoryProvider for GoogleContactsDirectoryProvider {
    fn provider_name(&self) -> &'static str {
        "google_contacts_directory"
    }

    fn display_name(&self) -> &'static str {
        "Google People Directory"
    }

    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        validate_google_credential(credential)?;
        let client = GoogleContactsClient::new(credential)?;
        client.lookup_contacts(http, query, limit)
    }
}

pub struct GoogleContactsDirectoryOfficeProbeAdapter;

impl OfficeProbeAdapter for GoogleContactsDirectoryOfficeProbeAdapter {
    fn provider_kind(&self) -> &'static str {
        "google_contacts_directory"
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
                });
            }
        };
        if validate_google_credential(&adapted).is_err() {
            return Ok(OfficeProbeResult {
                account_key: account.account_key.clone(),
                provider_kind: account.provider_kind.clone(),
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "contacts_transport_config_missing".to_string(),
            });
        }
        let client = GoogleContactsClient::new(&adapted)?;
        client.list_connections(http, 1)?;
        Ok(OfficeProbeResult {
            account_key: account.account_key.clone(),
            provider_kind: account.provider_kind.clone(),
            configured: true,
            disposition: OfficeProbeDisposition::Ready,
            reason: "google_contacts_people_ok".to_string(),
        })
    }
}

struct GoogleContactsClient<'a> {
    credential: &'a ContactsDirectoryProviderCredential,
}

impl<'a> GoogleContactsClient<'a> {
    fn new(credential: &'a ContactsDirectoryProviderCredential) -> Result<Self> {
        validate_google_credential(credential)?;
        Ok(Self { credential })
    }

    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        let limit = limit.clamp(1, 50);
        let normalized_query = query.trim();
        if normalized_query.is_empty() {
            return self.list_connections(http, limit);
        }
        let payload: GoogleSearchContactsResponse = request_google_api_json(
            http,
            "google_contacts_lookup",
            "GET",
            &build_google_api_url(
                &self.credential.base_url,
                "/people:searchContacts",
                &[
                    ("query", normalized_query.to_string()),
                    (
                        "readMask",
                        "names,emailAddresses,organizations,biographies".to_string(),
                    ),
                    ("pageSize", limit.to_string()),
                ],
            ),
            &[("Authorization", self.authorization_header().as_str())],
            None,
        )?;
        payload
            .results
            .into_iter()
            .filter_map(|item| item.person)
            .map(|person| person.into_contact_entry())
            .collect()
    }

    fn list_connections(
        &self,
        http: &mut dyn OfficeHttpClient,
        limit: usize,
    ) -> Result<Vec<ContactEntry>> {
        let payload: GoogleConnectionsResponse = request_google_api_json(
            http,
            "google_contacts_connections",
            "GET",
            &build_google_api_url(
                &self.credential.base_url,
                "/people/me/connections",
                &[
                    (
                        "personFields",
                        "names,emailAddresses,organizations,biographies".to_string(),
                    ),
                    ("pageSize", limit.to_string()),
                ],
            ),
            &[("Authorization", self.authorization_header().as_str())],
            None,
        )?;
        payload
            .connections
            .into_iter()
            .map(|person| person.into_contact_entry())
            .collect()
    }

    fn authorization_header(&self) -> String {
        format!("Bearer {}", self.credential.secret)
    }
}

#[derive(Debug, Default, Deserialize)]
struct GoogleSearchContactsResponse {
    #[serde(default, rename = "results")]
    results: Vec<GoogleSearchContactResult>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleSearchContactResult {
    #[serde(default)]
    person: Option<GooglePerson>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleConnectionsResponse {
    #[serde(default)]
    connections: Vec<GooglePerson>,
}

#[derive(Debug, Default, Deserialize)]
struct GooglePerson {
    #[serde(default, rename = "resourceName")]
    resource_name: String,
    #[serde(default)]
    names: Vec<GoogleName>,
    #[serde(default, rename = "emailAddresses")]
    email_addresses: Vec<GoogleEmailAddress>,
    #[serde(default)]
    organizations: Vec<GoogleOrganization>,
    #[serde(default)]
    biographies: Vec<GoogleBiography>,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleName {
    #[serde(default, rename = "displayName")]
    display_name: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleEmailAddress {
    #[serde(default)]
    value: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleOrganization {
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
}

#[derive(Debug, Default, Deserialize)]
struct GoogleBiography {
    #[serde(default)]
    value: String,
}

impl GooglePerson {
    fn into_contact_entry(self) -> Result<ContactEntry> {
        let organization = self
            .organizations
            .first()
            .map(|item| item.name.trim().to_string())
            .unwrap_or_default();
        let notes = self
            .organizations
            .first()
            .map(|item| item.title.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.biographies
                    .first()
                    .map(|item| item.value.trim().to_string())
            })
            .unwrap_or_default();
        normalize_contact_entry(ContactEntry {
            id: self.resource_name,
            display_name: self
                .names
                .first()
                .map(|item| item.display_name.trim().to_string())
                .unwrap_or_default(),
            emails: self
                .email_addresses
                .into_iter()
                .map(|item| item.value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .collect(),
            aliases: Vec::new(),
            organization,
            notes,
            updated_at_unix_secs: crate::util::current_unix_secs(),
        })
    }
}

fn validate_google_credential(credential: &ContactsDirectoryProviderCredential) -> Result<()> {
    if credential.secret.trim().is_empty() {
        return Err(Error::config(
            "google_contacts_provider",
            "access token must not be empty",
        ));
    }
    if credential.base_url.trim().is_empty() {
        return Err(Error::config(
            "google_contacts_provider",
            "contacts_base_url must not be empty",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{OfficeAccountIdentityClass, OfficeCapability, OfficeCredential};

    #[test]
    fn google_contacts_provider_reports_people_capabilities() {
        let provider = GoogleContactsDirectoryProvider;
        assert_eq!(provider.provider_name(), "google_contacts_directory");
    }

    #[test]
    fn google_contacts_probe_adapter_reports_missing_transport_shape_before_network() {
        let adapter = GoogleContactsDirectoryOfficeProbeAdapter;
        let mut http = crate::office::UnavailableOfficeHttpClient;
        let result = adapter
            .probe(
                &mut http,
                &OfficeAccount {
                    account_key: "google-contacts".to_string(),
                    provider_kind: "google_contacts_directory".to_string(),
                    external_account_id: "alice@gmail.com".to_string(),
                    account_label: "Google Contacts".to_string(),
                    identity_class: OfficeAccountIdentityClass::Personal,
                    enabled_capabilities: vec![OfficeCapability::ContactsDirectory],
                },
                &OfficeCredential {
                    account_key: "google-contacts".to_string(),
                    access_token: String::new(),
                    refresh_token: String::new(),
                    token_endpoint: String::new(),
                    expires_at_unix_secs: 0,
                    updated_at: 0,
                    metadata: std::collections::BTreeMap::new(),
                },
            )
            .expect("probe result");
        assert_eq!(result.provider_kind, "google_contacts_directory");
        assert_eq!(
            result.disposition,
            OfficeProbeDisposition::MissingCredential
        );
    }
}
