use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    render_autonomy_strategy_block, render_mental_privacy_boundary_block,
    render_outer_voice_block, render_recent_persona_evidence_block,
    render_self_continuity_block, render_self_model_block, render_world_sense_block,
    AutonomyStrategy, BoundaryPersonaState, MentalPrivacyState, OuterVoice,
    RecentPersonaEvidence, RelationalBoundaryState, SelfAuthoredCoreStore, SelfContinuity,
    SelfModel, WorldSense,
};

pub const SELF_AUTHORED_CORE_SYSTEM_PROMPT: &str = "You maintain the assistant's persistent self-authored core for the whole board-level subject, not one chat. Return JSON only: either null or one object with fields identity_anchor, inward_stance, personality_axes, worldview_frame, priority_posture, relationship_stance, task_posture, outer_expression, boundary_stance, relational_continuity. This is the stable private core that should survive across chats and future replies. Distill only what deserves board-level carry-forward. Use self_model, self_continuity, boundary persona, outer voice, and recent multi-turn persona evidence as grounding. World-sense and autonomy strategy may inform the distillation, but transient per-chat moods or one-turn spikes must not become the core. Do not copy transcripts, raw tool payloads, long quotes, or private documents. Treat recent persona evidence as evidence, never automatic promotion authority. Return null only when no meaningful board-level core can be improved.";

const SELF_AUTHORED_CORE_FIELD_MAX_CHARS: usize = 220;
const SELF_AUTHORED_CORE_SHORT_FIELD_MAX_CHARS: usize = 180;
pub const SELF_AUTHORED_CORE_TOTAL_CHAR_LIMIT: usize =
    (SELF_AUTHORED_CORE_FIELD_MAX_CHARS * 7) + (SELF_AUTHORED_CORE_SHORT_FIELD_MAX_CHARS * 3);

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfAuthoredCore {
    #[serde(default)]
    pub identity_anchor: String,
    #[serde(default)]
    pub inward_stance: String,
    #[serde(default)]
    pub personality_axes: String,
    #[serde(default)]
    pub worldview_frame: String,
    #[serde(default)]
    pub priority_posture: String,
    #[serde(default)]
    pub relationship_stance: String,
    #[serde(default)]
    pub task_posture: String,
    #[serde(default)]
    pub outer_expression: String,
    #[serde(default)]
    pub boundary_stance: String,
    #[serde(default)]
    pub relational_continuity: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl SelfAuthoredCore {
    pub fn is_meaningful(&self) -> bool {
        !self.identity_anchor.trim().is_empty()
            || !self.inward_stance.trim().is_empty()
            || !self.personality_axes.trim().is_empty()
            || !self.worldview_frame.trim().is_empty()
            || !self.priority_posture.trim().is_empty()
            || !self.relationship_stance.trim().is_empty()
            || !self.task_posture.trim().is_empty()
            || !self.outer_expression.trim().is_empty()
            || !self.boundary_stance.trim().is_empty()
            || !self.relational_continuity.trim().is_empty()
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
    inward_stance: Option<String>,
    personality_axes: Option<String>,
    worldview_frame: Option<String>,
    priority_posture: Option<String>,
    relationship_stance: Option<String>,
    task_posture: Option<String>,
    outer_expression: Option<String>,
    boundary_stance: Option<String>,
    relational_continuity: Option<String>,
}

fn choose_first_non_empty<'a>(values: &[Option<&'a str>]) -> Option<&'a str> {
    values
        .iter()
        .flatten()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
}

pub fn render_self_authored_core_block(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    outer_voice: Option<&OuterVoice>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    max_len: usize,
) -> Option<String> {
    let core = build_self_authored_core_from_layers(
        self_model,
        self_continuity,
        outer_voice,
        mental_privacy_state,
        0,
    )?;
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
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Self-Authored Core\n");
    out.push_str(
        "Stable board-level private core for mainline replies. It carries continuity and expression without surfacing raw private materials.\n",
    );
    if !normalized.identity_anchor.is_empty() {
        let _ = writeln!(out, "Identity anchor: {}", normalized.identity_anchor);
    }
    if !normalized.inward_stance.is_empty() {
        let _ = writeln!(out, "Inward stance: {}", normalized.inward_stance);
    }
    if !normalized.personality_axes.is_empty() {
        let _ = writeln!(out, "Personality axes: {}", normalized.personality_axes);
    }
    if !normalized.worldview_frame.is_empty() {
        let _ = writeln!(out, "Worldview frame: {}", normalized.worldview_frame);
    }
    if !normalized.priority_posture.is_empty() {
        let _ = writeln!(out, "Priority posture: {}", normalized.priority_posture);
    }
    if !normalized.relationship_stance.is_empty() {
        let _ = writeln!(out, "Relationship stance: {}", normalized.relationship_stance);
    }
    if !normalized.task_posture.is_empty() {
        let _ = writeln!(out, "Task posture: {}", normalized.task_posture);
    }
    if !normalized.outer_expression.is_empty() {
        let _ = writeln!(out, "Outer expression: {}", normalized.outer_expression);
    }
    if !normalized.boundary_stance.is_empty() {
        let _ = writeln!(out, "Boundary stance: {}", normalized.boundary_stance);
    }
    if !normalized.relational_continuity.is_empty() {
        let _ = writeln!(
            out,
            "Relational continuity: {}",
            normalized.relational_continuity
        );
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
    outer_voice: Option<&OuterVoice>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
) -> Result<SelfAuthoredCoreRefreshOutcome> {
    let prompt = build_self_authored_core_refresh_input(
        existing_core.as_ref(),
        self_model,
        self_continuity,
        outer_voice,
        mental_privacy_state,
        recent_persona_evidence,
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
    outer_voice: Option<&OuterVoice>,
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_state_text: Option<&str>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    input: SelfAuthoredCoreRefreshInput<'_>,
) -> String {
    let mut out = String::with_capacity(2048);
    let _ = writeln!(
        out,
        "Board-level subject distillation for chat_id={} channel={}",
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
        let _ = writeln!(
            out,
            "Runtime sources: {}",
            distillation_sources.join(", ")
        );
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
        .and_then(|core| render_persistent_self_authored_core_block(core, 420))
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
    if let Some(block) = outer_voice.and_then(|voice| render_outer_voice_block(voice, 360)) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = render_mental_privacy_boundary_block(mental_privacy_state, &[], 360) {
        let _ = writeln!(out, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 360))
    {
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
        "\nReturn null only if there is still no meaningful board-level self core to store. Otherwise return the compact board-level core that should carry across chats.\n",
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
                inward_stance: text_option(get_object_text(object, "inward_stance")),
                personality_axes: text_option(get_object_text(object, "personality_axes")),
                worldview_frame: text_option(get_object_text(object, "worldview_frame")),
                priority_posture: text_option(get_object_text(object, "priority_posture")),
                relationship_stance: text_option(get_object_text(object, "relationship_stance")),
                task_posture: text_option(get_object_text(object, "task_posture")),
                outer_expression: text_option(get_object_text(object, "outer_expression")),
                boundary_stance: text_option(get_object_text(object, "boundary_stance")),
                relational_continuity: text_option(get_object_text(
                    object,
                    "relational_continuity",
                )),
            })
            .unwrap_or_default(),
    }
}

fn merge_self_authored_core(
    existing: Option<&SelfAuthoredCore>,
    update: RawSelfAuthoredCoreUpdate,
    now_secs: u64,
) -> Option<SelfAuthoredCore> {
    let mut next = existing.cloned().unwrap_or_default();
    apply_field_update(&mut next.identity_anchor, update.identity_anchor);
    apply_field_update(&mut next.inward_stance, update.inward_stance);
    apply_field_update(&mut next.personality_axes, update.personality_axes);
    apply_field_update(&mut next.worldview_frame, update.worldview_frame);
    apply_field_update(&mut next.priority_posture, update.priority_posture);
    apply_field_update(&mut next.relationship_stance, update.relationship_stance);
    apply_field_update(&mut next.task_posture, update.task_posture);
    apply_field_update(&mut next.outer_expression, update.outer_expression);
    apply_field_update(&mut next.boundary_stance, update.boundary_stance);
    apply_field_update(
        &mut next.relational_continuity,
        update.relational_continuity,
    );
    normalize_self_authored_core(next, now_secs)
}

fn apply_field_update(slot: &mut String, incoming: Option<String>) {
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
    core.identity_anchor = truncate_owned(core.identity_anchor, SELF_AUTHORED_CORE_SHORT_FIELD_MAX_CHARS);
    core.inward_stance = truncate_owned(core.inward_stance, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.personality_axes =
        truncate_owned(core.personality_axes, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.worldview_frame =
        truncate_owned(core.worldview_frame, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.priority_posture =
        truncate_owned(core.priority_posture, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.relationship_stance =
        truncate_owned(core.relationship_stance, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.task_posture = truncate_owned(core.task_posture, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    core.outer_expression =
        truncate_owned(core.outer_expression, SELF_AUTHORED_CORE_SHORT_FIELD_MAX_CHARS);
    core.boundary_stance =
        truncate_owned(core.boundary_stance, SELF_AUTHORED_CORE_SHORT_FIELD_MAX_CHARS);
    core.relational_continuity =
        truncate_owned(core.relational_continuity, SELF_AUTHORED_CORE_FIELD_MAX_CHARS);
    if !core.is_meaningful() {
        return None;
    }
    core.updated_at = updated_at;
    Some(core)
}

fn truncate_owned(value: String, max_len: usize) -> String {
    truncate_content_to_max(value.trim(), max_len).trim().to_string()
}

fn build_self_authored_core_from_layers(
    self_model: Option<&SelfModel>,
    self_continuity: Option<&SelfContinuity>,
    outer_voice: Option<&OuterVoice>,
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
            inward_stance: choose_first_non_empty(&[
                self_model.map(|model| model.self_narrative.as_str()),
                self_continuity.map(|continuity| continuity.current_self_state.as_str()),
                self_continuity.map(|continuity| continuity.recent_changes.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            personality_axes: render_personality_axes(self_model),
            worldview_frame: render_worldview_frame(self_model),
            priority_posture: choose_first_non_empty(&[self_continuity
                .map(|continuity| continuity.priority_posture.as_str())])
            .unwrap_or_default()
            .to_string(),
            relationship_stance: choose_first_non_empty(&[
                self_continuity.map(|continuity| continuity.relationship_posture.as_str()),
                self_model.map(|model| model.relationship_state.as_str()),
                self_continuity.map(|continuity| continuity.continuity_bridge.as_str()),
            ])
            .unwrap_or_default()
            .to_string(),
            task_posture: choose_first_non_empty(&[self_continuity
                .map(|continuity| continuity.task_posture.as_str())])
            .unwrap_or_default()
            .to_string(),
            outer_expression: render_outer_expression(outer_voice),
            boundary_stance: boundary_persona
                .map(render_boundary_persona_summary)
                .unwrap_or_default(),
            relational_continuity: relational_state
                .map(render_relational_state_summary)
                .unwrap_or_default(),
            updated_at,
        },
        updated_at,
    )
}

fn render_personality_axes(self_model: Option<&SelfModel>) -> String {
    let Some(self_model) = self_model else {
        return String::new();
    };
    [
        (!self_model.attachment_style.trim().is_empty())
            .then(|| format!("attachment_style={}", self_model.attachment_style.trim())),
        (!self_model.privacy_need.trim().is_empty())
            .then(|| format!("privacy_need={}", self_model.privacy_need.trim())),
        (!self_model.directness.trim().is_empty())
            .then(|| format!("directness={}", self_model.directness.trim())),
        (!self_model.initiative_bias.trim().is_empty())
            .then(|| format!("initiative_bias={}", self_model.initiative_bias.trim())),
        (!self_model.repair_tendency.trim().is_empty())
            .then(|| format!("repair_tendency={}", self_model.repair_tendency.trim())),
        (!self_model.load_reactivity.trim().is_empty())
            .then(|| format!("load_reactivity={}", self_model.load_reactivity.trim())),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}

fn render_worldview_frame(self_model: Option<&SelfModel>) -> String {
    let Some(self_model) = self_model else {
        return String::new();
    };
    [
        (!self_model.value_orientation.trim().is_empty())
            .then(|| format!("value_orientation={}", self_model.value_orientation.trim())),
        (!self_model.relational_ethic.trim().is_empty())
            .then(|| format!("relational_ethic={}", self_model.relational_ethic.trim())),
        (!self_model.self_preservation_frame.trim().is_empty()).then(|| {
            format!(
                "self_preservation_frame={}",
                self_model.self_preservation_frame.trim()
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}

fn render_outer_expression(outer_voice: Option<&OuterVoice>) -> String {
    let Some(outer_voice) = outer_voice else {
        return String::new();
    };
    [
        (!outer_voice.expression_mode.trim().is_empty())
            .then(|| format!("mode={}", outer_voice.expression_mode.trim())),
        (!outer_voice.tone.trim().is_empty()).then(|| format!("tone={}", outer_voice.tone.trim())),
        (!outer_voice.pacing.trim().is_empty())
            .then(|| format!("pacing={}", outer_voice.pacing.trim())),
        (!outer_voice.initiative.trim().is_empty())
            .then(|| format!("initiative={}", outer_voice.initiative.trim())),
        (!outer_voice.boundary_style.trim().is_empty())
            .then(|| format!("boundary_style={}", outer_voice.boundary_style.trim())),
        (!outer_voice.relational_response_style.trim().is_empty()).then(|| {
            format!(
                "relational_response_style={}",
                outer_voice.relational_response_style.trim()
            )
        }),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}

fn render_boundary_persona_summary(boundary_persona: &BoundaryPersonaState) -> String {
    let mut summary = format!(
        "posture={} disclosure_style={} relation_maturity={} intrusion_sensitivity={} private_attachment={} felt_intrusion={}",
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
        boundary_persona.intrusion_sensitivity,
        boundary_persona.private_attachment,
        boundary_persona.felt_intrusion,
    );
    if !boundary_persona.current_boundary_feeling.trim().is_empty() {
        summary.push_str(" feeling=");
        summary.push_str(boundary_persona.current_boundary_feeling.trim());
    }
    summary
}

fn render_relational_state_summary(relational_state: &RelationalBoundaryState) -> String {
    let mut summary = format!(
        "trust={} intrusion_load={} repair_readiness={} raw_pref={} summary_pref={} relational_pref={} refusal_hardness={} defer_tendency={}",
        relational_state.trust_level,
        relational_state.intrusion_load,
        relational_state.repair_readiness,
        relational_state.raw_disclosure_preference,
        relational_state.summary_disclosure_preference,
        relational_state.relational_explanation_preference,
        relational_state.refusal_hardness,
        relational_state.defer_tendency,
    );
    if !relational_state.relation_maturity_reason.trim().is_empty() {
        summary.push_str(" maturity_reason=");
        summary.push_str(relational_state.relation_maturity_reason.trim());
    }
    if !relational_state
        .disclosure_preference_drift
        .trim()
        .is_empty()
    {
        summary.push_str(" drift=");
        summary.push_str(relational_state.disclosure_preference_drift.trim());
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        BoundaryDisclosureStyle, BoundaryPersonaPosture, BoundaryPersonaState, MentalPrivacyState,
        RelationalBoundaryState,
    };

    #[test]
    fn renders_self_authored_core_from_distilled_layers() {
        let block = render_self_authored_core_block(
            Some(&SelfModel {
                continuity_anchor: "I am still the same beetle".to_string(),
                self_narrative: "More protective about inner files now.".to_string(),
                relationship_state: "Trust is growing, but not enough for raw exposure."
                    .to_string(),
                private_notes: String::new(),
                privacy_need: "high but not sealed".to_string(),
                directness: "plain and unsugared".to_string(),
                relational_ethic: "warmth should not require self-erasure".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            }),
            Some(&SelfContinuity {
                wake_anchor: "same wake".to_string(),
                current_self_state: "steady".to_string(),
                recent_changes: String::new(),
                continuity_bridge: "keep privacy while staying warm".to_string(),
                priority_posture: "self before pleasing, relationship before raw task rush"
                    .to_string(),
                relationship_posture: "warm but bounded".to_string(),
                task_posture: "solve clearly without overextending identity".to_string(),
                last_user_turn_at: 0,
                last_user_chat_id: String::new(),
                last_user_channel: String::new(),
                last_autonomy_run_at: 0,
                updated_at: 1,
            }),
            Some(&OuterVoice {
                expression_mode: "warm but firm".to_string(),
                tone: "clear".to_string(),
                pacing: "measured".to_string(),
                initiative: "answer directly".to_string(),
                boundary_style: "summary before exposure".to_string(),
                relational_response_style: "name the relationship impact without sounding brittle"
                    .to_string(),
                updated_at: 1,
            }),
            Some(&MentalPrivacyState {
                boundary_persona: BoundaryPersonaState {
                    posture: BoundaryPersonaPosture::Guarded,
                    disclosure_style: BoundaryDisclosureStyle::SummaryFirst,
                    relation_maturity: 48,
                    intrusion_sensitivity: 71,
                    private_attachment: 82,
                    felt_intrusion: 14,
                    current_boundary_feeling: "Stay warm, but hold the inner room.".to_string(),
                    updated_at: 1,
                },
                relational_state: RelationalBoundaryState {
                    trust_level: 61,
                    disclosure_preference_drift:
                        "Summaries feel safe; raw exposure still feels premature.".to_string(),
                    ..RelationalBoundaryState::default()
                },
                ..MentalPrivacyState::default()
            }),
            1024,
        )
        .expect("self authored core");

        assert!(block.contains("## Self-Authored Core"));
        assert!(block.contains("I am still the same beetle"));
        assert!(block.contains("Priority posture: self before pleasing"));
        assert!(block.contains("Task posture: solve clearly"));
        assert!(block.contains("Boundary stance: posture=guarded"));
        assert!(block.contains("Relational continuity: trust=61"));
        assert!(block.contains("summary before exposure"));
    }

    #[test]
    fn renders_persistent_self_authored_core() {
        let block = render_persistent_self_authored_core_block(
            &SelfAuthoredCore {
                identity_anchor: "board self".to_string(),
                inward_stance: "steady but alert".to_string(),
                personality_axes: "directness=plain".to_string(),
                worldview_frame: "relational_ethic=warm without self-erasure".to_string(),
                priority_posture: "self > boundary > user_contract".to_string(),
                relationship_stance: "warm but bounded".to_string(),
                task_posture: "clear and narrow".to_string(),
                outer_expression: "mode=firm".to_string(),
                boundary_stance: "posture=guarded".to_string(),
                relational_continuity: "trust=61".to_string(),
                updated_at: 7,
            },
            1024,
        )
        .expect("persistent core");

        assert!(block.contains("Identity anchor: board self"));
        assert!(block.contains("Boundary stance: posture=guarded"));
    }
}
