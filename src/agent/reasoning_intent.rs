use super::deliberation::TurnDeliberationGate;
use super::request_semantics::{ActionFamily, EvidenceNeed, ExecutionPreference, RequestSemantics};
use super::strategy::AgentRunStrategy;
use crate::memory::TurnReasoningIntentLedger;
use crate::util::truncate_content_to_max;
use crate::ProgrammableReasoningRuntimeContract;

const INTENT_SUMMARY_MAX_CHARS: usize = 160;
const INTENT_SIGNAL_MAX_ITEMS: usize = 6;
const INTENT_SIGNAL_MAX_CHARS: usize = 32;
const INTENT_TOOL_MAX_ITEMS: usize = 4;
const INTENT_TOOL_MAX_CHARS: usize = 32;
const INTENT_RENDER_MAX_CHARS: usize = 360;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ProgrammableReasoningIntentKind {
    #[default]
    Disabled,
    TurnLocalReadonly,
    MemoryQuery,
    CapabilityBridge,
    EngineeringSynthesis,
}

impl ProgrammableReasoningIntentKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::TurnLocalReadonly => "turn_local_readonly",
            Self::MemoryQuery => "memory_query",
            Self::CapabilityBridge => "capability_bridge",
            Self::EngineeringSynthesis => "engineering_synthesis",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ProgrammableReasoningStrategy {
    #[default]
    Disabled,
    PromptOnly,
    PreferNativeToolRound,
    RequireNativeToolRound,
}

impl ProgrammableReasoningStrategy {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::PromptOnly => "prompt_only",
            Self::PreferNativeToolRound => "prefer_native_tool_round",
            Self::RequireNativeToolRound => "require_native_tool_round",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProgrammableReasoningIntent {
    pub(crate) kind: ProgrammableReasoningIntentKind,
    pub(crate) strategy: ProgrammableReasoningStrategy,
    pub(crate) confidence: u8,
    pub(crate) summary: String,
    pub(crate) rationale: Vec<String>,
    pub(crate) preferred_tools: Vec<String>,
    pub(crate) runtime_grounding_required: bool,
}

impl ProgrammableReasoningIntent {
    pub(crate) fn is_meaningful(&self) -> bool {
        self.kind != ProgrammableReasoningIntentKind::Disabled
            || !self.summary.trim().is_empty()
            || !self.rationale.is_empty()
            || !self.preferred_tools.is_empty()
            || self.runtime_grounding_required
    }

    pub(crate) fn requires_native_tool_round(&self) -> bool {
        self.strategy == ProgrammableReasoningStrategy::RequireNativeToolRound
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ProgrammableReasoningIntentInput<'a> {
    pub(crate) strategy: AgentRunStrategy,
    pub(crate) runtime_contract: ProgrammableReasoningRuntimeContract,
    pub(crate) request_semantics: RequestSemantics,
    pub(crate) deliberation_gate: &'a TurnDeliberationGate,
    pub(crate) has_tools: bool,
    pub(crate) active_task_context_present: bool,
    pub(crate) governed_memory_evidence_present: bool,
}

pub(crate) fn compile_programmable_reasoning_intent(
    input: ProgrammableReasoningIntentInput<'_>,
) -> ProgrammableReasoningIntent {
    if input.strategy != AgentRunStrategy::LinuxEnhanced
        || !input.runtime_contract.execution_enabled
    {
        return ProgrammableReasoningIntent::default();
    }

    let runtime_grounding_required = matches!(
        input.request_semantics.evidence_need,
        EvidenceNeed::PublicRuntime | EvidenceNeed::HostTool
    );
    let rationale = normalize_intent_list(build_rationale(&input), INTENT_SIGNAL_MAX_ITEMS);
    let (kind, strategy, summary, preferred_tools, confidence) = select_intent_strategy(&input);

    normalize_intent(
        kind,
        strategy,
        confidence,
        summary,
        rationale,
        preferred_tools,
        runtime_grounding_required,
    )
}

pub(crate) fn render_programmable_reasoning_intent_block(
    intent: &ProgrammableReasoningIntent,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 || !intent.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(INTENT_RENDER_MAX_CHARS));
    out.push_str("Kind: ");
    out.push_str(intent.kind.label());
    out.push('\n');
    out.push_str("Strategy: ");
    out.push_str(intent.strategy.label());
    out.push('\n');
    out.push_str("Confidence: ");
    out.push_str(intent.confidence.to_string().as_str());
    out.push('\n');
    if !intent.summary.trim().is_empty() {
        out.push_str("Summary: ");
        out.push_str(intent.summary.trim());
        out.push('\n');
    }
    out.push_str("Runtime grounding required: ");
    out.push_str(if intent.runtime_grounding_required {
        "true"
    } else {
        "false"
    });
    if !intent.rationale.is_empty() {
        out.push('\n');
        out.push_str("Signals: ");
        out.push_str(&intent.rationale.join(" | "));
    }
    if !intent.preferred_tools.is_empty() {
        out.push('\n');
        out.push_str("Preferred tools: ");
        out.push_str(&intent.preferred_tools.join(", "));
    }
    let rendered =
        truncate_content_to_max(out.trim_end(), max_len.min(INTENT_RENDER_MAX_CHARS)).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn build_turn_reasoning_intent_ledger(
    intent: &ProgrammableReasoningIntent,
) -> TurnReasoningIntentLedger {
    TurnReasoningIntentLedger {
        kind: intent.kind.label().to_string(),
        strategy: intent.strategy.label().to_string(),
        confidence: intent.confidence,
        summary: truncate_content_to_max(intent.summary.trim(), INTENT_SUMMARY_MAX_CHARS)
            .into_owned(),
        rationale: normalize_intent_list(intent.rationale.clone(), INTENT_SIGNAL_MAX_ITEMS),
        preferred_tools: normalize_tools(intent.preferred_tools.clone()),
        runtime_grounding_required: intent.runtime_grounding_required,
    }
}

fn select_intent_strategy(
    input: &ProgrammableReasoningIntentInput<'_>,
) -> (
    ProgrammableReasoningIntentKind,
    ProgrammableReasoningStrategy,
    &'static str,
    Vec<String>,
    u8,
) {
    if matches!(
        input.request_semantics.evidence_need,
        EvidenceNeed::ArchiveMemory | EvidenceNeed::CanonicalMemory
    ) || input.request_semantics.execution_preference == ExecutionPreference::MemoryFirst
    {
        return (
            ProgrammableReasoningIntentKind::MemoryQuery,
            ProgrammableReasoningStrategy::PromptOnly,
            "Compile governed memory evidence before answering.",
            vec!["lua_memory_query".to_string()],
            input.request_semantics.confidence.max(80),
        );
    }

    if input.has_tools
        && input.request_semantics.execution_preference == ExecutionPreference::ToolFirst
        && (input.deliberation_gate.class == crate::memory::TurnDeliberationClass::HardReasoning
            || input.active_task_context_present
            || matches!(
                input.request_semantics.action_family,
                ActionFamily::ActionRequest
                    | ActionFamily::ActiveAction
                    | ActionFamily::TaskExecution
            ))
    {
        return (
            ProgrammableReasoningIntentKind::EngineeringSynthesis,
            ProgrammableReasoningStrategy::RequireNativeToolRound,
            "Compile runtime and tool evidence into a structured action answer before replying.",
            vec!["lua_query".to_string()],
            input.request_semantics.confidence.max(90),
        );
    }

    if input.has_tools
        && input.request_semantics.execution_preference == ExecutionPreference::ToolFirst
    {
        return (
            ProgrammableReasoningIntentKind::CapabilityBridge,
            ProgrammableReasoningStrategy::PreferNativeToolRound,
            "Bridge the current turn intent with tool evidence before replying.",
            vec!["lua_query".to_string()],
            input.request_semantics.confidence.max(82),
        );
    }

    if input.deliberation_gate.class == crate::memory::TurnDeliberationClass::HardReasoning
        || input.active_task_context_present
        || input.governed_memory_evidence_present
    {
        return (
            ProgrammableReasoningIntentKind::TurnLocalReadonly,
            ProgrammableReasoningStrategy::PromptOnly,
            "Reconcile the active turn evidence before committing to the reply.",
            Vec::new(),
            input.request_semantics.confidence.max(76),
        );
    }

    (
        ProgrammableReasoningIntentKind::Disabled,
        ProgrammableReasoningStrategy::Disabled,
        "",
        Vec::new(),
        0,
    )
}

fn build_rationale(input: &ProgrammableReasoningIntentInput<'_>) -> Vec<String> {
    let mut rationale = Vec::with_capacity(6);
    if input.deliberation_gate.class == crate::memory::TurnDeliberationClass::HardReasoning {
        rationale.push("hard_reasoning".to_string());
    }
    if input.deliberation_gate.prefer_explicit_blocker {
        rationale.push("explicit_blocker".to_string());
    }
    match input.request_semantics.evidence_need {
        EvidenceNeed::PublicRuntime => rationale.push("public_runtime".to_string()),
        EvidenceNeed::HostTool => rationale.push("host_tool".to_string()),
        EvidenceNeed::ArchiveMemory => rationale.push("archive_memory".to_string()),
        EvidenceNeed::CanonicalMemory => rationale.push("canonical_memory".to_string()),
        EvidenceNeed::None => {}
    }
    match input.request_semantics.execution_preference {
        ExecutionPreference::ToolFirst => rationale.push("tool_first".to_string()),
        ExecutionPreference::MemoryFirst => rationale.push("memory_first".to_string()),
        ExecutionPreference::AnswerDirect => {}
    }
    if input.active_task_context_present {
        rationale.push("active_task_context".to_string());
    }
    if input.governed_memory_evidence_present {
        rationale.push("governed_memory_evidence".to_string());
    }
    rationale
}

fn normalize_intent(
    kind: ProgrammableReasoningIntentKind,
    strategy: ProgrammableReasoningStrategy,
    confidence: u8,
    summary: &str,
    rationale: Vec<String>,
    preferred_tools: Vec<String>,
    runtime_grounding_required: bool,
) -> ProgrammableReasoningIntent {
    ProgrammableReasoningIntent {
        kind,
        strategy,
        confidence,
        summary: truncate_content_to_max(summary.trim(), INTENT_SUMMARY_MAX_CHARS)
            .trim()
            .to_string(),
        rationale,
        preferred_tools: normalize_tools(preferred_tools),
        runtime_grounding_required,
    }
}

fn normalize_tools(items: Vec<String>) -> Vec<String> {
    normalize_intent_list(items, INTENT_TOOL_MAX_ITEMS)
        .into_iter()
        .map(|item| {
            truncate_content_to_max(item.trim(), INTENT_TOOL_MAX_CHARS)
                .trim()
                .to_string()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn normalize_intent_list(items: Vec<String>, max_items: usize) -> Vec<String> {
    let mut normalized = Vec::with_capacity(items.len().min(max_items));
    for item in items {
        let value = truncate_content_to_max(item.trim(), INTENT_SIGNAL_MAX_CHARS)
            .trim()
            .to_string();
        if value.is_empty() || normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= max_items {
            break;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics, ResumeRelation,
    };

    fn semantics(
        evidence_need: EvidenceNeed,
        execution_preference: ExecutionPreference,
        action_family: ActionFamily,
    ) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference,
            action_family,
            resume_relation: ResumeRelation::IndependentTurn,
            confidence: 91,
        }
    }

    fn linux_runtime_contract() -> crate::ProgrammableReasoningRuntimeContract {
        crate::ProgrammableReasoningRuntimeContract {
            stage: crate::ProgrammableReasoningStage::IntentCompiler,
            linux_only: true,
            execution_backend: crate::ProgrammableReasoningExecutionBackend::LuaSandbox,
            execution_enabled: true,
            proposal_only_persistence: true,
            operator_visible_contract: true,
            user_authored_scripts: false,
            direct_host_tool_execution: false,
            second_execution_plane_forbidden: true,
        }
    }

    #[test]
    fn compiler_escalates_hard_runtime_action_into_engineering_synthesis() {
        let intent = compile_programmable_reasoning_intent(ProgrammableReasoningIntentInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            runtime_contract: linux_runtime_contract(),
            request_semantics: semantics(
                EvidenceNeed::HostTool,
                ExecutionPreference::ToolFirst,
                ActionFamily::ActionRequest,
            ),
            deliberation_gate: &TurnDeliberationGate {
                class: crate::memory::TurnDeliberationClass::HardReasoning,
                compact_reply: false,
                prefer_explicit_blocker: true,
                rationale: vec!["request_complexity".to_string()],
            },
            has_tools: true,
            active_task_context_present: true,
            governed_memory_evidence_present: true,
        });

        assert_eq!(
            intent.kind,
            ProgrammableReasoningIntentKind::EngineeringSynthesis
        );
        assert_eq!(
            intent.strategy,
            ProgrammableReasoningStrategy::RequireNativeToolRound
        );
        assert!(intent.runtime_grounding_required);
        assert!(intent
            .preferred_tools
            .iter()
            .any(|tool| tool == "lua_query"));
    }

    #[test]
    fn compiler_prefers_memory_query_for_memory_first_turn() {
        let intent = compile_programmable_reasoning_intent(ProgrammableReasoningIntentInput {
            strategy: AgentRunStrategy::LinuxEnhanced,
            runtime_contract: linux_runtime_contract(),
            request_semantics: semantics(
                EvidenceNeed::ArchiveMemory,
                ExecutionPreference::MemoryFirst,
                ActionFamily::Conversation,
            ),
            deliberation_gate: &TurnDeliberationGate {
                class: crate::memory::TurnDeliberationClass::Standard,
                compact_reply: false,
                prefer_explicit_blocker: false,
                rationale: vec!["archive_grounding".to_string()],
            },
            has_tools: true,
            active_task_context_present: false,
            governed_memory_evidence_present: true,
        });

        assert_eq!(intent.kind, ProgrammableReasoningIntentKind::MemoryQuery);
        assert_eq!(intent.strategy, ProgrammableReasoningStrategy::PromptOnly);
        assert!(!intent.runtime_grounding_required);
    }
}
