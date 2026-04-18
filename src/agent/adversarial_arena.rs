use super::counterfactual::{
    CounterfactualAnalysis, CounterfactualBranchKind, CounterfactualBranchProjection,
};
use super::strategy::AgentRunStrategy;
use crate::memory::{TurnAdversarialArenaClaimLedger, TurnAdversarialArenaLedger};
use crate::reasoning::{
    build_adversarial_arena_timeline_event, normalize_arena_claim, AdversarialArenaAdjudication,
    AdversarialArenaDisposition, AdversarialArenaRole, AdversarialArenaSubjectKind,
    AdversarialArenaTimelineEvent,
};
use crate::util::truncate_content_to_max;
use crate::ProgrammableReasoningRuntimeContract;

const ARENA_SUMMARY_MAX_CHARS: usize = 160;
const ARENA_RENDER_MAX_CHARS: usize = 420;
const ARENA_SIGNAL_MAX_ITEMS: usize = 4;
const ARENA_REVISION_MARGIN: i16 = 6;
const ARENA_CLARIFICATION_MARGIN: i16 = 2;

#[derive(Clone, Debug)]
pub(crate) struct TurnStrategyArenaInput<'a> {
    pub(crate) strategy: AgentRunStrategy,
    pub(crate) runtime_contract: ProgrammableReasoningRuntimeContract,
    pub(crate) counterfactual_analysis: Option<&'a CounterfactualAnalysis>,
}

pub(crate) fn compile_turn_strategy_adjudication(
    input: TurnStrategyArenaInput<'_>,
) -> AdversarialArenaAdjudication {
    if input.strategy != AgentRunStrategy::LinuxEnhanced
        || !input.runtime_contract.execution_enabled
    {
        return AdversarialArenaAdjudication::default();
    }
    let Some(counterfactual) = input.counterfactual_analysis else {
        return AdversarialArenaAdjudication::default();
    };
    let Some(challenger_branch) = counterfactual.alternatives.first() else {
        return AdversarialArenaAdjudication::default();
    };

    let snapshot = &counterfactual.snapshot;
    let defender_score = score_claim(&counterfactual.selected_branch, snapshot, false);
    let attacker_score = score_claim(challenger_branch, snapshot, true);
    let defender = normalize_arena_claim(
        AdversarialArenaRole::Defender,
        counterfactual.selected_branch.kind.label(),
        &counterfactual.selected_branch.summary,
        defender_score,
        &counterfactual.selected_branch.rationale,
        counterfactual.selected_branch.requires_native_tool_round,
    );
    let attacker = normalize_arena_claim(
        AdversarialArenaRole::Attacker,
        challenger_branch.kind.label(),
        &challenger_branch.summary,
        attacker_score,
        &challenger_branch.rationale,
        challenger_branch.requires_native_tool_round,
    );

    let defender_score_i16 = i16::from(defender_score);
    let attacker_score_i16 = i16::from(attacker_score);
    let should_hold_for_clarification = challenger_branch.kind
        == CounterfactualBranchKind::ClarifyBeforeAction
        && (snapshot.prefer_explicit_blocker || snapshot.confidence < 75)
        && attacker_score_i16 + ARENA_CLARIFICATION_MARGIN >= defender_score_i16;

    let (disposition, winner) = if should_hold_for_clarification {
        (
            AdversarialArenaDisposition::HoldForClarification,
            attacker.clone(),
        )
    } else if attacker_score_i16 >= defender_score_i16 + ARENA_REVISION_MARGIN {
        (
            AdversarialArenaDisposition::ReviseToAttacker,
            attacker.clone(),
        )
    } else {
        (
            AdversarialArenaDisposition::UpholdDefender,
            defender.clone(),
        )
    };

    let summary = summarize_adjudication(disposition, &winner, &defender, &attacker);
    AdversarialArenaAdjudication {
        subject_kind: AdversarialArenaSubjectKind::TurnStrategy,
        disposition,
        summary,
        winner,
        defender,
        attacker,
    }
}

pub(crate) fn render_adversarial_arena_guidance_block(
    adjudication: &AdversarialArenaAdjudication,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !adjudication.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(ARENA_RENDER_MAX_CHARS));
    out.push_str("## Adversarial Arena\n");
    out.push_str("Adjudication: ");
    out.push_str(adjudication.disposition.label());
    out.push('\n');
    out.push_str("Winner: ");
    out.push_str(adjudication.winner.label.trim());
    out.push('\n');
    out.push_str("Summary: ");
    out.push_str(adjudication.summary.trim());
    if !adjudication.defender.label.trim().is_empty() {
        out.push('\n');
        out.push_str("Defender: ");
        out.push_str(adjudication.defender.label.trim());
    }
    if !adjudication.attacker.label.trim().is_empty() {
        out.push('\n');
        out.push_str("Attacker: ");
        out.push_str(adjudication.attacker.label.trim());
    }
    let rendered =
        truncate_content_to_max(out.trim_end(), max_len.min(ARENA_RENDER_MAX_CHARS)).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn build_turn_adversarial_arena_ledger(
    adjudication: &AdversarialArenaAdjudication,
) -> TurnAdversarialArenaLedger {
    TurnAdversarialArenaLedger {
        subject_kind: adjudication.subject_kind.label().to_string(),
        disposition: adjudication.disposition.label().to_string(),
        summary: truncate_content_to_max(adjudication.summary.trim(), ARENA_SUMMARY_MAX_CHARS)
            .into_owned(),
        winner: arena_claim_to_ledger(&adjudication.winner),
        defender: arena_claim_to_ledger(&adjudication.defender),
        attacker: arena_claim_to_ledger(&adjudication.attacker),
    }
}

pub(crate) fn build_turn_adversarial_arena_timeline_event(
    adjudication: &AdversarialArenaAdjudication,
    recorded_at: u64,
) -> AdversarialArenaTimelineEvent {
    build_adversarial_arena_timeline_event(adjudication, recorded_at)
}

fn arena_claim_to_ledger(
    claim: &crate::reasoning::AdversarialArenaClaim,
) -> TurnAdversarialArenaClaimLedger {
    TurnAdversarialArenaClaimLedger {
        role: claim.role.label().to_string(),
        label: claim.label.clone(),
        evidence_score: claim.evidence_score,
        summary: claim.summary.clone(),
        signals: claim.signals.clone(),
        requires_native_tool_round: claim.requires_native_tool_round,
    }
}

fn summarize_adjudication(
    disposition: AdversarialArenaDisposition,
    winner: &crate::reasoning::AdversarialArenaClaim,
    defender: &crate::reasoning::AdversarialArenaClaim,
    attacker: &crate::reasoning::AdversarialArenaClaim,
) -> String {
    let mut summary = match disposition {
        AdversarialArenaDisposition::UpholdDefender => format!(
            "Defender held: {} stayed ahead of {}.",
            defender.label, attacker.label
        ),
        AdversarialArenaDisposition::ReviseToAttacker => format!(
            "Attacker revised the plan: {} displaced {}.",
            attacker.label, defender.label
        ),
        AdversarialArenaDisposition::HoldForClarification => format!(
            "Arena held for clarification: {} blocked {} until the missing blocker is resolved.",
            attacker.label, defender.label
        ),
    };
    if !winner.signals.is_empty() {
        summary.push_str(" Signals: ");
        summary.push_str(
            &winner
                .signals
                .iter()
                .take(ARENA_SIGNAL_MAX_ITEMS)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    truncate_content_to_max(summary.as_str(), ARENA_SUMMARY_MAX_CHARS).into_owned()
}

fn score_claim(
    branch: &CounterfactualBranchProjection,
    snapshot: &super::counterfactual::CounterfactualTurnSnapshot,
    attacker: bool,
) -> u8 {
    let mut score = i16::from(branch.score);
    if branch.kind == CounterfactualBranchKind::ClarifyBeforeAction
        && (snapshot.prefer_explicit_blocker || snapshot.confidence < 75)
    {
        score += 14;
    }
    if branch.kind == CounterfactualBranchKind::MemoryQuery
        && snapshot.execution_preference
            == super::request_semantics::ExecutionPreference::MemoryFirst
    {
        score += 10;
    }
    if branch.requires_native_tool_round && snapshot.runtime_grounding_required {
        score += 10;
    }
    if branch.kind == CounterfactualBranchKind::DirectReply && snapshot.runtime_grounding_required {
        score -= 18;
    }
    if branch.kind == CounterfactualBranchKind::DirectReply
        && snapshot.action_family != super::request_semantics::ActionFamily::Conversation
    {
        score -= 12;
    }
    if !branch.requires_native_tool_round
        && snapshot.runtime_grounding_required
        && branch.kind != CounterfactualBranchKind::ClarifyBeforeAction
    {
        score -= 8;
    }
    if attacker && branch.kind == CounterfactualBranchKind::DirectReply {
        score -= 4;
    }
    score.clamp(0, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::counterfactual::{
        CounterfactualAnalysis, CounterfactualBranchProjection, CounterfactualTurnSnapshot,
    };
    use crate::agent::request_semantics::{
        ActionFamily, EvidenceNeed, ExecutionPreference, RequestKind,
    };
    use crate::memory::TurnDeliberationClass;
    use crate::{ProgrammableReasoningExecutionBackend, ProgrammableReasoningStage};

    fn linux_runtime_contract() -> ProgrammableReasoningRuntimeContract {
        ProgrammableReasoningRuntimeContract {
            stage: ProgrammableReasoningStage::AdversarialArena,
            linux_only: true,
            execution_backend: ProgrammableReasoningExecutionBackend::LuaSandbox,
            execution_enabled: true,
            proposal_only_persistence: true,
            operator_visible_contract: true,
            user_authored_scripts: false,
            direct_host_tool_execution: false,
            second_execution_plane_forbidden: true,
        }
    }

    #[test]
    fn arena_holds_for_clarification_when_blocker_beats_live_action() {
        let adjudication = compile_turn_strategy_adjudication(TurnStrategyArenaInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            runtime_contract: linux_runtime_contract(),
            counterfactual_analysis: Some(&CounterfactualAnalysis {
                snapshot: CounterfactualTurnSnapshot {
                    request_kind: RequestKind::General,
                    evidence_need: EvidenceNeed::HostTool,
                    execution_preference: ExecutionPreference::ToolFirst,
                    action_family: ActionFamily::ActiveAction,
                    confidence: 62,
                    deliberation_class: TurnDeliberationClass::HardReasoning,
                    compact_reply: false,
                    prefer_explicit_blocker: true,
                    reasoning_kind: super::super::reasoning_intent::ProgrammableReasoningIntentKind::EngineeringSynthesis,
                    reasoning_strategy: super::super::reasoning_intent::ProgrammableReasoningStrategy::RequireNativeToolRound,
                    runtime_grounding_required: true,
                    has_tools: true,
                    active_task_context_present: true,
                    governed_memory_evidence_present: false,
                },
                selected_branch: CounterfactualBranchProjection {
                    kind: CounterfactualBranchKind::StructuredToolSynthesis,
                    score: 88,
                    summary: "Collect live evidence, then synthesize.".to_string(),
                    rationale: vec!["runtime_grounding".to_string()],
                    requires_native_tool_round: true,
                },
                alternatives: vec![CounterfactualBranchProjection {
                    kind: CounterfactualBranchKind::ClarifyBeforeAction,
                    score: 82,
                    summary: "Ask for the missing blocker before acting.".to_string(),
                    rationale: vec!["explicit_blocker".to_string()],
                    requires_native_tool_round: false,
                }],
                summary: "Prefer live synthesis.".to_string(),
            }),
        });

        assert_eq!(
            adjudication.disposition,
            AdversarialArenaDisposition::HoldForClarification
        );
        assert_eq!(adjudication.winner.label, "clarify_before_action");
        assert!(!adjudication.winner_requires_native_tool_round());
    }
}
