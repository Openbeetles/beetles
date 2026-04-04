//! session_manage 工具：管理会话（列表、清空、删除、信息）。
//! session_manage tool: manage sessions (list, clear, delete, info).

use crate::error::{Error, Result};
use crate::memory::SessionStore;
use crate::tools::{Tool, ToolContext, ToolMetadata, parse_tool_args};
use serde_json::json;
use std::sync::Arc;

pub struct SessionManageTool {
    store: Arc<dyn SessionStore + Send + Sync>,
}

impl SessionManageTool {
    pub fn new(store: Arc<dyn SessionStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl Tool for SessionManageTool {
    fn name(&self) -> &'static str {
        "session_manage"
    }
    fn description(&self) -> &'static str {
        "Manage chat sessions. Op: list (all session IDs), info (load recent messages for a chat_id), clear (clear a session), delete (delete a session)."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: list|info|clear|delete"},"chat_id":{"type":"string","description":"Chat ID (required for info/clear/delete)"},"recent_n":{"type":"integer","description":"Number of recent messages for info (default 10, max 50)"}},"required":["op"]}"#
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_session_manage")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_session_manage", "missing op"))?;

        match op {
            "list" => {
                let ids = self.store.list_chat_ids()?;
                Ok(json!({"op": "list", "chat_ids": ids, "count": ids.len()}).to_string())
            }
            "info" => {
                let chat_id = obj
                    .get("chat_id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_session_manage", "missing chat_id"))?;
                let recent_n = obj.get("recent_n").and_then(|x| x.as_u64()).unwrap_or(10) as usize;
                let recent_n = recent_n.min(50);
                let messages = self.store.load_recent(chat_id, recent_n)?;
                let msg_list: Vec<serde_json::Value> = messages
                    .iter()
                    .map(|m| json!({"role": m.role, "content": m.content}))
                    .collect();
                Ok(json!({"op": "info", "chat_id": chat_id, "messages": msg_list, "count": msg_list.len()}).to_string())
            }
            "clear" => {
                let chat_id = obj
                    .get("chat_id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_session_manage", "missing chat_id"))?;
                self.store.clear(chat_id)?;
                Ok(json!({"op": "clear", "chat_id": chat_id, "ok": true}).to_string())
            }
            "delete" => {
                let chat_id = obj
                    .get("chat_id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_session_manage", "missing chat_id"))?;
                self.store.delete(chat_id)?;
                Ok(json!({"op": "delete", "chat_id": chat_id, "ok": true}).to_string())
            }
            _ => Err(Error::config(
                "tool_session_manage",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}
