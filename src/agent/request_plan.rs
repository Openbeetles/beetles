//! Per-request tool exposure and invocation plan.
//! Centralizes runtime tool visibility plus typed tool-demand mapping so the
//! agent loop stays thin and request understanding stays outside prompt hacks.

use super::adversarial_arena::render_adversarial_arena_guidance_block;
use super::counterfactual::{render_counterfactual_guidance_block, CounterfactualAnalysis};
use super::reasoning_intent::{
    render_programmable_reasoning_intent_block, ProgrammableReasoningIntent,
};
use super::reply_surface::ReplySurface;
use super::request_semantics::RequestSemantics;
use crate::bus::PcMsg;
use crate::llm::tool_fallback::{append_tool_fallback_instructions, recover_text_tool_calls};
use crate::llm::{LlmClient, LlmResponse, ToolCallSupport, ToolChoicePolicy, ToolSpec};
use crate::tools::{ToolPolicyContext, ToolRegistry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolCallMode {
    Disabled,
    Native,
    PromptGuided,
}

#[derive(Clone)]
pub(crate) struct AgentRequestPlan<'a> {
    tool_policy: ToolPolicyContext<'a>,
    tool_specs: Vec<ToolSpec>,
    tool_call_mode: ToolCallMode,
    reply_surface: ReplySurface,
    semantics: RequestSemantics,
    strategy: super::strategy::AgentRunStrategy,
    programmable_reasoning_intent: Option<ProgrammableReasoningIntent>,
    counterfactual_analysis: Option<CounterfactualAnalysis>,
    adversarial_arena_adjudication: Option<crate::reasoning::AdversarialArenaAdjudication>,
}

impl<'a> AgentRequestPlan<'a> {
    #[cfg(test)]
    pub(crate) fn build(
        msg: &'a PcMsg,
        registry: &ToolRegistry,
        worker_llm: &(dyn LlmClient + Send + Sync),
        strategy: super::strategy::AgentRunStrategy,
        semantics: RequestSemantics,
    ) -> Self {
        Self::build_for_prepared_turn(
            msg,
            registry,
            worker_llm,
            strategy,
            semantics,
            ReplySurface::for_prepared_turn(msg.ingress, semantics, false),
        )
    }

    pub(crate) fn build_for_prepared_turn(
        msg: &'a PcMsg,
        registry: &ToolRegistry,
        worker_llm: &(dyn LlmClient + Send + Sync),
        strategy: super::strategy::AgentRunStrategy,
        semantics: RequestSemantics,
        reply_surface: ReplySurface,
    ) -> Self {
        let tool_policy = ToolPolicyContext::new(msg.ingress, msg.channel.as_ref());
        let tool_specs = registry.tool_specs_for_llm(&tool_policy);
        let tool_call_mode = if tool_specs.is_empty() {
            ToolCallMode::Disabled
        } else {
            match worker_llm.model_compat().tool_call_support {
                ToolCallSupport::Native => ToolCallMode::Native,
                ToolCallSupport::PromptGuided => ToolCallMode::PromptGuided,
            }
        };
        Self {
            tool_policy,
            tool_specs,
            tool_call_mode,
            reply_surface,
            semantics,
            strategy,
            programmable_reasoning_intent: None,
            counterfactual_analysis: None,
            adversarial_arena_adjudication: None,
        }
    }

    pub(crate) fn with_programmable_reasoning_intent(
        mut self,
        intent: Option<&ProgrammableReasoningIntent>,
    ) -> Self {
        self.programmable_reasoning_intent = intent.cloned();
        self
    }

    pub(crate) fn with_counterfactual_analysis(
        mut self,
        analysis: Option<&CounterfactualAnalysis>,
    ) -> Self {
        self.counterfactual_analysis = analysis.cloned();
        self
    }

    pub(crate) fn with_adversarial_arena_adjudication(
        mut self,
        adjudication: Option<&crate::reasoning::AdversarialArenaAdjudication>,
    ) -> Self {
        self.adversarial_arena_adjudication = adjudication.cloned();
        self
    }

    pub(crate) fn policy(&self) -> &ToolPolicyContext<'a> {
        &self.tool_policy
    }

    pub(crate) fn has_tools(&self) -> bool {
        !self.tool_specs.is_empty()
    }

    pub(crate) fn reply_surface(&self) -> ReplySurface {
        self.reply_surface
    }

    pub(crate) fn uses_native_tools(&self) -> bool {
        matches!(self.tool_call_mode, ToolCallMode::Native)
    }

    pub(crate) fn request_tools(&self) -> Option<&[ToolSpec]> {
        self.uses_native_tools()
            .then_some(self.tool_specs.as_slice())
    }

    pub(crate) fn tool_choice(&self, round: usize, any_tool_used: bool) -> ToolChoicePolicy {
        if !self.uses_native_tools() || any_tool_used {
            return ToolChoicePolicy::Auto;
        }
        if self.requires_native_tool_first_round(round) {
            return ToolChoicePolicy::Require;
        }
        ToolChoicePolicy::Auto
    }

    fn requires_native_tool_first_round(&self, round: usize) -> bool {
        if round == 0 {
            if let Some(adjudication) = self.adversarial_arena_adjudication.as_ref() {
                return adjudication.winner_requires_native_tool_round();
            }
        }
        if round == 0
            && self
                .counterfactual_analysis
                .as_ref()
                .is_some_and(CounterfactualAnalysis::requires_native_tool_round)
        {
            return true;
        }
        if round == 0
            && self
                .programmable_reasoning_intent
                .as_ref()
                .is_some_and(ProgrammableReasoningIntent::requires_native_tool_round)
        {
            return true;
        }
        if round > 0
            || self.strategy != super::strategy::AgentRunStrategy::LinuxEnhanced
            || self.semantics.execution_preference
                != super::request_semantics::ExecutionPreference::ToolFirst
            || self.semantics.confidence < 75
        {
            return false;
        }
        matches!(
            self.semantics.evidence_need,
            super::request_semantics::EvidenceNeed::PublicRuntime
                | super::request_semantics::EvidenceNeed::HostTool
        )
    }

    pub(crate) fn apply_system_prompt(&self, system: &mut String, max_len: usize) {
        if let Some(adversarial_arena) = self
            .adversarial_arena_adjudication
            .as_ref()
            .and_then(|adjudication| render_adversarial_arena_guidance_block(adjudication, max_len))
        {
            let _ = crate::agent::context::append_capped_section(
                system,
                "\n\n",
                &adversarial_arena,
                max_len,
            );
        }
        if let Some(counterfactual) = self
            .counterfactual_analysis
            .as_ref()
            .and_then(|analysis| render_counterfactual_guidance_block(analysis, max_len))
        {
            let _ = crate::agent::context::append_capped_section(
                system,
                "\n\n",
                &counterfactual,
                max_len,
            );
        }
        if let Some(reasoning_intent) = self
            .programmable_reasoning_intent
            .as_ref()
            .and_then(|intent| render_programmable_reasoning_intent_block(intent, max_len))
        {
            let _ = crate::agent::context::append_capped_section(
                system,
                "\n\n",
                &reasoning_intent,
                max_len,
            );
        }
        if matches!(self.tool_call_mode, ToolCallMode::PromptGuided) {
            append_tool_fallback_instructions(system, max_len, &self.tool_specs);
        }
    }

    pub(crate) fn recover_response(&self, response: LlmResponse) -> LlmResponse {
        if self.has_tools() {
            recover_text_tool_calls(response)
        } else {
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::counterfactual::{
        CounterfactualAnalysis, CounterfactualBranchKind, CounterfactualBranchProjection,
        CounterfactualTurnSnapshot,
    };
    use crate::agent::reasoning_intent::{
        ProgrammableReasoningIntent, ProgrammableReasoningIntentKind, ProgrammableReasoningStrategy,
    };
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
        RequestSemantics,
    };
    use crate::agent::AgentRunStrategy;
    use crate::llm::{LlmHttpClient, LlmModelCompat, Message, StopReason, ToolChoicePolicy};
    use crate::reasoning::{
        AdversarialArenaAdjudication, AdversarialArenaClaim, AdversarialArenaDisposition,
        AdversarialArenaRole, AdversarialArenaSubjectKind,
    };
    use crate::tools::{Tool, ToolMetadata};
    use crate::Result;

    struct VisibleTool;
    struct NamedTool {
        name: &'static str,
        description: &'static str,
        metadata: ToolMetadata,
    }
    struct NativeLlm;
    struct PromptGuidedLlm;

    fn semantics(
        evidence_need: EvidenceNeed,
        execution_preference: ExecutionPreference,
    ) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference,
            action_family: ActionFamily::Conversation,
            confidence: 90,
        }
    }

    impl Tool for VisibleTool {
        fn name(&self) -> &'static str {
            "visible"
        }

        fn description(&self) -> &str {
            "visible tool"
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }
    }

    impl Tool for NamedTool {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn schema(&self) -> &str {
            r#"{"type":"object"}"#
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
        }

        fn metadata(&self) -> ToolMetadata {
            self.metadata
        }
    }

    impl crate::llm::LlmClient for NativeLlm {
        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: String::new(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }

    impl crate::llm::LlmClient for PromptGuidedLlm {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::prompt_guided()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            Ok(LlmResponse {
                content: String::new(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }

    #[test]
    fn request_plan_prefers_native_tools_when_supported() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "hi", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics::conservative_default(),
        );
        assert!(plan.has_tools());
        assert!(plan.uses_native_tools());
        assert_eq!(plan.request_tools().map(|specs| specs.len()), Some(1));
    }

    #[test]
    fn request_plan_falls_back_to_prompt_guided_mode() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "hi", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &PromptGuidedLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics::conservative_default(),
        );
        assert!(plan.has_tools());
        assert!(!plan.uses_native_tools());
        assert!(plan.request_tools().is_none());
    }

    #[test]
    fn operational_requests_require_first_round_tool_for_linux_native_mode() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "process",
            description: "process inspection",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::HostTool, ExecutionPreference::ToolFirst),
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
        assert_eq!(plan.tool_choice(1, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn public_operational_observability_requests_require_first_round_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "board_info",
            description: "whole host status",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看系统状态", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::PublicRuntime, ExecutionPreference::ToolFirst),
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn memory_evidence_requests_do_not_force_first_round_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "memory_search",
            description: "search archive evidence",
            metadata: ToolMetadata::task(),
        }));
        registry.register(Box::new(NamedTool {
            name: "memory_get",
            description: "read archive evidence",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "检查我们历史里我对北岛的偏好", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(
                EvidenceNeed::ArchiveMemory,
                ExecutionPreference::MemoryFirst,
            ),
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn low_confidence_tool_first_semantics_do_not_force_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看系统状态", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics {
                confidence: 40,
                ..semantics(EvidenceNeed::PublicRuntime, ExecutionPreference::ToolFirst)
            },
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn embedded_mode_keeps_tool_choice_flexible() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "process",
            description: "process inspection",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::Embedded,
            semantics(EvidenceNeed::HostTool, ExecutionPreference::ToolFirst),
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn conversational_questions_do_not_force_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "1+1 等于多少", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics::conservative_default(),
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn active_action_resume_requires_native_tool_on_first_round() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "继续配置", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics {
                request_kind: RequestKind::General,
                evidence_need: EvidenceNeed::HostTool,
                disclosure_surface: DisclosureSurface::Governed,
                execution_preference: ExecutionPreference::ToolFirst,
                action_family: ActionFamily::ActiveAction,
                confidence: 100,
            },
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
        assert_eq!(plan.tool_choice(1, false), ToolChoicePolicy::Auto);
        assert_eq!(plan.tool_choice(0, true), ToolChoicePolicy::Auto);
    }

    #[test]
    fn task_execution_resume_requires_native_tool_on_first_round() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "继续配置", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics {
                request_kind: RequestKind::General,
                evidence_need: EvidenceNeed::HostTool,
                disclosure_surface: DisclosureSurface::Governed,
                execution_preference: ExecutionPreference::ToolFirst,
                action_family: ActionFamily::TaskExecution,
                confidence: 100,
            },
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn active_action_supply_input_requires_native_tool_on_first_round() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "授权码是 hqvqcibpdvqgbdba", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics {
                request_kind: RequestKind::General,
                evidence_need: EvidenceNeed::HostTool,
                disclosure_surface: DisclosureSurface::Governed,
                execution_preference: ExecutionPreference::ToolFirst,
                action_family: ActionFamily::ActiveAction,
                confidence: 92,
            },
        );
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn request_plan_does_not_add_linux_inspection_guidance() {
        let mut registry = ToolRegistry::new();
        for (name, description) in [
            ("board_info", "whole host status"),
            ("process", "process inspection"),
            ("network", "network inspection"),
            ("network_scan", "wifi diagnostics"),
        ] {
            registry.register(Box::new(NamedTool {
                name,
                description,
                metadata: ToolMetadata::task(),
            }));
        }
        let msg = PcMsg::new_inbound("telegram", "chat", "查看系统状态并排查网络问题", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::PublicRuntime, ExecutionPreference::ToolFirst),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn request_plan_does_not_add_request_specific_guidance_for_process_or_network() {
        let mut registry = ToolRegistry::new();
        for (name, description) in [
            ("process", "process inspection"),
            ("network", "network inspection"),
        ] {
            registry.register(Box::new(NamedTool {
                name,
                description,
                metadata: ToolMetadata::task(),
            }));
        }
        let msg =
            PcMsg::new_inbound("telegram", "chat", "检查当前服务和网络状态", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::HostTool, ExecutionPreference::ToolFirst),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn request_plan_keeps_general_conversation_without_guidance() {
        let mut registry = ToolRegistry::new();
        for (name, description) in [
            ("board_info", "whole host status"),
            ("process", "process inspection"),
            ("network", "network inspection"),
            ("network_scan", "wifi diagnostics"),
        ] {
            registry.register(Box::new(NamedTool {
                name,
                description,
                metadata: ToolMetadata::task(),
            }));
        }
        let msg = PcMsg::new_inbound("telegram", "chat", "今天过得怎么样", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics::conservative_default(),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn request_plan_does_not_add_retrieval_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "memory_search",
            description: "search archive evidence",
            metadata: ToolMetadata::task(),
        }));
        registry.register(Box::new(NamedTool {
            name: "memory_get",
            description: "inspect cited archive records",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "检查我们历史里我对北岛的偏好", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(
                EvidenceNeed::ArchiveMemory,
                ExecutionPreference::MemoryFirst,
            ),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn request_plan_does_not_add_private_garden_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "private_garden",
            description: "free private workspace",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "我们继续聊", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            RequestSemantics::conservative_default(),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn request_plan_does_not_add_archive_memory_guidance() {
        let mut registry = ToolRegistry::new();
        for (name, description) in [
            ("memory_search", "search archive evidence"),
            ("memory_get", "inspect cited archive records"),
        ] {
            registry.register(Box::new(NamedTool {
                name,
                description,
                metadata: ToolMetadata::task(),
            }));
        }
        let msg = PcMsg::new_inbound("telegram", "chat", "检查我们历史里我对北岛的偏好", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(
                EvidenceNeed::ArchiveMemory,
                ExecutionPreference::MemoryFirst,
            ),
        );
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.is_empty());
    }

    #[test]
    fn programmable_reasoning_intent_can_force_native_tool_round() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "resolve current runtime issue", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::None, ExecutionPreference::AnswerDirect),
        )
        .with_programmable_reasoning_intent(Some(&ProgrammableReasoningIntent {
            kind: ProgrammableReasoningIntentKind::EngineeringSynthesis,
            strategy: ProgrammableReasoningStrategy::RequireNativeToolRound,
            confidence: 91,
            summary: "Compile runtime evidence before answering".to_string(),
            rationale: vec!["hard_reasoning".to_string(), "host_tool".to_string()],
            preferred_tools: vec!["visible".to_string()],
            runtime_grounding_required: true,
        }));

        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn counterfactual_analysis_can_force_native_tool_round_and_append_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "继续排查并修这个运行时故障", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::None, ExecutionPreference::AnswerDirect),
        )
        .with_counterfactual_analysis(Some(&CounterfactualAnalysis {
            snapshot: CounterfactualTurnSnapshot::default(),
            selected_branch: CounterfactualBranchProjection {
                kind: CounterfactualBranchKind::StructuredToolSynthesis,
                score: 94,
                summary: "Collect live evidence, then synthesize one coherent action answer."
                    .to_string(),
                rationale: vec![
                    "hard_reasoning".to_string(),
                    "runtime_grounding".to_string(),
                ],
                requires_native_tool_round: true,
            },
            alternatives: vec![CounterfactualBranchProjection {
                kind: CounterfactualBranchKind::DirectReply,
                score: 36,
                summary: "Answer immediately from the current context.".to_string(),
                rationale: vec!["under_grounded".to_string()],
                requires_native_tool_round: false,
            }],
            summary: "Prefer structured tool synthesis over direct reply.".to_string(),
        }));

        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);

        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
        assert!(system.contains("## Counterfactual Sandbox"));
        assert!(system.contains("Selected branch: structured_tool_synthesis"));
        assert!(system.contains("Rejected: direct_reply"));
    }

    #[test]
    fn adversarial_arena_can_override_counterfactual_tool_bias_and_append_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound(
            "telegram",
            "chat",
            "继续配置邮箱，但如果信息不够先别乱配",
            false,
        )
        .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::HostTool, ExecutionPreference::ToolFirst),
        )
        .with_counterfactual_analysis(Some(&CounterfactualAnalysis {
            snapshot: CounterfactualTurnSnapshot::default(),
            selected_branch: CounterfactualBranchProjection {
                kind: CounterfactualBranchKind::StructuredToolSynthesis,
                score: 94,
                summary: "Collect live evidence, then synthesize one coherent action answer."
                    .to_string(),
                rationale: vec!["hard_reasoning".to_string()],
                requires_native_tool_round: true,
            },
            alternatives: vec![CounterfactualBranchProjection {
                kind: CounterfactualBranchKind::ClarifyBeforeAction,
                score: 89,
                summary: "Ask for the missing approval or parameter before acting.".to_string(),
                rationale: vec!["explicit_blocker".to_string()],
                requires_native_tool_round: false,
            }],
            summary: "Counterfactual still leans toward live synthesis.".to_string(),
        }))
        .with_adversarial_arena_adjudication(Some(&AdversarialArenaAdjudication {
            subject_kind: AdversarialArenaSubjectKind::TurnStrategy,
            disposition: AdversarialArenaDisposition::HoldForClarification,
            summary: "Attacker blocked the live tool path because the missing blocker is more material than fresh evidence."
                .to_string(),
            defender: AdversarialArenaClaim {
                role: AdversarialArenaRole::Defender,
                label: "structured_tool_synthesis".to_string(),
                summary: "Collect live evidence, then synthesize.".to_string(),
                evidence_score: 84,
                signals: vec!["host_tool".to_string()],
                requires_native_tool_round: true,
            },
            attacker: AdversarialArenaClaim {
                role: AdversarialArenaRole::Attacker,
                label: "clarify_before_action".to_string(),
                summary: "Ask for the missing blocker before acting.".to_string(),
                evidence_score: 88,
                signals: vec!["explicit_blocker".to_string()],
                requires_native_tool_round: false,
            },
            winner: AdversarialArenaClaim {
                role: AdversarialArenaRole::Attacker,
                label: "clarify_before_action".to_string(),
                summary: "Ask for the missing blocker before acting.".to_string(),
                evidence_score: 88,
                signals: vec!["explicit_blocker".to_string()],
                requires_native_tool_round: false,
            },
        }));

        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);

        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
        assert!(system.contains("## Adversarial Arena"));
        assert!(system.contains("Adjudication: hold_for_clarification"));
        assert!(system.contains("Winner: clarify_before_action"));
    }
}
