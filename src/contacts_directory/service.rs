use crate::contacts_directory::{
    clamp_contacts_directory_limit, normalize_contact_entry, normalize_match_key,
    slugify_contact_id, ContactEntry, ContactsDirectoryEmailResolution, ContactsDirectoryLookupHit,
    ContactsDirectoryStatus, ContactsDirectoryStore, ContactsDirectoryUpsertResult,
};
use crate::error::{Error, Result};
use std::cmp::Reverse;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct ContactsDirectoryService {
    store: Arc<dyn ContactsDirectoryStore + Send + Sync>,
}

impl ContactsDirectoryService {
    pub fn new(store: Arc<dyn ContactsDirectoryStore + Send + Sync>) -> Self {
        Self { store }
    }

    pub fn status(&self) -> Result<ContactsDirectoryStatus> {
        let items = self.store.list()?;
        Ok(ContactsDirectoryStatus {
            total_contacts: items.len(),
            contacts_with_email: items.iter().filter(|item| !item.emails.is_empty()).count(),
            contacts_with_alias: items.iter().filter(|item| !item.aliases.is_empty()).count(),
            latest_updated_at_unix_secs: items
                .iter()
                .map(|item| item.updated_at_unix_secs)
                .max()
                .unwrap_or(0),
        })
    }

    pub fn list(&self, limit: Option<usize>) -> Result<Vec<ContactEntry>> {
        let limit = clamp_contacts_directory_limit(limit);
        let mut items = self.store.list()?;
        items.sort_by_key(|item| {
            (
                normalize_match_key(&item.display_name),
                Reverse(item.updated_at_unix_secs),
                item.id.clone(),
            )
        });
        items.truncate(limit);
        Ok(items)
    }

    pub fn lookup(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ContactsDirectoryLookupHit>> {
        let query = normalize_match_key(query);
        if query.is_empty() {
            return Err(Error::config(
                "contacts_directory_lookup",
                "query must not be empty",
            ));
        }

        let limit = clamp_contacts_directory_limit(limit);
        let mut hits = self
            .store
            .list()?
            .into_iter()
            .filter_map(|contact| score_contact_match(&contact, &query))
            .collect::<Vec<_>>();
        hits.sort_by_key(|hit| {
            (
                Reverse(hit.score),
                normalize_match_key(&hit.contact.display_name),
                hit.contact.id.clone(),
            )
        });
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn resolve_primary_email(&self, query: &str) -> Result<ContactsDirectoryEmailResolution> {
        let mut hits = self
            .lookup(query, Some(5))?
            .into_iter()
            .filter(|hit| !hit.contact.emails.is_empty())
            .collect::<Vec<_>>();
        if hits.is_empty() {
            return Err(Error::config(
                "contacts_directory_email_resolve",
                format!("no contact with email matches query '{query}'"),
            ));
        }
        let best = hits.remove(0);
        if hits
            .first()
            .is_some_and(|candidate| candidate.score == best.score)
        {
            return Err(Error::config(
                "contacts_directory_email_resolve",
                format!("contact query '{query}' is ambiguous"),
            ));
        }
        Ok(ContactsDirectoryEmailResolution {
            query: query.to_string(),
            contact_id: best.contact.id,
            display_name: best.contact.display_name,
            email: best.contact.emails.into_iter().next().unwrap_or_default(),
            match_reason: best.match_reason,
            score: best.score,
        })
    }

    pub fn upsert(&self, mut contact: ContactEntry) -> Result<ContactsDirectoryUpsertResult> {
        let existing = self.store.list()?;
        let existing_ids = existing
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        let created = if contact.id.trim().is_empty() {
            let matching_id = infer_existing_id_from_email(&existing, &contact);
            match matching_id {
                Some(id) => {
                    contact.id = id;
                    false
                }
                None => {
                    contact.id = allocate_contact_id(&contact, &existing_ids);
                    true
                }
            }
        } else {
            let normalized_id = slugify_contact_id(contact.id.trim());
            let exists = existing.iter().any(|item| item.id == normalized_id);
            contact.id = normalized_id;
            !exists
        };

        contact = normalize_contact_entry(contact)?;
        if contact.updated_at_unix_secs == 0 {
            contact.updated_at_unix_secs = crate::util::current_unix_secs();
        }
        self.store.upsert(&contact)?;
        Ok(ContactsDirectoryUpsertResult { created, contact })
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let id = slugify_contact_id(id);
        if id.is_empty() {
            return Err(Error::config(
                "contacts_directory_delete",
                "id must not be empty",
            ));
        }
        self.store.delete(&id)
    }
}

fn infer_existing_id_from_email(
    existing: &[ContactEntry],
    contact: &ContactEntry,
) -> Option<String> {
    let primary_email = contact
        .emails
        .first()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())?;
    let mut matches = existing
        .iter()
        .filter(|item| item.emails.iter().any(|email| email == &primary_email));
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first.id.clone())
    }
}

fn allocate_contact_id(contact: &ContactEntry, existing_ids: &BTreeSet<String>) -> String {
    let seed = contact
        .display_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let base = slugify_contact_id(if seed.is_empty() {
        contact
            .emails
            .first()
            .map(String::as_str)
            .unwrap_or("contact")
    } else {
        &seed
    });
    if !existing_ids.contains(&base) {
        return base;
    }
    for idx in 2..=9999 {
        let candidate = format!("{base}-{idx}");
        if !existing_ids.contains(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", crate::util::current_unix_secs())
}

fn score_contact_match(contact: &ContactEntry, query: &str) -> Option<ContactsDirectoryLookupHit> {
    let mut best_score = 0u32;
    let mut best_reason = None::<&str>;

    let name_key = normalize_match_key(&contact.display_name);
    let organization_key = normalize_match_key(&contact.organization);

    update_best_match(
        query,
        &name_key,
        "display_name_exact",
        "display_name_prefix",
        "display_name_contains",
        &mut best_score,
        &mut best_reason,
        420,
        340,
        260,
    );

    for alias in &contact.aliases {
        let alias_key = normalize_match_key(alias);
        update_best_match(
            query,
            &alias_key,
            "alias_exact",
            "alias_prefix",
            "alias_contains",
            &mut best_score,
            &mut best_reason,
            390,
            320,
            240,
        );
    }

    for email in &contact.emails {
        let email_key = normalize_match_key(email);
        update_best_match(
            query,
            &email_key,
            "email_exact",
            "email_prefix",
            "email_contains",
            &mut best_score,
            &mut best_reason,
            520,
            460,
            360,
        );
    }

    if !organization_key.is_empty() && organization_key.contains(query) && best_score < 180 {
        best_score = 180;
        best_reason = Some("organization_contains");
    }

    best_reason.map(|reason| ContactsDirectoryLookupHit {
        contact: contact.clone(),
        match_reason: reason.to_string(),
        score: best_score,
    })
}

#[allow(clippy::too_many_arguments)]
fn update_best_match(
    query: &str,
    value: &str,
    exact_reason: &'static str,
    prefix_reason: &'static str,
    contains_reason: &'static str,
    best_score: &mut u32,
    best_reason: &mut Option<&'static str>,
    exact_score: u32,
    prefix_score: u32,
    contains_score: u32,
) {
    if value.is_empty() {
        return;
    }
    let (score, reason) = if value == query {
        (exact_score, exact_reason)
    } else if value.starts_with(query) {
        (prefix_score, prefix_reason)
    } else if value.contains(query) {
        (contains_score, contains_reason)
    } else {
        return;
    };
    if score > *best_score {
        *best_score = score;
        *best_reason = Some(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::ContactsDirectoryService;
    use crate::contacts_directory::{ContactEntry, ContactsDirectoryStore};
    use crate::error::Result;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryStore {
        items: Mutex<HashMap<String, ContactEntry>>,
    }

    impl ContactsDirectoryStore for MemoryStore {
        fn list(&self) -> Result<Vec<ContactEntry>> {
            Ok(self.items.lock().unwrap().values().cloned().collect())
        }

        fn get(&self, id: &str) -> Result<Option<ContactEntry>> {
            Ok(self.items.lock().unwrap().get(id).cloned())
        }

        fn upsert(&self, contact: &ContactEntry) -> Result<()> {
            self.items
                .lock()
                .unwrap()
                .insert(contact.id.clone(), contact.clone());
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self.items.lock().unwrap().remove(id).is_some())
        }
    }

    fn build_service() -> ContactsDirectoryService {
        let store = Arc::new(MemoryStore::default());
        let service = ContactsDirectoryService::new(store);
        service
            .upsert(ContactEntry {
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@example.com".to_string()],
                aliases: vec!["阿丽丝".to_string()],
                organization: "Beetle".to_string(),
                ..ContactEntry::default()
            })
            .expect("seed alice");
        service
            .upsert(ContactEntry {
                display_name: "Bob Li".to_string(),
                emails: vec!["bob@example.com".to_string()],
                organization: "Other".to_string(),
                ..ContactEntry::default()
            })
            .expect("seed bob");
        service
    }

    #[test]
    fn lookup_prefers_exact_email_matches() {
        let service = build_service();
        let hits = service
            .lookup("alice@example.com", Some(5))
            .expect("lookup contacts");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].match_reason, "email_exact");
        assert_eq!(hits[0].contact.display_name, "Alice Zhang");
    }

    #[test]
    fn lookup_matches_aliases() {
        let service = build_service();
        let hits = service.lookup("阿丽", Some(5)).expect("lookup by alias");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].contact.display_name, "Alice Zhang");
    }

    #[test]
    fn resolve_primary_email_returns_best_unique_match() {
        let service = build_service();
        let resolution = service
            .resolve_primary_email("alice@example.com")
            .expect("resolve email");

        assert_eq!(resolution.contact_id, "alice-zhang");
        assert_eq!(resolution.email, "alice@example.com");
    }

    #[test]
    fn upsert_reuses_existing_id_for_unique_email() {
        let service = build_service();
        let alice = service
            .lookup("alice@example.com", Some(1))
            .unwrap()
            .remove(0);
        let updated = service
            .upsert(ContactEntry {
                display_name: "Alice Zhang".to_string(),
                emails: vec!["alice@example.com".to_string()],
                notes: "friend".to_string(),
                ..ContactEntry::default()
            })
            .expect("update alice");

        assert!(!updated.created);
        assert_eq!(updated.contact.id, alice.contact.id);
    }
}
