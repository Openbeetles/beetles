//! Shared contacts directory domain: local people records, lookup scoring, and persistent store.

mod service;
mod store;

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub use service::ContactsDirectoryService;
pub use store::StateFsContactsDirectoryStore;

pub const REL_PATH_CONTACTS_DIRECTORY: &str = "office/contacts_directory.json";
pub const DEFAULT_CONTACTS_DIRECTORY_LIMIT: usize = 10;
pub const MAX_CONTACTS_DIRECTORY_LIMIT: usize = 50;
pub const MAX_CONTACTS_DIRECTORY_ENTRIES: usize = 256;

const MAX_CONTACT_ID_CHARS: usize = 96;
const MAX_CONTACT_NAME_CHARS: usize = 128;
const MAX_CONTACT_EMAILS: usize = 8;
const MAX_CONTACT_ALIASES: usize = 12;
const MAX_CONTACT_ALIAS_CHARS: usize = 64;
const MAX_CONTACT_ORGANIZATION_CHARS: usize = 128;
const MAX_CONTACT_NOTES_CHARS: usize = 512;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub emails: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub organization: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub updated_at_unix_secs: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectorySegment {
    #[serde(default)]
    pub items: Vec<ContactEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryLookupHit {
    pub contact: ContactEntry,
    pub match_reason: String,
    pub score: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryEmailResolution {
    pub query: String,
    pub contact_id: String,
    pub display_name: String,
    pub email: String,
    pub match_reason: String,
    pub score: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryStatus {
    pub total_contacts: usize,
    pub contacts_with_email: usize,
    pub contacts_with_alias: usize,
    pub latest_updated_at_unix_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactsDirectoryUpsertResult {
    pub created: bool,
    pub contact: ContactEntry,
}

pub trait ContactsDirectoryStore: Send + Sync {
    fn list(&self) -> Result<Vec<ContactEntry>>;
    fn get(&self, id: &str) -> Result<Option<ContactEntry>>;
    fn upsert(&self, contact: &ContactEntry) -> Result<()>;
    fn delete(&self, id: &str) -> Result<bool>;
}

pub fn clamp_contacts_directory_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_CONTACTS_DIRECTORY_LIMIT)
        .clamp(1, MAX_CONTACTS_DIRECTORY_LIMIT)
}

pub(crate) fn normalize_contact_entry(mut contact: ContactEntry) -> Result<ContactEntry> {
    contact.id = normalize_field(&contact.id, MAX_CONTACT_ID_CHARS);
    contact.display_name = normalize_field(&contact.display_name, MAX_CONTACT_NAME_CHARS);
    contact.organization = normalize_field(&contact.organization, MAX_CONTACT_ORGANIZATION_CHARS);
    contact.notes = normalize_field(&contact.notes, MAX_CONTACT_NOTES_CHARS);
    contact.aliases = normalize_aliases(&contact.aliases);
    contact.emails = normalize_emails(&contact.emails)?;

    if contact.display_name.is_empty() {
        contact.display_name = contact
            .aliases
            .first()
            .cloned()
            .or_else(|| contact.emails.first().cloned())
            .unwrap_or_default();
    }
    if contact.display_name.is_empty() {
        return Err(Error::config(
            "contacts_directory_contact",
            "display_name or email is required",
        ));
    }

    Ok(contact)
}

pub(crate) fn slugify_contact_id(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in value.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
            continue;
        }
        if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "contact".to_string()
    } else {
        truncate_content_to_max(trimmed, MAX_CONTACT_ID_CHARS).into_owned()
    }
}

pub(crate) fn normalize_match_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn normalize_aliases(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        let normalized = normalize_field(value, MAX_CONTACT_ALIAS_CHARS);
        if normalized.is_empty() {
            continue;
        }
        let dedupe_key = normalize_match_key(&normalized);
        if seen.insert(dedupe_key) {
            out.push(normalized);
        }
        if out.len() >= MAX_CONTACT_ALIASES {
            break;
        }
    }
    out
}

fn normalize_emails(values: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for value in values {
        let normalized = normalize_field(value, MAX_CONTACT_NAME_CHARS).to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        if !looks_like_email(&normalized) {
            return Err(Error::config(
                "contacts_directory_contact",
                format!("invalid email '{normalized}'"),
            ));
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
        if out.len() >= MAX_CONTACT_EMAILS {
            break;
        }
    }
    Ok(out)
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    !local.is_empty() && !domain.is_empty() && !domain.starts_with('.') && !domain.ends_with('.')
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_contacts_directory_limit, normalize_contact_entry, slugify_contact_id, ContactEntry,
        DEFAULT_CONTACTS_DIRECTORY_LIMIT, MAX_CONTACTS_DIRECTORY_LIMIT,
    };

    #[test]
    fn normalize_contact_entry_falls_back_to_email_for_name() {
        let contact = normalize_contact_entry(ContactEntry {
            emails: vec!["Alice@Example.com".to_string()],
            ..ContactEntry::default()
        })
        .expect("normalize contact");

        assert_eq!(contact.display_name, "alice@example.com");
        assert_eq!(contact.emails, vec!["alice@example.com"]);
    }

    #[test]
    fn normalize_contact_entry_rejects_invalid_email() {
        let error = normalize_contact_entry(ContactEntry {
            display_name: "Alice".to_string(),
            emails: vec!["invalid-email".to_string()],
            ..ContactEntry::default()
        })
        .expect_err("invalid email must fail");

        assert_eq!(error.stage(), "contacts_directory_contact");
    }

    #[test]
    fn slugify_contact_id_keeps_unicode_letters() {
        assert_eq!(slugify_contact_id("张 三 / Beetle"), "张-三-beetle");
    }

    #[test]
    fn clamp_contacts_directory_limit_stays_within_bounds() {
        assert_eq!(
            clamp_contacts_directory_limit(None),
            DEFAULT_CONTACTS_DIRECTORY_LIMIT
        );
        assert_eq!(clamp_contacts_directory_limit(Some(0)), 1);
        assert_eq!(
            clamp_contacts_directory_limit(Some(999)),
            MAX_CONTACTS_DIRECTORY_LIMIT
        );
    }
}
