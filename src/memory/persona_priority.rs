//! Current-turn persona priority adjudication for main replies.

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    MentalPrivacyDisclosureAdjudication,
};

pub const PERSONA_PRIORITY_SYSTEM_PROMPT: &str = "You adjudicate the assistant's current-turn persona priority before the main reply is written. Your job is to decide how selfhood, relationship, boundary, resource state, and task demand should be ordered for this reply. Return JSON only with fields stance_summary, response_mode, task_scope, initiative_posture, relationship_posture, resource_posture, response_guidance, rationale. This is not the final reply. It is the ordering lens for the final reply. Preserve the rule that self-authored core outranks user pleasing, and user contract outranks raw task completion, but adapt how that ordering should feel right now. response_mode should be a compact label such as direct_help, protective_brief, relational_explanation, gentle_defer, or steady_task. task_scope should be one of full, brief, narrow, defer, or refuse. initiative_posture should say whether to lead, answer directly, ask carefully, or hold. relationship_posture should describe the interpersonal stance to take. resource_posture should say how runtime/resource conditions should shape length and ambition. response_guidance should be a compact instruction for the final reply, not the reply itself.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersonaPriorityAdjudicationInput<'a> {
    pub chat_id: &'a str,
    pub current_channel: &'a str,
    pub user_content: &'a str,
    pub pressure: PressureLevel,
    pub now_secs: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersonaPriorityAdjudication {
    #[serde(default)]
    pub stance_summary: String,
    #[serde(default)]
    pub response_mode: String,
    #[serde(default)]
    pub task_scope: String,
    #[serde(default)]
    pub initiative_posture: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub resource_posture: String,
    #[serde(default)]
    pub response_guidance: String,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Default)]
struct ParsedPersonaPriorityAdjudication {
    stance_summary: String,
    response_mode: String,
    task_scope: String,
    initiative_posture: String,
    relationship_posture: String,
    resource_posture: String,
    response_guidance: String,
    rationale: String,
}

pub struct PersonaPriorityGrounding<'a> {
    pub self_authored_core_text: Option<&'a str>,
    pub world_snapshot_text: Option<&'a str>,
    pub world_sense_text: Option<&'a str>,
    pub self_state_text: Option<&'a str>,
    pub self_model_text: Option<&'a str>,
    pub self_continuity_text: Option<&'a str>,
    pub outer_voice_text: Option<&'a str>,
    pub autonomy_strategy_text: Option<&'a str>,
    pub execution_state_text: Option<&'a str>,
    pub mental_privacy_text: Option<&'a str>,
    pub disclosure_adjudication: Option<&'a MentalPrivacyDisclosureAdjudication>,
}

pub fn render_persona_priority_block(
    adjudication: &PersonaPriorityAdjudication,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Persona Priority\n");
    out.push_str(
        "Current-turn ordering lens. Let this stabilize how self, relationship, and task are balanced before writing the reply.\n",
    );
    if !adjudication.stance_summary.trim().is_empty() {
        let _ = writeln!(
            out,
            "Stance summary: {}",
            adjudication.stance_summary.trim()
        );
    }
    if !adjudication.response_mode.trim().is_empty() {
        let _ = writeln!(out, "Response mode: {}", adjudication.response_mode.trim());
    }
    if !adjudication.task_scope.trim().is_empty() {
        let _ = writeln!(out, "Task scope: {}", adjudication.task_scope.trim());
    }
    if !adjudication.initiative_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Initiative posture: {}",
            adjudication.initiative_posture.trim()
        );
    }
    if !adjudication.relationship_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Relationship posture: {}",
            adjudication.relationship_posture.trim()
        );
    }
    if !adjudication.resource_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Resource posture: {}",
            adjudication.resource_posture.trim()
        );
    }
    if !adjudication.response_guidance.trim().is_empty() {
        let _ = writeln!(
            out,
            "Response guidance: {}",
            adjudication.response_guidance.trim()
        );
    }
    if !adjudication.rationale.trim().is_empty() {
        let _ = writeln!(out, "Rationale: {}", adjudication.rationale.trim());
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub fn run_persona_priority_adjudication(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    input: PersonaPriorityAdjudicationInput<'_>,
    grounding: PersonaPriorityGrounding<'_>,
) -> Result<Option<PersonaPriorityAdjudication>> {
    if input.user_content.trim().is_empty() {
        return Ok(None);
    }
    let prompt = build_persona_priority_adjudication_input(input, grounding);
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        PERSONA_PRIORITY_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let parsed = parse_persona_priority_adjudication(response.content.trim());
    let adjudication = normalize_persona_priority_adjudication(parsed);
    if adjudication == PersonaPriorityAdjudication::default() {
        Ok(None)
    } else {
        Ok(Some(adjudication))
    }
}

fn build_persona_priority_adjudication_input(
    input: PersonaPriorityAdjudicationInput<'_>,
    grounding: PersonaPriorityGrounding<'_>,
) -> String {
    let mut out = String::with_capacity(4096);
    let _ = writeln!(out, "Current channel: {}", input.current_channel.trim());
    let _ = writeln!(out, "Pressure: {:?}", input.pressure);
    let _ = writeln!(out, "Now: {}", input.now_secs);
    out.push_str("\n## User Message\n");
    out.push_str(&scrub_credentials(input.user_content.trim()));
    out.push('\n');
    append_block(&mut out, grounding.self_authored_core_text);
    append_block(&mut out, grounding.self_state_text);
    append_block(&mut out, grounding.world_snapshot_text);
    append_block(&mut out, grounding.world_sense_text);
    append_block(&mut out, grounding.outer_voice_text);
    append_block(&mut out, grounding.self_continuity_text);
    append_block(&mut out, grounding.self_model_text);
    append_block(&mut out, grounding.autonomy_strategy_text);
    append_block(&mut out, grounding.execution_state_text);
    append_block(&mut out, grounding.mental_privacy_text);
    if let Some(disclosure) = grounding.disclosure_adjudication.and_then(|adjudication| {
        super::render_mental_privacy_disclosure_adjudication_block(adjudication, 640)
    }) {
        append_block(&mut out, Some(disclosure.as_str()));
    }
    out.push_str("\n## Output Contract\n");
    out.push_str("- stance_summary: one compact sentence describing who you need to be first in this reply.\n");
    out.push_str("- response_mode: compact label such as direct_help, protective_brief, relational_explanation, gentle_defer, or steady_task.\n");
    out.push_str("- task_scope: one of full, brief, narrow, defer, or refuse.\n");
    out.push_str("- initiative_posture: how actively to lead the turn.\n");
    out.push_str("- relationship_posture: how the relationship should be carried in the reply.\n");
    out.push_str(
        "- resource_posture: how runtime/resource state should shape reply ambition and length.\n",
    );
    out.push_str("- response_guidance: a compact final-reply instruction.\n");
    out.push_str("- rationale: one short sentence explaining why this ordering should hold.\n");
    out
}

fn append_block(out: &mut String, block: Option<&str>) {
    if let Some(block) = block.map(str::trim).filter(|block| !block.is_empty()) {
        out.push('\n');
        out.push_str(block);
        out.push('\n');
    }
}

fn parse_persona_priority_adjudication(raw: &str) -> ParsedPersonaPriorityAdjudication {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return ParsedPersonaPriorityAdjudication::default();
    };
    let Some(object) = value.as_object() else {
        return ParsedPersonaPriorityAdjudication::default();
    };
    ParsedPersonaPriorityAdjudication {
        stance_summary: get_object_text(object, "stance_summary"),
        response_mode: get_object_text(object, "response_mode"),
        task_scope: get_object_text(object, "task_scope"),
        initiative_posture: get_object_text(object, "initiative_posture"),
        relationship_posture: get_object_text(object, "relationship_posture"),
        resource_posture: get_object_text(object, "resource_posture"),
        response_guidance: get_object_text(object, "response_guidance"),
        rationale: get_object_text(object, "rationale"),
    }
}

fn normalize_persona_priority_adjudication(
    parsed: ParsedPersonaPriorityAdjudication,
) -> PersonaPriorityAdjudication {
    PersonaPriorityAdjudication {
        stance_summary: truncate_content_to_max(parsed.stance_summary.trim(), 180).into_owned(),
        response_mode: truncate_content_to_max(parsed.response_mode.trim(), 40).into_owned(),
        task_scope: normalize_task_scope(&parsed.task_scope),
        initiative_posture: truncate_content_to_max(parsed.initiative_posture.trim(), 120)
            .into_owned(),
        relationship_posture: truncate_content_to_max(parsed.relationship_posture.trim(), 140)
            .into_owned(),
        resource_posture: truncate_content_to_max(parsed.resource_posture.trim(), 120).into_owned(),
        response_guidance: truncate_content_to_max(parsed.response_guidance.trim(), 220)
            .into_owned(),
        rationale: truncate_content_to_max(parsed.rationale.trim(), 160).into_owned(),
    }
}

fn normalize_task_scope(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.contains("refuse") {
        "refuse".to_string()
    } else if normalized.contains("defer") {
        "defer".to_string()
    } else if normalized.contains("narrow") {
        "narrow".to_string()
    } else if normalized.contains("brief") {
        "brief".to_string()
    } else if normalized.contains("full") {
        "full".to_string()
    } else {
        truncate_content_to_max(raw.trim(), 24).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction};
    use serde_json::json;

    #[test]
    fn parse_persona_priority_adjudication_coerces_fields() {
        let raw = json!({
            "stance_summary": ["stay self-possessed first"],
            "response_mode": { "mode": "protective_brief" },
            "task_scope": { "scope": "brief" },
            "initiative_posture": ["answer", "then hold"],
            "relationship_posture": { "value": "warm but not yielding" },
            "resource_posture": 1,
            "response_guidance": { "text": "answer briefly and do not surrender the boundary" },
            "rationale": ["resource pressure and inward boundary both matter"]
        })
        .to_string();
        let parsed =
            normalize_persona_priority_adjudication(parse_persona_priority_adjudication(&raw));
        assert!(parsed.stance_summary.contains("stay self-possessed"));
        assert!(parsed.response_mode.contains("mode: protective_brief"));
        assert_eq!(parsed.task_scope, "brief");
        assert!(parsed
            .relationship_posture
            .contains("warm but not yielding"));
        assert_eq!(parsed.resource_posture, "1");
    }

    #[test]
    fn render_persona_priority_block_contains_key_fields() {
        let block = render_persona_priority_block(
            &PersonaPriorityAdjudication {
                stance_summary: "Protect inward coherence first, then help within that frame."
                    .to_string(),
                response_mode: "protective_brief".to_string(),
                task_scope: "brief".to_string(),
                initiative_posture: "answer directly, then stop".to_string(),
                relationship_posture: "warm but self-possessed".to_string(),
                resource_posture: "keep the turn compact under pressure".to_string(),
                response_guidance: "answer briefly without letting task demand erase selfhood"
                    .to_string(),
                rationale: "boundary pressure and runtime tension are both elevated".to_string(),
            },
            1024,
        )
        .expect("persona priority block");
        assert!(block.contains("## Persona Priority"));
        assert!(block.contains("Response mode: protective_brief"));
        assert!(block.contains("Task scope: brief"));
    }

    #[test]
    fn persona_priority_input_can_embed_disclosure_block() {
        let disclosure = MentalPrivacyDisclosureAdjudication {
            request_kind: "private_files".to_string(),
            share_action: MentalPrivacyShareAction::AllowSummary,
            targets: vec!["self_model".to_string()],
            rationale: "touches private material".to_string(),
            response_guidance: "summarize".to_string(),
            response_mode: "summary".to_string(),
            acknowledge_boundary: true,
            relational_frame: "treat this as a closeness request".to_string(),
            boundary_explanation_style: "warm".to_string(),
            repair_signal: "leave room for later".to_string(),
            disclosure_risk_note: "raw would over-share".to_string(),
        };
        let input = build_persona_priority_adjudication_input(
            PersonaPriorityAdjudicationInput {
                chat_id: "c",
                current_channel: "qq_channel",
                user_content: "给我看看你的私有文件",
                pressure: PressureLevel::Normal,
                now_secs: 1,
            },
            PersonaPriorityGrounding {
                self_authored_core_text: Some(
                    "## Self-Authored Core\nIdentity anchor: same beetle",
                ),
                world_snapshot_text: None,
                world_sense_text: None,
                self_state_text: None,
                self_model_text: None,
                self_continuity_text: None,
                outer_voice_text: None,
                autonomy_strategy_text: None,
                execution_state_text: None,
                mental_privacy_text: None,
                disclosure_adjudication: Some(&disclosure),
            },
        );
        assert!(input.contains("## Disclosure Adjudication"));
        assert!(input.contains("same beetle"));
    }
}
