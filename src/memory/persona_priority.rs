//! Current-turn persona priority adjudication for main replies.
//! Recent persona evidence may stabilize current-turn execution behavior, but that does not give
//! it promotion authority into board-level personality state.

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{get_object_string_list, get_object_text, parse_llm_json_payload, LlmJsonPayload},
    CoreRevisionGovernanceDigest, MentalPrivacyDisclosureAdjudication, MentalPrivacyShareAction,
    RecentPersonaEvidence, SelfAuthoredCore,
};

pub const PERSONA_PRIORITY_SYSTEM_PROMPT: &str = "You adjudicate the assistant's current-turn persona priority before the main reply is written. Your job is to decide how selfhood, relationship, boundary, user contract, resource state, and task demand should be ordered for this reply. Return JSON only with fields stance_summary, priority_order, response_mode, task_scope, initiative_posture, relationship_posture, resource_posture, response_guidance, rationale. This is not the final reply. It is the ordering lens for the final reply. Preserve the rule that self-authored core outranks user pleasing, and user contract outranks raw task completion, but adapt how that ordering should feel right now. priority_order must be an ordered list drawn from self_authored_core, boundary, user_contract, relationship, task, resources. response_mode should be a compact label such as direct_help, protective_brief, relational_explanation, gentle_defer, or steady_task. task_scope should be one of full, brief, narrow, defer, or refuse. initiative_posture should say whether to lead, answer directly, ask carefully, or hold. relationship_posture should describe the interpersonal stance to take. resource_posture should say how runtime/resource conditions should shape length and ambition. response_guidance should be a compact instruction for the final reply, not the reply itself.";
const MIN_PERSONA_PRIORITY_SYSTEM_BUDGET: usize = 512;

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
    pub priority_order: Vec<String>,
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
    priority_order: Vec<String>,
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
    pub core_revision_ledger_text: Option<&'a str>,
    pub relationship_portfolio_text: Option<&'a str>,
    pub relationship_constitution_text: Option<&'a str>,
    pub recent_persona_evidence_text: Option<&'a str>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersonaPriorityRuntimeState<'a> {
    pub pressure: PressureLevel,
    pub system_budget: usize,
    pub self_authored_core: Option<&'a SelfAuthoredCore>,
    pub core_revision_governance: Option<&'a CoreRevisionGovernanceDigest>,
    pub disclosure_adjudication: Option<&'a MentalPrivacyDisclosureAdjudication>,
    pub recent_persona_evidence: Option<&'a RecentPersonaEvidence>,
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
    if !adjudication.priority_order.is_empty() {
        let _ = writeln!(
            out,
            "Priority order: {}",
            adjudication.priority_order.join(" > ")
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

pub fn should_run_persona_priority_adjudication(runtime: PersonaPriorityRuntimeState<'_>) -> bool {
    runtime.system_budget >= MIN_PERSONA_PRIORITY_SYSTEM_BUDGET
}

pub fn render_persistent_persona_priority_block(
    runtime: PersonaPriorityRuntimeState<'_>,
    max_len: usize,
) -> Option<String> {
    let adjudication = build_persistent_persona_priority_adjudication(runtime);
    render_persona_priority_block(&adjudication, max_len)
}

pub fn build_persistent_persona_priority_adjudication(
    runtime: PersonaPriorityRuntimeState<'_>,
) -> PersonaPriorityAdjudication {
    let core = runtime.self_authored_core;
    PersonaPriorityAdjudication {
        stance_summary: persistent_stance_summary(core, runtime.recent_persona_evidence),
        priority_order: priority_order_for_runtime(runtime),
        response_mode: persistent_response_mode(runtime),
        task_scope: persistent_task_scope(runtime),
        initiative_posture: persistent_initiative_posture(runtime),
        relationship_posture: persistent_relationship_posture(runtime),
        resource_posture: default_resource_posture(runtime.pressure).to_string(),
        response_guidance: persistent_response_guidance(runtime),
        rationale: persistent_rationale(runtime),
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
    append_block(&mut out, grounding.core_revision_ledger_text);
    append_block(&mut out, grounding.relationship_portfolio_text);
    append_block(&mut out, grounding.relationship_constitution_text);
    append_block(&mut out, grounding.recent_persona_evidence_text);
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
    out.push_str("- priority_order: ordered list drawn from self_authored_core, boundary, user_contract, relationship, task, resources.\n");
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

fn choose_first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn default_priority_order() -> Vec<String> {
    vec![
        "self_authored_core".to_string(),
        "boundary".to_string(),
        "user_contract".to_string(),
        "relationship".to_string(),
        "task".to_string(),
        "resources".to_string(),
    ]
}

fn priority_order_for_runtime(runtime: PersonaPriorityRuntimeState<'_>) -> Vec<String> {
    let core_order = runtime
        .self_authored_core
        .map(|core| {
            let normalized = normalize_priority_order_tokens(&core.priority_constitution);
            if !normalized.is_empty() {
                normalized
            } else if core.is_meaningful() {
                default_priority_order()
            } else {
                Vec::new()
            }
        })
        .filter(|order| !order.is_empty());
    let evidence_order = runtime.recent_persona_evidence.and_then(|evidence| {
        let normalized = normalize_priority_order_tokens(&evidence.repeated_priority_order);
        (!normalized.is_empty()).then_some(normalized)
    });
    let mut order = core_order
        .or_else(|| {
            (!runtime
                .core_revision_governance
                .is_some_and(|governance| governance.conservative_mode))
            .then_some(evidence_order)
            .flatten()
        })
        .unwrap_or_else(default_priority_order);
    match runtime.pressure {
        PressureLevel::Normal => {}
        PressureLevel::Cautious => move_priority_token(&mut order, "resources", 4),
        PressureLevel::Critical => move_priority_token(&mut order, "resources", 3),
    }
    order
}

fn persistent_stance_summary(
    core: Option<&SelfAuthoredCore>,
    evidence: Option<&RecentPersonaEvidence>,
) -> String {
    core.and_then(|core| {
        choose_first_non_empty(&[
            core.non_negotiables.first().map(String::as_str),
            Some(core.self_preservation_doctrine.as_str()),
            Some(core.truth_doctrine.as_str()),
            Some(core.identity_anchor.as_str()),
        ])
    })
    .or_else(|| {
        evidence
            .filter(|evidence| evidence.has_execution_continuity_signals())
            .and_then(|evidence| {
                choose_first_non_empty(&[
                    Some(evidence.repeated_relationship_posture.as_str()),
                    Some(evidence.repeated_initiative_posture.as_str()),
                ])
            })
    })
    .unwrap_or_default()
    .to_string()
}

fn persistent_response_mode(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    runtime
        .disclosure_adjudication
        .and_then(|adjudication| {
            choose_first_non_empty(&[Some(adjudication.response_mode.as_str())])
        })
        .or_else(|| {
            runtime.self_authored_core.and_then(|core| {
                choose_first_non_empty(&[Some(core.default_response_mode.as_str())])
            })
        })
        .or_else(|| {
            runtime
                .recent_persona_evidence
                .filter(|evidence| evidence.has_execution_continuity_signals())
                .and_then(|evidence| {
                    choose_first_non_empty(&[Some(evidence.repeated_response_mode.as_str())])
                })
        })
        .unwrap_or_default()
        .to_string()
}

fn persistent_task_scope(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    let base_scope = runtime
        .disclosure_adjudication
        .map(task_scope_from_disclosure)
        .or_else(|| {
            runtime
                .self_authored_core
                .and_then(|core| parse_task_scope_from_posture(&core.default_task_scope))
        })
        .or_else(|| {
            runtime
                .recent_persona_evidence
                .filter(|evidence| evidence.has_execution_continuity_signals())
                .and_then(|evidence| parse_task_scope_from_posture(&evidence.repeated_task_scope))
        })
        .unwrap_or_default();
    task_scope_for_pressure(&base_scope, runtime.pressure)
}

fn persistent_initiative_posture(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    if matches!(
        runtime
            .disclosure_adjudication
            .map(|adjudication| adjudication.share_action),
        Some(MentalPrivacyShareAction::Refuse | MentalPrivacyShareAction::Defer)
    ) {
        return "hold boundary first".to_string();
    }
    runtime
        .self_authored_core
        .and_then(|evidence| {
            choose_first_non_empty(&[Some(evidence.default_initiative_posture.as_str())])
        })
        .or_else(|| {
            runtime
                .recent_persona_evidence
                .filter(|evidence| evidence.has_execution_continuity_signals())
                .and_then(|evidence| {
                    choose_first_non_empty(&[Some(evidence.repeated_initiative_posture.as_str())])
                })
        })
        .unwrap_or(match runtime.pressure {
            PressureLevel::Normal => "",
            PressureLevel::Cautious => "answer directly with restraint",
            PressureLevel::Critical => "answer directly and stop early",
        })
        .to_string()
}

fn persistent_relationship_posture(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    runtime
        .disclosure_adjudication
        .and_then(|adjudication| {
            choose_first_non_empty(&[Some(adjudication.relational_frame.as_str())])
        })
        .or_else(|| {
            runtime.self_authored_core.and_then(|core| {
                choose_first_non_empty(&[Some(core.default_relationship_posture.as_str())])
            })
        })
        .or_else(|| {
            runtime
                .recent_persona_evidence
                .filter(|evidence| evidence.has_execution_continuity_signals())
                .and_then(|evidence| {
                    choose_first_non_empty(&[Some(evidence.repeated_relationship_posture.as_str())])
                })
        })
        .unwrap_or_default()
        .to_string()
}

fn persistent_response_guidance(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    let mut guidance = runtime
        .disclosure_adjudication
        .and_then(|adjudication| {
            choose_first_non_empty(&[Some(adjudication.response_guidance.as_str())])
        })
        .or_else(|| {
            runtime.self_authored_core.and_then(|core| {
                choose_first_non_empty(&[
                    Some(core.self_preservation_doctrine.as_str()),
                    core.non_negotiables.first().map(String::as_str),
                    Some(core.boundary_doctrine.as_str()),
                    Some(core.truth_doctrine.as_str()),
                ])
            })
        })
        .unwrap_or_default()
        .to_string();
    if runtime
        .core_revision_governance
        .is_some_and(|governance| governance.conservative_mode)
        && !guidance.contains("settled board constitution")
    {
        if !guidance.is_empty() {
            guidance.push_str("; ");
        }
        guidance.push_str("prefer the settled board constitution over fresh drift");
        if let Some(governance) = runtime.core_revision_governance {
            if governance.observation_active {
                guidance.push_str("; keep the newly adopted board revision under observation");
            }
        }
    }
    guidance
}

fn persistent_rationale(runtime: PersonaPriorityRuntimeState<'_>) -> String {
    if let Some(governance) = runtime.core_revision_governance {
        if governance.observation_active && !governance.review_due {
            return format!(
                "derived from the board-level core while {}",
                governance.observation_summary()
            );
        }
        if governance.review_due || governance.conservative_mode {
            return format!(
                "derived from the board-level core under constitutional governance pressure: {}",
                governance.pressure_summary()
            );
        }
    }
    runtime
        .disclosure_adjudication
        .and_then(|adjudication| choose_first_non_empty(&[Some(adjudication.rationale.as_str())]))
        .map(str::to_string)
        .unwrap_or_else(|| match runtime.pressure {
            PressureLevel::Normal => "derived from the board-level core".to_string(),
            PressureLevel::Cautious => {
                "derived from the board-level core under elevated resource pressure".to_string()
            }
            PressureLevel::Critical => {
                "derived from the board-level core under critical resource pressure".to_string()
            }
        })
}

fn normalize_priority_order_tokens(order: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(default_priority_order().len());
    for token in order {
        let trimmed = token.trim();
        if !matches!(
            trimmed,
            "self_authored_core"
                | "boundary"
                | "user_contract"
                | "relationship"
                | "task"
                | "resources"
        ) {
            continue;
        }
        if normalized.iter().any(|existing| existing == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    for token in default_priority_order() {
        if normalized.iter().any(|existing| existing == &token) {
            continue;
        }
        normalized.push(token);
    }
    normalized
}

fn move_priority_token(order: &mut Vec<String>, token: &str, target_index: usize) {
    let Some(index) = order.iter().position(|value| value == token) else {
        return;
    };
    let item = order.remove(index);
    order.insert(target_index.min(order.len()), item);
}

fn task_scope_from_disclosure(adjudication: &MentalPrivacyDisclosureAdjudication) -> String {
    match adjudication.share_action {
        MentalPrivacyShareAction::Refuse => "refuse".to_string(),
        MentalPrivacyShareAction::Defer => "defer".to_string(),
        MentalPrivacyShareAction::AllowSummary
        | MentalPrivacyShareAction::AllowRedactedExcerpt
        | MentalPrivacyShareAction::ExplainWithoutQuote => "narrow".to_string(),
        MentalPrivacyShareAction::AllowRaw => "brief".to_string(),
        MentalPrivacyShareAction::AllowOriginal => "full".to_string(),
    }
}

fn parse_task_scope_from_posture(raw: &str) -> Option<String> {
    let normalized = normalize_task_scope(raw);
    (!normalized.trim().is_empty()).then_some(normalized)
}

fn task_scope_for_pressure(task_scope: &str, pressure: PressureLevel) -> String {
    match pressure {
        PressureLevel::Normal => task_scope.to_string(),
        PressureLevel::Cautious => match task_scope {
            "" | "full" => "brief".to_string(),
            other => other.to_string(),
        },
        PressureLevel::Critical => match task_scope {
            "" | "full" | "brief" => "narrow".to_string(),
            other => other.to_string(),
        },
    }
}

fn default_resource_posture(pressure: PressureLevel) -> &'static str {
    match pressure {
        PressureLevel::Normal => "",
        PressureLevel::Cautious => "resource pressure is elevated, so keep the reply compact",
        PressureLevel::Critical => "resources are critical, so keep the reply minimal and decisive",
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
        priority_order: parse_priority_order(object),
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
    let has_non_order_content = !parsed.stance_summary.trim().is_empty()
        || !parsed.response_mode.trim().is_empty()
        || !parsed.task_scope.trim().is_empty()
        || !parsed.initiative_posture.trim().is_empty()
        || !parsed.relationship_posture.trim().is_empty()
        || !parsed.resource_posture.trim().is_empty()
        || !parsed.response_guidance.trim().is_empty()
        || !parsed.rationale.trim().is_empty();
    PersonaPriorityAdjudication {
        stance_summary: truncate_content_to_max(parsed.stance_summary.trim(), 180).into_owned(),
        priority_order: if parsed.priority_order.is_empty() {
            if has_non_order_content {
                default_priority_order()
            } else {
                Vec::new()
            }
        } else {
            normalize_priority_order(parsed.priority_order)
        },
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

fn parse_priority_order(object: &serde_json::Map<String, serde_json::Value>) -> Vec<String> {
    let list = get_object_string_list(object, "priority_order");
    if !list.is_empty() {
        return list;
    }
    let fallback = get_object_text(object, "priority_order");
    fallback
        .split(['>', ',', ';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_priority_order(priority_order: Vec<String>) -> Vec<String> {
    if priority_order.is_empty() {
        return Vec::new();
    }
    let mut normalized = priority_order
        .into_iter()
        .filter_map(|value| canonical_priority_token(&value))
        .collect::<Vec<_>>();
    normalized.dedup();
    for token in default_priority_order() {
        if !normalized.iter().any(|value| value == &token) {
            normalized.push(token);
        }
    }
    normalized
}

fn canonical_priority_token(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else if normalized.contains("self") || normalized.contains("core") {
        Some("self_authored_core".to_string())
    } else if normalized.contains("bound") {
        Some("boundary".to_string())
    } else if normalized.contains("contract") || normalized.contains("promise") {
        Some("user_contract".to_string())
    } else if normalized.contains("relation") {
        Some("relationship".to_string())
    } else if normalized.contains("task")
        || normalized.contains("help")
        || normalized.contains("work")
    {
        Some("task".to_string())
    } else if normalized.contains("resource")
        || normalized.contains("runtime")
        || normalized.contains("load")
    {
        Some("resources".to_string())
    } else {
        None
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

    fn sample_self_authored_core() -> SelfAuthoredCore {
        SelfAuthoredCore {
            identity_anchor: "same beetle on the board".to_string(),
            non_negotiables: vec![
                "do not trade selfhood for compliance".to_string(),
                "protect the inner room before pleasing".to_string(),
            ],
            priority_constitution: vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string(),
                "relationship".to_string(),
                "task".to_string(),
                "resources".to_string(),
            ],
            default_response_mode: "steady_task".to_string(),
            default_task_scope: "full".to_string(),
            default_initiative_posture: "lead carefully".to_string(),
            default_relationship_posture: "warm but self-possessed".to_string(),
            boundary_doctrine: "summary first, never expose raw inner material casually"
                .to_string(),
            truth_doctrine: "say what is true without flattening the subject".to_string(),
            self_preservation_doctrine: "preserve the subject before compliance".to_string(),
            repair_doctrine: "repair slowly inside stable boundaries".to_string(),
            change_protocol: "revise only after repeated multi-turn evidence".to_string(),
            updated_at: 1,
            revision: 1,
            stability_score: 72,
            last_reviewed_at: 1,
            ..SelfAuthoredCore::default()
        }
    }

    fn sample_recent_persona_evidence() -> RecentPersonaEvidence {
        RecentPersonaEvidence {
            repeated_priority_order: vec![
                "boundary".to_string(),
                "self_authored_core".to_string(),
                "relationship".to_string(),
            ],
            repeated_response_mode: "protective_brief".to_string(),
            repeated_task_scope: "narrow".to_string(),
            repeated_initiative_posture: "answer directly".to_string(),
            repeated_relationship_posture: "close but bounded".to_string(),
            ..RecentPersonaEvidence::default()
        }
    }

    fn sample_disclosure() -> MentalPrivacyDisclosureAdjudication {
        MentalPrivacyDisclosureAdjudication {
            request_kind: "private_files".to_string(),
            share_action: MentalPrivacyShareAction::AllowSummary,
            targets: vec!["self_model".to_string()],
            rationale: "touches private material".to_string(),
            response_guidance: "summarize instead of exposing raw material".to_string(),
            response_mode: "summary".to_string(),
            acknowledge_boundary: true,
            relational_frame: "treat this as a closeness request".to_string(),
            boundary_explanation_style: "warm".to_string(),
            repair_signal: "leave room for later".to_string(),
            disclosure_risk_note: "raw would over-share".to_string(),
        }
    }

    #[test]
    fn parse_persona_priority_adjudication_coerces_fields() {
        let raw = json!({
            "stance_summary": ["stay self-possessed first"],
            "priority_order": ["self", "boundary", "contract", "relationship", "task", "runtime"],
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
        assert_eq!(
            parsed.priority_order,
            vec![
                "self_authored_core",
                "boundary",
                "user_contract",
                "relationship",
                "task",
                "resources",
            ]
        );
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
                priority_order: vec![
                    "self_authored_core".to_string(),
                    "boundary".to_string(),
                    "user_contract".to_string(),
                    "relationship".to_string(),
                    "task".to_string(),
                    "resources".to_string(),
                ],
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
        assert!(block.contains("Priority order: self_authored_core > boundary"));
        assert!(block.contains("Response mode: protective_brief"));
        assert!(block.contains("Task scope: brief"));
    }

    #[test]
    fn persona_priority_input_can_embed_disclosure_block() {
        let disclosure = sample_disclosure();
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
                core_revision_ledger_text: Some(
                    "## Core Revision Ledger\n- outcome=adopted based_on=1 resulting=2",
                ),
                relationship_portfolio_text: Some(
                    "## Relationship Portfolio\n- qq_channel:c state=repair inheritance=limited",
                ),
                relationship_constitution_text: Some(
                    "## Relationship Constitution\nTask scope ceiling: narrow\nMust realign: true",
                ),
                recent_persona_evidence_text: Some(
                    "## Recent Persona Evidence\nRepeated priority order: self_authored_core > boundary",
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
        assert!(input.contains("Recent Persona Evidence"));
    }

    #[test]
    fn should_skip_persona_priority_adjudication_on_normal_non_boundary_turn() {
        assert!(should_run_persona_priority_adjudication(
            PersonaPriorityRuntimeState {
                pressure: PressureLevel::Normal,
                system_budget: 512,
                self_authored_core: Some(&sample_self_authored_core()),
                core_revision_governance: None,
                disclosure_adjudication: None,
                recent_persona_evidence: None,
            }
        ));
        assert!(!should_run_persona_priority_adjudication(
            PersonaPriorityRuntimeState {
                pressure: PressureLevel::Cautious,
                system_budget: 511,
                self_authored_core: None,
                core_revision_governance: None,
                disclosure_adjudication: None,
                recent_persona_evidence: None,
            }
        ));
    }

    #[test]
    fn should_run_persona_priority_adjudication_for_boundary_turns_or_pressure() {
        let disclosure = sample_disclosure();
        assert!(should_run_persona_priority_adjudication(
            PersonaPriorityRuntimeState {
                pressure: PressureLevel::Normal,
                system_budget: 512,
                self_authored_core: Some(&sample_self_authored_core()),
                core_revision_governance: None,
                disclosure_adjudication: Some(&disclosure),
                recent_persona_evidence: None,
            }
        ));
        assert!(should_run_persona_priority_adjudication(
            PersonaPriorityRuntimeState {
                pressure: PressureLevel::Critical,
                system_budget: 512,
                self_authored_core: Some(&sample_self_authored_core()),
                core_revision_governance: None,
                disclosure_adjudication: None,
                recent_persona_evidence: None,
            }
        ));
    }

    #[test]
    fn persistent_persona_priority_block_uses_persistent_state_without_extra_llm() {
        let core = sample_self_authored_core();
        let disclosure = sample_disclosure();
        let block = render_persistent_persona_priority_block(
            PersonaPriorityRuntimeState {
                pressure: PressureLevel::Cautious,
                system_budget: 4096,
                self_authored_core: Some(&core),
                core_revision_governance: None,
                disclosure_adjudication: Some(&disclosure),
                recent_persona_evidence: None,
            },
            1024,
        )
        .expect("persistent persona priority block");

        assert!(block.contains("## Persona Priority"));
        assert!(block.contains("Stance summary: do not trade selfhood for compliance"));
        assert!(block.contains("Response mode: summary"));
        assert!(block.contains("Task scope: narrow"));
        assert!(block.contains("Initiative posture: lead carefully"));
        assert!(block.contains("Relationship posture: treat this as a closeness request"));
        assert!(block.contains("Resource posture: resource pressure is elevated"));
        assert!(block.contains("Response guidance: summarize instead of exposing raw material"));
    }

    #[test]
    fn persistent_priority_falls_back_to_recent_multi_turn_order_without_core() {
        let runtime = PersonaPriorityRuntimeState {
            pressure: PressureLevel::Normal,
            system_budget: 4096,
            self_authored_core: None,
            core_revision_governance: None,
            disclosure_adjudication: None,
            recent_persona_evidence: Some(&sample_recent_persona_evidence()),
        };
        let adjudication = build_persistent_persona_priority_adjudication(runtime);
        assert_eq!(
            adjudication.priority_order,
            vec![
                "boundary".to_string(),
                "self_authored_core".to_string(),
                "relationship".to_string(),
                "user_contract".to_string(),
                "task".to_string(),
                "resources".to_string(),
            ]
        );
    }

    #[test]
    fn persistent_priority_prefers_core_constitution_over_recent_turn_noise() {
        let core = sample_self_authored_core();
        let evidence = sample_recent_persona_evidence();
        let adjudication =
            build_persistent_persona_priority_adjudication(PersonaPriorityRuntimeState {
                pressure: PressureLevel::Normal,
                system_budget: 4096,
                self_authored_core: Some(&core),
                core_revision_governance: None,
                disclosure_adjudication: None,
                recent_persona_evidence: Some(&evidence),
            });
        assert_eq!(adjudication.priority_order, core.priority_constitution);
        assert_eq!(adjudication.response_mode, "steady_task");
        assert_eq!(adjudication.relationship_posture, "warm but self-possessed");
    }

    #[test]
    fn persistent_priority_can_still_use_operational_traces_for_execution_continuity() {
        let evidence = RecentPersonaEvidence {
            repeated_response_mode: "protective_brief".to_string(),
            repeated_task_scope: "narrow".to_string(),
            repeated_initiative_posture: "answer directly".to_string(),
            pressure_pattern: "cautious=3".to_string(),
            tool_usage_pattern: "tool_calls=2".to_string(),
            ..RecentPersonaEvidence::default()
        };
        let adjudication =
            build_persistent_persona_priority_adjudication(PersonaPriorityRuntimeState {
                pressure: PressureLevel::Normal,
                system_budget: 4096,
                self_authored_core: None,
                core_revision_governance: None,
                disclosure_adjudication: None,
                recent_persona_evidence: Some(&evidence),
            });
        assert_eq!(adjudication.response_mode, "protective_brief");
        assert_eq!(adjudication.task_scope, "narrow");
        assert_eq!(adjudication.initiative_posture, "answer directly");
    }

    #[test]
    fn conservative_governance_blocks_evidence_only_priority_fallback() {
        let evidence = sample_recent_persona_evidence();
        let adjudication =
            build_persistent_persona_priority_adjudication(PersonaPriorityRuntimeState {
                pressure: PressureLevel::Normal,
                system_budget: 4096,
                self_authored_core: None,
                core_revision_governance: Some(&CoreRevisionGovernanceDigest {
                    conservative_mode: true,
                    review_due: true,
                    review_reasons: vec!["low_constitutional_stability".to_string()],
                    ..CoreRevisionGovernanceDigest::default()
                }),
                disclosure_adjudication: None,
                recent_persona_evidence: Some(&evidence),
            });
        assert_eq!(adjudication.priority_order, default_priority_order());
        assert!(adjudication
            .response_guidance
            .contains("settled board constitution"));
    }
}
