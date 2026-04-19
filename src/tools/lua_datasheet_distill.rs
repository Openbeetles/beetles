//! Linux-only programmable reasoning tool for single-turn datasheet distillation.

use crate::error::{Error, Result};
use crate::reasoning::{
    validate_engineering_distillation_result, CurrentExecutableLuaSandboxExecutor, LuaQueryBudget,
    LuaQueryRequest, LuaQueryResponse, ReasoningExecutor,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct LuaDatasheetDistillTool {
    executor: Arc<dyn ReasoningExecutor>,
}

#[derive(Serialize)]
struct LuaDatasheetDistillToolResponse {
    ok: bool,
    plane: &'static str,
    readonly: bool,
    source_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<crate::reasoning::EngineeringDistillationResult>,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    budget: LuaQueryBudget,
}

impl LuaDatasheetDistillTool {
    pub fn new(executor: Arc<dyn ReasoningExecutor>) -> Self {
        Self { executor }
    }
}

impl Default for LuaDatasheetDistillTool {
    fn default() -> Self {
        Self::new(Arc::new(CurrentExecutableLuaSandboxExecutor))
    }
}

impl Tool for LuaDatasheetDistillTool {
    fn name(&self) -> &'static str {
        "lua_datasheet_distill"
    }

    fn description(&self) -> &str {
        "Run a Linux-only Lua datasheet distillation script in a sandboxed subprocess. Input is explicit engineering reference text plus optional focus hints; output remains structured and reviewable."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk."},"source_text":{"type":"string","description":"Engineering reference text such as a datasheet, protocol excerpt, or register description."},"source_name":{"type":"string","description":"Optional source label."},"focus":{"type":"string","description":"Optional focus hint such as registers, protocol, or state_machine."},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the programmable reasoning budget window."}},"required":["script","source_text"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "lua_datasheet_distill_tool")?;
        let script = obj
            .get("script")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_datasheet_distill_tool", "missing script"))?;
        let source_text = obj
            .get("source_text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_datasheet_distill_tool", "missing source_text"))?;
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
            capabilities: default_lua_datasheet_distill_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        let output = build_tool_response(response, source_name, focus)?;
        serialize_tool_output("lua_datasheet_distill_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_user_ingress(false)
            .with_system_ingress(false)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

fn build_tool_response(
    response: LuaQueryResponse,
    source_name: String,
    focus: Option<String>,
) -> Result<LuaDatasheetDistillToolResponse> {
    let validated_result = if response.ok {
        let result = response
            .result
            .ok_or_else(|| Error::config("lua_datasheet_distill_tool", "missing result"))?;
        Some(validate_engineering_distillation_result(result)?)
    } else {
        None
    };

    Ok(LuaDatasheetDistillToolResponse {
        ok: response.ok,
        plane: "engineering_synthesis_plane",
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

fn default_lua_datasheet_distill_capabilities() -> Vec<String> {
    vec![
        "read_input".to_string(),
        "emit_result".to_string(),
        "emit_trace".to_string(),
        "propose_engineering_assets".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning::LuaQueryResponse;
    use serde_json::{json, Value};

    struct StubExecutor;

    impl ReasoningExecutor for StubExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": format!(
                        "Distilled {} chars of engineering reference.",
                        request
                            .input
                            .get("source_text")
                            .and_then(Value::as_str)
                            .map(str::len)
                            .unwrap_or_default()
                    ),
                    "asset_candidates": [{
                        "kind": "protocol_frame",
                        "title": "Status frame sketch",
                        "summary": "Summarizes the status frame for later review.",
                        "content": "| Byte | Field | Meaning |\\n| 0 | opcode | response opcode |",
                        "evidence_refs": ["frame layout", "section 4.1"],
                        "requires_adjudication": true
                    }]
                }),
                vec!["trace:datasheet".to_string()],
                request.budget.clone(),
            ))
        }
    }

    struct InvalidCandidateExecutor;

    impl ReasoningExecutor for InvalidCandidateExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": "bad",
                    "asset_candidates": [{
                        "kind": "datasheet_note",
                        "title": "Power-up note",
                        "summary": "Missing adjudication guard.",
                        "content": "Wait 5 ms after power-up.",
                        "evidence_refs": ["section 2.1"],
                        "requires_adjudication": false
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
                "lua_datasheet_distill_tool_test",
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
                "lua_datasheet_distill_tool_test",
                "network unused",
            ))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_datasheet_distill_tool_returns_validated_engineering_result_contract() {
        let tool = LuaDatasheetDistillTool::new(Arc::new(StubExecutor));
        let mut ctx = DummyCtx;

        let output = tool
            .execute(
                r#"{"script":"return {}","source_name":"BME280","focus":"protocol","source_text":"status frame spec"}"#,
                &mut ctx,
            )
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");

        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["plane"], json!("engineering_synthesis_plane"));
        assert_eq!(parsed["readonly"], json!(true));
        assert_eq!(parsed["source_name"], json!("BME280"));
        assert_eq!(parsed["focus"], json!("protocol"));
        assert_eq!(
            parsed["result"]["asset_candidates"][0]["kind"],
            json!("protocol_frame")
        );
        assert_eq!(parsed["trace"], json!(["trace:datasheet"]));
    }

    #[test]
    fn lua_datasheet_distill_tool_rejects_non_adjudicated_candidate() {
        let tool = LuaDatasheetDistillTool::new(Arc::new(InvalidCandidateExecutor));
        let mut ctx = DummyCtx;

        let error = tool
            .execute(
                r#"{"script":"return {}","source_text":"power timing table"}"#,
                &mut ctx,
            )
            .expect_err("candidate should be rejected");

        assert!(error
            .to_string()
            .contains("engineering distillation candidate must require adjudication"));
    }
}
