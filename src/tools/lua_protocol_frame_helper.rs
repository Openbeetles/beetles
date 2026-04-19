//! Linux-only programmable helper for structured protocol-frame extraction.

use crate::error::{Error, Result};
use crate::reasoning::{
    validate_protocol_frame_result, CurrentExecutableLuaSandboxExecutor, LuaQueryBudget,
    LuaQueryRequest, LuaQueryResponse, ReasoningExecutor,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct LuaProtocolFrameHelperTool {
    executor: Arc<dyn ReasoningExecutor>,
}

#[derive(Serialize)]
struct LuaProtocolFrameHelperToolResponse {
    ok: bool,
    plane: &'static str,
    readonly: bool,
    source_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<crate::reasoning::ProtocolFrameResult>,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    budget: LuaQueryBudget,
}

impl LuaProtocolFrameHelperTool {
    pub fn new(executor: Arc<dyn ReasoningExecutor>) -> Self {
        Self { executor }
    }
}

impl Default for LuaProtocolFrameHelperTool {
    fn default() -> Self {
        Self::new(Arc::new(CurrentExecutableLuaSandboxExecutor))
    }
}

impl Tool for LuaProtocolFrameHelperTool {
    fn name(&self) -> &'static str {
        "lua_protocol_frame_helper"
    }

    fn description(&self) -> &str {
        "Run a Linux-only Lua protocol-frame helper in a sandboxed subprocess. Input is explicit engineering reference text; output must be a structured, adjudication-required protocol frame proposal."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk."},"source_text":{"type":"string","description":"Engineering reference text such as protocol excerpts, frame layouts, or byte-level examples."},"source_name":{"type":"string","description":"Optional source label."},"focus":{"type":"string","description":"Optional focus hint such as status_frame or init_sequence."},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the programmable reasoning budget window."}},"required":["script","source_text"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "lua_protocol_frame_helper_tool")?;
        let script = obj
            .get("script")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_protocol_frame_helper_tool", "missing script"))?;
        let source_text = obj
            .get("source_text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                Error::config("lua_protocol_frame_helper_tool", "missing source_text")
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
            capabilities: default_lua_protocol_frame_helper_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        let output = build_tool_response(response, source_name, focus)?;
        serialize_tool_output("lua_protocol_frame_helper_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_user_ingress(false)
            .with_system_ingress(false)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

fn default_lua_protocol_frame_helper_capabilities() -> Vec<String> {
    vec![
        "read_input".to_string(),
        "emit_result".to_string(),
        "emit_trace".to_string(),
        "propose_protocol_frames".to_string(),
    ]
}

fn build_tool_response(
    response: LuaQueryResponse,
    source_name: String,
    focus: Option<String>,
) -> Result<LuaProtocolFrameHelperToolResponse> {
    let validated_result = if response.ok {
        let result = response
            .result
            .ok_or_else(|| Error::config("lua_protocol_frame_helper_tool", "missing result"))?;
        Some(validate_protocol_frame_result(result)?)
    } else {
        None
    };

    Ok(LuaProtocolFrameHelperToolResponse {
        ok: response.ok,
        plane: "engineering_protocol_frame_plane",
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
                        "Distilled protocol frame from {} chars.",
                        request
                            .input
                            .get("source_text")
                            .and_then(Value::as_str)
                            .map(str::len)
                            .unwrap_or_default()
                    ),
                    "frames": [{
                        "name": "status_response",
                        "direction": "device_to_host",
                        "summary": "Status response frame carrying opcode and state bits.",
                        "fields": [{
                            "name": "opcode",
                            "byte_range": {"start": 0, "end": 0},
                            "encoding": "u8",
                            "summary": "Response opcode byte.",
                            "fixed_value": "0x81",
                            "evidence_refs": ["section 4.1", "table 7"],
                            "requires_adjudication": true
                        }],
                        "evidence_refs": ["section 4.1", "table 7"],
                        "requires_adjudication": true
                    }]
                }),
                vec!["trace:frame".to_string()],
                request.budget.clone(),
            ))
        }
    }

    struct InvalidFieldExecutor;

    impl ReasoningExecutor for InvalidFieldExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": "bad",
                    "frames": [{
                        "name": "command_request",
                        "direction": "host_to_device",
                        "summary": "Command request frame.",
                        "fields": [{
                            "name": "length",
                            "byte_range": {"start": 1, "end": 1},
                            "encoding": "u8",
                            "summary": "Payload length byte.",
                            "evidence_refs": ["section 3.2"],
                            "requires_adjudication": false
                        }],
                        "evidence_refs": ["section 3.2"],
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
                "lua_protocol_frame_helper_tool_test",
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
                "lua_protocol_frame_helper_tool_test",
                "network unused",
            ))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_protocol_frame_helper_returns_validated_protocol_frame_contract() {
        let tool = LuaProtocolFrameHelperTool::new(Arc::new(StubExecutor));
        let mut ctx = DummyCtx;

        let output = tool
            .execute(
                r#"{"script":"return {}","source_name":"device_proto","focus":"status_frame","source_text":"status response layout"}"#,
                &mut ctx,
            )
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");

        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["plane"], json!("engineering_protocol_frame_plane"));
        assert_eq!(parsed["readonly"], json!(true));
        assert_eq!(parsed["source_name"], json!("device_proto"));
        assert_eq!(parsed["focus"], json!("status_frame"));
        assert_eq!(
            parsed["result"]["frames"][0]["name"],
            json!("status_response")
        );
        assert_eq!(
            parsed["result"]["frames"][0]["fields"][0]["encoding"],
            json!("u8")
        );
    }

    #[test]
    fn lua_protocol_frame_helper_rejects_non_adjudicated_field() {
        let tool = LuaProtocolFrameHelperTool::new(Arc::new(InvalidFieldExecutor));
        let mut ctx = DummyCtx;

        let error = tool
            .execute(
                r#"{"script":"return {}","source_text":"command request excerpt"}"#,
                &mut ctx,
            )
            .expect_err("field should be rejected");

        assert!(error
            .to_string()
            .contains("protocol frame field must require adjudication"));
    }
}
