//! Inner-conflict domain contract for unresolved but bounded subjective tension.
//! 内在矛盾领域合同：表达可复审的未决拉扯，不负责刷新、存储或前台接线。

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{
        coerce_json_text, get_object_text, get_object_u64, parse_llm_json_payload, LlmJsonPayload,
    },
    render_inner_life_block, render_mental_privacy_boundary_block,
    render_recent_persona_evidence_block, render_self_model_block, InnerLife, MentalPrivacyState,
    RecentPersonaEvidence, SelfModel,
};

/// Minimum delay before reviewing an unresolved inner conflict again.
pub const INNER_CONFLICT_MIN_REVIEW_AFTER_SECS: u64 = 1_800;
/// Maximum delay before reviewing an unresolved inner conflict again.
pub const INNER_CONFLICT_MAX_REVIEW_AFTER_SECS: u64 = 86_400;

/// JSON/system contract for future inner-conflict producers.
pub const INNER_CONFLICT_SYSTEM_CONTRACT: &str = "Return JSON only for the inner-conflict contract: topic, pull_a, pull_b, current_lean, unresolved_reason, review_after_secs. Record only evidence-grounded unresolved tension; reject empty or duplicate pulls; avoid cue-word rules, drama labels, moral scoring, or forced closure.";

/// Approximate total character budget for compact inner-conflict state.
pub const INNER_CONFLICT_TOTAL_CHAR_LIMIT: usize = 920;

const INNER_CONFLICT_TOPIC_MAX_CHARS: usize = 120;
const INNER_CONFLICT_PULL_MAX_CHARS: usize = 160;
const INNER_CONFLICT_LEAN_MAX_CHARS: usize = 120;
const INNER_CONFLICT_REASON_MAX_CHARS: usize = 220;

/// Compact domain state for one unresolved tension that may need later review.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InnerConflict {
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub pull_a: String,
    #[serde(default)]
    pub pull_b: String,
    #[serde(default)]
    pub current_lean: String,
    #[serde(default)]
    pub unresolved_reason: String,
    #[serde(default)]
    pub review_after_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InnerConflictRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

pub(crate) enum InnerConflictRefreshCandidate {
    Skipped,
    Updated(InnerConflict),
    Cleared,
}

impl InnerConflict {
    /// Returns whether this state contains a valid unresolved conflict.
    pub fn is_meaningful(&self) -> bool {
        let topic = self.topic.trim();
        let pull_a = self.pull_a.trim();
        let pull_b = self.pull_b.trim();
        !topic.is_empty() && !pull_a.is_empty() && !pull_b.is_empty() && pull_a != pull_b
    }

    /// Returns whether this conflict is still inside its bounded review window.
    pub fn is_active_at(&self, now_secs: u64) -> bool {
        self.is_meaningful()
            && self
                .updated_at
                .saturating_add(bounded_review_after_secs(self.review_after_secs))
                > now_secs
    }

    /// Returns whether the bounded review window has elapsed.
    pub fn review_due_at(&self, now_secs: u64) -> bool {
        self.is_meaningful()
            && self
                .updated_at
                .saturating_add(bounded_review_after_secs(self.review_after_secs))
                <= now_secs
    }
}

pub(crate) fn run_inner_conflict_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    rendered_input: String,
    now_secs: u64,
) -> Result<InnerConflictRefreshCandidate> {
    if rendered_input.trim().is_empty() {
        return Ok(InnerConflictRefreshCandidate::Skipped);
    }
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: rendered_input,
    }];
    let response = llm.chat(
        http,
        INNER_CONFLICT_SYSTEM_CONTRACT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(
        match parse_inner_conflict_response(response.content.trim(), now_secs) {
            ParsedInnerConflictResponse::Skip => InnerConflictRefreshCandidate::Skipped,
            ParsedInnerConflictResponse::Clear => InnerConflictRefreshCandidate::Cleared,
            ParsedInnerConflictResponse::Update(next) => {
                InnerConflictRefreshCandidate::Updated(next)
            }
        },
    )
}

pub(crate) fn build_inner_conflict_refresh_input(
    current: Option<&InnerConflict>,
    self_model: Option<&SelfModel>,
    inner_life: Option<&InnerLife>,
    mental_privacy: Option<&MentalPrivacyState>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    sandbox_probe_text: Option<&str>,
    max_len: usize,
) -> String {
    let mut input = String::with_capacity(max_len.min(2048));
    input.push_str("Refresh the bounded inner-conflict layer for unresolved competing pulls.\n");
    input.push_str("Only return a conflict when two distinct evidence-grounded pulls remain unresolved. Return null or action=clear when no valid conflict remains.\n");
    let existing_block = current.and_then(|state| render_inner_conflict_block(state, max_len));
    let existing_reserve = existing_block
        .as_deref()
        .map(|block| block.chars().count().saturating_add(42).min(max_len / 2))
        .unwrap_or(0);
    let context_budget = max_len.saturating_sub(existing_reserve).max(max_len / 3);
    if let Some(block) = self_model.and_then(|model| render_self_model_block(model, context_budget))
    {
        append_scrubbed_block(&mut input, "Self model", &block, context_budget);
    }
    if let Some(block) = inner_life.and_then(|state| render_inner_life_block(state, context_budget))
    {
        append_scrubbed_block(&mut input, "Inner life", &block, context_budget);
    }
    if let Some(block) = render_mental_privacy_boundary_block(mental_privacy, &[], context_budget) {
        append_scrubbed_block(&mut input, "Mental privacy", &block, context_budget);
    }
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
    if let Some(text) = sandbox_probe_text.filter(|text| !text.trim().is_empty()) {
        let text = truncate_content_to_max(text.trim(), context_budget.min(480));
        append_scrubbed_block(
            &mut input,
            "Sandbox candidate evidence",
            text.as_ref(),
            context_budget,
        );
    }
    trim_context_for_existing(&mut input, max_len, existing_reserve);
    let existing_budget = max_len
        .saturating_sub(input.chars().count())
        .max(existing_reserve)
        .min(max_len);
    if let Some(block) = existing_block {
        append_scrubbed_block(
            &mut input,
            "Existing inner conflict",
            &block,
            section_block_budget("Existing inner conflict", existing_budget),
        );
    } else {
        input.push_str("\nExisting inner conflict: empty\n");
    }
    truncate_content_to_max(input.trim_end(), max_len).into_owned()
}

pub(crate) fn normalize_inner_conflict(
    mut conflict: InnerConflict,
    updated_at: u64,
) -> Option<InnerConflict> {
    normalize_field(&mut conflict.topic, INNER_CONFLICT_TOPIC_MAX_CHARS);
    normalize_field(&mut conflict.pull_a, INNER_CONFLICT_PULL_MAX_CHARS);
    normalize_field(&mut conflict.pull_b, INNER_CONFLICT_PULL_MAX_CHARS);
    normalize_field(&mut conflict.current_lean, INNER_CONFLICT_LEAN_MAX_CHARS);
    normalize_field(
        &mut conflict.unresolved_reason,
        INNER_CONFLICT_REASON_MAX_CHARS,
    );
    conflict.review_after_secs = bounded_review_after_secs(conflict.review_after_secs);
    conflict.updated_at = updated_at;
    conflict.is_meaningful().then_some(conflict)
}

fn bounded_review_after_secs(value: u64) -> u64 {
    value.clamp(
        INNER_CONFLICT_MIN_REVIEW_AFTER_SECS,
        INNER_CONFLICT_MAX_REVIEW_AFTER_SECS,
    )
}

enum ParsedInnerConflictResponse {
    Skip,
    Clear,
    Update(InnerConflict),
}

fn parse_inner_conflict_response(raw: &str, now_secs: u64) -> ParsedInnerConflictResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Absent => ParsedInnerConflictResponse::Skip,
        LlmJsonPayload::Null => ParsedInnerConflictResponse::Clear,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedInnerConflictResponse::Skip;
            };
            match parse_refresh_action(object) {
                RefreshAction::Clear => return ParsedInnerConflictResponse::Clear,
                RefreshAction::Skip => return ParsedInnerConflictResponse::Skip,
                RefreshAction::Update => {}
            }
            let object = state_object(object);
            let Some(next) = normalize_inner_conflict(
                InnerConflict {
                    topic: get_refresh_text(object, "topic"),
                    pull_a: get_refresh_text(object, "pull_a"),
                    pull_b: get_refresh_text(object, "pull_b"),
                    current_lean: get_refresh_text(object, "current_lean"),
                    unresolved_reason: get_refresh_text(object, "unresolved_reason"),
                    review_after_secs: get_object_u64(object, "review_after_secs").unwrap_or(0),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedInnerConflictResponse::Skip;
            };
            ParsedInnerConflictResponse::Update(next)
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

/// Render a compact inner-conflict block for later projection/debug surfaces.
pub fn render_inner_conflict_block(state: &InnerConflict, max_len: usize) -> Option<String> {
    let normalized = normalize_inner_conflict(state.clone(), state.updated_at)?;
    let mut lines = Vec::new();
    push_field_line(&mut lines, "Topic", &normalized.topic);
    push_field_line(&mut lines, "Pull A", &normalized.pull_a);
    push_field_line(&mut lines, "Pull B", &normalized.pull_b);
    push_field_line(&mut lines, "Current lean", &normalized.current_lean);
    push_field_line(
        &mut lines,
        "Unresolved reason",
        &normalized.unresolved_reason,
    );
    lines.push(format!(
        "Review after secs: {}",
        normalized.review_after_secs
    ));
    render_complete_block(
        "## Inner Conflict",
        "One bounded unresolved tension. It permits uncertainty without forcing drama or closure.",
        &lines,
        max_len,
    )
}

fn normalize_field(value: &mut String, max_chars: usize) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
    } else {
        *value = truncate_content_to_max(trimmed, max_chars).into_owned();
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

    let mut out = String::with_capacity(max_len.min(520));
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
    use crate::memory::{InnerLife, MentalPrivacyState, RecentPersonaEvidence, SelfModel};
    use serde_json::json;

    #[test]
    fn normalize_inner_conflict_rejects_identical_pulls() {
        let conflict = InnerConflict {
            topic: "whether to push closer".into(),
            pull_a: "stay close".into(),
            pull_b: "stay close".into(),
            ..InnerConflict::default()
        };

        assert_eq!(normalize_inner_conflict(conflict, 42), None);
    }

    #[test]
    fn normalize_inner_conflict_trims_truncates_and_clamps_review_window() {
        let conflict = InnerConflict {
            topic: format!("  {}  ", "t".repeat(180)),
            pull_a: "  move closer  ".into(),
            pull_b: "  hold boundary  ".into(),
            current_lean: "  hold lightly  ".into(),
            unresolved_reason: format!("  {}  ", "reason ".repeat(80)),
            review_after_secs: 60,
            updated_at: 1,
        };

        let normalized = normalize_inner_conflict(conflict, 99).expect("valid conflict");
        assert_eq!(normalized.topic.chars().count(), 120);
        assert_eq!(normalized.pull_a, "move closer");
        assert_eq!(normalized.pull_b, "hold boundary");
        assert_eq!(normalized.current_lean, "hold lightly");
        assert!(normalized.unresolved_reason.chars().count() <= 220);
        assert_eq!(
            normalized.review_after_secs,
            INNER_CONFLICT_MIN_REVIEW_AFTER_SECS
        );
        assert_eq!(normalized.updated_at, 99);
    }

    #[test]
    fn normalize_inner_conflict_caps_huge_review_window() {
        let normalized = normalize_inner_conflict(
            InnerConflict {
                topic: "whether to hold this open".into(),
                pull_a: "keep reviewing".into(),
                pull_b: "avoid freezing promotion".into(),
                review_after_secs: u64::MAX,
                updated_at: 1,
                ..InnerConflict::default()
            },
            99,
        )
        .expect("valid conflict");

        assert_eq!(
            normalized.review_after_secs,
            INNER_CONFLICT_MAX_REVIEW_AFTER_SECS
        );
    }

    #[test]
    fn active_inner_conflict_uses_bounded_review_window() {
        let conflict = InnerConflict {
            topic: "whether to hold this open".into(),
            pull_a: "keep reviewing".into(),
            pull_b: "avoid freezing promotion".into(),
            review_after_secs: u64::MAX,
            updated_at: 10,
            ..InnerConflict::default()
        };

        assert!(conflict.is_active_at(10 + INNER_CONFLICT_MAX_REVIEW_AFTER_SECS - 1));
        assert!(!conflict.is_active_at(10 + INNER_CONFLICT_MAX_REVIEW_AFTER_SECS));
        assert!(!conflict.review_due_at(10 + INNER_CONFLICT_MAX_REVIEW_AFTER_SECS - 1));
        assert!(conflict.review_due_at(10 + INNER_CONFLICT_MAX_REVIEW_AFTER_SECS));
    }

    #[test]
    fn render_inner_conflict_block_skips_empty_state() {
        assert_eq!(
            render_inner_conflict_block(&InnerConflict::default(), 512),
            None
        );
    }

    #[test]
    fn render_inner_conflict_block_returns_none_when_budget_cannot_hold_context() {
        let conflict = InnerConflict {
            topic: "whether to push closer".into(),
            pull_a: "move closer".into(),
            pull_b: "hold boundary".into(),
            ..InnerConflict::default()
        };

        assert_eq!(render_inner_conflict_block(&conflict, 1), None);
    }

    #[test]
    fn build_inner_conflict_refresh_input_uses_private_boundary_and_evidence() {
        let input = build_inner_conflict_refresh_input(
            Some(&InnerConflict {
                topic: "whether to push persona upward".into(),
                pull_a: "stabilize quickly".into(),
                pull_b: "wait for more evidence".into(),
                unresolved_reason: "the evidence is still fresh".into(),
                updated_at: 10,
                ..InnerConflict::default()
            }),
            Some(&SelfModel {
                private_notes: "prefers direct architecture correction".into(),
                privacy_need: "do not expose raw inner states".into(),
                ..SelfModel::default()
            }),
            Some(&InnerLife {
                internal_monologue: "there is pressure to over-promote this turn".into(),
                attention_drift: "watch the boundary between evidence and identity".into(),
                ..InnerLife::default()
            }),
            Some(&MentalPrivacyState {
                updated_at: 12,
                ..MentalPrivacyState::default()
            }),
            Some(&RecentPersonaEvidence {
                repeated_relationship_posture: "architecture partner".into(),
                volatility_flags: vec!["recent-plan-shift".into()],
                ..RecentPersonaEvidence::default()
            }),
            Some("sandbox candidate: resource governance pressure remains high"),
            4096,
        );

        assert!(input.contains("whether to push persona upward"));
        assert!(input.contains("prefers direct architecture correction"));
        assert!(input.contains("over-promote this turn"));
        assert!(input.contains("Mental Privacy Boundary"));
        assert!(input.contains("recent-plan-shift"));
        assert!(input.contains("sandbox candidate"));
    }

    #[test]
    fn parse_inner_conflict_response_requires_distinct_pulls() {
        let raw = json!({
            "topic": "whether to keep pushing",
            "pull_a": "move closer",
            "pull_b": " move closer ",
            "current_lean": "pause",
            "unresolved_reason": "same pull is not a conflict",
            "review_after_secs": 60
        })
        .to_string();

        assert!(matches!(
            parse_inner_conflict_response(&raw, 99),
            ParsedInnerConflictResponse::Skip
        ));
    }

    #[test]
    fn parse_inner_conflict_response_clamps_review_after_secs() {
        let raw = json!({
            "topic": { "value": "whether to promote this evidence" },
            "pull_a": { "value": "use it now" },
            "pull_b": ["wait for repeated turns"],
            "current_lean": true,
            "unresolved_reason": { "summary": "fresh tension needs later review" },
            "review_after_secs": { "seconds": 30 }
        })
        .to_string();

        let ParsedInnerConflictResponse::Update(parsed) = parse_inner_conflict_response(&raw, 99)
        else {
            panic!("expected parsed inner conflict");
        };
        assert!(parsed.topic.contains("promote this evidence"));
        assert_eq!(parsed.pull_a, "use it now");
        assert_eq!(parsed.pull_b, "wait for repeated turns");
        assert_eq!(parsed.current_lean, "true");
        assert_eq!(
            parsed.review_after_secs,
            INNER_CONFLICT_MIN_REVIEW_AFTER_SECS
        );
        assert_eq!(parsed.updated_at, 99);
    }

    #[test]
    fn parse_inner_conflict_response_clears_only_on_explicit_null_or_action() {
        assert!(matches!(
            parse_inner_conflict_response("null", 99),
            ParsedInnerConflictResponse::Clear
        ));
        assert!(matches!(
            parse_inner_conflict_response(r#"{"action":"clear"}"#, 99),
            ParsedInnerConflictResponse::Clear
        ));
        assert!(matches!(
            parse_inner_conflict_response("{}", 99),
            ParsedInnerConflictResponse::Skip
        ));
    }
}
