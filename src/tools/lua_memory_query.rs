//! Linux-only read-only memory query tool built on the programmable Lua sandbox.

use crate::error::{Error, Result};
use crate::memory::{ContinuityCapsuleStore, LongTermMemoryQuery, LongTermMemoryStore};
use crate::reasoning::{
    build_memory_query_snapshot_from_stores, default_lua_memory_query_capabilities,
    validate_memory_query_result, LuaQueryBudget, LuaQueryRequest, LuaQueryResponse,
    MemoryQueryContinuityScope, MemoryQueryResult, MemoryQuerySelection, MemoryQuerySnapshot,
    ReasoningExecutor, MEMORY_QUERY_DEFAULT_CONTINUITY_LIMIT, MEMORY_QUERY_DEFAULT_LONG_TERM_LIMIT,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata, ToolRiskLevel,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct LuaMemoryQueryTool {
    executor: Arc<dyn ReasoningExecutor>,
    long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
    continuity_store: Arc<dyn ContinuityCapsuleStore + Send + Sync>,
}

#[derive(Serialize)]
struct LuaMemoryQueryToolResponse {
    ok: bool,
    plane: &'static str,
    readonly: bool,
    selection: MemoryQuerySelection,
    counts: crate::reasoning::MemoryQuerySnapshotCounts,
    snapshot_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<MemoryQueryResult>,
    #[serde(default)]
    trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_message: Option<String>,
    budget: LuaQueryBudget,
}

impl LuaMemoryQueryTool {
    pub fn new(
        executor: Arc<dyn ReasoningExecutor>,
        long_term_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
        continuity_store: Arc<dyn ContinuityCapsuleStore + Send + Sync>,
    ) -> Self {
        Self {
            executor,
            long_term_store,
            continuity_store,
        }
    }
}

impl Tool for LuaMemoryQueryTool {
    fn name(&self) -> &'static str {
        "lua_memory_query"
    }

    fn description(&self) -> &str {
        "Run a Linux-only single-turn Lua query against an explicit read-only memory snapshot composed from canonical long-term memory and continuity capsules. Output must stay within a structured reviewable contract: summary, groups, and adjudication-required candidates such as merge, split, stale, or conflict."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"script":{"type":"string","description":"Lua script chunk. It receives a read-only memory snapshot as global `input` and must return an object with summary, optional groups, and optional candidates."},"long_term_query":{"type":"object","description":"Optional structured long-term memory query filter."},"long_term_limit":{"type":"integer","description":"Optional long-term memory limit; defaults to 8 and is clamped to the memory query budget."},"continuity_scope":{"type":"object","description":"Optional continuity scope selector with scope_kind and scope_id."},"continuity_limit":{"type":"integer","description":"Optional continuity capsule limit; defaults to 6 and is clamped to the memory query budget."},"include_long_term":{"type":"boolean","description":"Whether to include canonical long-term memory records. Default true. At least one memory plane must remain enabled."},"include_continuity":{"type":"boolean","description":"Whether to include continuity capsules. Default true. At least one memory plane must remain enabled."},"timeout_ms":{"type":"integer","description":"Optional timeout in milliseconds; clamped into the programmable reasoning budget window."}},"required":["script"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "lua_memory_query_tool")?;
        let script = obj
            .get("script")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("lua_memory_query_tool", "missing script"))?;
        let selection = build_selection(&obj, ctx)?;
        let snapshot = self.build_snapshot(&selection)?;
        let timeout_ms = obj.get("timeout_ms").and_then(Value::as_u64);
        let request = LuaQueryRequest {
            script: script.to_string(),
            input: serde_json::to_value(&snapshot)
                .map_err(|error| Error::config("lua_memory_query_tool", error.to_string()))?,
            budget: timeout_ms
                .map(|value| LuaQueryBudget::default().with_timeout_ms(value))
                .unwrap_or_default(),
            capabilities: default_lua_memory_query_capabilities(),
        };
        let response = self.executor.execute_query(&request)?;
        let output = build_tool_response(snapshot, response)?;
        serialize_tool_output("lua_memory_query_tool", &output)
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_risk_level(ToolRiskLevel::Medium)
    }
}

impl LuaMemoryQueryTool {
    fn build_snapshot(&self, selection: &MemoryQuerySelection) -> Result<MemoryQuerySnapshot> {
        build_memory_query_snapshot_from_stores(
            selection,
            self.long_term_store.as_ref(),
            self.continuity_store.as_ref(),
        )
        .map_err(|error| Error::config("lua_memory_query_tool", error.to_string()))
    }
}

fn build_selection(
    obj: &serde_json::Map<String, Value>,
    ctx: &mut dyn ToolContext,
) -> Result<MemoryQuerySelection> {
    let long_term_query = obj
        .get("long_term_query")
        .cloned()
        .map(serde_json::from_value::<LongTermMemoryQuery>)
        .transpose()
        .map_err(|error| Error::config("lua_memory_query_tool", error.to_string()))?;
    let continuity_scope = obj
        .get("continuity_scope")
        .cloned()
        .map(serde_json::from_value::<MemoryQueryContinuityScope>)
        .transpose()
        .map_err(|error| Error::config("lua_memory_query_tool", error.to_string()))?
        .or_else(|| {
            ctx.current_chat_id()
                .map(|chat_id| MemoryQueryContinuityScope {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: chat_id.to_string(),
                })
        });
    let include_long_term = obj
        .get("include_long_term")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let include_continuity = obj
        .get("include_continuity")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !include_long_term && !include_continuity {
        return Err(Error::config(
            "lua_memory_query_tool",
            "at least one memory plane must be included",
        ));
    }

    Ok(MemoryQuerySelection {
        long_term_query,
        long_term_limit: obj
            .get("long_term_limit")
            .and_then(Value::as_u64)
            .unwrap_or(MEMORY_QUERY_DEFAULT_LONG_TERM_LIMIT as u64)
            as usize,
        continuity_scope,
        continuity_limit: obj
            .get("continuity_limit")
            .and_then(Value::as_u64)
            .unwrap_or(MEMORY_QUERY_DEFAULT_CONTINUITY_LIMIT as u64)
            as usize,
        include_long_term,
        include_continuity,
    })
}

fn build_tool_response(
    snapshot: MemoryQuerySnapshot,
    response: LuaQueryResponse,
) -> Result<LuaMemoryQueryToolResponse> {
    let validated_result = if response.ok {
        let result = response
            .result
            .ok_or_else(|| Error::config("lua_memory_query_tool", "missing result"))?;
        Some(validate_memory_query_result(result)?)
    } else {
        None
    };
    Ok(LuaMemoryQueryToolResponse {
        ok: response.ok,
        plane: "memory_query_plane",
        readonly: true,
        selection: snapshot.selection,
        counts: snapshot.counts,
        snapshot_digest: snapshot.snapshot_digest,
        result: validated_result,
        trace: response.trace,
        error_kind: response.error_kind,
        error_message: response.error_message,
        budget: response.budget,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{
        ContinuityCapsule, ContinuityCapsuleDraft, ContinuityCapsuleKind,
        ContinuityCapsuleScopeKind, ContinuityCapsuleSource, ContinuityCapsuleStatus,
        ContinuityCapsuleStore, ContinuityCapsuleWriteOutcome, LongTermMemoryConfidence,
        LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryFreshness, LongTermMemoryKind,
        LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemorySourceType,
        LongTermMemoryStaleHint,
    };
    use serde_json::json;

    struct StubExecutor;

    impl ReasoningExecutor for StubExecutor {
        fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
            Ok(LuaQueryResponse::success(
                json!({
                    "summary": format!(
                        "Analyzed {} long-term records and {} continuity capsules.",
                        request.input["counts"]["long_term_entries"].as_u64().unwrap_or_default(),
                        request.input["counts"]["continuity_capsules"].as_u64().unwrap_or_default()
                    ),
                    "groups": [{
                        "label": "device",
                        "summary": "Device records",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"]
                    }],
                    "candidates": [{
                        "kind": "conflict",
                        "summary": "Review device state mismatch.",
                        "rationale": "The canonical fact and continuity capsule disagree about the active state.",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"],
                        "requires_adjudication": true
                    }]
                }),
                vec!["trace:memory".to_string()],
                request.budget.clone(),
            ))
        }
    }

    #[derive(Default)]
    struct StubLongTermStore {
        entries: Vec<LongTermMemoryEntry>,
    }

    impl LongTermMemoryStore for StubLongTermStore {
        fn upsert_many(&self, _drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self.entries.iter().find(|entry| entry.id == id).cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.len())
        }
    }

    #[derive(Default)]
    struct StubContinuityStore {
        capsules: Vec<ContinuityCapsule>,
    }

    impl ContinuityCapsuleStore for StubContinuityStore {
        fn upsert_many(
            &self,
            _drafts: &[ContinuityCapsuleDraft],
            _now_secs: u64,
        ) -> Result<ContinuityCapsuleWriteOutcome> {
            Ok(ContinuityCapsuleWriteOutcome::default())
        }

        fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
            Ok(self
                .capsules
                .iter()
                .find(|capsule| capsule.capsule_id == capsule_id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
            Ok(self.capsules.iter().take(limit).cloned().collect())
        }

        fn count(&self) -> Result<usize> {
            Ok(self.capsules.len())
        }
    }

    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "lua_memory_query_tool_test",
                "network unused",
            ))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config(
                "lua_memory_query_tool_test",
                "network unused",
            ))
        }

        fn current_chat_id(&self) -> Option<&str> {
            Some("c2c:test")
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn lua_memory_query_tool_returns_validated_result_contract() {
        let tool = LuaMemoryQueryTool::new(
            Arc::new(StubExecutor),
            Arc::new(StubLongTermStore {
                entries: vec![sample_long_term_entry()],
            }),
            Arc::new(StubContinuityStore {
                capsules: vec![sample_continuity_capsule()],
            }),
        );
        let mut ctx = DummyCtx;

        let output = tool
            .execute(r#"{"script":"return {}"}"#, &mut ctx)
            .expect("tool output");
        let parsed: Value = serde_json::from_str(&output).expect("json");

        assert_eq!(parsed["ok"], json!(true));
        assert_eq!(parsed["plane"], json!("memory_query_plane"));
        assert_eq!(parsed["readonly"], json!(true));
        assert_eq!(parsed["counts"]["long_term_entries"], json!(1));
        assert_eq!(parsed["counts"]["continuity_capsules"], json!(1));
        assert_eq!(
            parsed["selection"]["continuity_scope"]["scope_id"],
            json!("c2c:test")
        );
        assert_eq!(parsed["result"]["candidates"][0]["kind"], json!("conflict"));
    }

    #[test]
    fn lua_memory_query_tool_rejects_empty_plane_selection() {
        let tool = LuaMemoryQueryTool::new(
            Arc::new(StubExecutor),
            Arc::new(StubLongTermStore::default()),
            Arc::new(StubContinuityStore::default()),
        );
        let mut ctx = DummyCtx;

        let error = tool
            .execute(
                r#"{"script":"return {}","include_long_term":false,"include_continuity":false}"#,
                &mut ctx,
            )
            .expect_err("selection should be rejected");

        assert!(error
            .to_string()
            .contains("at least one memory plane must be included"));
    }

    fn sample_long_term_entry() -> LongTermMemoryEntry {
        LongTermMemoryEntry {
            id: "fact:device_info".to_string(),
            kind: LongTermMemoryKind::Fact,
            topic: "device_info".to_string(),
            content: "Board is Beetle Linux".to_string(),
            keywords: vec!["device".to_string()],
            source_chat_id: Some("c2c:test".to_string()),
            source_type: LongTermMemorySourceType::SystemRuntime,
            source_scope: LongTermMemorySourceScope::World,
            confidence: LongTermMemoryConfidence::High,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::VerifyAgainstCurrentState,
            supporting_citations: vec!["board_info".to_string()],
            evidence_count: 2,
            created_at: 10,
            updated_at: 20,
            observed_at: 20,
            last_confirmed_at: 20,
            source_revision: 1,
            last_used_at: 0,
        }
    }

    fn sample_continuity_capsule() -> ContinuityCapsule {
        ContinuityCapsule {
            capsule_id: "capsule:device_status".to_string(),
            kind: ContinuityCapsuleKind::HandoffState,
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: "c2c:test".to_string(),
            source_chat_id: "c2c:test".to_string(),
            source_channel: "qq_channel".to_string(),
            run_id: String::new(),
            topic: "device_status".to_string(),
            summary: "User is investigating runtime status.".to_string(),
            outcome: String::new(),
            decisions: Vec::new(),
            next_step: "Review runtime snapshot".to_string(),
            unresolved: vec!["Need refreshed board status".to_string()],
            artifact_refs: Vec::new(),
            provenance_refs: vec!["source=post_reply_maintenance".to_string()],
            source: ContinuityCapsuleSource::PostReplyMaintenance,
            status: ContinuityCapsuleStatus::Active,
            supersedes: Vec::new(),
            observed_at: 20,
            updated_at: 25,
        }
    }
}
