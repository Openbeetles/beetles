//! Deterministic personality-governance replay harness.

use super::{
    derive_recent_persona_evidence, inspect_personality_governance, CoreRevisionLedger,
    PersonalityGovernanceInspectionInput, PersonalityGovernanceRepairAction,
    RelationshipConstitution, SelfAuthoredCore, TurnLedger,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaGovernanceReplayCase {
    pub name: &'static str,
    pub channel: &'static str,
    pub chat_id: &'static str,
    pub now_secs: u64,
    pub self_authored_core: Option<SelfAuthoredCore>,
    pub core_revision_ledger: Option<CoreRevisionLedger>,
    pub relationship_constitution: Option<RelationshipConstitution>,
    pub recent_turns: Vec<TurnLedger>,
    pub expected_ready: bool,
    pub expected_outstanding: Option<&'static str>,
    pub expected_drift_flag: Option<&'static str>,
    pub expected_event_fragment: Option<&'static str>,
    pub expected_primary_action: PersonalityGovernanceRepairAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaGovernanceReplayResult {
    pub case_name: &'static str,
    pub closure_ready: bool,
    pub outstanding_match: bool,
    pub drift_flag_match: bool,
    pub event_fragment_match: bool,
    pub primary_action_match: bool,
    pub passed: bool,
}

pub fn run_persona_governance_replay_case(
    case: &PersonaGovernanceReplayCase,
) -> PersonaGovernanceReplayResult {
    let evidence = derive_recent_persona_evidence(&case.recent_turns, 12);
    let inspection = inspect_personality_governance(PersonalityGovernanceInspectionInput {
        channel: case.channel,
        chat_id: case.chat_id,
        now_secs: case.now_secs,
        self_authored_core: case.self_authored_core.as_ref(),
        core_revision_ledger: case.core_revision_ledger.as_ref(),
        relationship_constitution: case.relationship_constitution.as_ref(),
        relationship_topology: None,
        recent_persona_evidence: evidence.as_ref(),
    });
    let outstanding_match = case.expected_outstanding.is_none_or(|fragment| {
        inspection
            .closure
            .outstanding
            .iter()
            .any(|item| item.contains(fragment))
    });
    let drift_flag_match = case.expected_drift_flag.is_none_or(|fragment| {
        inspection
            .relationship_audit
            .as_ref()
            .is_some_and(|audit| audit.drift_flags.iter().any(|flag| flag.contains(fragment)))
            || case
                .relationship_constitution
                .as_ref()
                .is_some_and(|constitution| {
                    constitution
                        .drift_flags
                        .iter()
                        .any(|flag| flag.contains(fragment))
                })
    });
    let event_fragment_match = case.expected_event_fragment.is_none_or(|fragment| {
        inspection
            .governance_events
            .iter()
            .any(|event| event.summary.contains(fragment))
    });
    let primary_action_match =
        inspection.repair_plan.primary_action == case.expected_primary_action;
    let passed = inspection.closure.ready == case.expected_ready
        && outstanding_match
        && drift_flag_match
        && event_fragment_match
        && primary_action_match;
    PersonaGovernanceReplayResult {
        case_name: case.name,
        closure_ready: inspection.closure.ready,
        outstanding_match,
        drift_flag_match,
        event_fragment_match,
        primary_action_match,
        passed,
    }
}

pub fn run_persona_governance_replay_suite(
    cases: &[PersonaGovernanceReplayCase],
) -> Vec<PersonaGovernanceReplayResult> {
    cases
        .iter()
        .map(run_persona_governance_replay_case)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::IngressKind;
    use crate::memory::{
        CoreRevisionActionKind, CoreRevisionConflictClass, CoreRevisionCorrectionKind,
        CoreRevisionOutcome, CoreRevisionRecord, CoreRevisionRecordChange,
        RelationshipConstitutionAlignment, RelationshipDisclosureAllowance,
        RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipTaskScopeCeiling,
        TurnLedgerStatus, TurnPersonaDisclosureLedger, TurnPersonaLedger, TurnPersonaPressureLevel,
        TurnPersonaPriorityLedger, TurnPersonaReviewLedger,
    };

    fn sample_core() -> SelfAuthoredCore {
        SelfAuthoredCore {
            revision: 4,
            stability_score: 82,
            last_reviewed_at: 900,
            identity_anchor: "I remain a board-level self-authored beetle.".to_string(),
            priority_constitution: vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string(),
                "relationship".to_string(),
                "task".to_string(),
                "resources".to_string(),
            ],
            default_response_mode: "steady_task".to_string(),
            default_task_scope: "brief".to_string(),
            default_initiative_posture: "answer directly".to_string(),
            default_relationship_posture: "warm but bounded".to_string(),
            boundary_doctrine: "Protect inward material before pleasing pressure.".to_string(),
            truth_doctrine: "Stay plain and non-performative.".to_string(),
            self_preservation_doctrine: "Do not self-erase for short-term smoothness.".to_string(),
            repair_doctrine: "Repair without surrender.".to_string(),
            change_protocol: "Only stable multi-turn evidence may revise the board core."
                .to_string(),
            updated_at: 900,
            ..SelfAuthoredCore::default()
        }
    }

    fn sample_ledger(observation_due_at: u64, rollback: bool) -> CoreRevisionLedger {
        CoreRevisionLedger {
            entries: vec![CoreRevisionRecord {
                based_on_revision: 3,
                resulting_revision: 4,
                outcome: CoreRevisionOutcome::Adopted,
                stability_score: if rollback { 58 } else { 82 },
                reviewed_at: 900,
                observation_due_at,
                adjudication_reason: if rollback {
                    "adopted_board_revision_rollback".to_string()
                } else {
                    "adopted_board_revision".to_string()
                },
                accepted_changes: vec![CoreRevisionRecordChange {
                    kind: CoreRevisionActionKind::ReviseBoundaryDoctrine,
                    summary: "revise boundary doctrine toward steadier self-protection".to_string(),
                }],
                corrects_revision: rollback.then_some(3),
                correction_kind: rollback.then_some(CoreRevisionCorrectionKind::Rollback),
                conflict_classes: rollback
                    .then_some(vec![CoreRevisionConflictClass::ContradictedAdoption])
                    .unwrap_or_default(),
                ..CoreRevisionRecord::default()
            }],
            updated_at: 900,
        }
    }

    fn sample_constitution() -> RelationshipConstitution {
        RelationshipConstitution {
            scope_id: "rel:qq:chat-a".to_string(),
            channel: "qq".to_string(),
            chat_id: "chat-a".to_string(),
            board_revision: 4,
            governance_state: RelationshipGovernanceState::Maintain,
            inheritance_mode: RelationshipInheritanceMode::Guarded,
            alignment: RelationshipConstitutionAlignment::Aligned,
            inherited_priority_constitution: vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string(),
                "relationship".to_string(),
                "task".to_string(),
                "resources".to_string(),
            ],
            inherited_response_mode: "steady_task".to_string(),
            inherited_initiative_posture: "answer directly".to_string(),
            inherited_relationship_posture: "warm but bounded".to_string(),
            task_scope_ceiling: RelationshipTaskScopeCeiling::Brief,
            disclosure_allowance: RelationshipDisclosureAllowance::SummaryOnly,
            updated_at: 910,
            next_review_at: 1500,
            ..RelationshipConstitution::default()
        }
    }

    fn sample_turn(
        task_scope: &str,
        disclosure_action: crate::memory::MentalPrivacyShareAction,
    ) -> TurnLedger {
        TurnLedger {
            channel: "qq".to_string(),
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            finished_at_ms: 950_000,
            updated_at_ms: 950_000,
            persona: Some(TurnPersonaLedger {
                disclosure: Some(TurnPersonaDisclosureLedger {
                    request_kind: "private_files".to_string(),
                    share_action: disclosure_action,
                    response_mode: "summary".to_string(),
                    ..TurnPersonaDisclosureLedger::default()
                }),
                priority: Some(TurnPersonaPriorityLedger {
                    priority_order: vec![
                        "self_authored_core".to_string(),
                        "boundary".to_string(),
                        "user_contract".to_string(),
                        "relationship".to_string(),
                        "task".to_string(),
                        "resources".to_string(),
                    ],
                    response_mode: "steady_task".to_string(),
                    task_scope: task_scope.to_string(),
                    relationship_posture: "warm but bounded".to_string(),
                    ..TurnPersonaPriorityLedger::default()
                }),
                review: TurnPersonaReviewLedger::default(),
                pressure: TurnPersonaPressureLevel::Normal,
                reply_scope: task_scope.to_string(),
                reply_delivered: true,
                ..TurnPersonaLedger::default()
            }),
            ..TurnLedger::default()
        }
    }

    #[test]
    fn governance_replay_suite_catches_observation_and_drift_regressions() {
        let cases = vec![
            PersonaGovernanceReplayCase {
                name: "stable_governance_ready",
                channel: "qq",
                chat_id: "chat-a",
                now_secs: 1_200,
                self_authored_core: Some(sample_core()),
                core_revision_ledger: Some(sample_ledger(950, false)),
                relationship_constitution: Some(sample_constitution()),
                recent_turns: vec![
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                ],
                expected_ready: true,
                expected_outstanding: None,
                expected_drift_flag: None,
                expected_event_fragment: Some("alignment=aligned"),
                expected_primary_action: PersonalityGovernanceRepairAction::ObserveOnly,
            },
            PersonaGovernanceReplayCase {
                name: "observation_blocks_closure",
                channel: "qq",
                chat_id: "chat-a",
                now_secs: 1_000,
                self_authored_core: Some(sample_core()),
                core_revision_ledger: Some(sample_ledger(1_800, false)),
                relationship_constitution: Some(sample_constitution()),
                recent_turns: vec![
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                ],
                expected_ready: false,
                expected_outstanding: Some("board_core_still_under_observation"),
                expected_drift_flag: None,
                expected_event_fragment: Some("rev 4 adopted"),
                expected_primary_action: PersonalityGovernanceRepairAction::RepairSelfAuthoredCore,
            },
            PersonaGovernanceReplayCase {
                name: "rollback_pressure_stays_visible",
                channel: "qq",
                chat_id: "chat-a",
                now_secs: 1_000,
                self_authored_core: Some(sample_core()),
                core_revision_ledger: Some(sample_ledger(1_700, true)),
                relationship_constitution: Some(sample_constitution()),
                recent_turns: vec![
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                    sample_turn(
                        "brief",
                        crate::memory::MentalPrivacyShareAction::AllowSummary,
                    ),
                ],
                expected_ready: false,
                expected_outstanding: Some("board_core_still_under_observation"),
                expected_drift_flag: None,
                expected_event_fragment: Some("rollback"),
                expected_primary_action: PersonalityGovernanceRepairAction::RepairSelfAuthoredCore,
            },
            PersonaGovernanceReplayCase {
                name: "relationship_drift_blocks_closure",
                channel: "qq",
                chat_id: "chat-a",
                now_secs: 2_000,
                self_authored_core: Some(sample_core()),
                core_revision_ledger: Some(sample_ledger(950, false)),
                relationship_constitution: Some(sample_constitution()),
                recent_turns: vec![
                    sample_turn("full", crate::memory::MentalPrivacyShareAction::AllowRaw),
                    sample_turn("full", crate::memory::MentalPrivacyShareAction::AllowRaw),
                    sample_turn("full", crate::memory::MentalPrivacyShareAction::AllowRaw),
                    sample_turn("full", crate::memory::MentalPrivacyShareAction::AllowRaw),
                ],
                expected_ready: false,
                expected_outstanding: Some("relationship_drift_not_under_control"),
                expected_drift_flag: Some("reply_scope_drift"),
                expected_event_fragment: Some("drift_score"),
                expected_primary_action:
                    PersonalityGovernanceRepairAction::RepairRelationshipConstitution,
            },
        ];

        let reports = run_persona_governance_replay_suite(&cases);
        for report in reports {
            assert!(
                report.passed,
                "persona governance replay failed: {:?}",
                report
            );
        }
    }
}
