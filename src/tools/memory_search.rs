//! Search the archive sidecar over retained transcripts, daily notes, and turn logs.

use crate::error::Result;
use crate::memory::{
    search_archive_records_detailed, ArchiveRecordSource, ArchiveSearchQuery,
    ArchiveSearchQueryReport, MemoryStore, SessionStore, TurnLedgerStore, MAX_ARCHIVE_SEARCH_LIMIT,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolClarificationField, ToolClarificationOption,
    ToolContext, ToolExecutionBlocker, ToolExecutionOutcome, ToolMetadata,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct MemorySearchTool {
    session_store: Arc<dyn SessionStore + Send + Sync>,
    memory_store: Arc<dyn MemoryStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
}

#[derive(Serialize)]
struct MemorySearchResponse<'a> {
    ok: bool,
    op: &'static str,
    query: &'a str,
    count: usize,
    hits: Vec<crate::memory::ArchiveSearchHit>,
    query_report: ArchiveSearchQueryReport,
    plane: &'static str,
    canonical: bool,
    traceability: &'static str,
    usage_hint: &'static str,
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
        "Search the archive sidecar across retained transcripts, daily notes, and turn logs. Returns citation-ready evidence hits with record_id and locator. These hits are archive evidence, not canonical shared memory, but they can support grounded shareable conclusions after distillation."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"query":{"type":"string","description":"What to search for in the archive sidecar."},"limit":{"type":"integer","description":"Max hits to return, default 4, max 8."},"chat_id":{"type":"string","description":"Optional chat_id filter. If omitted, search all retained chats and boost the current chat."},"sources":{"type":"array","description":"Optional source filter: transcript, daily_note, or turn_log.","items":{"type":"string","enum":["transcript","daily_note","turn_log"]}}},"required":["query"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        self.execute_outcome(args, ctx)
            .map(|outcome| outcome.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_memory_search")?;
        let Some(query) = obj
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return missing_query_outcome();
        };
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
        let sources = match parse_sources_choice(obj.get("sources")) {
            Ok(sources) => sources,
            Err(outcome) => return Ok(*outcome),
        };
        let result = search_archive_records_detailed(
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
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "tool_memory_search",
            &MemorySearchResponse {
                ok: true,
                op: "search",
                query,
                count: result.hits.len(),
                hits: result.hits,
                query_report: result.report,
                plane: "archive_evidence",
                canonical: false,
                traceability: "Each hit includes retrieval_trace with backend, matched_terms, score breakdown, and ranking/source/recency/selector reasons when available.",
                usage_hint: "Use memory_get with record_id or locator to inspect one cited archive record before concluding. Distill a grounded stable conclusion separately; if an exact detail is still unsupported, say so plainly.",
            },
        )?))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn parse_sources_choice(
    value: Option<&Value>,
) -> std::result::Result<Vec<ArchiveRecordSource>, Box<ToolExecutionOutcome>> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(source) = item.as_str() else {
            return Err(Box::new(invalid_sources_outcome()));
        };
        let parsed = source
            .parse::<ArchiveRecordSource>()
            .map_err(|_| Box::new(invalid_sources_outcome()))?;
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
    Ok(out)
}

fn missing_query_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "search",
            "warning": "memory_search: missing query",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "A memory query is still required before memory_search can continue.",
        vec!["query".to_string()],
        vec![ToolClarificationField {
            key: "query".to_string(),
            label: "Search query".to_string(),
            description: "Describe what you want to search for in the archive evidence."
                .to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    )))
}

fn invalid_sources_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "search",
            "warning": "memory_search: sources must be transcript, daily_note, or turn_log",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "A supported archive source selection is still required before memory_search can continue.",
        vec!["sources".to_string()],
        vec![ToolClarificationField {
            key: "sources".to_string(),
            label: "Archive sources".to_string(),
            description: "Choose one or more archive sources to search.".to_string(),
            required: true,
            secret: false,
            multiple: true,
            options: vec![
                ToolClarificationOption {
                    value: "transcript".to_string(),
                    label: "transcript".to_string(),
                },
                ToolClarificationOption {
                    value: "daily_note".to_string(),
                    label: "daily_note".to_string(),
                },
                ToolClarificationOption {
                    value: "turn_log".to_string(),
                    label: "turn_log".to_string(),
                },
            ],
        }],
    ))
}

#[cfg(test)]
mod tests {
    use super::MemorySearchTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::memory::{
        MemoryStore, SessionMessage, SessionMessageRecord, SessionStore, TurnLedger,
        TurnLedgerStore,
    };
    use crate::platform::ResponseBody;
    use crate::tools::{Tool, ToolContext, ToolExecutionBlockerKind};
    use std::sync::Arc;

    struct EmptySessionStore;

    impl SessionStore for EmptySessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }
        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }
        fn load_recent_records(
            &self,
            _chat_id: &str,
            _n: usize,
        ) -> Result<Vec<SessionMessageRecord>> {
            Ok(Vec::new())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    struct EmptyMemoryStore;

    impl MemoryStore for EmptyMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
        fn get_daily_note(&self, _name: &str) -> Result<String> {
            Ok(String::new())
        }
        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    struct EmptyTurnLedgerStore;

    impl TurnLedgerStore for EmptyTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }
        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }
        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    fn build_tool() -> MemorySearchTool {
        MemorySearchTool::new(
            Arc::new(EmptySessionStore),
            Arc::new(EmptyMemoryStore),
            Arc::new(EmptyTurnLedgerStore),
        )
    }

    #[test]
    fn memory_search_missing_query_returns_facts_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{}"#, &mut ctx)
            .expect("missing query should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker.missing_fields.iter().any(|item| item == "query"));
    }

    #[test]
    fn memory_search_unsupported_source_returns_choice_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{"query":"alice","sources":["foo"]}"#, &mut ctx)
            .expect("unsupported source should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }
}
