use super::deliberation::TurnDeliberationGate;
use super::reasoning_intent::{
    ProgrammableReasoningIntent, ProgrammableReasoningIntentKind, ProgrammableReasoningStrategy,
};
use super::request_semantics::{
    ActionFamily, EvidenceNeed, ExecutionPreference, RequestKind, RequestSemantics,
};
use super::strategy::AgentRunStrategy;
use crate::memory::{
    TurnCounterfactualBranchLedger, TurnCounterfactualLedger, TurnCounterfactualSnapshotLedger,
};
use crate::util::truncate_content_to_max;
use crate::ProgrammableReasoningRuntimeContract;

const COUNTERFACTUAL_SUMMARY_MAX_CHARS: usize = 160;
const COUNTERFACTUAL_BRANCH_MAX_CHARS: usize = 120;
const COUNTERFACTUAL_SIGNAL_MAX_ITEMS: usize = 4;
const COUNTERFACTUAL_SIGNAL_MAX_CHARS: usize = 32;
const COUNTERFACTUAL_ALTERNATIVE_MAX_ITEMS: usize = 2;
const COUNTERFACTUAL_RENDER_MAX_CHARS: usize = 420;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CounterfactualBranchKind {
    #[default]
    DirectReply,
    ClarifyBeforeAction,
    MemoryQuery,
    NativeToolRound,
    StructuredToolSynthesis,
}

impl CounterfactualBranchKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::DirectReply => "direct_reply",
            Self::ClarifyBeforeAction => "clarify_before_action",
            Self::MemoryQuery => "memory_query",
            Self::NativeToolRound => "native_tool_round",
            Self::StructuredToolSynthesis => "structured_tool_synthesis",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CounterfactualTurnSnapshot {
    pub(crate) request_kind: RequestKind,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) action_family: ActionFamily,
    pub(crate) confidence: u8,
    pub(crate) deliberation_class: crate::memory::TurnDeliberationClass,
    pub(crate) compact_reply: bool,
    pub(crate) prefer_explicit_blocker: bool,
    pub(crate) reasoning_kind: ProgrammableReasoningIntentKind,
    pub(crate) reasoning_strategy: ProgrammableReasoningStrategy,
    pub(crate) runtime_grounding_required: bool,
    pub(crate) has_tools: bool,
    pub(crate) active_task_context_present: bool,
    pub(crate) governed_memory_evidence_present: bool,
}

impl Default for CounterfactualTurnSnapshot {
    fn default() -> Self {
        Self {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::None,
            execution_preference: ExecutionPreference::AnswerDirect,
            action_family: ActionFamily::Conversation,
            confidence: 0,
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            compact_reply: false,
            prefer_explicit_blocker: false,
            reasoning_kind: ProgrammableReasoningIntentKind::Disabled,
            reasoning_strategy: ProgrammableReasoningStrategy::Disabled,
            runtime_grounding_required: false,
            has_tools: false,
            active_task_context_present: false,
            governed_memory_evidence_present: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CounterfactualBranchProjection {
    pub(crate) kind: CounterfactualBranchKind,
    pub(crate) score: u8,
    pub(crate) summary: String,
    pub(crate) rationale: Vec<String>,
    pub(crate) requires_native_tool_round: bool,
}

impl CounterfactualBranchProjection {
    pub(crate) fn is_meaningful(&self) -> bool {
        self.score > 0
            || self.kind != CounterfactualBranchKind::DirectReply
            || !self.summary.trim().is_empty()
            || !self.rationale.is_empty()
            || self.requires_native_tool_round
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CounterfactualAnalysis {
    pub(crate) snapshot: CounterfactualTurnSnapshot,
    pub(crate) selected_branch: CounterfactualBranchProjection,
    pub(crate) alternatives: Vec<CounterfactualBranchProjection>,
    pub(crate) summary: String,
}

impl CounterfactualAnalysis {
    pub(crate) fn is_meaningful(&self) -> bool {
        self.selected_branch.is_meaningful()
            || !self.alternatives.is_empty()
            || !self.summary.trim().is_empty()
    }

    pub(crate) fn requires_native_tool_round(&self) -> bool {
        self.selected_branch.requires_native_tool_round
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CounterfactualAnalysisInput<'a> {
    pub(crate) strategy: AgentRunStrategy,
    pub(crate) runtime_contract: ProgrammableReasoningRuntimeContract,
    pub(crate) request_semantics: RequestSemantics,
    pub(crate) deliberation_gate: &'a TurnDeliberationGate,
    pub(crate) reasoning_intent: Option<&'a ProgrammableReasoningIntent>,
    pub(crate) has_tools: bool,
    pub(crate) active_task_context_present: bool,
    pub(crate) governed_memory_evidence_present: bool,
}

pub(crate) fn compile_counterfactual_analysis(
    input: CounterfactualAnalysisInput<'_>,
) -> CounterfactualAnalysis {
    if input.strategy != AgentRunStrategy::LinuxEnhanced
        || !input.runtime_contract.execution_enabled
    {
        return CounterfactualAnalysis::default();
    }

    let snapshot = build_snapshot(&input);
    let mut branches = Vec::with_capacity(5);
    branches.push(score_direct_reply(&snapshot));
    if should_consider_clarify(&snapshot) {
        branches.push(score_clarify_before_action(&snapshot));
    }
    if should_consider_memory_query(&snapshot) {
        branches.push(score_memory_query(&snapshot));
    }
    if should_consider_native_tool_round(&snapshot) {
        branches.push(score_native_tool_round(&snapshot));
    }
    if should_consider_structured_tool_synthesis(&snapshot) {
        branches.push(score_structured_tool_synthesis(&snapshot));
    }
    branches.retain(CounterfactualBranchProjection::is_meaningful);
    branches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| branch_priority(right.kind).cmp(&branch_priority(left.kind)))
    });

    let Some(selected_branch) = branches.first().cloned() else {
        return CounterfactualAnalysis::default();
    };
    let mut remaining = branches.into_iter().skip(1).collect::<Vec<_>>();
    let mut alternatives = Vec::with_capacity(COUNTERFACTUAL_ALTERNATIVE_MAX_ITEMS);
    if selected_branch.kind != CounterfactualBranchKind::DirectReply {
        if let Some(index) = remaining
            .iter()
            .position(|branch| branch.kind == CounterfactualBranchKind::DirectReply)
        {
            alternatives.push(remaining.remove(index));
        }
    }
    for branch in remaining {
        if alternatives.len() >= COUNTERFACTUAL_ALTERNATIVE_MAX_ITEMS {
            break;
        }
        alternatives.push(branch);
    }
    let summary = summarize_counterfactual(&selected_branch, &alternatives);

    CounterfactualAnalysis {
        snapshot,
        selected_branch,
        alternatives,
        summary,
    }
}

pub(crate) fn render_counterfactual_guidance_block(
    analysis: &CounterfactualAnalysis,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !analysis.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(COUNTERFACTUAL_RENDER_MAX_CHARS));
    out.push_str("## Counterfactual Sandbox\n");
    out.push_str("Selected branch: ");
    out.push_str(analysis.selected_branch.kind.label());
    out.push('\n');
    out.push_str("Selected summary: ");
    out.push_str(analysis.selected_branch.summary.trim());
    out.push('\n');
    out.push_str("Decision summary: ");
    out.push_str(analysis.summary.trim());
    if !analysis.selected_branch.rationale.is_empty() {
        out.push('\n');
        out.push_str("Signals: ");
        out.push_str(&analysis.selected_branch.rationale.join(" | "));
    }
    if !analysis.alternatives.is_empty() {
        out.push('\n');
        let rejected = analysis
            .alternatives
            .iter()
            .map(|branch| branch.kind.label())
            .collect::<Vec<_>>();
        out.push_str("Rejected: ");
        out.push_str(&rejected.join(", "));
    }
    let rendered =
        truncate_content_to_max(out.trim_end(), max_len.min(COUNTERFACTUAL_RENDER_MAX_CHARS))
            .into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn build_turn_counterfactual_ledger(
    analysis: &CounterfactualAnalysis,
) -> TurnCounterfactualLedger {
    TurnCounterfactualLedger {
        summary: truncate_content_to_max(analysis.summary.trim(), COUNTERFACTUAL_SUMMARY_MAX_CHARS)
            .into_owned(),
        snapshot: TurnCounterfactualSnapshotLedger {
            request_kind: request_kind_label(analysis.snapshot.request_kind).to_string(),
            evidence_need: evidence_need_label(analysis.snapshot.evidence_need).to_string(),
            execution_preference: execution_preference_label(
                analysis.snapshot.execution_preference,
            )
            .to_string(),
            action_family: action_family_label(analysis.snapshot.action_family).to_string(),
            deliberation_class: analysis.snapshot.deliberation_class.label().to_string(),
            reasoning_kind: analysis.snapshot.reasoning_kind.label().to_string(),
            reasoning_strategy: analysis.snapshot.reasoning_strategy.label().to_string(),
            confidence: analysis.snapshot.confidence,
            runtime_grounding_required: analysis.snapshot.runtime_grounding_required,
            has_tools: analysis.snapshot.has_tools,
            active_task_context_present: analysis.snapshot.active_task_context_present,
            governed_memory_evidence_present: analysis.snapshot.governed_memory_evidence_present,
        },
        selected_branch: branch_projection_to_ledger(&analysis.selected_branch),
        alternatives: analysis
            .alternatives
            .iter()
            .map(branch_projection_to_ledger)
            .collect(),
    }
}

fn build_snapshot(input: &CounterfactualAnalysisInput<'_>) -> CounterfactualTurnSnapshot {
    let reasoning_intent = input.reasoning_intent.cloned().unwrap_or_default();
    CounterfactualTurnSnapshot {
        request_kind: input.request_semantics.request_kind,
        evidence_need: input.request_semantics.evidence_need,
        execution_preference: input.request_semantics.execution_preference,
        action_family: input.request_semantics.action_family,
        confidence: input
            .request_semantics
            .confidence
            .max(reasoning_intent.confidence),
        deliberation_class: input.deliberation_gate.class,
        compact_reply: input.deliberation_gate.compact_reply,
        prefer_explicit_blocker: input.deliberation_gate.prefer_explicit_blocker,
        reasoning_kind: reasoning_intent.kind,
        reasoning_strategy: reasoning_intent.strategy,
        runtime_grounding_required: reasoning_intent.runtime_grounding_required
            || matches!(
                input.request_semantics.evidence_need,
                EvidenceNeed::PublicRuntime | EvidenceNeed::HostTool
            ),
        has_tools: input.has_tools,
        active_task_context_present: input.active_task_context_present,
        governed_memory_evidence_present: input.governed_memory_evidence_present,
    }
}

fn should_consider_clarify(snapshot: &CounterfactualTurnSnapshot) -> bool {
    snapshot.action_family != ActionFamily::Conversation
        || snapshot.prefer_explicit_blocker
        || snapshot.confidence < 75
}

fn should_consider_memory_query(snapshot: &CounterfactualTurnSnapshot) -> bool {
    snapshot.reasoning_kind == ProgrammableReasoningIntentKind::MemoryQuery
        || snapshot.execution_preference == ExecutionPreference::MemoryFirst
        || snapshot.governed_memory_evidence_present
}

fn should_consider_native_tool_round(snapshot: &CounterfactualTurnSnapshot) -> bool {
    snapshot.has_tools
}

fn should_consider_structured_tool_synthesis(snapshot: &CounterfactualTurnSnapshot) -> bool {
    snapshot.has_tools
        && (snapshot.reasoning_kind.prefers_structured_tool_synthesis()
            || snapshot.deliberation_class == crate::memory::TurnDeliberationClass::HardReasoning
            || snapshot.active_task_context_present)
}

fn score_direct_reply(snapshot: &CounterfactualTurnSnapshot) -> CounterfactualBranchProjection {
    let mut score = 35_i16;
    let mut rationale = Vec::with_capacity(4);
    if snapshot.evidence_need == EvidenceNeed::None
        && snapshot.execution_preference == ExecutionPreference::AnswerDirect
    {
        score += 20;
        rationale.push("lightweight_turn".to_string());
    }
    if snapshot.deliberation_class == crate::memory::TurnDeliberationClass::Standard
        && !snapshot.active_task_context_present
        && !snapshot.governed_memory_evidence_present
    {
        score += 15;
        rationale.push("no_extra_grounding".to_string());
    }
    if snapshot.runtime_grounding_required {
        score -= 25;
        rationale.push("runtime_grounding".to_string());
    }
    if snapshot.action_family != ActionFamily::Conversation {
        score -= 20;
        rationale.push("action_in_flight".to_string());
    }
    if snapshot.deliberation_class == crate::memory::TurnDeliberationClass::HardReasoning {
        score -= 15;
        rationale.push("hard_reasoning".to_string());
    }
    if snapshot.reasoning_kind != ProgrammableReasoningIntentKind::Disabled
        && snapshot.reasoning_kind != ProgrammableReasoningIntentKind::TurnLocalReadonly
    {
        score -= 15;
        rationale.push("programmable_reasoning".to_string());
    }
    if snapshot.prefer_explicit_blocker {
        score -= 10;
        rationale.push("explicit_blocker".to_string());
    }
    CounterfactualBranchProjection {
        kind: CounterfactualBranchKind::DirectReply,
        score: clamp_score(score),
        summary: "Answer immediately from the current governed context.".to_string(),
        rationale: normalize_signals(rationale),
        requires_native_tool_round: false,
    }
}

fn score_clarify_before_action(
    snapshot: &CounterfactualTurnSnapshot,
) -> CounterfactualBranchProjection {
    let mut score = 30_i16;
    let mut rationale = Vec::with_capacity(4);
    if snapshot.prefer_explicit_blocker {
        score += 25;
        rationale.push("explicit_blocker".to_string());
    }
    if snapshot.confidence < 75 {
        score += 20;
        rationale.push("low_confidence".to_string());
    }
    if snapshot.action_family != ActionFamily::Conversation {
        score += 15;
        rationale.push("active_action".to_string());
    }
    if !snapshot.active_task_context_present && !snapshot.governed_memory_evidence_present {
        score += 10;
        rationale.push("missing_context".to_string());
    }
    if snapshot.reasoning_kind == ProgrammableReasoningIntentKind::MemoryQuery {
        score -= 10;
    }
    if snapshot.runtime_grounding_required && snapshot.has_tools {
        score -= 10;
        rationale.push("live_grounding_available".to_string());
    }
    CounterfactualBranchProjection {
        kind: CounterfactualBranchKind::ClarifyBeforeAction,
        score: clamp_score(score),
        summary: "Ask for the missing approval, parameter, or blocker detail before acting."
            .to_string(),
        rationale: normalize_signals(rationale),
        requires_native_tool_round: false,
    }
}

fn score_memory_query(snapshot: &CounterfactualTurnSnapshot) -> CounterfactualBranchProjection {
    let mut score = 30_i16;
    let mut rationale = Vec::with_capacity(4);
    if snapshot.reasoning_kind == ProgrammableReasoningIntentKind::MemoryQuery {
        score += 30;
        rationale.push("memory_query_intent".to_string());
    }
    if snapshot.execution_preference == ExecutionPreference::MemoryFirst {
        score += 20;
        rationale.push("memory_first".to_string());
    }
    if snapshot.governed_memory_evidence_present {
        score += 12;
        rationale.push("memory_evidence_present".to_string());
    }
    if matches!(
        snapshot.evidence_need,
        EvidenceNeed::PublicRuntime | EvidenceNeed::HostTool
    ) {
        score -= 15;
        rationale.push("runtime_evidence_needed".to_string());
    }
    if snapshot.action_family == ActionFamily::TaskExecution {
        score -= 10;
        rationale.push("durable_action".to_string());
    }
    CounterfactualBranchProjection {
        kind: CounterfactualBranchKind::MemoryQuery,
        score: clamp_score(score),
        summary: "Interrogate governed memory before committing to a reply.".to_string(),
        rationale: normalize_signals(rationale),
        requires_native_tool_round: false,
    }
}

fn score_native_tool_round(
    snapshot: &CounterfactualTurnSnapshot,
) -> CounterfactualBranchProjection {
    let mut score = 25_i16;
    let mut rationale = Vec::with_capacity(4);
    if snapshot.runtime_grounding_required {
        score += 25;
        rationale.push("runtime_grounding".to_string());
    }
    if matches!(
        snapshot.evidence_need,
        EvidenceNeed::PublicRuntime | EvidenceNeed::HostTool
    ) {
        score += 20;
        rationale.push("host_tool".to_string());
    }
    if matches!(
        snapshot.reasoning_strategy,
        ProgrammableReasoningStrategy::PreferNativeToolRound
            | ProgrammableReasoningStrategy::RequireNativeToolRound
    ) {
        score += 12;
        rationale.push("reasoning_tool_bias".to_string());
    }
    if snapshot.action_family != ActionFamily::Conversation {
        score += 10;
        rationale.push("active_action".to_string());
    }
    if snapshot.execution_preference == ExecutionPreference::MemoryFirst {
        score -= 10;
    }
    CounterfactualBranchProjection {
        kind: CounterfactualBranchKind::NativeToolRound,
        score: clamp_score(score),
        summary: "Probe live runtime and tool evidence before replying.".to_string(),
        rationale: normalize_signals(rationale),
        requires_native_tool_round: true,
    }
}

fn score_structured_tool_synthesis(
    snapshot: &CounterfactualTurnSnapshot,
) -> CounterfactualBranchProjection {
    let mut score = 25_i16;
    let mut rationale = Vec::with_capacity(5);
    if snapshot.reasoning_kind.prefers_structured_tool_synthesis() {
        score += 30;
        rationale.push(snapshot.reasoning_kind.label().to_string());
    }
    if snapshot.deliberation_class == crate::memory::TurnDeliberationClass::HardReasoning {
        score += 20;
        rationale.push("hard_reasoning".to_string());
    }
    if snapshot.active_task_context_present {
        score += 15;
        rationale.push("active_task_context".to_string());
    }
    if snapshot.action_family != ActionFamily::Conversation {
        score += 15;
        rationale.push("action_in_flight".to_string());
    }
    if snapshot.runtime_grounding_required {
        score += 10;
        rationale.push("runtime_grounding".to_string());
    }
    CounterfactualBranchProjection {
        kind: CounterfactualBranchKind::StructuredToolSynthesis,
        score: clamp_score(score),
        summary: "Collect live evidence, then synthesize one coherent action answer.".to_string(),
        rationale: normalize_signals(rationale),
        requires_native_tool_round: true,
    }
}

fn summarize_counterfactual(
    selected: &CounterfactualBranchProjection,
    alternatives: &[CounterfactualBranchProjection],
) -> String {
    if alternatives.is_empty() {
        return truncate_content_to_max(
            format!("Prefer {}.", selected.kind.label()).as_str(),
            COUNTERFACTUAL_SUMMARY_MAX_CHARS,
        )
        .into_owned();
    }
    let rejected = alternatives
        .iter()
        .map(|branch| branch.kind.label())
        .collect::<Vec<_>>()
        .join(" and ");
    truncate_content_to_max(
        format!(
            "Prefer {} over {} because its projected grounding and decision posture are stronger for this turn.",
            selected.kind.label(),
            rejected,
        )
        .as_str(),
        COUNTERFACTUAL_SUMMARY_MAX_CHARS,
    )
    .into_owned()
}

fn branch_projection_to_ledger(
    projection: &CounterfactualBranchProjection,
) -> TurnCounterfactualBranchLedger {
    TurnCounterfactualBranchLedger {
        branch: projection.kind.label().to_string(),
        score: projection.score,
        summary: truncate_content_to_max(
            projection.summary.trim(),
            COUNTERFACTUAL_BRANCH_MAX_CHARS,
        )
        .into_owned(),
        rationale: normalize_signals(projection.rationale.clone()),
        requires_native_tool_round: projection.requires_native_tool_round,
    }
}

fn normalize_signals(signals: Vec<String>) -> Vec<String> {
    signals
        .into_iter()
        .map(|signal| {
            truncate_content_to_max(signal.trim(), COUNTERFACTUAL_SIGNAL_MAX_CHARS)
                .trim()
                .to_string()
        })
        .filter(|signal| !signal.is_empty())
        .fold(Vec::new(), |mut acc, signal| {
            if !acc.contains(&signal) && acc.len() < COUNTERFACTUAL_SIGNAL_MAX_ITEMS {
                acc.push(signal);
            }
            acc
        })
}

fn clamp_score(score: i16) -> u8 {
    score.clamp(0, 100) as u8
}

fn branch_priority(kind: CounterfactualBranchKind) -> u8 {
    match kind {
        CounterfactualBranchKind::StructuredToolSynthesis => 5,
        CounterfactualBranchKind::MemoryQuery => 4,
        CounterfactualBranchKind::NativeToolRound => 3,
        CounterfactualBranchKind::ClarifyBeforeAction => 2,
        CounterfactualBranchKind::DirectReply => 1,
    }
}

fn request_kind_label(kind: RequestKind) -> &'static str {
    match kind {
        RequestKind::General => "general",
        RequestKind::HostDiagnostics => "host_diagnostics",
        RequestKind::MemoryRecall => "memory_recall",
        RequestKind::PrivateMaterialRequest => "private_material_request",
    }
}

fn evidence_need_label(need: EvidenceNeed) -> &'static str {
    match need {
        EvidenceNeed::None => "none",
        EvidenceNeed::PublicRuntime => "public_runtime",
        EvidenceNeed::HostTool => "host_tool",
        EvidenceNeed::ArchiveMemory => "archive_memory",
        EvidenceNeed::CanonicalMemory => "canonical_memory",
    }
}

fn execution_preference_label(preference: ExecutionPreference) -> &'static str {
    match preference {
        ExecutionPreference::AnswerDirect => "answer_direct",
        ExecutionPreference::ToolFirst => "tool_first",
        ExecutionPreference::MemoryFirst => "memory_first",
    }
}

fn action_family_label(family: ActionFamily) -> &'static str {
    match family {
        ActionFamily::Conversation => "conversation",
        ActionFamily::ActiveAction => "active_action",
        ActionFamily::TaskExecution => "task_execution",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProgrammableReasoningExecutionBackend, ProgrammableReasoningStage};

    fn linux_runtime_contract() -> ProgrammableReasoningRuntimeContract {
        ProgrammableReasoningRuntimeContract {
            stage: ProgrammableReasoningStage::CapabilityAtomsExchange,
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

    fn hard_gate() -> TurnDeliberationGate {
        TurnDeliberationGate {
            class: crate::memory::TurnDeliberationClass::HardReasoning,
            compact_reply: false,
            prefer_explicit_blocker: true,
            rationale: vec!["hard_reasoning".to_string()],
        }
    }

    #[test]
    fn analysis_prefers_structured_tool_synthesis_for_hard_runtime_action() {
        let analysis = compile_counterfactual_analysis(CounterfactualAnalysisInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            runtime_contract: linux_runtime_contract(),
            request_semantics: RequestSemantics {
                request_kind: RequestKind::General,
                evidence_need: EvidenceNeed::HostTool,
                disclosure_surface: super::super::request_semantics::DisclosureSurface::Governed,
                execution_preference: ExecutionPreference::ToolFirst,
                action_family: ActionFamily::ActiveAction,
                confidence: 88,
            },
            deliberation_gate: &hard_gate(),
            reasoning_intent: Some(&ProgrammableReasoningIntent {
                kind: ProgrammableReasoningIntentKind::CapabilityAtomsExchange,
                strategy: ProgrammableReasoningStrategy::RequireNativeToolRound,
                confidence: 92,
                summary: "Compile runtime evidence before answering.".to_string(),
                rationale: vec!["hard_reasoning".to_string()],
                preferred_tools: vec!["lua_tool_bridge".to_string()],
                runtime_grounding_required: true,
            }),
            has_tools: true,
            active_task_context_present: true,
            governed_memory_evidence_present: false,
        });

        assert_eq!(
            analysis.selected_branch.kind,
            CounterfactualBranchKind::StructuredToolSynthesis
        );
        assert!(analysis.requires_native_tool_round());
        assert!(analysis
            .alternatives
            .iter()
            .any(|branch| branch.kind == CounterfactualBranchKind::DirectReply));
    }

    #[test]
    fn analysis_prefers_memory_query_for_memory_first_turn() {
        let analysis = compile_counterfactual_analysis(CounterfactualAnalysisInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            runtime_contract: linux_runtime_contract(),
            request_semantics: RequestSemantics {
                request_kind: RequestKind::MemoryRecall,
                evidence_need: EvidenceNeed::ArchiveMemory,
                disclosure_surface: super::super::request_semantics::DisclosureSurface::Governed,
                execution_preference: ExecutionPreference::MemoryFirst,
                action_family: ActionFamily::Conversation,
                confidence: 83,
            },
            deliberation_gate: &TurnDeliberationGate {
                class: crate::memory::TurnDeliberationClass::Standard,
                compact_reply: false,
                prefer_explicit_blocker: false,
                rationale: Vec::new(),
            },
            reasoning_intent: Some(&ProgrammableReasoningIntent {
                kind: ProgrammableReasoningIntentKind::MemoryQuery,
                strategy: ProgrammableReasoningStrategy::PromptOnly,
                confidence: 84,
                summary: "Compile governed memory evidence before answering.".to_string(),
                rationale: vec!["archive_memory".to_string()],
                preferred_tools: vec!["lua_memory_query".to_string()],
                runtime_grounding_required: false,
            }),
            has_tools: true,
            active_task_context_present: false,
            governed_memory_evidence_present: true,
        });

        assert_eq!(
            analysis.selected_branch.kind,
            CounterfactualBranchKind::MemoryQuery
        );
        assert!(!analysis.requires_native_tool_round());
    }
}
