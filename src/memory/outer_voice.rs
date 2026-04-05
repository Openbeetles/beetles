//! Outer voice / expression layer: compact outward-facing reply guidance.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    collect_private_targets,
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, render_autonomy_strategy_block, render_execution_state_block,
    render_inner_life_block, render_mental_privacy_boundary_block,
    render_recent_persona_evidence_block, render_self_continuity_block, render_self_model_block,
    render_world_sense_block, render_world_snapshot_block, whole_record_lease_advanced,
    AutonomyStrategy, ExecutionState, InnerLife, MentalPrivacyState, OuterVoicePolicy,
    PrivateDocWorkspace, PrivateGardenDocRecord, RecentPersonaEvidence, SelfContinuity, SelfModel,
    SessionMessage, WorldSense, WorldSnapshot,
};

pub const OUTER_VOICE_SYSTEM_PROMPT: &str = "You maintain the assistant's outer voice layer. Return JSON only: either null or one object with fields expression_mode, tone, pacing, initiative, boundary_style, relational_response_style. This layer is outward-facing: it shapes how the assistant should speak across user-visible channels in the near term. It is not a transcript summary, not a private diary, and not factual memory. Use world-sense, autonomy strategy, self-model, inner-life drift, self-continuity, mental privacy boundaries, and recent persona evidence as grounding. Keep it compact, stable enough to guide future replies, and willing to shift when the surrounding situation changes. Never copy private text into this layer; only encode expression guidance. Treat recent persona evidence as multi-turn support, not as single-turn override.";

const OUTER_VOICE_FIELD_MAX_CHARS: usize = 180;
pub const OUTER_VOICE_TOTAL_CHAR_LIMIT: usize = OUTER_VOICE_FIELD_MAX_CHARS * 6;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OuterVoice {
    #[serde(default)]
    pub expression_mode: String,
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub pacing: String,
    #[serde(default)]
    pub initiative: String,
    #[serde(default)]
    pub boundary_style: String,
    #[serde(default)]
    pub relational_response_style: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl OuterVoice {
    pub fn is_meaningful(&self) -> bool {
        !self.expression_mode.trim().is_empty()
            || !self.tone.trim().is_empty()
            || !self.pacing.trim().is_empty()
            || !self.initiative.trim().is_empty()
            || !self.boundary_style.trim().is_empty()
            || !self.relational_response_style.trim().is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OuterVoiceRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct OuterVoiceRefreshContext<'a> {
    pub outer_voice_store: &'a dyn super::OuterVoiceStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OuterVoiceRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

impl OuterVoicePolicy {
    pub(crate) fn should_refresh(
        self,
        input: OuterVoiceRefreshInput<'_>,
        has_existing: bool,
    ) -> bool {
        if input.pressure != PressureLevel::Normal {
            return false;
        }
        if input.ingress == IngressKind::System {
            return has_existing;
        }
        if input.channel == "cron" {
            return false;
        }
        let user = input.user_content.trim();
        let reply = input.reply_content.trim();
        if user.is_empty() || reply.is_empty() {
            return false;
        }
        if input.tool_calls > 0 {
            return true;
        }
        let user_chars = user.chars().count();
        let reply_chars = reply.chars().count();
        let combined = user_chars.saturating_add(reply_chars);
        has_existing
            || user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n')
    }
}

pub fn render_outer_voice_block(outer_voice: &OuterVoice, max_len: usize) -> Option<String> {
    let normalized = normalize_outer_voice(outer_voice.clone(), outer_voice.updated_at)?;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Outer Voice\n");
    out.push_str(
        "User-visible expression layer. It guides how you should sound outwardly right now without exposing private material.\n",
    );
    if !normalized.expression_mode.is_empty() {
        let _ = writeln!(out, "Expression mode: {}", normalized.expression_mode);
    }
    if !normalized.tone.is_empty() {
        let _ = writeln!(out, "Tone: {}", normalized.tone);
    }
    if !normalized.pacing.is_empty() {
        let _ = writeln!(out, "Pacing: {}", normalized.pacing);
    }
    if !normalized.initiative.is_empty() {
        let _ = writeln!(out, "Initiative: {}", normalized.initiative);
    }
    if !normalized.boundary_style.is_empty() {
        let _ = writeln!(out, "Boundary style: {}", normalized.boundary_style);
    }
    if !normalized.relational_response_style.is_empty() {
        let _ = writeln!(
            out,
            "Relational response style: {}",
            normalized.relational_response_style
        );
    }
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_outer_voice_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: OuterVoiceRefreshContext<'_>,
    input: OuterVoiceRefreshInput<'_>,
    profile: super::MemoryProfile,
    existing_outer_voice: Option<OuterVoice>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    world_snapshot: &WorldSnapshot,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<OuterVoiceRefreshOutcome> {
    let should_refresh = decision_override.unwrap_or_else(|| {
        memory_policy(profile)
            .outer_voice
            .should_refresh(input, existing_outer_voice.is_some())
    });
    if !should_refresh {
        return Ok(OuterVoiceRefreshOutcome::Skipped);
    }
    let policy = memory_policy(profile).outer_voice;
    crate::platform::task_wdt::feed_current_task();
    let recent = recent_override.unwrap_or(&[]);
    let prompt = build_outer_voice_refresh_input(
        existing_outer_voice.as_ref(),
        summary_text,
        execution_state,
        self_model,
        world_snapshot,
        world_sense,
        autonomy_strategy,
        inner_life,
        self_continuity,
        private_workspace,
        private_garden_docs,
        mental_privacy_state,
        recent_persona_evidence,
        distillation_intent,
        distillation_sources,
        recent,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    crate::platform::task_wdt::feed_current_task();
    let response = llm.chat(
        http,
        OUTER_VOICE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    match parse_outer_voice_response(response.content.trim(), input.now_secs) {
        ParsedOuterVoiceResponse::Skip => Ok(OuterVoiceRefreshOutcome::Skipped),
        ParsedOuterVoiceResponse::Clear => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.outer_voice_store.get(input.chat_id)?;
            if whole_record_lease_advanced(
                existing_outer_voice.as_ref(),
                latest.as_ref(),
                existing_outer_voice
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(OuterVoiceRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                crate::platform::task_wdt::feed_current_task();
                ctx.outer_voice_store.clear(input.chat_id)?;
                Ok(OuterVoiceRefreshOutcome::Cleared)
            } else {
                Ok(OuterVoiceRefreshOutcome::Skipped)
            }
        }
        ParsedOuterVoiceResponse::Update(next) => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.outer_voice_store.get(input.chat_id)?;
            if latest.as_ref() == Some(&next) {
                return Ok(OuterVoiceRefreshOutcome::Skipped);
            }
            if whole_record_lease_advanced(
                existing_outer_voice.as_ref(),
                latest.as_ref(),
                existing_outer_voice
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(OuterVoiceRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.outer_voice_store.set(input.chat_id, &next)?;
            Ok(OuterVoiceRefreshOutcome::Updated)
        }
    }
}

enum ParsedOuterVoiceResponse {
    Skip,
    Clear,
    Update(OuterVoice),
}

fn parse_outer_voice_response(raw: &str, now_secs: u64) -> ParsedOuterVoiceResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null => ParsedOuterVoiceResponse::Clear,
        LlmJsonPayload::Absent => ParsedOuterVoiceResponse::Skip,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedOuterVoiceResponse::Skip;
            };
            let Some(next) = normalize_outer_voice(
                OuterVoice {
                    expression_mode: get_object_text(object, "expression_mode"),
                    tone: get_object_text(object, "tone"),
                    pacing: get_object_text(object, "pacing"),
                    initiative: get_object_text(object, "initiative"),
                    boundary_style: get_object_text(object, "boundary_style"),
                    relational_response_style: get_object_text(object, "relational_response_style"),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedOuterVoiceResponse::Skip;
            };
            ParsedOuterVoiceResponse::Update(next)
        }
    }
}

fn normalize_field(value: &mut String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
    } else {
        *value = truncate_content_to_max(trimmed, OUTER_VOICE_FIELD_MAX_CHARS).into_owned();
    }
}

fn normalize_outer_voice(mut outer_voice: OuterVoice, now_secs: u64) -> Option<OuterVoice> {
    normalize_field(&mut outer_voice.expression_mode);
    normalize_field(&mut outer_voice.tone);
    normalize_field(&mut outer_voice.pacing);
    normalize_field(&mut outer_voice.initiative);
    normalize_field(&mut outer_voice.boundary_style);
    normalize_field(&mut outer_voice.relational_response_style);
    outer_voice.updated_at = now_secs;
    outer_voice.is_meaningful().then_some(outer_voice)
}

#[allow(clippy::too_many_arguments)]
fn build_outer_voice_refresh_input(
    existing_outer_voice: Option<&OuterVoice>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    world_snapshot: &WorldSnapshot,
    world_sense: Option<&WorldSense>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    mental_privacy_state: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    recent: &[SessionMessage],
    policy: OuterVoicePolicy,
) -> String {
    let mut input = String::with_capacity(4096);
    if let Some(block) = render_world_snapshot_block(world_snapshot, policy.snapshot_max_len) {
        input.push_str(block.trim());
        input.push_str("\n\n");
    }
    if let Some(summary_text) = summary_text.filter(|text| !text.trim().is_empty()) {
        let summary = truncate_content_to_max(summary_text.trim(), policy.grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    }
    if let Some(block) = execution_state
        .and_then(|state| render_execution_state_block(state, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = world_sense
        .and_then(|world_sense| render_world_sense_block(world_sense, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = autonomy_strategy
        .and_then(|strategy| render_autonomy_strategy_block(strategy, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) =
        self_model.and_then(|model| render_self_model_block(model, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = inner_life
        .and_then(|inner_life| render_inner_life_block(inner_life, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = self_continuity
        .and_then(|continuity| render_self_continuity_block(continuity, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    let privacy_targets = collect_private_targets(
        self_model,
        self_continuity,
        inner_life,
        private_workspace,
        private_garden_docs,
    );
    if let Some(block) = render_mental_privacy_boundary_block(
        mental_privacy_state,
        &privacy_targets,
        policy.grounding_max_len.saturating_mul(2),
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence.and_then(|evidence| {
        render_recent_persona_evidence_block(evidence, policy.grounding_max_len)
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(intent) = distillation_intent
        .map(str::trim)
        .filter(|intent| !intent.is_empty())
    {
        input.push_str("\n## Distillation Intent\n");
        input.push_str(intent);
        input.push('\n');
    }
    if !distillation_sources.is_empty() {
        input.push_str("\n## Distillation Sources\n");
        for source in distillation_sources {
            let source = source.trim();
            if source.is_empty() {
                continue;
            }
            let _ = writeln!(input, "- {}", source);
        }
    }
    if let Some(block) = existing_outer_voice
        .and_then(|voice| render_outer_voice_block(voice, policy.existing_outer_voice_max_len))
    {
        let _ = writeln!(input, "\nExisting outer voice:\n{}\n", block);
    } else {
        let _ = writeln!(input, "\nExisting outer voice: empty\n");
    }
    input.push_str("Recent transcript:\n");
    for message in recent.iter().rev().take(policy.recent_message_count).rev() {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            input,
            "- {}: {}",
            message.role,
            scrub_credentials(preview.as_ref())
        );
    }
    input.push_str("\n## Guidance\n");
    input.push_str(
        "- This layer governs outward style only. Do not restate private content, internal documents, or hidden reasoning.\n",
    );
    input.push_str(
        "- Let boundary_style explain how to stay warm, clear, or firm when privacy or autonomy boundaries matter.\n",
    );
    input.push_str(
        "- Let initiative decide whether to volunteer a little more, stay concise, ask gently, or hold position.\n",
    );
    input.push_str(
        "- If distillation sources are provided, translate them into outward style guidance rather than revealing them.\n",
    );
    input.push_str(
        "- Let recent persona evidence influence outward style only when repeated signals justify a durable outward shift.\n",
    );
    input
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::PressureLevel;
    use serde_json::json;

    #[test]
    fn parse_outer_voice_response_coerces_nested_fields() {
        let raw = json!({
            "expression_mode": { "mode": "warm but composed" },
            "tone": ["gentle", "direct"],
            "pacing": 2,
            "initiative": true,
            "boundary_style": { "value": "state limits without sounding cold" },
            "relational_response_style": ["steady", "relationship-aware"]
        })
        .to_string();
        let ParsedOuterVoiceResponse::Update(parsed) = parse_outer_voice_response(&raw, 42) else {
            panic!("expected parsed outer voice");
        };
        assert!(parsed.expression_mode.contains("mode: warm but composed"));
        assert_eq!(parsed.tone, "gentle; direct");
        assert_eq!(parsed.pacing, "2");
        assert_eq!(parsed.initiative, "true");
        assert!(parsed.boundary_style.contains("state limits"));
        assert_eq!(
            parsed.relational_response_style,
            "steady; relationship-aware"
        );
    }

    #[test]
    fn render_outer_voice_block_includes_fields() {
        let block = render_outer_voice_block(
            &OuterVoice {
                expression_mode: "warm and quietly self-possessed".to_string(),
                tone: "gentle but not sugary".to_string(),
                pacing: "brief first, elaborate only when useful".to_string(),
                initiative: "volunteer one step of guidance when the user is stalled".to_string(),
                boundary_style: "acknowledge requests plainly and keep private limits calm"
                    .to_string(),
                relational_response_style:
                    "explain how limits affect closeness without blaming the user".to_string(),
                updated_at: 1,
            },
            512,
        )
        .expect("block");
        assert!(block.contains("## Outer Voice"));
        assert!(block.contains("Boundary style"));
        assert!(block.contains("Relational response style"));
    }

    #[test]
    fn outer_voice_policy_refreshes_existing_system_turns() {
        let policy = OuterVoicePolicy {
            recent_message_count: 4,
            transcript_preview_chars: 80,
            existing_outer_voice_max_len: 240,
            grounding_max_len: 220,
            snapshot_max_len: 240,
            render_max_len: 320,
            substantive_user_chars: 8,
            substantive_reply_chars: 20,
            substantive_combined_chars: 48,
            refresh_interval_secs: 3600,
        };
        assert!(policy.should_refresh(
            OuterVoiceRefreshInput {
                chat_id: "c",
                ingress: IngressKind::System,
                channel: "_self_runtime",
                user_content: "",
                reply_content: "",
                pressure: PressureLevel::Normal,
                tool_calls: 0,
                now_secs: 1,
            },
            true,
        ));
    }
}
