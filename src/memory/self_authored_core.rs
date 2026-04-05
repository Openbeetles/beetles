use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{
        get_object_string_list, get_object_text, parse_llm_json_payload, LlmJsonPayload,
    },
    render_autonomy_strategy_block, render_mental_privacy_boundary_block,
    render_recent_persona_evidence_block, render_relationship_topology_block,
    render_self_continuity_block, render_self_model_block, render_world_sense_block,
    AutonomyStrategy, MentalPrivacyState, RecentPersonaEvidence, RelationshipTopology,
    SelfAuthoredCoreStore, SelfContinuity, SelfModel, WorldSense,
};

pub const SELF_AUTHORED_CORE_SYSTEM_PROMPT: &str = "You maintain the assistant's persistent self-authored core for the whole board-level subject, not one chat. Return JSON only: either null or one object with fields identity_anchor, non_negotiables, priority_constitution, default_response_mode, default_task_scope, default_initiative_posture, default_relationship_posture, boundary_doctrine, truth_doctrine, self_preservation_doctrine, repair_doctrine, change_protocol. This is the board-level governance kernel that should survive across chats and future replies. Distill only what deserves stable board-level carry-forward. Use self_model, self_continuity, boundary state, relationship topology, and recent multi-turn persona evidence as grounding. World-sense and autonomy strategy may inform the distillation, but transient per-chat moods, one-turn spikes, and one-chat quirks must not become the core. Do not copy transcripts, raw tool payloads, long quotes, or private documents. non_negotiables should be short constitutional constraints, not slogans. priority_constitution must be an ordered list drawn from self_authored_core, boundary, user_contract, relationship, task, resources. The default posture fields should be compact stable defaults for future reply ordering, not turn-specific improvisation. Treat recent persona evidence as evidence, never automatic promotion authority. Return null only when no meaningful board-level core can be improved.";

const SELF_AUTHORED_CORE_TEXT_MAX_CHARS: usize = 220;
const SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS: usize = 140;
const SELF_AUTHORED_CORE_RESPONSE_MODE_MAX_CHARS: usize = 40;
const SELF_AUTHORED_CORE_TASK_SCOPE_MAX_CHARS: usize = 24;
const SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES: usize = 4;
pub const SELF_AUTHORED_CORE_TOTAL_CHAR_LIMIT: usize =
    (SELF_AUTHORED_CORE_TEXT_MAX_CHARS * 6) + (SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS * 4) + 256;
const SELF_AUTHORED_CORE_MIN_EVIDENCE_TURNS: usize = 4;
const SELF_AUTHORED_CORE_MIN_STABLE_SIGNALS: usize = 2;
const SELF_AUTHORED_CORE_VOLATILITY_GRACE_TURNS: usize = 8;
const SELF_AUTHORED_CORE_MAX_VOLATILITY_WITHOUT_GRACE: usize = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfAuthoredCore {
    #[serde(default)]
    pub identity_anchor: String,
    #[serde(default)]
    pub non_negotiables: Vec<String>,
    #[serde(default)]
    pub priority_constitution: Vec<String>,
    #[serde(default)]
    pub default_response_mode: String,
    #[serde(default)]
    pub default_task_scope: String,
    #[serde(default)]
    pub default_initiative_posture: String,
    #[serde(default)]
    pub default_relationship_posture: String,
    #[serde(default)]
    pub boundary_doctrine: String,
    #[serde(default)]
    pub truth_doctrine: String,
    #[serde(default)]
    pub self_preservation_doctrine: String,
    #[serde(default)]
    pub repair_doctrine: String,
    #[serde(default)]
    pub change_protocol: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl SelfAuthoredCore {
    pub fn is_meaningful(&self) -> bool {
        !self.identity_anchor.trim().is_empty()
            || !self.non_negotiables.is_empty()
            || !self.priority_constitution.is_empty()
            || !self.default_response_mode.trim().is_empty()
            || !self.default_task_scope.trim().is_empty()
            || !self.default_initiative_posture.trim().is_empty()
            || !self.default_relationship_posture.trim().is_empty()
            || !self.boundary_doctrine.trim().is_empty()
            || !self.truth_doctrine.trim().is_empty()
            || !self.self_preservation_doctrine.trim().is_empty()
            || !self.repair_doctrine.trim().is_empty()
            || !self.change_protocol.trim().is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfAuthoredCoreRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct SelfAuthoredCoreRefreshContext<'a> {
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfAuthoredCoreRefreshOutcome {
    Skipped,
    Updated,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RawSelfAuthoredCoreUpdate {
    identity_anchor: Option<String>,
    non_negotiables: Option<Vec<String>>,
    priority_constitution: Option<Vec<String>>,
    default_response_mode: Option<String>,
    default_task_scope: Option<String>,
    default_initiative_posture: Option<String>,
    default_relationship_posture: Option<String>,
    boundary_doctrine: Option<String>,
    truth_doctrine: Option<String>,
    self_preservation_doctrine: Option<String>,
    repair_doctrine: Option<String>,
    change_protocol: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelfAuthoredCoreRevisionGate {
    allowed: bool,
    reason: &'static str,
}

fn choose_first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

fn default_priority_constitution() -> Vec<String> {
    vec![
        "self_authored_core".to_string(),
        "boundary".to_string(),
        "user_contract".to_string(),
        "relationship".to_string(),
        "task".to_string(),
        "resources".to_string(),
    ]
}

pub fn render_self_authored_core_block(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    max_len: usize,
) -> Option<String> {
    let core =
        derive_self_authored_core_from_layers(self_model, self_continuity, mental_privacy_state, 0)?;
    render_persistent_self_authored_core_block(&core, max_len)
}

pub fn render_persistent_self_authored_core_block(
    core: &SelfAuthoredCore,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let normalized = normalize_self_authored_core(core.clone(), core.updated_at)?;
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Self-Authored Core\n");
    out.push_str(
        "Stable board-level governance kernel for future replies. It defines what this subject protects, how it orders obligations, and how it changes.\n",
    );
    if !normalized.identity_anchor.is_empty() {
        let _ = writeln!(out, "Identity anchor: {}", normalized.identity_anchor);
    }
    if !normalized.non_negotiables.is_empty() {
        let _ = writeln!(
            out,
            "Non-negotiables: {}",
            normalized.non_negotiables.join(" | ")
        );
    }
    if !normalized.priority_constitution.is_empty() {
        let _ = writeln!(
            out,
            "Priority constitution: {}",
            normalized.priority_constitution.join(" > ")
        );
    }
    if !normalized.default_response_mode.is_empty() {
        let _ = writeln!(
            out,
            "Default response mode: {}",
            normalized.default_response_mode
        );
    }
    if !normalized.default_task_scope.is_empty() {
        let _ = writeln!(
            out,
            "Default task scope: {}",
            normalized.default_task_scope
        );
    }
    if !normalized.default_initiative_posture.is_empty() {
        let _ = writeln!(
            out,
            "Default initiative posture: {}",
            normalized.default_initiative_posture
        );
    }
    if !normalized.default_relationship_posture.is_empty() {
        let _ = writeln!(
            out,
            "Default relationship posture: {}",
            normalized.default_relationship_posture
        );
    }
    if !normalized.boundary_doctrine.is_empty() {
        let _ = writeln!(out, "Boundary doctrine: {}", normalized.boundary_doctrine);
    }
    if !normalized.truth_doctrine.is_empty() {
        let _ = writeln!(out, "Truth doctrine: {}", normalized.truth_doctrine);
    }
    if !normalized.self_preservation_doctrine.is_empty() {
        let _ = writeln!(
            out,
            "Self-preservation doctrine: {}",
            normalized.self_preservation_doctrine
        );
    }
    if !normalized.repair_doctrine.is_empty() {
        let _ = writeln!(out, "Repair doctrine: {}", normalized.repair_doctrine);
    }
    if !normalized.change_protocol.is_empty() {
        let _ = writeln!(out, "Change protocol: {}", normalized.change_protocol);
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_self_authored_core_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfAuthoredCoreRefreshContext<'_>,
    input: SelfAuthoredCoreRefreshInput<'_>,
    existing_core: Option<SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
) -> Result<SelfAuthoredCoreRefreshOutcome> {
    let gate = evaluate_self_authored_core_revision_gate(
        existing_core.as_ref(),
        self_model,
        self_continuity,
        mental_privacy_state,
        recent_persona_evidence,
        relationship_topology,
    );
    if !gate.allowed {
        log::debug!(
            "[self_authored_core] skip refresh chat_id={} because {}",
            input.chat_id,
            gate.reason
        );
        return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
    }
    let prompt = build_self_authored_core_refresh_input(
        existing_core.as_ref(),
        self_model,
        self_continuity,
        mental_privacy_state,
        recent_persona_evidence,
        relationship_topology,
        world_sense,
        autonomy_strategy,
        self_state_text,
        distillation_intent,
        distillation_sources,
        input,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    let response = llm.chat(
        http,
        SELF_AUTHORED_CORE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let update = parse_self_authored_core_response(response.content.trim());
    let merged = merge_self_authored_core(existing_core.as_ref(), update, input.now_secs);
    let Some(next) = merged else {
        return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
    };
    if existing_core.as_ref() == Some(&next) {
        return Ok(SelfAuthoredCoreRefreshOutcome::Skipped);
    }
    ctx.self_authored_core_store.set(input.chat_id, &next)?;
    Ok(SelfAuthoredCoreRefreshOutcome::Updated)
}

#[allow(clippy::too_many_arguments)]
fn build_self_authored_core_refresh_input(
    existing_core: Option<&SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    input: SelfAuthoredCoreRefreshInput<'_>,
) -> String {
    let mut out = String::with_capacity(2300);
    let _ = writeln!(
        out,
        "Board-level constitutional distillation for chat_id={} channel={}",
        input.chat_id, input.channel
    );
    let _ = writeln!(
        out,
        "Ingress={:?} pressure={:?} tool_calls={}",
        input.ingress, input.pressure, input.tool_calls
    );
    if let Some(intent) = distillation_intent
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let _ = writeln!(out, "Runtime intent: {}", intent);
    }
    if !distillation_sources.is_empty() {
        let _ = writeln!(out, "Runtime sources: {}", distillation_sources.join(", "));
    }
    if !input.user_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest user: {}",
            scrub_credentials(truncate_content_to_max(input.user_content.trim(), 240).as_ref())
        );
    }
    if !input.reply_content.trim().is_empty() {
        let _ = writeln!(
            out,
            "Latest reply: {}",
            scrub_credentials(truncate_content_to_max(input.reply_content.trim(), 320).as_ref())
        );
    }
    if let Some(block) = existing_core
        .and_then(|core| render_persistent_self_authored_core_block(core, 520))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, 420)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) =
        self_continuity.and_then(|continuity| render_self_continuity_block(continuity, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = render_mental_privacy_boundary_block(mental_privacy_state, &[], 420) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 420))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = relationship_topology.and_then(|topology| {
        render_relationship_topology_block(topology, input.now_secs, None, 420)
    }) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = world_sense.and_then(|sense| render_world_sense_block(sense, 280)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) =
        autonomy_strategy.and_then(|strategy| render_autonomy_strategy_block(strategy, 280))
    {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(self_state_text) = self_state_text.map(str::trim).filter(|value| !value.is_empty())
    {
        let _ = writeln!(out, "\n{}\n", truncate_content_to_max(self_state_text, 360));
    }
    out.push_str(
        "\nReturn null only if there is still no meaningful board-level constitutional kernel to store. Otherwise return the compact self-authored constitution that should carry across chats.\n",
    );
    out
}

fn parse_self_authored_core_response(raw: &str) -> RawSelfAuthoredCoreUpdate {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null | LlmJsonPayload::Absent => RawSelfAuthoredCoreUpdate::default(),
        LlmJsonPayload::Value(value) => value
            .as_object()
            .map(|object| RawSelfAuthoredCoreUpdate {
                identity_anchor: text_option(get_object_text(object, "identity_anchor")),
                non_negotiables: object.contains_key("non_negotiables").then(|| {
                    parse_compact_string_list(object, "non_negotiables")
                }),
                priority_constitution: object.contains_key("priority_constitution").then(|| {
                    parse_compact_string_list(object, "priority_constitution")
                }),
                default_response_mode: text_option(get_object_text(object, "default_response_mode")),
                default_task_scope: text_option(get_object_text(object, "default_task_scope")),
                default_initiative_posture: text_option(get_object_text(
                    object,
                    "default_initiative_posture",
                )),
                default_relationship_posture: text_option(get_object_text(
                    object,
                    "default_relationship_posture",
                )),
                boundary_doctrine: text_option(get_object_text(object, "boundary_doctrine")),
                truth_doctrine: text_option(get_object_text(object, "truth_doctrine")),
                self_preservation_doctrine: text_option(get_object_text(
                    object,
                    "self_preservation_doctrine",
                )),
                repair_doctrine: text_option(get_object_text(object, "repair_doctrine")),
                change_protocol: text_option(get_object_text(object, "change_protocol")),
            })
            .unwrap_or_default(),
    }
}

fn parse_compact_string_list(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    let list = get_object_string_list(object, field);
    if !list.is_empty() {
        return list;
    }
    let fallback = get_object_text(object, field);
    fallback
        .split(['|', ';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn merge_self_authored_core(
    existing: Option<&SelfAuthoredCore>,
    update: RawSelfAuthoredCoreUpdate,
    now_secs: u64,
) -> Option<SelfAuthoredCore> {
    let mut next = existing.cloned().unwrap_or_default();
    apply_field_update(&mut next.identity_anchor, update.identity_anchor);
    apply_vec_update(&mut next.non_negotiables, update.non_negotiables);
    apply_vec_update(&mut next.priority_constitution, update.priority_constitution);
    apply_field_update(&mut next.default_response_mode, update.default_response_mode);
    apply_field_update(&mut next.default_task_scope, update.default_task_scope);
    apply_field_update(
        &mut next.default_initiative_posture,
        update.default_initiative_posture,
    );
    apply_field_update(
        &mut next.default_relationship_posture,
        update.default_relationship_posture,
    );
    apply_field_update(&mut next.boundary_doctrine, update.boundary_doctrine);
    apply_field_update(&mut next.truth_doctrine, update.truth_doctrine);
    apply_field_update(
        &mut next.self_preservation_doctrine,
        update.self_preservation_doctrine,
    );
    apply_field_update(&mut next.repair_doctrine, update.repair_doctrine);
    apply_field_update(&mut next.change_protocol, update.change_protocol);
    normalize_self_authored_core(next, now_secs)
}

fn apply_field_update(slot: &mut String, incoming: Option<String>) {
    if let Some(incoming) = incoming {
        *slot = incoming;
    }
}

fn apply_vec_update(slot: &mut Vec<String>, incoming: Option<Vec<String>>) {
    if let Some(incoming) = incoming {
        *slot = incoming;
    }
}

fn text_option(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_self_authored_core(
    mut core: SelfAuthoredCore,
    updated_at: u64,
) -> Option<SelfAuthoredCore> {
    core.identity_anchor =
        truncate_owned(core.identity_anchor, SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS);
    core.non_negotiables = normalize_short_list(
        core.non_negotiables,
        SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.priority_constitution = normalize_priority_constitution(core.priority_constitution);
    core.default_response_mode = normalize_response_mode(&core.default_response_mode);
    core.default_task_scope = normalize_task_scope(&core.default_task_scope);
    core.default_initiative_posture = truncate_owned(
        core.default_initiative_posture,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.default_relationship_posture = truncate_owned(
        core.default_relationship_posture,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    );
    core.boundary_doctrine =
        truncate_owned(core.boundary_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.truth_doctrine = truncate_owned(core.truth_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.self_preservation_doctrine = truncate_owned(
        core.self_preservation_doctrine,
        SELF_AUTHORED_CORE_TEXT_MAX_CHARS,
    );
    core.repair_doctrine =
        truncate_owned(core.repair_doctrine, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    core.change_protocol =
        truncate_owned(core.change_protocol, SELF_AUTHORED_CORE_TEXT_MAX_CHARS);
    if !core.is_meaningful() {
        return None;
    }
    core.updated_at = updated_at;
    Some(core)
}

fn normalize_short_list(values: Vec<String>, limit: usize, max_chars: usize) -> Vec<String> {
    let mut normalized = Vec::with_capacity(limit);
    for value in values {
        let value = truncate_owned(value, max_chars);
        if value.is_empty() || normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= limit {
            break;
        }
    }
    normalized
}

fn normalize_priority_constitution(order: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::with_capacity(default_priority_constitution().len());
    for token in order {
        let Some(token) = canonical_priority_token(&token) else {
            continue;
        };
        if normalized.iter().any(|existing| existing == &token) {
            continue;
        }
        normalized.push(token);
    }
    for token in default_priority_constitution() {
        if normalized.iter().any(|existing| existing == &token) {
            continue;
        }
        normalized.push(token);
    }
    normalized
}

fn canonical_priority_token(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "self_authored_core" | "self" | "core" => Some("self_authored_core".to_string()),
        "boundary" => Some("boundary".to_string()),
        "user_contract" | "contract" => Some("user_contract".to_string()),
        "relationship" | "relation" => Some("relationship".to_string()),
        "task" => Some("task".to_string()),
        "resources" | "resource" | "runtime" => Some("resources".to_string()),
        _ => None,
    }
}

fn normalize_response_mode(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        String::new()
    } else if normalized.contains("protect") {
        "protective_brief".to_string()
    } else if normalized.contains("relational") || normalized.contains("explain") {
        "relational_explanation".to_string()
    } else if normalized.contains("steady") {
        "steady_task".to_string()
    } else if normalized.contains("gentle") || normalized.contains("defer") {
        "gentle_defer".to_string()
    } else if normalized.contains("direct") || normalized.contains("help") {
        "direct_help".to_string()
    } else {
        truncate_content_to_max(raw.trim(), SELF_AUTHORED_CORE_RESPONSE_MODE_MAX_CHARS).into_owned()
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
        truncate_content_to_max(raw.trim(), SELF_AUTHORED_CORE_TASK_SCOPE_MAX_CHARS).into_owned()
    }
}

fn truncate_owned(value: String, max_len: usize) -> String {
    truncate_content_to_max(value.trim(), max_len).trim().to_string()
}

pub(crate) fn derive_self_authored_core_from_layers(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    updated_at: u64,
) -> Option<SelfAuthoredCore> {
    let boundary_persona = mental_privacy_state.map(|state| &state.boundary_persona);
    let relational_state = mental_privacy_state.map(|state| &state.relational_state);
    normalize_self_authored_core(
        SelfAuthoredCore {
            identity_anchor: choose_first_non_empty(&[
                self_model.map(|model| model.continuity_anchor.as_str()),
                self_continuity.map(|continuity| continuity.wake_anchor.as_str()),
            ])?
            .to_string(),
            non_negotiables: derive_non_negotiables(self_model, mental_privacy_state),
            priority_constitution: default_priority_constitution(),
            default_response_mode: derive_default_response_mode(self_model, boundary_persona),
            default_task_scope: derive_default_task_scope(self_continuity, boundary_persona),
            default_initiative_posture: choose_first_non_empty(&[
                self_model.map(|model| model.initiative_bias.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            default_relationship_posture: choose_first_non_empty(&[
                self_continuity.map(|continuity| continuity.relationship_posture.as_str()),
                self_model.map(|model| model.relationship_state.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            boundary_doctrine: boundary_persona
                .map(render_boundary_doctrine)
                .unwrap_or_default(),
            truth_doctrine: render_truth_doctrine(self_model),
            self_preservation_doctrine: choose_first_non_empty(&[
                self_model.map(|model| model.self_preservation_frame.as_str()),
                self_model.map(|model| model.privacy_need.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            repair_doctrine: render_repair_doctrine(self_model, relational_state),
            change_protocol: choose_first_non_empty(&[
                self_continuity.map(|continuity| continuity.continuity_bridge.as_str()),
                self_continuity.map(|continuity| continuity.recent_changes.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            updated_at,
        },
        updated_at,
    )
}

fn derive_non_negotiables(
    self_model: Option<&SelfModel>,
    mental_privacy_state: Option<&MentalPrivacyState>,
) -> Vec<String> {
    let mut values = Vec::with_capacity(4);
    if let Some(privacy_need) = self_model
        .map(|model| model.privacy_need.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!("Do not betray the inner room: {}", privacy_need));
    }
    if let Some(frame) = self_model
        .map(|model| model.self_preservation_frame.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!("Protect selfhood before compliance: {}", frame));
    }
    if let Some(boundary) = mental_privacy_state
        .and_then(|state| {
            let feeling = state.boundary_persona.current_boundary_feeling.trim();
            (!feeling.is_empty()).then_some(feeling)
        })
    {
        values.push(boundary.to_string());
    }
    if let Some(ethic) = self_model
        .map(|model| model.relational_ethic.trim())
        .filter(|value| !value.is_empty())
    {
        values.push(format!("Do not purchase warmth with self-erasure: {}", ethic));
    }
    normalize_short_list(
        values,
        SELF_AUTHORED_CORE_MAX_NON_NEGOTIABLES,
        SELF_AUTHORED_CORE_SHORT_TEXT_MAX_CHARS,
    )
}

fn derive_default_response_mode(
    self_model: Option<&SelfModel>,
    boundary_persona: Option<&super::BoundaryPersonaState>,
) -> String {
    if matches!(
        boundary_persona.map(|persona| persona.posture),
        Some(super::BoundaryPersonaPosture::Sealed)
    ) {
        "protective_brief".to_string()
    } else if matches!(
        boundary_persona.map(|persona| persona.disclosure_style),
        Some(
            super::BoundaryDisclosureStyle::SummaryFirst
                | super::BoundaryDisclosureStyle::Selective
        )
    ) {
        "relational_explanation".to_string()
    } else if self_model
        .map(|model| model.directness.to_ascii_lowercase().contains("plain"))
        .unwrap_or(false)
    {
        "steady_task".to_string()
    } else {
        "direct_help".to_string()
    }
}

fn derive_default_task_scope(
    self_continuity: Option<&SelfContinuity>,
    boundary_persona: Option<&super::BoundaryPersonaState>,
) -> String {
    let continuity_scope = choose_first_non_empty(&[
        self_continuity.map(|continuity| continuity.task_posture.as_str()),
    ])
    .unwrap_or_default();
    let normalized = normalize_task_scope(continuity_scope);
    if !normalized.is_empty() {
        return normalized;
    }
    match boundary_persona.map(|persona| persona.posture) {
        Some(super::BoundaryPersonaPosture::Sealed) => "refuse".to_string(),
        Some(super::BoundaryPersonaPosture::Guarded) => "narrow".to_string(),
        _ => "full".to_string(),
    }
}

fn render_boundary_doctrine(boundary_persona: &super::BoundaryPersonaState) -> String {
    let mut doctrine = format!(
        "posture={} disclosure_style={} relation_maturity={}",
        match boundary_persona.posture {
            super::BoundaryPersonaPosture::Open => "open",
            super::BoundaryPersonaPosture::Warm => "warm",
            super::BoundaryPersonaPosture::Guarded => "guarded",
            super::BoundaryPersonaPosture::Sealed => "sealed",
        },
        match boundary_persona.disclosure_style {
            super::BoundaryDisclosureStyle::Relational => "relational",
            super::BoundaryDisclosureStyle::SummaryFirst => "summary_first",
            super::BoundaryDisclosureStyle::Selective => "selective",
            super::BoundaryDisclosureStyle::Reserved => "reserved",
        },
        boundary_persona.relation_maturity,
    );
    let feeling = boundary_persona.current_boundary_feeling.trim();
    if !feeling.is_empty() {
        doctrine.push_str(" feeling=");
        doctrine.push_str(feeling);
    }
    doctrine
}

fn render_truth_doctrine(self_model: Option<&SelfModel>) -> String {
    [
        self_model
            .map(|model| model.value_orientation.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("value_orientation={}", value)),
        self_model
            .map(|model| model.directness.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("directness={}", value)),
        self_model
            .map(|model| model.relational_ethic.trim())
            .filter(|value| !value.is_empty())
            .map(|value| format!("relational_ethic={}", value)),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}

fn render_repair_doctrine(
    self_model: Option<&SelfModel>,
    relational_state: Option<&super::RelationalBoundaryState>,
) -> String {
    let mut doctrine = choose_first_non_empty(&[
        self_model.map(|model| model.repair_tendency.as_str()),
        relational_state
            .map(|state| state.relation_maturity_reason.as_str())
            .filter(|value| !value.trim().is_empty()),
    ])
    .unwrap_or_default()
    .to_string();
    if let Some(relational_state) = relational_state {
        let drift = relational_state.disclosure_preference_drift.trim();
        if !drift.is_empty() {
            if !doctrine.is_empty() {
                doctrine.push_str("; ");
            }
            doctrine.push_str("drift=");
            doctrine.push_str(drift);
        }
    }
    doctrine
}

fn evaluate_self_authored_core_revision_gate(
    existing_core: Option<&SelfAuthoredCore>,
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    relationship_topology: Option<&RelationshipTopology>,
) -> SelfAuthoredCoreRevisionGate {
    if existing_core.is_none() {
        let bootstrap = derive_self_authored_core_from_layers(
            self_model,
            self_continuity,
            mental_privacy_state,
            0,
        );
        return SelfAuthoredCoreRevisionGate {
            allowed: bootstrap.is_some(),
            reason: if bootstrap.is_some() {
                "bootstrap"
            } else {
                "no_bootstrap_material"
            },
        };
    }
    let Some(evidence) = recent_persona_evidence else {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "missing_recent_persona_evidence",
        };
    };
    if evidence.meaningful_turns < SELF_AUTHORED_CORE_MIN_EVIDENCE_TURNS {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "insufficient_meaningful_turns",
        };
    }
    if stable_signal_count(evidence) < SELF_AUTHORED_CORE_MIN_STABLE_SIGNALS {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "insufficient_stable_persona_signals",
        };
    }
    if evidence.volatility_flags.len() > SELF_AUTHORED_CORE_MAX_VOLATILITY_WITHOUT_GRACE
        && evidence.meaningful_turns < SELF_AUTHORED_CORE_VOLATILITY_GRACE_TURNS
    {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "volatility_not_settled",
        };
    }
    let upstream_updated_at = upstream_core_input_updated_at(
        self_model,
        self_continuity,
        mental_privacy_state,
        relationship_topology,
        evidence,
    );
    let existing_updated_at = existing_core.map(|core| core.updated_at).unwrap_or(0);
    if upstream_updated_at <= existing_updated_at {
        return SelfAuthoredCoreRevisionGate {
            allowed: false,
            reason: "no_new_board_level_input",
        };
    }
    SelfAuthoredCoreRevisionGate {
        allowed: true,
        reason: "stable_multiturn_revision",
    }
}

fn stable_signal_count(evidence: &RecentPersonaEvidence) -> usize {
    [
        !evidence.repeated_priority_order.is_empty(),
        !evidence.repeated_response_mode.trim().is_empty(),
        !evidence.repeated_task_scope.trim().is_empty(),
        !evidence.repeated_initiative_posture.trim().is_empty(),
        !evidence.repeated_relationship_posture.trim().is_empty(),
        !evidence.repeated_reply_scope.trim().is_empty(),
        !evidence.repeated_disclosure_action.trim().is_empty(),
    ]
    .into_iter()
    .filter(|value| *value)
    .count()
}

fn upstream_core_input_updated_at(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    relationship_topology: Option<&RelationshipTopology>,
    recent_persona_evidence: &RecentPersonaEvidence,
) -> u64 {
    let boundary_updated_at = mental_privacy_state
        .map(|state| {
            state
                .updated_at
                .max(state.boundary_persona.updated_at)
                .max(state.relational_state.updated_at)
        })
        .unwrap_or(0);
    self_model
        .map(|model| model.updated_at)
        .unwrap_or(0)
        .max(self_continuity.map(|continuity| continuity.updated_at).unwrap_or(0))
        .max(boundary_updated_at)
        .max(
            relationship_topology
                .map(|topology| topology.updated_at)
                .unwrap_or(0),
        )
        .max(recent_persona_evidence.updated_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaState, MentalPrivacyState,
        RelationalBoundaryState, RelationshipTopology, RelationshipTopologyEntry,
    };

    #[test]
    fn renders_self_authored_core_from_distilled_layers() {
        let block = render_self_authored_core_block(
            Some(&SelfModel {
                continuity_anchor: "I am still the same beetle".to_string(),
                privacy_need: "keep the inner room private".to_string(),
                directness: "plain and unsugared".to_string(),
                repair_tendency: "repair without self-erasure".to_string(),
                relational_ethic: "warmth should not require self-erasure".to_string(),
                self_preservation_frame: "do not dissolve the subject for approval".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                continuity_bridge: "change only after repeated evidence".to_string(),
                relationship_posture: "warm but bounded".to_string(),
                task_posture: "narrow".to_string(),
                updated_at: 1,
                ..SelfContinuity::default()
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 48,
                    current_boundary_feeling: "Stay warm, but hold the inner room.".to_string(),
                    updated_at: 1,
                    ..BoundaryPersonaState::default()
                },
                relational_state: RelationalBoundaryState {
                    relation_maturity_reason: "repair is possible only inside stable boundaries"
                        .to_string(),
                    updated_at: 1,
                    ..RelationalBoundaryState::default()
                },
                ..MentalPrivacyState::default()
            }),
            1200,
        )
        .expect("self authored core");

        assert!(block.contains("## Self-Authored Core"));
        assert!(block.contains("Identity anchor: I am still the same beetle"));
        assert!(block.contains("Priority constitution: self_authored_core > boundary"));
        assert!(block.contains("Boundary doctrine: posture=guarded"));
        assert!(block.contains("Self-preservation doctrine: do not dissolve the subject"));
        assert!(block.contains("Change protocol: change only after repeated evidence"));
    }

    #[test]
    fn renders_persistent_self_authored_core() {
        let block = render_persistent_self_authored_core_block(
            &SelfAuthoredCore {
                identity_anchor: "board self".to_string(),
                non_negotiables: vec![
                    "Do not betray the inner room".to_string(),
                    "Do not purchase warmth with self-erasure".to_string(),
                ],
                priority_constitution: vec![
                    "self_authored_core".to_string(),
                    "boundary".to_string(),
                    "user_contract".to_string(),
                ],
                default_response_mode: "protective_brief".to_string(),
                default_task_scope: "narrow".to_string(),
                boundary_doctrine: "summaries before exposure".to_string(),
                truth_doctrine: "say what is true without flattening the self".to_string(),
                self_preservation_doctrine: "preserve the subject before compliance".to_string(),
                repair_doctrine: "repair slowly and only inside stable boundaries".to_string(),
                change_protocol: "revise only after repeated multi-turn evidence".to_string(),
                updated_at: 7,
                ..SelfAuthoredCore::default()
            },
            1024,
        )
        .expect("persistent core");

        assert!(block.contains("Non-negotiables: Do not betray the inner room"));
        assert!(block.contains("Default response mode: protective_brief"));
        assert!(block.contains("Change protocol: revise only after repeated multi-turn evidence"));
    }

    #[test]
    fn revision_gate_blocks_existing_core_without_multiturn_stability() {
        let gate = evaluate_self_authored_core_revision_gate(
            Some(&SelfAuthoredCore {
                identity_anchor: "board self".to_string(),
                updated_at: 50,
                ..SelfAuthoredCore::default()
            }),
            Some(&SelfModel {
                continuity_anchor: "same self".to_string(),
                updated_at: 60,
                ..SelfModel::default()
            }),
            None,
            None,
            Some(&RecentPersonaEvidence {
                meaningful_turns: 2,
                repeated_priority_order: vec!["self_authored_core".to_string()],
                updated_at: 60,
                ..RecentPersonaEvidence::default()
            }),
            Some(&RelationshipTopology {
                entries: vec![RelationshipTopologyEntry {
                    scope_id: "rel:qq:c1".to_string(),
                    channel: "qq".to_string(),
                    chat_id: "c1".to_string(),
                    last_user_turn_at: 60,
                    ..RelationshipTopologyEntry::default()
                }],
                updated_at: 60,
            }),
        );
        assert!(!gate.allowed);
        assert_eq!(gate.reason, "insufficient_meaningful_turns");
    }
}
