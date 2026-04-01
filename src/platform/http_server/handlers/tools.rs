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
            name: "file_edit",
            description: "对文本文件做定向局部改写",
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
            description: "整机/整机主机状态快照（CPU/RAM/存储/WiFi STA 等总体信息）",
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
            description: "系统管理动作与状态存储信息（管理员能力）",
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
            description: "WiFi/AP 扫描与 WiFi 连通性检查，不用于通用 Linux 网络诊断",
        });
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        tools.push(ToolInfo {
            name: "process",
            description: "Linux 进程检查（列表/单 PID 详情）",
        });
        tools.push(ToolInfo {
            name: "network",
            description: "Linux 网络检查与探测（接口/DNS/路由/解析/Ping/HTTP 探测）",
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
        assert!(names.contains(&"file_edit"));
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
    fn tools_api_descriptions_match_linux_tool_boundaries() {
        let tools = tool_infos();
        let board_info = tools.iter().find(|tool| tool.name == "board_info").unwrap();
        assert!(board_info.description.contains("状态快照"));

        #[cfg(feature = "tools_diagnostics")]
        {
            let network_scan = tools
                .iter()
                .find(|tool| tool.name == "network_scan")
                .unwrap();
            assert!(network_scan.description.contains("WiFi/AP 扫描"));
            assert!(network_scan
                .description
                .contains("不用于通用 Linux 网络诊断"));
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            let process = tools.iter().find(|tool| tool.name == "process").unwrap();
            assert!(process.description.contains("单 PID 详情"));

            let network = tools.iter().find(|tool| tool.name == "network").unwrap();
            assert!(network.description.contains("接口/DNS/路由/解析/Ping/HTTP"));
        }
    }
}
