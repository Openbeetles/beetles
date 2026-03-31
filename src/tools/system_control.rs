//! system_control 工具：系统控制（重启、SPIFFS 用量）。
//! system_control tool: system control (restart, SPIFFS usage).

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::json;
use std::sync::Arc;

pub struct SystemControlTool {
    platform: Arc<dyn Platform>,
}

impl SystemControlTool {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl Tool for SystemControlTool {
    fn name(&self) -> &'static str {
        "system_control"
    }
    fn description(&self) -> &'static str {
        "System control operations. Op: restart (requires confirm=true), spiffs_usage (storage usage)."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "description": "Operation: restart|spiffs_usage" },
                "confirm": { "type": "boolean", "description": "Must be true for restart operation" }
            },
            "required": ["op"]
        })
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_system_control")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_system_control", "missing op"))?;

        match op {
            "restart" => {
                let confirm = obj
                    .get("confirm")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if !confirm {
                    return Ok(json!({
                        "op": "restart",
                        "ok": false,
                        "error": "restart requires confirm=true for safety"
                    })
                    .to_string());
                }
                log::warn!("[system_control] restart requested via tool");
                self.platform.request_restart();
                Ok(json!({"op": "restart", "ok": true, "message": "restarting..."}).to_string())
            }
            "spiffs_usage" => {
                let usage = self.platform.spiffs_usage();
                match usage {
                    Some((used, total)) => {
                        let percent = if total > 0 {
                            (used as f64 / total as f64 * 100.0) as u32
                        } else {
                            0
                        };
                        Ok(json!({
                            "op": "spiffs_usage",
                            "used_bytes": used,
                            "total_bytes": total,
                            "used_percent": percent
                        })
                        .to_string())
                    }
                    None => Ok(json!({
                        "op": "spiffs_usage",
                        "error": "SPIFFS usage not available on this platform"
                    })
                    .to_string()),
                }
            }
            _ => Err(Error::config(
                "tool_system_control",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::admin()
    }
}
