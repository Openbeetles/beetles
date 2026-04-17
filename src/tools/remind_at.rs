//! remind_at 工具：创建持久提醒，可选桥接本地或远端日历事件。

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
use crate::calendar::{CalendarProviderRegistry, CalendarService};
use crate::error::{Error, Result};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{
    OfficeAuthoritySource, OfficeCapability, OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::reminder::{
    normalize_reminder_item, ReminderItem, DEFAULT_REMINDER_CALENDAR_DURATION_SECS,
};
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
use std::sync::atomic::{AtomicU32, Ordering};

static REMINDER_SEQ: AtomicU32 = AtomicU32::new(1);

pub struct RemindAtTool {
    store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
    local_calendar_store: Option<std::sync::Arc<dyn CalendarStore + Send + Sync>>,
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    calendar_service: Option<CalendarService>,
}

#[derive(Serialize)]
struct RemindMutationResponse {
    op: &'static str,
    ok: bool,
    reminder: ReminderItem,
}

#[derive(Serialize)]
struct RemindGetResponse {
    op: &'static str,
    reminder: ReminderItem,
}

#[derive(Clone, Default)]
struct ReminderCalendarLink {
    provider: String,
    account_key: String,
    event_id: String,
    calendar_id: String,
    remote_id: String,
}

#[derive(Serialize)]
struct RemindListResponse {
    count: usize,
    items: Vec<ReminderItem>,
}

impl RemindAtTool {
    pub fn new(store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>) -> Self {
        Self {
            store,
            local_calendar_store: None,
            #[cfg(all(
                feature = "capability_office",
                not(any(target_arch = "xtensa", target_arch = "riscv32"))
            ))]
            calendar_service: None,
        }
    }

    pub fn with_local_calendar(
        store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
        local_calendar_store: std::sync::Arc<dyn CalendarStore + Send + Sync>,
    ) -> Self {
        Self {
            store,
            local_calendar_store: Some(local_calendar_store),
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
        store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
        local_calendar_store: std::sync::Arc<dyn CalendarStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_service: OfficeService,
    ) -> Self {
        Self::with_office_authority(
            store,
            local_calendar_store,
            providers,
            std::sync::Arc::new(SnapshotOfficeAuthoritySource::new(office_service)),
        )
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_authority(
        store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
        local_calendar_store: std::sync::Arc<dyn CalendarStore + Send + Sync>,
        providers: CalendarProviderRegistry,
        office_authority: std::sync::Arc<dyn OfficeAuthoritySource + Send + Sync>,
    ) -> Self {
        let credential_store = std::sync::Arc::new(
            crate::calendar::OfficeBackedCalendarProviderCredentialStore::with_authority(
                office_authority.clone(),
            ),
        );
        let calendar_service = CalendarService::with_office_authority(
            local_calendar_store.clone(),
            credential_store,
            providers,
            Some(office_authority),
        );
        Self {
            store,
            local_calendar_store: Some(local_calendar_store),
            calendar_service: Some(calendar_service),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    pub fn with_office_calendar_service(
        store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
        local_calendar_store: std::sync::Arc<dyn CalendarStore + Send + Sync>,
        calendar_service: CalendarService,
    ) -> Self {
        Self {
            store,
            local_calendar_store: Some(local_calendar_store),
            calendar_service: Some(calendar_service),
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn office_operation_failure(
        &self,
        op: &'static str,
        provider: &str,
        account_key: Option<&str>,
        error: &Error,
    ) -> Result<ToolExecutionOutcome> {
        let Some(service) = self.calendar_service.as_ref() else {
            return Err(Error::config(
                "remind_at",
                format!("calendar provider '{provider}' is unavailable in this runtime"),
            ));
        };
        build_office_operation_failure_outcome(OfficeOperationFailureInput {
            stage: "tool_remind_at",
            op,
            provider: Some(provider),
            account_key,
            capability: OfficeCapability::Calendar,
            default_account_key: service.office_default_account_key()?,
            resolve_hint: service.office_resolve_hint(Some(provider), account_key)?,
            account_assessments: service.office_account_assessments()?,
            error,
        })
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn upsert_remote_calendar_link(
        &self,
        reminder: &mut ReminderItem,
        provider: &str,
        event: &CalendarEvent,
        previous_link: &ReminderCalendarLink,
        link_changed: bool,
        ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        let service = self.calendar_service.as_ref().ok_or_else(|| {
            Error::config(
                "remind_at",
                format!("calendar provider '{provider}' is unavailable in this runtime"),
            )
        })?;
        let resolved_account_key = service
            .resolve_account_key_for_provider(
                provider,
                (!reminder.calendar_account_key.trim().is_empty())
                    .then_some(reminder.calendar_account_key.as_str()),
            )?
            .ok_or_else(|| {
                Error::config(
                    "remind_at",
                    format!("calendar provider '{provider}' is unavailable in this runtime"),
                )
            })?;
        let mut http = ToolContextHttpClient::new(ctx);
        let stored = service.upsert(
            Some(&mut http),
            provider,
            Some(resolved_account_key.as_str()),
            event,
            previous_link.is_empty() || link_changed,
        )?;
        reminder.calendar_event_id = stored.id;
        reminder.calendar_provider = stored.provider;
        reminder.calendar_account_key = resolved_account_key;
        reminder.calendar_calendar_id = stored.calendar_id;
        reminder.calendar_remote_id = stored.remote_id;
        Ok(())
    }

    #[cfg(not(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    )))]
    fn upsert_remote_calendar_link(
        &self,
        _reminder: &mut ReminderItem,
        provider: &str,
        _event: &CalendarEvent,
        _previous_link: &ReminderCalendarLink,
        _link_changed: bool,
        _ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        Err(Error::config(
            "remind_at",
            format!("calendar provider '{provider}' is unavailable in this runtime"),
        ))
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn delete_remote_calendar_link(
        &self,
        link: &ReminderCalendarLink,
        ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        let service = self.calendar_service.as_ref().ok_or_else(|| {
            Error::config(
                "remind_at",
                format!(
                    "calendar provider '{}' is unavailable in this runtime",
                    link.provider
                ),
            )
        })?;
        let mut http = ToolContextHttpClient::new(ctx);
        service
            .delete(
                Some(&mut http),
                &link.provider,
                (!link.account_key.is_empty()).then_some(link.account_key.as_str()),
                &link.event_id,
            )
            .map(|_| ())
    }

    #[cfg(not(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    )))]
    fn delete_remote_calendar_link(
        &self,
        link: &ReminderCalendarLink,
        _ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        Err(Error::config(
            "remind_at",
            format!(
                "calendar provider '{}' is unavailable in this runtime",
                link.provider
            ),
        ))
    }

    fn execute_impl(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<ToolExecutionOutcome> {
        let channel = ctx
            .current_channel()
            .ok_or_else(|| {
                Error::config(
                    "remind_at",
                    "no current channel (tool used outside session)",
                )
            })?
            .to_string();
        let chat_id = ctx
            .current_chat_id()
            .ok_or_else(|| {
                Error::config(
                    "remind_at",
                    "no current chat_id (tool used outside session)",
                )
            })?
            .to_string();
        let obj = parse_tool_args(args, "remind_at")?;
        let op = obj.get("op").and_then(Value::as_str).unwrap_or("schedule");
        match op {
            "schedule" => self.execute_schedule(&channel, &chat_id, &obj, ctx),
            "get" => self.execute_get(&channel, &chat_id, &obj),
            "update" => self.execute_update(&channel, &chat_id, &obj, ctx),
            "delete" => self.execute_delete(&channel, &chat_id, &obj, ctx),
            _ => Err(Error::config("remind_at", format!("unknown op: {}", op))),
        }
    }

    fn execute_schedule(
        &self,
        channel: &str,
        chat_id: &str,
        obj: &serde_json::Map<String, Value>,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let at_val = obj
            .get("at")
            .ok_or_else(|| Error::config("remind_at", "missing at"))?;
        let context = required_context(obj)?;
        let at_secs = parse_at_to_unix_secs(at_val)?;
        let now_secs = current_unix_secs();
        let mut reminder = ReminderItem {
            id: next_reminder_id(),
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            at_unix_secs: at_secs,
            context: context.to_string(),
            updated_at: now_secs,
            ..ReminderItem::default()
        };
        populate_calendar_request(&mut reminder, obj, at_secs)?;
        if let Err(error) =
            self.apply_calendar_link(&mut reminder, ReminderCalendarLink::default(), ctx)
        {
            if let Some(provider) = desired_calendar_provider(&reminder) {
                if provider != CALENDAR_PROVIDER_LOCAL {
                    #[cfg(all(
                        feature = "capability_office",
                        not(any(target_arch = "xtensa", target_arch = "riscv32"))
                    ))]
                    {
                        return self.office_operation_failure(
                            "schedule",
                            provider.as_str(),
                            (!reminder.calendar_account_key.trim().is_empty())
                                .then_some(reminder.calendar_account_key.as_str()),
                            &error,
                        );
                    }
                }
            }
            return Err(error);
        }
        let reminder = normalize_reminder_item(reminder)?;
        self.store.upsert(&reminder)?;
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "remind_at",
            &RemindMutationResponse {
                op: "schedule",
                ok: true,
                reminder,
            },
        )?))
    }

    fn execute_get(
        &self,
        channel: &str,
        chat_id: &str,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<ToolExecutionOutcome> {
        let id = required_id(obj)?;
        let reminder = self
            .store
            .get(channel, chat_id, id)?
            .ok_or_else(|| Error::config("remind_at", "reminder not found"))?;
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "remind_at",
            &RemindGetResponse {
                op: "get",
                reminder,
            },
        )?))
    }

    fn execute_update(
        &self,
        channel: &str,
        chat_id: &str,
        obj: &serde_json::Map<String, Value>,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let id = required_id(obj)?;
        let mut reminder = self
            .store
            .get(channel, chat_id, id)?
            .ok_or_else(|| Error::config("remind_at", "reminder not found"))?;
        let previous_context = reminder.context.clone();
        let previous_link = ReminderCalendarLink::from_reminder(&reminder);
        let mut changed = false;

        if let Some(at) = obj.get("at") {
            let new_at = parse_at_to_unix_secs(at)?;
            if reminder.at_unix_secs != new_at {
                let old_start = reminder.calendar_start_at_unix_secs;
                let old_end = reminder.calendar_end_at_unix_secs;
                reminder.at_unix_secs = new_at;
                if old_start != 0 && old_end > old_start && !obj.contains_key("calendar_end_at") {
                    let duration = old_end.saturating_sub(old_start);
                    reminder.calendar_start_at_unix_secs = new_at;
                    reminder.calendar_end_at_unix_secs = new_at.saturating_add(duration);
                }
                changed = true;
            }
        }
        if let Some(context) = obj.get("context").and_then(Value::as_str) {
            let context = context.trim();
            if context.is_empty() {
                return Err(Error::config("remind_at", "context must not be empty"));
            }
            if reminder.context != context {
                reminder.context = context.to_string();
                if reminder.calendar_notes == previous_context {
                    reminder.calendar_notes = reminder.context.clone();
                }
                changed = true;
            }
        }
        if obj
            .get("clear_calendar")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            if let Err(error) = self.delete_calendar_link(&previous_link, ctx) {
                if !previous_link.is_local() {
                    #[cfg(all(
                        feature = "capability_office",
                        not(any(target_arch = "xtensa", target_arch = "riscv32"))
                    ))]
                    {
                        return self.office_operation_failure(
                            "delete",
                            &previous_link.provider,
                            (!previous_link.account_key.is_empty())
                                .then_some(previous_link.account_key.as_str()),
                            &error,
                        );
                    }
                }
                return Err(error);
            }
            self.clear_calendar_link(&mut reminder);
            changed = true;
        } else {
            changed |= self.apply_calendar_update_patch(&mut reminder, obj)?;
            if changed || !previous_link.is_empty() {
                if let Err(error) =
                    self.apply_calendar_link(&mut reminder, previous_link.clone(), ctx)
                {
                    if let Some(provider) = desired_calendar_provider(&reminder).or_else(|| {
                        (!previous_link.provider.is_empty())
                            .then_some(previous_link.provider.clone())
                    }) {
                        if provider != CALENDAR_PROVIDER_LOCAL {
                            #[cfg(all(
                                feature = "capability_office",
                                not(any(target_arch = "xtensa", target_arch = "riscv32"))
                            ))]
                            {
                                let account_key =
                                    if !reminder.calendar_account_key.trim().is_empty() {
                                        Some(reminder.calendar_account_key.as_str())
                                    } else if !previous_link.account_key.is_empty() {
                                        Some(previous_link.account_key.as_str())
                                    } else {
                                        None
                                    };
                                return self.office_operation_failure(
                                    "update",
                                    provider.as_str(),
                                    account_key,
                                    &error,
                                );
                            }
                        }
                    }
                    return Err(error);
                }
            }
        }
        if !changed {
            return Err(Error::config("remind_at", "no fields to update"));
        }
        reminder.updated_at = current_unix_secs();
        let reminder = normalize_reminder_item(reminder)?;
        self.store.upsert(&reminder)?;
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "remind_at",
            &RemindMutationResponse {
                op: "update",
                ok: true,
                reminder,
            },
        )?))
    }

    fn execute_delete(
        &self,
        channel: &str,
        chat_id: &str,
        obj: &serde_json::Map<String, Value>,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let id = required_id(obj)?;
        let reminder = self
            .store
            .get(channel, chat_id, id)?
            .ok_or_else(|| Error::config("remind_at", "reminder not found"))?;
        let link = ReminderCalendarLink::from_reminder(&reminder);
        if let Err(error) = self.delete_calendar_link(&link, ctx) {
            if !link.is_local() {
                #[cfg(all(
                    feature = "capability_office",
                    not(any(target_arch = "xtensa", target_arch = "riscv32"))
                ))]
                {
                    return self.office_operation_failure(
                        "delete",
                        &link.provider,
                        (!link.account_key.is_empty()).then_some(link.account_key.as_str()),
                        &error,
                    );
                }
            }
            return Err(error);
        }
        if !self.store.delete(channel, chat_id, id)? {
            return Err(Error::config("remind_at", "failed to delete reminder"));
        }
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "remind_at",
            &RemindMutationResponse {
                op: "delete",
                ok: true,
                reminder,
            },
        )?))
    }

    fn apply_calendar_update_patch(
        &self,
        reminder: &mut ReminderItem,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<bool> {
        let mut changed = false;
        if obj.contains_key("calendar_provider") {
            let provider = nullable_string(obj.get("calendar_provider"));
            if reminder.calendar_provider != provider {
                reminder.calendar_provider = provider;
                changed = true;
            }
        }
        if obj.contains_key("calendar_account_key") {
            let account_key = nullable_string(obj.get("calendar_account_key"));
            if reminder.calendar_account_key != account_key {
                reminder.calendar_account_key = account_key;
                changed = true;
            }
        }
        if obj.contains_key("calendar_id") {
            let calendar_id = nullable_string(obj.get("calendar_id"));
            if reminder.calendar_calendar_id != calendar_id {
                reminder.calendar_calendar_id = calendar_id;
                changed = true;
            }
        }
        if obj.contains_key("calendar_timezone") {
            let timezone = nullable_string(obj.get("calendar_timezone"));
            if reminder.calendar_timezone != timezone {
                reminder.calendar_timezone = timezone;
                changed = true;
            }
        }
        if obj.contains_key("calendar_location") {
            let location = nullable_string(obj.get("calendar_location"));
            if reminder.calendar_location != location {
                reminder.calendar_location = location;
                changed = true;
            }
        }
        if obj.contains_key("calendar_notes") {
            let notes = nullable_string(obj.get("calendar_notes"));
            if reminder.calendar_notes != notes {
                reminder.calendar_notes = notes;
                changed = true;
            }
        }
        if obj.contains_key("calendar_end_at") {
            let end_at = parse_optional_time(obj.get("calendar_end_at"), "calendar_end_at")?;
            match end_at {
                Some(end_at) => {
                    let start_at = reminder.at_unix_secs;
                    if reminder.calendar_start_at_unix_secs != start_at
                        || reminder.calendar_end_at_unix_secs != end_at
                    {
                        reminder.calendar_start_at_unix_secs = start_at;
                        reminder.calendar_end_at_unix_secs = end_at;
                        changed = true;
                    }
                }
                None => {
                    if reminder.calendar_start_at_unix_secs != 0
                        || reminder.calendar_end_at_unix_secs != 0
                    {
                        reminder.calendar_start_at_unix_secs = 0;
                        reminder.calendar_end_at_unix_secs = 0;
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }

    fn apply_calendar_link(
        &self,
        reminder: &mut ReminderItem,
        previous_link: ReminderCalendarLink,
        ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        let Some(provider) = desired_calendar_provider(reminder) else {
            if !previous_link.is_empty() {
                self.delete_calendar_link(&previous_link, ctx)?;
            }
            self.clear_calendar_link(reminder);
            return Ok(());
        };
        if reminder.calendar_start_at_unix_secs == 0 && reminder.calendar_end_at_unix_secs == 0 {
            reminder.calendar_start_at_unix_secs = reminder.at_unix_secs;
            reminder.calendar_end_at_unix_secs = reminder
                .at_unix_secs
                .saturating_add(DEFAULT_REMINDER_CALENDAR_DURATION_SECS);
        }
        let desired_account_key = if provider == CALENDAR_PROVIDER_LOCAL {
            String::new()
        } else {
            reminder.calendar_account_key.clone()
        };
        let link_changed = !previous_link.is_empty()
            && (previous_link.provider != provider
                || previous_link.account_key != desired_account_key);
        if link_changed {
            self.delete_calendar_link(&previous_link, ctx)?;
            self.clear_calendar_link(reminder);
        } else if !previous_link.is_empty() {
            reminder.calendar_event_id = previous_link.event_id.clone();
            reminder.calendar_calendar_id = previous_link.calendar_id.clone();
            reminder.calendar_remote_id = previous_link.remote_id.clone();
        }
        let start_at = reminder.calendar_start_at_unix_secs;
        let end_at = reminder.calendar_end_at_unix_secs;
        let mut event = CalendarEvent {
            id: if reminder.calendar_event_id.is_empty() {
                format!("remcal_{}", reminder.id)
            } else {
                reminder.calendar_event_id.clone()
            },
            title: reminder.context.clone(),
            start_at_unix_secs: start_at,
            end_at_unix_secs: end_at,
            timezone: reminder.calendar_timezone.clone(),
            location: reminder.calendar_location.clone(),
            notes: if reminder.calendar_notes.trim().is_empty() {
                reminder.context.clone()
            } else {
                reminder.calendar_notes.clone()
            },
            provider: provider.clone(),
            calendar_id: reminder.calendar_calendar_id.clone(),
            remote_id: reminder.calendar_remote_id.clone(),
            status: CalendarEventStatus::Confirmed,
            updated_at: reminder.updated_at,
        };
        event = normalize_calendar_event(event)?;
        if provider == CALENDAR_PROVIDER_LOCAL {
            let local_store = self.local_calendar_store.as_ref().ok_or_else(|| {
                Error::config(
                    "remind_at",
                    "local calendar store unavailable for reminder bridge",
                )
            })?;
            local_store.upsert(&event)?;
            let stored = local_store.get(&event.id)?.ok_or_else(|| {
                Error::config("remind_at", "calendar event missing after reminder upsert")
            })?;
            reminder.calendar_event_id = stored.id;
            reminder.calendar_provider = stored.provider;
            reminder.calendar_account_key.clear();
            reminder.calendar_calendar_id = stored.calendar_id;
            reminder.calendar_remote_id = stored.remote_id;
            return Ok(());
        }
        self.upsert_remote_calendar_link(
            reminder,
            provider.as_str(),
            &event,
            &previous_link,
            link_changed,
            ctx,
        )
    }

    fn delete_calendar_link(
        &self,
        link: &ReminderCalendarLink,
        _ctx: &mut dyn ToolContext,
    ) -> Result<()> {
        if link.is_empty() {
            return Ok(());
        }
        if link.is_local() {
            if let Some(local_store) = self.local_calendar_store.as_ref() {
                let _ = local_store.delete(&link.event_id)?;
            }
            return Ok(());
        }
        self.delete_remote_calendar_link(link, _ctx)
    }

    fn clear_calendar_link(&self, reminder: &mut ReminderItem) {
        reminder.calendar_event_id.clear();
        reminder.calendar_provider.clear();
        reminder.calendar_account_key.clear();
        reminder.calendar_calendar_id.clear();
        reminder.calendar_remote_id.clear();
        reminder.calendar_start_at_unix_secs = 0;
        reminder.calendar_end_at_unix_secs = 0;
        reminder.calendar_timezone.clear();
        reminder.calendar_location.clear();
        reminder.calendar_notes.clear();
    }
}

/// 将 at 解析为 Unix 秒：数字直接使用；字符串先尝试 u64，否则按 ISO8601 简式解析（YYYY-MM-DDTHH:MM:SS 或带 Z）。
fn parse_at_to_unix_secs(v: &Value) -> Result<u64> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| Error::config("remind_at", "at must be non-negative number")),
        Value::String(s) => parse_iso8601(s)
            .ok_or_else(|| Error::config("remind_at", "at must be Unix seconds or ISO8601 string")),
        _ => Err(Error::config("remind_at", "at must be number or string")),
    }
}

fn required_context(obj: &serde_json::Map<String, Value>) -> Result<&str> {
    obj.get("context")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::config("remind_at", "missing context"))
}

fn parse_optional_time(value: Option<&Value>, field: &'static str) -> Result<Option<u64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::Number(number) => number
            .as_u64()
            .map(Some)
            .ok_or_else(|| Error::config("remind_at", format!("{field} must be non-negative"))),
        Value::String(string) => parse_iso8601(string).map(Some).ok_or_else(|| {
            Error::config(
                "remind_at",
                format!("{field} must be Unix seconds or ISO8601 string"),
            )
        }),
        _ => Err(Error::config(
            "remind_at",
            format!("{field} must be number, string, or null"),
        )),
    }
}

fn populate_calendar_request(
    reminder: &mut ReminderItem,
    obj: &serde_json::Map<String, Value>,
    at_secs: u64,
) -> Result<()> {
    let provider = nullable_string(obj.get("calendar_provider"));
    let account_key = nullable_string(obj.get("calendar_account_key"));
    let calendar_id = nullable_string(obj.get("calendar_id"));
    let timezone = nullable_string(obj.get("calendar_timezone"));
    let location = nullable_string(obj.get("calendar_location"));
    let notes = nullable_string(obj.get("calendar_notes"));
    let explicit_calendar_signal = !provider.is_empty()
        || !account_key.is_empty()
        || !calendar_id.is_empty()
        || !timezone.is_empty()
        || !location.is_empty()
        || !notes.is_empty()
        || obj.contains_key("calendar_end_at");
    if !explicit_calendar_signal {
        return Ok(());
    }
    if provider.is_empty() {
        return Err(Error::config(
            "remind_at",
            "calendar_provider is required when calendar bridge fields are provided",
        ));
    }
    reminder.calendar_provider = provider;
    reminder.calendar_account_key = account_key;
    reminder.calendar_calendar_id = calendar_id;
    reminder.calendar_start_at_unix_secs = at_secs;
    reminder.calendar_end_at_unix_secs =
        parse_optional_time(obj.get("calendar_end_at"), "calendar_end_at")?
            .unwrap_or(at_secs.saturating_add(DEFAULT_REMINDER_CALENDAR_DURATION_SECS));
    reminder.calendar_timezone = timezone;
    reminder.calendar_location = location;
    reminder.calendar_notes = notes;
    Ok(())
}

fn desired_calendar_provider(reminder: &ReminderItem) -> Option<String> {
    let provider = reminder.calendar_provider.trim();
    (!provider.is_empty()).then(|| provider.to_string())
}

fn required_id(obj: &serde_json::Map<String, Value>) -> Result<&str> {
    obj.get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::config("remind_at", "missing id"))
}

fn nullable_string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}

fn next_reminder_id() -> String {
    format!("rem_{}", REMINDER_SEQ.fetch_add(1, Ordering::Relaxed))
}

impl Tool for RemindAtTool {
    fn name(&self) -> &'static str {
        "remind_at"
    }

    fn description(&self) -> &'static str {
        "Manage persistent reminders. Ops: schedule (default), get, update, delete. Reminders can optionally stay linked to local or office calendar events."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: schedule|get|update|delete. Defaults to schedule."},"id":{"type":"string","description":"Reminder id for get, update, or delete."},"at":{"description":"ISO8601 (e.g. 2025-03-10T12:00:00Z) or Unix seconds","oneOf":[{"type":"number"},{"type":"string"}]},"context":{"description":"Reminder text to show at that time","type":"string"},"calendar_provider":{"type":"string","description":"Optional calendar provider to bridge this reminder into a calendar event. Use local for the local calendar."},"calendar_account_key":{"type":"string","description":"Optional office calendar account key when the provider has multiple accounts."},"calendar_id":{"type":"string","description":"Optional calendar target ID when the provider supports multiple calendars."},"calendar_end_at":{"description":"Optional calendar event end time as Unix seconds, ISO8601, or null. Defaults to 15 minutes after at.","oneOf":[{"type":"number"},{"type":"string"},{"type":"null"}]},"calendar_timezone":{"type":"string","description":"Optional calendar timezone label"},"calendar_location":{"type":"string","description":"Optional calendar location"},"calendar_notes":{"type":"string","description":"Optional calendar notes"},"clear_calendar":{"type":"boolean","description":"Remove the linked calendar event and clear reminder calendar bridge fields during update."}}}"#
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

impl ReminderCalendarLink {
    fn from_reminder(reminder: &ReminderItem) -> Self {
        Self {
            provider: reminder.calendar_provider.clone(),
            account_key: reminder.calendar_account_key.clone(),
            event_id: reminder.calendar_event_id.clone(),
            calendar_id: reminder.calendar_calendar_id.clone(),
            remote_id: reminder.calendar_remote_id.clone(),
        }
    }

    fn is_empty(&self) -> bool {
        self.event_id.trim().is_empty() || self.provider.trim().is_empty()
    }

    fn is_local(&self) -> bool {
        self.provider == CALENDAR_PROVIDER_LOCAL
    }
}

/// remind_list 工具：查询当前会话未到点提醒。
pub struct RemindListTool {
    store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
}

impl RemindListTool {
    pub fn new(store: std::sync::Arc<dyn crate::memory::RemindAtStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl Tool for RemindListTool {
    fn name(&self) -> &'static str {
        "remind_list"
    }

    fn description(&self) -> &'static str {
        "List upcoming reminders for current chat. Args: limit (optional, default 10, max 20)."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"limit":{"type":"number","description":"max items to return, default 10, max 20"}}}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let chat_id = ctx.current_chat_id().ok_or_else(|| {
            Error::config(
                "remind_list",
                "no current chat_id (tool used outside session)",
            )
        })?;
        let channel = ctx.current_channel().ok_or_else(|| {
            Error::config(
                "remind_list",
                "no current channel (tool used outside session)",
            )
        })?;
        let obj = parse_tool_args(args, "remind_list")?;
        let limit = obj
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 20) as usize;
        let now = current_unix_secs();
        let items = self.store.list_upcoming(channel, chat_id, now, limit)?;
        serialize_tool_output(
            "remind_list",
            &RemindListResponse {
                count: items.len(),
                items,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::calendar::{
        CalendarHttpClient, CalendarOperation, CalendarProvider, CalendarProviderCredential,
        CalendarProviderCredentialStatus, CalendarProviderCredentialStore,
        CalendarProviderRegistry, CalendarService,
    };
    use crate::calendar::{CalendarQuery, CalendarStore};
    use crate::error::Result;
    use crate::memory::RemindAtStore;
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::office::{
        OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
        OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore, OfficeSelectionPolicy,
        OfficeService,
    };
    use crate::platform::ResponseBody;
    use crate::tools::ToolContext;
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    use crate::tools::ToolExecutionFailureKind;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct StubRemindStore {
        items: Mutex<BTreeMap<String, ReminderItem>>,
    }

    impl crate::memory::RemindAtStore for StubRemindStore {
        fn get(&self, _channel: &str, _chat_id: &str, id: &str) -> Result<Option<ReminderItem>> {
            Ok(self.items.lock().expect("items lock").get(id).cloned())
        }

        fn upsert(&self, reminder: &ReminderItem) -> Result<()> {
            self.items
                .lock()
                .expect("items lock")
                .insert(reminder.id.clone(), reminder.clone());
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, id: &str) -> Result<bool> {
            Ok(self.items.lock().expect("items lock").remove(id).is_some())
        }

        fn pop_due(&self, _now_unix_secs: u64) -> Result<Option<ReminderItem>> {
            Ok(None)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<ReminderItem>> {
            Ok(self
                .items
                .lock()
                .expect("items lock")
                .values()
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct StubCalendarStore {
        items: Mutex<BTreeMap<String, CalendarEvent>>,
    }

    impl CalendarStore for StubCalendarStore {
        fn list(&self, _query: CalendarQuery) -> Result<Vec<CalendarEvent>> {
            Ok(self
                .items
                .lock()
                .expect("calendar lock")
                .values()
                .cloned()
                .collect())
        }

        fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
            Ok(self.items.lock().expect("calendar lock").get(id).cloned())
        }

        fn upsert(&self, event: &CalendarEvent) -> Result<()> {
            self.items
                .lock()
                .expect("calendar lock")
                .insert(event.id.clone(), event.clone());
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<bool> {
            Ok(self
                .items
                .lock()
                .expect("calendar lock")
                .remove(id)
                .is_some())
        }
    }

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn request_with_headers(
            &mut self,
            _method: &str,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: Option<&[u8]>,
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn patch_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn put_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn delete_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Err(crate::error::Error::config("dummy_ctx", "http unsupported"))
        }

        fn current_chat_id(&self) -> Option<&str> {
            Some("chat")
        }

        fn current_channel(&self) -> Option<&str> {
            Some("qq_channel")
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn remind_at_tool_can_link_local_calendar_event() {
        let remind_store = Arc::new(StubRemindStore::default());
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let remind_store_dyn: Arc<dyn crate::memory::RemindAtStore + Send + Sync> =
            remind_store.clone();
        let tool = RemindAtTool::with_local_calendar(remind_store_dyn, calendar_store);
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"客户回访","calendar_provider":"local"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");

        assert_eq!(created["ok"], true);
        assert_eq!(created["reminder"]["calendar_provider"], "local");
        let event_id = created["reminder"]["calendar_event_id"]
            .as_str()
            .expect("calendar event id");
        let event = calendar_store_impl
            .get(event_id)
            .expect("event lookup")
            .expect("event");
        assert_eq!(event.title, "客户回访");
        assert_eq!(event.status, CalendarEventStatus::Confirmed);
    }

    #[test]
    fn remind_at_tool_get_returns_saved_reminder() {
        let remind_store = Arc::new(StubRemindStore::default());
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let remind_store_dyn: Arc<dyn crate::memory::RemindAtStore + Send + Sync> =
            remind_store.clone();
        let tool = RemindAtTool::with_local_calendar(remind_store_dyn, calendar_store);
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"客户回访","calendar_provider":"local"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");
        let reminder_id = created["reminder"]["id"].as_str().expect("reminder id");

        let fetched = tool
            .execute(
                &format!(r#"{{"op":"get","id":"{}"}}"#, reminder_id),
                &mut ctx,
            )
            .expect("get reminder");
        let fetched: Value = serde_json::from_str(&fetched).expect("get payload");

        assert_eq!(fetched["op"], "get");
        assert_eq!(fetched["reminder"]["id"], reminder_id);
        assert_eq!(fetched["reminder"]["context"], "客户回访");
        assert_eq!(fetched["reminder"]["calendar_provider"], "local");
    }

    #[test]
    fn remind_at_tool_update_updates_linked_local_calendar_event() {
        let remind_store = Arc::new(StubRemindStore::default());
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let remind_store_dyn: Arc<dyn crate::memory::RemindAtStore + Send + Sync> =
            remind_store.clone();
        let tool = RemindAtTool::with_local_calendar(remind_store_dyn, calendar_store);
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"客户回访","calendar_provider":"local"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");
        let reminder_id = created["reminder"]["id"].as_str().expect("reminder id");
        let event_id = created["reminder"]["calendar_event_id"]
            .as_str()
            .expect("calendar event id");

        let updated = tool
            .execute(
                &format!(
                    r#"{{"op":"update","id":"{}","at":1700003600,"context":"客户回访（改期）","calendar_location":"会议室 A"}} "#,
                    reminder_id
                ),
                &mut ctx,
            )
            .expect("update reminder");
        let updated: Value = serde_json::from_str(&updated).expect("update payload");

        assert_eq!(updated["ok"], true);
        assert_eq!(updated["reminder"]["calendar_provider"], "local");
        let event = calendar_store_impl
            .get(event_id)
            .expect("event lookup")
            .expect("event");
        assert_eq!(event.title, "客户回访（改期）");
        assert_eq!(event.location, "会议室 A");
        assert_eq!(event.start_at_unix_secs, 1_700_003_600);
        assert_eq!(
            event.end_at_unix_secs,
            1_700_003_600 + DEFAULT_REMINDER_CALENDAR_DURATION_SECS
        );
    }

    #[test]
    fn remind_at_tool_delete_clears_linked_local_calendar_event() {
        let remind_store = Arc::new(StubRemindStore::default());
        let calendar_store_impl = Arc::new(StubCalendarStore::default());
        let calendar_store: Arc<dyn CalendarStore + Send + Sync> = calendar_store_impl.clone();
        let remind_store_dyn: Arc<dyn crate::memory::RemindAtStore + Send + Sync> =
            remind_store.clone();
        let tool = RemindAtTool::with_local_calendar(remind_store_dyn, calendar_store);
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"客户回访","calendar_provider":"local"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");
        let reminder_id = created["reminder"]["id"].as_str().expect("reminder id");
        let event_id = created["reminder"]["calendar_event_id"]
            .as_str()
            .expect("calendar event id")
            .to_string();

        let deleted = tool
            .execute(
                &format!(r#"{{"op":"delete","id":"{}"}}"#, reminder_id),
                &mut ctx,
            )
            .expect("delete reminder");
        let deleted: Value = serde_json::from_str(&deleted).expect("delete payload");

        assert_eq!(deleted["ok"], true);
        assert!(remind_store
            .get("qq_channel", "chat", reminder_id)
            .expect("reminder lookup")
            .is_none());
        assert!(calendar_store_impl
            .get(&event_id)
            .expect("event lookup")
            .is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn remind_at_tool_routes_remote_calendar_through_office_default_account() {
        let provider = Arc::new(RecordingRemoteProvider::default());
        let tool = RemindAtTool::with_office_calendar_service(
            Arc::new(StubRemindStore::default()),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(provider.clone()),
        );
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"周一例会提醒","calendar_provider":"mock_remote"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");

        assert_eq!(created["reminder"]["calendar_provider"], "mock_remote");
        assert_eq!(created["reminder"]["calendar_account_key"], "calendar-work");
        let calls = provider.calls();
        assert_eq!(calls.len(), 1);
        assert!(matches!(
            &calls[0],
            RemoteCall::Create { account_key, event }
                if account_key == "calendar-work"
                    && event.provider == "mock_remote"
                    && event.title == "周一例会提醒"
        ));
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn remind_at_tool_update_updates_remote_calendar_event() {
        let provider = Arc::new(RecordingRemoteProvider::default());
        let tool = RemindAtTool::with_office_calendar_service(
            Arc::new(StubRemindStore::default()),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(provider.clone()),
        );
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"周一例会提醒","calendar_provider":"mock_remote"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");
        let reminder_id = created["reminder"]["id"].as_str().expect("reminder id");
        let event_id = created["reminder"]["calendar_event_id"]
            .as_str()
            .expect("calendar event id")
            .to_string();

        let updated = tool
            .execute(
                &format!(
                    r#"{{"op":"update","id":"{}","at":1700007200,"context":"周一例会提醒（改期）","calendar_location":"大会议室"}} "#,
                    reminder_id
                ),
                &mut ctx,
            )
            .expect("update reminder");
        let updated: Value = serde_json::from_str(&updated).expect("update payload");

        assert_eq!(updated["ok"], true);
        let calls = provider.calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            &calls[1],
            RemoteCall::Update { account_key, event }
                if account_key == "calendar-work"
                    && event.id == event_id
                    && event.title == "周一例会提醒（改期）"
                    && event.location == "大会议室"
                    && event.start_at_unix_secs == 1_700_007_200
        ));
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn remind_at_tool_delete_clears_remote_calendar_event() {
        let remind_store = Arc::new(StubRemindStore::default());
        let provider = Arc::new(RecordingRemoteProvider::default());
        let tool = RemindAtTool::with_office_calendar_service(
            remind_store.clone(),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(provider.clone()),
        );
        let mut ctx = DummyCtx;

        let created = tool
            .execute(
                r#"{"at":1700000000,"context":"周一例会提醒","calendar_provider":"mock_remote"}"#,
                &mut ctx,
            )
            .expect("create reminder");
        let created: Value = serde_json::from_str(&created).expect("create payload");
        let reminder_id = created["reminder"]["id"].as_str().expect("reminder id");
        let event_id = created["reminder"]["calendar_event_id"]
            .as_str()
            .expect("calendar event id")
            .to_string();

        let deleted = tool
            .execute(
                &format!(r#"{{"op":"delete","id":"{}"}}"#, reminder_id),
                &mut ctx,
            )
            .expect("delete reminder");
        let deleted: Value = serde_json::from_str(&deleted).expect("delete payload");

        assert_eq!(deleted["ok"], true);
        let calls = provider.calls();
        assert_eq!(calls.len(), 2);
        assert!(matches!(
            &calls[1],
            RemoteCall::Delete { account_key, id }
                if account_key == "calendar-work" && id == &event_id
        ));
        assert!(remind_store
            .get("qq_channel", "chat", reminder_id)
            .expect("reminder lookup")
            .is_none());
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[test]
    fn remind_at_tool_remote_calendar_failure_returns_structured_office_failure() {
        let tool = RemindAtTool::with_office_calendar_service(
            Arc::new(StubRemindStore::default()),
            Arc::new(StubCalendarStore::default()),
            build_remote_calendar_service(Arc::new(FailingRemoteProvider)),
        );
        let mut ctx = DummyCtx;

        let outcome = tool
            .execute_outcome(
                r#"{"at":1700000000,"context":"客户提醒","calendar_provider":"mock_remote"}"#,
                &mut ctx,
            )
            .expect("structured failure outcome");
        let payload: Value = serde_json::from_str(&outcome.content).expect("failure payload");

        assert_eq!(
            outcome.failure_kind,
            Some(ToolExecutionFailureKind::Permanent)
        );
        assert_eq!(payload["ok"], false);
        assert_eq!(payload["office_assessment"]["capability"], "calendar");
        assert_eq!(
            payload["office_assessment"]["default_account_key"],
            "calendar-work"
        );
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
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl RecordingRemoteProvider {
        fn calls(&self) -> Vec<RemoteCall> {
            self.calls.lock().expect("calls lock").clone()
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

        fn supports(&self, op: CalendarOperation) -> bool {
            matches!(
                op,
                CalendarOperation::Create | CalendarOperation::Update | CalendarOperation::Delete
            )
        }

        fn list_events(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Ok(Vec::new())
        }

        fn get_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Ok(None)
        }

        fn create_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(RemoteCall::Create {
                    account_key: credential.account_key.clone(),
                    event: event.clone(),
                });
            let mut event = event.clone();
            event.calendar_id = "team".to_string();
            event.remote_id = format!("remote-{}", event.id);
            Ok(event)
        }

        fn update_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            credential: &CalendarProviderCredential,
            event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(RemoteCall::Update {
                    account_key: credential.account_key.clone(),
                    event: event.clone(),
                });
            Ok(event.clone())
        }

        fn delete_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            credential: &CalendarProviderCredential,
            id: &str,
        ) -> Result<bool> {
            self.calls
                .lock()
                .expect("calls lock")
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
            matches!(op, CalendarOperation::Create)
        }

        fn list_events(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _query: CalendarQuery,
        ) -> Result<Vec<CalendarEvent>> {
            Ok(Vec::new())
        }

        fn get_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<Option<CalendarEvent>> {
            Ok(None)
        }

        fn create_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(crate::error::Error::config(
                "remind_at_test_provider",
                "remote calendar unavailable",
            ))
        }

        fn update_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _event: &CalendarEvent,
        ) -> Result<CalendarEvent> {
            Err(crate::error::Error::config(
                "remind_at_test_provider",
                "update unsupported",
            ))
        }

        fn delete_event(
            &self,
            _http: &mut dyn CalendarHttpClient,
            _credential: &CalendarProviderCredential,
            _id: &str,
        ) -> Result<bool> {
            Err(crate::error::Error::config(
                "remind_at_test_provider",
                "delete unsupported",
            ))
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubCalendarCredentialStore {
        items: Mutex<BTreeMap<String, CalendarProviderCredential>>,
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl StubCalendarCredentialStore {
        fn seed(&self, credential: &CalendarProviderCredential) {
            self.items
                .lock()
                .expect("credential lock")
                .insert(credential.account_key.clone(), credential.clone());
        }
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
                .expect("credential lock")
                .get(account_key)
                .cloned())
        }

        fn find_account_keys_by_provider(&self, provider: &str) -> Result<Vec<String>> {
            Ok(self
                .items
                .lock()
                .expect("credential lock")
                .values()
                .filter(|credential| credential.provider == provider)
                .map(|credential| credential.account_key.clone())
                .collect())
        }

        fn set(&self, credential: &CalendarProviderCredential) -> Result<()> {
            self.seed(credential);
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .expect("credential lock")
                .remove(account_key);
            Ok(())
        }

        fn list_statuses(&self) -> Result<Vec<CalendarProviderCredentialStatus>> {
            Ok(self
                .items
                .lock()
                .expect("credential lock")
                .values()
                .map(CalendarProviderCredential::status)
                .collect())
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubOfficeCredentialStore;

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl OfficeCredentialStore for StubOfficeCredentialStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(Vec::new())
        }

        fn set(&self, _credential: &OfficeCredential) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    #[derive(Default)]
    struct StubRuntimeStatusStore;

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(
            &self,
            _account_key: &str,
        ) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }

        fn set(&self, _status: &crate::office::OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    fn build_remote_calendar_service(provider: Arc<dyn CalendarProvider>) -> CalendarService {
        let credential_store = Arc::new(StubCalendarCredentialStore::default());
        credential_store.seed(&CalendarProviderCredential {
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
        });
        let mut registry = OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "calendar-work".to_string(),
            provider_kind: "mock_remote".to_string(),
            external_account_id: "work@example.com".to_string(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Calendar],
        });
        let mut binding = crate::office::OfficeCapabilityBinding::default();
        binding.set_default_account(OfficeCapability::Calendar, "calendar-work".to_string());
        let office_service = OfficeService::new(
            registry,
            binding,
            OfficeSelectionPolicy::default(),
            Arc::new(StubOfficeCredentialStore),
            Arc::new(StubRuntimeStatusStore),
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
}
