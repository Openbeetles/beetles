//! Per-request tool exposure and invocation plan.
//! Centralizes runtime tool visibility plus typed tool-demand mapping so the
//! agent loop stays thin and request understanding stays outside prompt hacks.

use super::request_semantics::{EvidenceNeed, ExecutionPreference, RequestSemantics};
use super::strategy::AgentRunStrategy;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolUseDemand {
    Flexible,
    Preferred,
    RequiredFirstTurn,
}

pub(crate) struct AgentRequestPlan<'a> {
    tool_policy: ToolPolicyContext<'a>,
    tool_specs: Vec<ToolSpec>,
    tool_call_mode: ToolCallMode,
    tool_use_demand: ToolUseDemand,
}

impl<'a> AgentRequestPlan<'a> {
    pub(crate) fn build(
        msg: &'a PcMsg,
        registry: &ToolRegistry,
        worker_llm: &(dyn LlmClient + Send + Sync),
        strategy: AgentRunStrategy,
        semantics: RequestSemantics,
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
        let tool_use_demand = classify_tool_use_demand(strategy, semantics, &tool_specs);
        Self {
            tool_policy,
            tool_specs,
            tool_call_mode,
            tool_use_demand,
        }
    }

    pub(crate) fn policy(&self) -> &ToolPolicyContext<'a> {
        &self.tool_policy
    }

    pub(crate) fn has_tools(&self) -> bool {
        !self.tool_specs.is_empty()
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
        match self.tool_use_demand {
            ToolUseDemand::RequiredFirstTurn if round <= 1 => ToolChoicePolicy::Require,
            _ => ToolChoicePolicy::Auto,
        }
    }

    pub(crate) fn apply_system_prompt(&self, system: &mut String, max_len: usize) {
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

fn classify_tool_use_demand(
    strategy: AgentRunStrategy,
    semantics: RequestSemantics,
    tool_specs: &[ToolSpec],
) -> ToolUseDemand {
    if strategy != AgentRunStrategy::LinuxEnhanced || tool_specs.is_empty() {
        return ToolUseDemand::Flexible;
    }
    if !semantics.supported_by_tools(tool_specs) {
        return ToolUseDemand::Flexible;
    }
    match semantics.execution_preference {
        ExecutionPreference::ToolFirst | ExecutionPreference::MemoryFirst => {
            ToolUseDemand::RequiredFirstTurn
        }
        ExecutionPreference::AnswerDirect => match semantics.evidence_need {
            EvidenceNeed::None => ToolUseDemand::Flexible,
            EvidenceNeed::PublicRuntime
            | EvidenceNeed::HostTool
            | EvidenceNeed::ArchiveMemory
            | EvidenceNeed::CanonicalMemory => ToolUseDemand::Preferred,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::request_semantics::{DisclosureSurface, RequestKind, RequestSemantics};
    use crate::llm::{LlmHttpClient, LlmModelCompat, Message, StopReason, ToolChoicePolicy};
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
        assert_eq!(plan.tool_choice(1, false), ToolChoicePolicy::Require);
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
    fn memory_evidence_requests_require_first_round_tool() {
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
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn unsupported_semantics_do_not_force_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看系统状态", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &NativeLlm,
            AgentRunStrategy::LinuxEnhanced,
            semantics(EvidenceNeed::PublicRuntime, ExecutionPreference::ToolFirst),
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
}
