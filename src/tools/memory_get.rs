//! Read one cited archive record from the searchable archive sidecar.

use crate::error::{Error, Result};
use crate::memory::{
    archive_get_default_content_len, get_archive_record, ArchiveRecordLocator, ArchiveRecordSource,
    MemoryStore, SessionStore, TurnLedgerStore, MAX_ARCHIVE_GET_CONTENT_LEN,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct MemoryGetTool {
    session_store: Arc<dyn SessionStore + Send + Sync>,
    memory_store: Arc<dyn MemoryStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
}

impl MemoryGetTool {
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

impl Tool for MemoryGetTool {
    fn name(&self) -> &'static str {
        "memory_get"
    }

    fn description(&self) -> &'static str {
        "Fetch one cited archive-sidecar record by record_id or locator fields. Use this after memory_search to inspect a specific transcript message, daily note, or turn log. Returned content is archive evidence, not canonical shared memory."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"record_id":{"type":"string","description":"Opaque archive record id returned by memory_search."},"source":{"type":"string","enum":["transcript","daily_note","turn_log"],"description":"Source type when not using record_id."},"chat_id":{"type":"string","description":"Chat id for transcript or turn_log locators."},"message_index":{"type":"integer","description":"Transcript message index returned by memory_search."},"note_name":{"type":"string","description":"Daily note file name, e.g. 2026-04-02.md."},"req_id":{"type":"string","description":"Turn log request id returned by memory_search."},"focus_query":{"type":"string","description":"Optional focus term for excerpting the returned record."},"max_chars":{"type":"integer","description":"Max content chars to return, default 1800, max 4096."}}}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_memory_get")?;
        let locator = parse_locator(&obj)?;
        let focus_query = obj
            .get("focus_query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let max_chars = obj
            .get("max_chars")
            .and_then(Value::as_u64)
            .unwrap_or(archive_get_default_content_len() as u64)
            .clamp(256, MAX_ARCHIVE_GET_CONTENT_LEN as u64) as usize;
        let record = get_archive_record(
            self.session_store.as_ref(),
            self.memory_store.as_ref(),
            self.turn_ledger_store.as_ref(),
            &locator,
            focus_query,
            max_chars,
        )?;
        Ok(json!({
            "ok": record.is_some(),
            "found": record.is_some(),
            "op": "get",
            "locator": locator,
            "record": record,
            "plane": "archive_evidence",
            "canonical": false,
            "usage_hint": "Treat the returned record as evidence only. Distill stable conclusions separately; do not equate archive records with canonical shared memory."
        })
        .to_string())
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn parse_locator(obj: &serde_json::Map<String, Value>) -> Result<ArchiveRecordLocator> {
    if let Some(record_id) = obj
        .get("record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return ArchiveRecordLocator::parse_record_id(record_id).ok_or_else(|| {
            Error::config(
                "tool_memory_get",
                format!("invalid archive record_id: {}", record_id),
            )
        });
    }

    let source = obj
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::config("tool_memory_get", "missing source or record_id"))?;
    let source = source
        .parse::<ArchiveRecordSource>()
        .map_err(|_| Error::config("tool_memory_get", "unsupported source"))?;
    let chat_id = obj
        .get("chat_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let message_index = obj
        .get("message_index")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    let note_name = obj
        .get("note_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let req_id = obj
        .get("req_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let valid = match source {
        ArchiveRecordSource::Transcript => chat_id.is_some() && message_index.is_some(),
        ArchiveRecordSource::DailyNote => note_name.is_some(),
        ArchiveRecordSource::TurnLog => chat_id.is_some(),
    };
    if !valid {
        return Err(Error::config(
            "tool_memory_get",
            "locator fields do not match the requested source",
        ));
    }

    Ok(ArchiveRecordLocator {
        source,
        chat_id,
        message_index,
        note_name,
        req_id,
    })
}
