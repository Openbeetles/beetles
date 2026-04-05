//! 最近一轮执行账本：记录对话最近一次请求的执行与交付摘要。
//! Latest turn ledger: execution and delivery summary for the most recent request.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction, PersonaPriorityAdjudication,
};

pub const REL_PATH_TURN_LEDGERS: &str = "memory/turn_ledgers";
pub const REL_PATH_TURN_LEDGERS_LEGACY: &str = "memory/turn_ledgers.json";
pub const REL_PATH_TURN_LEDGER_HISTORY: &str = "memory/turn_ledger_history";
pub const TURN_LEDGER_HISTORY_MAX_ITEMS: usize = 32;
const TURN_LEDGER_PREVIEW_MAX_CHARS: usize = 240;
const TURN_LEDGER_REASON_MAX_CHARS: usize = 96;
const TURN_PERSONA_TEXT_MAX_CHARS: usize = 160;
const TURN_PERSONA_SCOPE_MAX_CHARS: usize = 24;

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
    pub waiting_notice_sent: bool,
    #[serde(default)]
    pub progress_updates_sent: u8,
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
    pub user_preview: String,
    #[serde(default)]
    pub reply_preview: String,
    #[serde(default)]
    pub status: TurnLedgerStatus,
    #[serde(default)]
    pub reason: String,
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
    pub final_answer_recovered: bool,
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
    pub persona: Option<TurnPersonaLedger>,
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
}

pub fn build_turn_ledger_start(
    req_id: &str,
    channel: &str,
    ingress: IngressKind,
    user_content: &str,
    started_at_ms: u64,
) -> TurnLedger {
    TurnLedger {
        req_id: req_id.to_string(),
        channel: channel.to_string(),
        ingress,
        user_preview: normalize_turn_preview(user_content),
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
        let ledger = build_turn_ledger_start(
            "req-1",
            "qq_channel",
            IngressKind::User,
            "  这是一个很长的输入\n\n需要被压成预览  ",
            123,
        );
        assert_eq!(ledger.req_id, "req-1");
        assert_eq!(ledger.channel, "qq_channel");
        assert_eq!(ledger.status, TurnLedgerStatus::Running);
        assert_eq!(ledger.user_preview, "这是一个很长的输入\n\n需要被压成预览");
        assert_eq!(ledger.started_at_ms, 123);
        assert_eq!(ledger.updated_at_ms, 123);
    }

    #[test]
    fn normalize_turn_reason_trims_and_caps() {
        let reason = format!("  {}  ", "x".repeat(128));
        let normalized = normalize_turn_reason(&reason);
        assert_eq!(normalized.len(), TURN_LEDGER_REASON_MAX_CHARS);
        assert!(normalized.chars().all(|ch| ch == 'x'));
    }
}
