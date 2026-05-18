//! Compact terminal turn evidence used by LLM/Agent continuity on embedded targets.

use crate::bus::IngressKind;
use crate::error::Result;
use serde::{Deserialize, Serialize};

use super::{
    turn_ledger_observed_at_ms, RecentPersonaEvidence, TurnLedger, TurnLedgerStatus,
    TurnPersonaLedger, RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
};

/// Host-side relative storage namespace for compact terminal turn evidence.
pub const REL_PATH_TURN_CONTINUITY_EVIDENCE: &str = "memory/turn_continuity_evidence";
/// Maximum compact evidence records retained per relationship scope.
pub const TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS: usize = 16;

/// Minimal terminal evidence that may affect future reply/persona continuity.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnContinuityEvidence {
    #[serde(default)]
    pub ingress: IngressKind,
    #[serde(default)]
    pub status: TurnLedgerStatus,
    #[serde(default)]
    pub final_reply_delivered: bool,
    #[serde(default)]
    pub canonical_reply_source: String,
    #[serde(default)]
    pub observed_at_ms: u64,
    #[serde(default)]
    pub persona: Option<TurnPersonaLedger>,
}

impl TurnContinuityEvidence {
    pub fn from_turn_ledger(ledger: &TurnLedger) -> Option<Self> {
        ledger.status.is_terminal().then(|| Self {
            ingress: ledger.ingress,
            status: ledger.status,
            final_reply_delivered: ledger.final_reply_delivered,
            canonical_reply_source: ledger.canonical_reply_source.clone(),
            observed_at_ms: turn_ledger_observed_at_ms(ledger),
            persona: ledger.persona.clone(),
        })
    }
}

/// Store for compact terminal turn evidence used by prompt/persona continuity.
pub trait TurnContinuityEvidenceStore: Send + Sync {
    /// Append one terminal evidence record for a relationship scope.
    fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()>;
    /// Clear compact evidence for a relationship scope.
    fn clear(&self, chat_id: &str) -> Result<()>;
    /// Return recent evidence in newest-first order.
    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>>;

    /// Derive recent persona continuity evidence from compact terminal turn records.
    fn recent_persona_evidence(&self, chat_id: &str) -> Result<Option<RecentPersonaEvidence>> {
        let evidence = self.list_recent(
            chat_id,
            super::RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK
                .min(TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS),
        )?;
        Ok(
            super::derive_recent_persona_evidence_from_continuity_evidence(
                &evidence,
                RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
            ),
        )
    }
}
