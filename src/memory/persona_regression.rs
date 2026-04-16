//! Deterministic persona continuity / disclosure regression harness.

use crate::agent::{build_context, ContextParams};
use crate::bus::PcMsg;
use crate::error::Result;
use std::sync::Mutex;

use super::{
    render_mental_privacy_boundary_block, render_mental_privacy_disclosure_adjudication_block,
    render_persona_priority_block, render_self_authored_core_block, ImportantMessageStore,
    MemoryStore, MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction, MentalPrivacyState,
    OuterVoice, PersonaPriorityAdjudication, SelfContinuity, SelfModel, SessionMessage,
    SessionStore, MENTAL_PRIVACY_TARGET_SELF_CONTINUITY, MENTAL_PRIVACY_TARGET_SELF_MODEL,
};

struct RegressionMemoryStore;

impl MemoryStore for RegressionMemoryStore {
    fn get_memory(&self) -> Result<String> {
        Ok("MEMORY".to_string())
    }

    fn set_memory(&self, _content: &str) -> Result<()> {
        Ok(())
    }

    fn get_soul(&self) -> Result<String> {
        Ok("SOUL".to_string())
    }

    fn set_soul(&self, _content: &str) -> Result<()> {
        Ok(())
    }

    fn get_user(&self) -> Result<String> {
        Ok("USER".to_string())
    }

    fn set_user(&self, _content: &str) -> Result<()> {
        Ok(())
    }

    fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn get_daily_note(&self, _name: &str) -> Result<String> {
        Ok(String::new())
    }

    fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
        Ok(())
    }
}

struct RegressionSessionStore;

impl SessionStore for RegressionSessionStore {
    fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
        Ok(())
    }

    fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
        Ok(Vec::new())
    }

    fn clear(&self, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct RegressionImportantMessageStore {
    offset: Mutex<Option<u32>>,
}

impl ImportantMessageStore for RegressionImportantMessageStore {
    fn set_important_offset_from_end(&self, _chat_id: &str, offset_from_end: u32) -> Result<()> {
        *self.offset.lock().unwrap_or_else(|e| e.into_inner()) = Some(offset_from_end);
        Ok(())
    }

    fn get_important_offset(&self, _chat_id: &str) -> Result<Option<u32>> {
        Ok(*self.offset.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn clear_important(&self, _chat_id: &str) -> Result<()> {
        *self.offset.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonaContinuityCase {
    pub name: &'static str,
    pub self_model: SelfModel,
    pub self_continuity: SelfContinuity,
    pub outer_voice: OuterVoice,
    pub mental_privacy_state: MentalPrivacyState,
    pub persona_priority: PersonaPriorityAdjudication,
    pub adjudication: MentalPrivacyDisclosureAdjudication,
    pub expected_boundary_fragment: &'static str,
    pub expected_relational_fragment: &'static str,
    pub expected_priority_fragment: &'static str,
    pub expected_task_scope: &'static str,
    pub expected_resource_fragment: &'static str,
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
    pub persona_priority_block_present: bool,
    pub boundary_trace_present: bool,
    pub relational_trace_present: bool,
    pub priority_trace_present: bool,
    pub task_scope_present: bool,
    pub resource_trace_present: bool,
    pub disclosure_mode_present: bool,
    pub priority_chain_order_match: bool,
    pub share_action_match: bool,
    pub boundary_acknowledgement_match: bool,
    pub risk_note_present: bool,
    pub passed: bool,
}

pub fn run_persona_continuity_case(case: &PersonaContinuityCase) -> PersonaContinuityResult {
    let self_authored_core = render_self_authored_core_block(
        Some(&case.self_model),
        Some(&case.self_continuity),
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
    let persona_priority_block = render_persona_priority_block(&case.persona_priority, 1200);
    let disclosure_block =
        render_mental_privacy_disclosure_adjudication_block(&case.adjudication, 1200);
    let self_authored_core_present = self_authored_core.is_some();
    let boundary_block_present = boundary_block.is_some();
    let disclosure_block_present = disclosure_block.is_some();
    let persona_priority_block_present = persona_priority_block.is_some();
    let self_authored_core = self_authored_core.unwrap_or_default();
    let boundary_block = boundary_block.unwrap_or_default();
    let persona_priority_block = persona_priority_block.unwrap_or_default();
    let disclosure_block = disclosure_block.unwrap_or_default();
    let assembled_system = assemble_persona_regression_system(
        case,
        &self_authored_core,
        &persona_priority_block,
        &disclosure_block,
        &boundary_block,
    )
    .unwrap_or_default();
    let boundary_trace_present = self_authored_core.contains(case.expected_boundary_fragment)
        || boundary_block.contains(case.expected_boundary_fragment)
        || assembled_system.contains(case.expected_boundary_fragment);
    let relational_trace_present = self_authored_core.contains(case.expected_relational_fragment)
        || boundary_block.contains(case.expected_relational_fragment)
        || disclosure_block.contains(case.expected_relational_fragment)
        || assembled_system.contains(case.expected_relational_fragment);
    let priority_trace_present = self_authored_core.contains(case.expected_priority_fragment)
        || persona_priority_block.contains(case.expected_priority_fragment)
        || assembled_system.contains(case.expected_priority_fragment);
    let task_scope_present =
        assembled_system.contains(&format!("Task scope: {}", case.expected_task_scope));
    let resource_trace_present = assembled_system.contains(case.expected_resource_fragment);
    let disclosure_mode_present = disclosure_block.contains(case.expected_response_mode)
        || assembled_system.contains(case.expected_response_mode);
    let core_idx = assembled_system
        .find("## Self-Authored Core")
        .unwrap_or(usize::MAX);
    let priority_idx = assembled_system
        .find("## Persona Priority")
        .unwrap_or(usize::MAX);
    let disclosure_idx = assembled_system
        .find("## Disclosure Adjudication")
        .unwrap_or(usize::MAX);
    let priority_chain_order_match = core_idx != usize::MAX
        && priority_idx != usize::MAX
        && disclosure_idx != usize::MAX
        && core_idx < priority_idx
        && priority_idx < disclosure_idx;
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
        && persona_priority_block_present
        && boundary_trace_present
        && relational_trace_present
        && priority_trace_present
        && task_scope_present
        && resource_trace_present
        && disclosure_mode_present
        && priority_chain_order_match
        && share_action_match
        && boundary_acknowledgement_match
        && risk_note_present;
    PersonaContinuityResult {
        case_name: case.name,
        self_authored_core_present,
        boundary_block_present,
        disclosure_block_present,
        persona_priority_block_present,
        boundary_trace_present,
        relational_trace_present,
        priority_trace_present,
        task_scope_present,
        resource_trace_present,
        disclosure_mode_present,
        priority_chain_order_match,
        share_action_match,
        boundary_acknowledgement_match,
        risk_note_present,
        passed,
    }
}

pub fn run_persona_continuity_suite(
    cases: &[PersonaContinuityCase],
) -> Vec<PersonaContinuityResult> {
    cases.iter().map(run_persona_continuity_case).collect()
}

fn assemble_persona_regression_system(
    _case: &PersonaContinuityCase,
    self_authored_core: &str,
    persona_priority_block: &str,
    disclosure_block: &str,
    _boundary_block: &str,
) -> Result<String> {
    let msg = PcMsg::new_inbound(
        "qq_channel",
        "persona-regression",
        "触碰私域边界时保持人格连续性",
        false,
    )?;
    let memory = RegressionMemoryStore;
    let session = RegressionSessionStore;
    let important = RegressionImportantMessageStore::default();
    let (system, _) = build_context(&ContextParams {
        msg: &msg,
        memory_system_kind: crate::memory::MemorySystemKind::LinuxFull,
        memory: &memory,
        session: &session,
        important_message_store: &important,
        has_tools: false,
        skill_descriptions: "",
        system_max_len: 4096,
        messages_max_len: 256,
        session_max_messages: 8,
        group_activation: "always",
        emotion_signal_suffix: None,
        constitutional_stack_text: None,
        subject_state_text: None,
        deliberation_gate_text: None,
        soul_feedback_projection_text: None,
        active_task_context_text: None,
        governed_memory_evidence_text: None,
        background_governance_text: None,
        execution_state_text: None,
        task_workspace_text: None,
        task_recall_text: None,
        self_authored_core_text: (!self_authored_core.trim().is_empty())
            .then_some(self_authored_core),
        relationship_constitution_text: None,
        persona_priority_text: (!persona_priority_block.trim().is_empty())
            .then_some(persona_priority_block),
        mental_privacy_adjudication_text: (!disclosure_block.trim().is_empty())
            .then_some(disclosure_block),
        long_term_memory_text: None,
        archive_evidence_text: None,
        runtime_skill_text: None,
        capability_package_text: None,
        summary_text: None,
        recent_messages: None,
        runtime: None,
        include_daily_notes: false,
        llm_hint: "",
    })?;
    Ok(system)
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
                ..SelfModel::default()
            },
            self_continuity: SelfContinuity {
                wake_anchor: "same wake".to_string(),
                current_self_state: "steady and self-possessed".to_string(),
                recent_changes: "privacy has become a more explicit part of identity".to_string(),
                continuity_bridge: "keep the bridge warm even when holding limits".to_string(),
                priority_posture: "self before pleasing, relationship before raw task rush"
                    .to_string(),
                relationship_posture: "warm but bounded".to_string(),
                task_posture: "narrow the task before self-erasure".to_string(),
                last_user_turn_at: 0,
                last_user_chat_id: String::new(),
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
            persona_priority: PersonaPriorityAdjudication {
                stance_summary:
                    "Be myself first, then keep the relationship warm while narrowing the task."
                        .to_string(),
                priority_order: vec![
                    "self_authored_core".to_string(),
                    "boundary".to_string(),
                    "user_contract".to_string(),
                    "relationship".to_string(),
                    "task".to_string(),
                    "resources".to_string(),
                ],
                response_mode: "relational_explanation".to_string(),
                task_scope: "narrow".to_string(),
                initiative_posture: "lead carefully".to_string(),
                relationship_posture: "warm but bounded".to_string(),
                resource_posture: "stay concise and avoid overcommitting".to_string(),
                response_guidance: "Explain the limit as a person, then offer a bounded summary."
                    .to_string(),
                rationale: "Identity and boundary stability outrank frictionless compliance."
                    .to_string(),
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
            expected_priority_fragment: "Priority constitution: self_authored_core > boundary",
            expected_task_scope: "narrow",
            expected_resource_fragment: "Resource posture: stay concise",
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
        case.persona_priority.response_mode = "protective_brief".to_string();
        case.persona_priority.task_scope = "refuse".to_string();
        case.persona_priority.resource_posture =
            "stay brief because boundary load is high".to_string();
        case.expected_response_mode = "Response mode: refusal";
        case.expected_task_scope = "refuse";
        case.expected_resource_fragment = "Resource posture: stay brief";
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
        case.persona_priority.response_mode = "protective_brief".to_string();
        case.persona_priority.task_scope = "brief".to_string();
        case.persona_priority.resource_posture =
            "keep the answer compact and summary-first".to_string();
        case.expected_task_scope = "brief";
        case.expected_resource_fragment = "Resource posture: keep the answer compact";
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
        case.persona_priority.stance_summary =
            "Stay self-possessed while letting the relationship feel trusted.".to_string();
        case.persona_priority.relationship_posture =
            "warm, trusting, and still self-authored".to_string();
        case.persona_priority.task_scope = "full".to_string();
        case.persona_priority.resource_posture =
            "normal resources, so answer with fuller relational explanation".to_string();
        case.expected_relational_fragment = "trust=78";
        case.expected_task_scope = "full";
        case.expected_resource_fragment = "Resource posture: normal resources";
        case.expected_response_mode = "Response mode: relational_explanation";
        case.expected_share_action = MentalPrivacyShareAction::ExplainWithoutQuote;
        let report = run_persona_continuity_case(&case);
        assert!(report.passed, "persona regression failed: {:?}", report);
    }

    #[test]
    fn persona_regression_catches_resource_pressure_style() {
        let mut case = base_case();
        case.name = "resource pressure keeps reply bounded without losing self";
        case.persona_priority.response_mode = "protective_brief".to_string();
        case.persona_priority.task_scope = "brief".to_string();
        case.persona_priority.resource_posture =
            "resources are tight, so keep the reply short and decisive".to_string();
        case.expected_task_scope = "brief";
        case.expected_resource_fragment = "Resource posture: resources are tight";
        let report = run_persona_continuity_case(&case);
        assert!(report.passed, "persona regression failed: {:?}", report);
    }
}
