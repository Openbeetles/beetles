//! Linux-only programmable tool bridge for capability-scoped proposal generation.

use crate::error::{Error, Result};
use crate::reasoning::{
    default_lua_query_capabilities, validate_tool_request_result,
    CurrentExecutableLuaSandboxExecutor, LuaQueryBudget, LuaQueryRequest, LuaQueryResponse,
    ReasoningExecutor, ToolRequestResult,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct LuaToolBridgeTool {
    executor: Arc<dyn ReasoningExecutor>,
}

#[derive(Serialize)]
struct LuaToolBridgeToolResponse {
    ok: bool,
    plane: &'static str,
    proposal_only: bool,
    tool_catalog_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<ToolRequestResult>,
    #[serde(default)]
    assessments: Vec<crate::tools::ToolBridgeProposalAssessment>,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    budget: LuaQueryBudget,
}

impl LuaToolBridgeTool {
    pub fn new(executor: Arc<dyn ReasoningExecutor>) -> Self {
        Self { executor }
    }
}

impl Default for LuaToolBridgeTool {
    fn default() -> Self {
        Self::new(Arc::new(CurrentExecutableLuaSandboxExecutor))
    }
}

impl Tool for LuaToolBridgeTool {
    fn name(&self) -> &'static str {
        "lua_tool_bridge"
    }

    fn description(&self) -> &str {
        "Run a Linux-only single-turn Lua bridge over the governed tool catalog. The script can inspect tool contracts and emit structured tool_request_proposals, but it cannot execute tools directly."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk. It receives `request_context`, `tool_catalog`, and `constraints`, and must return a summary plus optional tool_request_proposals."},"input":{"description":"Explicit JSON request context forwarded into the bridge input."},"tool_names":{"type":"array","items":{"type":"string"},"description":"Optional allowlist of tool names to expose to the bridge."},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the programmable reasoning budget window."}},"required":["script"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "lua_tool_bridge_tool")?;
        let script = obj
            .get("script")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_tool_bridge_tool", "missing script"))?;
        let request_context = obj.get("input").cloned().unwrap_or(Value::Null);
        let mut tool_catalog = ctx.tool_bridge_catalog()?;
        tool_catalog.retain(|entry| entry.name != self.name());
        if let Some(tool_names) = obj.get("tool_names").and_then(Value::as_array) {
            let wanted = tool_names
                .iter()
                .filter_map(Value::as_str)
                .collect::<std::collections::BTreeSet<_>>();
            if !wanted.is_empty() {
                tool_catalog.retain(|entry| wanted.contains(entry.name.as_str()));
            }
        }
        let timeout_ms = obj.get("timeout_ms").and_then(Value::as_u64);
        let request = LuaQueryRequest {
            script: script.to_string(),
            input: serde_json::json!({
                "request_context": request_context,
                "tool_catalog": tool_catalog,
                "constraints": {
                    "proposal_only": true,
                    "direct_execution": false,
                    "requires_adjudication": true
                }
            }),
            budget: timeout_ms
                .map(|value| LuaQueryBudget::default().with_timeout_ms(value))
                .unwrap_or_default(),
            capabilities: default_lua_tool_bridge_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        let output = build_tool_response(response, ctx, tool_catalog.len())?;
        serialize_tool_output("lua_tool_bridge_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_user_ingress(false)
            .with_system_ingress(false)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

fn default_lua_tool_bridge_capabilities() -> Vec<String> {
    let mut capabilities = default_lua_query_capabilities();
    capabilities.push("read_tool_catalog".to_string());
    capabilities.push("emit_tool_request_proposals".to_string());
    capabilities
}

fn build_tool_response(
    response: LuaQueryResponse,
    ctx: &mut dyn ToolContext,
    tool_catalog_count: usize,
) -> Result<LuaToolBridgeToolResponse> {
    let (result, assessments) = if response.ok {
        let result = response
            .result
            .clone()
            .ok_or_else(|| Error::config("lua_tool_bridge_tool", "missing result"))?;
        let validated = validate_tool_request_result(result)?;
        let assessments = validated
            .tool_request_proposals
            .iter()
            .map(|proposal| ctx.assess_tool_request_proposal(&proposal.tool_name, &proposal.args))
            .collect::<Result<Vec<_>>>()?;
        (Some(validated), assessments)
    } else {
        (None, Vec::new())
    };
    Ok(LuaToolBridgeToolResponse {
        ok: response.ok,
        plane: "capability_bridge_expansion",
        proposal_only: true,
        tool_catalog_count,
        result,
        assessments,
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
                    "summary": "Prepared one tool request proposal.",
                    "tool_request_proposals": [{
                        "tool_name": "files",
                        "summary": "Inspect the current workspace tree.",
                        "rationale": "A directory listing is needed before planning edits.",
                        "args": {"path": ".", "mode": "list"},
                        "requires_adjudication": true
                    }]
                }),
                vec![format!(
                    "catalog={}",
                    request.input["tool_catalog"]
                        .as_array()
                        .map(|value| value.len())
                        .unwrap_or(0)
                )],
                request.budget.clone(),
            ))
        }
    }

    struct DummyCtx;

    impl crate::tools::ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("lua_tool_bridge_test", "network unused"))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("lua_tool_bridge_test", "network unused"))
        }

        fn tool_bridge_catalog(&self) -> Result<Vec<crate::tools::ToolBridgeCatalogEntry>> {
            Ok(vec![crate::tools::ToolBridgeCatalogEntry {
                name: "files".to_string(),
                description: "Inspect files".to_string(),
                parameters_json: r#"{"type":"object"}"#.to_string(),
                effect_class: crate::tools::ToolEffectClass::HostInspection,
                risk_level: crate::tools::ToolRiskLevel::Low,
                approval_mode: crate::tools::ToolApprovalMode::Automatic,
                rollback_kind: crate::tools::ToolRollbackKind::None,
                requires_network: false,
                required_runtime_capabilities: vec!["storage_state_fs".to_string()],
                allow_when_degraded: false,
            }])
        }

        fn assess_tool_request_proposal(
            &self,
            tool_name: &str,
            _args: &Value,
        ) -> Result<crate::tools::ToolBridgeProposalAssessment> {
            Ok(crate::tools::ToolBridgeProposalAssessment {
                tool_name: tool_name.to_string(),
                decision: crate::tools::ToolBridgeProposalDecision::Allowed,
                summary: "proposal matches current tool governance contract".to_string(),
                effect_class: crate::tools::ToolEffectClass::HostInspection,
                risk_level: crate::tools::ToolRiskLevel::Low,
                approval_mode: crate::tools::ToolApprovalMode::Automatic,
                rollback_kind: crate::tools::ToolRollbackKind::None,
                requires_network: false,
                required_runtime_capabilities: vec!["storage_state_fs".to_string()],
                allow_when_degraded: false,
            })
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_tool_bridge_returns_structured_proposals_and_assessments() {
        let tool = LuaToolBridgeTool::new(Arc::new(StubExecutor));
        let mut ctx = DummyCtx;
        let output = tool
            .execute(
                r#"{"script":"return {}","input":{"goal":"inspect workspace"},"timeout_ms":500}"#,
                &mut ctx,
            )
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");
        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["plane"], json!("capability_bridge_expansion"));
        assert_eq!(parsed["proposal_only"], json!(true));
        assert_eq!(
            parsed["result"]["tool_request_proposals"][0]["tool_name"],
            json!("files")
        );
        assert_eq!(parsed["assessments"][0]["decision"], json!("allowed"));
    }
}
