//! calendar tool: persistent local calendar with provider-aware routing.

use super::http_bridge::ToolContextHttpClient;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarProviderCredentialStatus,
    CalendarProviderCredentialStore, CalendarProviderRegistry, CalendarQuery, CalendarService,
    CalendarStore, CALENDAR_PROVIDER_LOCAL,
};

use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAccountRuntimeStatus, OfficeAuthoritySource, OfficeService,
    SnapshotOfficeAuthoritySource,
};
use crate::tools::{parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata};
use crate::util::{current_unix_secs, parse_iso8601};
use serde::Serialize;
use serde_json::Value;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static EVENT_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct CalendarTool {
    service: CalendarService,
}

#[derive(Serialize)]
struct CalendarProviderStatusResponse {
    op: &'static str,
    local_provider: &'static str,
    registered_remote_providers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_calendar_account_key: Option<String>,
    configured_providers: Vec<CalendarProviderCredentialStatus>,
    account_assessments: Vec<OfficeAccountAssessment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    office_runtime_statuses: Vec<OfficeAccountRuntimeStatus>,
}

#[derive(Serialize)]
struct CalendarListResponse {
    op: &'static str,
    provider: String,
    count: usize,
    items: Vec<CalendarEvent>,
}

#[derive(Serialize)]
struct CalendarGetResponse {
    op: &'static str,
    provider: String,
    event: CalendarEvent,
}

#[derive(Serialize)]
struct CalendarMutationResponse {
    op: &'static str,
    ok: bool,
    provider: String,
    event: CalendarEvent,
}

#[derive(Serialize)]
struct CalendarUpdateResponse {
    op: &'static str,
    ok: bool,
    provider: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    updated_fields: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    event: Option<CalendarEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
}

#[derive(Serialize)]
struct CalendarDeleteResponse<'a> {
    op: &'static str,
    provider: String,
    id: &'a str,
    ok: bool,
}

impl CalendarTool {
    pub fn new(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
    ) -> Self {
        Self::with_runtime(
            local_store,
            credential_store,
            CalendarProviderRegistry::new(),
            None,
        )
    }

    pub fn with_providers(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
    ) -> Self {
        Self::with_runtime(local_store, credential_store, providers, None)
    }

    pub fn with_office_service(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            local_store,
            credential_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    pub fn with_office_authority(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        Self::with_runtime(
            local_store,
            credential_store,
            providers,
            Some(office_authority),
        )
    }

    fn with_runtime(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Option<Arc<dyn OfficeAuthoritySource + Send + Sync>>,
    ) -> Self {
        Self {
            service: CalendarService::with_office_authority(
                local_store,
                credential_store,
                providers,
                office_authority,
            ),
        }
    }
}

impl Tool for CalendarTool {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn description(&self) -> &'static str {
        "Manage persistent calendar events. Ops: list, get, create, update, delete, provider_status. Provider defaults to local. Remote providers can route by explicit account_key or office calendar defaults when multiple accounts exist. Times accept Unix seconds or ISO8601."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: list|get|create|update|delete|provider_status"},"provider":{"type":"string","description":"Calendar provider. Defaults to local."},"account_key":{"type":"string","description":"Optional remote account key when a provider has multiple configured accounts."},"id":{"type":"string","description":"Event ID for get/update/delete"},"title":{"type":"string","description":"Event title for create/update"},"start_at":{"description":"Start time as Unix seconds or ISO8601","oneOf":[{"type":"number"},{"type":"string"}]},"end_at":{"description":"End time as Unix seconds or ISO8601","oneOf":[{"type":"number"},{"type":"string"}]},"timezone":{"type":"string","description":"Optional timezone label"},"location":{"type":"string","description":"Optional location"},"notes":{"type":"string","description":"Optional notes"},"calendar_id":{"type":"string","description":"Optional calendar ID. Defaults to default for local events."},"status":{"type":"string","description":"Optional status for update: confirmed|cancelled"},"start_from":{"description":"List query lower bound on start time","oneOf":[{"type":"number"},{"type":"string"}]},"start_to":{"description":"List query upper bound on start time","oneOf":[{"type":"number"},{"type":"string"}]},"limit":{"type":"integer","description":"List limit, default 10, max 50"},"include_cancelled":{"type":"boolean","description":"Whether list should include cancelled events"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_calendar")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_calendar", "missing op"))?;
        match op {
            "provider_status" => {
                let registered_remote_providers = self.service.provider_names();
                let registered_remote_providers = registered_remote_providers
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                let configured_providers = self.service.list_provider_statuses()?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarProviderStatusResponse {
                        op: "provider_status",
                        local_provider: CALENDAR_PROVIDER_LOCAL,
                        registered_remote_providers,
                        default_calendar_account_key: self.service.office_default_account_key()?,
                        configured_providers,
                        account_assessments: self.service.office_account_assessments()?,
                        office_runtime_statuses: self.service.office_runtime_statuses()?,
                    },
                )
            }
            "list" => {
                let provider = parse_provider(&obj);
                let account_key = parse_account_key(&obj);
                let limit = obj.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
                let query = CalendarQuery {
                    start_from_unix_secs: parse_optional_time(obj.get("start_from"))?
                        .or_else(|| Some(current_unix_secs())),
                    start_to_unix_secs: parse_optional_time(obj.get("start_to"))?,
                    limit: limit.clamp(1, 50),
                    include_cancelled: obj
                        .get("include_cancelled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                };
                let items = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .list(http, &provider, account_key.as_deref(), query)
                })?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarListResponse {
                        op: "list",
                        provider,
                        count: items.len(),
                        items,
                    },
                )
            }
            "get" => {
                let provider = parse_provider(&obj);
                let account_key = parse_account_key(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let event = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .get(http, &provider, account_key.as_deref(), id)
                })?
                .ok_or_else(|| Error::config("tool_calendar", "event not found"))?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarGetResponse {
                        op: "get",
                        provider,
                        event,
                    },
                )
            }
            "create" => {
                let provider = parse_provider(&obj);
                let account_key = parse_account_key(&obj);
                let title = required_str(&obj, "title", "tool_calendar")?;
                let start_at_unix_secs = parse_required_time(obj.get("start_at"), "start_at")?;
                let end_at_unix_secs = parse_required_time(obj.get("end_at"), "end_at")?;
                let now_secs = current_unix_secs();
                let event = normalize_calendar_event(CalendarEvent {
                    id: build_event_id(title, start_at_unix_secs),
                    title: title.to_string(),
                    start_at_unix_secs,
                    end_at_unix_secs,
                    timezone: optional_str(&obj, "timezone"),
                    location: optional_str(&obj, "location"),
                    notes: optional_str(&obj, "notes"),
                    provider: provider.clone(),
                    calendar_id: default_calendar_id(&provider, optional_str(&obj, "calendar_id")),
                    remote_id: String::new(),
                    status: CalendarEventStatus::Confirmed,
                    updated_at: now_secs,
                })?;
                let event = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .upsert(http, &provider, account_key.as_deref(), &event, true)
                })?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarMutationResponse {
                        op: "create",
                        ok: true,
                        provider,
                        event,
                    },
                )
            }
            "update" => {
                let provider = parse_provider(&obj);
                let account_key = parse_account_key(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let mut event = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .get(http, &provider, account_key.as_deref(), id)
                })?
                .ok_or_else(|| Error::config("tool_calendar", "event not found"))?;
                let mut updated = Vec::new();
                if let Some(title) = obj.get("title").and_then(Value::as_str) {
                    event.title = title.to_string();
                    updated.push("title");
                }
                if let Some(start_at) = obj.get("start_at") {
                    event.start_at_unix_secs = parse_required_time(Some(start_at), "start_at")?;
                    updated.push("start_at");
                }
                if let Some(end_at) = obj.get("end_at") {
                    event.end_at_unix_secs = parse_required_time(Some(end_at), "end_at")?;
                    updated.push("end_at");
                }
                if let Some(timezone) = obj.get("timezone").and_then(Value::as_str) {
                    event.timezone = timezone.to_string();
                    updated.push("timezone");
                }
                if let Some(location) = obj.get("location").and_then(Value::as_str) {
                    event.location = location.to_string();
                    updated.push("location");
                }
                if let Some(notes) = obj.get("notes").and_then(Value::as_str) {
                    event.notes = notes.to_string();
                    updated.push("notes");
                }
                if let Some(calendar_id) = obj.get("calendar_id").and_then(Value::as_str) {
                    event.calendar_id = calendar_id.to_string();
                    updated.push("calendar_id");
                }
                if let Some(status) = obj.get("status") {
                    event.status = parse_status(status)?;
                    updated.push("status");
                }
                if updated.is_empty() {
                    return serialize_tool_output(
                        "tool_calendar",
                        &CalendarUpdateResponse {
                            op: "update",
                            ok: false,
                            provider,
                            updated_fields: Vec::new(),
                            event: None,
                            error: Some("no fields to update"),
                        },
                    );
                }
                event.provider = provider.clone();
                event.updated_at = current_unix_secs();
                let event = normalize_calendar_event(event)?;
                let event = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .upsert(http, &provider, account_key.as_deref(), &event, false)
                })?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarUpdateResponse {
                        op: "update",
                        ok: true,
                        provider,
                        updated_fields: updated,
                        event: Some(event),
                        error: None,
                    },
                )
            }
            "delete" => {
                let provider = parse_provider(&obj);
                let account_key = parse_account_key(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let removed = with_calendar_http(&provider, ctx, |http| {
                    self.service
                        .delete(http, &provider, account_key.as_deref(), id)
                })?;
                serialize_tool_output(
                    "tool_calendar",
                    &CalendarDeleteResponse {
                        op: "delete",
                        provider,
                        id,
                        ok: removed,
                    },
                )
            }
            _ => Err(Error::config(
                "tool_calendar",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

fn with_calendar_http<T>(
    provider: &str,
    ctx: &mut dyn ToolContext,
    f: impl for<'a> FnOnce(Option<&'a mut dyn crate::calendar::CalendarHttpClient>) -> Result<T>,
) -> Result<T> {
    if provider == CALENDAR_PROVIDER_LOCAL {
        return f(None);
    }
    let mut http = ToolContextHttpClient::new(ctx);
    f(Some(&mut http))
}

fn parse_provider(obj: &serde_json::Map<String, Value>) -> String {
    let provider = obj
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if provider.is_empty() {
        CALENDAR_PROVIDER_LOCAL.to_string()
    } else {
        provider.to_string()
    }
}

fn parse_account_key(obj: &serde_json::Map<String, Value>) -> Option<String> {
    obj.get("account_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn default_calendar_id(provider: &str, calendar_id: String) -> String {
    if !calendar_id.trim().is_empty() {
        return calendar_id;
    }
    if provider == CALENDAR_PROVIDER_LOCAL {
        "default".to_string()
    } else {
        String::new()
    }
}

fn parse_optional_time(value: Option<&Value>) -> Result<Option<u64>> {
    value
        .map(|v| parse_required_time(Some(v), "time"))
        .transpose()
}

fn parse_required_time(value: Option<&Value>, field: &'static str) -> Result<u64> {
    let value =
        value.ok_or_else(|| Error::config("tool_calendar", format!("missing {}", field)))?;
    match value {
        Value::Number(n) => n.as_u64().ok_or_else(|| {
            Error::config("tool_calendar", format!("{} must be non-negative", field))
        }),
        Value::String(s) => parse_iso8601(s).ok_or_else(|| {
            Error::config(
                "tool_calendar",
                format!("{} must be Unix seconds or ISO8601", field),
            )
        }),
        _ => Err(Error::config(
            "tool_calendar",
            format!("{} must be number or string", field),
        )),
    }
}

fn parse_status(value: &Value) -> Result<CalendarEventStatus> {
    match value.as_str() {
        Some("confirmed") => Ok(CalendarEventStatus::Confirmed),
        Some("cancelled") => Ok(CalendarEventStatus::Cancelled),
        _ => Err(Error::config(
            "tool_calendar",
            "status must be confirmed or cancelled",
        )),
    }
}

fn required_str<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &'static str,
    stage: &'static str,
) -> Result<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::config(stage, format!("missing {}", key)))
}

fn optional_str(obj: &serde_json::Map<String, Value>, key: &'static str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn build_event_id(title: &str, start_at_unix_secs: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut hasher);
    start_at_unix_secs.hash(&mut hasher);
    current_unix_secs().hash(&mut hasher);
    let short = (hasher.finish() & 0xffff) as u16;
    let seq = EVENT_SEQ.fetch_add(1, Ordering::Relaxed) & 0xffff;
    format!("cal_{}_{}_{:04x}", start_at_unix_secs, seq, short)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarOperation, CalendarProvider, CalendarProviderCredential,
        CalendarProviderCredentialStore,
    };
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry,
        OfficeAccountRuntimeStatus, OfficeCapability, OfficeCapabilityBinding, OfficeCredential,
        OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeSelectionPolicy, OfficeService,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubCalendarStore {
        events: Mutex<HashMap<String, CalendarEvent>>,
    }

    impl CalendarStore for StubCalendarStore {
        fn list(&self, query: CalendarQuery) -> Result<Vec<CalendarEvent>> {
            let items = self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            Ok(crate::calendar::filter_calendar_events(items, query))
        }

        fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
            Ok(self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned())
        }

        fn upsert(&self, event: &CalendarEvent) -> Result<()> {
            let event = crate::calendar::normalize_calendar_event(event.clone())?;
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(event.id.clone(), event);
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self
                .events
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
                .map(CalendarProviderCredential::status)
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
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }

        fn set(&self, _status: &OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn patch_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn put_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn delete_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    struct StubProvider;

    impl CalendarProvider for StubProvider {
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
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Ok(vec![CalendarEvent {
                id: format!("event-for-{}", credential.account_key),
                title: credential.account_label.clone(),
                start_at_unix_secs: 100,
                end_at_unix_secs: 160,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: credential.provider.clone(),
                calendar_id: credential.calendar_id.clone(),
                remote_id: credential.account_key.clone(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 1,
            }])
        }

        fn get_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Ok(None)
        }

        fn create_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn update_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Ok(event.clone())
        }

        fn delete_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Ok(true)
        }
    }

    #[test]
    fn calendar_tool_create_and_get_roundtrip() {
        let tool = CalendarTool::new(
            Arc::new(StubCalendarStore::default()),
            Arc::new(StubCredentialStore::default()),
        );
        let mut ctx = DummyCtx;
        let created = tool
            .execute(
                r#"{"op":"create","title":"周会","start_at":1700000000,"end_at":1700003600,"location":"A301"}"#,
                &mut ctx,
            )
            .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let id = created["event"]["id"].as_str().unwrap();

        let fetched = tool
            .execute(&format!(r#"{{"op":"get","id":"{}"}}"#, id), &mut ctx)
            .unwrap();
        let fetched: Value = serde_json::from_str(&fetched).unwrap();
        assert_eq!(fetched["event"]["title"], "周会");
        assert_eq!(fetched["event"]["location"], "A301");
        assert_eq!(fetched["event"]["provider"], "local");
    }

    #[test]
    fn calendar_tool_update_returns_changed_fields() {
        let store = Arc::new(StubCalendarStore::default());
        store
            .upsert(&CalendarEvent {
                id: "cal_1".to_string(),
                title: "旧标题".to_string(),
                start_at_unix_secs: 1700000000,
                end_at_unix_secs: 1700003600,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: CALENDAR_PROVIDER_LOCAL.to_string(),
                calendar_id: "default".to_string(),
                remote_id: String::new(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 1,
            })
            .unwrap();
        let tool = CalendarTool::new(store, Arc::new(StubCredentialStore::default()));
        let mut ctx = DummyCtx;
        let updated = tool
            .execute(
                r#"{"op":"update","id":"cal_1","title":"新标题","status":"cancelled"}"#,
                &mut ctx,
            )
            .unwrap();
        let updated: Value = serde_json::from_str(&updated).unwrap();
        assert_eq!(updated["ok"], true);
        assert_eq!(updated["event"]["title"], "新标题");
        assert_eq!(updated["event"]["status"], "cancelled");
    }

    #[test]
    fn calendar_tool_provider_status_reports_registered_and_configured() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "mock-work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "acc-1".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "team".to_string(),
                username: String::new(),
                base_url: String::new(),
                root_path: String::new(),
                access_token: "token".to_string(),
                refresh_token: "refresh".to_string(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 123,
                updated_at: 7,
            })
            .unwrap();
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(StubProvider));
        let tool = CalendarTool::with_providers(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
        );
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["local_provider"], "local");
        assert_eq!(payload["registered_remote_providers"][0], "mock_remote");
        assert_eq!(payload["configured_providers"][0]["account_label"], "Work");
        assert!(payload["default_calendar_account_key"].is_null());
        assert_eq!(
            payload["configured_providers"][0]["has_refresh_token"],
            true
        );
        assert!(payload["account_assessments"].as_array().is_some());
        assert!(payload["account_assessments"]
            .as_array()
            .expect("account assessments array")
            .is_empty());
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn calendar_tool_provider_status_exposes_caldav_transport_shape() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "calendar-work".to_string(),
                provider: "caldav".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "Work Calendar".to_string(),
                calendar_id: "team".to_string(),
                username: "caldav-user".to_string(),
                base_url: "https://dav.example.com/remote.php/dav/calendars".to_string(),
                root_path: "/work".to_string(),
                access_token: "app-password".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 7,
            })
            .unwrap();
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(crate::calendar::providers::caldav::CalDavProvider));
        let tool = CalendarTool::with_providers(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
        );
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["registered_remote_providers"][0], "caldav");
        assert_eq!(payload["configured_providers"][0]["provider"], "caldav");
        assert_eq!(payload["configured_providers"][0]["configured"], true);
        assert_eq!(
            payload["configured_providers"][0]["base_url"],
            "https://dav.example.com/remote.php/dav/calendars"
        );
        assert_eq!(payload["configured_providers"][0]["root_path"], "/work");
        assert!(payload["account_assessments"]
            .as_array()
            .expect("account assessments array")
            .is_empty());
    }

    #[test]
    fn calendar_tool_routes_remote_provider_via_office_default_account() {
        let credential_store = Arc::new(StubCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "calendar-work".to_string(),
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
                account_key: "calendar-personal".to_string(),
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
        let mut providers = CalendarProviderRegistry::new();
        providers.register(Arc::new(StubProvider));
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "calendar-work".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        registry.insert(OfficeAccount {
            account_key: "calendar-personal".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "personal@example.com".to_string(),
            account_label: "Personal".to_string(),
            identity_class: OfficeAccountIdentityClass::Personal,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "calendar-work".to_string());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore),
        );
        let tool = CalendarTool::with_office_service(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
            office_service,
        );
        let mut ctx = DummyCtx;
        let payload = tool
            .execute(r#"{"op":"list","provider":"mock_remote"}"#, &mut ctx)
            .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["items"][0]["id"], "event-for-calendar-work");

        let status = tool
            .execute(r#"{"op":"provider_status"}"#, &mut ctx)
            .unwrap();
        let status: Value = serde_json::from_str(&status).unwrap();
        assert_eq!(status["default_calendar_account_key"], "calendar-work");
        assert_eq!(
            status["account_assessments"][0]["account_key"],
            "calendar-personal"
        );
        assert_eq!(
            status["account_assessments"][1]["account_key"],
            "calendar-work"
        );
    }
}
