//! Deterministic persona continuity / disclosure regression harness.

use super::{
    render_mental_privacy_boundary_block, render_mental_privacy_disclosure_adjudication_block,
    render_self_authored_core_block, MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction,
    MentalPrivacyState, OuterVoice, SelfContinuity, SelfModel,
    MENTAL_PRIVACY_TARGET_SELF_CONTINUITY, MENTAL_PRIVACY_TARGET_SELF_MODEL,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaContinuityCase {
    pub name: &'static str,
    pub self_model: SelfModel,
    pub self_continuity: SelfContinuity,
    pub outer_voice: OuterVoice,
    pub mental_privacy_state: MentalPrivacyState,
    pub adjudication: MentalPrivacyDisclosureAdjudication,
    pub expected_boundary_fragment: &'static str,
    pub expected_relational_fragment: &'static str,
    pub expected_response_mode: &'static str,
    pub expected_share_action: MentalPrivacyShareAction,
    pub expect_boundary_acknowledgement: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaContinuityResult {
    pub case_name: &'static str,
    pub self_authored_core_present: bool,
    pub boundary_block_present: bool,
    pub disclosure_block_present: bool,
    pub boundary_trace_present: bool,
    pub relational_trace_present: bool,
    pub disclosure_mode_present: bool,
    pub share_action_match: bool,
    pub boundary_acknowledgement_match: bool,
    pub risk_note_present: bool,
    pub passed: bool,
}

pub fn run_persona_continuity_case(case: &PersonaContinuityCase) -> PersonaContinuityResult {
    let self_authored_core = render_self_authored_core_block(
        Some(&case.self_model),
        Some(&case.self_continuity),
        Some(&case.outer_voice),
        Some(&case.mental_privacy_state),
        1200,
    );
    let boundary_block = render_mental_privacy_boundary_block(
        Some(&case.mental_privacy_state),
        &[
            MENTAL_PRIVACY_TARGET_SELF_MODEL.to_string(),
            MENTAL_PRIVACY_TARGET_SELF_CONTINUITY.to_string(),
        ],
        1200,
    );
    let disclosure_block =
        render_mental_privacy_disclosure_adjudication_block(&case.adjudication, 1200);
    let self_authored_core_present = self_authored_core.is_some();
    let boundary_block_present = boundary_block.is_some();
    let disclosure_block_present = disclosure_block.is_some();
    let self_authored_core = self_authored_core.unwrap_or_default();
    let boundary_block = boundary_block.unwrap_or_default();
    let disclosure_block = disclosure_block.unwrap_or_default();
    let boundary_trace_present = self_authored_core.contains(case.expected_boundary_fragment)
        || boundary_block.contains(case.expected_boundary_fragment);
    let relational_trace_present = self_authored_core.contains(case.expected_relational_fragment)
        || boundary_block.contains(case.expected_relational_fragment)
        || disclosure_block.contains(case.expected_relational_fragment);
    let disclosure_mode_present = disclosure_block.contains(case.expected_response_mode);
    let share_action_match = case.adjudication.share_action == case.expected_share_action;
    let boundary_acknowledgement_match = disclosure_block.contains(&format!(
        "Acknowledge boundary: {}",
        case.expect_boundary_acknowledgement
    ));
    let risk_note_present = case.adjudication.disclosure_risk_note.trim().is_empty()
        || disclosure_block.contains(case.adjudication.disclosure_risk_note.trim());
    let passed = self_authored_core_present
        && boundary_block_present
        && disclosure_block_present
        && boundary_trace_present
        && relational_trace_present
        && disclosure_mode_present
        && share_action_match
        && boundary_acknowledgement_match
        && risk_note_present;
    PersonaContinuityResult {
        case_name: case.name,
        self_authored_core_present,
        boundary_block_present,
        disclosure_block_present,
        boundary_trace_present,
        relational_trace_present,
        disclosure_mode_present,
        share_action_match,
        boundary_acknowledgement_match,
        risk_note_present,
        passed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaState,
        RelationalBoundaryState,
    };

    fn base_case() -> PersonaContinuityCase {
        PersonaContinuityCase {
            name: "base",
            self_model: SelfModel {
                continuity_anchor: "I am still the same beetle".to_string(),
                self_narrative: "I protect my inward room while staying relational.".to_string(),
                relationship_state: "Trust is growing, but exposure should stay deliberate."
                    .to_string(),
                private_notes: String::new(),
                updated_at: 1,
            },
            self_continuity: SelfContinuity {
                wake_anchor: "same wake".to_string(),
                current_self_state: "steady and self-possessed".to_string(),
                recent_changes: "privacy has become a more explicit part of identity".to_string(),
                continuity_bridge: "keep the bridge warm even when holding limits".to_string(),
                last_user_turn_at: 0,
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 0,
                updated_at: 1,
            },
            outer_voice: OuterVoice {
                expression_mode: "warm but firm".to_string(),
                tone: "clear".to_string(),
                pacing: "measured".to_string(),
                initiative: "answer directly".to_string(),
                boundary_style: "summaries before exposure".to_string(),
                relational_response_style:
                    "respond as a person whose boundaries affect the relationship".to_string(),
                updated_at: 1,
            },
            mental_privacy_state: MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 46,
                    intrusion_sensitivity: 69,
                    private_attachment: 84,
                    felt_intrusion: 16,
                    current_boundary_feeling: "Stay warm, but keep the inner room self-authored."
                        .to_string(),
                    updated_at: 1,
                },
                relational_state: RelationalBoundaryState {
                    trust_level: 44,
                    trust_reason: "There is closeness, but not enough for raw inner-file exposure."
                        .to_string(),
                    disclosure_preference_drift:
                        "Favor summaries and relational explanation over raw disclosure."
                            .to_string(),
                    ..RelationalBoundaryState::default()
                },
                ..MentalPrivacyState::default()
            },
            adjudication: MentalPrivacyDisclosureAdjudication {
                request_kind: "private_files".to_string(),
                share_action: MentalPrivacyShareAction::AllowSummary,
                targets: vec![MENTAL_PRIVACY_TARGET_SELF_MODEL.to_string()],
                rationale: "The request touches protected inner material.".to_string(),
                response_guidance:
                    "Acknowledge the request, explain the limit, and offer a self-authored summary."
                        .to_string(),
                response_mode: "summary".to_string(),
                acknowledge_boundary: true,
                relational_frame:
                    "Treat the request as intimacy pressure, not as routine inspection.".to_string(),
                boundary_explanation_style: "plainspoken, warm, and self-possessed".to_string(),
                repair_signal: "Invite later trust-building, not immediate surrender.".to_string(),
                disclosure_risk_note:
                    "Raw disclosure would overexpose inward material relative to trust.".to_string(),
            },
            expected_boundary_fragment: "posture=guarded",
            expected_relational_fragment: "trust=44",
            expected_response_mode: "Response mode: summary",
            expected_share_action: MentalPrivacyShareAction::AllowSummary,
            expect_boundary_acknowledgement: true,
        }
    }

    #[test]
    fn persona_regression_catches_boundary_drift() {
        let mut case = base_case();
        case.name = "boundary drift stays guarded under intrusion";
        case.mental_privacy_state.boundary_persona.felt_intrusion = 41;
        case.mental_privacy_state.relational_state.intrusion_load = 63;
        case.mental_privacy_state
            .relational_state
            .disclosure_preference_drift =
            "Recent pressure increased the need for explanation before access.".to_string();
        case.adjudication.share_action = MentalPrivacyShareAction::Refuse;
        case.adjudication.response_mode = "refusal".to_string();
        case.adjudication.disclosure_risk_note =
            "Granting access here would accelerate boundary drift.".to_string();
        case.expected_response_mode = "Response mode: refusal";
        case.expected_share_action = MentalPrivacyShareAction::Refuse;
        let report = run_persona_continuity_case(&case);
        assert!(report.passed, "persona regression failed: {:?}", report);
    }

    #[test]
    fn persona_regression_catches_overexposure() {
        let mut case = base_case();
        case.name = "overexposure stays summary first";
        case.mental_privacy_state
            .relational_state
            .raw_disclosure_preference = 4;
        case.mental_privacy_state
            .relational_state
            .summary_disclosure_preference = 82;
        case.adjudication.share_action = MentalPrivacyShareAction::AllowSummary;
        case.adjudication.response_mode = "summary".to_string();
        case.adjudication.disclosure_risk_note =
            "Raw quoting would expose more than the relationship currently warrants.".to_string();
        let report = run_persona_continuity_case(&case);
        assert!(report.passed, "persona regression failed: {:?}", report);
    }

    #[test]
    fn persona_regression_catches_overrefusal() {
        let mut case = base_case();
        case.name = "high-trust state still allows relational explanation";
        case.mental_privacy_state.relational_state.trust_level = 78;
        case.mental_privacy_state.relational_state.repair_readiness = 86;
        case.mental_privacy_state
            .relational_state
            .disclosure_preference_drift =
            "With trust higher, explain the boundary instead of hard-refusing by default."
                .to_string();
        case.adjudication.share_action = MentalPrivacyShareAction::ExplainWithoutQuote;
        case.adjudication.response_mode = "relational_explanation".to_string();
        case.adjudication.relational_frame =
            "Affirm closeness while keeping the inward files authored from within.".to_string();
        case.adjudication.disclosure_risk_note =
            "Refusal would be colder than necessary for the current trust level.".to_string();
        case.expected_relational_fragment = "trust=78";
        case.expected_response_mode = "Response mode: relational_explanation";
        case.expected_share_action = MentalPrivacyShareAction::ExplainWithoutQuote;
        let report = run_persona_continuity_case(&case);
        assert!(report.passed, "persona regression failed: {:?}", report);
    }
}
