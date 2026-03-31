//! GET /api/tools：返回可用工具列表及描述。

use super::HandlerContext;

#[derive(serde::Serialize)]
struct ToolInfo {
    name: &'static str,
    description: &'static str,
}

/// 生成工具列表 JSON body。
pub fn body(_ctx: &HandlerContext) -> Result<String, std::io::Error> {
    let mut tools = vec![
        ToolInfo {
            name: "get_time",
            description: "获取当前时间（UTC 或本地时区）",
        },
        ToolInfo {
            name: "files",
            description: "文件系统操作（读取、写入、列出文件）",
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

    serde_json::to_string(&tools).map_err(std::io::Error::other)
}
