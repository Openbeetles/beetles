//! memory_manage 工具：管理长期记忆、灵魂设定、用户配置与每日笔记。
//! memory_manage tool: manage long-term memory, soul, user config, and daily notes.

use crate::error::{Error, Result};
use crate::memory::{MemoryStore, MAX_MEMORY_CONTENT_LEN, MAX_SOUL_USER_LEN};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::json;
use std::sync::Arc;

pub struct MemoryManageTool {
    store: Arc<dyn MemoryStore + Send + Sync>,
}

impl MemoryManageTool {
    pub fn new(store: Arc<dyn MemoryStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl Tool for MemoryManageTool {
    fn name(&self) -> &'static str {
        "memory_manage"
    }
    fn description(&self) -> &'static str {
        "Manage persistent memory, soul/user config, and daily notes. Op: get_memory, set_memory, get_soul, set_soul, get_user, set_user, list_daily_notes, get_daily_note, write_daily_note."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "description": "Operation: get_memory|set_memory|get_soul|set_soul|get_user|set_user|list_daily_notes|get_daily_note|write_daily_note" },
                "content": { "type": "string", "description": "Content for set_memory/set_soul/set_user/write_daily_note" },
                "name": { "type": "string", "description": "Daily note name (e.g. 2025-03-10.md) for get_daily_note/write_daily_note" },
                "recent_n": { "type": "integer", "description": "Max number of daily notes to list (default 10, max 30)" },
                "append": { "type": "boolean", "description": "If true, append to existing note instead of overwrite (default false, for write_daily_note)" }
            },
            "required": ["op"]
        })
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_memory_manage")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_memory_manage", "missing op"))?;

        match op {
            "get_memory" => {
                let content = self.store.get_memory()?;
                Ok(json!({"op": "get_memory", "content": content}).to_string())
            }
            "set_memory" => {
                let content = obj
                    .get("content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing content"))?;
                if content.len() > MAX_MEMORY_CONTENT_LEN {
                    return Err(Error::config(
                        "tool_memory_manage",
                        format!("content exceeds {} bytes", MAX_MEMORY_CONTENT_LEN),
                    ));
                }
                self.store.set_memory(content)?;
                Ok(json!({"op": "set_memory", "ok": true}).to_string())
            }
            "get_soul" => {
                let content = self.store.get_soul()?;
                Ok(json!({"op": "get_soul", "content": content}).to_string())
            }
            "set_soul" => {
                let content = obj
                    .get("content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing content"))?;
                if content.len() > MAX_SOUL_USER_LEN {
                    return Err(Error::config(
                        "tool_memory_manage",
                        format!("content exceeds {} bytes", MAX_SOUL_USER_LEN),
                    ));
                }
                self.store.set_soul(content)?;
                Ok(json!({"op": "set_soul", "ok": true}).to_string())
            }
            "get_user" => {
                let content = self.store.get_user()?;
                Ok(json!({"op": "get_user", "content": content}).to_string())
            }
            "set_user" => {
                let content = obj
                    .get("content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing content"))?;
                if content.len() > MAX_SOUL_USER_LEN {
                    return Err(Error::config(
                        "tool_memory_manage",
                        format!("content exceeds {} bytes", MAX_SOUL_USER_LEN),
                    ));
                }
                self.store.set_user(content)?;
                Ok(json!({"op": "set_user", "ok": true}).to_string())
            }
            "list_daily_notes" => {
                let recent_n = obj.get("recent_n").and_then(|x| x.as_u64()).unwrap_or(10) as usize;
                let recent_n = recent_n.min(crate::constants::DAILY_NOTE_MAX_LIST);
                let names = self.store.list_daily_note_names(recent_n)?;
                Ok(json!({"op": "list_daily_notes", "names": names}).to_string())
            }
            "get_daily_note" => {
                let name = obj
                    .get("name")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing name"))?;
                validate_daily_note_name(name)?;
                let content = self.store.get_daily_note(name)?;
                Ok(json!({"op": "get_daily_note", "name": name, "content": content}).to_string())
            }
            "write_daily_note" => {
                let name = obj
                    .get("name")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing name"))?;
                validate_daily_note_name(name)?;
                let content = obj
                    .get("content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing content"))?;
                let append = obj.get("append").and_then(|x| x.as_bool()).unwrap_or(false);
                let final_content = if append {
                    let existing = self.store.get_daily_note(name).unwrap_or_default();
                    if existing.is_empty() {
                        content.to_string()
                    } else {
                        format!("{}\n{}", existing, content)
                    }
                } else {
                    content.to_string()
                };
                self.store.write_daily_note(name, &final_content)?;
                Ok(
                    json!({"op": "write_daily_note", "name": name, "ok": true, "append": append})
                        .to_string(),
                )
            }
            _ => Err(Error::config(
                "tool_memory_manage",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

fn validate_daily_note_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(Error::config("tool_memory_manage", "invalid name length"));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
    {
        return Err(Error::config(
            "tool_memory_manage",
            "name must match [a-zA-Z0-9_\\-.]",
        ));
    }
    Ok(())
}
