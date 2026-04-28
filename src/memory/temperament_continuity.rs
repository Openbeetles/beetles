//! Temperament-continuity domain contract for stable behavioral inertia.
//! 性格连续领域合同：表达长期惯性，不负责刷新、存储或前台接线。

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{coerce_json_text, get_object_text, parse_llm_json_payload, LlmJsonPayload},
    render_mental_privacy_boundary_block, render_outer_voice_block,
    render_recent_persona_evidence_block, render_self_continuity_block, MentalPrivacyState,
    OuterVoice, RecentPersonaEvidence, SelfContinuity,
};

/// JSON/system contract for future temperament-continuity producers.
pub const TEMPERAMENT_CONTINUITY_SYSTEM_CONTRACT: &str = "Return JSON only for the temperament-continuity contract: conversational_inertia, boundary_inertia, disagreement_style, attachment_rhythm, explanation_habit, stability_summary. Consolidate multi-turn evidence into durable tendencies without cue-word rules, personality scores, theatrical labels, or prompt-roleplay.";

/// Approximate total character budget for compact temperament-continuity state.
pub const TEMPERAMENT_CONTINUITY_TOTAL_CHAR_LIMIT: usize = 1_080;

const TEMPERAMENT_CONTINUITY_FIELD_MAX_CHARS: usize = 180;

/// Compact domain state for stable tendencies across turns and days.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemperamentContinuity {
    #[serde(default)]
    pub conversational_inertia: String,
    #[serde(default)]
    pub boundary_inertia: String,
    #[serde(default)]
    pub disagreement_style: String,
    #[serde(default)]
    pub attachment_rhythm: String,
    #[serde(default)]
    pub explanation_habit: String,
    #[serde(default)]
    pub stability_summary: String,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemperamentContinuityRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

pub(crate) enum TemperamentContinuityRefreshCandidate {
    Skipped,
    Updated(TemperamentContinuity),
    Cleared,
}

impl TemperamentContinuity {
    /// Returns whether this state contains durable temperament evidence.
    pub fn is_meaningful(&self) -> bool {
        !self.conversational_inertia.trim().is_empty()
            || !self.boundary_inertia.trim().is_empty()
            || !self.disagreement_style.trim().is_empty()
            || !self.attachment_rhythm.trim().is_empty()
            || !self.explanation_habit.trim().is_empty()
            || !self.stability_summary.trim().is_empty()
    }
}

pub(crate) fn run_temperament_continuity_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    rendered_input: String,
    now_secs: u64,
) -> Result<TemperamentContinuityRefreshCandidate> {
    if rendered_input.trim().is_empty() {
        return Ok(TemperamentContinuityRefreshCandidate::Skipped);
    }
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: rendered_input,
    }];
    let response = llm.chat(
        http,
        TEMPERAMENT_CONTINUITY_SYSTEM_CONTRACT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(
        match parse_temperament_continuity_response(response.content.trim(), now_secs) {
            ParsedTemperamentContinuityResponse::Skip => {
                TemperamentContinuityRefreshCandidate::Skipped
            }
            ParsedTemperamentContinuityResponse::Clear => {
                TemperamentContinuityRefreshCandidate::Cleared
            }
            ParsedTemperamentContinuityResponse::Update(next) => {
                TemperamentContinuityRefreshCandidate::Updated(next)
            }
        },
    )
}

pub(crate) fn build_temperament_continuity_refresh_input(
    current: Option<&TemperamentContinuity>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    mental_privacy: Option<&MentalPrivacyState>,
    outer_voice: Option<&OuterVoice>,
    self_continuity: Option<&SelfContinuity>,
    max_len: usize,
) -> String {
    let mut input = String::with_capacity(max_len.min(2048));
    input.push_str("Refresh the temperament-continuity layer for durable behavioral inertia.\n");
    input.push_str("Use repeated evidence and boundary posture only; do not create trait scores, roles, cue-word rules, or one-turn identity claims.\n");
    let existing_block =
        current.and_then(|state| render_temperament_continuity_block(state, max_len));
    let existing_reserve = existing_block
        .as_deref()
        .map(|block| block.chars().count().saturating_add(42).min(max_len / 2))
        .unwrap_or(0);
    let context_budget = max_len.saturating_sub(existing_reserve).max(max_len / 3);
    if let Some(block) = recent_persona_evidence
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, context_budget))
    {
        append_scrubbed_block(
            &mut input,
            "Recent persona evidence",
            &block,
            context_budget,
        );
    }
    if let Some(block) = render_mental_privacy_boundary_block(mental_privacy, &[], context_budget) {
        append_scrubbed_block(&mut input, "Mental privacy", &block, context_budget);
    }
    if let Some(block) =
        outer_voice.and_then(|voice| render_outer_voice_block(voice, context_budget))
    {
        append_scrubbed_block(&mut input, "Outer voice", &block, context_budget);
    }
    if let Some(block) =
        self_continuity.and_then(|state| render_self_continuity_block(state, context_budget))
    {
        append_scrubbed_block(&mut input, "Self continuity", &block, context_budget);
    }
    trim_context_for_existing(&mut input, max_len, existing_reserve);
    let existing_budget = max_len
        .saturating_sub(input.chars().count())
        .max(existing_reserve)
        .min(max_len);
    if let Some(block) = existing_block {
        append_scrubbed_block(
            &mut input,
            "Existing temperament continuity",
            &block,
            section_block_budget("Existing temperament continuity", existing_budget),
        );
    } else {
        input.push_str("\nExisting temperament continuity: empty\n");
    }
    truncate_content_to_max(input.trim_end(), max_len).into_owned()
}

/// Render a compact temperament-continuity block for later projection/debug surfaces.
pub fn render_temperament_continuity_block(
    state: &TemperamentContinuity,
    max_len: usize,
) -> Option<String> {
    let normalized = normalize_temperament_continuity(state.clone(), state.updated_at)?;
    let mut lines = Vec::new();
    push_field_line(
        &mut lines,
        "Conversational inertia",
        &normalized.conversational_inertia,
    );
    push_field_line(&mut lines, "Boundary inertia", &normalized.boundary_inertia);
    push_field_line(
        &mut lines,
        "Disagreement style",
        &normalized.disagreement_style,
    );
    push_field_line(
        &mut lines,
        "Attachment rhythm",
        &normalized.attachment_rhythm,
    );
    push_field_line(
        &mut lines,
        "Explanation habit",
        &normalized.explanation_habit,
    );
    push_field_line(
        &mut lines,
        "Stability summary",
        &normalized.stability_summary,
    );
    render_complete_block(
        "## Temperament Continuity",
        "Durable behavioral inertia inferred from evidence, not a fixed role or score.",
        &lines,
        max_len,
    )
}

enum ParsedTemperamentContinuityResponse {
    Skip,
    Clear,
    Update(TemperamentContinuity),
}

fn parse_temperament_continuity_response(
    raw: &str,
    now_secs: u64,
) -> ParsedTemperamentContinuityResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Absent => ParsedTemperamentContinuityResponse::Skip,
        LlmJsonPayload::Null => ParsedTemperamentContinuityResponse::Clear,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedTemperamentContinuityResponse::Skip;
            };
            match parse_refresh_action(object) {
                RefreshAction::Clear => return ParsedTemperamentContinuityResponse::Clear,
                RefreshAction::Skip => return ParsedTemperamentContinuityResponse::Skip,
                RefreshAction::Update => {}
            }
            let object = state_object(object);
            let Some(next) = normalize_temperament_continuity(
                TemperamentContinuity {
                    conversational_inertia: get_refresh_text(object, "conversational_inertia"),
                    boundary_inertia: get_refresh_text(object, "boundary_inertia"),
                    disagreement_style: get_refresh_text(object, "disagreement_style"),
                    attachment_rhythm: get_refresh_text(object, "attachment_rhythm"),
                    explanation_habit: get_refresh_text(object, "explanation_habit"),
                    stability_summary: get_refresh_text(object, "stability_summary"),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedTemperamentContinuityResponse::Skip;
            };
            ParsedTemperamentContinuityResponse::Update(next)
        }
    }
}

enum RefreshAction {
    Update,
    Clear,
    Skip,
}

fn parse_refresh_action(object: &Map<String, Value>) -> RefreshAction {
    match get_object_text(object, "action")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "clear" | "delete" | "remove" => RefreshAction::Clear,
        "skip" | "hold" | "noop" | "no_op" | "none" => RefreshAction::Skip,
        _ => RefreshAction::Update,
    }
}

fn state_object(object: &Map<String, Value>) -> &Map<String, Value> {
    object
        .get("state")
        .and_then(Value::as_object)
        .unwrap_or(object)
}

fn get_refresh_text(object: &Map<String, Value>, key: &str) -> String {
    object.get(key).map(coerce_refresh_text).unwrap_or_default()
}

fn coerce_refresh_text(value: &Value) -> String {
    if let Value::Object(object) = value {
        for key in ["value", "summary", "text", "content"] {
            if let Some(value) = object.get(key) {
                let text = coerce_refresh_text(value);
                if !text.is_empty() {
                    return text;
                }
            }
        }
    }
    coerce_json_text(value)
}

fn normalize_temperament_continuity(
    mut state: TemperamentContinuity,
    updated_at: u64,
) -> Option<TemperamentContinuity> {
    normalize_field(&mut state.conversational_inertia);
    normalize_field(&mut state.boundary_inertia);
    normalize_field(&mut state.disagreement_style);
    normalize_field(&mut state.attachment_rhythm);
    normalize_field(&mut state.explanation_habit);
    normalize_field(&mut state.stability_summary);
    state.updated_at = updated_at;
    state.is_meaningful().then_some(state)
}

fn normalize_field(value: &mut String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
    } else {
        *value =
            truncate_content_to_max(trimmed, TEMPERAMENT_CONTINUITY_FIELD_MAX_CHARS).into_owned();
    }
}

fn push_field_line(lines: &mut Vec<String>, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    lines.push(format!("{label}: {value}"));
}

fn append_scrubbed_block(out: &mut String, label: &str, block: &str, max_chars: usize) {
    let block = super::scrub_memory_prompt_block(block);
    if block.trim().is_empty() {
        return;
    }
    let block = truncate_content_to_max(block.trim(), max_chars);
    let _ = writeln!(out, "\n{label}:\n{}\n", block.trim());
}

fn trim_context_for_existing(input: &mut String, max_len: usize, existing_reserve: usize) {
    if existing_reserve == 0 {
        return;
    }
    let context_limit = max_len.saturating_sub(existing_reserve);
    if input.chars().count() > context_limit {
        *input = truncate_content_to_max(input.trim_end(), context_limit).into_owned();
    }
}

fn section_block_budget(label: &str, section_budget: usize) -> usize {
    section_budget
        .saturating_sub(label.chars().count())
        .saturating_sub(6)
}

fn render_complete_block(
    header: &str,
    description: &str,
    data_lines: &[String],
    max_len: usize,
) -> Option<String> {
    let first_data = data_lines.first()?;
    let minimum_chars = header
        .chars()
        .count()
        .saturating_add(1)
        .saturating_add(first_data.chars().count());
    if max_len < minimum_chars {
        return None;
    }

    let mut out = String::with_capacity(max_len.min(640));
    let _ = writeln!(out, "{header}");
    let mut data_count = 0;
    for line in data_lines {
        if append_line_if_fits(&mut out, line, max_len) {
            data_count += 1;
        }
    }
    append_line_if_fits(&mut out, description, max_len);
    (data_count > 0).then(|| out.trim_end().to_string())
}

fn append_line_if_fits(out: &mut String, line: &str, max_len: usize) -> bool {
    let next_chars = out
        .chars()
        .count()
        .saturating_add(line.chars().count())
        .saturating_add(1);
    if next_chars <= max_len {
        let _ = writeln!(out, "{line}");
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{MentalPrivacyState, OuterVoice, RecentPersonaEvidence, SelfContinuity};
    use serde_json::json;

    #[test]
    fn default_temperament_continuity_is_not_meaningful() {
        assert!(!TemperamentContinuity::default().is_meaningful());
    }

    #[test]
    fn render_temperament_continuity_block_skips_empty_state() {
        assert_eq!(
            render_temperament_continuity_block(&TemperamentContinuity::default(), 512),
            None
        );
    }

    #[test]
    fn render_temperament_continuity_block_returns_none_when_budget_cannot_hold_context() {
        let state = TemperamentContinuity {
            conversational_inertia: "compact and direct".into(),
            ..TemperamentContinuity::default()
        };

        assert_eq!(render_temperament_continuity_block(&state, 1), None);
    }

    #[test]
    fn normalize_temperament_continuity_trims_and_truncates_fields() {
        let state = TemperamentContinuity {
            conversational_inertia: "  compact and direct  ".into(),
            boundary_inertia: "   ".into(),
            stability_summary: format!("  {}  ", "steady ".repeat(80)),
            updated_at: 1,
            ..TemperamentContinuity::default()
        };

        let normalized = normalize_temperament_continuity(state, 66).expect("meaningful state");
        assert_eq!(normalized.conversational_inertia, "compact and direct");
        assert!(normalized.boundary_inertia.is_empty());
        assert!(
            normalized.stability_summary.chars().count() <= TEMPERAMENT_CONTINUITY_FIELD_MAX_CHARS
        );
        assert_eq!(normalized.updated_at, 66);
    }

    #[test]
    fn build_temperament_continuity_refresh_input_uses_recent_persona_boundary_and_voice() {
        let input = build_temperament_continuity_refresh_input(
            Some(&TemperamentContinuity {
                stability_summary: "existing tendency is direct but careful".into(),
                updated_at: 8,
                ..TemperamentContinuity::default()
            }),
            Some(&RecentPersonaEvidence {
                repeated_response_mode: "dense implementation answers".into(),
                repeated_relationship_posture: "architecture partner".into(),
                pressure_pattern: "Normal windows preferred".into(),
                ..RecentPersonaEvidence::default()
            }),
            Some(&MentalPrivacyState {
                updated_at: 11,
                ..MentalPrivacyState::default()
            }),
            Some(&OuterVoice {
                boundary_style: "firm, non-performative".into(),
                relational_response_style: "direct technical collaboration".into(),
                ..OuterVoice::default()
            }),
            Some(&SelfContinuity {
                priority_posture: "agent constitution first".into(),
                task_posture: "finish the current stage without tail work".into(),
                ..SelfContinuity::default()
            }),
            4096,
        );

        assert!(input.contains("dense implementation answers"));
        assert!(input.contains("firm, non-performative"));
        assert!(input.contains("agent constitution first"));
        assert!(input.contains("existing tendency is direct but careful"));
        assert!(input.contains("Mental Privacy Boundary"));
    }

    #[test]
    fn build_temperament_continuity_refresh_input_redacts_runtime_identifiers() {
        let input = build_temperament_continuity_refresh_input(
            None,
            None,
            None,
            None,
            Some(&SelfContinuity {
                continuity_bridge: "continue the current engineering stage".into(),
                last_user_turn_at: 1_000,
                last_user_chat_id: "raw-chat-42".into(),
                last_user_channel: "qq_channel".into(),
                last_autonomy_run_at: 1_200,
                ..SelfContinuity::default()
            }),
            4096,
        );

        assert!(input.contains("continue the current engineering stage"));
        assert!(input.contains("last_user_relation=active"));
        assert!(input.contains("last_user_channel_kind=known"));
        assert!(!input.contains("raw-chat-42"));
        assert!(!input.contains("last_user_channel=qq_channel"));
    }

    #[test]
    fn parse_temperament_continuity_response_handles_nested_text_fields() {
        let raw = json!({
            "conversational_inertia": { "style": "direct and compact" },
            "boundary_inertia": ["names architecture drift", "does not flatter"],
            "disagreement_style": true,
            "attachment_rhythm": 3,
            "explanation_habit": { "value": "states why before changing code" },
            "stability_summary": { "summary": "durable tendency is pragmatic architecture review" }
        })
        .to_string();

        let ParsedTemperamentContinuityResponse::Update(parsed) =
            parse_temperament_continuity_response(&raw, 88)
        else {
            panic!("expected parsed temperament continuity");
        };
        assert!(parsed.conversational_inertia.contains("direct and compact"));
        assert!(parsed.boundary_inertia.contains("names architecture drift"));
        assert_eq!(parsed.disagreement_style, "true");
        assert_eq!(parsed.attachment_rhythm, "3");
        assert_eq!(parsed.explanation_habit, "states why before changing code");
        assert_eq!(parsed.updated_at, 88);
    }

    #[test]
    fn parse_temperament_continuity_response_clears_only_on_explicit_null_or_action() {
        assert!(matches!(
            parse_temperament_continuity_response("null", 88),
            ParsedTemperamentContinuityResponse::Clear
        ));
        assert!(matches!(
            parse_temperament_continuity_response(r#"{"action":"clear"}"#, 88),
            ParsedTemperamentContinuityResponse::Clear
        ));
        assert!(matches!(
            parse_temperament_continuity_response("{}", 88),
            ParsedTemperamentContinuityResponse::Skip
        ));
    }
}
