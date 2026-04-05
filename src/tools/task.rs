//! task tool: durable personal task management with optional local calendar sync.

use crate::calendar::{
    CALENDAR_PROVIDER_LOCAL, CalendarEvent, CalendarEventStatus, CalendarProviderCredentialStore,
    CalendarProviderRegistry, CalendarService, CalendarStore,
};
use crate::error::{Error, Result};
use crate::task::{TaskItem, TaskPriority, TaskQuery, TaskStatus, TaskStore, normalize_task_item};
use crate::tools::{Tool, ToolContext, ToolMetadata, parse_tool_args, serialize_tool_output};
use crate::util::{current_unix_secs, parse_iso8601};
use serde::Serialize;
use serde_json::Value;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

static TASK_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct TaskTool {
    store: Arc<dyn TaskStore + Send + Sync>,
    calendar_service: CalendarService,
}

#[derive(Serialize)]
struct TaskListResponse {
    op: &'static str,
    count: usize,
    tasks: Vec<TaskItem>,
}

#[derive(Serialize)]
struct TaskGetResponse {
    op: &'static str,
    task: TaskItem,
}

#[derive(Serialize)]
struct TaskMutationResponse {
    op: &'static str,
    ok: bool,
    task: TaskItem,
}

#[derive(Serialize)]
struct TaskUpdateResponse {
    op: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    updated_fields: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<TaskItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'static str>,
}

#[derive(Serialize)]
struct TaskDeleteResponse<'a> {
    op: &'static str,
    id: &'a str,
    ok: bool,
}

impl TaskTool {
    pub fn new(
        store: Arc<dyn TaskStore + Send + Sync>,
        calendar_store: Arc<dyn CalendarStore + Send + Sync>,
        credential_store: Arc<dyn CalendarProviderCredentialStore + Send + Sync>,
    ) -> Self {
        Self {
            store,
            calendar_service: CalendarService::new(
                calendar_store,
                credential_store,
                CalendarProviderRegistry::new(),
            ),
        }
    }
}

impl Tool for TaskTool {
    fn name(&self) -> &'static str {
        "task"
    }

    fn description(&self) -> &'static str {
        "Manage durable tasks. Ops: list, get, create, update, complete, delete. Supports due_at reminders and optional local calendar sync."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: list|get|create|update|complete|delete"},"id":{"type":"string","description":"Task ID for get/update/complete/delete"},"title":{"type":"string","description":"Task title"},"detail":{"type":"string","description":"Optional task detail"},"project":{"type":"string","description":"Optional project/group label"},"status":{"type":"string","description":"Task status: open|in_progress|completed|cancelled"},"priority":{"type":"string","description":"Task priority: low|normal|high"},"due_at":{"description":"Optional due reminder time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_start_at":{"description":"Optional local calendar start time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_end_at":{"description":"Optional local calendar end time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_timezone":{"type":"string","description":"Optional local calendar timezone label"},"calendar_location":{"type":"string","description":"Optional local calendar location"},"calendar_notes":{"type":"string","description":"Optional local calendar notes"},"clear_calendar":{"type":"boolean","description":"Remove linked local calendar event and clear calendar fields"},"limit":{"type":"integer","description":"List limit, default 20, max 50"},"include_completed":{"type":"boolean","description":"Whether list should include completed/cancelled tasks"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_task")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_task", "missing op"))?;
        let (channel, chat_id) = require_session_scope(ctx)?;
        match op {
            "list" => {
                let query = TaskQuery {
                    status: obj.get("status").map(parse_status).transpose()?,
                    project: optional_string(obj.get("project")),
                    include_completed: obj
                        .get("include_completed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    limit: obj.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize,
                };
                let tasks = self.store.list(channel, chat_id, query)?;
                serialize_tool_output(
                    "tool_task",
                    &TaskListResponse {
                        op: "list",
                        count: tasks.len(),
                        tasks,
                    },
                )
            }
            "get" => {
                let id = required_string(&obj, "id")?;
                let task = self
                    .store
                    .get(channel, chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                serialize_tool_output("tool_task", &TaskGetResponse { op: "get", task })
            }
            "create" => {
                let now_secs = current_unix_secs();
                let mut task = TaskItem {
                    id: build_task_id(required_string(&obj, "title")?),
                    channel: channel.to_string(),
                    chat_id: chat_id.to_string(),
                    title: required_string(&obj, "title")?.to_string(),
                    detail: optional_string(obj.get("detail")),
                    project: optional_string(obj.get("project")),
                    status: obj
                        .get("status")
                        .map(parse_status)
                        .transpose()?
                        .unwrap_or(TaskStatus::Open),
                    priority: obj
                        .get("priority")
                        .map(parse_priority)
                        .transpose()?
                        .unwrap_or(TaskPriority::Normal),
                    due_at_unix_secs: parse_nullable_time(obj.get("due_at"), "due_at")?
                        .unwrap_or(0),
                    due_notified_at_unix_secs: 0,
                    calendar_event_id: String::new(),
                    calendar_start_at_unix_secs: parse_nullable_time(
                        obj.get("calendar_start_at"),
                        "calendar_start_at",
                    )?
                    .unwrap_or(0),
                    calendar_end_at_unix_secs: parse_nullable_time(
                        obj.get("calendar_end_at"),
                        "calendar_end_at",
                    )?
                    .unwrap_or(0),
                    calendar_timezone: optional_string(obj.get("calendar_timezone")),
                    calendar_location: optional_string(obj.get("calendar_location")),
                    calendar_notes: optional_string(obj.get("calendar_notes")),
                    completed_at_unix_secs: 0,
                    updated_at: now_secs,
                };
                if task.status == TaskStatus::Completed {
                    task.completed_at_unix_secs = now_secs;
                }
                task = normalize_task_item(task)?;
                self.sync_local_calendar(&mut task, now_secs)?;
                self.store.upsert(&task)?;
                serialize_tool_output(
                    "tool_task",
                    &TaskMutationResponse {
                        op: "create",
                        ok: true,
                        task,
                    },
                )
            }
            "update" => {
                let now_secs = current_unix_secs();
                let id = required_string(&obj, "id")?;
                let mut task = self
                    .store
                    .get(channel, chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                let mut updated = Vec::new();
                if let Some(title) = obj.get("title").and_then(Value::as_str) {
                    task.title = title.to_string();
                    updated.push("title");
                }
                if obj.contains_key("detail") {
                    task.detail = nullable_string(obj.get("detail"));
                    updated.push("detail");
                }
                if obj.contains_key("project") {
                    task.project = nullable_string(obj.get("project"));
                    updated.push("project");
                }
                if let Some(status) = obj.get("status") {
                    task.status = parse_status(status)?;
                    updated.push("status");
                }
                if let Some(priority) = obj.get("priority") {
                    task.priority = parse_priority(priority)?;
                    updated.push("priority");
                }
                if obj.contains_key("due_at") {
                    task.due_at_unix_secs =
                        parse_nullable_time(obj.get("due_at"), "due_at")?.unwrap_or(0);
                    task.due_notified_at_unix_secs = 0;
                    updated.push("due_at");
                }
                if obj.contains_key("calendar_start_at") {
                    task.calendar_start_at_unix_secs =
                        parse_nullable_time(obj.get("calendar_start_at"), "calendar_start_at")?
                            .unwrap_or(0);
                    updated.push("calendar_start_at");
                }
                if obj.contains_key("calendar_end_at") {
                    task.calendar_end_at_unix_secs =
                        parse_nullable_time(obj.get("calendar_end_at"), "calendar_end_at")?
                            .unwrap_or(0);
                    updated.push("calendar_end_at");
                }
                if obj.contains_key("calendar_timezone") {
                    task.calendar_timezone = nullable_string(obj.get("calendar_timezone"));
                    updated.push("calendar_timezone");
                }
                if obj.contains_key("calendar_location") {
                    task.calendar_location = nullable_string(obj.get("calendar_location"));
                    updated.push("calendar_location");
                }
                if obj.contains_key("calendar_notes") {
                    task.calendar_notes = nullable_string(obj.get("calendar_notes"));
                    updated.push("calendar_notes");
                }
                if obj
                    .get("clear_calendar")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    self.delete_local_calendar(&task.calendar_event_id)?;
                    task.calendar_event_id.clear();
                    task.calendar_start_at_unix_secs = 0;
                    task.calendar_end_at_unix_secs = 0;
                    task.calendar_timezone.clear();
                    task.calendar_location.clear();
                    task.calendar_notes.clear();
                    updated.push("clear_calendar");
                }
                if updated.is_empty() {
                    return serialize_tool_output(
                        "tool_task",
                        &TaskUpdateResponse {
                            op: "update",
                            ok: false,
                            updated_fields: Vec::new(),
                            task: None,
                            error: Some("no fields to update"),
                        },
                    );
                }
                if task.status == TaskStatus::Completed {
                    task.completed_at_unix_secs = now_secs;
                } else {
                    task.completed_at_unix_secs = 0;
                }
                task.updated_at = now_secs;
                task = normalize_task_item(task)?;
                self.sync_local_calendar(&mut task, now_secs)?;
                self.store.upsert(&task)?;
                serialize_tool_output(
                    "tool_task",
                    &TaskUpdateResponse {
                        op: "update",
                        ok: true,
                        updated_fields: updated,
                        task: Some(task),
                        error: None,
                    },
                )
            }
            "complete" => {
                let now_secs = current_unix_secs();
                let id = required_string(&obj, "id")?;
                let mut task = self
                    .store
                    .get(channel, chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                task.status = TaskStatus::Completed;
                task.completed_at_unix_secs = now_secs;
                task.updated_at = now_secs;
                task = normalize_task_item(task)?;
                self.store.upsert(&task)?;
                serialize_tool_output(
                    "tool_task",
                    &TaskMutationResponse {
                        op: "complete",
                        ok: true,
                        task,
                    },
                )
            }
            "delete" => {
                let id = required_string(&obj, "id")?;
                if let Some(task) = self.store.get(channel, chat_id, id)? {
                    self.delete_local_calendar(&task.calendar_event_id)?;
                }
                let removed = self.store.delete(channel, chat_id, id)?;
                serialize_tool_output(
                    "tool_task",
                    &TaskDeleteResponse {
                        op: "delete",
                        id,
                        ok: removed,
                    },
                )
            }
            _ => Err(Error::config("tool_task", format!("unknown op: {}", op))),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

impl TaskTool {
    fn sync_local_calendar(&self, task: &mut TaskItem, now_secs: u64) -> Result<()> {
        let has_calendar_range =
            task.calendar_start_at_unix_secs != 0 && task.calendar_end_at_unix_secs != 0;
        if !has_calendar_range {
            self.delete_local_calendar(&task.calendar_event_id)?;
            task.calendar_event_id.clear();
            return Ok(());
        }
        let event = CalendarEvent {
            id: if task.calendar_event_id.is_empty() {
                format!("tskcal_{}", task.id)
            } else {
                task.calendar_event_id.clone()
            },
            title: task.title.clone(),
            start_at_unix_secs: task.calendar_start_at_unix_secs,
            end_at_unix_secs: task.calendar_end_at_unix_secs,
            timezone: task.calendar_timezone.clone(),
            location: task.calendar_location.clone(),
            notes: if task.calendar_notes.trim().is_empty() {
                task.detail.clone()
            } else {
                task.calendar_notes.clone()
            },
            provider: CALENDAR_PROVIDER_LOCAL.to_string(),
            calendar_id: "default".to_string(),
            remote_id: String::new(),
            status: CalendarEventStatus::Confirmed,
            updated_at: now_secs,
        };
        let created = task.calendar_event_id.is_empty();
        let event = self
            .calendar_service
            .upsert(None, CALENDAR_PROVIDER_LOCAL, &event, created)?;
        task.calendar_event_id = event.id;
        Ok(())
    }

    fn delete_local_calendar(&self, event_id: &str) -> Result<()> {
        if event_id.trim().is_empty() {
            return Ok(());
        }
        let _ = self
            .calendar_service
            .delete(None, CALENDAR_PROVIDER_LOCAL, event_id)?;
        Ok(())
    }
}

fn require_session_scope(ctx: &dyn ToolContext) -> Result<(&str, &str)> {
    let channel = ctx
        .current_channel()
        .ok_or_else(|| Error::config("tool_task", "no current channel"))?;
    let chat_id = ctx
        .current_chat_id()
        .ok_or_else(|| Error::config("tool_task", "no current chat_id"))?;
    Ok((channel, chat_id))
}

fn required_string<'a>(
    obj: &'a serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::config("tool_task", format!("missing {}", key)))
}

fn optional_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn nullable_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.to_string(),
        _ => String::new(),
    }
}

fn parse_status(value: &Value) -> Result<TaskStatus> {
    match value.as_str() {
        Some("open") => Ok(TaskStatus::Open),
        Some("in_progress") => Ok(TaskStatus::InProgress),
        Some("completed") => Ok(TaskStatus::Completed),
        Some("cancelled") => Ok(TaskStatus::Cancelled),
        _ => Err(Error::config(
            "tool_task",
            "status must be open, in_progress, completed, or cancelled",
        )),
    }
}

fn parse_priority(value: &Value) -> Result<TaskPriority> {
    match value.as_str() {
        Some("low") => Ok(TaskPriority::Low),
        Some("normal") => Ok(TaskPriority::Normal),
        Some("high") => Ok(TaskPriority::High),
        _ => Err(Error::config(
            "tool_task",
            "priority must be low, normal, or high",
        )),
    }
}

fn parse_nullable_time(value: Option<&Value>, field: &'static str) -> Result<Option<u64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(|| {
            Error::config(
                "tool_task",
                format!("{} must be a non-negative number", field),
            )
        }),
        Some(Value::String(text)) => parse_iso8601(text).map(Some).ok_or_else(|| {
            Error::config(
                "tool_task",
                format!("{} must be Unix seconds or ISO8601", field),
            )
        }),
        Some(_) => Err(Error::config(
            "tool_task",
            format!("{} must be number, string, or null", field),
        )),
    }
}

fn build_task_id(title: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.hash(&mut hasher);
    current_unix_secs().hash(&mut hasher);
    let short = (hasher.finish() & 0xffff) as u16;
    let seq = TASK_SEQ.fetch_add(1, Ordering::Relaxed) & 0xffff;
    format!("tsk_{}_{}_{:04x}", current_unix_secs(), seq, short)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{
        CalendarProviderCredential, CalendarProviderCredentialStore, CalendarQuery,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubTaskStore {
        items: Mutex<HashMap<String, TaskItem>>,
    }

    impl TaskStore for StubTaskStore {
        fn list(&self, channel: &str, chat_id: &str, query: TaskQuery) -> Result<Vec<TaskItem>> {
            let items = self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|item| item.channel == channel && item.chat_id == chat_id)
                .cloned()
                .collect::<Vec<_>>();
            Ok(crate::task::filter_tasks(items, query))
        }

        fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned()
                .filter(|item| item.channel == channel && item.chat_id == chat_id))
        }

        fn upsert(&self, task: &TaskItem) -> Result<()> {
            let task = crate::task::normalize_task_item(task.clone())?;
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(task.id.clone(), task);
            Ok(())
        }

        fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            let matched = items
                .get(id)
                .map(|item| item.channel == channel && item.chat_id == chat_id)
                .unwrap_or(false);
            if !matched {
                return Ok(false);
            }
            Ok(items.remove(id).is_some())
        }

        fn claim_due(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
            let mut due = items
                .values()
                .filter(|item| {
                    item.due_at_unix_secs != 0
                        && item.due_at_unix_secs <= now_unix_secs
                        && item.due_notified_at_unix_secs == 0
                        && !item.status.is_terminal()
                })
                .cloned()
                .collect::<Vec<_>>();
            due.sort_by(|left, right| left.due_at_unix_secs.cmp(&right.due_at_unix_secs));
            if due.len() > limit {
                due.truncate(limit);
            }
            for task in &due {
                if let Some(item) = items.get_mut(&task.id) {
                    item.due_notified_at_unix_secs = now_unix_secs;
                }
            }
            Ok(due)
        }
    }

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
            let event = crate::calendar::normalize_calendar_event(event.clone())?;
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(event.id.clone(), event);
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
    struct StubCredentialStore;

    impl CalendarProviderCredentialStore for StubCredentialStore {
        fn get(&self, _provider: &str) -> Result<Option<CalendarProviderCredential>> {
            Ok(None)
        }

        fn set(&self, _credential: &CalendarProviderCredential) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _provider: &str) -> Result<()> {
            Ok(())
        }

        fn list_statuses(&self) -> Result<Vec<crate::calendar::CalendarProviderCredentialStatus>> {
            Ok(Vec::new())
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

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }

        fn current_chat_id(&self) -> Option<&str> {
            Some("chat-1")
        }

        fn current_channel(&self) -> Option<&str> {
            Some("qq_channel")
        }
    }

    #[test]
    fn task_tool_create_and_complete_roundtrip() {
        let tool = TaskTool::new(
            Arc::new(StubTaskStore::default()),
            Arc::new(StubCalendarStore::default()),
            Arc::new(StubCredentialStore),
        );
        let mut ctx = DummyCtx;
        let created = tool
            .execute(
                r#"{"op":"create","title":"跟进供应商","project":"beetle","due_at":1700000000}"#,
                &mut ctx,
            )
            .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let id = created["task"]["id"].as_str().unwrap();
        assert_eq!(created["task"]["project"], "beetle");

        let completed = tool
            .execute(&format!(r#"{{"op":"complete","id":"{}"}}"#, id), &mut ctx)
            .unwrap();
        let completed: Value = serde_json::from_str(&completed).unwrap();
        assert_eq!(completed["task"]["status"], "completed");
    }

    #[test]
    fn task_tool_syncs_local_calendar_when_range_present() {
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let tool = TaskTool::new(
            Arc::new(StubTaskStore::default()),
            Arc::clone(&calendar_store),
            Arc::new(StubCredentialStore),
        );
        let mut ctx = DummyCtx;
        let created = tool
            .execute(
                r#"{"op":"create","title":"周会","calendar_start_at":1700000000,"calendar_end_at":1700003600,"calendar_location":"A301"}"#,
                &mut ctx,
            )
            .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let event_id = created["task"]["calendar_event_id"].as_str().unwrap();
        let event = calendar_store_impl.get(event_id).unwrap().unwrap();
        assert_eq!(event.title, "周会");
        assert_eq!(event.location, "A301");
    }
}
