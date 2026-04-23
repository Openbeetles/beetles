use crate::calendar::{CalendarEvent, CalendarProviderCredential, CalendarQuery};
use crate::error::Result;
use crate::office::office_refactor_helpers::define_provider_registry;
use crate::office::OfficeHttpClient;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarOperation {
    List,
    Get,
    Create,
    Update,
    Delete,
}

pub trait CalendarProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn supports(&self, op: CalendarOperation) -> bool;
    fn list_events(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>>;
    fn get_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>>;
    fn create_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent>;
    fn update_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent>;
    fn delete_event(
        &self,
        http: &mut dyn OfficeHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool>;
}

define_provider_registry!(CalendarProviderRegistry, crate::calendar::CalendarProvider);
