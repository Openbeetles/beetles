//! 世界感知层：把当前外部处境压缩成可持续的世界感觉。

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::reminder::ReminderItem;
use crate::task::{TaskPriority, TaskQuery, TaskStatus, TaskStore};
use crate::util::{epoch_to_ymdhms, scrub_credentials, truncate_content_to_max, weekday_name};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use super::{
    board_subject_scope_id,
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, relationship_scope_id, render_autonomy_strategy_block,
    render_execution_state_block, render_self_continuity_block, whole_record_lease_advanced,
    AutonomyStrategy, AutonomyStrategyStore, ExecutionState, ExecutionStateStore, MemoryProfile,
    RemindAtStore, SelfContinuity, SelfContinuityStore, SessionMessage, SessionStore,
    SessionSummaryStore, WorldSensePolicy, WorldSenseStore,
};

pub const WORLD_SENSE_SYSTEM_PROMPT: &str = "You maintain the assistant's private world-sense layer. Return JSON only: either null or one object with fields current_scene, body_state, social_field, world_changes, external_focus. This layer describes the outer situation you currently feel yourself to be in: environment, device/body condition, interaction field, and what in the outside world deserves attention now. Do not write self-model, inner-life drift, or transcript summary. Keep it compact, situational, and current.";

const WORLD_SENSE_FIELD_MAX_CHARS: usize = 220;
pub const WORLD_SENSE_TOTAL_CHAR_LIMIT: usize = WORLD_SENSE_FIELD_MAX_CHARS * 5;
const WORLD_TASK_QUERY_LIMIT: usize = 50;
const WORLD_REMINDER_PREVIEW_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSnapshot {
    pub weekday: String,
    pub hour: u32,
    pub day_phase: String,
    pub interaction_mode: String,
    pub activity_rhythm: String,
    pub situational_pull: String,
    pub resource_tension: String,
    pub pressure: PressureLevel,
    pub memory_available_bytes: u32,
    pub active_http_count: u32,
    pub active_wss_count: u32,
    pub active_agent_tasks: u32,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    pub storage_used_kb: u32,
    pub storage_total_kb: u32,
    pub wifi_connected: bool,
    pub audio_recording: bool,
    pub audio_playing: bool,
    pub source_channel: String,
    pub open_tasks: usize,
    pub in_progress_tasks: usize,
    pub due_tasks: usize,
    pub high_priority_tasks: usize,
    pub upcoming_reminders: usize,
    pub next_reminder_at: u64,
    pub user_idle_secs: u64,
    pub autonomy_idle_secs: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldSense {
    #[serde(default)]
    pub current_scene: String,
    #[serde(default)]
    pub body_state: String,
    #[serde(default)]
    pub social_field: String,
    #[serde(default)]
    pub world_changes: String,
    #[serde(default)]
    pub external_focus: String,
    #[serde(default)]
    pub source_fingerprint: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl WorldSense {
    pub fn is_meaningful(&self) -> bool {
        !self.current_scene.trim().is_empty()
            || !self.body_state.trim().is_empty()
            || !self.social_field.trim().is_empty()
            || !self.world_changes.trim().is_empty()
            || !self.external_focus.trim().is_empty()
    }
}

#[derive(Clone, Copy)]
pub struct WorldSnapshotContext<'a> {
    pub chat_id: &'a str,
    pub source_channel: &'a str,
    pub now_secs: u64,
    pub self_continuity: Option<&'a SelfContinuity>,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldSenseRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct WorldSenseRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldSenseRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

impl WorldSensePolicy {
    pub(crate) fn should_refresh(
        self,
        input: WorldSenseRefreshInput<'_>,
        has_existing: bool,
    ) -> bool {
        if input.ingress != IngressKind::User || input.channel == "cron" {
            return false;
        }
        if input.pressure != PressureLevel::Normal {
            return false;
        }
        let user = input.user_content.trim();
        let reply = input.reply_content.trim();
        if user.is_empty() || reply.is_empty() {
            return false;
        }
        if input.tool_calls > 0 {
            return true;
        }
        let user_chars = user.chars().count();
        let reply_chars = reply.chars().count();
        let combined = user_chars.saturating_add(reply_chars);
        let substantive = user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n');
        substantive || has_existing
    }
}

pub(crate) fn load_world_snapshot_reminders(
    ctx: WorldSnapshotContext<'_>,
) -> Result<Vec<ReminderItem>> {
    let source_channel = normalize_channel(ctx.source_channel);
    if source_channel.is_empty() {
        return Ok(Vec::new());
    }
    ctx.remind_store.list_upcoming(
        source_channel.as_ref(),
        ctx.chat_id,
        ctx.now_secs,
        WORLD_REMINDER_PREVIEW_LIMIT,
    )
}

pub(crate) fn load_world_snapshot_tasks(
    ctx: WorldSnapshotContext<'_>,
) -> Result<Vec<crate::task::TaskItem>> {
    let source_channel = normalize_channel(ctx.source_channel);
    if source_channel.is_empty() {
        return Ok(Vec::new());
    }
    ctx.task_store.list(
        source_channel.as_ref(),
        ctx.chat_id,
        TaskQuery {
            include_completed: false,
            limit: WORLD_TASK_QUERY_LIMIT,
            ..TaskQuery::default()
        },
    )
}

pub(crate) fn build_world_snapshot_from_commitments(
    ctx: WorldSnapshotContext<'_>,
    reminders: &[ReminderItem],
    tasks: &[crate::task::TaskItem],
) -> WorldSnapshot {
    let resource = crate::orchestrator::snapshot();
    let (_, _, _, hour, _, _) = epoch_to_ymdhms(ctx.now_secs);
    let weekday = weekday_name(ctx.now_secs / 86400).to_string();
    let day_phase = describe_day_phase(hour).to_string();
    let source_channel = normalize_channel(ctx.source_channel).to_string();
    let open_tasks = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Open)
        .count();
    let in_progress_tasks = tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InProgress)
        .count();
    let due_tasks = tasks
        .iter()
        .filter(|task| task.due_at_unix_secs > 0 && task.due_at_unix_secs <= ctx.now_secs)
        .count();
    let high_priority_tasks = tasks
        .iter()
        .filter(|task| task.priority == TaskPriority::High)
        .count();
    let user_idle_secs = ctx.self_continuity.map_or(0, |continuity| {
        ctx.now_secs.saturating_sub(continuity.last_user_turn_at)
    });
    let autonomy_idle_secs = ctx.self_continuity.map_or(0, |continuity| {
        ctx.now_secs.saturating_sub(continuity.last_autonomy_run_at)
    });
    let interaction_mode =
        describe_interaction_mode(source_channel.as_str(), user_idle_secs, autonomy_idle_secs)
            .to_string();
    let activity_rhythm = describe_activity_rhythm(
        user_idle_secs,
        autonomy_idle_secs,
        resource.active_http_count,
        resource.active_agent_tasks,
        reminders.len(),
        due_tasks,
        in_progress_tasks,
    )
    .to_string();
    let situational_pull = describe_situational_pull(
        source_channel.as_str(),
        user_idle_secs,
        due_tasks,
        high_priority_tasks,
        reminders.first().map(|item| item.at_unix_secs).unwrap_or(0),
        ctx.now_secs,
    )
    .to_string();
    let resource_tension = describe_resource_tension(
        resource.pressure,
        resource.heap_free_internal,
        resource.storage_used_kb,
        resource.storage_total_kb,
        resource.inbound_depth,
        resource.outbound_depth,
    )
    .to_string();
    WorldSnapshot {
        weekday,
        hour,
        day_phase,
        interaction_mode,
        activity_rhythm,
        situational_pull,
        resource_tension,
        pressure: resource.pressure,
        memory_available_bytes: resource.heap_free_internal,
        active_http_count: resource.active_http_count,
        active_wss_count: resource.active_wss_count,
        active_agent_tasks: resource.active_agent_tasks,
        inbound_depth: resource.inbound_depth,
        outbound_depth: resource.outbound_depth,
        storage_used_kb: resource.storage_used_kb,
        storage_total_kb: resource.storage_total_kb,
        wifi_connected: crate::state::wifi_sta_connected(),
        audio_recording: resource.audio_recording,
        audio_playing: resource.audio_playing,
        source_channel,
        open_tasks,
        in_progress_tasks,
        due_tasks,
        high_priority_tasks,
        upcoming_reminders: reminders.len(),
        next_reminder_at: reminders.first().map(|item| item.at_unix_secs).unwrap_or(0),
        user_idle_secs,
        autonomy_idle_secs,
    }
}

pub fn build_world_snapshot(ctx: WorldSnapshotContext<'_>) -> Result<WorldSnapshot> {
    let reminders = load_world_snapshot_reminders(ctx)?;
    let tasks = load_world_snapshot_tasks(ctx)?;
    Ok(build_world_snapshot_from_commitments(
        ctx, &reminders, &tasks,
    ))
}

pub fn render_world_snapshot_block(snapshot: &WorldSnapshot, max_len: usize) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## World Snapshot\n");
    let _ = writeln!(
        out,
        "Outer scene now: {} {}:00-{}:59, {}.",
        snapshot.weekday, snapshot.hour, snapshot.hour, snapshot.day_phase
    );
    let _ = writeln!(
        out,
        "Derived state: interaction_mode={}, activity_rhythm={}, situational_pull={}, resource_tension={}.",
        snapshot.interaction_mode,
        snapshot.activity_rhythm,
        snapshot.situational_pull,
        snapshot.resource_tension
    );
    let _ = writeln!(
        out,
        "Device/body: pressure={:?}, wifi_connected={}, mem_available_kb={}, active_http={}, active_wss={}, audio_recording={}, audio_playing={}.",
        snapshot.pressure,
        snapshot.wifi_connected,
        snapshot.memory_available_bytes / 1024,
        snapshot.active_http_count,
        snapshot.active_wss_count,
        snapshot.audio_recording,
        snapshot.audio_playing
    );
    let _ = writeln!(
        out,
        "Runtime load: agent_tasks={}, inbound_depth={}, outbound_depth={}, storage={} / {} KB.",
        snapshot.active_agent_tasks,
        snapshot.inbound_depth,
        snapshot.outbound_depth,
        snapshot.storage_used_kb,
        snapshot.storage_total_kb
    );
    if !snapshot.source_channel.is_empty() {
        let _ = writeln!(
            out,
            "Interaction field: channel={}.",
            snapshot.source_channel
        );
    }
    let _ = writeln!(
        out,
        "Task/reminder field: open_tasks={}, in_progress_tasks={}, due_tasks={}, high_priority_tasks={}, upcoming_reminders={}, next_reminder_at={}.",
        snapshot.open_tasks,
        snapshot.in_progress_tasks,
        snapshot.due_tasks,
        snapshot.high_priority_tasks,
        snapshot.upcoming_reminders,
        snapshot.next_reminder_at
    );
    let _ = writeln!(
        out,
        "Recent activity: user_idle_secs={}, autonomy_idle_secs={}.",
        snapshot.user_idle_secs, snapshot.autonomy_idle_secs
    );
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn render_world_sense_block(world_sense: &WorldSense, max_len: usize) -> Option<String> {
    let normalized = normalize_world_sense(world_sense.clone(), world_sense.updated_at)?;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## World Sense\n");
    out.push_str("Private outer-situation layer. It records what the world currently feels like around you.\n");
    if !normalized.current_scene.is_empty() {
        let _ = writeln!(out, "Current scene: {}", normalized.current_scene);
    }
    if !normalized.body_state.is_empty() {
        let _ = writeln!(out, "Body state: {}", normalized.body_state);
    }
    if !normalized.social_field.is_empty() {
        let _ = writeln!(out, "Social field: {}", normalized.social_field);
    }
    if !normalized.world_changes.is_empty() {
        let _ = writeln!(out, "World changes: {}", normalized.world_changes);
    }
    if !normalized.external_focus.is_empty() {
        let _ = writeln!(out, "External focus: {}", normalized.external_focus);
    }
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn world_snapshot_fingerprint(snapshot: &WorldSnapshot) -> u64 {
    let mut hasher = DefaultHasher::new();
    snapshot.weekday.hash(&mut hasher);
    snapshot.hour.hash(&mut hasher);
    snapshot.day_phase.hash(&mut hasher);
    snapshot.interaction_mode.hash(&mut hasher);
    snapshot.activity_rhythm.hash(&mut hasher);
    snapshot.situational_pull.hash(&mut hasher);
    snapshot.resource_tension.hash(&mut hasher);
    format!("{:?}", snapshot.pressure).hash(&mut hasher);
    snapshot.memory_available_bytes.hash(&mut hasher);
    snapshot.active_http_count.hash(&mut hasher);
    snapshot.active_wss_count.hash(&mut hasher);
    snapshot.active_agent_tasks.hash(&mut hasher);
    snapshot.inbound_depth.hash(&mut hasher);
    snapshot.outbound_depth.hash(&mut hasher);
    snapshot.storage_used_kb.hash(&mut hasher);
    snapshot.storage_total_kb.hash(&mut hasher);
    snapshot.wifi_connected.hash(&mut hasher);
    snapshot.audio_recording.hash(&mut hasher);
    snapshot.audio_playing.hash(&mut hasher);
    snapshot.source_channel.hash(&mut hasher);
    snapshot.open_tasks.hash(&mut hasher);
    snapshot.in_progress_tasks.hash(&mut hasher);
    snapshot.due_tasks.hash(&mut hasher);
    snapshot.high_priority_tasks.hash(&mut hasher);
    snapshot.upcoming_reminders.hash(&mut hasher);
    snapshot.next_reminder_at.hash(&mut hasher);
    // Do not hash the raw idle counters here. They move every second, which turns every
    // idle tick into an artificial "world changed" signal and wakes background LLM work
    // even when the qualitative world state is unchanged. The derived fields above
    // (`interaction_mode`, `activity_rhythm`, `situational_pull`) already capture the
    // meaningful bucket transitions.
    hasher.finish()
}

pub fn run_world_sense_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: WorldSenseRefreshContext<'_>,
    input: WorldSenseRefreshInput<'_>,
    snapshot: &WorldSnapshot,
    profile: MemoryProfile,
) -> Result<WorldSenseRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    let existing_world_sense = ctx.world_sense_store.get(&relationship_id)?;
    let summary_text = ctx
        .session_summary_store
        .get_with_count(input.chat_id)?
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(input.chat_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    let autonomy_strategy = ctx.autonomy_strategy_store.get(subject_id)?;
    run_world_sense_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing_world_sense,
        snapshot,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_continuity.as_ref(),
        autonomy_strategy.as_ref(),
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_world_sense_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: WorldSenseRefreshContext<'_>,
    input: WorldSenseRefreshInput<'_>,
    profile: MemoryProfile,
    existing_world_sense: Option<WorldSense>,
    snapshot: &WorldSnapshot,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_continuity: Option<&SelfContinuity>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<WorldSenseRefreshOutcome> {
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    if !decision_override.unwrap_or_else(|| {
        memory_policy(profile)
            .world_sense
            .should_refresh(input, existing_world_sense.is_some())
    }) {
        return Ok(WorldSenseRefreshOutcome::Skipped);
    }

    let policy = memory_policy(profile).world_sense;
    crate::platform::task_wdt::feed_current_task();
    let owned_recent;
    let recent = if let Some(recent) = recent_override {
        recent_window(recent, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        recent_window(owned_recent.as_slice(), policy.recent_message_count)
    };
    crate::platform::task_wdt::feed_current_task();
    let prompt = build_world_sense_refresh_input(
        existing_world_sense.as_ref(),
        snapshot,
        summary_text,
        execution_state,
        self_continuity,
        autonomy_strategy,
        recent,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    crate::platform::task_wdt::feed_current_task();
    let response = llm.chat(
        http,
        WORLD_SENSE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    match parse_world_sense_response(response.content.trim(), snapshot, input.now_secs) {
        ParsedWorldSenseResponse::Skip => Ok(WorldSenseRefreshOutcome::Skipped),
        ParsedWorldSenseResponse::Clear => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.world_sense_store.get(&relationship_id)?;
            if whole_record_lease_advanced(
                existing_world_sense.as_ref(),
                latest.as_ref(),
                existing_world_sense
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                Ok(WorldSenseRefreshOutcome::Skipped)
            } else if latest.is_some() {
                crate::platform::task_wdt::feed_current_task();
                ctx.world_sense_store.clear(&relationship_id)?;
                Ok(WorldSenseRefreshOutcome::Cleared)
            } else {
                Ok(WorldSenseRefreshOutcome::Skipped)
            }
        }
        ParsedWorldSenseResponse::Update(next) => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.world_sense_store.get(&relationship_id)?;
            let lease_advanced = whole_record_lease_advanced(
                existing_world_sense.as_ref(),
                latest.as_ref(),
                existing_world_sense
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            );
            if latest.as_ref() == Some(&next) || lease_advanced {
                Ok(WorldSenseRefreshOutcome::Skipped)
            } else {
                crate::platform::task_wdt::feed_current_task();
                ctx.world_sense_store.set(&relationship_id, &next)?;
                Ok(WorldSenseRefreshOutcome::Updated)
            }
        }
    }
}

enum ParsedWorldSenseResponse {
    Skip,
    Clear,
    Update(WorldSense),
}

fn parse_world_sense_response(
    raw: &str,
    snapshot: &WorldSnapshot,
    now_secs: u64,
) -> ParsedWorldSenseResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null => ParsedWorldSenseResponse::Clear,
        LlmJsonPayload::Absent => ParsedWorldSenseResponse::Skip,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedWorldSenseResponse::Skip;
            };
            let Some(next) = normalize_world_sense(
                WorldSense {
                    current_scene: get_object_text(object, "current_scene"),
                    body_state: get_object_text(object, "body_state"),
                    social_field: get_object_text(object, "social_field"),
                    world_changes: get_object_text(object, "world_changes"),
                    external_focus: get_object_text(object, "external_focus"),
                    source_fingerprint: world_snapshot_fingerprint(snapshot),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedWorldSenseResponse::Skip;
            };
            ParsedWorldSenseResponse::Update(next)
        }
    }
}

fn recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn normalize_world_sense(mut world_sense: WorldSense, now_secs: u64) -> Option<WorldSense> {
    world_sense.current_scene = truncate_content_to_max(
        world_sense.current_scene.trim(),
        WORLD_SENSE_FIELD_MAX_CHARS,
    )
    .trim()
    .to_string();
    world_sense.body_state =
        truncate_content_to_max(world_sense.body_state.trim(), WORLD_SENSE_FIELD_MAX_CHARS)
            .trim()
            .to_string();
    world_sense.social_field =
        truncate_content_to_max(world_sense.social_field.trim(), WORLD_SENSE_FIELD_MAX_CHARS)
            .trim()
            .to_string();
    world_sense.world_changes = truncate_content_to_max(
        world_sense.world_changes.trim(),
        WORLD_SENSE_FIELD_MAX_CHARS,
    )
    .trim()
    .to_string();
    world_sense.external_focus = truncate_content_to_max(
        world_sense.external_focus.trim(),
        WORLD_SENSE_FIELD_MAX_CHARS,
    )
    .trim()
    .to_string();
    world_sense.updated_at = now_secs;
    world_sense.is_meaningful().then_some(world_sense)
}

fn normalize_channel(channel: &str) -> &str {
    let channel = channel.trim();
    if channel.is_empty() || channel.starts_with('_') || channel == "cron" {
        ""
    } else {
        channel
    }
}

fn describe_day_phase(hour: u32) -> &'static str {
    match hour {
        5..=10 => "morning",
        11..=16 => "daytime",
        17..=21 => "evening",
        _ => "night",
    }
}

fn describe_interaction_mode(
    source_channel: &str,
    user_idle_secs: u64,
    autonomy_idle_secs: u64,
) -> &'static str {
    if source_channel.is_empty() {
        if autonomy_idle_secs > 0 && autonomy_idle_secs <= 10 * 60 {
            "self_maintenance"
        } else {
            "background_idle"
        }
    } else if source_channel == "voice" {
        "live_voice_exchange"
    } else if user_idle_secs <= 3 * 60 {
        "live_exchange"
    } else if user_idle_secs <= 30 * 60 {
        "paused_exchange"
    } else {
        "stale_thread_watch"
    }
}

fn describe_activity_rhythm(
    user_idle_secs: u64,
    autonomy_idle_secs: u64,
    active_http_count: u32,
    active_agent_tasks: u32,
    upcoming_reminders: usize,
    due_tasks: usize,
    in_progress_tasks: usize,
) -> &'static str {
    if active_http_count > 0 || active_agent_tasks > 0 {
        "active_processing"
    } else if due_tasks > 0 || in_progress_tasks > 0 {
        "task_pulled"
    } else if upcoming_reminders > 0 && user_idle_secs <= 30 * 60 {
        "lightly_primed"
    } else if user_idle_secs > 2 * 60 * 60 {
        "long_idle"
    } else if autonomy_idle_secs > 2 * 60 * 60 {
        "autonomy_dormant"
    } else {
        "steady"
    }
}

fn describe_situational_pull(
    source_channel: &str,
    user_idle_secs: u64,
    due_tasks: usize,
    high_priority_tasks: usize,
    next_reminder_at: u64,
    now_secs: u64,
) -> &'static str {
    if due_tasks > 0
        || (next_reminder_at > 0 && next_reminder_at.saturating_sub(now_secs) <= 15 * 60)
    {
        "time_sensitive"
    } else if high_priority_tasks > 0 {
        "task_followthrough"
    } else if !source_channel.is_empty() && user_idle_secs <= 5 * 60 {
        "reply_ready"
    } else if source_channel.is_empty() || user_idle_secs > 60 * 60 {
        "self_maintenance_window"
    } else {
        "light_watch"
    }
}

fn describe_resource_tension(
    pressure: PressureLevel,
    memory_available_bytes: u32,
    storage_used_kb: u32,
    storage_total_kb: u32,
    inbound_depth: u32,
    outbound_depth: u32,
) -> &'static str {
    let storage_ratio = if storage_total_kb == 0 {
        0
    } else {
        storage_used_kb.saturating_mul(100) / storage_total_kb
    };
    if pressure != PressureLevel::Normal
        || memory_available_bytes < 128 * 1024
        || storage_ratio >= 90
        || inbound_depth >= 3
        || outbound_depth >= 3
    {
        "tight"
    } else if memory_available_bytes < 512 * 1024
        || storage_ratio >= 75
        || inbound_depth > 0
        || outbound_depth > 0
    {
        "guarded"
    } else {
        "light"
    }
}

#[allow(clippy::too_many_arguments)]
fn build_world_sense_refresh_input(
    existing_world_sense: Option<&WorldSense>,
    snapshot: &WorldSnapshot,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_continuity: Option<&SelfContinuity>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    recent: &[SessionMessage],
    policy: WorldSensePolicy,
) -> String {
    let mut input = String::with_capacity(4096);
    if let Some(block) = render_world_snapshot_block(snapshot, policy.snapshot_max_len) {
        input.push_str(block.trim());
        input.push_str("\n\n");
    }
    input.push_str("## Snapshot Reading Guide\n");
    input.push_str("- current_scene should describe the present outer scene using interaction_mode, activity_rhythm, and situational_pull.\n");
    input.push_str("- body_state should translate device/body/resource_tension into a lived operational condition.\n");
    input.push_str("- social_field should capture how open, paused, direct, or distant the interaction field feels right now.\n");
    input.push_str("- world_changes should name what recently shifted in the outside situation, not your inward state.\n");
    input.push_str(
        "- external_focus should say what in the external world deserves attention next.\n\n",
    );
    if let Some(summary_text) = summary_text.filter(|text| !text.trim().is_empty()) {
        let summary = truncate_content_to_max(summary_text.trim(), policy.grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    }
    if let Some(block) = execution_state.and_then(|state| {
        render_execution_state_block(
            state,
            policy.grounding_max_len.min(
                memory_policy(MemoryProfile::Standard)
                    .execution_state
                    .render_max_len,
            ),
        )
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = self_continuity
        .and_then(|continuity| render_self_continuity_block(continuity, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = autonomy_strategy
        .and_then(|strategy| render_autonomy_strategy_block(strategy, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = existing_world_sense.and_then(|world_sense| {
        render_world_sense_block(world_sense, policy.existing_world_sense_max_len)
    }) {
        let _ = writeln!(input, "\nExisting world sense:\n{}\n", block);
    } else {
        let _ = writeln!(input, "\nExisting world sense: empty\n");
    }
    input.push_str("Recent transcript:\n");
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            input,
            "- {}: {}",
            message.role,
            scrub_credentials(preview.as_ref())
        );
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_world_sense_response_coerces_nested_fields() {
        let snapshot = WorldSnapshot {
            weekday: "Wednesday".to_string(),
            hour: 19,
            day_phase: "evening".to_string(),
            interaction_mode: "live_exchange".to_string(),
            activity_rhythm: "steady".to_string(),
            situational_pull: "reply_ready".to_string(),
            resource_tension: "light".to_string(),
            pressure: PressureLevel::Normal,
            memory_available_bytes: 512 * 1024,
            active_http_count: 1,
            active_wss_count: 1,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            storage_used_kb: 8,
            storage_total_kb: 64,
            wifi_connected: true,
            audio_recording: false,
            audio_playing: false,
            source_channel: "qq".to_string(),
            open_tasks: 1,
            in_progress_tasks: 0,
            due_tasks: 0,
            high_priority_tasks: 0,
            upcoming_reminders: 0,
            next_reminder_at: 0,
            user_idle_secs: 0,
            autonomy_idle_secs: 0,
        };
        let raw = json!({
            "current_scene": { "place": "desk", "status": ["quiet", "focused"] },
            "body_state": ["powered", "stable"],
            "social_field": "direct chat",
            "world_changes": 2,
            "external_focus": { "task": "fix parser" }
        })
        .to_string();
        let ParsedWorldSenseResponse::Update(parsed) =
            parse_world_sense_response(&raw, &snapshot, 42)
        else {
            panic!("expected parsed world sense");
        };
        assert!(parsed.current_scene.contains("place: desk"));
        assert_eq!(parsed.body_state, "powered; stable");
        assert_eq!(parsed.world_changes, "2");
        assert!(parsed.external_focus.contains("task: fix parser"));
    }

    #[test]
    fn render_world_snapshot_block_includes_outer_state() {
        let block = render_world_snapshot_block(
            &WorldSnapshot {
                weekday: "Wednesday".to_string(),
                hour: 19,
                day_phase: "evening".to_string(),
                interaction_mode: "live_exchange".to_string(),
                activity_rhythm: "steady".to_string(),
                situational_pull: "reply_ready".to_string(),
                resource_tension: "light".to_string(),
                pressure: PressureLevel::Normal,
                memory_available_bytes: 512 * 1024,
                active_http_count: 1,
                active_wss_count: 1,
                active_agent_tasks: 0,
                inbound_depth: 0,
                outbound_depth: 0,
                storage_used_kb: 8,
                storage_total_kb: 64,
                wifi_connected: true,
                audio_recording: false,
                audio_playing: false,
                source_channel: "qq_channel".to_string(),
                open_tasks: 2,
                in_progress_tasks: 1,
                due_tasks: 1,
                high_priority_tasks: 1,
                upcoming_reminders: 2,
                next_reminder_at: 123,
                user_idle_secs: 45,
                autonomy_idle_secs: 90,
            },
            512,
        )
        .expect("snapshot block");
        assert!(block.contains("## World Snapshot"));
        assert!(block.contains("qq_channel"));
        assert!(block.contains("Task/reminder field:"));
        assert!(block.contains("interaction_mode=live_exchange"));
    }

    #[test]
    fn render_world_sense_block_includes_focus() {
        let block = render_world_sense_block(
            &WorldSense {
                current_scene: "Quiet evening after a recent user exchange.".to_string(),
                body_state: "Network is stable and system pressure is low.".to_string(),
                social_field: "The user is present but not rapid-fire.".to_string(),
                world_changes: "The outside world is settling back into idle.".to_string(),
                external_focus: "Keep light watch on near-term reminders.".to_string(),
                source_fingerprint: 1,
                updated_at: 2,
            },
            512,
        )
        .expect("world sense block");
        assert!(block.contains("## World Sense"));
        assert!(block.contains("External focus"));
    }

    #[test]
    fn world_snapshot_fingerprint_ignores_raw_idle_counter_drift() {
        let first = WorldSnapshot {
            weekday: "Wednesday".to_string(),
            hour: 19,
            day_phase: "evening".to_string(),
            interaction_mode: "paused_exchange".to_string(),
            activity_rhythm: "steady".to_string(),
            situational_pull: "light_watch".to_string(),
            resource_tension: "light".to_string(),
            pressure: PressureLevel::Normal,
            memory_available_bytes: 512 * 1024,
            active_http_count: 0,
            active_wss_count: 1,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            storage_used_kb: 8,
            storage_total_kb: 64,
            wifi_connected: true,
            audio_recording: false,
            audio_playing: false,
            source_channel: "qq_channel".to_string(),
            open_tasks: 0,
            in_progress_tasks: 0,
            due_tasks: 0,
            high_priority_tasks: 0,
            upcoming_reminders: 0,
            next_reminder_at: 0,
            user_idle_secs: 600,
            autonomy_idle_secs: 120,
        };
        let mut second = first.clone();
        second.user_idle_secs = 660;
        second.autonomy_idle_secs = 180;

        assert_eq!(
            world_snapshot_fingerprint(&first),
            world_snapshot_fingerprint(&second)
        );

        second.activity_rhythm = "autonomy_dormant".to_string();
        assert_ne!(
            world_snapshot_fingerprint(&first),
            world_snapshot_fingerprint(&second)
        );
    }
}
