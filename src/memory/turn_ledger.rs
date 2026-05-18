//! 最近一轮执行账本：记录对话最近一次请求的执行与交付摘要。
//! Latest turn ledger: execution and delivery summary for the most recent request.

use crate::bus::{IngressKind, MessageBodyKind, MessageTransport, PcMsg};
use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Mutex;

use super::{
    derive_recent_persona_evidence, MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction,
    PersonaPriorityAdjudication, RecentPersonaEvidence, RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK,
    RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
};

pub const REL_PATH_TURN_LEDGERS: &str = "memory/turn_ledgers";
pub const REL_PATH_TURN_LEDGER_HISTORY: &str = "memory/turn_ledger_history";
pub const TURN_LEDGER_HISTORY_MAX_ITEMS: usize = 32;
const TURN_LEDGER_PREVIEW_MAX_CHARS: usize = 240;
const TURN_LEDGER_REASON_MAX_CHARS: usize = 96;
const TURN_PERSONA_TEXT_MAX_CHARS: usize = 160;
const TURN_PERSONA_SCOPE_MAX_CHARS: usize = 24;
const TURN_SUBJECT_STATE_TEXT_MAX_CHARS: usize = 72;
const TURN_SUBJECT_STATE_SUMMARY_MAX_CHARS: usize = 160;
const TURN_OBSERVATION_TEXT_MAX_CHARS: usize = 96;
const TURN_REASONING_SIGNAL_MAX_CHARS: usize = 32;
const TURN_REASONING_SUMMARY_MAX_CHARS: usize = 160;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnExecutionClass {
    #[default]
    DirectReply,
    ToolAssisted,
    TaskExecution,
    Interrupted,
}

impl TurnExecutionClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirectReply => "direct_reply",
            Self::ToolAssisted => "tool_assisted",
            Self::TaskExecution => "task_execution",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnDeliberationClass {
    FastInteractive,
    #[default]
    Standard,
    HardReasoning,
}

impl TurnDeliberationClass {
    pub fn label(self) -> &'static str {
        match self {
            Self::FastInteractive => "fast_interactive",
            Self::Standard => "standard",
            Self::HardReasoning => "hard_reasoning",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnPersonaPressureLevel {
    #[default]
    Normal,
    Cautious,
    Critical,
}

impl TurnPersonaPressureLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Cautious => "cautious",
            Self::Critical => "critical",
        }
    }
}

impl From<crate::orchestrator::PressureLevel> for TurnPersonaPressureLevel {
    fn from(value: crate::orchestrator::PressureLevel) -> Self {
        match value {
            crate::orchestrator::PressureLevel::Normal => Self::Normal,
            crate::orchestrator::PressureLevel::Cautious => Self::Cautious,
            crate::orchestrator::PressureLevel::Critical => Self::Critical,
        }
    }
}

fn turn_persona_share_action_label(action: MentalPrivacyShareAction) -> &'static str {
    match action {
        MentalPrivacyShareAction::AllowOriginal => "allow_original",
        MentalPrivacyShareAction::AllowRaw => "allow_raw",
        MentalPrivacyShareAction::AllowSummary => "allow_summary",
        MentalPrivacyShareAction::AllowRedactedExcerpt => "allow_redacted_excerpt",
        MentalPrivacyShareAction::ExplainWithoutQuote => "explain_without_quote",
        MentalPrivacyShareAction::Refuse => "refuse",
        MentalPrivacyShareAction::Defer => "defer",
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnPersonaDisclosureLedger {
    #[serde(default)]
    pub request_kind: String,
    #[serde(default)]
    pub share_action: MentalPrivacyShareAction,
    #[serde(default)]
    pub acknowledge_boundary: bool,
    #[serde(default)]
    pub targets: Vec<String>,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub response_guidance: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnPersonaPriorityLedger {
    #[serde(default)]
    pub stance_summary: String,
    #[serde(default)]
    pub priority_order: Vec<String>,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub task_scope: String,
    #[serde(default)]
    pub initiative_posture: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub resource_posture: String,
    #[serde(default)]
    pub response_guidance: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnPersonaReviewLedger {
    #[serde(default)]
    pub action: MentalPrivacyShareAction,
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub rewrite_applied: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnToolPathLedger {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub react_rounds: u32,
    #[serde(default)]
    pub current_primary_delivered: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnBlockerLedger {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub failed_calls: u32,
    #[serde(default)]
    pub total_calls: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnModeSnapshotLedger {
    #[serde(default)]
    pub current_mode: String,
    #[serde(default)]
    pub allow_non_voice_outbound: bool,
    #[serde(default)]
    pub allow_idle_self_runtime: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnObservationLedger {
    #[serde(default)]
    pub execution_class: TurnExecutionClass,
    #[serde(default)]
    pub deliberation_class: TurnDeliberationClass,
    #[serde(default)]
    pub final_outcome: String,
    #[serde(default)]
    pub pressure: TurnPersonaPressureLevel,
    #[serde(default)]
    pub mode: TurnModeSnapshotLedger,
    #[serde(default)]
    pub tool_path: TurnToolPathLedger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<TurnBlockerLedger>,
}

impl TurnObservationLedger {
    pub fn is_meaningful(&self) -> bool {
        self.execution_class != TurnExecutionClass::DirectReply
            || self.deliberation_class != TurnDeliberationClass::Standard
            || !self.final_outcome.trim().is_empty()
            || self.pressure != TurnPersonaPressureLevel::Normal
            || !self.mode.current_mode.trim().is_empty()
            || !self.tool_path.path.trim().is_empty()
            || self.tool_path.tool_calls > 0
            || self.tool_path.react_rounds > 0
            || self.tool_path.current_primary_delivered
            || self.blocker.is_some()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnReasoningIntentLedger {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub strategy: String,
    #[serde(default)]
    pub confidence: u8,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub rationale: Vec<String>,
    #[serde(default)]
    pub preferred_tools: Vec<String>,
    #[serde(default)]
    pub runtime_grounding_required: bool,
}

impl TurnReasoningIntentLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.kind.trim().is_empty()
            || !self.strategy.trim().is_empty()
            || self.confidence > 0
            || !self.summary.trim().is_empty()
            || !self.rationale.is_empty()
            || !self.preferred_tools.is_empty()
            || self.runtime_grounding_required
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnCounterfactualSnapshotLedger {
    #[serde(default)]
    pub request_kind: String,
    #[serde(default)]
    pub evidence_need: String,
    #[serde(default)]
    pub execution_preference: String,
    #[serde(default)]
    pub action_family: String,
    #[serde(default)]
    pub deliberation_class: String,
    #[serde(default)]
    pub reasoning_kind: String,
    #[serde(default)]
    pub reasoning_strategy: String,
    #[serde(default)]
    pub confidence: u8,
    #[serde(default)]
    pub runtime_grounding_required: bool,
    #[serde(default)]
    pub has_tools: bool,
    #[serde(default)]
    pub active_task_context_present: bool,
    #[serde(default)]
    pub governed_memory_evidence_present: bool,
}

impl TurnCounterfactualSnapshotLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.request_kind.trim().is_empty()
            || !self.evidence_need.trim().is_empty()
            || !self.execution_preference.trim().is_empty()
            || !self.action_family.trim().is_empty()
            || !self.deliberation_class.trim().is_empty()
            || !self.reasoning_kind.trim().is_empty()
            || !self.reasoning_strategy.trim().is_empty()
            || self.confidence > 0
            || self.runtime_grounding_required
            || self.has_tools
            || self.active_task_context_present
            || self.governed_memory_evidence_present
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnCounterfactualBranchLedger {
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub score: u8,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub rationale: Vec<String>,
    #[serde(default)]
    pub requires_native_tool_round: bool,
}

impl TurnCounterfactualBranchLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.branch.trim().is_empty()
            || self.score > 0
            || !self.summary.trim().is_empty()
            || !self.rationale.is_empty()
            || self.requires_native_tool_round
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnCounterfactualLedger {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub snapshot: TurnCounterfactualSnapshotLedger,
    #[serde(default)]
    pub selected_branch: TurnCounterfactualBranchLedger,
    #[serde(default)]
    pub alternatives: Vec<TurnCounterfactualBranchLedger>,
}

impl TurnCounterfactualLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.summary.trim().is_empty()
            || self.snapshot.is_meaningful()
            || self.selected_branch.is_meaningful()
            || !self.alternatives.is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnAdversarialArenaClaimLedger {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub evidence_score: u8,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub requires_native_tool_round: bool,
}

impl TurnAdversarialArenaClaimLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.role.trim().is_empty()
            || !self.label.trim().is_empty()
            || self.evidence_score > 0
            || !self.summary.trim().is_empty()
            || !self.signals.is_empty()
            || self.requires_native_tool_round
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnAdversarialArenaLedger {
    #[serde(default)]
    pub subject_kind: String,
    #[serde(default)]
    pub disposition: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub winner: TurnAdversarialArenaClaimLedger,
    #[serde(default)]
    pub defender: TurnAdversarialArenaClaimLedger,
    #[serde(default)]
    pub attacker: TurnAdversarialArenaClaimLedger,
}

impl TurnAdversarialArenaLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.subject_kind.trim().is_empty()
            || !self.disposition.trim().is_empty()
            || !self.summary.trim().is_empty()
            || self.winner.is_meaningful()
            || self.defender.is_meaningful()
            || self.attacker.is_meaningful()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSubjectStateLedger {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub identity_anchor: String,
    #[serde(default)]
    pub governance_mode: String,
    #[serde(default)]
    pub relationship_state: String,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub task_scope: String,
    #[serde(default)]
    pub initiative_posture: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub resource_posture: String,
    #[serde(default)]
    pub boundary_mode: String,
}

impl TurnSubjectStateLedger {
    pub fn is_meaningful(&self) -> bool {
        !self.summary.trim().is_empty()
            || !self.identity_anchor.trim().is_empty()
            || !self.governance_mode.trim().is_empty()
            || !self.relationship_state.trim().is_empty()
            || !self.response_mode.trim().is_empty()
            || !self.task_scope.trim().is_empty()
            || !self.initiative_posture.trim().is_empty()
            || !self.relationship_posture.trim().is_empty()
            || !self.resource_posture.trim().is_empty()
            || !self.boundary_mode.trim().is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSoulReplyLedger {
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub identity_anchor: String,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub expression_mode: String,
}

impl TurnSoulReplyLedger {
    pub fn is_meaningful(&self) -> bool {
        self.applied
            || !self.summary.trim().is_empty()
            || !self.identity_anchor.trim().is_empty()
            || !self.response_mode.trim().is_empty()
            || !self.relationship_posture.trim().is_empty()
            || !self.expression_mode.trim().is_empty()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSoulInitiativeLedger {
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub governance_mode: String,
    #[serde(default)]
    pub initiative_posture: String,
    #[serde(default)]
    pub compact_reply: bool,
    #[serde(default)]
    pub explicit_blocker: bool,
}

impl TurnSoulInitiativeLedger {
    pub fn is_meaningful(&self) -> bool {
        self.applied
            || !self.summary.trim().is_empty()
            || !self.governance_mode.trim().is_empty()
            || !self.initiative_posture.trim().is_empty()
            || self.compact_reply
            || self.explicit_blocker
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSoulStrategyLedger {
    #[serde(default)]
    pub applied: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub current_mode: String,
    #[serde(default)]
    pub next_focus: String,
    #[serde(default)]
    pub idle_enabled: bool,
    #[serde(default)]
    pub idle_interval_secs: u64,
    #[serde(default)]
    pub post_reply_self_runtime_enqueued: bool,
}

impl TurnSoulStrategyLedger {
    pub fn is_meaningful(&self) -> bool {
        self.applied
            || !self.summary.trim().is_empty()
            || !self.current_mode.trim().is_empty()
            || !self.next_focus.trim().is_empty()
            || self.idle_enabled
            || self.idle_interval_secs > 0
            || self.post_reply_self_runtime_enqueued
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnSoulFeedbackLedger {
    #[serde(default)]
    pub reply: TurnSoulReplyLedger,
    #[serde(default)]
    pub initiative: TurnSoulInitiativeLedger,
    #[serde(default)]
    pub strategy: TurnSoulStrategyLedger,
}

impl TurnSoulFeedbackLedger {
    pub fn is_meaningful(&self) -> bool {
        self.reply.is_meaningful()
            || self.initiative.is_meaningful()
            || self.strategy.is_meaningful()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnPersonaLedger {
    #[serde(default)]
    pub disclosure: Option<TurnPersonaDisclosureLedger>,
    #[serde(default)]
    pub priority: Option<TurnPersonaPriorityLedger>,
    #[serde(default)]
    pub review: TurnPersonaReviewLedger,
    #[serde(default)]
    pub touched_targets: Vec<String>,
    #[serde(default)]
    pub pressure: TurnPersonaPressureLevel,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub reply_scope: String,
    #[serde(default)]
    pub reply_delivered: bool,
}

impl TurnPersonaLedger {
    pub fn is_meaningful(&self) -> bool {
        self.disclosure.is_some()
            || self.priority.is_some()
            || self.review.applied
            || self.review.rewrite_applied
            || !self.touched_targets.is_empty()
            || self.pressure != TurnPersonaPressureLevel::Normal
            || self.tool_calls > 0
            || !self.reply_scope.trim().is_empty()
            || self.reply_delivered
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnLedgerStatus {
    #[default]
    Running,
    Answered,
    Interrupted,
    Failed,
}

impl TurnLedgerStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Answered => "answered",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnDeliveryLedger {
    #[serde(default)]
    pub append_only_ack_sent: u8,
    #[serde(default)]
    pub append_only_heartbeat_sent: u8,
    #[serde(default)]
    pub append_only_first_tool_milestone_sent: u8,
    #[serde(default)]
    #[serde(alias = "progress_updates_sent")]
    pub edit_phase_header_updates_sent: u8,
    #[serde(default)]
    pub partial_updates_sent: u8,
    #[serde(default)]
    pub tool_outbound_intents_seen: u8,
    #[serde(default)]
    pub tool_visible_updates_sent: u8,
    #[serde(default)]
    pub explicit_outbound_sent: u8,
    #[serde(default)]
    pub tool_outbound_suppressed: u8,
    #[serde(default)]
    pub current_primary_delivered: bool,
    #[serde(default)]
    pub finalize_streamed: bool,
    #[serde(default)]
    pub visible_text_updates_sent: u8,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnLedger {
    #[serde(default)]
    pub req_id: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub ingress: IngressKind,
    #[serde(default)]
    pub source_transport: MessageTransport,
    #[serde(default)]
    pub platform_message_id: String,
    #[serde(default)]
    pub platform_event_id: String,
    #[serde(default)]
    pub inbound_dedup_key: String,
    #[serde(default)]
    pub body_kind: MessageBodyKind,
    #[serde(default)]
    pub has_media: bool,
    #[serde(default)]
    pub user_preview: String,
    #[serde(default)]
    pub reply_preview: String,
    #[serde(default)]
    pub status: TurnLedgerStatus,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub outbound_source: String,
    #[serde(default)]
    pub canonical_reply_source: String,
    #[serde(default)]
    pub started_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default)]
    pub finished_at_ms: u64,
    #[serde(default)]
    pub react_rounds: u32,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub any_tool_used: bool,
    #[serde(default)]
    pub final_reply_delivered: bool,
    #[serde(default)]
    pub reply_handoff_ms: u64,
    #[serde(default)]
    pub post_reply_ms: u64,
    #[serde(default)]
    pub total_ms: u64,
    #[serde(default)]
    pub ttft_ms: u64,
    #[serde(default)]
    pub delivery: TurnDeliveryLedger,
    #[serde(default)]
    pub subject_state: Option<TurnSubjectStateLedger>,
    #[serde(default)]
    pub observation: Option<TurnObservationLedger>,
    #[serde(default)]
    pub persona: Option<TurnPersonaLedger>,
    #[serde(default)]
    pub soul_feedback: Option<TurnSoulFeedbackLedger>,
    #[serde(default)]
    pub reasoning_intent: Option<TurnReasoningIntentLedger>,
    #[serde(default)]
    pub counterfactual: Option<TurnCounterfactualLedger>,
    #[serde(default)]
    pub adversarial_arena: Option<TurnAdversarialArenaLedger>,
}

pub trait TurnLedgerStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>>;
    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnLedger>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        Ok(self.get(chat_id)?.into_iter().collect())
    }

    fn recent_persona_evidence(&self, chat_id: &str) -> Result<Option<RecentPersonaEvidence>> {
        let ledgers = self.list_recent(chat_id, RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK)?;
        Ok(derive_recent_persona_evidence(
            &ledgers,
            RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
        ))
    }
}

/// In-memory turn ledger store for profiles that should not persist full ledger history.
pub struct VolatileTurnLedgerStore {
    max_chats: usize,
    values: Mutex<HashMap<String, TurnLedger>>,
}

impl VolatileTurnLedgerStore {
    /// Create a bounded volatile ledger store, clearing older chat entries when the cap is reached.
    pub fn new(max_chats: usize) -> Self {
        Self {
            max_chats: max_chats.max(1),
            values: Mutex::new(HashMap::new()),
        }
    }
}

impl TurnLedgerStore for VolatileTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        Ok(self
            .values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(chat_id)
            .cloned())
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        if !values.contains_key(chat_id) && values.len() >= self.max_chats {
            values.clear();
        }
        values.insert(chat_id.to_string(), ledger.clone());
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(chat_id);
        Ok(())
    }
}

pub fn build_turn_ledger_start(msg: &PcMsg, started_at_ms: u64) -> TurnLedger {
    TurnLedger {
        req_id: msg.req_id.clone().unwrap_or_default(),
        channel: msg.channel.to_string(),
        ingress: msg.ingress,
        source_transport: msg.source_transport,
        platform_message_id: msg.platform_message_id.clone(),
        platform_event_id: msg.platform_event_id.clone(),
        inbound_dedup_key: msg.inbound_dedup_key.clone(),
        body_kind: msg.body_kind(),
        has_media: msg.has_media_body(),
        user_preview: normalize_turn_preview(&msg.content),
        started_at_ms,
        updated_at_ms: started_at_ms,
        status: TurnLedgerStatus::Running,
        ..TurnLedger::default()
    }
}

pub fn normalize_turn_preview(content: &str) -> String {
    truncate_content_to_max(content.trim(), TURN_LEDGER_PREVIEW_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_reason(reason: &str) -> String {
    truncate_content_to_max(reason.trim(), TURN_LEDGER_REASON_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_persona_scope(scope: &str) -> String {
    truncate_content_to_max(scope.trim(), TURN_PERSONA_SCOPE_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_subject_state_text(content: &str) -> String {
    truncate_content_to_max(content.trim(), TURN_SUBJECT_STATE_TEXT_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_subject_state_summary(content: &str) -> String {
    truncate_content_to_max(content.trim(), TURN_SUBJECT_STATE_SUMMARY_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_observation_text(content: &str) -> String {
    truncate_content_to_max(content.trim(), TURN_OBSERVATION_TEXT_MAX_CHARS)
        .trim()
        .to_string()
}

pub fn normalize_turn_persona_targets(targets: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(targets.len());
    for target in targets {
        let target = truncate_content_to_max(target.trim(), 48)
            .trim()
            .to_string();
        if target.is_empty() || normalized.iter().any(|existing| existing == &target) {
            continue;
        }
        normalized.push(target);
    }
    normalized
}

pub fn render_turn_persona_ledger_block(
    persona: &TurnPersonaLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !persona.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Latest Turn Persona Outcome\n");
    let _ = writeln!(out, "Pressure: {}", persona.pressure.as_str());
    if !persona.reply_scope.trim().is_empty() {
        let _ = writeln!(out, "Reply scope: {}", persona.reply_scope.trim());
    }
    let _ = writeln!(out, "Reply delivered: {}", persona.reply_delivered);
    if persona.tool_calls > 0 {
        let _ = writeln!(out, "Tool calls: {}", persona.tool_calls);
    }
    if let Some(disclosure) = persona.disclosure.as_ref() {
        let mut summary = format!(
            "action={}",
            turn_persona_share_action_label(disclosure.share_action)
        );
        if !disclosure.request_kind.trim().is_empty() {
            summary.push_str(" request=");
            summary.push_str(disclosure.request_kind.trim());
        }
        if !disclosure.response_mode.trim().is_empty() {
            summary.push_str(" mode=");
            summary.push_str(disclosure.response_mode.trim());
        }
        if disclosure.acknowledge_boundary {
            summary.push_str(" acknowledge_boundary=true");
        }
        let _ = writeln!(out, "Disclosure: {}", summary);
        if !disclosure.targets.is_empty() {
            let _ = writeln!(out, "Disclosure targets: {}", disclosure.targets.join(", "));
        }
        if !disclosure.response_guidance.trim().is_empty() {
            let guidance = truncate_content_to_max(
                disclosure.response_guidance.trim(),
                TURN_PERSONA_TEXT_MAX_CHARS,
            );
            let _ = writeln!(out, "Disclosure guidance: {}", guidance);
        }
    }
    if let Some(priority) = persona.priority.as_ref() {
        if !priority.stance_summary.trim().is_empty() {
            let summary = truncate_content_to_max(
                priority.stance_summary.trim(),
                TURN_PERSONA_TEXT_MAX_CHARS,
            );
            let _ = writeln!(out, "Priority stance: {}", summary);
        }
        if !priority.priority_order.is_empty() {
            let _ = writeln!(
                out,
                "Priority order: {}",
                priority.priority_order.join(" > ")
            );
        }
        if !priority.task_scope.trim().is_empty() {
            let _ = writeln!(out, "Priority task scope: {}", priority.task_scope.trim());
        }
        if !priority.response_mode.trim().is_empty() {
            let _ = writeln!(out, "Priority mode: {}", priority.response_mode.trim());
        }
        if !priority.response_guidance.trim().is_empty() {
            let guidance = truncate_content_to_max(
                priority.response_guidance.trim(),
                TURN_PERSONA_TEXT_MAX_CHARS,
            );
            let _ = writeln!(out, "Priority guidance: {}", guidance);
        }
    }
    let _ = writeln!(
        out,
        "Privacy review: action={} applied={} rewrite_applied={}",
        turn_persona_share_action_label(persona.review.action),
        persona.review.applied,
        persona.review.rewrite_applied
    );
    if !persona.touched_targets.is_empty() {
        let _ = writeln!(
            out,
            "Touched targets: {}",
            persona.touched_targets.join(", ")
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn render_turn_observation_ledger_block(
    observation: &TurnObservationLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !observation.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Latest Turn Observation\n");
    let _ = writeln!(
        out,
        "Execution class: {}",
        observation.execution_class.label()
    );
    let _ = writeln!(
        out,
        "Deliberation: {}",
        observation.deliberation_class.label()
    );
    if !observation.final_outcome.trim().is_empty() {
        let _ = writeln!(out, "Final outcome: {}", observation.final_outcome.trim());
    }
    let _ = writeln!(out, "Pressure: {}", observation.pressure.as_str());
    if !observation.mode.current_mode.trim().is_empty() {
        let _ = writeln!(out, "Mode: {}", observation.mode.current_mode.trim());
        let _ = writeln!(
            out,
            "Mode budget: non_voice_outbound={} idle_self_runtime={}",
            observation.mode.allow_non_voice_outbound, observation.mode.allow_idle_self_runtime
        );
    }
    if !observation.tool_path.path.trim().is_empty() {
        let _ = writeln!(out, "Tool path: {}", observation.tool_path.path.trim());
    }
    if observation.tool_path.tool_calls > 0 || observation.tool_path.react_rounds > 0 {
        let _ = writeln!(
            out,
            "Tool stats: calls={} rounds={} current_primary_delivered={}",
            observation.tool_path.tool_calls,
            observation.tool_path.react_rounds,
            observation.tool_path.current_primary_delivered,
        );
    }
    if let Some(blocker) = observation.blocker.as_ref() {
        let _ = writeln!(
            out,
            "Blocker: {} {}/{}",
            blocker.kind.trim(),
            blocker.failed_calls,
            blocker.total_calls
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn render_turn_reasoning_intent_ledger_block(
    reasoning_intent: &TurnReasoningIntentLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !reasoning_intent.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(512));
    out.push_str("## Latest Programmable Reasoning Intent\n");
    if !reasoning_intent.kind.trim().is_empty() {
        let _ = writeln!(out, "Kind: {}", reasoning_intent.kind.trim());
    }
    if !reasoning_intent.strategy.trim().is_empty() {
        let _ = writeln!(out, "Strategy: {}", reasoning_intent.strategy.trim());
    }
    if reasoning_intent.confidence > 0 {
        let _ = writeln!(out, "Confidence: {}", reasoning_intent.confidence);
    }
    if !reasoning_intent.summary.trim().is_empty() {
        let _ = writeln!(
            out,
            "Summary: {}",
            truncate_content_to_max(
                reasoning_intent.summary.trim(),
                TURN_REASONING_SUMMARY_MAX_CHARS
            )
        );
    }
    let _ = writeln!(
        out,
        "Runtime grounding required: {}",
        reasoning_intent.runtime_grounding_required
    );
    if !reasoning_intent.rationale.is_empty() {
        let normalized = reasoning_intent
            .rationale
            .iter()
            .map(|item| {
                truncate_content_to_max(item.trim(), TURN_REASONING_SIGNAL_MAX_CHARS)
                    .trim()
                    .to_string()
            })
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if !normalized.is_empty() {
            let _ = writeln!(out, "Signals: {}", normalized.join(" | "));
        }
    }
    if !reasoning_intent.preferred_tools.is_empty() {
        let normalized = reasoning_intent
            .preferred_tools
            .iter()
            .map(|item| {
                truncate_content_to_max(item.trim(), TURN_REASONING_SIGNAL_MAX_CHARS)
                    .trim()
                    .to_string()
            })
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if !normalized.is_empty() {
            let _ = writeln!(out, "Preferred tools: {}", normalized.join(", "));
        }
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn render_turn_counterfactual_ledger_block(
    counterfactual: &TurnCounterfactualLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !counterfactual.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Latest Counterfactual Sandbox\n");
    if !counterfactual.summary.trim().is_empty() {
        let _ = writeln!(out, "Summary: {}", counterfactual.summary.trim());
    }
    if counterfactual.selected_branch.is_meaningful() {
        let _ = writeln!(
            out,
            "Selected: {} ({})",
            counterfactual.selected_branch.branch.trim(),
            counterfactual.selected_branch.score
        );
        if !counterfactual.selected_branch.summary.trim().is_empty() {
            let _ = writeln!(
                out,
                "Selected summary: {}",
                counterfactual.selected_branch.summary.trim()
            );
        }
    }
    if !counterfactual.alternatives.is_empty() {
        let branches = counterfactual
            .alternatives
            .iter()
            .filter(|branch| branch.is_meaningful())
            .map(|branch| format!("{} ({})", branch.branch.trim(), branch.score))
            .collect::<Vec<_>>();
        if !branches.is_empty() {
            let _ = writeln!(out, "Rejected: {}", branches.join(" | "));
        }
    }
    if counterfactual.snapshot.is_meaningful() {
        let _ = writeln!(
            out,
            "Snapshot: request_kind={} evidence_need={} action_family={} deliberation={} reasoning={} strategy={} confidence={}",
            counterfactual.snapshot.request_kind.trim(),
            counterfactual.snapshot.evidence_need.trim(),
            counterfactual.snapshot.action_family.trim(),
            counterfactual.snapshot.deliberation_class.trim(),
            counterfactual.snapshot.reasoning_kind.trim(),
            counterfactual.snapshot.reasoning_strategy.trim(),
            counterfactual.snapshot.confidence
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn render_turn_adversarial_arena_ledger_block(
    arena: &TurnAdversarialArenaLedger,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !arena.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Latest Adversarial Arena\n");
    if !arena.subject_kind.trim().is_empty() {
        let _ = writeln!(out, "Subject: {}", arena.subject_kind.trim());
    }
    if !arena.disposition.trim().is_empty() {
        let _ = writeln!(out, "Disposition: {}", arena.disposition.trim());
    }
    if !arena.summary.trim().is_empty() {
        let _ = writeln!(out, "Summary: {}", arena.summary.trim());
    }
    if arena.winner.is_meaningful() {
        let _ = writeln!(
            out,
            "Winner: {} ({})",
            arena.winner.label.trim(),
            arena.winner.evidence_score
        );
    }
    if arena.defender.is_meaningful() {
        let _ = writeln!(
            out,
            "Defender: {} ({})",
            arena.defender.label.trim(),
            arena.defender.evidence_score
        );
    }
    if arena.attacker.is_meaningful() {
        let _ = writeln!(
            out,
            "Attacker: {} ({})",
            arena.attacker.label.trim(),
            arena.attacker.evidence_score
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn turn_ledger_observed_at_ms(ledger: &TurnLedger) -> u64 {
    if ledger.finished_at_ms > 0 {
        ledger.finished_at_ms
    } else if ledger.updated_at_ms > 0 {
        ledger.updated_at_ms
    } else {
        ledger.started_at_ms
    }
}

pub fn build_turn_persona_disclosure_ledger(
    adjudication: &MentalPrivacyDisclosureAdjudication,
) -> TurnPersonaDisclosureLedger {
    TurnPersonaDisclosureLedger {
        request_kind: truncate_content_to_max(adjudication.request_kind.trim(), 32).into_owned(),
        share_action: adjudication.share_action,
        acknowledge_boundary: adjudication.acknowledge_boundary,
        targets: normalize_turn_persona_targets(&adjudication.targets),
        response_mode: truncate_content_to_max(adjudication.response_mode.trim(), 40).into_owned(),
        response_guidance: truncate_content_to_max(
            adjudication.response_guidance.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
    }
}

pub fn build_turn_persona_priority_ledger(
    adjudication: &PersonaPriorityAdjudication,
) -> TurnPersonaPriorityLedger {
    TurnPersonaPriorityLedger {
        stance_summary: truncate_content_to_max(
            adjudication.stance_summary.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
        priority_order: normalize_turn_persona_targets(&adjudication.priority_order),
        response_mode: truncate_content_to_max(adjudication.response_mode.trim(), 40).into_owned(),
        task_scope: normalize_turn_persona_scope(&adjudication.task_scope),
        initiative_posture: truncate_content_to_max(
            adjudication.initiative_posture.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
        relationship_posture: truncate_content_to_max(
            adjudication.relationship_posture.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
        resource_posture: truncate_content_to_max(
            adjudication.resource_posture.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
        response_guidance: truncate_content_to_max(
            adjudication.response_guidance.trim(),
            TURN_PERSONA_TEXT_MAX_CHARS,
        )
        .into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_turn_ledger_start_keeps_compact_user_preview() {
        let mut msg = crate::bus::PcMsg::new_inbound(
            "qq_channel",
            "chat-1",
            "  这是一个很长的输入\n\n需要被压成预览  ",
            false,
        )
        .expect("message");
        msg.req_id = Some("req-1".to_string());
        msg.source_transport = crate::bus::MessageTransport::Wss;
        msg.platform_message_id = "msg-1".to_string();
        msg.platform_event_id = "evt-1".to_string();
        msg.inbound_dedup_key = "qq_message:msg-1".to_string();

        let ledger = build_turn_ledger_start(&msg, 123);
        assert_eq!(ledger.req_id, "req-1");
        assert_eq!(ledger.channel, "qq_channel");
        assert_eq!(ledger.status, TurnLedgerStatus::Running);
        assert_eq!(ledger.user_preview, "这是一个很长的输入\n\n需要被压成预览");
        assert_eq!(ledger.started_at_ms, 123);
        assert_eq!(ledger.updated_at_ms, 123);
        assert_eq!(ledger.source_transport, crate::bus::MessageTransport::Wss);
        assert_eq!(ledger.platform_message_id, "msg-1");
        assert_eq!(ledger.platform_event_id, "evt-1");
        assert_eq!(ledger.inbound_dedup_key, "qq_message:msg-1");
    }

    #[test]
    fn normalize_turn_reason_trims_and_caps() {
        let reason = format!("  {}  ", "x".repeat(128));
        let normalized = normalize_turn_reason(&reason);
        assert_eq!(normalized.len(), TURN_LEDGER_REASON_MAX_CHARS);
        assert!(normalized.chars().all(|ch| ch == 'x'));
    }

    #[test]
    fn turn_subject_state_ledger_is_meaningful_when_summary_exists() {
        let ledger = TurnSubjectStateLedger {
            summary: "adaptive | protective_brief | narrow".to_string(),
            ..TurnSubjectStateLedger::default()
        };
        assert!(ledger.is_meaningful());
    }

    #[test]
    fn render_turn_observation_ledger_block_includes_mode_tool_path_and_blocker() {
        let rendered = render_turn_observation_ledger_block(
            &TurnObservationLedger {
                execution_class: TurnExecutionClass::ToolAssisted,
                deliberation_class: TurnDeliberationClass::HardReasoning,
                final_outcome: "surface_finalization".to_string(),
                pressure: TurnPersonaPressureLevel::Cautious,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: "surface_finalization".to_string(),
                    tool_calls: 3,
                    react_rounds: 2,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "retryable".to_string(),
                    failed_calls: 2,
                    total_calls: 2,
                }),
            },
            420,
        )
        .expect("observation block");

        assert!(rendered.contains("## Latest Turn Observation"));
        assert!(rendered.contains("Execution class: tool_assisted"));
        assert!(rendered.contains("Deliberation: hard_reasoning"));
        assert!(rendered.contains("Mode: normal"));
        assert!(rendered.contains("Pressure: cautious"));
        assert!(rendered.contains("Tool path: surface_finalization"));
        assert!(rendered.contains("Final outcome: surface_finalization"));
        assert!(rendered.contains("Blocker: retryable 2/2"));
    }

    #[test]
    fn render_turn_reasoning_intent_ledger_block_includes_kind_strategy_and_tools() {
        let rendered = render_turn_reasoning_intent_ledger_block(
            &TurnReasoningIntentLedger {
                kind: "engineering_synthesis".to_string(),
                strategy: "require_native_tool_round".to_string(),
                confidence: 92,
                summary: "Compile runtime evidence before answering.".to_string(),
                rationale: vec!["hard_reasoning".to_string(), "host_tool".to_string()],
                preferred_tools: vec!["lua_query".to_string(), "office_status".to_string()],
                runtime_grounding_required: true,
            },
            420,
        )
        .expect("reasoning intent block");

        assert!(rendered.contains("## Latest Programmable Reasoning Intent"));
        assert!(rendered.contains("Kind: engineering_synthesis"));
        assert!(rendered.contains("Strategy: require_native_tool_round"));
        assert!(rendered.contains("Confidence: 92"));
        assert!(rendered.contains("Preferred tools: lua_query, office_status"));
        assert!(rendered.contains("Runtime grounding required: true"));
    }

    #[test]
    fn render_turn_counterfactual_ledger_block_includes_selected_and_rejected_branches() {
        let rendered = render_turn_counterfactual_ledger_block(
            &TurnCounterfactualLedger {
                summary: "Prefer structured tool synthesis over direct reply.".to_string(),
                snapshot: TurnCounterfactualSnapshotLedger {
                    request_kind: "general".to_string(),
                    evidence_need: "host_tool".to_string(),
                    execution_preference: "tool_first".to_string(),
                    action_family: "action_request".to_string(),
                    deliberation_class: "hard_reasoning".to_string(),
                    reasoning_kind: "engineering_synthesis".to_string(),
                    reasoning_strategy: "require_native_tool_round".to_string(),
                    confidence: 91,
                    runtime_grounding_required: true,
                    has_tools: true,
                    active_task_context_present: true,
                    governed_memory_evidence_present: false,
                },
                selected_branch: TurnCounterfactualBranchLedger {
                    branch: "structured_tool_synthesis".to_string(),
                    score: 94,
                    summary: "Collect live evidence, then synthesize one coherent action answer."
                        .to_string(),
                    rationale: vec!["hard_reasoning".to_string()],
                    requires_native_tool_round: true,
                },
                alternatives: vec![TurnCounterfactualBranchLedger {
                    branch: "direct_reply".to_string(),
                    score: 34,
                    summary: "Answer immediately from the current context.".to_string(),
                    rationale: vec!["under_grounded".to_string()],
                    requires_native_tool_round: false,
                }],
            },
            420,
        )
        .expect("counterfactual block");

        assert!(rendered.contains("## Latest Counterfactual Sandbox"));
        assert!(rendered.contains("Selected: structured_tool_synthesis (94)"));
        assert!(rendered.contains("Rejected: direct_reply (34)"));
    }

    #[test]
    fn render_turn_adversarial_arena_ledger_block_includes_winner_and_challenger() {
        let rendered = render_turn_adversarial_arena_ledger_block(
            &TurnAdversarialArenaLedger {
                subject_kind: "turn_strategy".to_string(),
                disposition: "hold_for_clarification".to_string(),
                summary: "Attacker overturned the live action path because the blocker signal was stronger.".to_string(),
                winner: TurnAdversarialArenaClaimLedger {
                    role: "attacker".to_string(),
                    label: "clarify_before_action".to_string(),
                    evidence_score: 88,
                    summary: "Ask for the missing blocker before acting.".to_string(),
                    signals: vec!["explicit_blocker".to_string()],
                    requires_native_tool_round: false,
                },
                defender: TurnAdversarialArenaClaimLedger {
                    role: "defender".to_string(),
                    label: "structured_tool_synthesis".to_string(),
                    evidence_score: 84,
                    summary: "Collect live evidence, then synthesize.".to_string(),
                    signals: vec!["host_tool".to_string()],
                    requires_native_tool_round: true,
                },
                attacker: TurnAdversarialArenaClaimLedger {
                    role: "attacker".to_string(),
                    label: "clarify_before_action".to_string(),
                    evidence_score: 88,
                    summary: "Ask for the missing blocker before acting.".to_string(),
                    signals: vec!["explicit_blocker".to_string()],
                    requires_native_tool_round: false,
                },
            },
            420,
        )
        .expect("arena block");

        assert!(rendered.contains("## Latest Adversarial Arena"));
        assert!(rendered.contains("Disposition: hold_for_clarification"));
        assert!(rendered.contains("Winner: clarify_before_action (88)"));
        assert!(rendered.contains("Defender: structured_tool_synthesis (84)"));
        assert!(rendered.contains("Attacker: clarify_before_action (88)"));
    }

    #[test]
    fn turn_delivery_ledger_deserializes_legacy_progress_alias_into_edit_phase_headers() {
        let ledger: TurnDeliveryLedger = serde_json::from_value(serde_json::json!({
            "progress_updates_sent": 3,
            "append_only_ack_sent": 1,
        }))
        .expect("legacy delivery ledger");

        assert_eq!(ledger.edit_phase_header_updates_sent, 3);
        assert_eq!(ledger.append_only_ack_sent, 1);

        let serialized = serde_json::to_value(&ledger).expect("serialize delivery ledger");
        assert_eq!(
            serialized.get("edit_phase_header_updates_sent"),
            Some(&serde_json::Value::from(3))
        );
        assert!(serialized.get("progress_updates_sent").is_none());
    }
}
