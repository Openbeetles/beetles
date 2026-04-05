use crate::calendar::{
    CALENDAR_PROVIDER_LOCAL, CalendarEvent, CalendarHttpClient, CalendarOperation,
    CalendarProviderCredentialStore, CalendarProviderRegistry, CalendarQuery, CalendarStore,
};
use crate::error::{Error, Result};
use std::sync::Arc;

pub struct CalendarService {
    local_store: Arc<dyn CalendarStore + Send + Sync>,
    credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
    providers: CalendarProviderRegistry,
}

impl CalendarService {
    pub fn new(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
    ) -> Self {
        Self {
            local_store,
            credential_store,
            providers,
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.providers.names()
    }

    pub fn list_provider_statuses(
        &self,
    ) -> Result<Vec<crate::calendar::CalendarProviderCredentialStatus>> {
        self.credential_store.list_statuses()
    }

    pub fn list(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.list(query);
        }
        let (provider_impl, credential) = self.resolve_remote(provider, CalendarOperation::List)?;
        let http = require_http(provider, http)?;
        provider_impl.list_events(http, &credential, query)
    }

    pub fn get(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.get(id);
        }
        let (provider_impl, credential) = self.resolve_remote(provider, CalendarOperation::Get)?;
        let http = require_http(provider, http)?;
        provider_impl.get_event(http, &credential, id)
    }

    pub fn upsert(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        event: &CalendarEvent,
        is_create: bool,
    ) -> Result<CalendarEvent> {
        if is_local_provider(provider) {
            self.local_store.upsert(event)?;
            return self
                .local_store
                .get(&event.id)?
                .ok_or_else(|| Error::config("calendar_service", "event missing after upsert"));
        }
        let op = if is_create {
            CalendarOperation::Create
        } else {
            CalendarOperation::Update
        };
        let (provider_impl, credential) = self.resolve_remote(provider, op)?;
        let http = require_http(provider, http)?;
        if is_create {
            provider_impl.create_event(http, &credential, event)
        } else {
            provider_impl.update_event(http, &credential, event)
        }
    }

    pub fn delete(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        id: &str,
    ) -> Result<bool> {
        if is_local_provider(provider) {
            return self.local_store.delete(id);
        }
        let (provider_impl, credential) =
            self.resolve_remote(provider, CalendarOperation::Delete)?;
        let http = require_http(provider, http)?;
        provider_impl.delete_event(http, &credential, id)
    }

    fn resolve_remote(
        &self,
        provider: &str,
        op: CalendarOperation,
    ) -> Result<(
        std::sync::Arc<dyn crate::calendar::CalendarProvider>,
        crate::calendar::CalendarProviderCredential,
    )> {
        let provider_impl = self.providers.get(provider).ok_or_else(|| {
            Error::config(
                "calendar_provider",
                format!("provider '{}' is not registered", provider),
            )
        })?;
        if !provider_impl.supports(op) {
            return Err(Error::config(
                "calendar_provider",
                format!("provider '{}' does not support {:?}", provider, op),
            ));
        }
        let credential = self.credential_store.get(provider)?.ok_or_else(|| {
            Error::config(
                "calendar_provider",
                format!("provider '{}' has no configured credential", provider),
            )
        })?;
        if credential.access_token.trim().is_empty() {
            return Err(Error::config(
                "calendar_provider",
                format!("provider '{}' credential is missing access_token", provider),
            ));
        }
        Ok((provider_impl, credential))
    }
}

fn is_local_provider(provider: &str) -> bool {
    provider.trim().is_empty() || provider == CALENDAR_PROVIDER_LOCAL
}

fn require_http<'a>(
    provider: &str,
    http: Option<&'a mut dyn CalendarHttpClient>,
) -> Result<&'a mut dyn CalendarHttpClient> {
    http.ok_or_else(|| {
        Error::config(
            "calendar_provider",
            format!("provider '{}' requires HTTP context", provider),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarEventStatus, CalendarProvider, CalendarProviderCredential,
        CalendarProviderCredentialStatus, CalendarProviderCredentialStore,
    };
    use crate::platform::ResponseBody;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubCalendarStore {
        items: Mutex<HashMap<String, CalendarEvent>>,
    }

    impl CalendarStore for StubCalendarStore {
        fn list(&self, query: CalendarQuery) -> Result<Vec<CalendarEvent>> {
            let items = self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            Ok(crate::calendar::filter_calendar_events(items, query))
        }

        fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned())
        }

        fn upsert(&self, event: &CalendarEvent) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(event.id.clone(), event.clone());
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(id)
                .is_some())
        }
    }

    #[derive(Default)]
    struct StubCredentialStore {
        items: Mutex<HashMap<String, CalendarProviderCredential>>,
    }

    impl CalendarProviderCredentialStore for StubCredentialStore {
        fn get(&self, provider: &str) -> Result<Option<CalendarProviderCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(provider)
                .cloned())
        }

        fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.provider.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, provider: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(provider);
            Ok(())
        }

        fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .map(|credential| credential.status())
                .collect())
        }
    }

    struct StubHttp;

    impl CalendarHttpClient for StubHttp {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn patch_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn put_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }

        fn delete_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((200, ResponseBody::Heap(Vec::new())))
        }
    }

    struct MockRemoteProvider;

    impl CalendarProvider for MockRemoteProvider {
        fn provider_name(&self) -> &'static str {
            "mock_remote"
        }

        fn display_name(&self) -> &'static str {
            "Mock Remote"
        }

        fn supports(&self, _op: CalendarOperation) -> bool {
            true
        }

        fn list_events(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Ok(vec![CalendarEvent {
                id: "remote-1".to_string(),
                title: "远端会议".to_string(),
                start_at_unix_secs: 100,
                end_at_unix_secs: 160,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: "mock_remote".to_string(),
                calendar_id: "default".to_string(),
                remote_id: "r1".to_string(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 1,
            }])
        }

        fn get_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Ok(Some(CalendarEvent {
                id: id.to_string(),
                title: "远端详情".to_string(),
                start_at_unix_secs: 100,
                end_at_unix_secs: 160,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: "mock_remote".to_string(),
                calendar_id: "default".to_string(),
                remote_id: "r1".to_string(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 1,
            }))
        }

        fn create_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn update_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn delete_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Ok(true)
        }
    }

    #[test]
    fn service_routes_local_queries_to_local_store() {
        let local_store = Arc::new(StubCalendarStore::default());
        local_store
            .upsert(&CalendarEvent {
                id: "local-1".to_string(),
                title: "本地事项".to_string(),
                start_at_unix_secs: 10,
                end_at_unix_secs: 20,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: "local".to_string(),
                calendar_id: "default".to_string(),
                remote_id: String::new(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 1,
            })
            .unwrap();
        let service = CalendarService::new(
            local_store,
            Arc::new(StubCredentialStore::default()),
            CalendarProviderRegistry::new(),
        );
        let items = service
            .list(None, "local", CalendarQuery::upcoming(0, 10))
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "local-1");
    }

    #[test]
    fn service_routes_remote_queries_when_provider_and_credential_exist() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                provider: "mock_remote".to_string(),
                account_id: String::new(),
                account_label: String::new(),
                calendar_id: "default".to_string(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
            })
            .unwrap();
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(MockRemoteProvider));
        let service = CalendarService::new(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
        );
        let mut http = StubHttp;
        let items = service
            .list(
                Some(&mut http),
                "mock_remote",
                CalendarQuery {
                    start_from_unix_secs: None,
                    start_to_unix_secs: None,
                    limit: 10,
                    include_cancelled: false,
                },
            )
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].provider, "mock_remote");
    }
}
