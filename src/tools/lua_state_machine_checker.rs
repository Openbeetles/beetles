//! Linux-only programmable checker for structured state-machine analysis.

use crate::error::Result;
use crate::reasoning::{
    validate_state_machine_result, CurrentExecutableLuaSandboxExecutor, ReasoningExecutor,
};
use crate::tools::{
    lua_protocol_frame_helper::{
        build_lua_sandboxed_helper_response, prepare_lua_sandboxed_helper_invocation,
    },
    serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct LuaStateMachineCheckerTool {
    executor: Arc<dyn ReasoningExecutor>,
}

const LUA_STATE_MACHINE_HELPER_CAPABILITIES: [&str; 4] = [
    "read_input",
    "emit_result",
    "emit_trace",
    "propose_state_machines",
];

impl LuaStateMachineCheckerTool {
    pub fn new(executor: Arc<dyn ReasoningExecutor>) -> Self {
        Self { executor }
    }
}

impl Default for LuaStateMachineCheckerTool {
    fn default() -> Self {
        Self::new(Arc::new(CurrentExecutableLuaSandboxExecutor))
    }
}

impl Tool for LuaStateMachineCheckerTool {
    fn name(&self) -> &'static str {
        "lua_state_machine_checker"
    }

    fn description(&self) -> &str {
        "Run a Linux-only Lua state-machine checker in a sandboxed subprocess. Input is explicit engineering reference text; output must be a structured, adjudication-required state machine proposal with optional findings."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk."},"source_text":{"type":"string","description":"Engineering reference text such as state diagrams, init flows, or transition notes."},"source_name":{"type":"string","description":"Optional source label."},"focus":{"type":"string","description":"Optional focus hint such as boot_flow or power_state."},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the programmable reasoning budget window."}},"required":["script","source_text"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let invocation = prepare_lua_sandboxed_helper_invocation(
            args,
            "lua_state_machine_checker_tool",
            &LUA_STATE_MACHINE_HELPER_CAPABILITIES,
        )?;
        let response = self.executor.execute_query(&invocation.request)?;
        let output = build_lua_sandboxed_helper_response(
            response,
            "lua_state_machine_checker_tool",
            "engineering_state_machine_plane",
            invocation.source_name,
            invocation.focus,
            validate_state_machine_result,
        )?;
        serialize_tool_output("lua_state_machine_checker_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_risk_level(ToolRiskLevel::Medium)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::reasoning::{LuaQueryRequest, LuaQueryResponse};
    use serde_json::{json, Value};

    struct StubExecutor;

    impl ReasoningExecutor for StubExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": format!(
                        "Checked state machine from {} chars.",
                        request
                            .input
                            .get("source_text")
                            .and_then(Value::as_str)
                            .map(str::len)
                            .unwrap_or_default()
                    ),
                    "machines": [{
                        "name": "boot_flow",
                        "summary": "Boot state progression from reset to ready.",
                        "states": [
                            {
                                "name": "RESET",
                                "role": "initial",
                                "summary": "Power-on reset state.",
                                "evidence_refs": ["figure 2"],
                                "requires_adjudication": true
                            },
                            {
                                "name": "READY",
                                "role": "terminal",
                                "summary": "Ready for commands.",
                                "evidence_refs": ["figure 2"],
                                "requires_adjudication": true
                            }
                        ],
                        "transitions": [{
                            "from": "RESET",
                            "to": "READY",
                            "trigger": "init_complete",
                            "summary": "Initialization finishes successfully.",
                            "evidence_refs": ["section 3.1"],
                            "requires_adjudication": true
                        }],
                        "findings": [{
                            "kind": "unsafe_loop",
                            "summary": "Review whether retries can loop forever before READY.",
                            "state_refs": ["RESET"],
                            "transition_refs": ["RESET->READY:init_complete"],
                            "evidence_refs": ["section 3.1"],
                            "requires_adjudication": true
                        }],
                        "evidence_refs": ["figure 2", "section 3.1"],
                        "requires_adjudication": true
                    }]
                }),
                vec!["trace:machine".to_string()],
                request.budget.clone(),
            ))
        }
    }

    struct InvalidTransitionExecutor;

    impl ReasoningExecutor for InvalidTransitionExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": "bad",
                    "machines": [{
                        "name": "boot_flow",
                        "summary": "Boot state progression.",
                        "states": [
                            {
                                "name": "RESET",
                                "role": "initial",
                                "summary": "Reset.",
                                "evidence_refs": ["figure 2"],
                                "requires_adjudication": true
                            },
                            {
                                "name": "READY",
                                "role": "terminal",
                                "summary": "Ready.",
                                "evidence_refs": ["figure 2"],
                                "requires_adjudication": true
                            }
                        ],
                        "transitions": [{
                            "from": "RESET",
                            "to": "READY",
                            "trigger": "init_complete",
                            "summary": "Initialization completes.",
                            "evidence_refs": ["section 3.1"],
                            "requires_adjudication": false
                        }],
                        "evidence_refs": ["figure 2"],
                        "requires_adjudication": true
                    }]
                }),
                vec!["trace:invalid".to_string()],
                request.budget.clone(),
            ))
        }
    }

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "lua_state_machine_checker_tool_test",
                "network unused",
            ))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "lua_state_machine_checker_tool_test",
                "network unused",
            ))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_state_machine_checker_returns_validated_state_machine_contract() {
        let tool = LuaStateMachineCheckerTool::new(Arc::new(StubExecutor));
        let mut ctx = DummyCtx;

        let output = tool
            .execute(
                r#"{"script":"return {}","source_name":"power_seq","focus":"boot_flow","source_text":"reset to ready state flow"}"#,
                &mut ctx,
            )
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");

        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["plane"], json!("engineering_state_machine_plane"));
        assert_eq!(parsed["readonly"], json!(true));
        assert_eq!(parsed["source_name"], json!("power_seq"));
        assert_eq!(parsed["focus"], json!("boot_flow"));
        assert_eq!(parsed["result"]["machines"][0]["name"], json!("boot_flow"));
        assert_eq!(
            parsed["result"]["machines"][0]["findings"][0]["kind"],
            json!("unsafe_loop")
        );
    }

    #[test]
    fn lua_state_machine_checker_rejects_non_adjudicated_transition() {
        let tool = LuaStateMachineCheckerTool::new(Arc::new(InvalidTransitionExecutor));
        let mut ctx = DummyCtx;

        let error = tool
            .execute(
                r#"{"script":"return {}","source_text":"boot transition excerpt"}"#,
                &mut ctx,
            )
            .expect_err("transition should be rejected");

        assert!(error
            .to_string()
            .contains("state machine transition must require adjudication"));
    }
}
