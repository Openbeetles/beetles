//! cron_manage 工具：持久化定时任务管理（增删改查）。
//! cron_manage tool: persistent cron task management (CRUD).

use crate::constants::{CRON_TASKS_MAX_ENTRIES, CRON_TASK_MAX_ACTION_LEN};
use crate::error::{Error, Result};
use crate::memory::MemoryStore;
use crate::tools::cron::parse_cron_field;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

/// 持久化 cron 任务文件的相对路径。
const CRON_TASKS_REL_PATH: &str = "memory/cron_tasks.json";

/// 单条持久化 cron 任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronTask {
    pub id: String,
    pub expr: String,
    pub action: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
}

pub struct CronManageTool {
    store: Arc<dyn MemoryStore + Send + Sync>,
}

impl CronManageTool {
    pub fn new(store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        Self { store }
    }

    fn load_tasks(&self) -> Result<Vec<CronTask>> {
        let content = self.store.get_daily_note(CRON_TASKS_REL_PATH);
        match content {
            Ok(s) if !s.is_empty() => serde_json::from_str(&s)
                .map_err(|e| Error::config("tool_cron_manage", e.to_string())),
            _ => Ok(Vec::new()),
        }
    }

    fn save_tasks(&self, tasks: &[CronTask]) -> Result<()> {
        let data = serde_json::to_string_pretty(tasks)
            .map_err(|e| Error::config("tool_cron_manage", e.to_string()))?;
        self.store.write_daily_note(CRON_TASKS_REL_PATH, &data)?;
        crate::cron::mark_cron_persisted_tasks_dirty();
        Ok(())
    }
}

impl Tool for CronManageTool {
    fn name(&self) -> &'static str {
        "cron_manage"
    }
    fn description(&self) -> &'static str {
        "Manage persistent cron tasks. Op: add (create a scheduled task), list, remove (by id), update (toggle enabled or change expr/action). Max 16 tasks. Tasks are checked every 60s by the cron loop."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "description": "Operation: add|list|remove|update" },
                "id": { "type": "string", "description": "Task ID (for remove/update)" },
                "expr": { "type": "string", "description": "5-field cron expression (for add/update)" },
                "action": { "type": "string", "description": "Action text to inject as message when triggered (for add/update, max 512 bytes)" },
                "enabled": { "type": "boolean", "description": "Enable/disable task (for update, default true)" }
            },
            "required": ["op"]
        })
    }
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_cron_manage")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_cron_manage", "missing op"))?;

        match op {
            "add" => {
                let expr = obj
                    .get("expr")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_cron_manage", "missing expr"))?;
                let action = obj
                    .get("action")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_cron_manage", "missing action"))?;

                if action.len() > CRON_TASK_MAX_ACTION_LEN {
                    return Err(Error::config(
                        "tool_cron_manage",
                        format!("action exceeds {} bytes", CRON_TASK_MAX_ACTION_LEN),
                    ));
                }

                // Validate cron expression
                validate_cron_expr(expr)?;

                let mut tasks = self.load_tasks()?;
                if tasks.len() >= CRON_TASKS_MAX_ENTRIES {
                    return Err(Error::config(
                        "tool_cron_manage",
                        format!("max {} tasks reached", CRON_TASKS_MAX_ENTRIES),
                    ));
                }

                let channel = ctx.current_channel().unwrap_or("cron").to_string();
                let chat_id = ctx.current_chat_id().unwrap_or("cron").to_string();

                let id = format!("ct_{}", crate::util::current_unix_secs());
                let task = CronTask {
                    id: id.clone(),
                    expr: expr.to_string(),
                    action: action.to_string(),
                    channel,
                    chat_id,
                    enabled: true,
                };
                tasks.push(task);
                self.save_tasks(&tasks)?;

                Ok(json!({"op": "add", "id": id, "ok": true}).to_string())
            }
            "list" => {
                let tasks = self.load_tasks()?;
                let now = crate::util::current_unix_secs();
                let task_list: Vec<serde_json::Value> = tasks
                    .iter()
                    .map(|t| {
                        let next_trigger = if t.enabled {
                            crate::tools::cron::next_cron_trigger(&t.expr, now)
                                .ok()
                                .map(|(secs, iso)| json!({"unix_secs": secs, "iso": iso}))
                        } else {
                            None
                        };
                        json!({
                            "id": t.id,
                            "expr": t.expr,
                            "action": t.action,
                            "channel": t.channel,
                            "chat_id": t.chat_id,
                            "enabled": t.enabled,
                            "next_trigger": next_trigger
                        })
                    })
                    .collect();
                Ok(json!({"op": "list", "tasks": task_list, "count": task_list.len()}).to_string())
            }
            "remove" => {
                let id = obj
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_cron_manage", "missing id"))?;
                let mut tasks = self.load_tasks()?;
                let before_len = tasks.len();
                tasks.retain(|t| t.id != id);
                if tasks.len() == before_len {
                    return Ok(
                        json!({"op": "remove", "ok": false, "error": "task not found"}).to_string(),
                    );
                }
                self.save_tasks(&tasks)?;
                Ok(json!({"op": "remove", "id": id, "ok": true}).to_string())
            }
            "update" => {
                let id = obj
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_cron_manage", "missing id"))?;
                let mut tasks = self.load_tasks()?;
                let task = tasks
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(|| Error::config("tool_cron_manage", "task not found"))?;

                let mut updated = Vec::new();
                if let Some(expr) = obj.get("expr").and_then(|x| x.as_str()) {
                    validate_cron_expr(expr)?;
                    task.expr = expr.to_string();
                    updated.push("expr");
                }
                if let Some(action) = obj.get("action").and_then(|x| x.as_str()) {
                    if action.len() > CRON_TASK_MAX_ACTION_LEN {
                        return Err(Error::config(
                            "tool_cron_manage",
                            format!("action exceeds {} bytes", CRON_TASK_MAX_ACTION_LEN),
                        ));
                    }
                    task.action = action.to_string();
                    updated.push("action");
                }
                if let Some(enabled) = obj.get("enabled").and_then(|x| x.as_bool()) {
                    task.enabled = enabled;
                    updated.push("enabled");
                }

                if updated.is_empty() {
                    return Ok(json!({
                        "op": "update",
                        "ok": false,
                        "error": "no fields to update"
                    })
                    .to_string());
                }

                self.save_tasks(&tasks)?;
                Ok(json!({"op": "update", "id": id, "updated": updated, "ok": true}).to_string())
            }
            _ => Err(Error::config(
                "tool_cron_manage",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

/// Validate a 5-field cron expression by parsing each field.
fn validate_cron_expr(expr: &str) -> Result<()> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(Error::config(
            "tool_cron_manage",
            "expr must have exactly 5 fields: min hour dom month dow",
        ));
    }
    parse_cron_field(parts[0], 0, 59)?;
    parse_cron_field(parts[1], 0, 23)?;
    parse_cron_field(parts[2], 1, 31)?;
    parse_cron_field(parts[3], 1, 12)?;
    parse_cron_field(parts[4], 0, 6)?;
    Ok(())
}

/// Load persisted cron tasks from SPIFFS (for use by cron loop).
/// Returns empty vec on any error.
pub fn load_persisted_cron_tasks(store: &dyn MemoryStore) -> Vec<CronTask> {
    match store.get_daily_note(CRON_TASKS_REL_PATH) {
        Ok(s) if !s.is_empty() => serde_json::from_str(&s).unwrap_or_default(),
        _ => Vec::new(),
    }
}
