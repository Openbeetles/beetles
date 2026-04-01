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

fn tool_infos() -> Vec<ToolInfo> {
    let mut tools = vec![
        ToolInfo::new("get_time"),
        ToolInfo::new("calendar"),
        ToolInfo::new("files"),
        ToolInfo::new("file_write"),
        ToolInfo::new("file_edit"),
        ToolInfo::new("remind_at"),
        ToolInfo::new("remind_list"),
        ToolInfo::new("board_info"),
        ToolInfo::new("kv_store"),
    ];

    #[cfg(feature = "tools_network_extra")]
    {
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo::new("document_search"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo::new("document_read"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo::new("document_extract"));
        tools.push(ToolInfo::new("web_search"));
        tools.push(ToolInfo::new("analyze_image"));
    }

    #[cfg(feature = "tools_diagnostics")]
    {
        tools.push(ToolInfo::new("device_control"));
        tools.push(ToolInfo::new("i2c_device"));
        tools.push(ToolInfo::new("i2c_sensor"));
        tools.push(ToolInfo::new("memory_manage"));
        tools.push(ToolInfo::new("session_manage"));
        tools.push(ToolInfo::new("system_control"));
        tools.push(ToolInfo::new("cron_manage"));
        tools.push(ToolInfo::new("sensor_watch"));
        tools.push(ToolInfo::new("network_scan"));
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        tools.push(ToolInfo::new("process"));
        tools.push(ToolInfo::new("network"));
    }

    tools
}

/// 生成工具列表 JSON body。
pub fn body(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    serde_json::to_string(&tool_infos()).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::tool_infos;
    use serde_json::Value;

    #[test]
    fn tools_api_hides_internal_document_read_helpers() {
        let payload = serde_json::to_string(&tool_infos()).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        let names: Vec<&str> = parsed
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|item| item.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"file_write"));
        assert!(names.contains(&"file_edit"));
        assert!(names.contains(&"calendar"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"document_search"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"document_extract"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"process"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"network"));
        assert!(!names.contains(&"web_fetch"));
        assert!(!names.contains(&"pdf_read"));
    }

    #[test]
    fn tools_api_uses_compact_translation_keys() {
        let tools = tool_infos();
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
