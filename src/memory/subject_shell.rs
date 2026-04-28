//! Deterministic subject-shell compiler for current prompt/runtime materials.
//! 当前 prepare/runtime 材料的确定性主体壳层编译器。

use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;
use std::fmt::Write as _;

use super::{OuterVoice, RelationshipConstitution, SelfAuthoredCore, SelfContinuity, SelfModel};

const SUBJECT_SHELL_FIELD_MAX_CHARS: usize = 120;
const SUBJECT_SHELL_SUMMARY_MAX_CHARS: usize = 180;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SubjectShell {
    pub(crate) body_ownership: String,
    pub(crate) memory_ownership: String,
    pub(crate) relationship_position: String,
    pub(crate) perception_context: String,
    pub(crate) situated_now: String,
    pub(crate) current_reasoning_basis: String,
    pub(crate) source_notes: String,
    pub(crate) inhabited_shell_summary: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SubjectShellCompileInput<'a> {
    pub(crate) now_secs: u64,
    pub(crate) platform: &'a str,
    pub(crate) device_identity: &'a str,
    pub(crate) relationship_scope: &'a str,
    pub(crate) channel: &'a str,
    pub(crate) chat_id: &'a str,
    pub(crate) pressure: PressureLevel,
    pub(crate) self_authored_core: Option<&'a SelfAuthoredCore>,
    pub(crate) self_continuity: Option<&'a SelfContinuity>,
    pub(crate) self_model: Option<&'a SelfModel>,
    pub(crate) outer_voice: Option<&'a OuterVoice>,
    pub(crate) relationship_constitution: Option<&'a RelationshipConstitution>,
    pub(crate) summary_text: Option<&'a str>,
    pub(crate) recent_turn_observation_text: Option<&'a str>,
    pub(crate) active_task_context_text: Option<&'a str>,
    pub(crate) governed_memory_evidence_text: Option<&'a str>,
    pub(crate) long_term_memory_text: Option<&'a str>,
    pub(crate) continuity_capsule_text: Option<&'a str>,
    pub(crate) world_snapshot_text: Option<&'a str>,
    pub(crate) world_sense_text: Option<&'a str>,
    pub(crate) memory_health_issues: &'a [String],
}

impl Default for SubjectShellCompileInput<'_> {
    fn default() -> Self {
        Self {
            now_secs: 0,
            platform: "",
            device_identity: "",
            relationship_scope: "",
            channel: "",
            chat_id: "",
            pressure: PressureLevel::Normal,
            self_authored_core: None,
            self_continuity: None,
            self_model: None,
            outer_voice: None,
            relationship_constitution: None,
            summary_text: None,
            recent_turn_observation_text: None,
            active_task_context_text: None,
            governed_memory_evidence_text: None,
            long_term_memory_text: None,
            continuity_capsule_text: None,
            world_snapshot_text: None,
            world_sense_text: None,
            memory_health_issues: &[],
        }
    }
}

pub(crate) fn compile_subject_shell(input: SubjectShellCompileInput<'_>) -> Option<SubjectShell> {
    let body_ownership = compile_body_ownership(input.self_authored_core);
    let memory_ownership = compile_memory_ownership(
        input.self_continuity,
        input.self_model,
        input.summary_text,
        input.continuity_capsule_text,
    );
    let relationship_position = compile_relationship_position(
        input.relationship_scope,
        input.channel,
        input.chat_id,
        input.relationship_constitution,
        input.self_model,
        input.outer_voice,
    );
    let current_reasoning_basis = first_non_empty(&[
        input.governed_memory_evidence_text,
        input.world_sense_text,
        input.world_snapshot_text,
        input.recent_turn_observation_text,
        input.long_term_memory_text,
        input.active_task_context_text,
        input.summary_text,
    ]);
    if body_ownership.trim().is_empty()
        || (memory_ownership.trim().is_empty() && relationship_position.trim().is_empty())
    {
        return None;
    }

    let situated_now = compile_situated_now(&input);
    let perception_context = first_non_empty(&[
        input.world_sense_text,
        input.world_snapshot_text,
        Some(situated_now.as_str()),
    ]);
    let source_notes = compile_source_notes(&input, &memory_ownership, &relationship_position);
    let inhabited_shell_summary = compile_summary(
        body_ownership.as_str(),
        memory_ownership.as_str(),
        relationship_position.as_str(),
    );

    Some(SubjectShell {
        body_ownership,
        memory_ownership,
        relationship_position,
        perception_context,
        situated_now,
        current_reasoning_basis,
        source_notes,
        inhabited_shell_summary,
    })
}

fn compile_body_ownership(core: Option<&SelfAuthoredCore>) -> String {
    let Some(core) = core.filter(|core| core.is_meaningful()) else {
        return String::new();
    };
    let mut parts = Vec::new();
    push_trimmed(&mut parts, &core.identity_anchor);
    push_trimmed(&mut parts, &core.default_response_mode);
    push_trimmed(&mut parts, &core.self_preservation_doctrine);
    join_limited(&parts)
}

fn compile_memory_ownership(
    continuity: Option<&SelfContinuity>,
    model: Option<&SelfModel>,
    summary_text: Option<&str>,
    continuity_capsule_text: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(continuity) = continuity.filter(|continuity| continuity.is_meaningful()) {
        push_trimmed(&mut parts, &continuity.wake_anchor);
        push_trimmed(&mut parts, &continuity.continuity_bridge);
        push_trimmed(&mut parts, &continuity.task_posture);
    }
    if parts.is_empty() {
        if let Some(model) = model.filter(|model| model.is_meaningful()) {
            push_trimmed(&mut parts, &model.continuity_anchor);
            push_trimmed(&mut parts, &model.self_narrative);
        }
    }
    if parts.is_empty() {
        push_optional(&mut parts, continuity_capsule_text);
        push_optional(&mut parts, summary_text);
    }
    join_limited(&parts)
}

fn compile_relationship_position(
    relationship_scope: &str,
    channel: &str,
    chat_id: &str,
    constitution: Option<&RelationshipConstitution>,
    model: Option<&SelfModel>,
    outer_voice: Option<&OuterVoice>,
) -> String {
    let scope_line = relationship_scope_line(relationship_scope, channel, chat_id);
    if let Some(constitution) = constitution {
        let mut line = format!(
            "{}/{}",
            constitution.governance_state.label(),
            constitution.alignment.label()
        );
        if !scope_line.is_empty() {
            let _ = write!(line, " {scope_line}");
        }
        if !constitution
            .inherited_relationship_posture
            .trim()
            .is_empty()
        {
            let _ = write!(
                line,
                " {}",
                constitution.inherited_relationship_posture.trim()
            );
        }
        return normalize_field(&line);
    }
    let mut parts = Vec::new();
    push_trimmed(&mut parts, &scope_line);
    if let Some(model) = model.filter(|model| model.is_meaningful()) {
        push_trimmed(&mut parts, &model.relationship_state);
        push_trimmed(&mut parts, &model.relational_ethic);
    }
    if let Some(voice) = outer_voice.filter(|voice| voice.is_meaningful()) {
        push_trimmed(&mut parts, &voice.relational_response_style);
    }
    join_limited(&parts)
}

fn compile_situated_now(input: &SubjectShellCompileInput<'_>) -> String {
    let mut parts = Vec::new();
    parts.push(format!("now={}", input.now_secs));
    if !input.channel.trim().is_empty() {
        parts.push(format!("channel={}", input.channel.trim()));
    }
    if !input.relationship_scope.trim().is_empty() {
        parts.push("relation=active".to_string());
    }
    if input.relationship_scope.trim().is_empty() && !input.chat_id.trim().is_empty() {
        parts.push("relation=active".to_string());
    }
    parts.push(format!("pressure={}", pressure_label(input.pressure)));
    if !input.platform.trim().is_empty() {
        parts.push(format!("platform={}", input.platform.trim()));
    }
    if !input.device_identity.trim().is_empty() {
        parts.push(format!("device={}", input.device_identity.trim()));
    }
    normalize_field(&parts.join(" "))
}

fn compile_source_notes(
    input: &SubjectShellCompileInput<'_>,
    memory_ownership: &str,
    relationship_position: &str,
) -> String {
    let mut notes = Vec::new();
    if memory_ownership.trim().is_empty() {
        notes.push("limited_sources: memory_continuity");
    }
    if relationship_position.trim().is_empty()
        || (input.relationship_constitution.is_none()
            && input
                .self_model
                .is_none_or(|model| model.relationship_state.trim().is_empty())
            && input
                .outer_voice
                .is_none_or(|voice| voice.relational_response_style.trim().is_empty())
            && input.relationship_scope.trim().is_empty()
            && input.channel.trim().is_empty()
            && input.chat_id.trim().is_empty())
    {
        notes.push("limited_sources: relationship_position");
    }
    if input
        .governed_memory_evidence_text
        .is_none_or(|text| text.trim().is_empty())
        && input
            .recent_turn_observation_text
            .is_none_or(|text| text.trim().is_empty())
        && input
            .long_term_memory_text
            .is_none_or(|text| text.trim().is_empty())
        && input
            .world_snapshot_text
            .is_none_or(|text| text.trim().is_empty())
        && input
            .world_sense_text
            .is_none_or(|text| text.trim().is_empty())
    {
        notes.push("limited_sources: reasoning_basis");
    }

    let health_issue = input
        .memory_health_issues
        .iter()
        .map(|issue| issue.trim())
        .find(|issue| !issue.is_empty());
    if let Some(issue) = health_issue {
        notes.push(issue);
    }
    join_limited(
        &notes
            .iter()
            .map(|note| note.to_string())
            .collect::<Vec<_>>(),
    )
}

fn compile_summary(body: &str, memory: &str, relationship: &str) -> String {
    let mut parts = Vec::new();
    push_trimmed(&mut parts, body);
    push_trimmed(&mut parts, memory);
    push_trimmed(&mut parts, relationship);
    truncate_content_to_max(parts.join(" | ").trim(), SUBJECT_SHELL_SUMMARY_MAX_CHARS)
        .trim()
        .to_string()
}

fn relationship_scope_line(relationship_scope: &str, channel: &str, chat_id: &str) -> String {
    let mut parts = Vec::new();
    if !relationship_scope.trim().is_empty() || !chat_id.trim().is_empty() {
        parts.push("active_relationship".to_string());
    }
    if !channel.trim().is_empty() {
        parts.push(format!("channel_kind={}", channel.trim()));
    }
    join_limited(&parts)
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

fn push_optional(parts: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value {
        push_trimmed(parts, value);
    }
}

fn push_trimmed(parts: &mut Vec<String>, value: &str) {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
}

fn join_limited(parts: &[String]) -> String {
    normalize_field(&parts.join(" | "))
}

fn normalize_field(input: &str) -> String {
    truncate_content_to_max(input.trim(), SUBJECT_SHELL_FIELD_MAX_CHARS)
        .trim()
        .to_string()
}

fn pressure_label(pressure: PressureLevel) -> &'static str {
    match pressure {
        PressureLevel::Normal => "normal",
        PressureLevel::Cautious => "cautious",
        PressureLevel::Critical => "critical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        RelationshipConstitution, RelationshipConstitutionAlignment, RelationshipGovernanceState,
        SelfAuthoredCore, SelfContinuity,
    };
    use crate::orchestrator::PressureLevel;

    #[test]
    fn default_input_returns_none() {
        assert_eq!(
            compile_subject_shell(SubjectShellCompileInput::default()),
            None
        );
    }

    #[test]
    fn grounded_shell_includes_body_memory_and_relationship() {
        let shell = compile_subject_shell(SubjectShellCompileInput {
            now_secs: 42,
            platform: "Linux",
            device_identity: "beetle-linux-dev",
            relationship_scope: "qq_channel:chat-grounded",
            channel: "qq_channel",
            chat_id: "chat-grounded",
            pressure: PressureLevel::Normal,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board-level beetle subject".to_string(),
                default_response_mode: "direct".to_string(),
                ..SelfAuthoredCore::default()
            }),
            self_continuity: Some(&SelfContinuity {
                wake_anchor: "wake as the same board subject".to_string(),
                continuity_bridge: "carry the prior task thread forward".to_string(),
                task_posture: "continue current implementation carefully".to_string(),
                ..SelfContinuity::default()
            }),
            relationship_constitution: Some(&RelationshipConstitution {
                governance_state: RelationshipGovernanceState::Maintain,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                inherited_relationship_posture: "collaborative implementation partner".to_string(),
                ..RelationshipConstitution::default()
            }),
            summary_text: Some("recent work is about deterministic subject state"),
            active_task_context_text: Some("implement P0-S subject shell compiler"),
            governed_memory_evidence_text: Some("recalled exact plan constraints"),
            ..SubjectShellCompileInput::default()
        })
        .expect("subject shell");

        assert!(shell.body_ownership.contains("board-level beetle subject"));
        assert!(shell
            .memory_ownership
            .contains("wake as the same board subject"));
        assert!(shell.relationship_position.contains("maintain/adaptive"));
        assert!(shell.situated_now.contains("now=42"));
        assert!(shell
            .current_reasoning_basis
            .contains("recalled exact plan constraints"));
        assert!(shell
            .inhabited_shell_summary
            .contains("board-level beetle subject"));
    }

    #[test]
    fn world_and_scope_inputs_influence_shell_projection() {
        let shell = compile_subject_shell(SubjectShellCompileInput {
            now_secs: 42,
            platform: "Linux",
            device_identity: "beetle-linux-dev",
            relationship_scope: "telegram:chat-world",
            channel: "telegram",
            chat_id: "chat-world",
            pressure: PressureLevel::Normal,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board-level beetle subject".to_string(),
                ..SelfAuthoredCore::default()
            }),
            self_continuity: Some(&SelfContinuity {
                wake_anchor: "wake as same subject".to_string(),
                ..SelfContinuity::default()
            }),
            relationship_constitution: Some(&RelationshipConstitution {
                governance_state: RelationshipGovernanceState::Maintain,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                ..RelationshipConstitution::default()
            }),
            world_snapshot_text: Some("world snapshot: router is offline"),
            world_sense_text: Some("world sense: operator is debugging runtime context"),
            ..SubjectShellCompileInput::default()
        })
        .expect("subject shell");

        assert!(shell.situated_now.contains("platform=Linux"));
        assert!(shell.situated_now.contains("device=beetle-linux-dev"));
        assert!(shell.situated_now.contains("relation=active"));
        assert!(shell.situated_now.contains("channel=telegram"));
        assert!(!shell.situated_now.contains("chat-world"));
        assert!(shell.relationship_position.contains("active_relationship"));
        assert!(!shell.relationship_position.contains("chat-world"));
        assert!(shell
            .current_reasoning_basis
            .contains("world sense: operator is debugging runtime context"));
        assert!(shell
            .inhabited_shell_summary
            .contains("active_relationship"));
    }

    #[test]
    fn complete_source_coverage_omits_source_notes() {
        let shell = compile_subject_shell(SubjectShellCompileInput {
            now_secs: 42,
            platform: "Linux",
            device_identity: "",
            relationship_scope: "telegram:chat-complete",
            channel: "telegram",
            chat_id: "chat-complete",
            pressure: PressureLevel::Normal,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board-level beetle subject".to_string(),
                ..SelfAuthoredCore::default()
            }),
            self_continuity: Some(&SelfContinuity {
                wake_anchor: "wake as same subject".to_string(),
                ..SelfContinuity::default()
            }),
            relationship_constitution: Some(&RelationshipConstitution {
                governance_state: RelationshipGovernanceState::Maintain,
                alignment: RelationshipConstitutionAlignment::Adaptive,
                ..RelationshipConstitution::default()
            }),
            world_snapshot_text: Some("world snapshot: router is online"),
            world_sense_text: Some("world sense: runtime context grounded"),
            governed_memory_evidence_text: Some("governed memory evidence"),
            ..SubjectShellCompileInput::default()
        })
        .expect("subject shell");

        assert!(!shell.situated_now.contains("device=Linux"));
        assert!(!shell.situated_now.contains("device_identity=Linux"));
        assert!(shell.source_notes.is_empty());
    }

    #[test]
    fn platform_only_runtime_context_does_not_emit_pseudo_device_identity() {
        let shell = compile_subject_shell(SubjectShellCompileInput {
            now_secs: 42,
            platform: "Linux",
            device_identity: "",
            relationship_scope: "telegram:chat-platform",
            channel: "telegram",
            chat_id: "chat-platform",
            pressure: PressureLevel::Normal,
            self_authored_core: Some(&SelfAuthoredCore {
                identity_anchor: "board-level beetle subject".to_string(),
                ..SelfAuthoredCore::default()
            }),
            self_continuity: Some(&SelfContinuity {
                wake_anchor: "wake as same subject".to_string(),
                ..SelfContinuity::default()
            }),
            world_sense_text: Some("world sense: runtime context grounded"),
            ..SubjectShellCompileInput::default()
        })
        .expect("subject shell");

        assert!(shell.situated_now.contains("platform=Linux"));
        assert!(!shell.situated_now.contains("device=Linux"));
    }

    #[test]
    fn task_only_input_returns_none() {
        assert_eq!(
            compile_subject_shell(SubjectShellCompileInput {
                active_task_context_text: Some("implement a compiler"),
                governed_memory_evidence_text: Some("reviewed plan constraints"),
                ..SubjectShellCompileInput::default()
            }),
            None
        );
    }
}
