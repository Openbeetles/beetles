//! Linux-only programmable reasoning tool for single-turn Lua queries.

use crate::error::{Error, Result};
use crate::reasoning::{
    default_lua_query_capabilities, CurrentExecutableLuaSandboxExecutor, LuaQueryBudget,
    LuaQueryRequest, ReasoningExecutor,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct LuaQueryTool {
    executor: Arc<dyn ReasoningExecutor>,
}

impl LuaQueryTool {
    pub fn new(executor: Arc<dyn ReasoningExecutor>) -> Self {
        Self { executor }
    }
}

impl Default for LuaQueryTool {
    fn default() -> Self {
        Self::new(Arc::new(CurrentExecutableLuaSandboxExecutor))
    }
}

impl Tool for LuaQueryTool {
    fn name(&self) -> &'static str {
        "lua_query"
    }

    fn description(&self) -> &str {
        "Run a Linux-only single-turn Lua query in a sandboxed subprocess. Input is explicit JSON; output is structured JSON with result, trace, and error fields. No host filesystem, shell, network, or direct memory writes are available."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk. Return value becomes result."},"input":{"description":"Explicit JSON input injected as global `input`"},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the P1 budget window."}},"required":["script","input"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "lua_query_tool")?;
        let script = obj
            .get("script")
            .and_then(|value| value.as_str())
            .ok_or_else(|| Error::config("lua_query_tool", "missing script"))?;
        let input = obj
            .get("input")
            .cloned()
            .ok_or_else(|| Error::config("lua_query_tool", "missing input"))?;
        let timeout_ms = obj.get("timeout_ms").and_then(|value| value.as_u64());
        let request = LuaQueryRequest {
            script: script.to_string(),
            input,
            budget: timeout_ms
                .map(|value| LuaQueryBudget::default().with_timeout_ms(value))
                .unwrap_or_default(),
            capabilities: default_lua_query_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        serialize_tool_output("lua_query_tool", &response)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_user_ingress(false)
            .with_system_ingress(false)
            .with_risk_level(ToolRiskLevel::Medium)
    }
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
                    "script_len": request.script.len(),
                    "echo": request.input,
                }),
                vec!["trace:ok".to_string()],
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
            Err(Error::config("lua_query_tool_test", "network unused"))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("lua_query_tool_test", "network unused"))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_query_tool_returns_structured_json() {
        let tool = LuaQueryTool::new(Arc::new(StubExecutor));
        let mut ctx = DummyCtx;
        let output = tool
            .execute(
                r#"{"script":"return input","input":{"hello":"world"},"timeout_ms":500}"#,
                &mut ctx,
            )
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");
        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["result"]["echo"], json!({"hello":"world"}));
        assert_eq!(parsed["trace"], json!(["trace:ok"]));
        assert_eq!(parsed["budget"]["timeout_ms"], json!(500));
    }
}
