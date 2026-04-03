//! Search the archive sidecar over retained transcripts, daily notes, and turn logs.

use crate::error::{Error, Result};
use crate::memory::{
    search_archive_records, ArchiveRecordSource, ArchiveSearchQuery, MemoryStore, SessionStore,
    TurnLedgerStore, MAX_ARCHIVE_SEARCH_LIMIT,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct MemorySearchTool {
    session_store: Arc<dyn SessionStore + Send + Sync>,
    memory_store: Arc<dyn MemoryStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
}

impl MemorySearchTool {
    pub fn new(
        session_store: Arc<dyn SessionStore + Send + Sync>,
        memory_store: Arc<dyn MemoryStore + Send + Sync>,
        turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
    ) -> Self {
        Self {
            session_store,
            memory_store,
            turn_ledger_store,
        }
    }
}

impl Tool for MemorySearchTool {
    fn name(&self) -> &'static str {
        "memory_search"
    }

    fn description(&self) -> &'static str {
        "Search the archive sidecar across retained transcripts, daily notes, and turn logs. Returns citation-ready evidence hits with record_id and locator. These hits are archive evidence only, not canonical shared memory."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"query":{"type":"string","description":"What to search for in the archive sidecar."},"limit":{"type":"integer","description":"Max hits to return, default 4, max 8."},"chat_id":{"type":"string","description":"Optional chat_id filter. If omitted, search all retained chats and boost the current chat."},"sources":{"type":"array","description":"Optional source filter: transcript, daily_note, or turn_log.","items":{"type":"string","enum":["transcript","daily_note","turn_log"]}}},"required":["query"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_memory_search")?;
        let query = obj
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("tool_memory_search", "missing query"))?;
        let limit = obj
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(4)
            .clamp(1, MAX_ARCHIVE_SEARCH_LIMIT as u64) as usize;
        let chat_id = obj
            .get("chat_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let sources = parse_sources(obj.get("sources"))?;
        let hits = search_archive_records(
            self.session_store.as_ref(),
            self.memory_store.as_ref(),
            self.turn_ledger_store.as_ref(),
            ArchiveSearchQuery {
                query,
                preferred_chat_id: ctx.current_chat_id(),
                chat_id_filter: chat_id,
                sources: &sources,
                limit,
            },
        )?;
        Ok(json!({
            "ok": true,
            "op": "search",
            "query": query,
            "count": hits.len(),
            "hits": hits,
            "plane": "archive_evidence",
            "canonical": false,
            "traceability": "Each hit includes retrieval_trace with backend, matched_terms, score breakdown, and ranking/source/recency/selector reasons when available.",
            "usage_hint": "Use memory_get with record_id or locator to inspect one cited archive record before concluding."
        })
        .to_string())
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn parse_sources(value: Option<&Value>) -> Result<Vec<ArchiveRecordSource>> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let source = item
            .as_str()
            .ok_or_else(|| Error::config("tool_memory_search", "sources items must be strings"))?;
        let parsed = source.parse::<ArchiveRecordSource>().map_err(|_| {
            Error::config(
                "tool_memory_search",
                format!("unsupported archive source: {}", source),
            )
        })?;
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
    Ok(out)
}
