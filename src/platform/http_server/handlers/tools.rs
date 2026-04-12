//! GET /api/tools: returns available tools with translation keys only.

use super::HandlerContext;

#[derive(serde::Serialize)]
struct ToolInfo {
    name: &'static str,
    i18n_key: String,
}

impl ToolInfo {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            i18n_key: format!("tools.{}", name),
        }
    }
}

fn tool_infos(ctx: &HandlerContext) -> Vec<ToolInfo> {
    ctx.tool_registry
        .tool_names()
        .into_iter()
        .map(ToolInfo::new)
        .collect()
}

/// 生成工具列表 JSON body。
pub fn body(ctx: &HandlerContext) -> Result<String, std::io::Error> {
    serde_json::to_string(&tool_infos(ctx)).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::tool_infos;
    use serde_json::Value;

    #[test]
    fn tools_api_uses_registry_order() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let payload = serde_json::to_string(&tool_infos(&ctx)).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        let names: Vec<&str> = parsed
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert_eq!(names, ctx.tool_registry.tool_names());
        assert!(!names.is_empty());
    }

    #[test]
    fn tools_api_uses_compact_translation_keys() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let tools = tool_infos(&ctx);
        let board_info = tools.iter().find(|tool| tool.name == "board_info").unwrap();
        assert_eq!(board_info.i18n_key, "tools.board_info");

        #[cfg(feature = "tools_diagnostics")]
        {
            let network_scan = tools
                .iter()
                .find(|tool| tool.name == "network_scan")
                .unwrap();
            assert_eq!(network_scan.i18n_key, "tools.network_scan");
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let process = tools.iter().find(|tool| tool.name == "process").unwrap();
            assert_eq!(process.i18n_key, "tools.process");

            let network = tools.iter().find(|tool| tool.name == "network").unwrap();
            assert_eq!(network.i18n_key, "tools.network");
        }
    }
}
