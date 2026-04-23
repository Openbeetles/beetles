use crate::calendar::{CalendarEvent, CalendarProviderCredential, CalendarQuery};
use crate::error::Result;
use crate::office::office_refactor_helpers::define_provider_registry;
use crate::platform::ResponseBody;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalendarOperation {
    List,
    Get,
    Create,
    Update,
    Delete,
}

pub trait CalendarHttpClient {
    fn request_with_headers(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        match method {
            "GET" => self.get_with_headers(url, headers),
            "POST" => self.post_with_headers(url, headers, body.unwrap_or_default()),
            "PATCH" => self.patch_with_headers(url, headers, body.unwrap_or_default()),
            "PUT" => self.put_with_headers(url, headers, body.unwrap_or_default()),
            "DELETE" => self.delete_with_headers(url, headers),
            other => Err(crate::error::Error::config(
                "calendar_http_method",
                format!("unsupported http method: {other}"),
            )),
        }
    }
    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)>;
    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;
    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;
    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;
    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)>;
}

pub trait CalendarProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn supports(&self, op: CalendarOperation) -> bool;
    fn list_events(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>>;
    fn get_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<Option<CalendarEvent>>;
    fn create_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent>;
    fn update_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        event: &CalendarEvent,
    ) -> Result<CalendarEvent>;
    fn delete_event(
        &self,
        http: &mut dyn CalendarHttpClient,
        credential: &CalendarProviderCredential,
        id: &str,
    ) -> Result<bool>;
}

define_provider_registry!(CalendarProviderRegistry, crate::calendar::CalendarProvider);
