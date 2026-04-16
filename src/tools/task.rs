//! task tool: durable personal task management with optional local calendar sync.

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use super::http_bridge::ToolContextHttpClient;
use crate::calendar::{
    normalize_calendar_event, CalendarEvent, CalendarEventStatus, CalendarStore,
    CALENDAR_PROVIDER_LOCAL,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::calendar::{
    CalendarProviderRegistry, CalendarService, OfficeBackedCalendarProviderCredentialStore,
};
use crate::error::{Error, Result};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{
    OfficeAuthoritySource, OfficeCapability, OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::task::{normalize_task_item, TaskItem, TaskPriority, TaskQuery, TaskStatus, TaskStore};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::tools::office_failure::{
    build_office_operation_failure_outcome, OfficeOperationFailureInput,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolExecutionOutcome, ToolMetadata,
};
use crate::util::{current_unix_secs, parse_iso8601};
use serde::Serialize;
use serde_json::Value;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static TASK_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct TaskTool {
    store: Arc<dyn TaskStore + Send + Sync>,
    calendar_store: Arc<dyn CalendarStore + Send + Sync>,
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    calendar_service: Option<CalendarService>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TaskCalendarLink {
    event_id: String,
    provider: String,
    account_key: String,
    calendar_id: String,
    remote_id: String,
}

impl TaskCalendarLink {
    fn from_task(task: &TaskItem) -> Self {
        let provider = if task.calendar_provider.trim().is_empty() {
            if task.calendar_event_id.trim().is_empty() {
                String::new()
            } else {
                CALENDAR_PROVIDER_LOCAL.to_string()
            }
        } else {
            task.calendar_provider.trim().to_string()
        };
        Self {
            event_id: task.calendar_event_id.trim().to_string(),
            provider,
            account_key: task.calendar_account_key.trim().to_string(),
            calendar_id: task.calendar_calendar_id.trim().to_string(),
            remote_id: task.calendar_remote_id.trim().to_string(),
        }
    }

    fn is_empty(&self) -> bool {
        self.event_id.is_empty()
    }

    fn is_local(&self) -> bool {
        self.provider.is_empty() || self.provider == CALENDAR_PROVIDER_LOCAL
    }
}

impl TaskTool {
    pub fn new(
        store: Arc<dyn TaskStore + Send + Sync>,
        calendar_store: Arc<dyn CalendarStore + Send + Sync>,
    ) -> Self {
        Self {
            store,
            calendar_store,
            #[cfg(all(
                feature = "capability_office",
                not(any(target_arch = "xtensa", target_arch = "riscv32"))
            ))]
            calendar_service: None,
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_service(
        store: Arc<dyn TaskStore + Send + Sync>,
        calendar_store: Arc<dyn CalendarStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            store,
            calendar_store,
            providers,
            Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_authority(
        store: Arc<dyn TaskStore + Send + Sync>,
        calendar_store: Arc<dyn CalendarStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        let credential_store = Arc::new(
            OfficeBackedCalendarProviderCredentialStore::with_authority(office_authority.clone()),
        );
        let calendar_service = CalendarService::with_office_authority(
            Arc::clone(&calendar_store),
            credential_store,
            providers,
            Some(office_authority),
        );
        Self {
            store,
            calendar_store,
            calendar_service: Some(calendar_service),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_calendar_service(
        store: Arc<dyn TaskStore + Send + Sync>,
        calendar_store: Arc<dyn CalendarStore + Send + Sync>,
        calendar_service: CalendarService,
    ) -> Self {
        Self {
            store,
            calendar_store,
            calendar_service: Some(calendar_service),
        }
    }
}

impl Tool for TaskTool {
    fn name(&self) -> &'static str {
        "task"
    }

    fn description(&self) -> &'static str {
        "Manage durable tasks. Ops: list, get, create, update, complete, delete. Supports due_at reminders and optional calendar sync to local or configured office calendars."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: list|get|create|update|complete|delete"},"id":{"type":"string","description":"Task ID for get/update/complete/delete"},"title":{"type":"string","description":"Task title"},"detail":{"type":"string","description":"Optional task detail"},"project":{"type":"string","description":"Optional project/group label"},"status":{"type":"string","description":"Task status: open|in_progress|completed|cancelled"},"priority":{"type":"string","description":"Task priority: low|normal|high"},"due_at":{"description":"Optional due reminder time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_provider":{"type":"string","description":"Optional calendar provider for task sync. Defaults to local."},"calendar_account_key":{"type":"string","description":"Optional office calendar account key when the provider has multiple accounts."},"calendar_start_at":{"description":"Optional calendar start time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_end_at":{"description":"Optional calendar end time as Unix seconds, ISO8601, or null to clear","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_timezone":{"type":"string","description":"Optional calendar timezone label"},"calendar_location":{"type":"string","description":"Optional calendar location"},"calendar_notes":{"type":"string","description":"Optional calendar notes"},"clear_calendar":{"type":"boolean","description":"Remove linked calendar event and clear calendar fields"},"limit":{"type":"integer","description":"List limit, default 20, max 50"},"include_completed":{"type":"boolean","description":"Whether list should include completed/cancelled tasks"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        Ok(self.execute_impl(args, ctx)?.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        self.execute_impl(args, ctx)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

impl TaskTool {
    fn execute_impl(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_task")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_task", "missing op"))?;
        let (channel, chat_id) = require_session_scope(ctx)?;
        let channel = channel.to_string();
        let chat_id = chat_id.to_string();
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
                let tasks = self.store.list(&channel, &chat_id, query)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskListResponse {
                        op: "list",
                        count: tasks.len(),
                        tasks,
                    },
                )?))
            }
            "get" => {
                let id = required_string(&obj, "id")?;
                let task = self
                    .store
                    .get(&channel, &chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskGetResponse { op: "get", task },
                )?))
            }
            "create" => {
                let now_secs = current_unix_secs();
                let mut task = TaskItem {
                    id: build_task_id(required_string(&obj, "title")?),
                    channel: channel.clone(),
                    chat_id: chat_id.clone(),
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
                    calendar_provider: optional_string(obj.get("calendar_provider")),
                    calendar_account_key: optional_string(obj.get("calendar_account_key")),
                    calendar_calendar_id: String::new(),
                    calendar_remote_id: String::new(),
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
                if let Err(error) =
                    self.sync_calendar(&mut task, TaskCalendarLink::default(), now_secs, ctx)
                {
                    if let Some((provider, account_key)) =
                        task_calendar_failure_context(&task, &TaskCalendarLink::default())
                    {
                        return self.office_operation_failure(
                            "create",
                            Some(provider.as_str()),
                            account_key.as_deref(),
                            &error,
                        );
                    }
                    return Err(error);
                }
                self.store.upsert(&task)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskMutationResponse {
                        op: "create",
                        ok: true,
                        task,
                    },
                )?))
            }
            "update" => {
                let now_secs = current_unix_secs();
                let id = required_string(&obj, "id")?;
                let mut task = self
                    .store
                    .get(&channel, &chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                let previous_calendar_link = TaskCalendarLink::from_task(&task);
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
                if obj.contains_key("calendar_provider") {
                    let previous_provider = task.calendar_provider.clone();
                    task.calendar_provider = nullable_string(obj.get("calendar_provider"));
                    if task.calendar_provider != previous_provider
                        && !obj.contains_key("calendar_account_key")
                    {
                        task.calendar_account_key.clear();
                    }
                    updated.push("calendar_provider");
                }
                if obj.contains_key("calendar_account_key") {
                    task.calendar_account_key = nullable_string(obj.get("calendar_account_key"));
                    updated.push("calendar_account_key");
                }
                if obj
                    .get("clear_calendar")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    self.clear_calendar_link(&mut task);
                    task.calendar_start_at_unix_secs = 0;
                    task.calendar_end_at_unix_secs = 0;
                    task.calendar_timezone.clear();
                    task.calendar_location.clear();
                    task.calendar_notes.clear();
                    updated.push("clear_calendar");
                }
                if updated.is_empty() {
                    return Ok(ToolExecutionOutcome::text(serialize_tool_output(
                        "tool_task",
                        &TaskUpdateResponse {
                            op: "update",
                            ok: false,
                            updated_fields: Vec::new(),
                            task: None,
                            error: Some("no fields to update"),
                        },
                    )?));
                }
                if task.status == TaskStatus::Completed {
                    task.completed_at_unix_secs = now_secs;
                } else {
                    task.completed_at_unix_secs = 0;
                }
                task.updated_at = now_secs;
                task = normalize_task_item(task)?;
                if let Err(error) =
                    self.sync_calendar(&mut task, previous_calendar_link.clone(), now_secs, ctx)
                {
                    if let Some((provider, account_key)) =
                        task_calendar_failure_context(&task, &previous_calendar_link)
                    {
                        return self.office_operation_failure(
                            "update",
                            Some(provider.as_str()),
                            account_key.as_deref(),
                            &error,
                        );
                    }
                    return Err(error);
                }
                self.store.upsert(&task)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskUpdateResponse {
                        op: "update",
                        ok: true,
                        updated_fields: updated,
                        task: Some(task),
                        error: None,
                    },
                )?))
            }
            "complete" => {
                let now_secs = current_unix_secs();
                let id = required_string(&obj, "id")?;
                let mut task = self
                    .store
                    .get(&channel, &chat_id, id)?
                    .ok_or_else(|| Error::config("tool_task", "task not found"))?;
                let previous_calendar_link = TaskCalendarLink::from_task(&task);
                task.status = TaskStatus::Completed;
                task.completed_at_unix_secs = now_secs;
                task.updated_at = now_secs;
                task = normalize_task_item(task)?;
                if let Err(error) =
                    self.sync_calendar(&mut task, previous_calendar_link.clone(), now_secs, ctx)
                {
                    if let Some((provider, account_key)) =
                        task_calendar_failure_context(&task, &previous_calendar_link)
                    {
                        return self.office_operation_failure(
                            "complete",
                            Some(provider.as_str()),
                            account_key.as_deref(),
                            &error,
                        );
                    }
                    return Err(error);
                }
                self.store.upsert(&task)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskMutationResponse {
                        op: "complete",
                        ok: true,
                        task,
                    },
                )?))
            }
            "delete" => {
                let id = required_string(&obj, "id")?;
                if let Some(task) = self.store.get(&channel, &chat_id, id)? {
                    let previous_calendar_link = TaskCalendarLink::from_task(&task);
                    if let Err(error) = self.delete_calendar_link(&previous_calendar_link, ctx) {
                        if let Some((provider, account_key)) =
                            previous_link_failure_context(&previous_calendar_link)
                        {
                            return self.office_operation_failure(
                                "delete",
                                Some(provider.as_str()),
                                account_key.as_deref(),
                                &error,
                            );
                        }
                        return Err(error);
                    }
                }
                let removed = self.store.delete(&channel, &chat_id, id)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_task",
                    &TaskDeleteResponse {
                        op: "delete",
                        id,
                        ok: removed,
                    },
                )?))
            }
            _ => Err(Error::config("tool_task", format!("unknown op: {}", op))),
        }
    }

    fn office_operation_failure(
        &self,
        op: &str,
        provider: Option<&str>,
        account_key: Option<&str>,
        error: &Error,
    ) -> Result<ToolExecutionOutcome> {
        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let Some(calendar_service) = self.calendar_service.as_ref() else {
                return Err(Error::config(
                    "tool_task",
                    "office calendar runtime is unavailable in this context",
                ));
            };
            build_office_operation_failure_outcome(OfficeOperationFailureInput {
                stage: "tool_task",
                op,
                provider,
                account_key,
                capability: OfficeCapability::Calendar,
                default_account_key: calendar_service.office_default_account_key()?,
                resolve_hint: None,
                account_assessments: calendar_service.office_account_assessments()?,
                error,
            })
        }
        #[cfg(not(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        )))]
        {
            let _ = (op, provider, account_key);
            Err(Error::config(
                "tool_task",
                format!("office calendar failure handling unavailable: {}", error),
            ))
        }
    }

    fn sync_calendar(
        &self,
        task: &mut TaskItem,
        previous_link: TaskCalendarLink,
        now_secs: u64,
        ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        let has_calendar_range =
            task.calendar_start_at_unix_secs != 0 && task.calendar_end_at_unix_secs != 0;
        if !has_calendar_range {
            self.delete_calendar_link(&previous_link, ctx)?;
            self.clear_calendar_link(task);
            return Ok(());
        }
        let provider = desired_calendar_provider(task);
        let desired_account_key = if provider == CALENDAR_PROVIDER_LOCAL {
            String::new()
        } else {
            task.calendar_account_key.clone()
        };
        let link_changed = !previous_link.is_empty()
            && (previous_link.provider != provider
                || previous_link.account_key != desired_account_key);
        if link_changed {
            self.delete_calendar_link(&previous_link, ctx)?;
            self.clear_calendar_link(task);
        } else if !previous_link.is_empty() {
            task.calendar_event_id = previous_link.event_id.clone();
            task.calendar_calendar_id = previous_link.calendar_id.clone();
            task.calendar_remote_id = previous_link.remote_id.clone();
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
            provider: provider.clone(),
            calendar_id: default_calendar_id_for_task(&provider, &task.calendar_calendar_id),
            remote_id: task.calendar_remote_id.clone(),
            status: if task.status.is_terminal() {
                CalendarEventStatus::Cancelled
            } else {
                CalendarEventStatus::Confirmed
            },
            updated_at: now_secs,
        };
        let event = normalize_calendar_event(event)?;
        let (event, resolved_account_key) = self.upsert_calendar_event(
            &provider,
            desired_account_key.as_str(),
            &event,
            previous_link.is_empty() || link_changed,
            ctx,
        )?;
        task.calendar_event_id = event.id;
        task.calendar_provider = event.provider;
        task.calendar_account_key = if provider == CALENDAR_PROVIDER_LOCAL {
            String::new()
        } else {
            resolved_account_key
        };
        task.calendar_calendar_id = event.calendar_id;
        task.calendar_remote_id = event.remote_id;
        Ok(())
    }

    fn upsert_calendar_event(
        &self,
        provider: &str,
        _account_key: &str,
        event: &CalendarEvent,
        _is_create: bool,
        _ctx: &mut dyn ToolContext,
    ) -> Result<(CalendarEvent, String)> {
        if provider == CALENDAR_PROVIDER_LOCAL {
            self.calendar_store.upsert(event)?;
            let event = self
                .calendar_store
                .get(&event.id)?
                .ok_or_else(|| Error::config("tool_task", "calendar event missing after upsert"));
            return event.map(|event| (event, String::new()));
        }
        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let service = self.calendar_service.as_ref().ok_or_else(|| {
                Error::config(
                    "tool_task",
                    format!(
                        "calendar provider '{}' is unavailable in this runtime",
                        provider
                    ),
                )
            })?;
            let resolved_account_key = service
                .resolve_account_key_for_provider(
                    provider,
                    (!_account_key.trim().is_empty()).then_some(_account_key),
                )?
                .ok_or_else(|| {
                    Error::config(
                        "tool_task",
                        format!(
                            "calendar provider '{}' is unavailable in this runtime",
                            provider
                        ),
                    )
                })?;
            let mut http = ToolContextHttpClient::new(_ctx);
            let event = service.upsert(
                Some(&mut http),
                provider,
                Some(resolved_account_key.as_str()),
                event,
                _is_create,
            );
            return event.map(|event| (event, resolved_account_key));
        }
        #[allow(unreachable_code)]
        Err(Error::config(
            "tool_task",
            format!(
                "calendar provider '{}' is unavailable in this runtime",
                provider
            ),
        ))
    }

    fn delete_calendar_link(
        &self,
        link: &TaskCalendarLink,
        _ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        if link.is_empty() {
            return Ok(());
        }
        if link.is_local() {
            let _ = self.calendar_store.delete(&link.event_id)?;
            return Ok(());
        }
        #[cfg(all(
            feature = "capability_office",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        {
            let service = self.calendar_service.as_ref().ok_or_else(|| {
                Error::config(
                    "tool_task",
                    format!(
                        "calendar provider '{}' is unavailable in this runtime",
                        link.provider
                    ),
                )
            })?;
            let mut http = ToolContextHttpClient::new(_ctx);
            let _ = service.delete(
                Some(&mut http),
                &link.provider,
                (!link.account_key.is_empty()).then_some(link.account_key.as_str()),
                &link.event_id,
            )?;
            return Ok(());
        }
        #[allow(unreachable_code)]
        Err(Error::config(
            "tool_task",
            format!(
                "calendar provider '{}' is unavailable in this runtime",
                link.provider
            ),
        ))
    }

    fn clear_calendar_link(&self, task: &mut TaskItem) {
        task.calendar_event_id.clear();
        task.calendar_provider.clear();
        task.calendar_account_key.clear();
        task.calendar_calendar_id.clear();
        task.calendar_remote_id.clear();
    }
}

fn desired_calendar_provider(task: &TaskItem) -> String {
    let provider = task.calendar_provider.trim();
    if provider.is_empty() {
        CALENDAR_PROVIDER_LOCAL.to_string()
    } else {
        provider.to_string()
    }
}

fn default_calendar_id_for_task(provider: &str, calendar_id: &str) -> String {
    if !calendar_id.trim().is_empty() {
        return calendar_id.to_string();
    }
    if provider == CALENDAR_PROVIDER_LOCAL {
        "default".to_string()
    } else {
        String::new()
    }
}

fn task_calendar_failure_context(
    task: &TaskItem,
    previous_link: &TaskCalendarLink,
) -> Option<(String, Option<String>)> {
    let provider = desired_calendar_provider(task);
    if provider != CALENDAR_PROVIDER_LOCAL {
        let account_key = (!task.calendar_account_key.trim().is_empty())
            .then(|| task.calendar_account_key.clone());
        return Some((provider, account_key));
    }
    previous_link_failure_context(previous_link)
}

fn previous_link_failure_context(link: &TaskCalendarLink) -> Option<(String, Option<String>)> {
    if link.is_empty() || link.is_local() {
        return None;
    }
    Some((
        link.provider.clone(),
        (!link.account_key.trim().is_empty()).then(|| link.account_key.clone()),
    ))
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
    use crate::calendar::CalendarQuery;
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::calendar::{
        CalendarOperation, CalendarProvider, CalendarProviderCredential,
        CalendarProviderCredentialStore, CalendarProviderRegistry, CalendarService,
    };
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCapabilityBinding, OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        OfficeSelectionPolicy, OfficeService,
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

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubOfficeCredentialStore {
        items: Mutex<HashMap<String, OfficeCredential>>,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
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

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubRuntimeStatusStore {
        items: Mutex<HashMap<String, crate::office::OfficeAccountRuntimeStatus>>,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(
            &self,
            account_key: &str,
        ) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, status: &crate::office::OfficeAccountRuntimeStatus) -> Result<()> {
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

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubCalendarCredentialStore {
        items: Mutex<HashMap<String, CalendarProviderCredential>>,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl CalendarProviderCredentialStore for StubCalendarCredentialStore {
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

        fn list_statuses(&self) -> Result<Vec<crate::calendar::CalendarProviderCredentialStatus>> {
            Ok(Vec::new())
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Clone, Debug, PartialEq, Eq)]
    enum RemoteCall {
        Create {
            account_key: String,
            event: CalendarEvent,
        },
        Update {
            account_key: String,
            event: CalendarEvent,
        },
        Delete {
            account_key: String,
            id: String,
        },
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct RecordingRemoteProvider {
        calls: Mutex<Vec<RemoteCall>>,
        events: Mutex<HashMap<String, CalendarEvent>>,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl RecordingRemoteProvider {
        fn calls(&self) -> Vec<RemoteCall> {
            self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl CalendarProvider for RecordingRemoteProvider {
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
            Ok(self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn get_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Ok(self
                .events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(id)
                .cloned())
        }

        fn create_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            let created = CalendarEvent {
                id: event.id.clone(),
                title: event.title.clone(),
                start_at_unix_secs: event.start_at_unix_secs,
                end_at_unix_secs: event.end_at_unix_secs,
                timezone: event.timezone.clone(),
                location: event.location.clone(),
                notes: event.notes.clone(),
                provider: credential.provider.clone(),
                calendar_id: credential.calendar_id.clone(),
                remote_id: format!("remote-{}", event.id),
                status: event.status,
                updated_at: event.updated_at,
            };
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(created.id.clone(), created.clone());
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(RemoteCall::Create {
                    account_key: credential.account_key.clone(),
                    event: created.clone(),
                });
            Ok(created)
        }

        fn update_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(event.id.clone(), event.clone());
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(RemoteCall::Update {
                    account_key: credential.account_key.clone(),
                    event: event.clone(),
                });
            Ok(event.clone())
        }

        fn delete_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            credential: &CalendarProviderCredential,
            id: &str,
        ) -> Result<bool> {
            self.events
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(id);
            self.calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(RemoteCall::Delete {
                    account_key: credential.account_key.clone(),
                    id: id.to_string(),
                });
            Ok(true)
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    struct FailingRemoteProvider;

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl CalendarProvider for FailingRemoteProvider {
        fn provider_name(&self) -> &'static str {
            "mock_remote"
        }

        fn display_name(&self) -> &'static str {
            "Mock Remote"
        }

        fn supports(&self, op: CalendarOperation) -> bool {
            matches!(
                op,
                CalendarOperation::List
                    | CalendarOperation::Get
                    | CalendarOperation::Create
                    | CalendarOperation::Update
                    | CalendarOperation::Delete
            )
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
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(Error::config(
                "calendar_provider",
                "remote calendar unavailable",
            ))
        }

        fn update_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(Error::config(
                "calendar_provider",
                "remote calendar unavailable",
            ))
        }

        fn delete_event(
            &self,
            _http: &mut dyn crate::calendar::CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Err(Error::config(
                "calendar_provider",
                "remote calendar unavailable",
            ))
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn build_remote_calendar_service(provider: Arc<dyn CalendarProvider>) -> CalendarService {
        let credential_store = Arc::new(StubCalendarCredentialStore::default());
        credential_store
            .set(&CalendarProviderCredential {
                account_key: "calendar-work".to_string(),
                provider: "mock_remote".to_string(),
                account_id: "work@example.com".to_string(),
                account_label: "Work".to_string(),
                calendar_id: "team".to_string(),
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
            account_key: "calendar-work".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let mut binding = OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "calendar-work".to_string());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore::default()),
            Arc::new(StubRuntimeStatusStore::default()),
        );
        let mut providers = CalendarProviderRegistry::new();
        providers.register(provider);
        CalendarService::with_office_service(
            Arc::new(StubCalendarStore::default()),
            credential_store,
            providers,
            Some(office_service),
        )
    }

    #[test]
    fn task_tool_create_and_complete_roundtrip() {
        let tool = TaskTool::new(
            Arc::new(StubTaskStore::default()),
            Arc::new(StubCalendarStore::default()),
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

    #[test]
    fn task_tool_complete_cancels_linked_local_calendar_event() {
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let tool = TaskTool::new(
            Arc::new(StubTaskStore::default()),
            Arc::clone(&calendar_store),
        );
        let mut ctx = DummyCtx;
        let created = tool
            .execute(
                r#"{"op":"create","title":"客户回访","calendar_start_at":1700000000,"calendar_end_at":1700003600}"#,
                &mut ctx,
            )
            .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_id = created["task"]["id"].as_str().unwrap();
        let event_id = created["task"]["calendar_event_id"].as_str().unwrap();

        let completed = tool
            .execute(
                &format!(r#"{{"op":"complete","id":"{}"}}"#, task_id),
                &mut ctx,
            )
            .unwrap();
        let completed: Value = serde_json::from_str(&completed).unwrap();
        assert_eq!(completed["task"]["status"], "completed");

        let event = calendar_store_impl.get(event_id).unwrap().unwrap();
        assert_eq!(event.status, CalendarEventStatus::Cancelled);
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn task_tool_routes_remote_calendar_through_office_default_account_and_keeps_lifecycle_in_sync()
    {
        let provider = Arc::new(RecordingRemoteProvider::default());
        let tool = TaskTool::with_office_calendar_service(
            Arc::new(StubTaskStore::default()),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(provider.clone()),
        );
        let mut ctx = DummyCtx;
        let created = tool
            .execute(
                r#"{"op":"create","title":"客户例会","calendar_provider":"mock_remote","calendar_start_at":1700000000,"calendar_end_at":1700003600}"#,
                &mut ctx,
            )
            .unwrap();
        let created: Value = serde_json::from_str(&created).unwrap();
        let task_id = created["task"]["id"].as_str().unwrap();
        let event_id = created["task"]["calendar_event_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(created["task"]["calendar_provider"], "mock_remote");
        assert_eq!(created["task"]["calendar_account_key"], "calendar-work");
        assert_eq!(created["task"]["calendar_calendar_id"], "team");
        assert_eq!(
            created["task"]["calendar_remote_id"],
            format!("remote-{event_id}")
        );

        let completed = tool
            .execute(
                &format!(r#"{{"op":"complete","id":"{}"}}"#, task_id),
                &mut ctx,
            )
            .unwrap();
        let completed: Value = serde_json::from_str(&completed).unwrap();
        assert_eq!(completed["task"]["status"], "completed");

        let deleted = tool
            .execute(
                &format!(r#"{{"op":"delete","id":"{}"}}"#, task_id),
                &mut ctx,
            )
            .unwrap();
        let deleted: Value = serde_json::from_str(&deleted).unwrap();
        assert_eq!(deleted["ok"], true);

        let calls = provider.calls();
        assert_eq!(calls.len(), 3);
        assert!(matches!(
            &calls[0],
            RemoteCall::Create { account_key, event }
                if account_key == "calendar-work"
                    && event.provider == "mock_remote"
                    && event.status == CalendarEventStatus::Confirmed
        ));
        assert!(matches!(
            &calls[1],
            RemoteCall::Update { account_key, event }
                if account_key == "calendar-work"
                    && event.id == event_id
                    && event.status == CalendarEventStatus::Cancelled
        ));
        assert!(matches!(
            &calls[2],
            RemoteCall::Delete { account_key, id }
                if account_key == "calendar-work" && id == &event_id
        ));
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn task_tool_remote_calendar_create_returns_structured_office_failure() {
        let tool = TaskTool::with_office_calendar_service(
            Arc::new(StubTaskStore::default()),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(Arc::new(FailingRemoteProvider)),
        );
        let mut ctx = DummyCtx;

        let outcome = tool
            .execute_outcome(
                r#"{"op":"create","title":"客户例会","calendar_provider":"mock_remote","calendar_start_at":1700000000,"calendar_end_at":1700003600}"#,
                &mut ctx,
            )
            .expect("structured failure outcome");
        let payload: Value = serde_json::from_str(&outcome.content).expect("failure json");

        assert_eq!(
            outcome.failure_kind,
            Some(crate::tools::ToolExecutionFailureKind::Permanent)
        );
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["office_assessment"]["capability"], "calendar");
        assert_eq!(
            payload["office_assessment"]["default_account_key"],
            "calendar-work"
        );
        assert_eq!(
            payload["office_assessment"]["account_assessments"][0]["account_key"],
            "calendar-work"
        );
    }
}
