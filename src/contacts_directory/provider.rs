use crate::contacts_directory::{ContactEntry, ContactsDirectoryProviderCredential};
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactsDirectoryOperation {
    Lookup,
}

pub trait ContactsDirectoryProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn lookup_contacts(
        &self,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>>;

    fn supports(&self, op: ContactsDirectoryOperation) -> bool {
        matches!(op, ContactsDirectoryOperation::Lookup)
    }
}

#[derive(Clone, Default)]
pub struct ContactsDirectoryProviderRegistry {
    providers: HashMap<&'static str, Arc<dyn ContactsDirectoryProvider>>,
}

impl ContactsDirectoryProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ContactsDirectoryProvider>) {
        self.providers.insert(provider.provider_name(), provider);
    }

    pub fn get(&self, provider: &str) -> Option<Arc<dyn ContactsDirectoryProvider>> {
        self.providers.get(provider).cloned()
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names = self.providers.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }
}
