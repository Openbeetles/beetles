//! Read one cited archive record from the searchable archive sidecar.

use crate::error::Result;
use crate::memory::{
    archive_get_default_content_len, get_archive_record, ArchiveRecordLocator, ArchiveRecordSource,
    MemoryStore, SessionStore, TurnLedgerStore, MAX_ARCHIVE_GET_CONTENT_LEN,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolClarificationField, ToolClarificationOption,
    ToolContext, ToolExecutionBlocker, ToolExecutionOutcome, ToolMetadata,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct MemoryGetTool {
    session_store: Arc<dyn SessionStore + Send + Sync>,
    memory_store: Arc<dyn MemoryStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
}

#[derive(Serialize)]
struct MemoryGetResponse {
    ok: bool,
    found: bool,
    op: &'static str,
    locator: ArchiveRecordLocator,
    record: Option<crate::memory::ArchiveRecord>,
    plane: &'static str,
    canonical: bool,
    usage_hint: &'static str,
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
        "Fetch one cited archive-sidecar record by record_id or locator fields. Use this after memory_search to inspect a specific transcript message, daily note, or turn log. Returned content is archive evidence, not canonical shared memory, but it may support a grounded shareable conclusion after verification."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"record_id":{"type":"string","description":"Opaque archive record id returned by memory_search."},"source":{"type":"string","enum":["transcript","daily_note","turn_log"],"description":"Source type when not using record_id."},"chat_id":{"type":"string","description":"Chat id for transcript or turn_log locators."},"message_index":{"type":"integer","description":"Transcript message index returned by memory_search."},"note_name":{"type":"string","description":"Daily note file name, e.g. 2026-04-02.md."},"req_id":{"type":"string","description":"Turn log request id returned by memory_search."},"focus_query":{"type":"string","description":"Optional focus term for excerpting the returned record."},"max_chars":{"type":"integer","description":"Max content chars to return, default 1800, max 4096."}}}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        self.execute_outcome(args, ctx)
            .map(|outcome| outcome.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        _ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_memory_get")?;
        let locator = match parse_locator_choice(&obj) {
            Ok(locator) => locator,
            Err(outcome) => return Ok(*outcome),
        };
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
        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "tool_memory_get",
            &MemoryGetResponse {
                ok: record.is_some(),
                found: record.is_some(),
                op: "get",
                locator,
                record,
                plane: "archive_evidence",
                canonical: false,
                usage_hint: "Treat the returned record as evidence only. Distill stable conclusions separately; do not equate archive records with canonical shared memory. If the exact detail stays unsupported after inspection, say that plainly.",
            },
        )?))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn parse_locator_choice(
    obj: &serde_json::Map<String, Value>,
) -> std::result::Result<ArchiveRecordLocator, Box<ToolExecutionOutcome>> {
    if let Some(record_id) = obj
        .get("record_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return ArchiveRecordLocator::parse_record_id(record_id)
            .ok_or_else(|| Box::new(invalid_record_id_outcome(record_id)));
    }

    let Some(source_raw) = obj
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(Box::new(missing_locator_outcome()));
    };
    let source = source_raw
        .parse::<ArchiveRecordSource>()
        .map_err(|_| Box::new(invalid_source_outcome()))?;
    let chat_id = obj
        .get("chat_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let message_id = obj
        .get("message_id")
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
        ArchiveRecordSource::Transcript => {
            chat_id.is_some() && (message_id.is_some() || message_index.is_some())
        }
        ArchiveRecordSource::DailyNote => note_name.is_some(),
        ArchiveRecordSource::TurnLog => chat_id.is_some(),
    };
    if !valid {
        return Err(Box::new(locator_fields_outcome(source)));
    }

    Ok(ArchiveRecordLocator {
        source,
        chat_id,
        message_id,
        message_index,
        note_name,
        req_id,
    })
}

fn missing_locator_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "get",
            "warning": "memory_get: missing record_id or source",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "A memory locator is still required before memory_get can continue.",
        vec!["record_id_or_source".to_string()],
        vec![
            ToolClarificationField {
                key: "record_id".to_string(),
                label: "Record ID".to_string(),
                description: "Preferred: use the record_id returned by memory_search.".to_string(),
                required: false,
                secret: false,
                multiple: false,
                options: Vec::new(),
            },
            ToolClarificationField {
                key: "source".to_string(),
                label: "Source".to_string(),
                description: "If you do not have record_id, choose a locator source.".to_string(),
                required: false,
                secret: false,
                multiple: false,
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
            },
        ],
    ))
}

fn invalid_record_id_outcome(record_id: &str) -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "get",
            "warning": format!("memory_get: invalid record_id {}", record_id),
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "record_id is invalid; a valid record_id returned by memory_search is still required.",
        vec!["record_id".to_string()],
        vec![ToolClarificationField {
            key: "record_id".to_string(),
            label: "Record ID".to_string(),
            description: "Use a record_id returned by memory_search.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    ))
}

fn invalid_source_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "get",
            "warning": "memory_get: source must be transcript, daily_note, or turn_log",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "A supported archive source is still required when record_id is not provided.",
        vec!["source".to_string()],
        vec![ToolClarificationField {
            key: "source".to_string(),
            label: "Source".to_string(),
            description: "Choose which archive source the locator belongs to.".to_string(),
            required: true,
            secret: false,
            multiple: false,
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

fn locator_fields_outcome(source: ArchiveRecordSource) -> ToolExecutionOutcome {
    let (summary, missing_fields, clarification_fields) = match source {
        ArchiveRecordSource::Transcript => (
            "Transcript locators still require chat_id plus message_id or message_index.",
            vec![
                "chat_id".to_string(),
                "message_id_or_message_index".to_string(),
            ],
            vec![
                ToolClarificationField {
                    key: "chat_id".to_string(),
                    label: "Chat ID".to_string(),
                    description: "Transcript records need the chat_id.".to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: Vec::new(),
                },
                ToolClarificationField {
                    key: "message_id_or_message_index".to_string(),
                    label: "Message locator".to_string(),
                    description:
                        "Provide either message_id or message_index for the transcript record."
                            .to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: Vec::new(),
                },
            ],
        ),
        ArchiveRecordSource::DailyNote => (
            "Daily note locators still require note_name.",
            vec!["note_name".to_string()],
            vec![ToolClarificationField {
                key: "note_name".to_string(),
                label: "Note name".to_string(),
                description: "Daily note file name such as 2026-04-02.md.".to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: Vec::new(),
            }],
        ),
        ArchiveRecordSource::TurnLog => (
            "Turn log locators still require chat_id.",
            vec!["chat_id".to_string()],
            vec![ToolClarificationField {
                key: "chat_id".to_string(),
                label: "Chat ID".to_string(),
                description: "Turn-log locators need the chat_id.".to_string(),
                required: true,
                secret: false,
                multiple: false,
                options: Vec::new(),
            }],
        ),
    };
    ToolExecutionOutcome::text(
        serde_json::json!({
            "ok": false,
            "op": "get",
            "warning": "memory_get: locator fields do not match the requested source",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        summary,
        missing_fields,
        clarification_fields,
    ))
}

#[cfg(test)]
mod tests {
    use super::MemoryGetTool;
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
        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }
        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }
        fn set_user(&self, _content: &str) -> Result<()> {
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

    fn build_tool() -> MemoryGetTool {
        MemoryGetTool::new(
            Arc::new(EmptySessionStore),
            Arc::new(EmptyMemoryStore),
            Arc::new(EmptyTurnLedgerStore),
        )
    }

    #[test]
    fn memory_get_missing_locator_returns_facts_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{}"#, &mut ctx)
            .expect("missing locator should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
    }

    #[test]
    fn memory_get_invalid_record_id_returns_facts_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{"record_id":"bad"}"#, &mut ctx)
            .expect("invalid record id should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
    }

    #[test]
    fn memory_get_unsupported_source_returns_choice_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{"source":"summary"}"#, &mut ctx)
            .expect("unsupported source should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn memory_get_mismatched_locator_returns_facts_blocker() {
        let tool = build_tool();
        let mut ctx = DummyCtx;
        let outcome = tool
            .execute_outcome(r#"{"source":"daily_note","chat_id":"chat-1"}"#, &mut ctx)
            .expect("mismatched locator should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
    }
}
