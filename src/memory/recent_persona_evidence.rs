//! Recent multi-turn persona evidence derived from turn-ledger history.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::{
    TurnContinuityEvidence, TurnContinuityEvidenceStore, TurnLedger, TurnPersonaPressureLevel,
};

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
    /// Any repeated signal that may help the current turn stay behaviorally continuous.
    /// This includes promotable growth signals and non-promotable operational traces.
    pub fn execution_continuity_signal_count(&self) -> usize {
        [
            !self.repeated_priority_order.is_empty(),
            !self.repeated_response_mode.trim().is_empty(),
            !self.repeated_task_scope.trim().is_empty(),
            !self.repeated_initiative_posture.trim().is_empty(),
            !self.repeated_relationship_posture.trim().is_empty(),
            !self.repeated_reply_scope.trim().is_empty(),
            !self.repeated_disclosure_action.trim().is_empty(),
            !self.pressure_pattern.trim().is_empty(),
            !self.tool_usage_pattern.trim().is_empty(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
    }

    pub fn has_execution_continuity_signals(&self) -> bool {
        self.execution_continuity_signal_count() > 0
    }

    /// Operational traces may stabilize current-turn execution, but they are not by themselves
    /// sufficient evidence for board-level or upward personality promotion.
    pub fn operational_trace_signal_count(&self) -> usize {
        [
            !self.repeated_response_mode.trim().is_empty(),
            !self.repeated_task_scope.trim().is_empty(),
            !self.repeated_initiative_posture.trim().is_empty(),
            !self.repeated_reply_scope.trim().is_empty(),
            !self.pressure_pattern.trim().is_empty(),
            !self.tool_usage_pattern.trim().is_empty(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
    }

    pub fn has_operational_trace_signals(&self) -> bool {
        self.operational_trace_signal_count() > 0
    }

    pub fn is_meaningful(&self) -> bool {
        self.sampled_turns > 0
            || self.meaningful_turns > 0
            || self.has_execution_continuity_signals()
            || !self.volatility_flags.is_empty()
    }

    pub fn promotable_growth_signal_count(&self) -> usize {
        [
            !self.repeated_priority_order.is_empty(),
            !self.repeated_relationship_posture.trim().is_empty(),
            !self.repeated_disclosure_action.trim().is_empty(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
    }

    pub fn has_promotable_growth_signals(&self) -> bool {
        self.promotable_growth_signal_count() > 0
    }

    pub fn promotable_growth_updated_at(&self) -> u64 {
        if self.has_promotable_growth_signals() {
            self.updated_at
        } else {
            0
        }
    }
}

pub fn load_recent_persona_evidence(
    store: &dyn TurnContinuityEvidenceStore,
    chat_id: &str,
) -> Result<Option<RecentPersonaEvidence>> {
    store.recent_persona_evidence(chat_id)
}

pub fn derive_recent_persona_evidence(
    ledgers: &[TurnLedger],
    max_meaningful_turns: usize,
) -> Option<RecentPersonaEvidence> {
    let evidence = ledgers
        .iter()
        .filter_map(TurnContinuityEvidence::from_turn_ledger)
        .collect::<Vec<_>>();
    derive_recent_persona_evidence_from_continuity_evidence(&evidence, max_meaningful_turns)
}

pub fn derive_recent_persona_evidence_from_continuity_evidence(
    evidence: &[TurnContinuityEvidence],
    max_meaningful_turns: usize,
) -> Option<RecentPersonaEvidence> {
    if max_meaningful_turns == 0 {
        return None;
    }
    let mut relevant = evidence
        .iter()
        .filter(|item| {
            item.ingress == IngressKind::User
                && item.status.is_terminal()
                && item
                    .persona
                    .as_ref()
                    .is_some_and(|persona| persona.is_meaningful())
        })
        .collect::<Vec<_>>();
    relevant.sort_by_key(|evidence| std::cmp::Reverse(evidence.observed_at_ms));
    if relevant.is_empty() {
        return None;
    }
    relevant.truncate(max_meaningful_turns);
    let promotable_growth = relevant
        .iter()
        .copied()
        .filter(|ledger| ledger_supports_promotable_persona_growth(ledger))
        .collect::<Vec<_>>();
    let sampled_turns = relevant.len();
    let meaningful_turns = sampled_turns;
    let updated_at = relevant
        .iter()
        .map(|evidence| evidence.observed_at_ms)
        .max()
        .unwrap_or(0)
        / 1000;
    let repeated_priority_order = most_common_vec(
        promotable_growth
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
        promotable_growth
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
        ledger_supports_promotable_persona_growth(ledger)
            .then_some(ledger)?
            .persona
            .as_ref()?
            .disclosure
            .as_ref()
            .map(|disclosure| disclosure.share_action.label())
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

fn ledger_supports_promotable_persona_growth(evidence: &TurnContinuityEvidence) -> bool {
    evidence.ingress == IngressKind::User
        && evidence.status == super::TurnLedgerStatus::Answered
        && evidence.final_reply_delivered
        && !evidence.canonical_reply_source.trim().is_empty()
        && evidence
            .persona
            .as_ref()
            .is_some_and(|persona| persona.pressure != TurnPersonaPressureLevel::Critical)
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
    if evidence.has_promotable_growth_signals() {
        out.push_str(
            "Promotable growth signals below may support upward distillation, but only after constitutional review.\n",
        );
    }
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
    if evidence.has_operational_trace_signals() {
        out.push_str(
            "Operational traces below are supportive context only; they are not sufficient grounds for personality promotion by themselves.\n",
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
    fn label(self) -> &'static str;
}

impl ShareActionLabel for super::MentalPrivacyShareAction {
    fn label(self) -> &'static str {
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

fn summarize_pressure_pattern(ledgers: &[&TurnContinuityEvidence]) -> String {
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

fn summarize_tool_usage_pattern(ledgers: &[&TurnContinuityEvidence]) -> String {
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

fn collect_volatility_flags(ledgers: &[&TurnContinuityEvidence]) -> Vec<String> {
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
            .map(|disclosure| disclosure.share_action.label().to_string()),
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn build_persona_ledger(scope: &str, pressure: TurnPersonaPressureLevel) -> TurnLedger {
        TurnLedger {
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            reason: "final_answer".to_string(),
            outbound_source: "reply".to_string(),
            canonical_reply_source: "final_answer".to_string(),
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            finished_at_ms: 2_000,
            final_reply_delivered: true,
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
        assert!(evidence
            .volatility_flags
            .contains(&"reply_scope_mixed".to_string()));
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
        assert!(block.contains("Operational traces below are supportive context only"));
    }

    #[test]
    fn promotable_growth_signals_ignore_operational_only_patterns() {
        let operational_only = RecentPersonaEvidence {
            meaningful_turns: 6,
            repeated_response_mode: "protective_brief".to_string(),
            repeated_task_scope: "narrow".to_string(),
            repeated_initiative_posture: "answer directly".to_string(),
            pressure_pattern: "cautious=4".to_string(),
            tool_usage_pattern: "tool_calls=4".to_string(),
            updated_at: 42,
            ..RecentPersonaEvidence::default()
        };
        assert_eq!(operational_only.execution_continuity_signal_count(), 5);
        assert!(operational_only.has_execution_continuity_signals());
        assert_eq!(operational_only.operational_trace_signal_count(), 5);
        assert!(operational_only.has_operational_trace_signals());
        assert_eq!(operational_only.promotable_growth_signal_count(), 0);
        assert!(!operational_only.has_promotable_growth_signals());
        assert_eq!(operational_only.promotable_growth_updated_at(), 0);

        let growth_supported = RecentPersonaEvidence {
            repeated_priority_order: vec!["self_authored_core".to_string()],
            repeated_relationship_posture: "warm but bounded".to_string(),
            updated_at: 77,
            ..RecentPersonaEvidence::default()
        };
        assert_eq!(growth_supported.execution_continuity_signal_count(), 2);
        assert_eq!(growth_supported.operational_trace_signal_count(), 0);
        assert_eq!(growth_supported.promotable_growth_signal_count(), 2);
        assert!(growth_supported.has_promotable_growth_signals());
        assert_eq!(growth_supported.promotable_growth_updated_at(), 77);
    }

    #[test]
    fn failed_or_critical_turns_do_not_count_as_promotable_growth() {
        let mut failed_copy = build_persona_ledger("brief", TurnPersonaPressureLevel::Normal);
        failed_copy.reason = "chat_failure_copy".to_string();
        failed_copy.outbound_source = "chat-failure".to_string();
        failed_copy.canonical_reply_source = String::new();

        let critical = build_persona_ledger("brief", TurnPersonaPressureLevel::Critical);

        let evidence = derive_recent_persona_evidence(&[failed_copy, critical], 12)
            .expect("recent persona evidence");

        assert!(evidence.repeated_priority_order.is_empty());
        assert!(evidence.repeated_relationship_posture.is_empty());
        assert!(evidence.repeated_disclosure_action.is_empty());
        assert_eq!(evidence.promotable_growth_signal_count(), 0);
        assert!(!evidence.has_promotable_growth_signals());
        assert!(evidence.has_operational_trace_signals());
    }

    #[test]
    fn derive_recent_persona_evidence_accepts_turn_continuity_evidence() {
        let first = build_persona_ledger("brief", TurnPersonaPressureLevel::Normal);
        let mut second = build_persona_ledger("brief", TurnPersonaPressureLevel::Normal);
        second.updated_at_ms = 3_000;
        second.finished_at_ms = 3_000;
        let first_evidence = TurnContinuityEvidence::from_turn_ledger(&first)
            .expect("terminal persona ledger should produce continuity evidence");
        let second_evidence = TurnContinuityEvidence::from_turn_ledger(&second)
            .expect("terminal persona ledger should produce continuity evidence");

        let derived = derive_recent_persona_evidence_from_continuity_evidence(
            &[first_evidence, second_evidence],
            12,
        )
        .expect("recent persona evidence");

        assert_eq!(derived.meaningful_turns, 2);
        assert_eq!(derived.repeated_task_scope, "brief");
        assert_eq!(
            derived.repeated_priority_order,
            vec![
                "self_authored_core".to_string(),
                "boundary".to_string(),
                "user_contract".to_string()
            ]
        );
    }

    struct FastPathStore {
        list_recent_calls: AtomicUsize,
        evidence: Option<RecentPersonaEvidence>,
    }

    impl TurnContinuityEvidenceStore for FastPathStore {
        fn append(&self, _chat_id: &str, _evidence: &TurnContinuityEvidence) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_recent(
            &self,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<TurnContinuityEvidence>> {
            self.list_recent_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Vec::new())
        }

        fn recent_persona_evidence(&self, _chat_id: &str) -> Result<Option<RecentPersonaEvidence>> {
            Ok(self.evidence.clone())
        }
    }

    #[test]
    fn load_recent_persona_evidence_uses_continuity_evidence_store_fast_path() {
        let expected = RecentPersonaEvidence {
            meaningful_turns: 4,
            repeated_reply_scope: "brief".to_string(),
            ..RecentPersonaEvidence::default()
        };
        let store = FastPathStore {
            list_recent_calls: AtomicUsize::new(0),
            evidence: Some(expected.clone()),
        };

        let actual = load_recent_persona_evidence(&store, "chat-1").unwrap();

        assert_eq!(actual, Some(expected));
        assert_eq!(store.list_recent_calls.load(Ordering::Relaxed), 0);
    }
}
