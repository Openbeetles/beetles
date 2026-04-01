//! calendar tool: persistent local calendar with provider-aware routing.

use super::http_bridge::ToolContextHttpClient;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarProvider,
    CalendarProviderCredentialStatus, CalendarProviderCredentialStore, CalendarProviderRegistry,
    CalendarQuery, CalendarService, CalendarStore, CALENDAR_PROVIDER_LOCAL,
};

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::util::{current_unix_secs, parse_iso8601};
use serde_json::{json, Value};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static EVENT_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct CalendarTool {
    service: CalendarService,
}

impl CalendarTool {
    pub fn new(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
    ) -> Self {
        Self::with_providers(
            local_store,
            credential_store,
            CalendarProviderRegistry::new(),
        )
    }

    pub fn with_providers(
        local_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
        providers: CalendarProviderRegistry,
    ) -> Self {
        Self {
            service: CalendarService::new(local_store, credential_store, providers),
        }
    }
}

impl Tool for CalendarTool {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn description(&self) -> &'static str {
        "Manage persistent calendar events. Ops: list, get, create, update, delete, provider_status. Provider defaults to local. Times accept Unix seconds or ISO8601."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "description": "Operation: list|get|create|update|delete|provider_status" },
                "provider": { "type": "string", "description": "Calendar provider. Defaults to local." },
                "id": { "type": "string", "description": "Event ID for get/update/delete" },
                "title": { "type": "string", "description": "Event title for create/update" },
                "start_at": { "description": "Start time as Unix seconds or ISO8601", "oneOf": [{ "type": "number" }, { "type": "string" }] },
                "end_at": { "description": "End time as Unix seconds or ISO8601", "oneOf": [{ "type": "number" }, { "type": "string" }] },
                "timezone": { "type": "string", "description": "Optional timezone label" },
                "location": { "type": "string", "description": "Optional location" },
                "notes": { "type": "string", "description": "Optional notes" },
                "calendar_id": { "type": "string", "description": "Optional calendar ID. Defaults to default for local events." },
                "status": { "type": "string", "description": "Optional status for update: confirmed|cancelled" },
                "start_from": { "description": "List query lower bound on start time", "oneOf": [{ "type": "number" }, { "type": "string" }] },
                "start_to": { "description": "List query upper bound on start time", "oneOf": [{ "type": "number" }, { "type": "string" }] },
                "limit": { "type": "integer", "description": "List limit, default 10, max 50" },
                "include_cancelled": { "type": "boolean", "description": "Whether list should include cancelled events" }
            },
            "required": ["op"]
        })
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
                let configured_providers = self
                    .service
                    .list_provider_statuses()?
                    .into_iter()
                    .map(provider_status_to_json)
                    .collect::<Vec<_>>();
                Ok(json!({
                    "op": "provider_status",
                    "local_provider": CALENDAR_PROVIDER_LOCAL,
                    "registered_remote_providers": registered_remote_providers,
                    "configured_providers": configured_providers,
                })
                .to_string())
            }
            "list" => {
                let provider = parse_provider(&obj);
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
                    self.service.list(http, &provider, query)
                })?;
                let count = items.len();
                let items = items.into_iter().map(event_to_json).collect::<Vec<_>>();
                Ok(json!({
                    "op": "list",
                    "provider": provider,
                    "count": count,
                    "items": items,
                })
                .to_string())
            }
            "get" => {
                let provider = parse_provider(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let event = with_calendar_http(&provider, ctx, |http| {
                    self.service.get(http, &provider, id)
                })?
                .ok_or_else(|| Error::config("tool_calendar", "event not found"))?;
                Ok(json!({
                    "op": "get",
                    "provider": provider,
                    "event": event_to_json(event),
                })
                .to_string())
            }
            "create" => {
                let provider = parse_provider(&obj);
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
                    self.service.upsert(http, &provider, &event, true)
                })?;
                Ok(json!({
                    "op": "create",
                    "ok": true,
                    "provider": provider,
                    "event": event_to_json(event),
                })
                .to_string())
            }
            "update" => {
                let provider = parse_provider(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let mut event = with_calendar_http(&provider, ctx, |http| {
                    self.service.get(http, &provider, id)
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
                    return Ok(json!({
                        "op": "update",
                        "ok": false,
                        "provider": provider,
                        "error": "no fields to update",
                    })
                    .to_string());
                }
                event.provider = provider.clone();
                event.updated_at = current_unix_secs();
                let event = normalize_calendar_event(event)?;
                let event = with_calendar_http(&provider, ctx, |http| {
                    self.service.upsert(http, &provider, &event, false)
                })?;
                Ok(json!({
                    "op": "update",
                    "ok": true,
                    "provider": provider,
                    "updated_fields": updated,
                    "event": event_to_json(event),
                })
                .to_string())
            }
            "delete" => {
                let provider = parse_provider(&obj);
                let id = required_str(&obj, "id", "tool_calendar")?;
                let removed = with_calendar_http(&provider, ctx, |http| {
                    self.service.delete(http, &provider, id)
                })?;
                Ok(json!({
                    "op": "delete",
                    "provider": provider,
                    "id": id,
                    "ok": removed,
                })
                .to_string())
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

fn provider_status_to_json(status: CalendarProviderCredentialStatus) -> Value {
    json!({
        "provider": status.provider,
        "account_id": status.account_id,
        "account_label": status.account_label,
        "calendar_id": status.calendar_id,
        "configured": status.configured,
        "has_refresh_token": status.has_refresh_token,
        "expires_at_unix_secs": status.expires_at_unix_secs,
        "updated_at": status.updated_at,
    })
}

fn event_to_json(event: CalendarEvent) -> Value {
    json!({
        "id": event.id,
        "title": event.title,
        "start_at_unix_secs": event.start_at_unix_secs,
        "end_at_unix_secs": event.end_at_unix_secs,
        "timezone": event.timezone,
        "location": event.location,
        "notes": event.notes,
        "provider": event.provider,
        "calendar_id": event.calendar_id,
        "remote_id": event.remote_id,
        "status": match event.status {
            CalendarEventStatus::Confirmed => "confirmed",
            CalendarEventStatus::Cancelled => "cancelled",
        },
        "updated_at": event.updated_at
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarOperation, CalendarProviderCredential, CalendarProviderCredentialStore,
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
                .map(CalendarProviderCredential::status)
                .collect())
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
            _credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Ok(Vec::new())
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
                provider: "mock_remote".to_string(),
                account_id: "acc-1".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "team".to_string(),
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
        assert_eq!(
            payload["configured_providers"][0]["has_refresh_token"],
            true
        );
    }
}
