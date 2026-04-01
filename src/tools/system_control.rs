//! system_control tool: constrained system admin actions.

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::{json, Value};
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
        "System admin operations. Op: storage_usage (state storage usage), status (full board/system status), restart (requires confirm=true)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["storage_usage", "status", "restart"],
                    "description": "Operation: storage_usage | status | restart"
                },
                "confirm": {
                    "type": "boolean",
                    "description": "Must be true for restart"
                }
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
            "storage_usage" => {
                let usage = self.platform.spiffs_usage();
                let state_root = crate::platform::state_mount_path();
                match usage {
                    Some((total, used)) => {
                        let free = total.saturating_sub(used);
                        let used_percent = if total > 0 {
                            ((used as f64 / total as f64) * 100.0).round() as u32
                        } else {
                            0
                        };
                        Ok(json!({
                            "op": "storage_usage",
                            "path": state_root,
                            "total_bytes": total,
                            "used_bytes": used,
                            "free_bytes": free,
                            "used_percent": used_percent,
                        })
                        .to_string())
                    }
                    None => Ok(json!({
                        "op": "storage_usage",
                        "path": state_root,
                        "error": "storage usage not available on this platform"
                    })
                    .to_string()),
                }
            }
            "status" => {
                let payload = self.platform.board_info_json()?;
                let parsed: Value = serde_json::from_str(&payload).map_err(|e| Error::Other {
                    source: Box::new(e),
                    stage: "tool_system_control",
                })?;
                Ok(json!({
                    "op": "status",
                    "system": parsed,
                })
                .to_string())
            }
            "restart" => {
                let confirm = obj
                    .get("confirm")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                if !confirm {
                    return Ok(json!({
                        "op": "restart",
                        "ok": false,
                        "error": "restart requires confirm=true"
                    })
                    .to_string());
                }
                log::warn!("[system_control] restart requested via tool");
                self.platform.request_restart();
                Ok(json!({
                    "op": "restart",
                    "ok": true,
                    "message": "restart requested"
                })
                .to_string())
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
