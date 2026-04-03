//! Read-only exact/query access to the canonical shared factual plane.

use crate::error::{Error, Result};
use crate::memory::{
    long_term_memory_evidence_summary, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemoryQuery, LongTermMemorySlot, LongTermMemorySourceScope,
    LongTermMemoryStore,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct FactualMemoryTool {
    long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
}

impl FactualMemoryTool {
    pub fn new(long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>) -> Self {
        Self { long_term_store }
    }
}

impl Tool for FactualMemoryTool {
    fn name(&self) -> &'static str {
        "factual_memory"
    }

    fn description(&self) -> &'static str {
        "Read the canonical shared factual plane by exact slot or structured filters. Use this for stable shared facts, profiles, constraints, tasks, or projects. Returned records include evidence posture and provenance, unlike archive-only memory_search/memory_get."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"lookup_exact|query"},"kind":{"type":"string","description":"preference|profile|relationship|project|task|constraint|fact"},"topic":{"type":"string","description":"Stable slot topic key for exact lookup or query filter."},"source_scope":{"type":"string","description":"chat|user|world"},"source_chat_id":{"type":"string","description":"Optional source chat filter for query."},"freshness":{"type":"string","description":"stable|dynamic|volatile"},"include_stale":{"type":"boolean","description":"Whether stale records may be returned. Default false."},"limit":{"type":"integer","description":"Max records for query, default 4."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_factual_memory")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("tool_factual_memory", "missing op"))?;
        match op {
            "lookup_exact" => {
                let kind = parse_kind(
                    obj.get("kind")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::config("tool_factual_memory", "missing kind"))?,
                )?;
                let topic = obj
                    .get("topic")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_factual_memory", "missing topic"))?;
                let slot = LongTermMemorySlot {
                    kind,
                    topic: topic.to_string(),
                };
                let item = self.long_term_store.get_slot(&slot)?;
                Ok(json!({
                    "ok": true,
                    "op": "lookup_exact",
                    "slot": slot,
                    "item": item.as_ref().map(|entry| render_entry(entry)),
                    "current_chat_id": ctx.current_chat_id(),
                    "plane": "canonical_shared_factual",
                    "canonical": true,
                })
                .to_string())
            }
            "query" => {
                let limit = obj.get("limit").and_then(Value::as_u64).unwrap_or(4) as usize;
                let query = LongTermMemoryQuery {
                    kind: obj
                        .get("kind")
                        .and_then(Value::as_str)
                        .map(parse_kind)
                        .transpose()?,
                    topic: obj.get("topic").and_then(Value::as_str).map(str::to_string),
                    source_scope: obj
                        .get("source_scope")
                        .and_then(Value::as_str)
                        .map(parse_source_scope)
                        .transpose()?,
                    source_chat_id: obj
                        .get("source_chat_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    freshness: obj
                        .get("freshness")
                        .and_then(Value::as_str)
                        .map(parse_freshness)
                        .transpose()?,
                    include_stale: obj
                        .get("include_stale")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    limit,
                };
                let items = self.long_term_store.query(&query)?;
                Ok(json!({
                    "ok": true,
                    "op": "query",
                    "query": query.normalized(),
                    "count": items.len(),
                    "items": items.iter().map(render_entry).collect::<Vec<_>>(),
                    "plane": "canonical_shared_factual",
                    "canonical": true,
                })
                .to_string())
            }
            _ => Err(Error::config(
                "tool_factual_memory",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn render_entry(entry: &LongTermMemoryEntry) -> serde_json::Value {
    let now_secs = crate::util::current_unix_secs();
    json!({
        "entry": entry,
        "evidence": long_term_memory_evidence_summary(entry, now_secs),
    })
}

fn parse_kind(raw: &str) -> Result<LongTermMemoryKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "preference" => Ok(LongTermMemoryKind::Preference),
        "profile" => Ok(LongTermMemoryKind::Profile),
        "relationship" => Ok(LongTermMemoryKind::Relationship),
        "project" => Ok(LongTermMemoryKind::Project),
        "task" => Ok(LongTermMemoryKind::Task),
        "constraint" => Ok(LongTermMemoryKind::Constraint),
        "fact" => Ok(LongTermMemoryKind::Fact),
        _ => Err(Error::config("tool_factual_memory", "invalid kind")),
    }
}

fn parse_source_scope(raw: &str) -> Result<LongTermMemorySourceScope> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok(LongTermMemorySourceScope::Chat),
        "user" => Ok(LongTermMemorySourceScope::User),
        "world" => Ok(LongTermMemorySourceScope::World),
        _ => Err(Error::config("tool_factual_memory", "invalid source_scope")),
    }
}

fn parse_freshness(raw: &str) -> Result<LongTermMemoryFreshness> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "stable" => Ok(LongTermMemoryFreshness::Stable),
        "dynamic" => Ok(LongTermMemoryFreshness::Dynamic),
        "volatile" => Ok(LongTermMemoryFreshness::Volatile),
        _ => Err(Error::config("tool_factual_memory", "invalid freshness")),
    }
}
