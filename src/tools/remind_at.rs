//! remind_at 工具：按 at（ISO8601 或 Unix 秒）与 context 写入 RemindAtStore；到点由主循环外线程 pop_due 并注入 PcMsg。

use crate::error::{Error, Result};
use crate::i18n::{tr, Message as UiMessage};
use crate::memory::RemindAtStore;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::util::parse_iso8601;
use serde_json::{json, Value};

/// 需要注入 RemindAtStore；由 main 注册时传入。
pub struct RemindAtTool {
    store: std::sync::Arc<dyn RemindAtStore + Send + Sync>,
}

impl RemindAtTool {
    pub fn new(store: std::sync::Arc<dyn RemindAtStore + Send + Sync>) -> Self {
        Self { store }
    }
}

/// 将 at 解析为 Unix 秒：数字直接使用；字符串先尝试 u64，否则按 ISO8601 简式解析（YYYY-MM-DDTHH:MM:SS 或带 Z）。
fn parse_at_to_unix_secs(v: &Value) -> Result<u64> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| Error::config("remind_at", "at must be non-negative number")),
        Value::String(s) => parse_iso8601(s)
            .ok_or_else(|| Error::config("remind_at", "at must be Unix seconds or ISO8601 string")),
        _ => Err(Error::config("remind_at", "at must be number or string")),
    }
}

impl Tool for RemindAtTool {
    fn name(&self) -> &'static str {
        "remind_at"
    }
    fn description(&self) -> &'static str {
        "Schedule a reminder. Args: at (ISO8601 string or Unix seconds), context (string). At the given time the user will receive a message with the context."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"at":{"description":"ISO8601 (e.g. 2025-03-10T12:00:00Z) or Unix seconds","oneOf":[{"type":"number"},{"type":"string"}]},"context":{"description":"Reminder text to show at that time","type":"string"}},"required":["at","context"]}"#
    }
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let chat_id = ctx.current_chat_id().ok_or_else(|| {
            Error::config(
                "remind_at",
                "no current chat_id (tool used outside session)",
            )
        })?;
        let channel = ctx.current_channel().ok_or_else(|| {
            Error::config(
                "remind_at",
                "no current channel (tool used outside session)",
            )
        })?;
        let obj = parse_tool_args(args, "remind_at")?;
        let at_val = obj
            .get("at")
            .ok_or_else(|| Error::config("remind_at", "missing at"))?;
        let context = obj.get("context").and_then(Value::as_str).unwrap_or("");
        let at_secs = parse_at_to_unix_secs(at_val)?;
        self.store.add(channel, chat_id, at_secs, context)?;
        Ok(tr(UiMessage::RemindAtSetOk, ctx.user_locale()))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

/// remind_list 工具：查询当前会话未到点提醒。
pub struct RemindListTool {
    store: std::sync::Arc<dyn RemindAtStore + Send + Sync>,
}

impl RemindListTool {
    pub fn new(store: std::sync::Arc<dyn RemindAtStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl Tool for RemindListTool {
    fn name(&self) -> &'static str {
        "remind_list"
    }

    fn description(&self) -> &'static str {
        "List upcoming reminders for current chat. Args: limit (optional, default 10, max 20)."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"limit":{"type":"number","description":"max items to return, default 10, max 20"}}}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let chat_id = ctx.current_chat_id().ok_or_else(|| {
            Error::config(
                "remind_list",
                "no current chat_id (tool used outside session)",
            )
        })?;
        let channel = ctx.current_channel().ok_or_else(|| {
            Error::config(
                "remind_list",
                "no current channel (tool used outside session)",
            )
        })?;
        let obj = parse_tool_args(args, "remind_list")?;
        let limit = obj
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 20) as usize;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let items = self.store.list_upcoming(channel, chat_id, now, limit)?;
        let entries: Vec<serde_json::Value> = items
            .into_iter()
            .map(|(at_unix_secs, context)| {
                json!({
                    "at_unix_secs": at_unix_secs,
                    "context": context
                })
            })
            .collect();
        Ok(json!({
            "count": entries.len(),
            "items": entries
        })
        .to_string())
    }
}
