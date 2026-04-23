use crate::contacts_directory::{ContactEntry, ContactsDirectoryProviderCredential};
use crate::error::Result;
use crate::office::office_refactor_helpers::define_provider_registry;
use crate::office::OfficeHttpClient;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContactsDirectoryOperation {
    Lookup,
}

pub trait ContactsDirectoryProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn lookup_contacts(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &ContactsDirectoryProviderCredential,
        query: &str,
        limit: usize,
    ) -> Result<Vec<ContactEntry>>;

    fn supports(&self, op: ContactsDirectoryOperation) -> bool {
        matches!(op, ContactsDirectoryOperation::Lookup)
    }
}

define_provider_registry!(
    ContactsDirectoryProviderRegistry,
    crate::contacts_directory::ContactsDirectoryProvider
);
