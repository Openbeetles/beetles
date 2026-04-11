//! Per-request tool exposure and invocation plan.
//! Centralizes runtime tool visibility, native/prompt-guided mode selection,
//! and request/response assembly helpers so agent loop stays thin.

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
        let tool_use_demand = if tool_specs.is_empty() {
            ToolUseDemand::Flexible
        } else {
            classify_tool_use_demand(msg, strategy)
        };
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
        let guidance = match self.tool_use_demand {
            ToolUseDemand::Flexible => None,
            ToolUseDemand::Preferred => Some(
                "\n\n## Tool Guidance\nFor this request, prefer gathering concrete data or taking the needed action with tools before giving the final answer. Avoid guessing when a tool can materially improve correctness.",
            ),
            ToolUseDemand::RequiredFirstTurn => Some(
                "\n\n## Tool Guidance\nThis request likely requires checking current state or performing an action. On the first pass, use the provided tool invocation mechanism before giving a final answer unless the available tools clearly cannot satisfy the request.",
            ),
        };
        if let Some(guidance) = guidance {
            let remain = max_len.saturating_sub(system.len());
            if guidance.len() <= remain {
                system.push_str(guidance);
            }
        }
        if let Some(guidance) = self.iterative_retrieval_guidance() {
            let remain = max_len.saturating_sub(system.len());
            if guidance.len() <= remain {
                system.push_str(&guidance);
            }
        }
        if let Some(guidance) = self.linux_inspection_guidance() {
            let remain = max_len.saturating_sub(system.len());
            if guidance.len() <= remain {
                system.push_str(&guidance);
            }
        }
        if let Some(guidance) = self.internal_memory_governance_guidance() {
            let remain = max_len.saturating_sub(system.len());
            if guidance.len() <= remain {
                system.push_str(guidance);
            }
        }
    }

    pub(crate) fn recover_response(&self, response: LlmResponse) -> LlmResponse {
        if self.has_tools() {
            recover_text_tool_calls(response)
        } else {
            response
        }
    }

    pub(crate) fn missing_tool_followup(
        &self,
        round: usize,
        any_tool_used: bool,
        content: &str,
    ) -> Option<&'static str> {
        if any_tool_used || self.tool_call_mode == ToolCallMode::Disabled {
            return None;
        }
        if looks_like_explicit_limitation(content) {
            return None;
        }
        match self.tool_use_demand {
            ToolUseDemand::Flexible => None,
            ToolUseDemand::Preferred if round == 0 && content.chars().count() < 96 => Some(
                "[SYSTEM] This request would be stronger with concrete data or an actual action. If an available tool can materially improve the answer, use it now instead of replying from guesswork.",
            ),
            ToolUseDemand::RequiredFirstTurn if round == 0 => Some(
                "[SYSTEM] This request requires checking current state or performing an action. Do not answer from memory or guesswork. Use the available tool invocation mechanism now, then answer from the result. If no available tool can satisfy the request, explain that limitation explicitly.",
            ),
            ToolUseDemand::RequiredFirstTurn if round == 1 && content.chars().count() < 240 => {
                Some(
                    "[SYSTEM] You still have not used a tool for a request that needs one. Either call an available tool now or clearly explain why the available tools cannot complete the task.",
                )
            }
            _ => None,
        }
    }
}

impl AgentRequestPlan<'_> {
    fn iterative_retrieval_guidance(&self) -> Option<String> {
        if self.tool_use_demand == ToolUseDemand::Flexible {
            return None;
        }
        let mut guidance = String::from(
            "\n\n## Retrieval Discipline\nUse tools iteratively: search or list first only to locate concrete targets, then read one specific source, then answer. After you already have readable content, synthesize from it or extract one narrower section instead of repeating the same search or read unchanged. Treat web or URL-derived content as turn-local evidence, not durable user memory.",
        );
        let has_memory_search = self
            .tool_specs
            .iter()
            .any(|tool| tool.name == "memory_search");
        let has_memory_get = self.tool_specs.iter().any(|tool| tool.name == "memory_get");
        let has_factual_memory = self
            .tool_specs
            .iter()
            .any(|tool| tool.name == "factual_memory");
        if has_memory_search && has_memory_get {
            guidance.push_str(" For retained conversation history, daily notes, or turn logs, use memory_search to locate archive evidence and memory_get to inspect one cited record. Archive hits are evidence sources only, but they can support a shareable, user-facing conclusion after you distill and verify a stable fact. If the exact detail is still unsupported, say that plainly instead of refusing or inventing it.");
        }
        if has_factual_memory {
            guidance.push_str(" For exact canonical shared facts, slot-shaped profile values, durable constraints, or stable task/project records, prefer factual_memory over archive search. factual_memory returns canonical records plus evidence posture; use archive evidence only when you need supporting records or reconciliation.");
        }
        Some(guidance)
    }

    fn linux_inspection_guidance(&self) -> Option<String> {
        if self.tool_use_demand == ToolUseDemand::Flexible {
            return None;
        }
        let has_board_info = self.tool_specs.iter().any(|tool| tool.name == "board_info");
        let has_process = self.tool_specs.iter().any(|tool| tool.name == "process");
        let has_network = self.tool_specs.iter().any(|tool| tool.name == "network");
        let has_network_scan = self
            .tool_specs
            .iter()
            .any(|tool| tool.name == "network_scan");
        if !has_process && !has_network {
            return None;
        }
        if !has_board_info && !has_network_scan {
            let mut guidance = String::from(
                "\n\n## Linux Inspection Guidance\nWhen diagnosing a Linux host, prefer the most specific available tool instead of overloading a general snapshot.",
            );
            if has_process {
                guidance.push_str(" Use process for one specific process or service.");
            }
            if has_network {
                guidance.push_str(
                    " Use network for interfaces, DNS, routes, resolve, ping, and HTTP reachability.",
                );
            }
            return Some(guidance);
        }
        let mut guidance = String::from(
            "\n\n## Linux Inspection Guidance\nWhen diagnosing a Linux host, prefer the most specific available tool instead of overloading the general snapshot.",
        );
        if has_board_info {
            guidance.push_str(" Use board_info for whole-host status and resource pressure.");
        }
        if has_process {
            guidance.push_str(" Use process for one specific process or service.");
        }
        if has_network {
            guidance.push_str(
                " Use network for interfaces, DNS, routes, resolve, ping, and HTTP reachability.",
            );
        }
        if has_network_scan {
            guidance.push_str(" Use network_scan only for WiFi/AP scan or WiFi station checks.");
        }
        Some(guidance)
    }

    fn internal_memory_governance_guidance(&self) -> Option<&'static str> {
        self.tool_specs
            .iter()
            .any(|tool| tool.name == "private_garden")
            .then_some(
                "\n\n## Internal Memory Governance\nYour internal memory has layers with different roles. Keep kernel-facing private memory compact, stable, and repeatedly useful. Use `private_garden` for exploratory drafts, temporary organization, and self-owned working material. Before writing new private content, prefer reading, listing, or inspecting the current garden shape so you can update, merge, move, or prune in place instead of appending a history trail. When self-state reports Cautious or Tight pressure, consolidate or prune before creating more. If a garden insight becomes stable and load-bearing, distill it into the governed kernel later rather than duplicating the same material across both layers.",
            )
    }
}

fn classify_tool_use_demand(msg: &PcMsg, strategy: AgentRunStrategy) -> ToolUseDemand {
    if strategy != AgentRunStrategy::LinuxEnhanced || msg.ingress != crate::bus::IngressKind::User {
        return ToolUseDemand::Flexible;
    }
    if msg.is_group {
        return ToolUseDemand::Flexible;
    }
    let content = msg.content.trim();
    if content.is_empty() {
        return ToolUseDemand::Flexible;
    }

    let lower = content.to_ascii_lowercase();
    let char_count = content.chars().count();
    let separators = ['\n', ',', '，', '.', '。', '?', '？', ';', '；', ':', '：'];
    let separator_count = content.chars().filter(|ch| separators.contains(ch)).count();

    let has_path_like = content.contains('/')
        || content.contains('\\')
        || content.contains("://")
        || content.contains("~/")
        || content.contains('`');
    let file_markers = [
        ".rs", ".md", ".json", ".toml", ".yaml", ".yml", ".log", ".txt", ".py", ".sh",
    ];
    let operational_markers = [
        "查看",
        "看看",
        "检查",
        "排查",
        "分析",
        "读取",
        "搜索",
        "查找",
        "列出",
        "运行",
        "执行",
        "修复",
        "修改",
        "创建",
        "删除",
        "文件",
        "目录",
        "路径",
        "日志",
        "状态",
        "进程",
        "端口",
        "网络",
        "配置",
        "几点",
        "status",
        "check",
        "inspect",
        "read",
        "search",
        "find",
        "list",
        "run",
        "execute",
        "debug",
        "review",
        "fix",
        "edit",
        "file",
        "directory",
        "path",
        "log",
        "logs",
        "process",
        "port",
        "network",
        "config",
    ];
    let freshness_markers = [
        "现在", "当前", "今天", "最新", "latest", "current", "today", "now",
    ];
    let freshness_targets = [
        "几点",
        "时间",
        "天气",
        "温度",
        "状态",
        "日志",
        "版本",
        "价格",
        "time",
        "weather",
        "temperature",
        "status",
        "log",
        "logs",
        "version",
        "price",
        "news",
    ];
    let analysis_markers = [
        "先",
        "再",
        "并且",
        "同时",
        "分别",
        "步骤",
        "排查",
        "分析",
        "review",
        "analyze",
        "compare",
        "investigate",
        "debug",
        "plan",
    ];

    if has_path_like
        || file_markers.iter().any(|marker| lower.contains(marker))
        || (freshness_markers
            .iter()
            .any(|marker| content.contains(marker) || lower.contains(marker))
            && freshness_targets
                .iter()
                .any(|marker| content.contains(marker) || lower.contains(marker)))
        || operational_markers
            .iter()
            .any(|marker| content.contains(marker) || lower.contains(marker))
    {
        return ToolUseDemand::RequiredFirstTurn;
    }

    let analysis_hits = analysis_markers
        .iter()
        .filter(|marker| content.contains(**marker) || lower.contains(**marker))
        .count();
    if separator_count >= 2 || analysis_hits >= 2 || char_count >= 120 {
        ToolUseDemand::Preferred
    } else {
        ToolUseDemand::Flexible
    }
}

fn looks_like_explicit_limitation(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let markers = [
        "无法",
        "不能",
        "没法",
        "没有",
        "不支持",
        "做不到",
        "can't",
        "cannot",
        "unable",
        "not available",
        "do not have access",
        "don't have access",
    ];
    markers
        .iter()
        .any(|marker| content.contains(marker) || lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
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
        );
        assert!(plan.has_tools());
        assert!(!plan.uses_native_tools());
        assert!(plan.request_tools().is_none());
    }

    #[test]
    fn operational_requests_require_first_round_tool_for_linux_native_mode() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Require);
        assert_eq!(plan.tool_choice(1, false), ToolChoicePolicy::Require);
    }

    #[test]
    fn embedded_mode_keeps_tool_choice_flexible() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::Embedded);
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
    }

    #[test]
    fn request_plan_emits_missing_tool_followup_for_required_requests() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &PromptGuidedLlm,
            AgentRunStrategy::LinuxEnhanced,
        );
        assert!(plan
            .missing_tool_followup(0, false, "我来总结一下当前情况。")
            .is_some());
    }

    #[test]
    fn explicit_limitation_skips_missing_tool_followup() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan = AgentRequestPlan::build(
            &msg,
            &registry,
            &PromptGuidedLlm,
            AgentRunStrategy::LinuxEnhanced,
        );
        assert!(plan
            .missing_tool_followup(0, false, "我无法访问该日志，当前可用工具也不能读取它。")
            .is_none());
    }

    #[test]
    fn conversational_questions_do_not_force_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "1+1 等于多少", false).expect("pcmsg");
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        assert_eq!(plan.tool_choice(0, false), ToolChoicePolicy::Auto);
        assert!(plan
            .missing_tool_followup(0, false, "1+1 等于 2。")
            .is_none());
    }

    #[test]
    fn linux_inspection_tools_add_specialized_guidance() {
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
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.contains("Linux Inspection Guidance"));
        assert!(system.contains("board_info"));
        assert!(system.contains("process"));
        assert!(system.contains("network_scan only for WiFi/AP scan"));
    }

    #[test]
    fn linux_inspection_guidance_works_with_only_process_and_network() {
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
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.contains("Linux Inspection Guidance"));
        assert!(system.contains("Use process for one specific process or service."));
        assert!(system.contains(
            "Use network for interfaces, DNS, routes, resolve, ping, and HTTP reachability."
        ));
        assert!(!system.contains("board_info"));
        assert!(!system.contains("network_scan"));
    }

    #[test]
    fn linux_inspection_guidance_skips_general_conversation() {
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
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(!system.contains("Linux Inspection Guidance"));
    }

    #[test]
    fn required_requests_add_retrieval_discipline_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(VisibleTool));
        let msg = PcMsg::new_inbound("telegram", "chat", "查看 /tmp/app.log 最近错误", false)
            .expect("pcmsg");
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.contains("Retrieval Discipline"));
        assert!(system.contains("search or list first only to locate concrete targets"));
        assert!(system.contains("turn-local evidence"));
    }

    #[test]
    fn private_garden_adds_internal_memory_governance_guidance() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(NamedTool {
            name: "private_garden",
            description: "free private workspace",
            metadata: ToolMetadata::task(),
        }));
        let msg = PcMsg::new_inbound("telegram", "chat", "我们继续聊", false).expect("pcmsg");
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.contains("Internal Memory Governance"));
        assert!(system.contains("private_garden"));
        assert!(system.contains("update, merge, move, or prune in place"));
    }

    #[test]
    fn archive_memory_guidance_allows_grounded_shareable_answers() {
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
        let plan =
            AgentRequestPlan::build(&msg, &registry, &NativeLlm, AgentRunStrategy::LinuxEnhanced);
        let mut system = String::new();
        plan.apply_system_prompt(&mut system, 4096);
        assert!(system.contains("shareable, user-facing conclusion"));
        assert!(system.contains("exact detail is still unsupported"));
    }
}
