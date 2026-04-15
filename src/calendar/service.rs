use crate::calendar::{
    CalendarEvent, CalendarHttpClient, CalendarOperation, CalendarProviderCredentialStore,
    CalendarProviderRegistry, CalendarQuery, CalendarStore, CALENDAR_PROVIDER_LOCAL,
};
use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeCapability,
    OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::util::current_unix_secs;
use std::sync::Arc;

pub struct CalendarService {
    local_store: Arc<dyn CalendarStore + Send + Sync>,
    credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
    providers: CalendarProviderRegistry,
    office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
}

impl CalendarService {
    pub fn new(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
    ) -> Self {
        Self::with_office_authority(local_store, credential_store, providers, None)
    }

    pub fn with_office_service(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_service: Option<OfficeService>,
    ) -> Self {
        Self::with_office_authority(
            local_store,
            credential_store,
            providers,
            office_service.map(|office| {
                Arc::new(SnapshotOfficeAuthoritySource::new(office))
                    as Arc<dyn OfficeAuthoritySource + Send + Sync>
            }),
        )
    }

    pub fn with_office_authority(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            local_store,
            credential_store,
            providers,
            office_authority,
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

    pub fn office_default_account_key(&self) -> Result<Option<String>> {
        Ok(self
            .load_office_service()?
            .and_then(|service| service.default_account_key(OfficeCapability::Calendar)))
    }

    pub fn resolve_account_key_for_provider(
        &self,
        provider: &str,
        account_key: Option<&str>,
    ) -> Result<Option<String>> {
        if is_local_provider(provider) {
            return Ok(None);
        }
        self.resolve_account_key(provider, account_key).map(Some)
    }

    pub fn office_runtime_statuses(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        let calendar_accounts = service
            .accounts_for_capability(OfficeCapability::Calendar)
            .into_iter()
            .map(|account| account.account_key)
            .collect::<std::collections::BTreeSet<_>>();
        Ok(service
            .list_runtime_statuses()?
            .into_iter()
            .filter(|status| calendar_accounts.contains(&status.account_key))
            .collect())
    }

    pub fn office_account_assessments(&self) -> Result<Vec<OfficeAccountAssessment>> {
        let Some(service) = self.load_office_service()? else {
            return Ok(Vec::new());
        };
        service.assess_capability_accounts(OfficeCapability::Calendar, |provider_kind| {
            self.providers.get(provider_kind).is_some()
        })
    }

    pub fn list(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        query: CalendarQuery,
    ) -> Result<Vec<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.list(query);
        }
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, CalendarOperation::List)?;
        let http = require_http(provider, http)?;
        let result = provider_impl.list_events(http, &credential, query);
        self.record_runtime_activity(
            &credential.account_key,
            "calendar_list",
            result.as_ref().err(),
        );
        result
    }

    pub fn get(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<Option<CalendarEvent>> {
        if is_local_provider(provider) {
            return self.local_store.get(id);
        }
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, CalendarOperation::Get)?;
        let http = require_http(provider, http)?;
        let result = provider_impl.get_event(http, &credential, id);
        self.record_runtime_activity(
            &credential.account_key,
            "calendar_get",
            result.as_ref().err(),
        );
        result
    }

    pub fn upsert(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        account_key: Option<&str>,
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
        let (provider_impl, credential) = self.resolve_remote(provider, account_key, op)?;
        let http = require_http(provider, http)?;
        let result = if is_create {
            provider_impl.create_event(http, &credential, event)
        } else {
            provider_impl.update_event(http, &credential, event)
        };
        self.record_runtime_activity(
            &credential.account_key,
            if is_create {
                "calendar_create"
            } else {
                "calendar_update"
            },
            result.as_ref().err(),
        );
        result
    }

    pub fn delete(
        &self,
        http: Option<&mut dyn CalendarHttpClient>,
        provider: &str,
        account_key: Option<&str>,
        id: &str,
    ) -> Result<bool> {
        if is_local_provider(provider) {
            return self.local_store.delete(id);
        }
        let (provider_impl, credential) =
            self.resolve_remote(provider, account_key, CalendarOperation::Delete)?;
        let http = require_http(provider, http)?;
        let result = provider_impl.delete_event(http, &credential, id);
        self.record_runtime_activity(
            &credential.account_key,
            "calendar_delete",
            result.as_ref().err(),
        );
        result
    }

    fn resolve_remote(
        &self,
        provider: &str,
        account_key: Option<&str>,
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
        let account_key = self.resolve_account_key(provider, account_key)?;
        let credential = self.credential_store.get(&account_key)?.ok_or_else(|| {
            Error::config(
                "calendar_provider",
                format!(
                    "provider '{}' has no configured credential for account '{}'",
                    provider, account_key
                ),
            )
        })?;
        if credential.provider != provider {
            return Err(Error::config(
                "calendar_provider",
                format!(
                    "account '{}' is configured for provider '{}', not '{}'",
                    account_key, credential.provider, provider
                ),
            ));
        }
        if credential.access_token.trim().is_empty() {
            return Err(Error::config(
                "calendar_provider",
                format!("provider '{}' credential is missing access_token", provider),
            ));
        }
        Ok((provider_impl, credential))
    }

    fn resolve_account_key(&self, provider: &str, account_key: Option<&str>) -> Result<String> {
        if let Some(account_key) = account_key.filter(|value| !value.trim().is_empty()) {
            return Ok(account_key.to_string());
        }
        if let Some(office_service) = self.load_office_service()? {
            if let Some(account_key) = resolve_office_default_account_key(
                &office_service,
                self.credential_store.as_ref(),
                provider,
            )? {
                return Ok(account_key);
            }
        }
        let mut keys = self
            .credential_store
            .find_account_keys_by_provider(provider)?;
        keys.sort();
        match keys.len() {
            0 => Err(Error::config(
                "calendar_provider",
                format!("provider '{}' has no configured credential", provider),
            )),
            1 => Ok(keys.remove(0)),
            _ => Err(Error::config(
                "calendar_provider",
                format!(
                    "provider '{}' has multiple configured accounts; account_key is required",
                    provider
                ),
            )),
        }
    }

    fn load_office_service(&self) -> Result<Option<OfficeService>> {
        self.office_authority
            .as_ref()
            .map(|authority| authority.load())
            .transpose()
    }

    fn record_runtime_activity(
        &self,
        account_key: &str,
        activity_kind: &'static str,
        error: Option<&Error>,
    ) {
        let Some(office_service) = self.load_office_service().unwrap_or_else(|load_error| {
            log::warn!(
                "[calendar_runtime] failed to load office authority for {}: {}",
                account_key,
                load_error
            );
            None
        }) else {
            return;
        };
        let now = current_unix_secs();
        let mut status = match office_service.runtime_status(account_key) {
            Ok(Some(status)) => status,
            Ok(None) => OfficeAccountRuntimeStatus {
                account_key: account_key.to_string(),
                ..OfficeAccountRuntimeStatus::default()
            },
            Err(load_error) => {
                log::warn!(
                    "[calendar_runtime] failed to load runtime status for {}: {}",
                    account_key,
                    load_error
                );
                return;
            }
        };
        status.account_key = account_key.to_string();
        status.last_activity_kind = activity_kind.to_string();
        status.last_activity_ok = error.is_none();
        status.last_activity_at_unix_secs = now;
        status.updated_at = now;
        if let Some(error) = error {
            status.last_error = error.to_string();
        } else {
            status.last_error.clear();
            status.probe_ok = true;
        }
        if let Err(store_error) = office_service.set_runtime_status(&status) {
            log::warn!(
                "[calendar_runtime] failed to persist runtime status for {}: {}",
                account_key,
                store_error
            );
        }
    }
}

fn resolve_office_default_account_key(
    office_service: &OfficeService,
    credential_store: &(dyn CalendarProviderCredentialStore + Send + Sync),
    provider: &str,
) -> Result<Option<String>> {
    let Some(account_key) = office_service.default_account_key(OfficeCapability::Calendar) else {
        return Ok(None);
    };
    let Some(credential) = credential_store.get(&account_key)? else {
        return Err(Error::config(
            "calendar_provider",
            format!(
                "office-selected calendar account '{}' has no configured credential",
                account_key
            ),
        ));
    };
    if credential.provider == provider {
        Ok(Some(account_key))
    } else {
        Ok(None)
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
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry,
        OfficeAccountRuntimeStatus, OfficeCapability, OfficeCapabilityBinding, OfficeCredential,
        OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeSelectionPolicy,
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
    fn remote_provider_uses_office_default_account_before_ambiguity_error() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "work".to_string(),
                username: String::new(),
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
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "work".to_string());
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(MockRemoteProvider));
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
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
                .office_default_account_key()
                .expect("default account key")
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
            _http: &mut dyn CalendarHttpClient,
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
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Err(Error::config("calendar_provider_test", "remote get failed"))
        }

        fn create_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
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
            _http: &mut dyn CalendarHttpClient,
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
            _http: &mut dyn CalendarHttpClient,
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
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "work".to_string());
        let mut providers = CalendarProviderRegistry::new();
        providers.register(provider);
        let office_service = OfficeService::new(
            registry,
            binding,
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
