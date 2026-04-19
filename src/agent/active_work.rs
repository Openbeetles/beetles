//! Formal foreground active-work contract for the current chat.
//! 对当前会话前台主工作面的正式合同。

use crate::bus::PcMsg;
use crate::error::Result;
use crate::memory::{ExecutionState, ExecutionStatus};
use crate::orchestrator::snapshot as orchestrator_snapshot;
use crate::runtime::system_work::{
    CHANNEL_IDLE_MEMORY_FORGE, CHANNEL_LONG_TERM_MEMORY_REFRESH, CHANNEL_OPERATOR_MAINTENANCE,
    CHANNEL_POST_REPLY_MAINTENANCE, CHANNEL_SELF_RUNTIME,
};
use crate::task_execution::{
    current_or_next_step, TaskRunRecord, TaskRunStatus, TaskStep, TaskStepStatus,
};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::HashMap;
use std::fmt::Write as _;

pub const REL_PATH_ACTIVE_WORKS: &str = "memory/active_works.json";
pub const REL_PATH_DETACHED_WORKS: &str = "memory/detached_works.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActiveWorkKind {
    InteractiveAction,
    TaskExecution,
}

impl ActiveWorkKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InteractiveAction => "interactive_action",
            Self::TaskExecution => "task_execution",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundWorkStatus {
    Running,
    AwaitingUser,
    Suspended,
    Completed,
    Aborted,
    FailedTerminal,
}

impl ForegroundWorkStatus {
    pub const fn continuity_open(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingUser | Self::Suspended)
    }

    pub const fn default_blocks_background_llm(self) -> bool {
        matches!(self, Self::Running | Self::AwaitingUser)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Running => "active",
            Self::AwaitingUser => "awaiting_user",
            Self::Suspended => "suspended",
            Self::Completed => "completed",
            Self::Aborted => "aborted",
            Self::FailedTerminal => "failed_terminal",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundWorkRelation {
    Independent,
    ContinueExisting,
    SupplyRequestedInput,
    ReviseExisting,
    CancelExisting,
    StartNewWork,
    Ambiguous,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundWorkSettlement {
    pub kind: ActiveWorkKind,
    pub status: ForegroundWorkStatus,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub progress_summary: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub recent_outcome: String,
    #[serde(default)]
    pub active_artifact_refs: Vec<String>,
}

impl ForegroundWorkSettlement {
    pub fn into_record(self, updated_at: u64) -> Option<ActiveWorkRecord> {
        let record = ActiveWorkRecord {
            kind: self.kind,
            title: self.title,
            status: self.status,
            continuity_open: self.status.continuity_open(),
            blocks_background_llm: self.status.default_blocks_background_llm(),
            progress_summary: self.progress_summary,
            blocker: self.blocker,
            next_action: self.next_action,
            recent_outcome: self.recent_outcome,
            active_artifact_refs: self.active_artifact_refs,
            updated_at,
        };
        record.is_meaningful().then_some(record)
    }
}

impl From<&ActiveWorkRecord> for ForegroundWorkSettlement {
    fn from(value: &ActiveWorkRecord) -> Self {
        Self {
            kind: value.kind,
            status: value.status,
            title: value.title.clone(),
            progress_summary: value.progress_summary.clone(),
            blocker: value.blocker.clone(),
            next_action: value.next_action.clone(),
            recent_outcome: value.recent_outcome.clone(),
            active_artifact_refs: value.active_artifact_refs.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundWorkPacket {
    pub relation_to_last_work: ForegroundWorkRelation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<ForegroundWorkSettlement>,
}

impl ForegroundWorkPacket {
    pub const OPEN_TAG: &str = "<foreground_work_packet>";
    pub const CLOSE_TAG: &str = "</foreground_work_packet>";
}

pub(crate) const FOREGROUND_WORK_PACKET_GUIDANCE: &str = "## Foreground Work Contract\nWhen this turn creates, continues, revises, cancels, supersedes, or settles foreground work, append exactly one trailing <foreground_work_packet>...</foreground_work_packet> block after the user-facing reply. The block body must be raw JSON only with fields relation_to_last_work and settlement. relation_to_last_work must be one of independent, continue_existing, supply_requested_input, revise_existing, cancel_existing, start_new_work, ambiguous. settlement must be either null or one object with fields kind, status, title, progress_summary, blocker, next_action, recent_outcome, active_artifact_refs. kind must be interactive_action or task_execution. status must be running, awaiting_user, suspended, completed, aborted, or failed_terminal. The visible reply must remain fully user-facing and must not mention this packet. If this turn does not touch foreground work, do not append the block.";

pub(crate) fn append_foreground_work_packet_guidance(system: &mut String, max_len: usize) {
    let _ = crate::agent::context::append_capped_section(
        system,
        "\n\n",
        FOREGROUND_WORK_PACKET_GUIDANCE,
        max_len,
    );
}

pub(crate) fn foreground_work_packet_required(
    strategy: crate::agent::AgentRunStrategy,
    ingress: crate::bus::IngressKind,
    reply_surface: crate::agent::reply_surface::ReplySurface,
    any_tool_used: bool,
    foreground_work_context_present: bool,
) -> bool {
    strategy == crate::agent::AgentRunStrategy::LinuxEnhanced
        && ingress == crate::bus::IngressKind::User
        && (foreground_work_context_present
            || any_tool_used
            || reply_surface == crate::agent::reply_surface::ReplySurface::TaskExecution)
}

pub(crate) fn extract_foreground_work_packet(
    content: &str,
    required: bool,
) -> Result<(String, Option<ForegroundWorkPacket>)> {
    let Some(open_start) = content.find(ForegroundWorkPacket::OPEN_TAG) else {
        if required {
            return Err(crate::error::Error::config(
                "foreground_work_packet_missing",
                "foreground work packet required but missing",
            ));
        }
        return Ok((content.trim().to_string(), None));
    };
    let Some(close_start) = content[open_start..].find(ForegroundWorkPacket::CLOSE_TAG) else {
        return Err(crate::error::Error::config(
            "foreground_work_packet_invalid",
            "foreground work packet close tag missing",
        ));
    };
    let close_start = open_start + close_start;
    if content[close_start + ForegroundWorkPacket::CLOSE_TAG.len()..]
        .contains(ForegroundWorkPacket::OPEN_TAG)
    {
        return Err(crate::error::Error::config(
            "foreground_work_packet_invalid",
            "multiple foreground work packets are not allowed",
        ));
    }
    let json_start = open_start + ForegroundWorkPacket::OPEN_TAG.len();
    let raw_json = content[json_start..close_start].trim();
    let packet = serde_json::from_str::<ForegroundWorkPacket>(raw_json).map_err(|error| {
        crate::error::Error::config("foreground_work_packet_invalid", error.to_string())
    })?;
    let mut visible = String::with_capacity(content.len().saturating_sub(raw_json.len()));
    visible.push_str(content[..open_start].trim_end());
    let trailing = content[close_start + ForegroundWorkPacket::CLOSE_TAG.len()..].trim_start();
    if !trailing.is_empty() {
        if !visible.trim().is_empty() {
            visible.push_str("\n\n");
        }
        visible.push_str(trailing);
    }
    Ok((visible.trim().to_string(), Some(packet)))
}

pub(crate) fn render_foreground_work_packet_block(
    record: &ActiveWorkRecord,
    max_len: usize,
) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(384));
    out.push_str("## Foreground Work Packet\n");
    let _ = writeln!(out, "Kind: {}", record.kind.label());
    let _ = writeln!(out, "Status: {}", record.status.label());
    if !record.title.trim().is_empty() {
        let _ = writeln!(out, "Title: {}", record.title.trim());
    }
    if !record.progress_summary.trim().is_empty() {
        let _ = writeln!(out, "Progress: {}", record.progress_summary.trim());
    }
    if !record.blocker.trim().is_empty() {
        let _ = writeln!(out, "Blocker: {}", record.blocker.trim());
    }
    if !record.next_action.trim().is_empty() {
        let _ = writeln!(out, "Next: {}", record.next_action.trim());
    }
    if !record.recent_outcome.trim().is_empty() {
        let _ = writeln!(out, "Recent outcome: {}", record.recent_outcome.trim());
    }
    if !record.active_artifact_refs.is_empty() {
        let _ = writeln!(
            out,
            "Artifacts: {}",
            record.active_artifact_refs.join(" | ")
        );
    }
    let trimmed = out.trim_end();
    (!trimmed.is_empty())
        .then(|| crate::util::truncate_content_to_max(trimmed, max_len).into_owned())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveWorkRecord {
    pub kind: ActiveWorkKind,
    #[serde(default)]
    pub title: String,
    pub status: ForegroundWorkStatus,
    #[serde(default)]
    pub continuity_open: bool,
    #[serde(default)]
    pub blocks_background_llm: bool,
    #[serde(default)]
    pub progress_summary: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub recent_outcome: String,
    #[serde(default)]
    pub active_artifact_refs: Vec<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl ActiveWorkRecord {
    pub fn is_meaningful(&self) -> bool {
        !self.title.trim().is_empty()
            || !self.progress_summary.trim().is_empty()
            || !self.blocker.trim().is_empty()
            || !self.next_action.trim().is_empty()
            || !self.recent_outcome.trim().is_empty()
            || !self.active_artifact_refs.is_empty()
    }

    pub(crate) fn blocks_background_llm(&self) -> bool {
        self.continuity_open && self.blocks_background_llm
    }

    pub(crate) fn execution_state_projection(&self) -> ExecutionState {
        ExecutionState {
            status: foreground_status_to_execution_status(self.status),
            goal: self.title.clone(),
            progress: self.progress_summary.clone(),
            blocker: self.blocker.clone(),
            next_action: self.next_action.clone(),
            last_output: self.recent_outcome.clone(),
            updated_at: self.updated_at,
            ..ExecutionState::default()
        }
    }

    pub(crate) fn from_task_run(record: &TaskRunRecord) -> Option<Self> {
        let step = current_or_next_step(record);
        let status = foreground_status_from_task_run(record, step);
        let title = first_non_empty([
            Some(record.run.title.as_str()),
            Some(record.plan.goal.as_str()),
            Some(record.run.user_request.as_str()),
        ]);
        let candidate = Self {
            kind: ActiveWorkKind::TaskExecution,
            title,
            status,
            continuity_open: status.continuity_open(),
            blocks_background_llm: status.default_blocks_background_llm(),
            progress_summary: first_non_empty([
                step.map(|value| value.last_result_summary.as_str()),
                step.and_then(task_step_progress_fallback),
            ]),
            blocker: first_non_empty([
                Some(record.run.failure_reason.as_str()),
                step.and_then(task_step_blocker_fallback),
            ]),
            next_action: if status.continuity_open() {
                first_non_empty([
                    step.map(|value| value.instruction.as_str()),
                    step.map(|value| value.title.as_str()),
                ])
            } else {
                String::new()
            },
            recent_outcome: first_non_empty([
                Some(record.run.final_summary.as_str()),
                step.and_then(task_step_recent_outcome_fallback),
            ]),
            active_artifact_refs: step
                .map(|value| {
                    value
                        .expected_artifacts
                        .iter()
                        .map(|item| item.trim().to_string())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            updated_at: record.run.updated_at,
        };
        candidate.is_meaningful().then_some(candidate)
    }

    #[cfg(test)]
    pub(crate) fn from_interactive_execution_state(
        state: &ExecutionState,
        user_request: &str,
    ) -> Option<Self> {
        let status = foreground_status_from_execution_state(state);
        let candidate = Self {
            kind: ActiveWorkKind::InteractiveAction,
            title: first_non_empty([Some(state.goal.as_str()), Some(user_request)]),
            status,
            continuity_open: status.continuity_open(),
            blocks_background_llm: status.default_blocks_background_llm(),
            progress_summary: first_non_empty([
                Some(state.progress.as_str()),
                Some(state.last_output.as_str()),
            ]),
            blocker: first_non_empty([Some(state.blocker.as_str())]),
            next_action: first_non_empty([Some(state.next_action.as_str())]),
            recent_outcome: first_non_empty([Some(state.last_output.as_str())]),
            active_artifact_refs: Vec::new(),
            updated_at: state.updated_at,
        };
        candidate.is_meaningful().then_some(candidate)
    }
}

pub trait ActiveWorkStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>>;
    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

pub(crate) fn has_meaningful_foreground_work_for_chat(
    active_work_store: &dyn ActiveWorkStore,
    chat_id: &str,
) -> Result<bool> {
    Ok(active_work_store
        .get(chat_id)?
        .is_some_and(|record| record.blocks_background_llm()))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DetachedJobKind {
    LongTermMemoryRefresh,
    PostReplyMaintenance,
    IdleMemoryForge,
    SelfRuntimePostReply,
    SelfRuntimeIdleTick,
    OperatorMaintenance,
}

impl DetachedJobKind {
    pub const fn storage_key_fragment(self) -> &'static str {
        match self {
            Self::LongTermMemoryRefresh => "long_term_memory_refresh",
            Self::PostReplyMaintenance => "post_reply_maintenance",
            Self::IdleMemoryForge => "idle_memory_forge",
            Self::SelfRuntimePostReply => "self_runtime_post_reply",
            Self::SelfRuntimeIdleTick => "self_runtime_idle_tick",
            Self::OperatorMaintenance => "operator_maintenance",
        }
    }

    pub fn needs_llm(self) -> bool {
        matches!(
            self,
            Self::LongTermMemoryRefresh
                | Self::PostReplyMaintenance
                | Self::SelfRuntimePostReply
                | Self::SelfRuntimeIdleTick
        )
    }

    pub fn channel(self) -> &'static str {
        match self {
            Self::LongTermMemoryRefresh => CHANNEL_LONG_TERM_MEMORY_REFRESH,
            Self::PostReplyMaintenance => CHANNEL_POST_REPLY_MAINTENANCE,
            Self::IdleMemoryForge => CHANNEL_IDLE_MEMORY_FORGE,
            Self::SelfRuntimePostReply | Self::SelfRuntimeIdleTick => CHANNEL_SELF_RUNTIME,
            Self::OperatorMaintenance => CHANNEL_OPERATOR_MAINTENANCE,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DetachedWorkKey {
    pub owner_channel: String,
    pub owner_chat_id: String,
    pub kind: DetachedJobKind,
}

impl DetachedWorkKey {
    pub fn new(
        owner_channel: impl Into<String>,
        owner_chat_id: impl Into<String>,
        kind: DetachedJobKind,
    ) -> Self {
        Self {
            owner_channel: owner_channel.into().trim().to_string(),
            owner_chat_id: owner_chat_id.into().trim().to_string(),
            kind,
        }
    }

    pub fn storage_key(&self) -> String {
        format!(
            "{}\u{1f}|{}\u{1f}|{}",
            self.owner_channel,
            self.owner_chat_id,
            self.kind.storage_key_fragment()
        )
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetachedWorkState {
    Pending,
    Queued,
    Running,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachedWorkRecord {
    pub key: DetachedWorkKey,
    pub job: PcMsg,
    pub state: DetachedWorkState,
    pub wake_at_ms: u64,
    pub revision: u64,
    #[serde(default)]
    pub last_reason: String,
    pub updated_at_ms: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveForegroundState {
    pub has_foreground_work: bool,
    pub inbound_depth: u32,
    pub outbound_depth: u32,
    pub active_wss_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundDisposition {
    RunNow,
    Defer(&'static str),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetachedWorkWake {
    pub key: DetachedWorkKey,
    pub revision: u64,
}

pub trait DetachedWorkStore: Send + Sync {
    fn get(&self, key: &DetachedWorkKey) -> Result<Option<DetachedWorkRecord>>;
    fn list(&self) -> Result<Vec<DetachedWorkRecord>>;
    fn upsert(
        &self,
        key: &DetachedWorkKey,
        job: &PcMsg,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<DetachedWorkUpsertOutcome>;
    fn mark_queued(&self, key: &DetachedWorkKey, revision: u64) -> Result<bool>;
    fn claim_running(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
    ) -> Result<Option<DetachedWorkRecord>>;
    fn reschedule(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<Option<DetachedWorkRecord>>;
    fn finish(&self, key: &DetachedWorkKey, revision: u64) -> Result<()>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachedWorkUpsertOutcome {
    pub changed: bool,
    pub record: DetachedWorkRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ActiveWorkSyncInput<'a> {
    pub(crate) chat_id: &'a str,
    pub(crate) foreground_work_packet: Option<&'a ForegroundWorkPacket>,
    pub(crate) now_secs: u64,
}

pub(crate) fn load_active_work_for_chat(
    store: &dyn ActiveWorkStore,
    active_task_run: Option<&TaskRunRecord>,
    chat_id: &str,
) -> Result<Option<ActiveWorkRecord>> {
    if let Some(record) = store.get(chat_id)? {
        if record.is_meaningful() {
            return Ok(Some(record));
        }
    }
    Ok(active_task_run.and_then(ActiveWorkRecord::from_task_run))
}

pub(crate) fn sync_active_work_after_turn(
    store: &dyn ActiveWorkStore,
    input: ActiveWorkSyncInput<'_>,
) -> Result<()> {
    let next = input
        .foreground_work_packet
        .and_then(|packet| packet.settlement.clone())
        .and_then(|settlement| settlement.into_record(input.now_secs));
    if let Some(record) = next {
        store.set(input.chat_id, &record)
    } else {
        store.clear(input.chat_id)
    }
}

pub fn live_foreground_state_for_chat(
    active_work_store: &dyn ActiveWorkStore,
    chat_id: &str,
) -> Result<LiveForegroundState> {
    let has_foreground_work = has_meaningful_foreground_work_for_chat(active_work_store, chat_id)?;
    let snap = orchestrator_snapshot();
    Ok(LiveForegroundState {
        has_foreground_work,
        inbound_depth: snap.inbound_depth,
        outbound_depth: snap.outbound_depth,
        active_wss_count: snap.active_wss_count,
    })
}

pub fn idle_self_runtime_scheduler_block_reason_with_live_state(
    live: LiveForegroundState,
) -> Option<&'static str> {
    if live.has_foreground_work {
        Some("foreground_work_active")
    } else if cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && live.active_wss_count > 0
    {
        Some("external_wss_active")
    } else if live.inbound_depth > 0 || live.outbound_depth > 0 {
        Some("message_queues_busy")
    } else {
        None
    }
}

pub fn classify_background_job_disposition(
    kind: DetachedJobKind,
    live: LiveForegroundState,
) -> BackgroundDisposition {
    let reason = match kind {
        DetachedJobKind::SelfRuntimeIdleTick => {
            idle_self_runtime_scheduler_block_reason_with_live_state(live)
        }
        _ if live.has_foreground_work => Some("foreground_work_active"),
        _ if live.inbound_depth > 0 || live.outbound_depth > 0 => Some("message_queues_busy"),
        _ => None,
    };
    match reason {
        Some(reason) => BackgroundDisposition::Defer(reason),
        None => BackgroundDisposition::RunNow,
    }
}

pub fn upsert_detached_work_job(
    store: &dyn DetachedWorkStore,
    key: DetachedWorkKey,
    job: &PcMsg,
    delay_ms: u64,
    reason: &str,
) -> Result<DetachedWorkUpsertOutcome> {
    let wake_at_ms = current_unix_ms().saturating_add(delay_ms);
    store.upsert(&key, job, wake_at_ms, reason)
}

pub fn due_detached_work_records(
    store: &dyn DetachedWorkStore,
    now_ms: u64,
    limit: usize,
) -> Result<Vec<DetachedWorkRecord>> {
    let mut records = store
        .list()?
        .into_iter()
        .filter(|record| record.state == DetachedWorkState::Pending && record.wake_at_ms <= now_ms)
        .collect::<Vec<_>>();
    records.sort_by_key(|record| (record.wake_at_ms, record.updated_at_ms));
    if records.len() > limit {
        records.truncate(limit);
    }
    Ok(records)
}

pub fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn foreground_status_from_task_run(
    record: &TaskRunRecord,
    step: Option<&TaskStep>,
) -> ForegroundWorkStatus {
    match record.run.status {
        TaskRunStatus::Planning | TaskRunStatus::Running => ForegroundWorkStatus::Running,
        TaskRunStatus::Blocked => {
            if !record.run.failure_reason.trim().is_empty()
                || step.is_some_and(|value| {
                    matches!(
                        value.status,
                        TaskStepStatus::Blocked | TaskStepStatus::Failed
                    ) && !value.last_review_summary.trim().is_empty()
                })
            {
                ForegroundWorkStatus::AwaitingUser
            } else {
                ForegroundWorkStatus::Suspended
            }
        }
        TaskRunStatus::Failed => ForegroundWorkStatus::FailedTerminal,
        TaskRunStatus::Aborted => ForegroundWorkStatus::Aborted,
        TaskRunStatus::Completed | TaskRunStatus::PartialComplete => {
            ForegroundWorkStatus::Completed
        }
    }
}

#[cfg(test)]
fn foreground_status_from_execution_state(state: &ExecutionState) -> ForegroundWorkStatus {
    match state.status {
        ExecutionStatus::Active => ForegroundWorkStatus::Running,
        ExecutionStatus::Blocked => {
            if !state.blocker.trim().is_empty() || !state.next_action.trim().is_empty() {
                ForegroundWorkStatus::AwaitingUser
            } else {
                ForegroundWorkStatus::Suspended
            }
        }
        ExecutionStatus::Done => ForegroundWorkStatus::Completed,
    }
}

fn foreground_status_to_execution_status(status: ForegroundWorkStatus) -> ExecutionStatus {
    match status {
        ForegroundWorkStatus::Running | ForegroundWorkStatus::Suspended => ExecutionStatus::Active,
        ForegroundWorkStatus::AwaitingUser => ExecutionStatus::Blocked,
        ForegroundWorkStatus::Completed
        | ForegroundWorkStatus::Aborted
        | ForegroundWorkStatus::FailedTerminal => ExecutionStatus::Done,
    }
}

fn first_non_empty<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> String {
    parts
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

fn task_step_progress_fallback(step: &TaskStep) -> Option<&str> {
    if !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else if !step.title.trim().is_empty() {
        Some(step.title.as_str())
    } else {
        None
    }
}

fn task_step_blocker_fallback(step: &TaskStep) -> Option<&str> {
    if matches!(
        step.status,
        TaskStepStatus::Blocked | TaskStepStatus::Failed
    ) {
        if !step.last_review_summary.trim().is_empty() {
            Some(step.last_review_summary.as_str())
        } else if !step.title.trim().is_empty() {
            Some(step.title.as_str())
        } else {
            None
        }
    } else {
        None
    }
}

fn task_step_recent_outcome_fallback(step: &TaskStep) -> Option<&str> {
    if !step.last_result_summary.trim().is_empty() {
        Some(step.last_result_summary.as_str())
    } else if step.status.is_terminal() && !step.last_review_summary.trim().is_empty() {
        Some(step.last_review_summary.as_str())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_execution::{
        TaskPlan, TaskRun, TaskRunKind, TaskRunStatus, TaskStep, TaskStepStatus,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryActiveWorkStore {
        inner: Mutex<HashMap<String, ActiveWorkRecord>>,
    }

    impl ActiveWorkStore for MemoryActiveWorkStore {
        fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
            Ok(self
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), record.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryDetachedWorkStore {
        inner: Mutex<HashMap<String, DetachedWorkRecord>>,
    }

    impl DetachedWorkStore for MemoryDetachedWorkStore {
        fn get(&self, key: &DetachedWorkKey) -> Result<Option<DetachedWorkRecord>> {
            Ok(self
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key.storage_key())
                .cloned())
        }

        fn list(&self) -> Result<Vec<DetachedWorkRecord>> {
            Ok(self
                .inner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn upsert(
            &self,
            key: &DetachedWorkKey,
            job: &PcMsg,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<DetachedWorkUpsertOutcome> {
            let now_ms = 100;
            let storage_key = key.storage_key();
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let next = match inner.get(&storage_key) {
                Some(current)
                    if current.job == *job
                        && current.wake_at_ms == wake_at_ms
                        && current.last_reason == reason
                        && current.state == DetachedWorkState::Pending =>
                {
                    return Ok(DetachedWorkUpsertOutcome {
                        changed: false,
                        record: current.clone(),
                    });
                }
                Some(current) => DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: current.revision.saturating_add(1),
                    last_reason: reason.to_string(),
                    updated_at_ms: now_ms,
                },
                None => DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: 1,
                    last_reason: reason.to_string(),
                    updated_at_ms: now_ms,
                },
            };
            inner.insert(storage_key, next.clone());
            Ok(DetachedWorkUpsertOutcome {
                changed: true,
                record: next,
            })
        }

        fn mark_queued(&self, key: &DetachedWorkKey, revision: u64) -> Result<bool> {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = inner.get_mut(&key.storage_key()) else {
                return Ok(false);
            };
            if record.revision != revision || record.state != DetachedWorkState::Pending {
                return Ok(false);
            }
            record.state = DetachedWorkState::Queued;
            Ok(true)
        }

        fn claim_running(
            &self,
            key: &DetachedWorkKey,
            revision: u64,
        ) -> Result<Option<DetachedWorkRecord>> {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = inner.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision
                || !matches!(
                    record.state,
                    DetachedWorkState::Pending | DetachedWorkState::Queued
                )
            {
                return Ok(None);
            }
            record.state = DetachedWorkState::Running;
            Ok(Some(record.clone()))
        }

        fn reschedule(
            &self,
            key: &DetachedWorkKey,
            revision: u64,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<Option<DetachedWorkRecord>> {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = inner.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision {
                return Ok(None);
            }
            record.revision = record.revision.saturating_add(1);
            record.state = DetachedWorkState::Pending;
            record.wake_at_ms = wake_at_ms;
            record.last_reason = reason.to_string();
            Ok(Some(record.clone()))
        }

        fn finish(&self, key: &DetachedWorkKey, revision: u64) -> Result<()> {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner
                .get(&key.storage_key())
                .is_some_and(|record| record.revision == revision)
            {
                inner.remove(&key.storage_key());
            }
            Ok(())
        }
    }

    fn sample_task_run(status: TaskRunStatus) -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: "run-1".to_string(),
                kind: TaskRunKind::TaskExecution,
                source_channel: "qq_channel".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: "配置 QQ 邮箱".to_string(),
                title: "QQ 邮箱配置".to_string(),
                status,
                current_step_id: "s01".to_string(),
                planner_reason: String::new(),
                final_summary: "账户草案已创建".to_string(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 1,
                updated_at: 9,
                finished_at: 0,
            },
            plan: TaskPlan {
                goal: "配置 QQ 邮箱账户".to_string(),
                completion_definition: "账户已保存并通过校验".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![TaskStep {
                    step_id: "s01".to_string(),
                    title: "补认证信息".to_string(),
                    instruction: "写入 provider_kind 并补认证凭据".to_string(),
                    status: TaskStepStatus::Running,
                    tool_budget: 1,
                    retry_budget: 1,
                    expected_artifacts: Vec::new(),
                    review_criteria: Vec::new(),
                    attempt_count: 0,
                    last_result_summary: "账户草案已创建".to_string(),
                    last_review_summary: String::new(),
                    started_at: 0,
                    finished_at: 0,
                }],
            },
        }
    }

    #[test]
    fn loads_active_work_from_store_before_task_run_fallback() {
        let store = MemoryActiveWorkStore::default();
        let stored = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "查看系统状态".to_string(),
            status: ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: String::new(),
            blocker: String::new(),
            next_action: "执行 office_status".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 5,
        };
        store.set("chat-1", &stored).expect("store");

        let loaded = load_active_work_for_chat(
            &store,
            Some(&sample_task_run(TaskRunStatus::Running)),
            "chat-1",
        )
        .expect("load")
        .expect("active work");

        assert_eq!(loaded.kind, ActiveWorkKind::InteractiveAction);
        assert_eq!(loaded.title, "查看系统状态");
    }

    #[test]
    fn loads_active_work_from_task_run_when_store_missing() {
        let store = MemoryActiveWorkStore::default();
        let loaded = load_active_work_for_chat(
            &store,
            Some(&sample_task_run(TaskRunStatus::Running)),
            "chat-1",
        )
        .expect("load")
        .expect("active work");

        assert_eq!(loaded.kind, ActiveWorkKind::TaskExecution);
        assert_eq!(loaded.title, "QQ 邮箱配置");
        assert_eq!(loaded.status, ForegroundWorkStatus::Running);
    }

    #[test]
    fn sync_clears_active_work_when_packet_settlement_is_absent() {
        let store = MemoryActiveWorkStore::default();
        let record = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            status: ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: String::new(),
            blocker: String::new(),
            next_action: "补认证信息".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 1,
        };
        store.set("chat-1", &record).expect("store");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                foreground_work_packet: Some(&ForegroundWorkPacket {
                    relation_to_last_work: ForegroundWorkRelation::CancelExisting,
                    settlement: None,
                }),
                now_secs: 8,
            },
        )
        .expect("sync");

        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn sync_keeps_explicit_foreground_work_packet() {
        let store = MemoryActiveWorkStore::default();
        let interactive_work = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "配置 QQ 邮箱账户".to_string(),
            status: ForegroundWorkStatus::AwaitingUser,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: "缺少 provider_kind".to_string(),
            next_action: "补认证信息".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                foreground_work_packet: Some(&ForegroundWorkPacket {
                    relation_to_last_work: ForegroundWorkRelation::ContinueExisting,
                    settlement: Some(ForegroundWorkSettlement::from(&interactive_work)),
                }),
                now_secs: 7,
            },
        )
        .expect("sync");
        let record = store.get("chat-1").expect("get").expect("record");
        assert_eq!(record.kind, ActiveWorkKind::InteractiveAction);
        assert_eq!(record.blocker, "缺少 provider_kind");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                foreground_work_packet: Some(&ForegroundWorkPacket {
                    relation_to_last_work: ForegroundWorkRelation::Independent,
                    settlement: None,
                }),
                now_secs: 8,
            },
        )
        .expect("sync");
        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn sync_does_not_materialize_foreground_work_without_explicit_packet() {
        let store = MemoryActiveWorkStore::default();

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                foreground_work_packet: None,
                now_secs: 9,
            },
        )
        .expect("sync");

        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn completed_work_remains_meaningful_for_followup_replay() {
        let store = MemoryActiveWorkStore::default();
        let stored = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            status: ForegroundWorkStatus::Completed,
            continuity_open: false,
            blocks_background_llm: false,
            progress_summary: "账户和凭证都已保存".to_string(),
            blocker: String::new(),
            next_action: String::new(),
            recent_outcome: "邮件服务已激活".to_string(),
            active_artifact_refs: Vec::new(),
            updated_at: 12,
        };
        store.set("chat-1", &stored).expect("store");

        let loaded = load_active_work_for_chat(&store, None, "chat-1")
            .expect("load")
            .expect("active work");
        assert_eq!(loaded.status, ForegroundWorkStatus::Completed);
        assert_eq!(loaded.recent_outcome, "邮件服务已激活");
    }

    #[test]
    fn personality_governance_job_is_deferred_not_dropped_when_user_turn_is_waiting() {
        let disposition = classify_background_job_disposition(
            DetachedJobKind::SelfRuntimeIdleTick,
            LiveForegroundState {
                has_foreground_work: true,
                inbound_depth: 1,
                outbound_depth: 0,
                active_wss_count: 0,
            },
        );
        assert_eq!(
            disposition,
            BackgroundDisposition::Defer("foreground_work_active")
        );
    }

    #[test]
    fn duplicate_self_runtime_idle_tick_collapses_into_one_detached_work_item() {
        let store = MemoryDetachedWorkStore::default();
        let key =
            DetachedWorkKey::new("qq_channel", "chat-1", DetachedJobKind::SelfRuntimeIdleTick);
        let job = PcMsg::new_system(CHANNEL_SELF_RUNTIME, "chat-1", "{}").expect("job");

        let first = store
            .upsert(&key, &job, 100, "idle_tick")
            .expect("first upsert");
        let second = store
            .upsert(&key, &job, 100, "idle_tick")
            .expect("second upsert");

        assert!(first.changed);
        assert!(!second.changed);
        assert_eq!(store.list().expect("list").len(), 1);
    }
}
