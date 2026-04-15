use crate::error::Result;
use crate::mail::{
    MailMessage, MailMessageSummary, MailProviderCredential, MailQuery, MailSearchQuery,
    MailSendRequest,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailOperation {
    List,
    Search,
    Get,
    Send,
    Draft,
}

pub trait MailProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn supports(&self, op: MailOperation) -> bool;
    fn list_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>>;
    fn search_messages(
        &self,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>>;
    fn get_message(
        &self,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>>;
    fn send_message(
        &self,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary>;
    fn draft_message(
        &self,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary>;
}

#[derive(Default)]
pub struct MailProviderRegistry {
    providers: HashMap<&'static str, Arc<dyn MailProvider>>,
}

impl MailProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn MailProvider>) {
        self.providers.insert(provider.provider_name(), provider);
    }

    pub fn get(&self, provider: &str) -> Option<Arc<dyn MailProvider>> {
        self.providers.get(provider).cloned()
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names = self.providers.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }
}
