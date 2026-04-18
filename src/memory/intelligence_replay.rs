//! Replay-driven intelligence inspection for recent turn-ledger history.

use crate::error::Result;
use serde::{Deserialize, Serialize};

use super::{TurnDeliberationClass, TurnExecutionClass, TurnLedger, TurnLedgerStore};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntelligenceReplayAlertCode {
    SubjectStateCoverageLow,
    HardTurnUnderclassified,
    ToolLoopWithoutPrimaryDelivery,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceReplayAlert {
    pub code: IntelligenceReplayAlertCode,
    pub message: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceReplayTurnDigest {
    pub req_id: String,
    pub outcome: String,
    pub execution_class: String,
    pub deliberation_class: String,
    pub blocker_kind: String,
    pub response_mode: String,
    pub task_scope: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntelligenceReplayInspection {
    pub chat_id: String,
    pub total_turns: usize,
    pub meaningful_turns: usize,
    pub subject_state_coverage_percent: u8,
    pub observation_coverage_percent: u8,
    pub hard_reasoning_percent: u8,
    pub blocker_percent: u8,
    pub current_primary_delivery_percent: u8,
    pub latest_governance_mode: String,
    pub latest_response_mode: String,
    pub latest_task_scope: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<IntelligenceReplayAlert>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_turns: Vec<IntelligenceReplayTurnDigest>,
}

pub fn inspect_intelligence_replay(
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    limit: usize,
) -> Result<IntelligenceReplayInspection> {
    let turns = turn_ledger_store.list_recent(chat_id, limit)?;
    Ok(analyze_intelligence_replay(chat_id, &turns))
}

fn analyze_intelligence_replay(
    chat_id: &str,
    turns: &[TurnLedger],
) -> IntelligenceReplayInspection {
    let total_turns = turns.len();
    let meaningful_turns = turns
        .iter()
        .filter(|ledger| ledger.ingress == crate::bus::IngressKind::User)
        .count();
    let subject_state_turns = turns
        .iter()
        .filter(|ledger| {
            ledger
                .subject_state
                .as_ref()
                .is_some_and(|state| state.is_meaningful())
        })
        .count();
    let observation_turns = turns
        .iter()
        .filter(|ledger| {
            ledger
                .observation
                .as_ref()
                .is_some_and(|observation| observation.is_meaningful())
        })
        .count();
    let hard_reasoning_turns = turns
        .iter()
        .filter(|ledger| {
            ledger.observation.as_ref().is_some_and(|observation| {
                observation.deliberation_class == TurnDeliberationClass::HardReasoning
            })
        })
        .count();
    let blocker_turns = turns
        .iter()
        .filter(|ledger| {
            ledger
                .observation
                .as_ref()
                .is_some_and(|observation| observation.blocker.is_some())
        })
        .count();
    let tool_assisted_turns = turns
        .iter()
        .filter(|ledger| {
            ledger.observation.as_ref().is_some_and(|observation| {
                matches!(
                    observation.execution_class,
                    TurnExecutionClass::ToolAssisted | TurnExecutionClass::TaskExecution
                )
            })
        })
        .count();
    let current_primary_delivery_turns = turns
        .iter()
        .filter(|ledger| {
            ledger
                .observation
                .as_ref()
                .is_some_and(|observation| observation.tool_path.current_primary_delivered)
        })
        .count();
    let latest_subject = turns
        .iter()
        .filter_map(|ledger| ledger.subject_state.as_ref())
        .find(|state| state.is_meaningful());
    let mut alerts = Vec::with_capacity(4);
    if meaningful_turns >= 4 && subject_state_turns * 100 < meaningful_turns * 75 {
        alerts.push(IntelligenceReplayAlert {
            code: IntelligenceReplayAlertCode::SubjectStateCoverageLow,
            message: format!(
                "subject_state coverage is only {}% across recent meaningful turns",
                percent(subject_state_turns, meaningful_turns)
            ),
        });
    }
    if blocker_turns >= 2 && hard_reasoning_turns < blocker_turns {
        alerts.push(IntelligenceReplayAlert {
            code: IntelligenceReplayAlertCode::HardTurnUnderclassified,
            message: format!(
                "blocker turns ({blocker_turns}) exceed hard_reasoning turns ({hard_reasoning_turns})"
            ),
        });
    }
    if tool_assisted_turns >= 3 && blocker_turns >= 2 && current_primary_delivery_turns == 0 {
        alerts.push(IntelligenceReplayAlert {
            code: IntelligenceReplayAlertCode::ToolLoopWithoutPrimaryDelivery,
            message: "tool-assisted turns are accumulating without any current-primary delivery"
                .into(),
        });
    }
    IntelligenceReplayInspection {
        chat_id: chat_id.to_string(),
        total_turns,
        meaningful_turns,
        subject_state_coverage_percent: percent(subject_state_turns, meaningful_turns),
        observation_coverage_percent: percent(observation_turns, total_turns),
        hard_reasoning_percent: percent(hard_reasoning_turns, meaningful_turns),
        blocker_percent: percent(blocker_turns, meaningful_turns),
        current_primary_delivery_percent: percent(current_primary_delivery_turns, meaningful_turns),
        latest_governance_mode: latest_subject
            .map(|state| state.governance_mode.clone())
            .unwrap_or_default(),
        latest_response_mode: latest_subject
            .map(|state| state.response_mode.clone())
            .unwrap_or_default(),
        latest_task_scope: latest_subject
            .map(|state| state.task_scope.clone())
            .unwrap_or_default(),
        alerts,
        recent_turns: turns
            .iter()
            .take(6)
            .map(|ledger| IntelligenceReplayTurnDigest {
                req_id: ledger.req_id.clone(),
                outcome: ledger
                    .observation
                    .as_ref()
                    .map(|observation| observation.final_outcome.clone())
                    .unwrap_or_else(|| ledger.reason.clone()),
                execution_class: ledger
                    .observation
                    .as_ref()
                    .map(|observation| observation.execution_class.label().to_string())
                    .unwrap_or_default(),
                deliberation_class: ledger
                    .observation
                    .as_ref()
                    .map(|observation| observation.deliberation_class.label().to_string())
                    .unwrap_or_default(),
                blocker_kind: ledger
                    .observation
                    .as_ref()
                    .and_then(|observation| observation.blocker.as_ref())
                    .map(|blocker| blocker.kind.clone())
                    .unwrap_or_default(),
                response_mode: ledger
                    .subject_state
                    .as_ref()
                    .map(|state| state.response_mode.clone())
                    .unwrap_or_default(),
                task_scope: ledger
                    .subject_state
                    .as_ref()
                    .map(|state| state.task_scope.clone())
                    .unwrap_or_default(),
            })
            .collect(),
    }
}

fn percent(part: usize, total: usize) -> u8 {
    if total == 0 {
        return 0;
    }
    let value = (part.saturating_mul(100) + (total / 2)) / total;
    value.min(100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::IngressKind;
    use crate::memory::{
        TurnBlockerLedger, TurnDeliberationClass, TurnExecutionClass, TurnLedger, TurnLedgerStatus,
        TurnModeSnapshotLedger, TurnObservationLedger, TurnPersonaPressureLevel,
        TurnSubjectStateLedger, TurnToolPathLedger,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubTurnLedgerStore {
        items: Mutex<Vec<TurnLedger>>,
    }

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<TurnLedger>> {
            let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner()).clone();
            items.truncate(limit);
            Ok(items)
        }
    }

    fn ledger(
        req_id: &str,
        response_mode: Option<&str>,
        task_scope: Option<&str>,
        deliberation_class: TurnDeliberationClass,
        final_outcome: &str,
        blocker_kind: Option<&str>,
        current_primary_delivered: bool,
    ) -> TurnLedger {
        TurnLedger {
            req_id: req_id.to_string(),
            channel: "telegram".to_string(),
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            subject_state: response_mode.map(|mode| TurnSubjectStateLedger {
                governance_mode: "adaptive".to_string(),
                response_mode: mode.to_string(),
                task_scope: task_scope.unwrap_or_default().to_string(),
                ..TurnSubjectStateLedger::default()
            }),
            observation: Some(TurnObservationLedger {
                execution_class: TurnExecutionClass::ToolAssisted,
                deliberation_class,
                final_outcome: final_outcome.to_string(),
                pressure: TurnPersonaPressureLevel::Cautious,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: "surface_finalization".to_string(),
                    tool_calls: 2,
                    react_rounds: 2,
                    current_primary_delivered,
                },
                blocker: blocker_kind.map(|kind| TurnBlockerLedger {
                    kind: kind.to_string(),
                    failed_calls: 1,
                    total_calls: 1,
                }),
            }),
            ..TurnLedger::default()
        }
    }

    #[test]
    fn inspect_intelligence_replay_surfaces_calibration_alerts_and_latest_subject_state() {
        let store = StubTurnLedgerStore {
            items: Mutex::new(vec![
                ledger(
                    "req-4",
                    None,
                    None,
                    TurnDeliberationClass::Standard,
                    "surface_finalization",
                    Some("retryable"),
                    false,
                ),
                ledger(
                    "req-3",
                    Some("protective_brief"),
                    Some("narrow"),
                    TurnDeliberationClass::Standard,
                    "final_answer",
                    Some("retryable"),
                    false,
                ),
                ledger(
                    "req-2",
                    None,
                    None,
                    TurnDeliberationClass::Standard,
                    "surface_finalization",
                    Some("capability"),
                    false,
                ),
                ledger(
                    "req-1",
                    Some("steady"),
                    Some("brief"),
                    TurnDeliberationClass::FastInteractive,
                    "final_answer",
                    None,
                    true,
                ),
            ]),
        };

        let inspection = inspect_intelligence_replay(&store, "chat-1", 8).expect("inspection");

        assert_eq!(inspection.total_turns, 4);
        assert_eq!(inspection.meaningful_turns, 4);
        assert_eq!(inspection.latest_response_mode, "protective_brief");
        assert_eq!(inspection.latest_task_scope, "narrow");
        assert_eq!(inspection.latest_governance_mode, "adaptive");
        assert_eq!(inspection.recent_turns[0].req_id, "req-4");
        assert!(inspection
            .alerts
            .iter()
            .any(|alert| { alert.code == IntelligenceReplayAlertCode::SubjectStateCoverageLow }));
        assert!(inspection
            .alerts
            .iter()
            .any(|alert| { alert.code == IntelligenceReplayAlertCode::HardTurnUnderclassified }));
    }
}
