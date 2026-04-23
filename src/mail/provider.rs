use crate::error::Result;
use crate::mail::{
    MailMessage, MailMessageSummary, MailProviderCredential, MailQuery, MailSearchQuery,
    MailSendRequest,
};
use crate::office::office_refactor_helpers::define_provider_registry;
use crate::office::OfficeHttpClient;

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
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailQuery,
    ) -> Result<Vec<MailMessageSummary>>;
    fn search_messages(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        query: MailSearchQuery,
    ) -> Result<Vec<MailMessageSummary>>;
    fn get_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        id: &str,
    ) -> Result<Option<MailMessage>>;
    fn send_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary>;
    fn draft_message(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &MailProviderCredential,
        request: &MailSendRequest,
    ) -> Result<MailMessageSummary>;
}

define_provider_registry!(MailProviderRegistry, crate::mail::MailProvider);
