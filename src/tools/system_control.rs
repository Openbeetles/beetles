//! system_control tool: constrained system admin actions.

use crate::error::{Error, Result};
use crate::tools::{
    parse_tool_args, Tool, ToolApprovalMode, ToolContext, ToolEffectClass, ToolExecutionGovernance,
    ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
};
use crate::Platform;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct SystemControlTool {
    platform: Arc<dyn Platform>,
    tool_execution_governance: Arc<ToolExecutionGovernance>,
}

impl SystemControlTool {
    pub fn new(
        platform: Arc<dyn Platform>,
        tool_execution_governance: Arc<ToolExecutionGovernance>,
    ) -> Self {
        Self {
            platform,
            tool_execution_governance,
        }
    }
}

impl Tool for SystemControlTool {
    fn name(&self) -> &'static str {
        "system_control"
    }

    fn description(&self) -> &'static str {
        "System admin operations. Op: storage_usage, status, restart (confirm=true), tool_emergency_stop (confirm=true), or tool_emergency_resume (confirm=true)."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["storage_usage","status","restart","tool_emergency_stop","tool_emergency_resume"],"description":"Operation: storage_usage | status | restart | tool_emergency_stop | tool_emergency_resume"},"confirm":{"type":"boolean","description":"Must be true for restart and tool emergency-stop changes"},"reason":{"type":"string","description":"Optional operator reason for tool_emergency_stop"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
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
                crate::runtime::request_restart_with_continuity_flush(
                    Arc::clone(&self.platform),
                    ctx.current_chat_id(),
                    "tool_system_control_restart",
                );
                Ok(json!({
                    "op": "restart",
                    "ok": true,
                    "message": "restart requested"
                })
                .to_string())
            }
            "tool_emergency_stop" => {
                let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
                if !confirm {
                    return Ok(json!({
                        "op": "tool_emergency_stop",
                        "ok": false,
                        "error": "tool_emergency_stop requires confirm=true"
                    })
                    .to_string());
                }
                let reason = obj.get("reason").and_then(Value::as_str).unwrap_or("");
                let state = self
                    .tool_execution_governance
                    .set_emergency_stop(true, reason)?;
                Ok(json!({
                    "op": "tool_emergency_stop",
                    "ok": true,
                    "state": state,
                })
                .to_string())
            }
            "tool_emergency_resume" => {
                let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
                if !confirm {
                    return Ok(json!({
                        "op": "tool_emergency_resume",
                        "ok": false,
                        "error": "tool_emergency_resume requires confirm=true"
                    })
                    .to_string());
                }
                let state = self
                    .tool_execution_governance
                    .set_emergency_stop(false, "")?;
                Ok(json!({
                    "op": "tool_emergency_resume",
                    "ok": true,
                    "state": state,
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
            .with_effect_class(ToolEffectClass::SystemControl)
            .with_risk_level(ToolRiskLevel::Critical)
            .with_rollback_kind(ToolRollbackKind::Irreversible)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_system_control_governance")?;
        let op = obj.get("op").and_then(Value::as_str).unwrap_or("status");
        let confirm = obj.get("confirm").and_then(Value::as_bool).unwrap_or(false);
        Ok(match op {
            "storage_usage" | "status" => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_rollback_kind(ToolRollbackKind::None),
            "tool_emergency_stop" | "tool_emergency_resume" => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::SystemControl)
                .with_risk_level(ToolRiskLevel::High)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm),
            _ => self
                .metadata()
                .default_execution_shape(op)
                .with_effect_class(ToolEffectClass::SystemControl)
                .with_risk_level(ToolRiskLevel::Critical)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(confirm),
        })
    }
}
