//! Deterministic subject-state compiler for the current reply turn.
//! 当前回合的确定性主体状态编译器。

use crate::memory::{
    normalize_turn_subject_state_summary, normalize_turn_subject_state_text, FeltSignificance,
    InnerConflict, MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction,
    PersonaPriorityAdjudication, PersonalityRuntimeGovernanceGate, RelationshipConstitution,
    SelfAuthoredCore, SubjectShell, TemperamentContinuity, TurnSubjectStateLedger,
};
use crate::orchestrator::PressureLevel;
use crate::util::{truncate_content_to_max, truncate_to_byte_len};
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
    pub embodied_position: String,
    pub experience_ownership: String,
    pub perception_feedback: String,
    pub situated_now: String,
    pub current_reasoning_basis: String,
    pub source_notes: String,
    pub inhabited_shell_summary: String,
    pub significance_direction: String,
    pub self_reading: String,
    pub certainty_mode: String,
    pub resistance_mode: String,
    pub unresolved_tension: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SubjectStateCompileInput<'a> {
    pub subject_shell: Option<&'a SubjectShell>,
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub relationship_constitution: Option<&'a RelationshipConstitution>,
    pub persona_priority: Option<&'a PersonaPriorityAdjudication>,
    pub disclosure_adjudication: Option<&'a MentalPrivacyDisclosureAdjudication>,
    pub personality_governance_gate: Option<&'a PersonalityRuntimeGovernanceGate>,
    pub felt_significance: Option<&'a FeltSignificance>,
    pub temperament_continuity: Option<&'a TemperamentContinuity>,
    pub inner_conflict: Option<&'a InnerConflict>,
    pub now_secs: u64,
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
    let significance_direction = compile_significance_direction(input.felt_significance);
    let self_reading = compile_self_reading(input.temperament_continuity, input.self_authored_core);
    let certainty_mode = compile_certainty_mode(input.inner_conflict, input.now_secs);
    let resistance_mode = compile_resistance_mode(input.temperament_continuity, &boundary_mode);
    let unresolved_tension = compile_unresolved_tension(input.inner_conflict);
    let (
        embodied_position,
        experience_ownership,
        perception_feedback,
        situated_now,
        current_reasoning_basis,
        source_notes,
        inhabited_shell_summary,
    ) = input
        .subject_shell
        .map(|shell| {
            (
                normalize_field(&shell.body_ownership),
                normalize_field(&shell.memory_ownership),
                normalize_field(&shell.perception_context),
                normalize_field(&shell.situated_now),
                normalize_field(&shell.current_reasoning_basis),
                normalize_field(&shell.source_notes),
                normalize_field(&shell.inhabited_shell_summary),
            )
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
        embodied_position,
        experience_ownership,
        perception_feedback,
        situated_now,
        current_reasoning_basis,
        source_notes,
        inhabited_shell_summary,
        significance_direction,
        self_reading,
        certainty_mode,
        resistance_mode,
        unresolved_tension,
    };
    state.is_meaningful().then_some(state)
}

pub(crate) fn render_subject_state_block(state: &SubjectState, max_len: usize) -> Option<String> {
    if max_len < SUBJECT_STATE_RENDER_MIN_LEN || !state.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(420));
    if !append_line_if_fits(&mut out, "## Subject State", max_len) {
        return None;
    }
    let _ = append_line_if_fits(&mut out, "Resolved pre-reply stance.", max_len);
    if !state.governance_mode.is_empty() {
        let mut line = format!(
            "Governance: {}",
            compact_render_field(&state.governance_mode)
        );
        if !state.relationship_state.is_empty() {
            let _ = write!(
                line,
                " | relationship={}",
                compact_render_field(&state.relationship_state)
            );
        }
        let _ = append_line_if_fits(&mut out, &line, max_len);
    }
    let mut reply_stance = String::new();
    if !state.response_mode.is_empty() {
        let _ = write!(
            reply_stance,
            "mode={}",
            compact_render_field(&state.response_mode)
        );
    }
    if !state.task_scope.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(
            reply_stance,
            "scope={}",
            compact_render_field(&state.task_scope)
        );
    }
    if !state.initiative_posture.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(
            reply_stance,
            "initiative={}",
            compact_render_field(&state.initiative_posture)
        );
    }
    if !state.relationship_posture.is_empty() {
        if !reply_stance.is_empty() {
            reply_stance.push(' ');
        }
        let _ = write!(
            reply_stance,
            "relationship={}",
            compact_render_field(&state.relationship_posture)
        );
    }
    if !reply_stance.is_empty() {
        let line = format!("Reply stance: {}", reply_stance);
        let _ = append_line_if_fits(&mut out, &line, max_len);
    }
    if !state.boundary_mode.is_empty() {
        let mut line = format!("Boundary: {}", compact_render_field(&state.boundary_mode));
        if !state.resource_posture.is_empty() {
            let _ = write!(
                line,
                " | resources={}",
                compact_render_field(&state.resource_posture)
            );
        }
        let _ = append_line_if_fits(&mut out, &line, max_len);
    } else if !state.resource_posture.is_empty() {
        let line = format!(
            "Resources: {}",
            compact_render_field(&state.resource_posture)
        );
        let _ = append_line_if_fits(&mut out, &line, max_len);
    }
    if !state.identity_anchor.is_empty() {
        let line = format!("Identity: {}", compact_render_field(&state.identity_anchor));
        let _ = append_line_if_fits(&mut out, &line, max_len);
    }
    append_shell_mount_lines(&mut out, state, max_len);
    append_subjective_projection_line(&mut out, state, max_len);
    let rendered = out.trim_end().to_string();
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
            || !self.embodied_position.trim().is_empty()
            || !self.experience_ownership.trim().is_empty()
            || !self.perception_feedback.trim().is_empty()
            || !self.situated_now.trim().is_empty()
            || !self.current_reasoning_basis.trim().is_empty()
            || !self.source_notes.trim().is_empty()
            || !self.inhabited_shell_summary.trim().is_empty()
            || !self.significance_direction.trim().is_empty()
            || !self.self_reading.trim().is_empty()
            || !self.certainty_mode.trim().is_empty()
            || !self.resistance_mode.trim().is_empty()
            || !self.unresolved_tension.trim().is_empty()
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

fn compact_render_field(input: &str) -> String {
    truncate_to_byte_len(input.trim(), 16).trim().to_string()
}

fn compile_significance_direction(felt: Option<&FeltSignificance>) -> String {
    let Some(felt) = felt.filter(|state| state.is_meaningful()) else {
        return String::new();
    };
    let summary = felt.significance_summary.trim();
    if !summary.is_empty() {
        return normalize_field(summary);
    }
    let mut parts = Vec::new();
    push_first_list_item(&mut parts, "matters", &felt.what_matters_now);
    push_first_list_item(&mut parts, "closer", &felt.pull_closer);
    push_first_list_item(&mut parts, "back", &felt.pull_back);
    normalize_field(&parts.join(" | "))
}

fn compile_self_reading(
    temperament: Option<&TemperamentContinuity>,
    core: Option<&SelfAuthoredCore>,
) -> String {
    first_non_empty(&[
        temperament.map(|state| state.stability_summary.as_str()),
        core.map(|state| state.identity_anchor.as_str()),
    ])
}

fn compile_certainty_mode(inner_conflict: Option<&InnerConflict>, now_secs: u64) -> String {
    match inner_conflict {
        Some(conflict) if conflict.is_active_at(now_secs) => "forming".to_string(),
        Some(conflict) if conflict.review_due_at(now_secs) => "needs_review".to_string(),
        Some(conflict) if conflict.is_meaningful() => "held".to_string(),
        _ => String::new(),
    }
}

fn compile_resistance_mode(
    temperament: Option<&TemperamentContinuity>,
    boundary_mode: &str,
) -> String {
    first_non_empty(&[
        temperament.map(|state| state.boundary_inertia.as_str()),
        Some(boundary_mode),
    ])
}

fn compile_unresolved_tension(inner_conflict: Option<&InnerConflict>) -> String {
    inner_conflict
        .filter(|state| state.is_meaningful())
        .map(|state| normalize_field(&state.topic))
        .unwrap_or_default()
}

fn push_first_list_item(parts: &mut Vec<String>, label: &str, values: &[String]) {
    if let Some(value) = values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
    {
        parts.push(format!("{label}={value}"));
    }
}

fn append_subjective_projection_line(out: &mut String, state: &SubjectState, max_len: usize) {
    let mut parts = Vec::new();
    push_compact_pair(&mut parts, "direction", &state.significance_direction);
    push_compact_pair(&mut parts, "self", &state.self_reading);
    push_compact_pair(&mut parts, "certainty", &state.certainty_mode);
    push_compact_pair(&mut parts, "resistance", &state.resistance_mode);
    push_compact_pair(&mut parts, "tension", &state.unresolved_tension);
    if parts.is_empty() {
        return;
    }
    let mut candidates = Vec::new();
    candidates.push(format!("Subjective: {}", parts.join(" ")));
    if parts.len() > 3 {
        candidates.push(format!(
            "Subjective: {}",
            parts.iter().take(3).cloned().collect::<Vec<_>>().join(" ")
        ));
    }
    let mut compact_parts = Vec::new();
    push_compact_pair(&mut compact_parts, "self", &state.self_reading);
    push_compact_pair(&mut compact_parts, "certainty", &state.certainty_mode);
    if !compact_parts.is_empty() {
        candidates.push(format!("Subjective: {}", compact_parts.join(" ")));
    }
    push_compact_pair(&mut compact_parts, "tension", &state.unresolved_tension);
    if !compact_parts.is_empty() {
        candidates.push(format!("Subjective: {}", compact_parts.join(" ")));
    }
    candidates.push("Subjective: compact".to_string());
    for candidate in candidates {
        if append_line_if_fits(out, &candidate, max_len) {
            break;
        }
    }
}

fn append_shell_mount_lines(out: &mut String, state: &SubjectState, max_len: usize) {
    let mut mount_parts = Vec::new();
    push_compact_pair(&mut mount_parts, "body", &state.embodied_position);
    push_compact_pair(&mut mount_parts, "experience", &state.experience_ownership);
    push_compact_pair(&mut mount_parts, "perception", &state.perception_feedback);
    if mount_parts.is_empty() && state.inhabited_shell_summary.trim().is_empty() {
        return;
    }
    let mut candidates = Vec::new();
    let basis = (!state.current_reasoning_basis.trim().is_empty()).then(|| {
        format!(
            "basis={}",
            truncate_content_to_max(state.current_reasoning_basis.trim(), 20).trim()
        )
    });
    if !mount_parts.is_empty() {
        candidates.push(match basis.as_ref() {
            Some(basis) => format!("Mount: {} {}", mount_parts.join(" "), basis),
            None => format!("Mount: {}", mount_parts.join(" ")),
        });
        candidates.push(format!(
            "Mount: {}",
            mount_parts
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    if !state.inhabited_shell_summary.trim().is_empty() {
        candidates.push(match basis.as_ref() {
            Some(basis) => format!(
                "Mount: shell={} {}",
                truncate_content_to_max(state.inhabited_shell_summary.trim(), 24).trim(),
                basis
            ),
            None => format!(
                "Mount: shell={}",
                truncate_content_to_max(state.inhabited_shell_summary.trim(), 32).trim()
            ),
        });
    }
    if let Some(basis) = basis {
        candidates.push(format!("Mount: {basis}"));
    }
    candidates.push("Mount: compact".to_string());
    for candidate in candidates {
        if append_line_if_fits(out, &candidate, max_len) {
            break;
        }
    }

    if !state.source_notes.trim().is_empty() {
        let note = format!(
            "Source notes: {}",
            truncate_content_to_max(state.source_notes.trim(), 56).trim()
        );
        let _ = append_line_if_fits(out, &note, max_len);
    }
}

fn push_compact_pair(parts: &mut Vec<String>, label: &str, value: &str) {
    let value = value.trim();
    if !value.is_empty() {
        parts.push(format!(
            "{label}={}",
            truncate_content_to_max(value, 20).trim()
        ));
    }
}

fn append_line_if_fits(out: &mut String, line: &str, max_len: usize) -> bool {
    if out.len().saturating_add(line.len()).saturating_add(1) <= max_len {
        let _ = writeln!(out, "{line}");
        true
    } else {
        false
    }
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
        FeltSignificance, InnerConflict, MentalPrivacyShareAction,
        RelationshipConstitutionAlignment, RelationshipDisclosureAllowance,
        RelationshipGovernanceState, RelationshipTaskScopeCeiling, TemperamentContinuity,
    };

    #[test]
    fn compile_subject_state_prefers_current_turn_adjudication() {
        let state = compile_subject_state(SubjectStateCompileInput {
            subject_shell: None,
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
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            pressure: PressureLevel::Cautious,
            now_secs: 42,
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
                ..SubjectState::default()
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
            ..SubjectState::default()
        })
        .expect("ledger");

        assert_eq!(ledger.governance_mode, "adaptive");
        assert_eq!(ledger.response_mode, "protective_brief");
        assert_eq!(ledger.task_scope, "narrow");
        assert_eq!(ledger.boundary_mode, "explain_without_quote");
        assert!(ledger.summary.contains("adaptive"));
    }

    #[test]
    fn compile_subject_state_projects_subject_shell_fields() {
        let shell = crate::memory::SubjectShell {
            body_ownership: "body: board-level beetle subject".to_string(),
            memory_ownership: "memory: continuity bridge active".to_string(),
            relationship_position: "relationship: steady/adaptive".to_string(),
            perception_context: "world sense: runtime context".to_string(),
            situated_now: "now=42 runtime_platform=Linux pressure=normal".to_string(),
            current_reasoning_basis: "basis: current prepare context".to_string(),
            source_notes: "limited_sources: world_sense".to_string(),
            inhabited_shell_summary: "board-level beetle subject in steady/adaptive".to_string(),
        };

        let state = compile_subject_state(SubjectStateCompileInput {
            subject_shell: Some(&shell),
            self_authored_core: None,
            relationship_constitution: None,
            persona_priority: None,
            disclosure_adjudication: None,
            personality_governance_gate: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            pressure: PressureLevel::Normal,
            now_secs: 42,
        })
        .expect("subject state");

        assert_eq!(state.embodied_position, shell.body_ownership);
        assert_eq!(state.experience_ownership, shell.memory_ownership);
        assert_eq!(state.perception_feedback, shell.perception_context);
        assert_eq!(state.current_reasoning_basis, shell.current_reasoning_basis);
        assert_eq!(state.source_notes, shell.source_notes);

        let rendered = render_subject_state_block(&state, 520).expect("rendered");
        assert!(rendered.contains("Mount:"));
        assert!(rendered.contains("experience=memory: continuity"));
        assert!(rendered.contains("perception=world sense: runtime"));
        assert!(rendered.contains("Source notes: limited_sources: world_sense"));
    }

    #[test]
    fn compile_subject_state_projects_p3_humanization_fields() {
        let shell = crate::memory::SubjectShell {
            body_ownership: "body: board-level beetle subject".to_string(),
            memory_ownership: "memory: continuity bridge active".to_string(),
            relationship_position: "relationship: steady/adaptive".to_string(),
            perception_context: "world sense: runtime context".to_string(),
            situated_now: "now=42 runtime_platform=Linux pressure=normal".to_string(),
            current_reasoning_basis: "basis: current prepare context".to_string(),
            source_notes: "limited_sources: world_sense".to_string(),
            inhabited_shell_summary: "board-level beetle subject in steady/adaptive".to_string(),
        };
        let felt_significance = FeltSignificance {
            significance_summary: "builds trust by staying coherent".to_string(),
            pull_closer: vec!["share a concise plan".to_string()],
            pull_back: vec!["avoid raw private dumps".to_string()],
            updated_at: 42,
            ..FeltSignificance::default()
        };
        let temperament_continuity = TemperamentContinuity {
            stability_summary: "steady under pressure".to_string(),
            boundary_inertia: "keeps private material summarized".to_string(),
            updated_at: 42,
            ..TemperamentContinuity::default()
        };
        let inner_conflict = InnerConflict {
            topic: "whether to expose private reasoning".to_string(),
            pull_a: "be transparent with the user".to_string(),
            pull_b: "protect private workspace".to_string(),
            current_lean: "summarize the boundary".to_string(),
            unresolved_reason: "needs more relationship evidence".to_string(),
            review_after_secs: 1_800,
            updated_at: 42,
        };

        let state = compile_subject_state(SubjectStateCompileInput {
            subject_shell: Some(&shell),
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "identity fallback".to_string(),
                ..SelfAuthoredCore::default()
            }),
            relationship_constitution: None,
            persona_priority: None,
            disclosure_adjudication: None,
            personality_governance_gate: None,
            felt_significance: Some(&felt_significance),
            temperament_continuity: Some(&temperament_continuity),
            inner_conflict: Some(&inner_conflict),
            pressure: PressureLevel::Normal,
            now_secs: 42,
        })
        .expect("subject state");

        assert_eq!(state.embodied_position, shell.body_ownership);
        assert_eq!(
            state.significance_direction,
            "builds trust by staying coherent"
        );
        assert_eq!(state.self_reading, "steady under pressure");
        assert_eq!(state.certainty_mode, "forming");
        assert_eq!(state.resistance_mode, "keeps private material summarized");
        assert_eq!(
            state.unresolved_tension,
            "whether to expose private reasoning"
        );
    }

    #[test]
    fn compile_subject_state_falls_back_when_p3_layers_are_absent() {
        let state = compile_subject_state(SubjectStateCompileInput {
            subject_shell: None,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "identity fallback".to_string(),
                ..SelfAuthoredCore::default()
            }),
            relationship_constitution: None,
            persona_priority: None,
            disclosure_adjudication: Some(&MentalPrivacyDisclosureAdjudication {
                request_kind: String::new(),
                share_action: MentalPrivacyShareAction::AllowSummary,
                targets: Vec::new(),
                rationale: String::new(),
                response_guidance: String::new(),
                response_mode: "brief".to_string(),
                acknowledge_boundary: false,
                relational_frame: String::new(),
                boundary_explanation_style: String::new(),
                repair_signal: String::new(),
                disclosure_risk_note: String::new(),
            }),
            personality_governance_gate: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: None,
            pressure: PressureLevel::Normal,
            now_secs: 42,
        })
        .expect("subject state");

        assert_eq!(state.self_reading, "identity fallback");
        assert!(state.certainty_mode.is_empty());
        assert_eq!(state.resistance_mode, "allow_summary");
        assert!(state.significance_direction.is_empty());
        assert!(state.unresolved_tension.is_empty());
    }

    #[test]
    fn compile_subject_state_marks_expired_inner_conflict_for_review() {
        let expired_conflict = InnerConflict {
            topic: "whether to expose private reasoning".to_string(),
            pull_a: "be transparent".to_string(),
            pull_b: "protect private workspace".to_string(),
            current_lean: "summarize boundary".to_string(),
            unresolved_reason: "review window elapsed".to_string(),
            review_after_secs: 1_800,
            updated_at: 100,
        };

        let state = compile_subject_state(SubjectStateCompileInput {
            subject_shell: None,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "identity fallback".to_string(),
                ..SelfAuthoredCore::default()
            }),
            relationship_constitution: None,
            persona_priority: None,
            disclosure_adjudication: None,
            personality_governance_gate: None,
            felt_significance: None,
            temperament_continuity: None,
            inner_conflict: Some(&expired_conflict),
            now_secs: 2_000,
            pressure: PressureLevel::Normal,
        })
        .expect("subject state");

        assert_eq!(state.certainty_mode, "needs_review");
        assert_eq!(
            state.unresolved_tension,
            "whether to expose private reasoning"
        );
    }

    #[test]
    fn render_subject_state_block_keeps_p3_projection_compact_under_360() {
        let rendered = render_subject_state_block(
            &SubjectState {
                governance_mode: "adaptive".to_string(),
                response_mode: "protective_brief".to_string(),
                task_scope: "narrow".to_string(),
                resource_posture: "normal_budget".to_string(),
                boundary_mode: "allow_summary".to_string(),
                embodied_position: "board-level body".to_string(),
                significance_direction: "relationship coherence matters now".to_string(),
                self_reading: "steady under pressure".to_string(),
                certainty_mode: "forming".to_string(),
                resistance_mode: "keeps private material summarized".to_string(),
                unresolved_tension: "whether to expose private reasoning".to_string(),
                ..SubjectState::default()
            },
            360,
        )
        .expect("rendered");

        assert!(rendered.contains("Governance:"));
        assert!(rendered.contains("Reply stance:"));
        assert!(rendered.contains("Boundary:"));
        assert!(rendered.contains("Mount:"));
        assert!(rendered.contains("Subjective:"));
        assert!(rendered.contains("self=steady"));
        assert!(rendered.contains("certainty=forming"));
        assert!(!rendered.contains("pull_a"));
        assert!(!rendered.contains("pull_b"));
        assert!(rendered.len() <= 360);
    }

    #[test]
    fn render_subject_state_block_preserves_mount_before_p3_under_tight_budget() {
        let rendered = render_subject_state_block(
            &SubjectState {
                governance_mode: "adaptive".to_string(),
                relationship_state: "relationship-state-long".to_string(),
                response_mode: "protective-brief-mode".to_string(),
                task_scope: "narrow-but-named".to_string(),
                initiative_posture: "hold-current-thread".to_string(),
                resource_posture: "normal-budget-long".to_string(),
                boundary_mode: "allow-summary-now".to_string(),
                embodied_position: "board-level body".to_string(),
                significance_direction: "relationship coherence matters".to_string(),
                self_reading: "steady under pressure".to_string(),
                certainty_mode: "forming".to_string(),
                unresolved_tension: "private reasoning boundary".to_string(),
                ..SubjectState::default()
            },
            360,
        )
        .expect("rendered");

        assert!(rendered.contains("Governance:"));
        assert!(rendered.contains("Reply stance:"));
        assert!(rendered.contains("Boundary:"));
        assert!(rendered.contains("Mount:"));
        assert!(rendered.len() <= 360);
    }

    #[test]
    fn render_subject_state_360_preserves_core_stance_with_large_shell_fields() {
        let long = "x".repeat(SUBJECT_STATE_RENDER_FIELD_MAX_CHARS);
        let rendered = render_subject_state_block(
            &SubjectState {
                identity_anchor: long.clone(),
                governance_mode: long.clone(),
                relationship_state: long.clone(),
                response_mode: long.clone(),
                task_scope: long.clone(),
                initiative_posture: long.clone(),
                relationship_posture: long.clone(),
                resource_posture: long.clone(),
                boundary_mode: "allow_summary".to_string(),
                embodied_position: long.clone(),
                experience_ownership: long.clone(),
                perception_feedback: long.clone(),
                situated_now: long.clone(),
                current_reasoning_basis: long.clone(),
                source_notes: long.clone(),
                inhabited_shell_summary: long,
                ..SubjectState::default()
            },
            360,
        )
        .expect("rendered");

        assert!(rendered.contains("Governance:"));
        assert!(rendered.contains("Reply stance:"));
        assert!(rendered.contains("Boundary:"));
        assert!(rendered.contains("Mount:"));
        assert!(rendered.contains("basis="));
        assert!(rendered.len() <= 360);
    }

    #[test]
    fn render_subject_state_block_stays_within_budget_with_non_ascii_fields() {
        let long = "主体状态".repeat(SUBJECT_STATE_RENDER_FIELD_MAX_CHARS);
        let rendered = render_subject_state_block(
            &SubjectState {
                identity_anchor: long.clone(),
                governance_mode: long.clone(),
                relationship_state: long.clone(),
                response_mode: long.clone(),
                task_scope: long.clone(),
                initiative_posture: long.clone(),
                relationship_posture: long.clone(),
                resource_posture: long.clone(),
                boundary_mode: "allow_summary".to_string(),
                embodied_position: long.clone(),
                experience_ownership: long.clone(),
                perception_feedback: long.clone(),
                situated_now: long.clone(),
                current_reasoning_basis: long.clone(),
                source_notes: long.clone(),
                inhabited_shell_summary: long,
                ..SubjectState::default()
            },
            360,
        )
        .expect("rendered");

        assert!(rendered.len() <= 360);
        assert!(rendered.contains("Governance:"));
        assert!(rendered.contains("Reply stance:"));
        assert!(rendered.contains("Boundary:"));
    }
}
