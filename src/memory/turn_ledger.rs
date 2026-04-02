//! 最近一轮执行账本：记录对话最近一次请求的执行与交付摘要。
//! Latest turn ledger: execution and delivery summary for the most recent request.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub const REL_PATH_TURN_LEDGERS: &str = "memory/turn_ledgers.json";
const TURN_LEDGER_PREVIEW_MAX_CHARS: usize = 240;
const TURN_LEDGER_REASON_MAX_CHARS: usize = 96;

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
}

pub trait TurnLedgerStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>>;
    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
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
