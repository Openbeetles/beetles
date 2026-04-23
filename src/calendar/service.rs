use crate::calendar::{
    CalendarEvent, CalendarOperation, CalendarProvider, CalendarProviderCredential,
    CalendarProviderCredentialStore, CalendarProviderRegistry, CalendarQuery, CalendarStore,
    CALENDAR_PROVIDER_LOCAL,
};
use crate::error::{Error, Result};
use crate::office::{
    office_authority_from_service, OfficeAccountAssessment, OfficeAccountIdentityClass,
    OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability, OfficeCapabilityRuntime,
    OfficeCapabilityServiceCore, OfficeHttpClient, OfficeResolveResult, OfficeService,
};
use std::sync::Arc;

type CalendarRemoteCore = OfficeCapabilityServiceCore<
    CalendarProviderRegistry,
    dyn CalendarProviderCredentialStore + Send + Sync,
>;

pub struct CalendarService {
    local_store: Arc<dyn CalendarStore + Send + Sync>,
    core: CalendarRemoteCore,
}

struct CalendarRemoteRoute<'a> {
    provider: &'a str,
    account_key: Option<&'a str>,
    preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ops: &'a [CalendarOperation],
    activity_kind: &'static str,
}

impl CalendarService {
    pub fn new(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
    ) -> Self {
        Self::build(local_store, credential_store, providers, None)
    }

    pub fn with_office_service(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self::build(
            local_store,
            credential_store,
            providers,
            office_authority_from_service(office_service),
        )
    }

    pub fn with_office_authority(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self::build(local_store, credential_store, providers, office_authority)
    }

    fn build(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            local_store,
            core: OfficeCapabilityServiceCore::new(
                providers,
                credential_store,
                OfficeCapabilityRuntime::new(
                    OfficeCapability::Calendar,
                    "calendar_provider",
                    "calendar",
                    "calendar_runtime",
                    office_authority,
                ),
                "calendar_provider",
            ),
        }
    }

    pub fn provider_names(&self) -> Vec<&'static str> {
        self.core.provider_names()
    }

    pub fn resolve_provider_name(&self, provider: Option<&str>) -> Result<String> {
        self.resolve_provider_name_with_identity(provider, None)
    }

    pub fn resolve_provider_name_with_identity(
        &self,
        provider: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<String> {
        if let Some(provider) = provider.map(str::trim).filter(|value| !value.is_empty()) {
            return Ok(provider.to_string());
        }
        if let Some(route) = self.core.selected_route(None, preferred_identity_class)? {
            return Ok(route.provider);
        }
        Ok(CALENDAR_PROVIDER_LOCAL.to_string())
    }

    pub fn list_provider_statuses(
        &self,
    ) -> Result<Vec<crate::calendar::CalendarProviderCredentialStatus>> {
        self.core.list_provider_statuses()
    }

    pub fn office_resolve_hint(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeResolveResult>> {
        self.office_resolve_hint_with_identity(provider, account_key, None)
    }

    pub fn office_resolve_hint_with_identity(
        &self,
        provider: Option<&str>,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<Option<OfficeResolveResult>> {
        self.core
            .resolve_hint(provider, account_key, preferred_identity_class)
    }

    pub fn resolve_account_key_for_provider(
        &self,
        provider: &str,
        account_key: Option<&str>,
    ) -> Result<Option<String>> {
        self.resolve_account_key_for_provider_with_identity(provider, account_key, None)
    }

    pub fn resolve_account_key_for_provider_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<Option<String>> {
        if is_local_provider(provider) {
            return Ok(None);
        }
        self.resolve_account_key_with_identity(provider, account_key, preferred_identity_class)
            .map(Some)
    }

    pub fn default_calendar_id_for_provider_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<Option<String>> {
        let Some(account_key) = self.resolve_account_key_for_provider_with_identity(
            provider,
            account_key,
            preferred_identity_class,
        )?
        else {
            return Ok(None);
        };
        let (_, credential) = self.resolve_remote_with_identity(
            provider,
            Some(account_key.as_str()),
            preferred_identity_class,
            &[],
        )?;
        Ok((!credential.calendar_id.trim().is_empty()).then_some(credential.calendar_id))
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        self.core.runtime_statuses()
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        self.core.account_assessments()
    }

    pub fn office_identity_class_for_account(
        &self,
        account_key: Option<&str>,
    ) -> Result<Option<OfficeAccountIdentityClass>> {
        self.core.identity_class_for_account(account_key)
    }

    pub fn provider_supports(&self, provider: &str, op: CalendarOperation) -> bool {
        if is_local_provider(provider) {
            return true;
        }
        self.core.provider_supports(provider, op)
    }

    pub fn provider_is_routable_for_op(
        &self,
        provider: &str,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        op: CalendarOperation,
    ) -> bool {
        if is_local_provider(provider) {
            return true;
        }
        self.core
            .provider_is_routable_for_ops(provider, preferred_identity_class, &[op])
    }

    pub fn list(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        self.list_with_identity(http, provider, account_key, None, query)
    }

    pub fn list_with_identity(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.list(query);
        }
        self.run_remote_operation(
            http,
            CalendarRemoteRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[CalendarOperation::List],
                activity_kind: "calendar_list",
            },
            |http, provider_impl, credential| provider_impl.list_events(http, credential, query),
        )
    }

    pub fn get(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        self.get_with_identity(http, provider, account_key, None, id)
    }

    pub fn get_with_identity(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.get(id);
        }
        self.run_remote_operation(
            http,
            CalendarRemoteRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[CalendarOperation::Get],
                activity_kind: "calendar_get",
            },
            |http, provider_impl, credential| provider_impl.get_event(http, credential, id),
        )
    }

    pub fn upsert(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        event: &CalendarEvent,
        is_create: bool,
    ) -> Result<CalendarEvent> {
        self.upsert_with_identity(http, provider, account_key, None, event, is_create)
    }

    pub fn upsert_with_identity(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
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
        self.run_remote_operation(
            http,
            CalendarRemoteRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[op],
                activity_kind: if is_create {
                    "calendar_create"
                } else {
                    "calendar_update"
                },
            },
            |http, provider_impl, credential| {
                if is_create {
                    provider_impl.create_event(http, credential, event)
                } else {
                    provider_impl.update_event(http, credential, event)
                }
            },
        )
    }

    pub fn delete(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<bool> {
        self.delete_with_identity(http, provider, account_key, None, id)
    }

    pub fn delete_with_identity(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        id: &str,
    ) -> Result<bool> {
        if is_local_provider(provider) {
            return self.local_store.delete(id);
        }
        self.run_remote_operation(
            http,
            CalendarRemoteRoute {
                provider,
                account_key,
                preferred_identity_class,
                ops: &[CalendarOperation::Delete],
                activity_kind: "calendar_delete",
            },
            |http, provider_impl, credential| provider_impl.delete_event(http, credential, id),
        )
    }

    fn resolve_remote_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
        ops: &[CalendarOperation],
    ) -> Result<(Arc<dyn CalendarProvider>, CalendarProviderCredential)> {
        let (provider_impl, credential) = self.core.resolve_registered_remote(
            provider,
            account_key,
            preferred_identity_class,
            ops,
        )?;
        if credential.access_token.trim().is_empty() {
            return Err(Error::config(
                "calendar_provider",
                format!("provider '{}' credential is missing access_token", provider),
            ));
        }
        Ok((provider_impl, credential))
    }

    fn resolve_account_key_with_identity(
        &self,
        provider: &str,
        account_key: Option<&str>,
        preferred_identity_class: Option<OfficeAccountIdentityClass>,
    ) -> Result<String> {
        Ok(self
            .core
            .resolve_explicit_route(
                Some(provider),
                account_key,
                preferred_identity_class,
                "provider is required",
            )?
            .account_key)
    }

    fn run_remote_operation<T, F>(
        &self,
        http: Option<&mut dyn OfficeHttpClient>,
        route: CalendarRemoteRoute<'_>,
        execute: F,
    ) -> Result<T>
    where
        F: FnOnce(
            &mut dyn OfficeHttpClient,
            &Arc<dyn CalendarProvider>,
            &CalendarProviderCredential,
        ) -> Result<T>,
    {
        let (provider_impl, credential) = self.resolve_remote_with_identity(
            route.provider,
            route.account_key,
            route.preferred_identity_class,
            route.ops,
        )?;
        let http = require_http(route.provider, http)?;
        let result = execute(http, &provider_impl, &credential);
        self.core.record_runtime_activity(
            &credential.account_key,
            route.activity_kind,
            result.as_ref().err(),
        );
        result
    }
}

fn is_local_provider(provider: &str) -> bool {
    provider.trim().is_empty() || provider == CALENDAR_PROVIDER_LOCAL
}

fn require_http<'a>(
    provider: &str,
    http: Option<&'a mut dyn OfficeHttpClient>,
) -> Result<&'a mut dyn OfficeHttpClient> {
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
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry,
        OfficeAccountRuntimeStatus, OfficeCapability, OfficeCredential, OfficeCredentialStore,
        OfficeRuntimeStatusStore, OfficeSelectionPolicy,
    };
    use crate::platform::ResponseBody;
    use std::collections::{BTreeMap, HashMap};
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
        fn get(&self, account_key: &str) -> Result<Option<CalendarProviderCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|(_, credential)| credential.provider == provider)
                .map(|(account_key, _)| account_key.clone())
                .collect())
        }

        fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
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

    #[derive(Default)]
    struct StubOfficeCredentialStore {
        items: Mutex<HashMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for StubOfficeCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryRuntimeStatusStore {
        items: Mutex<BTreeMap<String, OfficeAccountRuntimeStatus>>,
    }

    impl OfficeRuntimeStatusStore for MemoryRuntimeStatusStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(status.account_key.clone(), status.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    struct StubHttp;

    impl OfficeHttpClient for StubHttp {
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

    #[test]
    fn remote_provider_requires_account_key_when_multiple_accounts_share_provider() {
        let local_store: Arc<dyn CalendarStore + Send + Sync> =
            Arc::new(StubCalendarStore::default());
        let credential_store_impl = Arc::new(StubCredentialStore::default());
        credential_store_impl
            .set(&CalendarProviderCredential {
                account_key: "work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "工作".to_string(),
                calendar_id: "default".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token-work".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
            })
            .unwrap();
        credential_store_impl
            .set(&CalendarProviderCredential {
                account_key: "personal".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "personal@example.com".to_string(),
                account_label: "私人".to_string(),
                calendar_id: "default".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token-personal".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 2,
            })
            .unwrap();
        let credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync> =
            credential_store_impl;
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(MockRemoteProvider));
        let service = CalendarService::new(local_store, credential_store, providers);

        let error = service
            .list(
                Some(&mut StubHttp),
                "mock_remote",
                None,
                CalendarQuery::upcoming(0, 10),
            )
            .expect_err("multiple accounts should require account_key");
        assert!(error.to_string().contains("account_key is required"));
    }

    #[test]
    fn remote_provider_uses_office_policy_identity_before_ambiguity_error() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "work".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token-work".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
            })
            .unwrap();
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "personal".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "personal@example.com".to_string(),
                account_label: "Personal".to_string(),
                calendar_id: "personal".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token-personal".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 2,
            })
            .unwrap();
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "work".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        registry.insert(OfficeAccount {
            account_key: "personal".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(MockRemoteProvider));
        let office_service = OfficeService::new(
            registry,
            OfficeSelectionPolicy {
                ask_when_ambiguous: false,
                preferred_identity_class: Some(OfficeAccountIdentityClass::Work),
            },
            Arc::new(StubOfficeCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );
        let service = CalendarService::with_office_service(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
            Some(office_service),
        );
        let mut http = StubHttp;
        let items = service
            .list(
                Some(&mut http),
                "mock_remote",
                None,
                CalendarQuery::upcoming(0, 10),
            )
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            service
                .resolve_account_key_for_provider("mock_remote", None)
                .expect("resolve account key")
                .as_deref(),
            Some("work")
        );
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
            _http: &mut dyn OfficeHttpClient,
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
            _http: &mut dyn OfficeHttpClient,
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
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn update_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn delete_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Ok(true)
        }
    }

    struct FailingRemoteProvider;

    impl CalendarProvider for FailingRemoteProvider {
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
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Err(Error::config(
                "calendar_provider_test",
                "remote list failed",
            ))
        }

        fn get_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Err(Error::config("calendar_provider_test", "remote get failed"))
        }

        fn create_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(Error::config(
                "calendar_provider_test",
                "remote create failed",
            ))
        }

        fn update_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(Error::config(
                "calendar_provider_test",
                "remote update failed",
            ))
        }

        fn delete_event(
            &self,
            _http: &mut dyn OfficeHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Err(Error::config(
                "calendar_provider_test",
                "remote delete failed",
            ))
        }
    }

    fn build_remote_office_service(
        runtime_status_store: Arc<MemoryRuntimeStatusStore>,
        provider: Arc<dyn CalendarProvider>,
    ) -> CalendarService {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "work".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token-work".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
            })
            .unwrap();
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "work".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let mut providers = CalendarProviderRegistry::new();
        providers.register(provider);
        let office_service = OfficeService::new(
            registry,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore::default()),
            runtime_status_store,
        );
        CalendarService::with_office_service(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
            Some(office_service),
        )
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
            .list(None, "local", None, CalendarQuery::upcoming(0, 10))
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "local-1");
    }

    #[test]
    fn service_routes_remote_queries_when_provider_and_credential_exist() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "mock-remote-default".to_string(),
                provider: "mock_remote".to_string(),
                account_id: String::new(),
                account_label: String::new(),
                calendar_id: "default".to_string(),
                username: String::new(),
                app_id: String::new(),
                base_url: String::new(),
                root_path: String::new(),
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
                None,
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

    #[test]
    fn remote_provider_records_runtime_activity_for_remote_operations() {
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service =
            build_remote_office_service(runtime_status_store.clone(), Arc::new(MockRemoteProvider));
        let mut http = StubHttp;

        let _ = service
            .list(
                Some(&mut http),
                "mock_remote",
                None,
                CalendarQuery::upcoming(0, 10),
            )
            .expect("list remote events");
        let status = runtime_status_store
            .get("work")
            .expect("load runtime after list")
            .expect("runtime status after list");
        assert_eq!(status.last_activity_kind, "calendar_list");
        assert!(status.last_activity_ok);
        assert!(status.last_error.is_empty());

        let _ = service
            .get(Some(&mut http), "mock_remote", None, "remote-1")
            .expect("get remote event");
        let status = runtime_status_store
            .get("work")
            .expect("load runtime after get")
            .expect("runtime status after get");
        assert_eq!(status.last_activity_kind, "calendar_get");
        assert!(status.last_activity_ok);

        let event = CalendarEvent {
            id: "remote-2".to_string(),
            title: "New remote meeting".to_string(),
            start_at_unix_secs: 100,
            end_at_unix_secs: 160,
            timezone: String::new(),
            location: String::new(),
            notes: String::new(),
            provider: "mock_remote".to_string(),
            calendar_id: "work".to_string(),
            remote_id: "r2".to_string(),
            status: CalendarEventStatus::Confirmed,
            updated_at: 1,
        };
        let _ = service
            .upsert(Some(&mut http), "mock_remote", None, &event, true)
            .expect("create remote event");
        let status = runtime_status_store
            .get("work")
            .expect("load runtime after create")
            .expect("runtime status after create");
        assert_eq!(status.last_activity_kind, "calendar_create");
        assert!(status.last_activity_ok);

        let _ = service
            .upsert(Some(&mut http), "mock_remote", None, &event, false)
            .expect("update remote event");
        let status = runtime_status_store
            .get("work")
            .expect("load runtime after update")
            .expect("runtime status after update");
        assert_eq!(status.last_activity_kind, "calendar_update");
        assert!(status.last_activity_ok);

        let _ = service
            .delete(Some(&mut http), "mock_remote", None, "remote-2")
            .expect("delete remote event");
        let status = runtime_status_store
            .get("work")
            .expect("load runtime after delete")
            .expect("runtime status after delete");
        assert_eq!(status.last_activity_kind, "calendar_delete");
        assert!(status.last_activity_ok);
        assert!(status.last_error.is_empty());
    }

    #[test]
    fn remote_provider_records_runtime_failure_for_remote_operations() {
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service = build_remote_office_service(
            runtime_status_store.clone(),
            Arc::new(FailingRemoteProvider),
        );
        let mut http = StubHttp;

        let error = service
            .list(
                Some(&mut http),
                "mock_remote",
                None,
                CalendarQuery::upcoming(0, 10),
            )
            .expect_err("failing provider should surface error");
        assert!(error.to_string().contains("remote list failed"));

        let status = runtime_status_store
            .get("work")
            .expect("load runtime after failure")
            .expect("runtime status after failure");
        assert_eq!(status.last_activity_kind, "calendar_list");
        assert!(!status.last_activity_ok);
        assert!(status.last_error.contains("remote list failed"));
    }
}
