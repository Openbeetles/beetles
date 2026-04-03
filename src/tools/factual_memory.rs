//! Read-only exact/query access to the canonical shared factual plane.

use crate::error::{Error, Result};
use crate::memory::{
    long_term_memory_evidence_summary, lookup_long_term_memory_slot,
    parse_explicit_long_term_slot_query, LongTermMemoryEntry, LongTermMemoryFreshness,
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
        "Read the canonical shared factual plane by exact slot or structured filters. Use this for stable shared facts, profiles, constraints, tasks, or projects. Exact lookup supports slot_query syntax and returns evidence posture, provenance, and nearby canonical candidates when the slot misses."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"lookup_exact|lookup_slot|query"},"slot_query":{"type":"string","description":"Exact slot syntax such as project:current_project, slot relationship:owner_relation, or profile.user_name."},"kind":{"type":"string","description":"preference|profile|relationship|project|task|constraint|fact"},"topic":{"type":"string","description":"Stable slot topic key for exact lookup or query filter."},"source_scope":{"type":"string","description":"chat|user|world"},"source_chat_id":{"type":"string","description":"Optional source chat filter for query."},"freshness":{"type":"string","description":"stable|dynamic|volatile"},"include_stale":{"type":"boolean","description":"Whether stale records may be returned. Default false."},"limit":{"type":"integer","description":"Max records for query, default 4."}},"required":["op"]}"#
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
            "lookup_exact" | "lookup_slot" => {
                let slot_query = obj
                    .get("slot_query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let slot = if let Some(slot_query) = slot_query {
                    parse_explicit_long_term_slot_query(slot_query)
                        .ok_or_else(|| Error::config("tool_factual_memory", "invalid slot_query"))?
                } else {
                    let kind =
                        parse_kind(obj.get("kind").and_then(Value::as_str).ok_or_else(|| {
                            Error::config("tool_factual_memory", "missing kind or slot_query")
                        })?)?;
                    let topic = obj.get("topic").and_then(Value::as_str).ok_or_else(|| {
                        Error::config("tool_factual_memory", "missing topic or slot_query")
                    })?;
                    LongTermMemorySlot {
                        kind,
                        topic: topic.to_string(),
                    }
                    .normalized()
                    .ok_or_else(|| Error::config("tool_factual_memory", "invalid slot"))?
                };
                let lookup = lookup_long_term_memory_slot(self.long_term_store.as_ref(), &slot, 4)?;
                Ok(json!({
                    "ok": true,
                    "op": op,
                    "slot_query": slot_query,
                    "slot": lookup.slot,
                    "status": if lookup.entry.is_some() { "exact_match" } else { "not_found" },
                    "exact_match": lookup.entry.is_some(),
                    "item": lookup.entry.as_ref().map(render_entry),
                    "nearby_candidates": lookup
                        .nearby_candidates
                        .iter()
                        .map(|entry| render_candidate(entry, &lookup.slot))
                        .collect::<Vec<_>>(),
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
        "slot": {
            "kind": entry.kind.label(),
            "topic": entry.topic.as_str(),
        },
        "content": entry.content.as_str(),
        "keywords": &entry.keywords,
        "evidence": long_term_memory_evidence_summary(entry, now_secs),
        "provenance": render_provenance(entry),
        "entry": entry,
    })
}

fn render_candidate(entry: &LongTermMemoryEntry, requested_slot: &LongTermMemorySlot) -> Value {
    json!({
        "slot": {
            "kind": entry.kind.label(),
            "topic": entry.topic.as_str(),
        },
        "content": entry.content.as_str(),
        "match_reason": candidate_match_reason(entry, requested_slot),
        "evidence": long_term_memory_evidence_summary(entry, crate::util::current_unix_secs()),
        "provenance": render_provenance(entry),
    })
}

fn render_provenance(entry: &LongTermMemoryEntry) -> Value {
    json!({
        "source_type": entry.source_type.label(),
        "source_scope": entry.source_scope.label(),
        "source_chat_id": entry.source_chat_id.as_deref(),
        "source_revision": entry.source_revision,
        "evidence_count": entry.evidence_count,
        "supporting_citations": &entry.supporting_citations,
        "last_confirmed_at": entry.last_confirmed_at,
        "last_used_at": entry.last_used_at,
    })
}

fn candidate_match_reason(
    entry: &LongTermMemoryEntry,
    requested_slot: &LongTermMemorySlot,
) -> String {
    if entry.kind == requested_slot.kind && entry.topic == requested_slot.topic {
        return "same canonical slot".to_string();
    }
    if entry.kind == requested_slot.kind {
        return "same kind and nearby topic".to_string();
    }
    "nearby topic across another canonical kind".to_string()
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
