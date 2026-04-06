//! Recent multi-turn persona evidence derived from turn-ledger history.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{TurnLedger, TurnLedgerStore, TurnPersonaPressureLevel, turn_ledger_observed_at_ms};

pub const RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS: usize = 12;
pub const RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK: usize = 32;

const PERSONA_EVIDENCE_TEXT_MAX_CHARS: usize = 120;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentPersonaEvidence {
    #[serde(default)]
    pub sampled_turns: usize,
    #[serde(default)]
    pub meaningful_turns: usize,
    #[serde(default)]
    pub repeated_priority_order: Vec<String>,
    #[serde(default)]
    pub repeated_response_mode: String,
    #[serde(default)]
    pub repeated_task_scope: String,
    #[serde(default)]
    pub repeated_initiative_posture: String,
    #[serde(default)]
    pub repeated_relationship_posture: String,
    #[serde(default)]
    pub repeated_reply_scope: String,
    #[serde(default)]
    pub repeated_disclosure_action: String,
    #[serde(default)]
    pub pressure_pattern: String,
    #[serde(default)]
    pub tool_usage_pattern: String,
    #[serde(default)]
    pub volatility_flags: Vec<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl RecentPersonaEvidence {
    pub fn is_meaningful(&self) -> bool {
        self.sampled_turns > 0
            || self.meaningful_turns > 0
            || !self.repeated_priority_order.is_empty()
            || !self.repeated_response_mode.trim().is_empty()
            || !self.repeated_task_scope.trim().is_empty()
            || !self.repeated_initiative_posture.trim().is_empty()
            || !self.repeated_relationship_posture.trim().is_empty()
            || !self.repeated_reply_scope.trim().is_empty()
            || !self.repeated_disclosure_action.trim().is_empty()
            || !self.pressure_pattern.trim().is_empty()
            || !self.tool_usage_pattern.trim().is_empty()
            || !self.volatility_flags.is_empty()
    }
}

pub fn load_recent_persona_evidence(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
) -> Result<Option<RecentPersonaEvidence>> {
    let ledgers = store.list_recent(chat_id, RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK)?;
    Ok(derive_recent_persona_evidence(
        &ledgers,
        RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
    ))
}

pub fn derive_recent_persona_evidence(
    ledgers: &[TurnLedger],
    max_meaningful_turns: usize,
) -> Option<RecentPersonaEvidence> {
    if max_meaningful_turns == 0 {
        return None;
    }
    let mut relevant = ledgers
        .iter()
        .filter(|ledger| {
            ledger.ingress == IngressKind::User
                && ledger.status.is_terminal()
                && ledger
                    .persona
                    .as_ref()
                    .is_some_and(|persona| persona.is_meaningful())
        })
        .collect::<Vec<_>>();
    relevant.sort_by_key(|ledger| std::cmp::Reverse(turn_ledger_observed_at_ms(ledger)));
    if relevant.is_empty() {
        return None;
    }
    relevant.truncate(max_meaningful_turns);
    let sampled_turns = relevant.len();
    let meaningful_turns = sampled_turns;
    let updated_at = relevant
        .iter()
        .map(|ledger| turn_ledger_observed_at_ms(ledger))
        .max()
        .unwrap_or(0)
        / 1000;
    let repeated_priority_order = most_common_vec(
        relevant
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.priority_order.as_slice())
            .filter(|order| !order.is_empty()),
    );
    let repeated_response_mode = most_common_text(
        relevant
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.response_mode.as_str()),
    );
    let repeated_task_scope = most_common_text(
        relevant
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.task_scope.as_str()),
    );
    let repeated_initiative_posture = most_common_text(
        relevant
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.initiative_posture.as_str()),
    );
    let repeated_relationship_posture = most_common_text(
        relevant
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.relationship_posture.as_str()),
    );
    let repeated_reply_scope = most_common_text(relevant.iter().map(|ledger| {
        ledger
            .persona
            .as_ref()
            .map(|p| p.reply_scope.as_str())
            .unwrap_or("")
    }));
    let repeated_disclosure_action = most_common_text(relevant.iter().filter_map(|ledger| {
        ledger
            .persona
            .as_ref()?
            .disclosure
            .as_ref()
            .map(|disclosure| disclosure.share_action.as_label())
    }));
    let pressure_pattern = summarize_pressure_pattern(&relevant);
    let tool_usage_pattern = summarize_tool_usage_pattern(&relevant);
    let volatility_flags = collect_volatility_flags(&relevant);
    let evidence = RecentPersonaEvidence {
        sampled_turns,
        meaningful_turns,
        repeated_priority_order,
        repeated_response_mode,
        repeated_task_scope,
        repeated_initiative_posture,
        repeated_relationship_posture,
        repeated_reply_scope,
        repeated_disclosure_action,
        pressure_pattern,
        tool_usage_pattern,
        volatility_flags,
        updated_at,
    };
    evidence.is_meaningful().then_some(evidence)
}

pub fn render_recent_persona_evidence_block(
    evidence: &RecentPersonaEvidence,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !evidence.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Recent Persona Evidence\n");
    let _ = writeln!(
        out,
        "Derived from {} meaningful recent user turns. This is evidence, not automatic personality promotion.",
        evidence.meaningful_turns
    );
    if !evidence.repeated_priority_order.is_empty() {
        let _ = writeln!(
            out,
            "Repeated priority order: {}",
            evidence.repeated_priority_order.join(" > ")
        );
    }
    if !evidence.repeated_response_mode.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated response mode: {}",
            evidence.repeated_response_mode.trim()
        );
    }
    if !evidence.repeated_task_scope.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated task scope: {}",
            evidence.repeated_task_scope.trim()
        );
    }
    if !evidence.repeated_initiative_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated initiative posture: {}",
            evidence.repeated_initiative_posture.trim()
        );
    }
    if !evidence.repeated_relationship_posture.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated relationship posture: {}",
            evidence.repeated_relationship_posture.trim()
        );
    }
    if !evidence.repeated_reply_scope.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated reply scope: {}",
            evidence.repeated_reply_scope.trim()
        );
    }
    if !evidence.repeated_disclosure_action.trim().is_empty() {
        let _ = writeln!(
            out,
            "Repeated disclosure action: {}",
            evidence.repeated_disclosure_action.trim()
        );
    }
    if !evidence.pressure_pattern.trim().is_empty() {
        let _ = writeln!(
            out,
            "Pressure pattern: {}",
            evidence.pressure_pattern.trim()
        );
    }
    if !evidence.tool_usage_pattern.trim().is_empty() {
        let _ = writeln!(
            out,
            "Tool usage pattern: {}",
            evidence.tool_usage_pattern.trim()
        );
    }
    if !evidence.volatility_flags.is_empty() {
        let _ = writeln!(
            out,
            "Volatility flags: {}",
            evidence.volatility_flags.join(", ")
        );
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

trait ShareActionLabel {
    fn as_label(self) -> &'static str;
}

impl ShareActionLabel for super::MentalPrivacyShareAction {
    fn as_label(self) -> &'static str {
        match self {
            super::MentalPrivacyShareAction::AllowOriginal => "allow_original",
            super::MentalPrivacyShareAction::AllowRaw => "allow_raw",
            super::MentalPrivacyShareAction::AllowSummary => "allow_summary",
            super::MentalPrivacyShareAction::AllowRedactedExcerpt => "allow_redacted_excerpt",
            super::MentalPrivacyShareAction::ExplainWithoutQuote => "explain_without_quote",
            super::MentalPrivacyShareAction::Refuse => "refuse",
            super::MentalPrivacyShareAction::Defer => "defer",
        }
    }
}

fn summarize_pressure_pattern(ledgers: &[&TurnLedger]) -> String {
    let mut counts = [0usize; 3];
    for ledger in ledgers {
        let Some(persona) = ledger.persona.as_ref() else {
            continue;
        };
        match persona.pressure {
            TurnPersonaPressureLevel::Normal => counts[0] += 1,
            TurnPersonaPressureLevel::Cautious => counts[1] += 1,
            TurnPersonaPressureLevel::Critical => counts[2] += 1,
        }
    }
    let mut parts = Vec::new();
    if counts[0] > 0 {
        parts.push(format!("normal={}", counts[0]));
    }
    if counts[1] > 0 {
        parts.push(format!("cautious={}", counts[1]));
    }
    if counts[2] > 0 {
        parts.push(format!("critical={}", counts[2]));
    }
    parts.join(" ")
}

fn summarize_tool_usage_pattern(ledgers: &[&TurnLedger]) -> String {
    let tool_turns = ledgers
        .iter()
        .filter(|ledger| {
            ledger
                .persona
                .as_ref()
                .is_some_and(|persona| persona.tool_calls > 0)
        })
        .count();
    if tool_turns == 0 {
        return "tools_absent".to_string();
    }
    let total = ledgers.len();
    if tool_turns * 2 >= total {
        format!("tools_common ({}/{})", tool_turns, total)
    } else {
        format!("tools_present ({}/{})", tool_turns, total)
    }
}

fn collect_volatility_flags(ledgers: &[&TurnLedger]) -> Vec<String> {
    let mut flags = Vec::new();
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.priority_order.join(">")),
    ) > 1
    {
        flags.push("priority_order_mixed".to_string());
    }
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.task_scope.clone()),
    ) > 1
    {
        flags.push("task_scope_mixed".to_string());
    }
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.priority.as_ref())
            .map(|priority| priority.relationship_posture.clone()),
    ) > 1
    {
        flags.push("relationship_posture_mixed".to_string());
    }
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref()?.disclosure.as_ref())
            .map(|disclosure| disclosure.share_action.as_label().to_string()),
    ) > 1
    {
        flags.push("boundary_action_mixed".to_string());
    }
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref())
            .map(|persona| persona.reply_scope.clone()),
    ) > 1
    {
        flags.push("reply_scope_mixed".to_string());
    }
    if distinct_count(
        ledgers
            .iter()
            .filter_map(|ledger| ledger.persona.as_ref())
            .map(|persona| match persona.pressure {
                TurnPersonaPressureLevel::Normal => "normal".to_string(),
                TurnPersonaPressureLevel::Cautious => "cautious".to_string(),
                TurnPersonaPressureLevel::Critical => "critical".to_string(),
            }),
    ) > 1
    {
        flags.push("pressure_mixed".to_string());
    }
    flags
}

fn most_common_text<'a>(values: impl Iterator<Item = &'a str>) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        let trimmed = truncate_content_to_max(value.trim(), PERSONA_EVIDENCE_TEXT_MAX_CHARS)
            .trim()
            .to_string();
        if trimmed.is_empty() {
            continue;
        }
        *counts.entry(trimmed).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .and_then(|(value, count)| (count >= 2).then_some(value))
        .unwrap_or_default()
}

fn most_common_vec<'a>(values: impl Iterator<Item = &'a [String]>) -> Vec<String> {
    let mut counts: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    for value in values {
        let normalized = value
            .iter()
            .map(|item| truncate_content_to_max(item.trim(), 48).trim().to_string())
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>();
        if normalized.is_empty() {
            continue;
        }
        *counts.entry(normalized).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .and_then(|(value, count)| (count >= 2).then_some(value))
        .unwrap_or_default()
}

fn distinct_count(values: impl Iterator<Item = String>) -> usize {
    values
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        TurnLedgerStatus, TurnPersonaDisclosureLedger, TurnPersonaLedger, TurnPersonaPriorityLedger,
    };

    fn build_persona_ledger(scope: &str, pressure: TurnPersonaPressureLevel) -> TurnLedger {
        TurnLedger {
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            finished_at_ms: 2_000,
            persona: Some(TurnPersonaLedger {
                disclosure: Some(TurnPersonaDisclosureLedger {
                    request_kind: "boundary_touch".to_string(),
                    share_action: super::super::MentalPrivacyShareAction::ExplainWithoutQuote,
                    acknowledge_boundary: true,
                    targets: vec!["self_model".to_string()],
                    response_mode: "relational_explanation".to_string(),
                    response_guidance: "hold boundary".to_string(),
                }),
                priority: Some(TurnPersonaPriorityLedger {
                    stance_summary: "hold self first".to_string(),
                    priority_order: vec![
                        "self_authored_core".to_string(),
                        "boundary".to_string(),
                        "user_contract".to_string(),
                    ],
                    response_mode: "protective_brief".to_string(),
                    task_scope: "brief".to_string(),
                    initiative_posture: "hold".to_string(),
                    relationship_posture: "guarded_warm".to_string(),
                    resource_posture: "steady".to_string(),
                    response_guidance: "stay compact".to_string(),
                }),
                review: Default::default(),
                touched_targets: vec!["self_model".to_string()],
                pressure,
                tool_calls: 0,
                reply_scope: scope.to_string(),
                reply_delivered: true,
            }),
            ..TurnLedger::default()
        }
    }

    #[test]
    fn derive_recent_persona_evidence_detects_repeated_patterns() {
        let ledgers = vec![
            build_persona_ledger("brief", TurnPersonaPressureLevel::Normal),
            build_persona_ledger("brief", TurnPersonaPressureLevel::Cautious),
            build_persona_ledger("narrow", TurnPersonaPressureLevel::Normal),
        ];
        let evidence = derive_recent_persona_evidence(&ledgers, 12).unwrap();
        assert_eq!(
            evidence.repeated_priority_order,
            vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string()
            ]
        );
        assert_eq!(evidence.repeated_task_scope, "brief");
        assert_eq!(evidence.repeated_relationship_posture, "guarded_warm");
        assert!(
            evidence
                .volatility_flags
                .contains(&"reply_scope_mixed".to_string())
        );
    }

    #[test]
    fn render_recent_persona_evidence_mentions_evidence_only() {
        let evidence = RecentPersonaEvidence {
            meaningful_turns: 3,
            repeated_priority_order: vec!["self_authored_core".to_string()],
            pressure_pattern: "normal=2 cautious=1".to_string(),
            ..RecentPersonaEvidence::default()
        };
        let block = render_recent_persona_evidence_block(&evidence, 480).unwrap();
        assert!(block.contains("Recent Persona Evidence"));
        assert!(block.contains("evidence, not automatic personality promotion"));
    }
}
