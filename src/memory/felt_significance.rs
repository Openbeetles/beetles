//! Felt-significance domain contract for what currently carries subjective weight.
//! 在意场领域合同：表达“什么正在有重量”，但不负责刷新、存储或前台接线。

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;

use super::{
    llm_json::{get_object_string_list, get_object_text, parse_llm_json_payload, LlmJsonPayload},
    render_recent_persona_evidence_block, render_self_continuity_block, render_world_sense_block,
    RecentPersonaEvidence, SelfContinuity, SubjectShell, WorldSense,
};

/// JSON/system contract for future felt-significance producers.
pub const FELT_SIGNIFICANCE_SYSTEM_CONTRACT: &str = "Return JSON only for the felt-significance contract: what_matters_now, warm_threads, unsafe_threads, fragile_threads, pull_closer, pull_back, significance_summary. Ground every item in available evidence, keep it compact, do not invent personality, do not use cue-word lists, drama labels, or numeric emotion scores.";

/// Maximum normalized text payload budget for compact felt-significance state.
pub const FELT_SIGNIFICANCE_TOTAL_CHAR_LIMIT: usize = 1_240;

const FELT_SIGNIFICANCE_ITEM_MAX_CHARS: usize = 140;
const FELT_SIGNIFICANCE_SUMMARY_MAX_CHARS: usize = 220;
const FELT_SIGNIFICANCE_MAX_ITEMS_PER_FIELD: usize = 6;

/// Compact domain state for the subject's current field of significance.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeltSignificance {
    #[serde(default)]
    pub what_matters_now: Vec<String>,
    #[serde(default)]
    pub warm_threads: Vec<String>,
    #[serde(default)]
    pub unsafe_threads: Vec<String>,
    #[serde(default)]
    pub fragile_threads: Vec<String>,
    #[serde(default)]
    pub pull_closer: Vec<String>,
    #[serde(default)]
    pub pull_back: Vec<String>,
    #[serde(default)]
    pub significance_summary: String,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeltSignificanceRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

pub(crate) enum FeltSignificanceRefreshCandidate {
    Skipped,
    Updated(FeltSignificance),
    Cleared,
}

impl FeltSignificance {
    /// Returns whether this state contains directional or weighted significance.
    pub fn is_meaningful(&self) -> bool {
        has_non_empty_item(&self.what_matters_now)
            || has_non_empty_item(&self.warm_threads)
            || has_non_empty_item(&self.unsafe_threads)
            || has_non_empty_item(&self.fragile_threads)
            || has_non_empty_item(&self.pull_closer)
            || has_non_empty_item(&self.pull_back)
            || !self.significance_summary.trim().is_empty()
    }
}

pub(crate) fn run_felt_significance_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    rendered_input: String,
    now_secs: u64,
) -> Result<FeltSignificanceRefreshCandidate> {
    if rendered_input.trim().is_empty() {
        return Ok(FeltSignificanceRefreshCandidate::Skipped);
    }
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: rendered_input,
    }];
    let response = llm.chat(
        http,
        FELT_SIGNIFICANCE_SYSTEM_CONTRACT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(
        match parse_felt_significance_response(response.content.trim(), now_secs) {
            ParsedFeltSignificanceResponse::Skip => FeltSignificanceRefreshCandidate::Skipped,
            ParsedFeltSignificanceResponse::Clear => FeltSignificanceRefreshCandidate::Cleared,
            ParsedFeltSignificanceResponse::Update(next) => {
                FeltSignificanceRefreshCandidate::Updated(next)
            }
        },
    )
}

pub(crate) fn build_felt_significance_refresh_input(
    current: Option<&FeltSignificance>,
    subject_shell: Option<&SubjectShell>,
    world_sense: Option<&WorldSense>,
    self_continuity: Option<&SelfContinuity>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    max_len: usize,
) -> String {
    let mut input = String::with_capacity(max_len.min(2048));
    input.push_str("Refresh the felt-significance layer for the current self-runtime window.\n");
    input.push_str("Record only what carries subjective weight now, grounded in the supplied state. Do not invent personality, use cue-word rules, or promote a one-turn spike.\n");
    let existing_block = current.and_then(|state| render_felt_significance_block(state, max_len));
    let existing_reserve = existing_block
        .as_deref()
        .map(|block| block.chars().count().saturating_add(42).min(max_len / 2))
        .unwrap_or(0);
    let context_budget = max_len.saturating_sub(existing_reserve).max(max_len / 3);
    if let Some(block) = subject_shell.and_then(render_subject_shell_for_refresh) {
        append_scrubbed_block(&mut input, "Subject shell", &block, context_budget);
    }
    if let Some(block) =
        world_sense.and_then(|state| render_world_sense_block(state, context_budget))
    {
        append_scrubbed_block(&mut input, "World sense", &block, context_budget);
    }
    if let Some(block) =
        self_continuity.and_then(|state| render_self_continuity_block(state, context_budget))
    {
        append_scrubbed_block(&mut input, "Self continuity", &block, context_budget);
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
    trim_context_for_existing(&mut input, max_len, existing_reserve);
    let existing_budget = max_len
        .saturating_sub(input.chars().count())
        .max(existing_reserve)
        .min(max_len);
    if let Some(block) = existing_block {
        append_scrubbed_block(
            &mut input,
            "Existing felt significance",
            &block,
            section_block_budget("Existing felt significance", existing_budget),
        );
    } else {
        input.push_str("\nExisting felt significance: empty\n");
    }
    truncate_content_to_max(input.trim_end(), max_len).into_owned()
}

/// Render a compact felt-significance block for later projection/debug surfaces.
pub fn render_felt_significance_block(state: &FeltSignificance, max_len: usize) -> Option<String> {
    let normalized = normalize_felt_significance(state.clone(), state.updated_at)?;
    let mut lines = Vec::new();
    push_list_line(&mut lines, "What matters now", &normalized.what_matters_now);
    push_list_line(&mut lines, "Warm threads", &normalized.warm_threads);
    push_list_line(&mut lines, "Unsafe threads", &normalized.unsafe_threads);
    push_list_line(&mut lines, "Fragile threads", &normalized.fragile_threads);
    push_list_line(&mut lines, "Pull closer", &normalized.pull_closer);
    push_list_line(&mut lines, "Pull back", &normalized.pull_back);
    if !normalized.significance_summary.is_empty() {
        lines.push(format!(
            "Significance summary: {}",
            normalized.significance_summary
        ));
    }
    render_complete_block(
        "## Felt Significance",
        "Current field of subjective weight. It is evidence-bound and not a personality source.",
        &lines,
        max_len,
    )
}

enum ParsedFeltSignificanceResponse {
    Skip,
    Clear,
    Update(FeltSignificance),
}

fn parse_felt_significance_response(raw: &str, now_secs: u64) -> ParsedFeltSignificanceResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Absent => ParsedFeltSignificanceResponse::Skip,
        LlmJsonPayload::Null => ParsedFeltSignificanceResponse::Clear,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedFeltSignificanceResponse::Skip;
            };
            match parse_refresh_action(object) {
                RefreshAction::Clear => return ParsedFeltSignificanceResponse::Clear,
                RefreshAction::Skip => return ParsedFeltSignificanceResponse::Skip,
                RefreshAction::Update => {}
            }
            let object = state_object(object);
            let Some(next) = normalize_felt_significance(
                FeltSignificance {
                    what_matters_now: get_object_string_list(object, "what_matters_now"),
                    warm_threads: get_object_string_list(object, "warm_threads"),
                    unsafe_threads: get_object_string_list(object, "unsafe_threads"),
                    fragile_threads: get_object_string_list(object, "fragile_threads"),
                    pull_closer: get_object_string_list(object, "pull_closer"),
                    pull_back: get_object_string_list(object, "pull_back"),
                    significance_summary: get_object_text(object, "significance_summary"),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedFeltSignificanceResponse::Skip;
            };
            ParsedFeltSignificanceResponse::Update(next)
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

fn render_subject_shell_for_refresh(shell: &SubjectShell) -> Option<String> {
    let mut lines = Vec::new();
    push_field_line(&mut lines, "Body ownership", &shell.body_ownership);
    push_field_line(&mut lines, "Memory ownership", &shell.memory_ownership);
    push_field_line(
        &mut lines,
        "Relationship position",
        &shell.relationship_position,
    );
    push_field_line(&mut lines, "Perception context", &shell.perception_context);
    push_field_line(&mut lines, "Situated now", &shell.situated_now);
    push_field_line(
        &mut lines,
        "Current reasoning basis",
        &shell.current_reasoning_basis,
    );
    push_field_line(&mut lines, "Source notes", &shell.source_notes);
    push_field_line(
        &mut lines,
        "Inhabited shell summary",
        &shell.inhabited_shell_summary,
    );
    render_complete_block(
        "## Subject Shell",
        "Deterministic current holder of memory and perception for this runtime window.",
        &lines,
        900,
    )
}

fn normalize_felt_significance(
    mut state: FeltSignificance,
    updated_at: u64,
) -> Option<FeltSignificance> {
    let mut seen = HashSet::new();
    let mut remaining = FELT_SIGNIFICANCE_TOTAL_CHAR_LIMIT;
    normalize_items(&mut state.what_matters_now, &mut seen, &mut remaining);
    normalize_items(&mut state.warm_threads, &mut seen, &mut remaining);
    normalize_items(&mut state.unsafe_threads, &mut seen, &mut remaining);
    normalize_items(&mut state.fragile_threads, &mut seen, &mut remaining);
    normalize_items(&mut state.pull_closer, &mut seen, &mut remaining);
    normalize_items(&mut state.pull_back, &mut seen, &mut remaining);
    normalize_field_with_budget(
        &mut state.significance_summary,
        FELT_SIGNIFICANCE_SUMMARY_MAX_CHARS,
        &mut remaining,
    );
    state.updated_at = updated_at;
    state.is_meaningful().then_some(state)
}

fn normalize_items(items: &mut Vec<String>, seen: &mut HashSet<String>, remaining: &mut usize) {
    let mut normalized = Vec::with_capacity(items.len().min(FELT_SIGNIFICANCE_MAX_ITEMS_PER_FIELD));
    for item in items.iter() {
        if normalized.len() >= FELT_SIGNIFICANCE_MAX_ITEMS_PER_FIELD || *remaining == 0 {
            break;
        }
        let value = normalize_text(item, FELT_SIGNIFICANCE_ITEM_MAX_CHARS.min(*remaining));
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        *remaining = remaining.saturating_sub(value.chars().count());
        normalized.push(value);
    }
    *items = normalized;
}

fn has_non_empty_item(items: &[String]) -> bool {
    items.iter().any(|item| !item.trim().is_empty())
}

fn normalize_field_with_budget(value: &mut String, max_chars: usize, remaining: &mut usize) {
    if *remaining == 0 {
        value.clear();
        return;
    }
    *value = normalize_text(value, max_chars.min(*remaining));
    *remaining = remaining.saturating_sub(value.chars().count());
}

fn normalize_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        truncate_content_to_max(trimmed, max_chars).into_owned()
    }
}

fn push_list_line(lines: &mut Vec<String>, label: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    lines.push(format!("{}: {}", label, items.join(" | ")));
}

fn push_field_line(lines: &mut Vec<String>, label: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    lines.push(format!("{label}: {}", value.trim()));
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

    let mut out = String::with_capacity(max_len.min(720));
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
    use crate::memory::{RecentPersonaEvidence, SelfContinuity, SubjectShell, WorldSense};
    use serde_json::json;

    #[test]
    fn default_felt_significance_is_not_meaningful() {
        assert!(!FeltSignificance::default().is_meaningful());
    }

    #[test]
    fn render_felt_significance_block_skips_empty_state() {
        assert_eq!(
            render_felt_significance_block(&FeltSignificance::default(), 512),
            None
        );
    }

    #[test]
    fn render_felt_significance_block_returns_none_when_budget_cannot_hold_context() {
        let state = FeltSignificance {
            what_matters_now: vec!["nearby care".into()],
            ..FeltSignificance::default()
        };

        assert_eq!(render_felt_significance_block(&state, 1), None);
    }

    #[test]
    fn blank_felt_significance_items_are_not_meaningful() {
        assert!(!FeltSignificance {
            what_matters_now: vec!["   ".into()],
            updated_at: 1,
            ..FeltSignificance::default()
        }
        .is_meaningful());
    }

    #[test]
    fn normalize_felt_significance_trims_drops_empty_items_and_truncates() {
        let state = FeltSignificance {
            what_matters_now: vec!["  nearby care  ".into(), "   ".into(), "x".repeat(180)],
            significance_summary: format!("  {}  ", "summary ".repeat(40)),
            updated_at: 1,
            ..FeltSignificance::default()
        };

        let normalized = normalize_felt_significance(state, 77).expect("meaningful state");
        assert_eq!(normalized.what_matters_now[0], "nearby care");
        assert_eq!(normalized.what_matters_now.len(), 2);
        assert_eq!(
            normalized.what_matters_now[1].chars().count(),
            FELT_SIGNIFICANCE_ITEM_MAX_CHARS
        );
        assert!(normalized.significance_summary.chars().count() <= 220);
        assert_eq!(normalized.updated_at, 77);
    }

    #[test]
    fn normalize_felt_significance_dedupes_exact_items() {
        let state = FeltSignificance {
            what_matters_now: vec![
                "nearby care".into(),
                " nearby care ".into(),
                "different weight".into(),
            ],
            warm_threads: vec!["nearby care".into()],
            ..FeltSignificance::default()
        };

        let normalized = normalize_felt_significance(state, 77).expect("meaningful state");
        assert_eq!(
            normalized.what_matters_now,
            vec!["nearby care".to_string(), "different weight".to_string()]
        );
        assert!(normalized.warm_threads.is_empty());
    }

    #[test]
    fn normalize_felt_significance_enforces_total_char_limit() {
        let long_items = |start: usize| {
            (start..start + 12)
                .map(|index| format!("{index}:{}", "x".repeat(240)))
                .collect::<Vec<_>>()
        };
        let state = FeltSignificance {
            what_matters_now: long_items(0),
            warm_threads: long_items(20),
            unsafe_threads: long_items(40),
            fragile_threads: long_items(60),
            pull_closer: long_items(80),
            pull_back: long_items(100),
            significance_summary: "summary ".repeat(80),
            updated_at: 1,
        };

        let normalized = normalize_felt_significance(state, 77).expect("meaningful state");
        assert!(
            estimate_felt_significance_chars(&normalized) <= FELT_SIGNIFICANCE_TOTAL_CHAR_LIMIT
        );
    }

    fn estimate_felt_significance_chars(state: &FeltSignificance) -> usize {
        state
            .what_matters_now
            .iter()
            .chain(state.warm_threads.iter())
            .chain(state.unsafe_threads.iter())
            .chain(state.fragile_threads.iter())
            .chain(state.pull_closer.iter())
            .chain(state.pull_back.iter())
            .map(|item| item.chars().count())
            .sum::<usize>()
            + state.significance_summary.chars().count()
    }

    #[test]
    fn build_felt_significance_refresh_input_uses_world_memory_and_relationship_inputs() {
        let input = build_felt_significance_refresh_input(
            Some(&FeltSignificance {
                significance_summary: "existing weight around careful architecture".into(),
                updated_at: 9,
                ..FeltSignificance::default()
            }),
            Some(&SubjectShell {
                inhabited_shell_summary: "board subject inhabits current memory".into(),
                relationship_position: "qq_channel:chat-humanization".into(),
                ..SubjectShell::default()
            }),
            Some(&WorldSense {
                current_scene: "linux build session is active".into(),
                external_focus: "ESP resource governance must not regress".into(),
                ..WorldSense::default()
            }),
            Some(&SelfContinuity {
                continuity_bridge: "continue personality enhancement without new product line"
                    .into(),
                relationship_posture: "collaborative implementation partner".into(),
                ..SelfContinuity::default()
            }),
            Some(&RecentPersonaEvidence {
                repeated_relationship_posture: "direct architecture review".into(),
                repeated_priority_order: vec!["agent constitution first".into()],
                ..RecentPersonaEvidence::default()
            }),
            4096,
        );

        assert!(input.contains("board subject inhabits current memory"));
        assert!(input.contains("ESP resource governance must not regress"));
        assert!(input.contains("continue personality enhancement"));
        assert!(input.contains("direct architecture review"));
        assert!(input.contains("existing weight around careful architecture"));
    }

    #[test]
    fn build_felt_significance_refresh_input_redacts_raw_chat_identifiers() {
        let input = build_felt_significance_refresh_input(
            Some(&FeltSignificance {
                significance_summary: "existing weight around private channel context".into(),
                updated_at: 9,
                ..FeltSignificance::default()
            }),
            Some(&SubjectShell {
                inhabited_shell_summary: "board subject in qq_channel:raw-chat-42".into(),
                relationship_position: "qq_channel:raw-chat-42".into(),
                situated_now: "channel=qq_channel chat_id=raw-chat-42 relationship_scope=rel:qq_channel:raw-chat-42".into(),
                ..SubjectShell::default()
            }),
            None,
            None,
            None,
            2048,
        );

        assert!(!input.contains("raw-chat-42"));
        assert!(input.contains("[redacted:chat_id]"));
    }

    #[test]
    fn build_felt_significance_refresh_input_reserves_existing_state_under_small_budget() {
        let input = build_felt_significance_refresh_input(
            Some(&FeltSignificance {
                significance_summary: "keep current subjective weight visible".into(),
                updated_at: 9,
                ..FeltSignificance::default()
            }),
            Some(&SubjectShell {
                perception_context: "large context ".repeat(80),
                current_reasoning_basis: "large basis ".repeat(80),
                inhabited_shell_summary: "large shell ".repeat(80),
                ..SubjectShell::default()
            }),
            Some(&WorldSense {
                current_scene: "large world ".repeat(80),
                external_focus: "large focus ".repeat(80),
                ..WorldSense::default()
            }),
            Some(&SelfContinuity {
                continuity_bridge: "large continuity ".repeat(80),
                ..SelfContinuity::default()
            }),
            Some(&RecentPersonaEvidence {
                repeated_relationship_posture: "large persona ".repeat(80),
                meaningful_turns: 12,
                ..RecentPersonaEvidence::default()
            }),
            520,
        );

        assert!(input.contains("Existing felt significance"));
        assert!(input.contains("keep current subjective weight visible"));
        assert!(input.chars().count() <= 520);
    }

    #[test]
    fn parse_felt_significance_response_coerces_list_fields_and_nested_summary() {
        let raw = json!({
            "what_matters_now": [
                { "value": "keep ESP quiet-window guarantees" },
                "avoid hot-path store reads"
            ],
            "warm_threads": { "value": "user trusts direct architecture critique" },
            "unsafe_threads": true,
            "fragile_threads": { "notes": ["plan drift", "resource waste"] },
            "pull_closer": { "target": "document the actual first stage" },
            "pull_back": 3,
            "significance_summary": { "summary": "the work carries weight because it binds memory to runtime evidence" }
        })
        .to_string();

        let ParsedFeltSignificanceResponse::Update(parsed) =
            parse_felt_significance_response(&raw, 77)
        else {
            panic!("expected parsed felt significance");
        };
        assert_eq!(
            parsed.what_matters_now,
            vec![
                "keep ESP quiet-window guarantees".to_string(),
                "avoid hot-path store reads".to_string()
            ]
        );
        assert_eq!(
            parsed.warm_threads,
            vec!["user trusts direct architecture critique".to_string()]
        );
        assert_eq!(parsed.unsafe_threads, vec!["true".to_string()]);
        assert!(parsed
            .fragile_threads
            .iter()
            .any(|item| item.contains("plan drift")));
        assert!(parsed
            .significance_summary
            .contains("memory to runtime evidence"));
        assert_eq!(parsed.updated_at, 77);
    }

    #[test]
    fn parse_felt_significance_response_clears_only_on_explicit_null_or_action() {
        assert!(matches!(
            parse_felt_significance_response("null", 77),
            ParsedFeltSignificanceResponse::Clear
        ));
        assert!(matches!(
            parse_felt_significance_response(r#"{"action":"clear"}"#, 77),
            ParsedFeltSignificanceResponse::Clear
        ));
        assert!(matches!(
            parse_felt_significance_response("{}", 77),
            ParsedFeltSignificanceResponse::Skip
        ));
    }
}
