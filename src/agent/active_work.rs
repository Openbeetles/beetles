//! Formal foreground active-work contract for the current chat.
//! 对当前会话前台主工作面的正式合同。

use crate::bus::PcMsg;
use crate::error::Result;
use crate::memory::{
    execution_state_has_pending_work, render_execution_state_block,
    should_resume_active_execution_state, ExecutionState, ExecutionStateStore, ExecutionStatus,
};
use crate::orchestrator::snapshot as orchestrator_snapshot;
use crate::runtime::system_work::{
    CHANNEL_IDLE_MEMORY_FORGE, CHANNEL_LONG_TERM_MEMORY_REFRESH, CHANNEL_OPERATOR_MAINTENANCE,
    CHANNEL_POST_REPLY_MAINTENANCE, CHANNEL_SELF_RUNTIME,
};
use crate::task_execution::{current_or_next_step, TaskRunKind, TaskRunRecord, TaskRunStatus};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::collections::HashMap;

pub const REL_PATH_ACTIVE_WORKS: &str = "memory/active_works.json";
pub const REL_PATH_DETACHED_WORKS: &str = "memory/detached_works.json";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActiveWorkKind {
    InteractiveAction,
    TaskExecution,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveWorkRecord {
    pub kind: ActiveWorkKind,
    #[serde(default)]
    pub title: String,
    pub state: ExecutionState,
}

impl ActiveWorkRecord {
    pub fn is_meaningful(&self) -> bool {
        self.state.status != ExecutionStatus::Done && self.state.is_meaningful()
    }

    pub(crate) fn should_resume(&self, user_content: &str) -> bool {
        self.state.status != ExecutionStatus::Done
            && should_resume_active_execution_state(&self.state, user_content)
    }

    pub(crate) fn from_task_run(record: &TaskRunRecord) -> Option<Self> {
        let mut state = ExecutionState {
            status: task_run_status_to_execution_status(record.run.status),
            goal: record.plan.goal.clone(),
            updated_at: record.run.updated_at,
            ..ExecutionState::default()
        };
        if let Some(step) = current_or_next_step(record) {
            if !step.last_result_summary.trim().is_empty() {
                state.progress = step.last_result_summary.clone();
            } else if !step.title.trim().is_empty() {
                state.progress = format!("Current step: {}", step.title);
            }
            if !step.instruction.trim().is_empty() {
                state.next_action = step.instruction.clone();
            } else if !step.title.trim().is_empty() {
                state.next_action = step.title.clone();
            }
            if !step.last_review_summary.trim().is_empty() {
                state
                    .latest_observations
                    .push(step.last_review_summary.clone());
            }
            if !step.title.trim().is_empty() {
                state.next_best_actions.push(step.title.clone());
            }
        }
        if !record.run.final_summary.trim().is_empty() {
            state.last_output = record.run.final_summary.clone();
        }
        if !record.run.failure_reason.trim().is_empty() {
            state.blocker = record.run.failure_reason.clone();
        }
        if state.goal.trim().is_empty() {
            state.goal = record.run.user_request.clone();
        }
        let title = if record.run.title.trim().is_empty() {
            state.goal.clone()
        } else {
            record.run.title.clone()
        };
        let candidate = Self {
            kind: match record.run.kind {
                TaskRunKind::InteractiveAction => ActiveWorkKind::InteractiveAction,
                TaskRunKind::TaskExecution => ActiveWorkKind::TaskExecution,
            },
            title,
            state,
        };
        candidate.is_meaningful().then_some(candidate)
    }

    pub(crate) fn from_execution_state(state: &ExecutionState) -> Option<Self> {
        if state.status == ExecutionStatus::Done || !execution_state_has_pending_work(state) {
            return None;
        }
        let title = if !state.goal.trim().is_empty() {
            state.goal.clone()
        } else if !state.next_action.trim().is_empty() {
            state.next_action.clone()
        } else if !state.blocker.trim().is_empty() {
            state.blocker.clone()
        } else {
            return None;
        };
        Some(Self {
            kind: ActiveWorkKind::InteractiveAction,
            title,
            state: state.clone(),
        })
    }
}

pub trait ActiveWorkStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<ActiveWorkRecord>>;
    fn set(&self, chat_id: &str, record: &ActiveWorkRecord) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

pub(crate) fn has_meaningful_foreground_work_for_chat(
    active_work_store: &dyn ActiveWorkStore,
    execution_state_store: &dyn ExecutionStateStore,
    chat_id: &str,
) -> Result<bool> {
    if active_work_store
        .get(chat_id)?
        .is_some_and(|record| record.is_meaningful())
    {
        return Ok(true);
    }
    Ok(execution_state_store
        .get(chat_id)?
        .is_some_and(|state| execution_state_has_pending_work(&state)))
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
    pub(crate) request_semantics: crate::agent::request_semantics::RequestSemantics,
    pub(crate) reply_surface: crate::agent::reply_surface::ReplySurface,
    pub(crate) active_task_run: Option<&'a TaskRunRecord>,
    pub(crate) execution_state: Option<&'a ExecutionState>,
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

pub(crate) fn render_active_work_block(
    record: &ActiveWorkRecord,
    max_len: usize,
) -> Option<String> {
    let kind = match record.kind {
        ActiveWorkKind::InteractiveAction => "interactive_action",
        ActiveWorkKind::TaskExecution => "task_execution",
    };
    let mut out = String::from("## Active Work\n");
    out.push_str(&format!("Kind: {}\n", kind));
    if !record.title.trim().is_empty() {
        out.push_str(&format!("Title: {}\n", record.title.trim()));
    }
    if let Some(state_block) = render_execution_state_block(&record.state, max_len) {
        out.push_str(&state_block);
    }
    let rendered = truncate_content_to_max(out.trim(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn sync_active_work_after_turn(
    store: &dyn ActiveWorkStore,
    input: ActiveWorkSyncInput<'_>,
) -> Result<()> {
    use crate::agent::reply_surface::ReplySurface;
    use crate::agent::request_semantics::ResumeRelation;

    if matches!(
        input.request_semantics.resume_relation,
        ResumeRelation::DenyOrCancelActiveAction | ResumeRelation::SwitchToNewRequest
    ) {
        return store.clear(input.chat_id);
    }
    let next = if input.reply_surface == ReplySurface::TaskExecution {
        input
            .active_task_run
            .and_then(ActiveWorkRecord::from_task_run)
    } else {
        let keep_interactive_work = should_keep_interactive_action_work(input.request_semantics);
        input
            .active_task_run
            .filter(|record| {
                keep_interactive_work && record.run.kind == TaskRunKind::InteractiveAction
            })
            .and_then(ActiveWorkRecord::from_task_run)
            .or_else(|| {
                keep_interactive_work
                    .then(|| {
                        input
                            .execution_state
                            .and_then(ActiveWorkRecord::from_execution_state)
                    })
                    .flatten()
            })
    };
    if let Some(record) = next {
        store.set(input.chat_id, &record)
    } else {
        store.clear(input.chat_id)
    }
}

pub(crate) fn should_keep_interactive_action_work(
    semantics: crate::agent::request_semantics::RequestSemantics,
) -> bool {
    use crate::agent::request_semantics::{ActionFamily, ResumeRelation};

    matches!(
        semantics.action_family,
        ActionFamily::ActionRequest | ActionFamily::ActiveAction
    ) || matches!(
        semantics.resume_relation,
        ResumeRelation::ConfirmActiveAction
            | ResumeRelation::SupplyActiveActionInput
            | ResumeRelation::ResumeActiveAction
    )
}

pub fn live_foreground_state_for_chat(
    active_work_store: &dyn ActiveWorkStore,
    execution_state_store: &dyn ExecutionStateStore,
    chat_id: &str,
) -> Result<LiveForegroundState> {
    let has_foreground_work =
        has_meaningful_foreground_work_for_chat(active_work_store, execution_state_store, chat_id)?;
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

fn task_run_status_to_execution_status(status: TaskRunStatus) -> ExecutionStatus {
    match status {
        TaskRunStatus::Planning | TaskRunStatus::Running => ExecutionStatus::Active,
        TaskRunStatus::Blocked | TaskRunStatus::Failed => ExecutionStatus::Blocked,
        TaskRunStatus::Completed | TaskRunStatus::PartialComplete | TaskRunStatus::Aborted => {
            ExecutionStatus::Done
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reply_surface::ReplySurface;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics, ResumeRelation,
    };
    use crate::task_execution::{
        build_interactive_action_run_record, TaskPlan, TaskRun, TaskRunKind, TaskRunStatus,
        TaskStep, TaskStepStatus,
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

    fn semantics(action_family: ActionFamily, resume_relation: ResumeRelation) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::HostTool,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference: ExecutionPreference::ToolFirst,
            action_family,
            resume_relation,
            confidence: 100,
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
            state: ExecutionState {
                goal: "查看当前系统状态".to_string(),
                next_action: "执行 office_status".to_string(),
                updated_at: 5,
                ..ExecutionState::default()
            },
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
        assert!(loaded.should_resume("继续"));
    }

    #[test]
    fn sync_clears_active_work_on_switch_request() {
        let store = MemoryActiveWorkStore::default();
        let record = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            state: ExecutionState {
                goal: "配置 QQ 邮箱账户".to_string(),
                next_action: "补认证信息".to_string(),
                updated_at: 1,
                ..ExecutionState::default()
            },
        };
        store.set("chat-1", &record).expect("store");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::ActionRequest,
                    ResumeRelation::SwitchToNewRequest,
                ),
                reply_surface: ReplySurface::GovernedConversation,
                active_task_run: None,
                execution_state: None,
            },
        )
        .expect("sync");

        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn sync_promotes_action_execution_state_but_not_plain_conversation() {
        let store = MemoryActiveWorkStore::default();
        let state = ExecutionState {
            status: ExecutionStatus::Blocked,
            goal: "配置 QQ 邮箱账户".to_string(),
            progress: "账户草案已创建".to_string(),
            blocker: "缺少 provider_kind".to_string(),
            next_action: "补认证信息".to_string(),
            updated_at: 7,
            ..ExecutionState::default()
        };
        let interactive_run = build_interactive_action_run_record(
            "run-2",
            "qq_channel",
            "chat-1",
            "配置 QQ 邮箱",
            &state,
            7,
        )
        .expect("interactive run");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::ActionRequest,
                    ResumeRelation::IndependentTurn,
                ),
                reply_surface: ReplySurface::GovernedConversation,
                active_task_run: Some(&interactive_run),
                execution_state: Some(&state),
            },
        )
        .expect("sync");
        let record = store.get("chat-1").expect("get").expect("record");
        assert_eq!(record.kind, ActiveWorkKind::InteractiveAction);
        assert_eq!(record.state.blocker, "缺少 provider_kind");

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::Conversation,
                    ResumeRelation::IndependentTurn,
                ),
                reply_surface: ReplySurface::PublicRuntime,
                active_task_run: None,
                execution_state: Some(&state),
            },
        )
        .expect("sync");
        assert!(store.get("chat-1").expect("get").is_none());
    }

    #[test]
    fn sync_falls_back_to_execution_state_when_interactive_run_is_not_materialized() {
        let store = MemoryActiveWorkStore::default();
        let state = ExecutionState {
            status: ExecutionStatus::Blocked,
            goal: "配置 QQ 邮箱账户".to_string(),
            progress: "账户草案已创建".to_string(),
            blocker: "缺少 SMTP 授权码".to_string(),
            next_action: "等待用户补充 SMTP 授权码".to_string(),
            updated_at: 9,
            ..ExecutionState::default()
        };

        sync_active_work_after_turn(
            &store,
            ActiveWorkSyncInput {
                chat_id: "chat-1",
                request_semantics: semantics(
                    ActionFamily::ActionRequest,
                    ResumeRelation::IndependentTurn,
                ),
                reply_surface: ReplySurface::GovernedConversation,
                active_task_run: None,
                execution_state: Some(&state),
            },
        )
        .expect("sync");

        let record = store.get("chat-1").expect("get").expect("record");
        assert_eq!(record.kind, ActiveWorkKind::InteractiveAction);
        assert_eq!(record.title, "配置 QQ 邮箱账户");
        assert_eq!(record.state.blocker, "缺少 SMTP 授权码");
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
