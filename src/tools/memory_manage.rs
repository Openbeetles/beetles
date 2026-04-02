//! memory_manage 工具：管理长期记忆、灵魂设定、用户配置与每日笔记。
//! memory_manage tool: manage long-term memory, soul, user config, and daily notes.

use crate::error::{Error, Result};
use crate::memory::{
    LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryFreshness, LongTermMemoryKind,
    LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, LongTermMemoryStore, MemoryStore, MAX_MEMORY_CONTENT_LEN,
    MAX_SOUL_USER_LEN,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::json;
use std::sync::Arc;

pub struct MemoryManageTool {
    store: Arc<dyn MemoryStore + Send + Sync>,
    long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
}

impl MemoryManageTool {
    pub fn new(
        store: Arc<dyn MemoryStore + Send + Sync>,
        long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
    ) -> Self {
        Self {
            store,
            long_term_store,
        }
    }
}

impl Tool for MemoryManageTool {
    fn name(&self) -> &'static str {
        "memory_manage"
    }
    fn description(&self) -> &'static str {
        "Manage persistent memory, structured long-term memory, soul/user config, and daily notes. Op: get_memory, set_memory, get_soul, set_soul, get_user, set_user, list_daily_notes, get_daily_note, write_daily_note, list_long_term, get_long_term, upsert_long_term, delete_long_term, delete_long_term_slot."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "description": "Operation: get_memory|set_memory|get_soul|set_soul|get_user|set_user|list_daily_notes|get_daily_note|write_daily_note|list_long_term|get_long_term|upsert_long_term|delete_long_term|delete_long_term_slot" },
                "content": { "type": "string", "description": "Content for set_memory/set_soul/set_user/write_daily_note" },
                "name": { "type": "string", "description": "Daily note name (e.g. 2025-03-10.md) for get_daily_note/write_daily_note" },
                "recent_n": { "type": "integer", "description": "Max number of daily notes to list (default 10, max 30)" },
                "append": { "type": "boolean", "description": "If true, append to existing note instead of overwrite (default false, for write_daily_note)" },
                "id": { "type": "string", "description": "Structured long-term memory id for get_long_term/delete_long_term" },
                "topic": { "type": "string", "description": "Structured long-term memory stable topic key, e.g. response_style or current_project" },
                "kind": { "type": "string", "description": "Structured long-term memory kind: preference|profile|relationship|project|task|constraint|fact" },
                "keywords": { "type": "array", "items": { "type": "string" }, "description": "Structured long-term memory keywords" },
                "source_type": { "type": "string", "description": "Structured long-term memory source type: conversation|manual_tool|system_runtime|external_observation" },
                "source_scope": { "type": "string", "description": "Structured long-term memory source scope: chat|user|world" },
                "confidence": { "type": "string", "description": "Structured long-term memory confidence: low|medium|high" },
                "freshness": { "type": "string", "description": "Structured long-term memory freshness: stable|dynamic|volatile" },
                "stale_hint": { "type": "string", "description": "Structured long-term memory stale hint: none|review_before_use|verify_against_current_state" }
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
            "list_long_term" => {
                let limit = obj.get("recent_n").and_then(|x| x.as_u64()).unwrap_or(20) as usize;
                let items = self.long_term_store.list(limit)?;
                Ok(json!({"op": "list_long_term", "items": items}).to_string())
            }
            "get_long_term" => {
                let id = obj
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing id"))?;
                let item = self.long_term_store.get(id)?;
                Ok(json!({"op": "get_long_term", "item": item}).to_string())
            }
            "upsert_long_term" => {
                let kind = obj
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing kind"))?;
                let kind = parse_long_term_kind(kind)?;
                let topic = obj
                    .get("topic")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing topic"))?;
                let content = obj
                    .get("content")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing content"))?;
                let keywords = obj
                    .get("keywords")
                    .and_then(|x| x.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let source_type = obj
                    .get("source_type")
                    .and_then(|x| x.as_str())
                    .map(parse_long_term_source_type)
                    .transpose()?;
                let source_scope = obj
                    .get("source_scope")
                    .and_then(|x| x.as_str())
                    .map(parse_long_term_source_scope)
                    .transpose()?;
                let confidence = obj
                    .get("confidence")
                    .and_then(|x| x.as_str())
                    .map(parse_long_term_confidence)
                    .transpose()?;
                let freshness = obj
                    .get("freshness")
                    .and_then(|x| x.as_str())
                    .map(parse_long_term_freshness)
                    .transpose()?;
                let stale_hint = obj
                    .get("stale_hint")
                    .and_then(|x| x.as_str())
                    .map(parse_long_term_stale_hint)
                    .transpose()?;
                let draft = LongTermMemoryDraft {
                    kind,
                    topic: topic.to_string(),
                    content: content.to_string(),
                    keywords,
                    source_chat_id: None,
                    source_type: Some(source_type.unwrap_or(LongTermMemorySourceType::ManualTool)),
                    source_scope,
                    confidence,
                    freshness,
                    stale_hint,
                    observed_at: None,
                    source_revision: None,
                };
                let changed_count = self
                    .long_term_store
                    .upsert_many(&[draft], crate::util::current_unix_secs())?;
                Ok(
                    json!({"op": "upsert_long_term", "ok": true, "changed_count": changed_count})
                        .to_string(),
                )
            }
            "delete_long_term" => {
                let id = obj
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing id"))?;
                let deleted = self.long_term_store.delete(id)?;
                Ok(json!({"op": "delete_long_term", "deleted": deleted}).to_string())
            }
            "delete_long_term_slot" => {
                let kind = obj
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing kind"))?;
                let kind = parse_long_term_kind(kind)?;
                let topic = obj
                    .get("topic")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| Error::config("tool_memory_manage", "missing topic"))?;
                let deleted = self.long_term_store.delete_slot(&LongTermMemorySlot {
                    kind,
                    topic: topic.to_string(),
                })?;
                Ok(json!({"op": "delete_long_term_slot", "deleted": deleted}).to_string())
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

fn parse_long_term_kind(kind: &str) -> Result<LongTermMemoryKind> {
    match kind.trim().to_ascii_lowercase().as_str() {
        "preference" => Ok(LongTermMemoryKind::Preference),
        "profile" => Ok(LongTermMemoryKind::Profile),
        "relationship" => Ok(LongTermMemoryKind::Relationship),
        "project" => Ok(LongTermMemoryKind::Project),
        "task" => Ok(LongTermMemoryKind::Task),
        "constraint" => Ok(LongTermMemoryKind::Constraint),
        "fact" => Ok(LongTermMemoryKind::Fact),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid kind: expected preference|profile|relationship|project|task|constraint|fact",
        )),
    }
}

fn parse_long_term_source_type(raw: &str) -> Result<LongTermMemorySourceType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "conversation" => Ok(LongTermMemorySourceType::Conversation),
        "manual_tool" => Ok(LongTermMemorySourceType::ManualTool),
        "system_runtime" => Ok(LongTermMemorySourceType::SystemRuntime),
        "external_observation" => Ok(LongTermMemorySourceType::ExternalObservation),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid source_type: expected conversation|manual_tool|system_runtime|external_observation",
        )),
    }
}

fn parse_long_term_source_scope(raw: &str) -> Result<LongTermMemorySourceScope> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok(LongTermMemorySourceScope::Chat),
        "user" => Ok(LongTermMemorySourceScope::User),
        "world" => Ok(LongTermMemorySourceScope::World),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid source_scope: expected chat|user|world",
        )),
    }
}

fn parse_long_term_confidence(raw: &str) -> Result<LongTermMemoryConfidence> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "low" => Ok(LongTermMemoryConfidence::Low),
        "medium" => Ok(LongTermMemoryConfidence::Medium),
        "high" => Ok(LongTermMemoryConfidence::High),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid confidence: expected low|medium|high",
        )),
    }
}

fn parse_long_term_freshness(raw: &str) -> Result<LongTermMemoryFreshness> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "stable" => Ok(LongTermMemoryFreshness::Stable),
        "dynamic" => Ok(LongTermMemoryFreshness::Dynamic),
        "volatile" => Ok(LongTermMemoryFreshness::Volatile),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid freshness: expected stable|dynamic|volatile",
        )),
    }
}

fn parse_long_term_stale_hint(raw: &str) -> Result<LongTermMemoryStaleHint> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(LongTermMemoryStaleHint::None),
        "review_before_use" => Ok(LongTermMemoryStaleHint::ReviewBeforeUse),
        "verify_against_current_state" => Ok(LongTermMemoryStaleHint::VerifyAgainstCurrentState),
        _ => Err(Error::config(
            "tool_memory_manage",
            "invalid stale_hint: expected none|review_before_use|verify_against_current_state",
        )),
    }
}
