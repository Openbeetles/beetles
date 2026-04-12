use crate::orchestrator::PressureLevel;
use crate::runtime::{PresenceState, RuntimeModeSnapshot};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowKind {
    InitiativeTick,
    UpcomingReminderNudge,
    ResumeTaskCheckIn,
    PostReplyMaintenance,
    SelfRuntimePostReply,
    SelfRuntimeIdleTick,
    RebootRecovery,
    OperatorMaintenance,
}

impl WorkflowKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InitiativeTick => "initiative_tick",
            Self::UpcomingReminderNudge => "upcoming_reminder_nudge",
            Self::ResumeTaskCheckIn => "resume_task_check_in",
            Self::PostReplyMaintenance => "post_reply_maintenance",
            Self::SelfRuntimePostReply => "self_runtime_post_reply",
            Self::SelfRuntimeIdleTick => "self_runtime_idle_tick",
            Self::RebootRecovery => "reboot_recovery",
            Self::OperatorMaintenance => "operator_maintenance",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTrigger {
    CronTick,
    DelayedDue,
    PostReply,
    ModeTransition,
    BootRecovery,
    OperatorRequested,
    StateDelta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDisposition {
    ExecuteNow,
    DeferUntil,
    Suppress,
    Cancel,
    NoTrigger,
    ExecuteFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEffect {
    Noop,
    EnqueueSystemJob,
    SendOutboundNudge,
    RunRepairPass,
    PersistRecoveryIntent,
    ReplayRecovery,
    RollbackRelease,
    RequestRestart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRecoveryPolicy {
    DropOnModeExit,
    RetryAfterModeResume,
    ReplayAfterBoot,
    OperatorAckRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkflowAdmissionSnapshot {
    pub runtime_mode: RuntimeModeSnapshot,
    pub presence_state: PresenceState,
    pub pressure: PressureLevel,
    pub active_agent_tasks: u16,
    pub inbound_depth: u16,
    pub outbound_depth: u16,
    pub allow_idle_self_runtime: bool,
    pub allow_non_voice_outbound: bool,
    pub recovery_safe_mode_active: bool,
    pub voice_exclusive_active: bool,
}

impl WorkflowAdmissionSnapshot {
    pub fn from_runtime(
        runtime_mode: RuntimeModeSnapshot,
        presence_state: PresenceState,
        pressure: PressureLevel,
        active_agent_tasks: usize,
        inbound_depth: usize,
        outbound_depth: usize,
    ) -> Self {
        Self {
            allow_idle_self_runtime: runtime_mode.action_budget.allow_idle_self_runtime,
            allow_non_voice_outbound: runtime_mode.action_budget.allow_non_voice_outbound,
            recovery_safe_mode_active: runtime_mode.recovery_safe_mode_active,
            voice_exclusive_active: runtime_mode.voice_exclusive_active,
            runtime_mode,
            presence_state,
            pressure,
            active_agent_tasks: active_agent_tasks.min(u16::MAX as usize) as u16,
            inbound_depth: inbound_depth.min(u16::MAX as usize) as u16,
            outbound_depth: outbound_depth.min(u16::MAX as usize) as u16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkflowAuditRecord {
    pub workflow: WorkflowKind,
    pub trigger: WorkflowTrigger,
    pub disposition: WorkflowDisposition,
    pub effect: WorkflowEffect,
    pub recovery_policy: WorkflowRecoveryPolicy,
    pub rationale: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppression_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_allowed_at: Option<u64>,
    pub happened_at: u64,
}

impl WorkflowAuditRecord {
    pub fn new(
        workflow: WorkflowKind,
        trigger: WorkflowTrigger,
        disposition: WorkflowDisposition,
        effect: WorkflowEffect,
        recovery_policy: WorkflowRecoveryPolicy,
        rationale: impl Into<String>,
        happened_at: u64,
    ) -> Self {
        Self {
            workflow,
            trigger,
            disposition,
            effect,
            recovery_policy,
            rationale: rationale.into(),
            scope_id: None,
            channel: None,
            chat_id: None,
            suppression_reason: None,
            next_allowed_at: None,
            happened_at,
        }
    }

    pub fn with_target(
        mut self,
        scope_id: Option<&str>,
        channel: Option<&str>,
        chat_id: Option<&str>,
    ) -> Self {
        self.scope_id = scope_id.map(ToOwned::to_owned);
        self.channel = channel.map(ToOwned::to_owned);
        self.chat_id = chat_id.map(ToOwned::to_owned);
        self
    }

    pub fn with_suppression_reason(mut self, reason: Option<&str>) -> Self {
        self.suppression_reason = reason.map(ToOwned::to_owned);
        self
    }

    pub fn with_next_allowed_at(mut self, next_allowed_at: Option<u64>) -> Self {
        self.next_allowed_at = next_allowed_at;
        self
    }
}
