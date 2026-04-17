//! Adversarial arena contracts and operator-visible audit feed.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const ARENA_SIGNAL_MAX_ITEMS: usize = 4;
const ARENA_SIGNAL_MAX_CHARS: usize = 32;
const ARENA_LABEL_MAX_CHARS: usize = 48;
const ARENA_SUMMARY_MAX_CHARS: usize = 180;
const ARENA_TIMELINE_DETAIL_MAX_CHARS: usize = 220;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const ARENA_EVENT_CAPACITY: usize = 32;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const ARENA_EVENT_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdversarialArenaSubjectKind {
    #[default]
    TurnStrategy,
}

impl AdversarialArenaSubjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::TurnStrategy => "turn_strategy",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdversarialArenaRole {
    #[default]
    Defender,
    Attacker,
}

impl AdversarialArenaRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Defender => "defender",
            Self::Attacker => "attacker",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdversarialArenaDisposition {
    #[default]
    UpholdDefender,
    ReviseToAttacker,
    HoldForClarification,
}

impl AdversarialArenaDisposition {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpholdDefender => "uphold_defender",
            Self::ReviseToAttacker => "revise_to_attacker",
            Self::HoldForClarification => "hold_for_clarification",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdversarialArenaClaim {
    pub role: AdversarialArenaRole,
    pub label: String,
    pub summary: String,
    pub evidence_score: u8,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub requires_native_tool_round: bool,
}

impl AdversarialArenaClaim {
    pub fn is_meaningful(&self) -> bool {
        !self.label.trim().is_empty()
            || !self.summary.trim().is_empty()
            || self.evidence_score > 0
            || !self.signals.is_empty()
            || self.requires_native_tool_round
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdversarialArenaAdjudication {
    pub subject_kind: AdversarialArenaSubjectKind,
    pub disposition: AdversarialArenaDisposition,
    pub summary: String,
    pub winner: AdversarialArenaClaim,
    pub defender: AdversarialArenaClaim,
    pub attacker: AdversarialArenaClaim,
}

impl AdversarialArenaAdjudication {
    pub fn is_meaningful(&self) -> bool {
        !self.summary.trim().is_empty()
            || self.winner.is_meaningful()
            || self.defender.is_meaningful()
            || self.attacker.is_meaningful()
            || self.disposition != AdversarialArenaDisposition::UpholdDefender
    }

    pub fn winner_requires_native_tool_round(&self) -> bool {
        self.winner.requires_native_tool_round
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AdversarialArenaTimelineEvent {
    pub recorded_at: u64,
    pub subject_kind: String,
    pub disposition: String,
    pub winner: String,
    pub loser: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AdversarialArenaAuditSummary {
    pub total_retained: usize,
    pub upheld: usize,
    pub revised: usize,
    pub held_for_clarification: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct AdversarialArenaAuditSnapshot {
    pub summary: AdversarialArenaAuditSummary,
    pub recent_events: Vec<AdversarialArenaTimelineEvent>,
}

pub fn append_adversarial_arena_event(event: AdversarialArenaTimelineEvent) {
    let mut state = adversarial_arena_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if state.len() >= ARENA_EVENT_CAPACITY {
        state.pop_front();
    }
    state.push_back(event);
}

pub fn adversarial_arena_snapshot(limit: usize) -> AdversarialArenaAuditSnapshot {
    let state = adversarial_arena_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let recent_events = state
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let mut summary = AdversarialArenaAuditSummary {
        total_retained: recent_events.len(),
        ..AdversarialArenaAuditSummary::default()
    };
    for event in &recent_events {
        match event.disposition.as_str() {
            "uphold_defender" => summary.upheld += 1,
            "revise_to_attacker" => summary.revised += 1,
            "hold_for_clarification" => summary.held_for_clarification += 1,
            _ => {}
        }
    }
    AdversarialArenaAuditSnapshot {
        summary,
        recent_events,
    }
}

fn adversarial_arena_state() -> &'static Mutex<VecDeque<AdversarialArenaTimelineEvent>> {
    static STATE: OnceLock<Mutex<VecDeque<AdversarialArenaTimelineEvent>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(VecDeque::with_capacity(ARENA_EVENT_CAPACITY)))
}

pub(crate) fn normalize_arena_claim(
    role: AdversarialArenaRole,
    label: &str,
    summary: &str,
    evidence_score: u8,
    signals: &[String],
    requires_native_tool_round: bool,
) -> AdversarialArenaClaim {
    AdversarialArenaClaim {
        role,
        label: crate::util::truncate_content_to_max(label.trim(), ARENA_LABEL_MAX_CHARS)
            .into_owned(),
        summary: crate::util::truncate_content_to_max(summary.trim(), ARENA_SUMMARY_MAX_CHARS)
            .into_owned(),
        evidence_score,
        signals: signals
            .iter()
            .filter_map(|signal| {
                let trimmed = signal.trim();
                (!trimmed.is_empty()).then(|| {
                    crate::util::truncate_content_to_max(trimmed, ARENA_SIGNAL_MAX_CHARS)
                        .into_owned()
                })
            })
            .take(ARENA_SIGNAL_MAX_ITEMS)
            .collect(),
        requires_native_tool_round,
    }
}

pub(crate) fn build_adversarial_arena_timeline_event(
    adjudication: &AdversarialArenaAdjudication,
    recorded_at: u64,
) -> AdversarialArenaTimelineEvent {
    AdversarialArenaTimelineEvent {
        recorded_at,
        subject_kind: adjudication.subject_kind.label().to_string(),
        disposition: adjudication.disposition.label().to_string(),
        winner: adjudication.winner.label.clone(),
        loser: if adjudication.winner.role == AdversarialArenaRole::Defender {
            adjudication.attacker.label.clone()
        } else {
            adjudication.defender.label.clone()
        },
        detail: crate::util::truncate_content_to_max(
            adjudication.summary.trim(),
            ARENA_TIMELINE_DETAIL_MAX_CHARS,
        )
        .into_owned(),
    }
}

#[cfg(test)]
pub(crate) fn reset_adversarial_arena_for_tests() {
    adversarial_arena_state()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
}

#[cfg(test)]
pub(crate) fn adversarial_arena_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_snapshot_counts_dispositions() {
        let _guard = adversarial_arena_test_guard();
        reset_adversarial_arena_for_tests();
        append_adversarial_arena_event(AdversarialArenaTimelineEvent {
            recorded_at: 10,
            subject_kind: "turn_strategy".to_string(),
            disposition: "uphold_defender".to_string(),
            winner: "direct_reply".to_string(),
            loser: "clarify_before_action".to_string(),
            detail: "upheld defender".to_string(),
        });
        append_adversarial_arena_event(AdversarialArenaTimelineEvent {
            recorded_at: 11,
            subject_kind: "turn_strategy".to_string(),
            disposition: "revise_to_attacker".to_string(),
            winner: "native_tool_round".to_string(),
            loser: "direct_reply".to_string(),
            detail: "revised".to_string(),
        });
        append_adversarial_arena_event(AdversarialArenaTimelineEvent {
            recorded_at: 12,
            subject_kind: "turn_strategy".to_string(),
            disposition: "hold_for_clarification".to_string(),
            winner: "clarify_before_action".to_string(),
            loser: "structured_tool_synthesis".to_string(),
            detail: "held".to_string(),
        });

        let snapshot = adversarial_arena_snapshot(8);
        assert_eq!(snapshot.summary.total_retained, 3);
        assert_eq!(snapshot.summary.upheld, 1);
        assert_eq!(snapshot.summary.revised, 1);
        assert_eq!(snapshot.summary.held_for_clarification, 1);
        assert_eq!(snapshot.recent_events.len(), 3);
    }
}
