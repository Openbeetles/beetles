//! Linux-only programmable checker for structured state-machine analysis.

use crate::error::{Error, Result};
use crate::reasoning::{
    validate_state_machine_result, CurrentExecutableLuaSandboxExecutor, LuaQueryBudget,
    LuaQueryRequest, LuaQueryResponse, ReasoningExecutor,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct LuaStateMachineCheckerTool {
    executor: Arc<dyn ReasoningExecutor>,
}

#[derive(Serialize)]
struct LuaStateMachineCheckerToolResponse {
    ok: bool,
    plane: &'static str,
    readonly: bool,
    source_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<crate::reasoning::StateMachineResult>,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    budget: LuaQueryBudget,
}

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
        let obj = parse_tool_args(args, "lua_state_machine_checker_tool")?;
        let script = obj
            .get("script")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_state_machine_checker_tool", "missing script"))?;
        let source_text = obj
            .get("source_text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::config("lua_state_machine_checker_tool", "missing source_text")
            })?;
        let timeout_ms = obj.get("timeout_ms").and_then(Value::as_u64);
        let source_name = obj
            .get("source_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("reference")
            .to_string();
        let focus = obj
            .get("focus")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let request = LuaQueryRequest {
            script: script.to_string(),
            input: serde_json::json!({
                "source_name": source_name,
                "source_text": source_text,
                "focus": focus,
            }),
            budget: timeout_ms
                .map(|value| LuaQueryBudget::default().with_timeout_ms(value))
                .unwrap_or_default(),
            capabilities: default_lua_state_machine_checker_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        let output = build_tool_response(response, source_name, focus)?;
        serialize_tool_output("lua_state_machine_checker_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_user_ingress(false)
            .with_system_ingress(false)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

fn default_lua_state_machine_checker_capabilities() -> Vec<String> {
    vec![
        "read_input".to_string(),
        "emit_result".to_string(),
        "emit_trace".to_string(),
        "propose_state_machines".to_string(),
    ]
}

fn build_tool_response(
    response: LuaQueryResponse,
    source_name: String,
    focus: Option<String>,
) -> Result<LuaStateMachineCheckerToolResponse> {
    let validated_result = if response.ok {
        let result = response
            .result
            .ok_or_else(|| Error::config("lua_state_machine_checker_tool", "missing result"))?;
        Some(validate_state_machine_result(result)?)
    } else {
        None
    };

    Ok(LuaStateMachineCheckerToolResponse {
        ok: response.ok,
        plane: "engineering_state_machine_plane",
        readonly: true,
        source_name,
        focus,
        result: validated_result,
        trace: response.trace,
        error_kind: response.error_kind,
        error_message: response.error_message,
        budget: response.budget,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
