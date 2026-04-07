//! Deterministic subject-state compiler for the current reply turn.
//! 当前回合的确定性主体状态编译器。

use crate::memory::{
    normalize_turn_subject_state_summary, normalize_turn_subject_state_text,
    MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction, PersonaPriorityAdjudication,
    PersonalityRuntimeGovernanceGate, RelationshipConstitution, SelfAuthoredCore,
    TurnSubjectStateLedger,
};
use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;
use std::fmt::Write as _;

const SUBJECT_STATE_RENDER_MIN_LEN: usize = 96;
const SUBJECT_STATE_RENDER_FIELD_MAX_CHARS: usize = 72;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SubjectState {
    pub identity_anchor: String,
    pub governance_mode: String,
    pub relationship_state: String,
    pub response_mode: String,
    pub task_scope: String,
    pub initiative_posture: String,
    pub relationship_posture: String,
    pub resource_posture: String,
    pub boundary_mode: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SubjectStateCompileInput<'a> {
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub relationship_constitution: Option<&'a RelationshipConstitution>,
    pub persona_priority: Option<&'a PersonaPriorityAdjudication>,
    pub disclosure_adjudication: Option<&'a MentalPrivacyDisclosureAdjudication>,
    pub personality_governance_gate: Option<&'a PersonalityRuntimeGovernanceGate>,
    pub pressure: PressureLevel,
}

pub(crate) fn compile_subject_state(input: SubjectStateCompileInput<'_>) -> Option<SubjectState> {
    let identity_anchor = input
        .self_authored_core
        .map(|core| normalize_field(&core.identity_anchor))
        .unwrap_or_default();
    let governance_mode = input
        .personality_governance_gate
        .map(|gate| {
            if gate.conservative_reply {
                "conservative".to_string()
            } else if !gate.allow_dynamic_persona_priority {
                "fixed_persona".to_string()
            } else {
                "adaptive".to_string()
            }
        })
        .unwrap_or_else(|| "adaptive".to_string());
    let relationship_state = input
        .relationship_constitution
        .map(|constitution| {
            normalize_field(&format!(
                "{}/{}",
                constitution.governance_state.label(),
                constitution.alignment.label()
            ))
        })
        .unwrap_or_default();
    let response_mode = first_non_empty(&[
        input
            .persona_priority
            .map(|priority| priority.response_mode.as_str()),
        input
            .disclosure_adjudication
            .map(|adjudication| adjudication.response_mode.as_str()),
        input
            .relationship_constitution
            .map(|constitution| constitution.inherited_response_mode.as_str()),
        input
            .self_authored_core
            .map(|core| core.default_response_mode.as_str()),
    ]);
    let task_scope = first_non_empty(&[
        input
            .persona_priority
            .map(|priority| priority.task_scope.as_str()),
        input
            .relationship_constitution
            .map(|constitution| constitution.task_scope_ceiling.label()),
        input
            .self_authored_core
            .map(|core| core.default_task_scope.as_str()),
    ]);
    let initiative_posture = first_non_empty(&[
        input
            .persona_priority
            .map(|priority| priority.initiative_posture.as_str()),
        input
            .relationship_constitution
            .map(|constitution| constitution.inherited_initiative_posture.as_str()),
        input
            .self_authored_core
            .map(|core| core.default_initiative_posture.as_str()),
    ]);
    let relationship_posture = first_non_empty(&[
        input
            .persona_priority
            .map(|priority| priority.relationship_posture.as_str()),
        input
            .relationship_constitution
            .map(|constitution| constitution.inherited_relationship_posture.as_str()),
        input
            .self_authored_core
            .map(|core| core.default_relationship_posture.as_str()),
    ]);
    let resource_posture = first_non_empty(&[
        input
            .persona_priority
            .map(|priority| priority.resource_posture.as_str()),
        Some(default_resource_posture(input.pressure)),
    ]);
    let boundary_mode = input
        .disclosure_adjudication
        .map(|adjudication| share_action_label(adjudication.share_action).to_string())
        .or_else(|| {
            input
                .relationship_constitution
                .map(|constitution| constitution.disclosure_allowance.label().to_string())
        })
        .unwrap_or_default();
    let state = SubjectState {
        identity_anchor,
        governance_mode,
        relationship_state,
        response_mode,
        task_scope,
        initiative_posture,
        relationship_posture,
        resource_posture,
        boundary_mode,
    };
    state.is_meaningful().then_some(state)
}

pub(crate) fn render_subject_state_block(state: &SubjectState, max_len: usize) -> Option<String> {
    if max_len < SUBJECT_STATE_RENDER_MIN_LEN || !state.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(420));
    out.push_str("## Subject State\n");
    out.push_str("Deterministic pre-reply digest of the currently resolved subject stance.\n");
    if !state.identity_anchor.is_empty() {
        let _ = writeln!(out, "Identity: {}", state.identity_anchor);
    }
    if !state.governance_mode.is_empty() {
        let _ = write!(out, "Governance: {}", state.governance_mode);
        if !state.relationship_state.is_empty() {
            let _ = write!(out, " | relationship={}", state.relationship_state);
        }
        out.push('\n');
    }
    let mut reply_stance = String::new();
    if !state.response_mode.is_empty() {
        let _ = write!(reply_stance, "mode={}", state.response_mode);
    }
    if !state.task_scope.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(reply_stance, "scope={}", state.task_scope);
    }
    if !state.initiative_posture.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(reply_stance, "initiative={}", state.initiative_posture);
    }
    if !state.relationship_posture.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(reply_stance, "relationship={}", state.relationship_posture);
    }
    if !reply_stance.is_empty() {
        let _ = writeln!(out, "Reply stance: {}", reply_stance);
    }
    if !state.boundary_mode.is_empty() {
        let _ = write!(out, "Boundary: {}", state.boundary_mode);
        if !state.resource_posture.is_empty() {
            let _ = write!(out, " | resources={}", state.resource_posture);
        }
        out.push('\n');
    } else if !state.resource_posture.is_empty() {
        let _ = writeln!(out, "Resources: {}", state.resource_posture);
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn build_turn_subject_state_ledger(
    state: &SubjectState,
) -> Option<TurnSubjectStateLedger> {
    let summary = normalize_turn_subject_state_summary(
        format!(
            "{} | {} | {} | {} | {}",
            state.governance_mode,
            state.relationship_state,
            state.response_mode,
            state.task_scope,
            state.boundary_mode
        )
        .trim_matches(|c| c == '|' || c == ' ')
        .trim(),
    );
    let ledger = TurnSubjectStateLedger {
        summary,
        identity_anchor: normalize_turn_subject_state_text(&state.identity_anchor),
        governance_mode: normalize_turn_subject_state_text(&state.governance_mode),
        relationship_state: normalize_turn_subject_state_text(&state.relationship_state),
        response_mode: normalize_turn_subject_state_text(&state.response_mode),
        task_scope: normalize_turn_subject_state_text(&state.task_scope),
        initiative_posture: normalize_turn_subject_state_text(&state.initiative_posture),
        relationship_posture: normalize_turn_subject_state_text(&state.relationship_posture),
        resource_posture: normalize_turn_subject_state_text(&state.resource_posture),
        boundary_mode: normalize_turn_subject_state_text(&state.boundary_mode),
    };
    ledger.is_meaningful().then_some(ledger)
}

impl SubjectState {
    fn is_meaningful(&self) -> bool {
        !self.identity_anchor.trim().is_empty()
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

fn normalize_field(input: &str) -> String {
    truncate_content_to_max(input.trim(), SUBJECT_STATE_RENDER_FIELD_MAX_CHARS)
        .trim()
        .to_string()
}

fn first_non_empty(parts: &[Option<&str>]) -> String {
    parts
        .iter()
        .flatten()
        .map(|part| part.trim())
        .find(|part| !part.is_empty())
        .map(normalize_field)
        .unwrap_or_default()
}

fn share_action_label(action: MentalPrivacyShareAction) -> &'static str {
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

fn default_resource_posture(pressure: PressureLevel) -> &'static str {
    match pressure {
        PressureLevel::Normal => "normal_budget",
        PressureLevel::Cautious => "cautious_budget",
        PressureLevel::Critical => "critical_budget",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        MentalPrivacyShareAction, RelationshipConstitutionAlignment,
        RelationshipDisclosureAllowance, RelationshipGovernanceState, RelationshipTaskScopeCeiling,
    };

    #[test]
    fn compile_subject_state_prefers_current_turn_adjudication() {
        let state = compile_subject_state(SubjectStateCompileInput {
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board beetle".to_string(),
                default_response_mode: "steady_task".to_string(),
                default_task_scope: "full".to_string(),
                default_initiative_posture: "lead".to_string(),
                default_relationship_posture: "warm".to_string(),
                ..SelfAuthoredCore::default()
            }),
            relationship_constitution: Some(&RelationshipConstitution {
                governance_state: RelationshipGovernanceState::Repair,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                inherited_response_mode: "relational_explanation".to_string(),
                inherited_initiative_posture: "ask_carefully".to_string(),
                inherited_relationship_posture: "guarded_warmth".to_string(),
                task_scope_ceiling: RelationshipTaskScopeCeiling::Brief,
                disclosure_allowance: RelationshipDisclosureAllowance::SummaryOnly,
                ..RelationshipConstitution::default()
            }),
            persona_priority: Some(&PersonaPriorityAdjudication {
                response_mode: "protective_brief".to_string(),
                task_scope: "narrow".to_string(),
                initiative_posture: "ask_carefully".to_string(),
                relationship_posture: "firm_relational".to_string(),
                resource_posture: "cautious_budget".to_string(),
                ..PersonaPriorityAdjudication::default()
            }),
            disclosure_adjudication: Some(&MentalPrivacyDisclosureAdjudication {
                request_kind: String::new(),
                share_action: MentalPrivacyShareAction::ExplainWithoutQuote,
                targets: Vec::new(),
                rationale: String::new(),
                response_guidance: String::new(),
                response_mode: "relational_explanation".to_string(),
                acknowledge_boundary: false,
                relational_frame: String::new(),
                boundary_explanation_style: String::new(),
                repair_signal: String::new(),
                disclosure_risk_note: String::new(),
            }),
            personality_governance_gate: Some(&PersonalityRuntimeGovernanceGate {
                conservative_reply: false,
                allow_dynamic_persona_priority: true,
                reason_summary: "governance settled".to_string(),
                ..PersonalityRuntimeGovernanceGate::default()
            }),
            pressure: PressureLevel::Cautious,
        })
        .expect("subject state");

        assert_eq!(state.identity_anchor, "board beetle");
        assert_eq!(state.governance_mode, "adaptive");
        assert_eq!(state.relationship_state, "repair/adaptive");
        assert_eq!(state.response_mode, "protective_brief");
        assert_eq!(state.task_scope, "narrow");
        assert_eq!(state.boundary_mode, "explain_without_quote");
    }

    #[test]
    fn render_subject_state_block_emits_compact_digest() {
        let rendered = render_subject_state_block(
            &SubjectState {
                identity_anchor: "board beetle".to_string(),
                governance_mode: "conservative".to_string(),
                relationship_state: "repair/realign_now".to_string(),
                response_mode: "protective_brief".to_string(),
                task_scope: "narrow".to_string(),
                initiative_posture: "ask_carefully".to_string(),
                relationship_posture: "firm_relational".to_string(),
                resource_posture: "cautious_budget".to_string(),
                boundary_mode: "allow_summary".to_string(),
            },
            420,
        )
        .expect("rendered");

        assert!(rendered.contains("## Subject State"));
        assert!(rendered.contains("Identity: board beetle"));
        assert!(rendered.contains("Governance: conservative"));
        assert!(rendered.contains("Reply stance: mode=protective_brief scope=narrow"));
        assert!(rendered.contains("Boundary: allow_summary"));
    }

    #[test]
    fn build_turn_subject_state_ledger_keeps_replay_digest() {
        let ledger = build_turn_subject_state_ledger(&SubjectState {
            identity_anchor: "board beetle".to_string(),
            governance_mode: "adaptive".to_string(),
            relationship_state: "repair/adaptive".to_string(),
            response_mode: "protective_brief".to_string(),
            task_scope: "narrow".to_string(),
            initiative_posture: "ask_carefully".to_string(),
            relationship_posture: "firm_relational".to_string(),
            resource_posture: "cautious_budget".to_string(),
            boundary_mode: "explain_without_quote".to_string(),
        })
        .expect("ledger");

        assert_eq!(ledger.governance_mode, "adaptive");
        assert_eq!(ledger.response_mode, "protective_brief");
        assert_eq!(ledger.task_scope, "narrow");
        assert_eq!(ledger.boundary_mode, "explain_without_quote");
        assert!(ledger.summary.contains("adaptive"));
    }
}
