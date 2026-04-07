//! continuity_snapshot tool: export/import core continuity state.
#![allow(clippy::too_many_arguments)]

use crate::error::{Error, Result};
use crate::memory::{
    export_continuity_snapshot, import_continuity_snapshot, inspect_memory_hygiene,
    inspect_personality_governance, inspect_working_recall, load_recent_persona_evidence,
    render_continuity_snapshot_markdown, render_memory_hygiene_inspection_markdown,
    render_personality_governance_inspection_markdown, render_working_recall_inspection_markdown,
    ContinuitySnapshot, ContinuitySnapshotExportContext, ContinuitySnapshotImportContext,
    ContinuitySnapshotImportMode, ContinuitySnapshotMode, CoreRevisionLedgerStore,
    ExecutionStateStore, LongTermMemoryStore, MemoryHygieneContext, MemoryProfile, MemoryStore,
    PersonalityGovernanceInspectionInput, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, SelfAuthoredCoreStore,
    SelfContinuityStore, SelfModelStore, SessionStore, SessionSummaryStore, TurnLedgerStore,
    WorkingRecallInspectionInput,
};
use crate::platform::{SkillStorage, StateFs};
use crate::task_execution::{
    inspect_task_learning, inspect_task_workspace, render_task_learning_inspection_markdown,
    render_task_workspace_inspection_markdown, TaskArtifactStore, TaskExecutionLedgerStore,
    TaskLearningStore, TaskRunStore,
};
use crate::tools::{
    parse_tool_args, render_tool_execution_governance_markdown, Tool, ToolContext,
    ToolExecutionGovernance, ToolMetadata,
};
use crate::util::current_unix_secs;
use serde_json::{json, Value};
use std::sync::Arc;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";

pub struct ContinuitySnapshotTool {
    state_fs: Arc<dyn StateFs + Send + Sync>,
    session_store: Arc<dyn SessionStore + Send + Sync>,
    memory_store: Arc<dyn MemoryStore + Send + Sync>,
    long_term_memory_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
    session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
    execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
    self_model_store: Arc<dyn SelfModelStore + Send + Sync>,
    self_authored_core_store: Arc<dyn SelfAuthoredCoreStore + Send + Sync>,
    core_revision_ledger_store: Arc<dyn CoreRevisionLedgerStore + Send + Sync>,
    self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
    relationship_constitution_store: Arc<dyn RelationshipConstitutionStore + Send + Sync>,
    relationship_portfolio_store: Arc<dyn RelationshipPortfolioStore + Send + Sync>,
    relationship_topology_store: Arc<dyn RelationshipTopologyStore + Send + Sync>,
    task_run_store: Arc<dyn TaskRunStore + Send + Sync>,
    task_artifact_store: Arc<dyn TaskArtifactStore + Send + Sync>,
    task_execution_ledger_store: Arc<dyn TaskExecutionLedgerStore + Send + Sync>,
    task_learning_store: Arc<dyn TaskLearningStore + Send + Sync>,
    skill_storage: Arc<dyn SkillStorage + Send + Sync>,
    tool_execution_governance: Arc<ToolExecutionGovernance>,
}

impl ContinuitySnapshotTool {
    pub fn new(
        state_fs: Arc<dyn StateFs + Send + Sync>,
        session_store: Arc<dyn SessionStore + Send + Sync>,
        memory_store: Arc<dyn MemoryStore + Send + Sync>,
        long_term_memory_store: Arc<dyn LongTermMemoryStore + Send + Sync>,
        session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
        execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
        self_model_store: Arc<dyn SelfModelStore + Send + Sync>,
        self_authored_core_store: Arc<dyn SelfAuthoredCoreStore + Send + Sync>,
        core_revision_ledger_store: Arc<dyn CoreRevisionLedgerStore + Send + Sync>,
        self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
        turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
        relationship_constitution_store: Arc<dyn RelationshipConstitutionStore + Send + Sync>,
        relationship_portfolio_store: Arc<dyn RelationshipPortfolioStore + Send + Sync>,
        relationship_topology_store: Arc<dyn RelationshipTopologyStore + Send + Sync>,
        task_run_store: Arc<dyn TaskRunStore + Send + Sync>,
        task_artifact_store: Arc<dyn TaskArtifactStore + Send + Sync>,
        task_execution_ledger_store: Arc<dyn TaskExecutionLedgerStore + Send + Sync>,
        task_learning_store: Arc<dyn TaskLearningStore + Send + Sync>,
        skill_storage: Arc<dyn SkillStorage + Send + Sync>,
        tool_execution_governance: Arc<ToolExecutionGovernance>,
    ) -> Self {
        Self {
            state_fs,
            session_store,
            memory_store,
            long_term_memory_store,
            session_summary_store,
            execution_state_store,
            self_model_store,
            self_authored_core_store,
            core_revision_ledger_store,
            self_continuity_store,
            turn_ledger_store,
            relationship_constitution_store,
            relationship_portfolio_store,
            relationship_topology_store,
            task_run_store,
            task_artifact_store,
            task_execution_ledger_store,
            task_learning_store,
            skill_storage,
            tool_execution_governance,
        }
    }
}

impl Tool for ContinuitySnapshotTool {
    fn name(&self) -> &'static str {
        "continuity_snapshot"
    }

    fn description(&self) -> &'static str {
        "Export, save, list, load, import, or inspect the assistant's continuity and personality-governance state for bootstrap, restore, and operator auditing. This is an operator/admin tool, not a normal conversational tool."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["export","import","list_saved","inspect_governance","inspect_recall","inspect_hygiene","inspect_tool_governance","inspect_task_learning","inspect_task_workspace"],"description":"Whether to export, import, list saved continuity snapshots, inspect personality governance, inspect working recall, inspect hygiene governance, inspect tool-execution governance, inspect task-learning routing, or inspect a task workspace."},"chat_id":{"type":"string","description":"Target chat_id. Defaults to the current chat when available."},"channel":{"type":"string","description":"Target channel for governance inspection. Defaults to the current channel when available."},"query":{"type":"string","description":"Recall or task-learning inspection query. Leave empty to inspect the current state without a search hint."},"run_id":{"type":"string","description":"Optional explicit task run id for workspace inspection."},"profile":{"type":"string","enum":["standard","embedded"],"description":"Memory profile used for recall or hygiene inspection. Default standard."},"mode":{"type":"string","enum":["bootstrap","full_restore","bootstrap_import"],"description":"Export mode or import mode. export accepts bootstrap|full_restore. import accepts bootstrap_import|full_restore."},"format":{"type":"string","enum":["json","markdown"],"description":"Rendering format. Default json."},"save_name":{"type":"string","description":"Optional saved snapshot name. On export, saves the snapshot under this name. On import, loads the saved snapshot with this name when snapshot is omitted."},"snapshot":{"description":"Snapshot payload to import. May be a JSON string or embedded object."}},"required":["op"]}"#
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
            .unwrap_or_default();
        let channel = obj
            .get("channel")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| ctx.current_channel().map(str::to_string))
            .unwrap_or_default();

        match op {
            "list_saved" => {
                let saved = self
                    .state_fs
                    .list_dir(REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS)?
                    .into_iter()
                    .filter_map(|name| {
                        name.strip_suffix(".json")
                            .map(str::to_string)
                            .filter(|value| !value.trim().is_empty())
                    })
                    .collect::<Vec<_>>();
                Ok(json!({
                    "ok": true,
                    "op": "list_saved",
                    "saved": saved,
                })
                .to_string())
            }
            "inspect_governance" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                if channel.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing channel"));
                }
                let subject_id = crate::memory::board_subject_scope_id();
                let relationship_scope_id =
                    crate::memory::relationship_scope_id(&channel, &chat_id);
                let self_authored_core = self.self_authored_core_store.get(subject_id)?;
                let core_revision_ledger = self.core_revision_ledger_store.get(subject_id)?;
                let relationship_constitution = self
                    .relationship_constitution_store
                    .get(&relationship_scope_id)?;
                let relationship_topology = self.relationship_topology_store.get(subject_id)?;
                let recent_persona_evidence = load_recent_persona_evidence(
                    self.turn_ledger_store.as_ref(),
                    &relationship_scope_id,
                )?;
                let inspection =
                    inspect_personality_governance(PersonalityGovernanceInspectionInput {
                        channel: &channel,
                        chat_id: &chat_id,
                        now_secs: current_unix_secs(),
                        self_authored_core: self_authored_core.as_ref(),
                        core_revision_ledger: core_revision_ledger.as_ref(),
                        relationship_constitution: relationship_constitution.as_ref(),
                        relationship_topology: relationship_topology.as_ref(),
                        recent_persona_evidence: recent_persona_evidence.as_ref(),
                    });
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_governance",
                        "chat_id": chat_id,
                        "channel": channel,
                        "format": "markdown",
                        "repair_plan": inspection.repair_plan.clone(),
                        "markdown": render_personality_governance_inspection_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_governance",
                        "chat_id": chat_id,
                        "channel": channel,
                        "format": "json",
                        "repair_plan": inspection.repair_plan.clone(),
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "inspect_recall" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                let query = obj
                    .get("query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("");
                let profile = parse_memory_profile(obj.get("profile"))?;
                let recall_policy = crate::memory::memory_policy(profile).long_term_recall;
                let recent = self.session_store.load_recent(
                    chat_id.as_str(),
                    recall_policy.recent_grounding_message_count,
                )?;
                let summary_text = self
                    .session_summary_store
                    .get_with_count(chat_id.as_str())?
                    .map(|(summary, _)| summary)
                    .filter(|summary| !summary.trim().is_empty());
                let inspection = inspect_working_recall(WorkingRecallInspectionInput {
                    chat_id: chat_id.as_str(),
                    query,
                    summary_text: summary_text.as_deref(),
                    recent: &recent,
                    system_max_len: 1200,
                    profile,
                    current_channel: (!channel.trim().is_empty()).then_some(channel.as_str()),
                    session_store: self.session_store.as_ref(),
                    memory_store: self.memory_store.as_ref(),
                    long_term_memory_store: self.long_term_memory_store.as_ref(),
                    turn_ledger_store: self.turn_ledger_store.as_ref(),
                    skill_storage: Some(self.skill_storage.as_ref()),
                    task_run_store: Some(self.task_run_store.as_ref()),
                    task_learning_store: Some(self.task_learning_store.as_ref()),
                });
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_recall",
                        "chat_id": chat_id,
                        "query": query,
                        "profile": inspection.profile,
                        "format": "markdown",
                        "markdown": render_working_recall_inspection_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_recall",
                        "chat_id": chat_id,
                        "query": query,
                        "profile": inspection.profile,
                        "format": "json",
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "inspect_hygiene" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                let profile = parse_memory_profile(obj.get("profile"))?;
                let inspection = inspect_memory_hygiene(
                    MemoryHygieneContext {
                        session_store: self.session_store.as_ref(),
                        session_summary_store: self.session_summary_store.as_ref(),
                        memory_store: self.memory_store.as_ref(),
                        turn_ledger_store: self.turn_ledger_store.as_ref(),
                        long_term_memory_store: self.long_term_memory_store.as_ref(),
                        skill_storage: self.skill_storage.as_ref(),
                    },
                    chat_id.as_str(),
                    profile,
                    current_unix_secs(),
                );
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_hygiene",
                        "chat_id": chat_id,
                        "format": "markdown",
                        "markdown": render_memory_hygiene_inspection_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_hygiene",
                        "chat_id": chat_id,
                        "format": "json",
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "inspect_task_learning" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                if channel.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing channel"));
                }
                let query = obj
                    .get("query")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("");
                let inspection = inspect_task_learning(
                    self.task_learning_store.as_ref(),
                    &channel,
                    &chat_id,
                    query,
                );
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_task_learning",
                        "chat_id": chat_id,
                        "channel": channel,
                        "query": query,
                        "format": "markdown",
                        "markdown": render_task_learning_inspection_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_task_learning",
                        "chat_id": chat_id,
                        "channel": channel,
                        "query": query,
                        "format": "json",
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "inspect_task_workspace" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                if channel.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing channel"));
                }
                let run_id = obj
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let inspection = inspect_task_workspace(
                    self.task_run_store.as_ref(),
                    self.task_artifact_store.as_ref(),
                    self.task_execution_ledger_store.as_ref(),
                    self.task_learning_store.as_ref(),
                    &channel,
                    &chat_id,
                    run_id,
                );
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_task_workspace",
                        "chat_id": chat_id,
                        "channel": channel,
                        "run_id": inspection.run_id,
                        "format": "markdown",
                        "markdown": render_task_workspace_inspection_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_task_workspace",
                        "chat_id": chat_id,
                        "channel": channel,
                        "run_id": inspection.run_id,
                        "format": "json",
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "inspect_tool_governance" => {
                let inspection = self.tool_execution_governance.inspect()?;
                let format = obj
                    .get("format")
                    .and_then(Value::as_str)
                    .unwrap_or("json")
                    .trim()
                    .to_ascii_lowercase();
                if format == "markdown" {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_tool_governance",
                        "format": "markdown",
                        "markdown": render_tool_execution_governance_markdown(&inspection),
                        "inspection": inspection,
                    })
                    .to_string())
                } else {
                    Ok(json!({
                        "ok": true,
                        "op": "inspect_tool_governance",
                        "format": "json",
                        "inspection": inspection,
                    })
                    .to_string())
                }
            }
            "export" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                let mode = parse_export_mode(obj.get("mode"))?;
                let snapshot = export_continuity_snapshot(
                    ContinuitySnapshotExportContext {
                        long_term_memory_store: self.long_term_memory_store.as_ref(),
                        session_summary_store: self.session_summary_store.as_ref(),
                        execution_state_store: self.execution_state_store.as_ref(),
                        self_model_store: self.self_model_store.as_ref(),
                        self_authored_core_store: self.self_authored_core_store.as_ref(),
                        core_revision_ledger_store: self.core_revision_ledger_store.as_ref(),
                        self_continuity_store: self.self_continuity_store.as_ref(),
                        relationship_constitution_store: self
                            .relationship_constitution_store
                            .as_ref(),
                        relationship_portfolio_store: self.relationship_portfolio_store.as_ref(),
                        relationship_topology_store: self.relationship_topology_store.as_ref(),
                    },
                    &chat_id,
                    mode,
                    current_unix_secs(),
                )?;
                let save_name = obj
                    .get("save_name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let saved_path = save_name
                    .map(snapshot_rel_path_for_name)
                    .transpose()?
                    .map(|rel_path| {
                        let payload = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
                            Error::config("tool_continuity_snapshot", error.to_string())
                        })?;
                        self.state_fs.write(&rel_path, &payload)?;
                        Ok(rel_path)
                    })
                    .transpose()?;
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
                        "saved_path": saved_path,
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
                        "saved_path": saved_path,
                        "snapshot": snapshot,
                    })
                };
                Ok(rendered.to_string())
            }
            "import" => {
                if chat_id.trim().is_empty() {
                    return Err(Error::config("tool_continuity_snapshot", "missing chat_id"));
                }
                let mode = parse_import_mode(obj.get("mode"))?;
                let snapshot = parse_snapshot(
                    obj.get("snapshot"),
                    obj.get("save_name").and_then(Value::as_str),
                    self.state_fs.as_ref(),
                )?;
                let outcome = import_continuity_snapshot(
                    ContinuitySnapshotImportContext {
                        long_term_memory_store: self.long_term_memory_store.as_ref(),
                        session_summary_store: self.session_summary_store.as_ref(),
                        execution_state_store: self.execution_state_store.as_ref(),
                        self_model_store: self.self_model_store.as_ref(),
                        self_authored_core_store: self.self_authored_core_store.as_ref(),
                        core_revision_ledger_store: self.core_revision_ledger_store.as_ref(),
                        self_continuity_store: self.self_continuity_store.as_ref(),
                        relationship_constitution_store: self
                            .relationship_constitution_store
                            .as_ref(),
                        relationship_portfolio_store: self.relationship_portfolio_store.as_ref(),
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
                "op must be export, import, list_saved, inspect_governance, inspect_recall, inspect_hygiene, inspect_tool_governance, inspect_task_learning, or inspect_task_workspace",
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

fn parse_memory_profile(value: Option<&Value>) -> Result<MemoryProfile> {
    match value.and_then(Value::as_str).map(str::trim) {
        None | Some("") | Some("standard") => Ok(MemoryProfile::Standard),
        Some("embedded") => Ok(MemoryProfile::Embedded),
        Some(other) => Err(Error::config(
            "tool_continuity_snapshot",
            format!("unsupported memory profile: {}", other),
        )),
    }
}

fn parse_snapshot(
    value: Option<&Value>,
    save_name: Option<&str>,
    state_fs: &dyn StateFs,
) -> Result<ContinuitySnapshot> {
    if let Some(value) = value {
        if let Some(raw) = value.as_str() {
            return serde_json::from_str(raw)
                .map_err(|error| Error::config("tool_continuity_snapshot", error.to_string()));
        }
        return serde_json::from_value(value.clone())
            .map_err(|error| Error::config("tool_continuity_snapshot", error.to_string()));
    }
    let save_name = save_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::config("tool_continuity_snapshot", "missing snapshot or save_name")
        })?;
    let rel_path = snapshot_rel_path_for_name(save_name)?;
    let bytes = state_fs
        .read(&rel_path)?
        .ok_or_else(|| Error::config("tool_continuity_snapshot", "saved snapshot not found"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| Error::config("tool_continuity_snapshot", error.to_string()))
}

fn snapshot_rel_path_for_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::config("tool_continuity_snapshot", "empty save_name"));
    }
    let normalized = trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if normalized.trim_matches('_').is_empty() {
        return Err(Error::config(
            "tool_continuity_snapshot",
            "invalid save_name",
        ));
    }
    Ok(format!(
        "{}/{}.json",
        REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS,
        normalized.trim_matches('_')
    ))
}
