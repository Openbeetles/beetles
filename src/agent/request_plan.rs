//! Per-request tool exposure and invocation plan.
//! Centralizes runtime tool visibility, native/prompt-guided mode selection,
//! and request/response assembly helpers so agent loop stays thin.

use crate::bus::PcMsg;
use crate::llm::tool_fallback::{append_tool_fallback_instructions, recover_text_tool_calls};
use crate::llm::{LlmClient, LlmResponse, ToolCallSupport, ToolSpec};
use crate::tools::{ToolPolicyContext, ToolRegistry};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolCallMode {
    Disabled,
    Native,
    PromptGuided,
}

pub(crate) struct AgentRequestPlan<'a> {
    tool_policy: ToolPolicyContext<'a>,
    tool_specs: Vec<ToolSpec>,
    tool_call_mode: ToolCallMode,
}

impl<'a> AgentRequestPlan<'a> {
    pub(crate) fn build(
        msg: &'a PcMsg,
        registry: &ToolRegistry,
        worker_llm: &(dyn LlmClient + Send + Sync),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmHttpClient, LlmModelCompat, Message, StopReason, ToolChoicePolicy};
    use crate::tools::Tool;
    use crate::Result;
    use serde_json::json;

    struct VisibleTool;
    struct NativeLlm;
    struct PromptGuidedLlm;

    impl Tool for VisibleTool {
        fn name(&self) -> &'static str {
            "visible"
        }

        fn description(&self) -> &str {
            "visible tool"
        }

        fn schema(&self) -> serde_json::Value {
            json!({"type":"object"})
        }

        fn execute(&self, _args: &str, _ctx: &mut dyn crate::tools::ToolContext) -> Result<String> {
            Ok(String::new())
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
        let plan = AgentRequestPlan::build(&msg, &registry, &NativeLlm);
        assert!(plan.has_tools());
        assert!(plan.uses_native_tools());
        assert_eq!(plan.request_tools().map(|specs| specs.len()), Some(1));
    }

    #[test]
    fn request_plan_falls_back_to_prompt_guided_mode() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "hi", false).expect("pcmsg");
        let plan = AgentRequestPlan::build(&msg, &registry, &PromptGuidedLlm);
        assert!(plan.has_tools());
        assert!(!plan.uses_native_tools());
        assert!(plan.request_tools().is_none());
    }
}
