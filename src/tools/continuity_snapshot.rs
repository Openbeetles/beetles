//! continuity_snapshot tool: export/import core continuity state.

use crate::error::{Error, Result};
use crate::memory::{
    export_continuity_snapshot, import_continuity_snapshot, render_continuity_snapshot_markdown,
    ContinuitySnapshot, ContinuitySnapshotExportContext, ContinuitySnapshotImportContext,
    ContinuitySnapshotImportMode, ContinuitySnapshotMode, ExecutionStateStore, LongTermMemoryStore,
    SelfContinuityStore, SelfModelStore, SessionSummaryStore,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::util::current_unix_secs;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ContinuitySnapshotTool {
    long_term_memory_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
    session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
    execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
    self_model_store: Arc<dyn SelfModelStore + Send + Sync>,
    self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
}

impl ContinuitySnapshotTool {
    pub fn new(
        long_term_memory_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
        session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
        execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
        self_model_store: Arc<dyn SelfModelStore + Send + Sync>,
        self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
    ) -> Self {
        Self {
            long_term_memory_store,
            session_summary_store,
            execution_state_store,
            self_model_store,
            self_continuity_store,
        }
    }
}

impl Tool for ContinuitySnapshotTool {
    fn name(&self) -> &'static str {
        "continuity_snapshot"
    }

    fn description(&self) -> &'static str {
        "Export or import the assistant's core continuity state for bootstrap or full restore. This is an operator/admin tool, not a normal conversational tool."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["export","import"],"description":"Whether to export a continuity snapshot or import one."},"chat_id":{"type":"string","description":"Target chat_id. Defaults to the current chat when available."},"mode":{"type":"string","enum":["bootstrap","full_restore","bootstrap_import"],"description":"Export mode or import mode. export accepts bootstrap|full_restore. import accepts bootstrap_import|full_restore."},"format":{"type":"string","enum":["json","markdown"],"description":"Export rendering format. Default json."},"snapshot":{"description":"Snapshot payload to import. May be a JSON string or embedded object."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_continuity_snapshot")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_continuity_snapshot", "missing op"))?;
        let chat_id = obj
            .get("chat_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| ctx.current_chat_id().map(str::to_string))
            .ok_or_else(|| Error::config("tool_continuity_snapshot", "missing chat_id"))?;

        match op {
            "export" => {
                let mode = parse_export_mode(obj.get("mode"))?;
                let snapshot = export_continuity_snapshot(
                    ContinuitySnapshotExportContext {
                        long_term_memory_store: self.long_term_memory_store.as_ref(),
                        session_summary_store: self.session_summary_store.as_ref(),
                        execution_state_store: self.execution_state_store.as_ref(),
                        self_model_store: self.self_model_store.as_ref(),
                        self_continuity_store: self.self_continuity_store.as_ref(),
                    },
                    &chat_id,
                    mode,
                    current_unix_secs(),
                )?;
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                let rendered = if format == "markdown" {
                    json!({
                        "ok": true,
                        "op": "export",
                        "chat_id": chat_id,
                        "mode": mode,
                        "format": "markdown",
                        "markdown": render_continuity_snapshot_markdown(&snapshot),
                        "snapshot": snapshot,
                    })
                } else {
                    json!({
                        "ok": true,
                        "op": "export",
                        "chat_id": chat_id,
                        "mode": mode,
                        "format": "json",
                        "snapshot": snapshot,
                    })
                };
                Ok(rendered.to_string())
            }
            "import" => {
                let mode = parse_import_mode(obj.get("mode"))?;
                let snapshot = parse_snapshot(obj.get("snapshot"))?;
                let outcome = import_continuity_snapshot(
                    ContinuitySnapshotImportContext {
                        long_term_memory_store: self.long_term_memory_store.as_ref(),
                        execution_state_store: self.execution_state_store.as_ref(),
                        self_model_store: self.self_model_store.as_ref(),
                        self_continuity_store: self.self_continuity_store.as_ref(),
                    },
                    &chat_id,
                    &snapshot,
                    mode,
                )?;
                Ok(json!({
                    "ok": true,
                    "op": "import",
                    "chat_id": chat_id,
                    "mode": mode,
                    "outcome": outcome,
                })
                .to_string())
            }
            _ => Err(Error::config(
                "tool_continuity_snapshot",
                "op must be export or import",
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::admin()
    }
}

fn parse_export_mode(value: Option<&Value>) -> Result<ContinuitySnapshotMode> {
    match value.and_then(Value::as_str).unwrap_or("bootstrap") {
        "bootstrap" => Ok(ContinuitySnapshotMode::Bootstrap),
        "full_restore" => Ok(ContinuitySnapshotMode::FullRestore),
        other => Err(Error::config(
            "tool_continuity_snapshot",
            format!("unsupported export mode: {}", other),
        )),
    }
}

fn parse_import_mode(value: Option<&Value>) -> Result<ContinuitySnapshotImportMode> {
    match value.and_then(Value::as_str).unwrap_or("bootstrap_import") {
        "bootstrap_import" | "bootstrap" => Ok(ContinuitySnapshotImportMode::BootstrapImport),
        "full_restore" => Ok(ContinuitySnapshotImportMode::FullRestore),
        other => Err(Error::config(
            "tool_continuity_snapshot",
            format!("unsupported import mode: {}", other),
        )),
    }
}

fn parse_snapshot(value: Option<&Value>) -> Result<ContinuitySnapshot> {
    let value =
        value.ok_or_else(|| Error::config("tool_continuity_snapshot", "missing snapshot"))?;
    if let Some(raw) = value.as_str() {
        serde_json::from_str(raw)
            .map_err(|error| Error::config("tool_continuity_snapshot", error.to_string()))
    } else {
        serde_json::from_value(value.clone())
            .map_err(|error| Error::config("tool_continuity_snapshot", error.to_string()))
    }
}
