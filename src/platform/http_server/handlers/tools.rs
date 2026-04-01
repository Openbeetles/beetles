//! GET /api/tools：返回可用工具列表及描述。

use super::HandlerContext;

#[derive(serde::Serialize)]
struct ToolInfo {
    name: &'static str,
    description: &'static str,
}

fn tool_infos() -> Vec<ToolInfo> {
    let mut tools = vec![
        ToolInfo {
            name: "get_time",
            description: "获取当前时间（UTC 或本地时区）",
        },
        ToolInfo {
            name: "files",
            description: "文件系统操作（读取、列出、删除文件）",
        },
        ToolInfo {
            name: "file_write",
            description: "向存储写入或追加文件",
        },
        ToolInfo {
            name: "remind_at",
            description: "设置定时提醒",
        },
        ToolInfo {
            name: "remind_list",
            description: "列出所有待执行的提醒",
        },
        ToolInfo {
            name: "board_info",
            description: "获取板型信息",
        },
        ToolInfo {
            name: "kv_store",
            description: "键值存储操作",
        },
    ];

    #[cfg(feature = "tools_network_extra")]
    {
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo {
            name: "document_search",
            description: "存储内文档检索",
        });
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo {
            name: "document_read",
            description: "统一文档读取",
        });
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        tools.push(ToolInfo {
            name: "document_extract",
            description: "文档定向提取",
        });
        tools.push(ToolInfo {
            name: "web_search",
            description: "网络搜索",
        });
        tools.push(ToolInfo {
            name: "analyze_image",
            description: "图像分析",
        });
    }

    #[cfg(feature = "tools_diagnostics")]
    {
        tools.push(ToolInfo {
            name: "device_control",
            description: "硬件设备控制",
        });
        tools.push(ToolInfo {
            name: "i2c_device",
            description: "I2C 设备操作",
        });
        tools.push(ToolInfo {
            name: "i2c_sensor",
            description: "I2C 传感器读取",
        });
        tools.push(ToolInfo {
            name: "memory_manage",
            description: "内存管理",
        });
        tools.push(ToolInfo {
            name: "session_manage",
            description: "会话管理",
        });
        tools.push(ToolInfo {
            name: "system_control",
            description: "系统控制",
        });
        tools.push(ToolInfo {
            name: "cron_manage",
            description: "定时任务管理",
        });
        tools.push(ToolInfo {
            name: "sensor_watch",
            description: "传感器监控",
        });
        tools.push(ToolInfo {
            name: "network_scan",
            description: "网络扫描",
        });
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
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"document_search"));
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        assert!(names.contains(&"document_extract"));
        assert!(!names.contains(&"web_fetch"));
        assert!(!names.contains(&"pdf_read"));
    }
}
