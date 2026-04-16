//! GET /api/memory/status: operator-facing memory snapshot and optional deep inspection.

use super::HandlerContext;
use crate::diagnosis::build_memory_runtime_diagnosis;
use crate::memory::{
    board_subject_scope_id, build_continuity_capsule_operator_summary,
    compute_core_revision_governance_digest, export_continuity_snapshot,
    inspect_intelligence_replay, inspect_memory_hygiene, inspect_working_recall,
    ContinuityCapsuleOperatorSummary, ContinuitySnapshotExportContext, ContinuitySnapshotManifest,
    ContinuitySnapshotMode, IntelligenceReplayInspection, MemoryHygieneContext,
    MemoryHygieneInspection, MemorySystemKind, WorkingRecallInspection,
    WorkingRecallInspectionInput,
};
use crate::platform::memory_operator_surface::{
    build_memory_operator_surface, MemoryOperatorInspectionTarget, MemoryOperatorRecallTrace,
    MemoryOperatorSurfaceSummary, MemoryOperatorTraceInput,
};
use crate::skills::{build_runtime_skill_operator_summary, is_runtime_skill_name};
use crate::task_execution::{
    build_task_execution_operator_snapshot, inspect_task_learning, inspect_task_workspace,
    TaskExecutionOperatorSnapshot, TaskLearningInspection, TaskWorkspaceInspection,
};
use crate::util::{current_unix_secs, percent_decode_query};
use serde::Serialize;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";
const MEMORY_STATUS_RECALL_SYSTEM_MAX_LEN: usize = 2_400;
const MEMORY_STATUS_RECENT_MESSAGES: usize = 24;

#[derive(Debug, Serialize)]
struct MemoryStoreStatus {
    runtime_skill_count: usize,
    memory_len: usize,
    soul_len: usize,
    user_len: usize,
    long_term_count: usize,
    continuity_capsule_count: usize,
}

#[derive(Debug, Serialize)]
struct MemoryPersonalityStatus {
    subject_id: String,
    self_model_present: bool,
    self_authored_core_present: bool,
    core_revision_ledger_present: bool,
    board_revision: u64,
    core_review_due: bool,
    core_conservative_mode: bool,
    observation_active: bool,
    self_continuity_present: bool,
    relationship_portfolio_present: bool,
    relationship_topology_present: bool,
}

#[derive(Debug, Serialize)]
struct MemoryContinuityTooling {
    export_supported: bool,
    import_supported: bool,
    inspect_recall_supported: bool,
    inspect_hygiene_supported: bool,
    inspect_task_learning_supported: bool,
    inspect_task_workspace_supported: bool,
    saved_snapshot_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    saved_snapshots: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemorySnapshotPreview {
    mode: String,
    manifest: ContinuitySnapshotManifest,
}

#[derive(Debug, Serialize)]
struct MemoryInspectionTarget {
    chat_id: String,
    channel: String,
    query: String,
    memory_system_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    run_id: Option<String>,
    message_count: usize,
    recent_message_count: usize,
    summary_present: bool,
    summary_message_count: usize,
    execution_state_present: bool,
}

#[derive(Debug, Serialize)]
struct MemoryDeepInspection {
    target: MemoryInspectionTarget,
    snapshot_preview: MemorySnapshotPreview,
    intelligence_replay: IntelligenceReplayInspection,
    recall: WorkingRecallInspection,
    hygiene: MemoryHygieneInspection,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_learning: Option<TaskLearningInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_workspace: Option<TaskWorkspaceInspection>,
}

#[derive(Debug, Serialize)]
struct MemoryLearningMetrics {
    validated_runtime_skills: usize,
    revision_pending_runtime_skills: usize,
    garbage_collectable_experience_crystals: usize,
    promoted_task_candidates: usize,
    observed_task_candidates: usize,
    rejected_task_candidates: usize,
}

#[derive(Debug, Serialize)]
struct MemoryLearningStatus {
    task_candidates: crate::task_execution::TaskLearningOperatorSnapshot,
    runtime_skills: crate::skills::RuntimeSkillOperatorSummary,
    experience_crystals: crate::ExperienceCrystalOperatorSummary,
    metrics: MemoryLearningMetrics,
}

#[derive(Debug, Serialize)]
struct MemoryStatusBody {
    memory_system_kind: String,
    memory_len: usize,
    soul_len: usize,
    user_len: usize,
    long_term_count: usize,
    continuity_capsule_count: usize,
    stores: MemoryStoreStatus,
    personality: MemoryPersonalityStatus,
    continuity_tooling: MemoryContinuityTooling,
    continuity_capsules: ContinuityCapsuleOperatorSummary,
    task_execution: TaskExecutionOperatorSnapshot,
    learning: MemoryLearningStatus,
    diagnosis: crate::diagnosis::DiagnosisResult,
    operator_surface: MemoryOperatorSurfaceSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    inspection: Option<MemoryDeepInspection>,
}

struct MemoryStatusRequest {
    chat_id: Option<String>,
    channel: Option<String>,
    deep: bool,
    query: String,
    run_id: Option<String>,
    memory_system_kind: MemorySystemKind,
    snapshot_mode: ContinuitySnapshotMode,
}

/// Generate a structured memory/operator JSON body.
pub fn body(ctx: &HandlerContext, uri: &str) -> Result<String, std::io::Error> {
    let request = parse_request(ctx, uri);
    if crate::platform::operator_surface::operator_surface_budget(
        request.memory_system_kind,
        crate::state::esp_operator_window_active(),
    )
    .window_required_for_deep_routes
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "embedded memory inspection requires operator window",
        ));
    }
    let subject_id = board_subject_scope_id();
    let memory_len = ctx
        .memory_store
        .get_memory()
        .map(|s| s.len())
        .unwrap_or_default();
    let soul_len = ctx
        .memory_store
        .get_soul()
        .map(|s| s.len())
        .unwrap_or_default();
    let user_len = ctx
        .memory_store
        .get_user()
        .map(|s| s.len())
        .unwrap_or_default();
    let long_term_count = ctx
        .platform
        .long_term_memory_store()
        .count()
        .unwrap_or_default();
    let continuity_capsule_count = ctx
        .platform
        .continuity_capsule_store()
        .count()
        .unwrap_or_default();
    let continuity_capsules =
        build_continuity_capsule_operator_summary(ctx.platform.continuity_capsule_store().as_ref())
            .map_err(std::io::Error::other)?;
    let runtime_skill_count = ctx
        .skill_storage
        .list_names()
        .map(|names| {
            names
                .into_iter()
                .filter(|name| is_runtime_skill_name(name))
                .count()
        })
        .unwrap_or_default();
    let self_model = ctx
        .platform
        .self_model_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let self_authored_core = ctx
        .platform
        .self_authored_core_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let core_revision_ledger = ctx
        .platform
        .core_revision_ledger_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let self_continuity = ctx
        .platform
        .self_continuity_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let relationship_portfolio = ctx
        .platform
        .relationship_portfolio_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let relationship_topology = ctx
        .platform
        .relationship_topology_store()
        .get(subject_id)
        .map_err(std::io::Error::other)?;
    let now_secs = current_unix_secs();
    let governance_digest = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        now_secs,
    );
    let continuity_tool_available = ctx.tool_registry.get("continuity_snapshot").is_some();
    let mut saved_snapshots = ctx
        .platform
        .state_fs()
        .list_dir(REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.strip_suffix(".json").map(str::to_string))
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>();
    saved_snapshots.sort();
    let task_execution = build_task_execution_operator_snapshot(
        ctx.platform.task_run_store().as_ref(),
        ctx.platform.task_artifact_store().as_ref(),
        ctx.platform.task_learning_store().as_ref(),
    )
    .map_err(std::io::Error::other)?;
    let runtime_skill_learning = build_runtime_skill_operator_summary(ctx.skill_storage.as_ref());
    let experience_crystals = crate::build_experience_crystal_operator_summary(
        &runtime_skill_learning,
        Some(&task_execution.learning),
    );
    let learning = MemoryLearningStatus {
        task_candidates: task_execution.learning.clone(),
        runtime_skills: runtime_skill_learning.clone(),
        experience_crystals: experience_crystals.clone(),
        metrics: MemoryLearningMetrics {
            validated_runtime_skills: runtime_skill_learning.validated,
            revision_pending_runtime_skills: runtime_skill_learning.revision_pending,
            garbage_collectable_experience_crystals: experience_crystals.garbage_collectable,
            promoted_task_candidates: task_execution.learning.candidate_promoted,
            observed_task_candidates: task_execution.learning.candidate_observed,
            rejected_task_candidates: task_execution.learning.candidate_rejected,
        },
    };
    let inspection = request
        .deep
        .then_some(request.chat_id.as_deref())
        .flatten()
        .map(|chat_id| build_deep_inspection(ctx, &request, chat_id))
        .transpose()?;
    let operator_surface = build_memory_operator_surface(
        ctx.platform.as_ref(),
        Some(ctx.tool_registry.as_ref()),
        inspection.as_ref().map(build_trace_input).as_ref(),
    )
    .map_err(std::io::Error::other)?;
    let diagnosis = build_memory_runtime_diagnosis(&operator_surface);
    let payload = MemoryStatusBody {
        memory_system_kind: ctx.platform.memory_system_kind().as_str().to_string(),
        memory_len,
        soul_len,
        user_len,
        long_term_count,
        continuity_capsule_count,
        stores: MemoryStoreStatus {
            runtime_skill_count,
            memory_len,
            soul_len,
            user_len,
            long_term_count,
            continuity_capsule_count,
        },
        personality: MemoryPersonalityStatus {
            subject_id: subject_id.to_string(),
            self_model_present: self_model
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
            self_authored_core_present: self_authored_core
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
            core_revision_ledger_present: core_revision_ledger
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
            board_revision: self_authored_core
                .as_ref()
                .map(|core| core.revision)
                .unwrap_or(0),
            core_review_due: governance_digest.review_due,
            core_conservative_mode: governance_digest.conservative_mode,
            observation_active: governance_digest.observation_active,
            self_continuity_present: self_continuity
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
            relationship_portfolio_present: relationship_portfolio
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
            relationship_topology_present: relationship_topology
                .as_ref()
                .is_some_and(|value| value.is_meaningful()),
        },
        continuity_tooling: MemoryContinuityTooling {
            export_supported: continuity_tool_available,
            import_supported: continuity_tool_available,
            inspect_recall_supported: continuity_tool_available,
            inspect_hygiene_supported: continuity_tool_available,
            inspect_task_learning_supported: continuity_tool_available,
            inspect_task_workspace_supported: continuity_tool_available,
            saved_snapshot_count: saved_snapshots.len(),
            saved_snapshots,
        },
        continuity_capsules,
        task_execution,
        learning,
        diagnosis,
        operator_surface,
        inspection,
    };
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}

fn build_trace_input(inspection: &MemoryDeepInspection) -> MemoryOperatorTraceInput {
    MemoryOperatorTraceInput {
        inspection_target: MemoryOperatorInspectionTarget {
            chat_id: inspection.target.chat_id.clone(),
            channel: inspection.target.channel.clone(),
            query: inspection.target.query.clone(),
            memory_system_kind: inspection.target.memory_system_kind.clone(),
            run_id: inspection.target.run_id.clone(),
            message_count: inspection.target.message_count,
            recent_message_count: inspection.target.recent_message_count,
            summary_present: inspection.target.summary_present,
            summary_message_count: inspection.target.summary_message_count,
            execution_state_present: inspection.target.execution_state_present,
        },
        snapshot_manifest: inspection.snapshot_preview.manifest.clone(),
        intelligence_replay: inspection.intelligence_replay.clone(),
        recall: MemoryOperatorRecallTrace {
            prompt_recall_intent: inspection.recall.prompt_recall_intent,
            shared_factual_report: inspection.recall.shared_factual_report.clone(),
            continuity_capsule_report: inspection.recall.continuity_capsule_report.clone(),
            runtime_skill_report: inspection.recall.runtime_skill_report.clone(),
            archive_recall_report: inspection.recall.archive_recall_report.clone(),
            task_recall_report: inspection.recall.task_recall_report.clone(),
            cross_plane_rerank: inspection.recall.cross_plane_rerank.clone(),
        },
    }
}

fn build_deep_inspection(
    ctx: &HandlerContext,
    request: &MemoryStatusRequest,
    chat_id: &str,
) -> Result<MemoryDeepInspection, std::io::Error> {
    let profile = request.memory_system_kind.memory_profile();
    let channel = request.channel.clone().unwrap_or_default();
    let summary_with_count = ctx
        .platform
        .session_summary_store()
        .get_with_count(chat_id)
        .map_err(std::io::Error::other)?;
    let summary_text = summary_with_count
        .as_ref()
        .map(|(summary, _)| summary.as_str());
    let summary_message_count = summary_with_count
        .as_ref()
        .map(|(_, count)| *count)
        .unwrap_or_default();
    let recent = ctx
        .session_store
        .load_recent(chat_id, MEMORY_STATUS_RECENT_MESSAGES)
        .map_err(std::io::Error::other)?;
    let message_count = ctx
        .session_store
        .message_count(chat_id)
        .unwrap_or(recent.len());
    let execution_state_present = ctx
        .platform
        .execution_state_store()
        .get(chat_id)
        .map_err(std::io::Error::other)?
        .is_some_and(|state| state.is_meaningful());
    let snapshot = export_continuity_snapshot(
        ContinuitySnapshotExportContext {
            long_term_memory_store: ctx.platform.long_term_memory_store().as_ref(),
            session_summary_store: ctx.platform.session_summary_store().as_ref(),
            execution_state_store: ctx.platform.execution_state_store().as_ref(),
            self_model_store: ctx.platform.self_model_store().as_ref(),
            self_authored_core_store: ctx.platform.self_authored_core_store().as_ref(),
            core_revision_ledger_store: ctx.platform.core_revision_ledger_store().as_ref(),
            self_continuity_store: ctx.platform.self_continuity_store().as_ref(),
            relationship_constitution_store: ctx
                .platform
                .relationship_constitution_store()
                .as_ref(),
            relationship_portfolio_store: ctx.platform.relationship_portfolio_store().as_ref(),
            relationship_topology_store: ctx.platform.relationship_topology_store().as_ref(),
        },
        chat_id,
        request.snapshot_mode,
        current_unix_secs(),
    )
    .map_err(std::io::Error::other)?;
    let intelligence_replay =
        inspect_intelligence_replay(ctx.platform.turn_ledger_store().as_ref(), chat_id, 12)
            .map_err(std::io::Error::other)?;
    let recall = inspect_working_recall(WorkingRecallInspectionInput {
        chat_id,
        query: request.query.as_str(),
        summary_text,
        recent: &recent,
        system_max_len: MEMORY_STATUS_RECALL_SYSTEM_MAX_LEN,
        profile,
        current_channel: (!channel.trim().is_empty()).then_some(channel.as_str()),
        session_store: ctx.session_store.as_ref(),
        memory_store: ctx.memory_store.as_ref(),
        long_term_memory_store: ctx.platform.long_term_memory_store().as_ref(),
        continuity_capsule_store: ctx.platform.continuity_capsule_store().as_ref(),
        turn_ledger_store: ctx.platform.turn_ledger_store().as_ref(),
        skill_storage: Some(ctx.skill_storage.as_ref()),
        task_run_store: Some(ctx.platform.task_run_store().as_ref()),
        task_learning_store: Some(ctx.platform.task_learning_store().as_ref()),
    });
    let hygiene = inspect_memory_hygiene(
        MemoryHygieneContext {
            session_store: ctx.session_store.as_ref(),
            session_summary_store: ctx.platform.session_summary_store().as_ref(),
            memory_store: ctx.memory_store.as_ref(),
            turn_ledger_store: ctx.platform.turn_ledger_store().as_ref(),
            long_term_memory_store: ctx.platform.long_term_memory_store().as_ref(),
            skill_storage: ctx.skill_storage.as_ref(),
        },
        chat_id,
        profile,
        current_unix_secs(),
    );
    let (task_learning, task_workspace) = if channel.trim().is_empty() {
        (None, None)
    } else {
        (
            Some(inspect_task_learning(
                ctx.platform.task_learning_store().as_ref(),
                channel.as_str(),
                chat_id,
                request.query.as_str(),
            )),
            Some(inspect_task_workspace(
                ctx.platform.task_run_store().as_ref(),
                ctx.platform.task_artifact_store().as_ref(),
                ctx.platform.task_execution_ledger_store().as_ref(),
                ctx.platform.task_learning_store().as_ref(),
                channel.as_str(),
                chat_id,
                request.run_id.as_deref(),
            )),
        )
    };
    Ok(MemoryDeepInspection {
        target: MemoryInspectionTarget {
            chat_id: chat_id.to_string(),
            channel,
            query: request.query.clone(),
            memory_system_kind: request.memory_system_kind.as_str().to_string(),
            run_id: request.run_id.clone(),
            message_count,
            recent_message_count: recent.len(),
            summary_present: summary_text.is_some(),
            summary_message_count,
            execution_state_present,
        },
        snapshot_preview: MemorySnapshotPreview {
            mode: match request.snapshot_mode {
                ContinuitySnapshotMode::Bootstrap => "bootstrap",
                ContinuitySnapshotMode::FullRestore => "full_restore",
            }
            .to_string(),
            manifest: snapshot.manifest,
        },
        intelligence_replay,
        recall,
        hygiene,
        task_learning,
        task_workspace,
    })
}

fn parse_request(ctx: &HandlerContext, uri: &str) -> MemoryStatusRequest {
    let chat_id = query_param_from_uri(uri, "chat_id");
    let deep = query_flag_from_uri(uri, "deep");
    let channel = query_param_from_uri(uri, "channel").or_else(|| {
        deep.then(|| {
            chat_id.as_ref().and_then(|_| {
                let config = ctx.config();
                let enabled_channel = config.enabled_channel.trim();
                (!enabled_channel.is_empty()).then_some(enabled_channel.to_string())
            })
        })
        .flatten()
    });
    MemoryStatusRequest {
        chat_id,
        channel,
        deep,
        query: query_param_from_uri(uri, "query").unwrap_or_default(),
        run_id: query_param_from_uri(uri, "run_id"),
        memory_system_kind: match query_param_from_uri(uri, "memory_system_kind").as_deref() {
            Some("esp_compact") => MemorySystemKind::EspCompact,
            Some("linux_full") => MemorySystemKind::LinuxFull,
            _ => ctx.platform.memory_system_kind(),
        },
        snapshot_mode: match query_param_from_uri(uri, "snapshot_mode").as_deref() {
            Some("full_restore") => ContinuitySnapshotMode::FullRestore,
            _ => ContinuitySnapshotMode::Bootstrap,
        },
    }
}

fn query_flag_from_uri(uri: &str, key: &str) -> bool {
    let query = uri.find('?').map(|index| &uri[index + 1..]).unwrap_or("");
    query.split('&').any(|pair| {
        let mut it = pair.splitn(2, '=');
        let Some(candidate) = it.next() else {
            return false;
        };
        if !candidate.eq_ignore_ascii_case(key) {
            return false;
        }
        match it.next().map(str::trim) {
            None => true,
            Some("") => true,
            Some("1" | "true" | "yes" | "on") => true,
            Some(_) => false,
        }
    })
}

fn query_param_from_uri(uri: &str, key: &str) -> Option<String> {
    let query = uri.find('?').map(|index| &uri[index + 1..]).unwrap_or("");
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        let Some(candidate) = it.next() else {
            continue;
        };
        if !candidate.eq_ignore_ascii_case(key) {
            continue;
        }
        let Some(value) = it.next() else {
            continue;
        };
        if value.trim().is_empty() {
            continue;
        }
        return Some(percent_decode_query(value).trim().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::body;
    use crate::config::AppConfig;
    use crate::error::{Error, Result};
    use crate::memory::{ExecutionState, ExecutionStatus, SelfAuthoredCore};
    use crate::platform::{Platform, SkillStorage};
    use crate::task_execution::{
        build_task_run_record, TaskArtifact, TaskArtifactKind, TaskArtifactRecord,
        TaskExecutionLedgerEntry, TaskExecutionRoute, TaskLearningCandidateState, TaskLearningKind,
        TaskLearningRecord, TaskLearningRoute, TaskLedgerKind, TaskPlannerDecision,
        TaskPlannerStepDraft, TaskRunStatus,
    };
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct TestSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for TestSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[test]
    fn memory_status_api_returns_structured_operator_snapshot() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let payload = body(&ctx, "/api/memory/status").unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["memory_system_kind"], "linux_full");
        assert!(parsed.get("inbound_depth").is_none());
        assert!(parsed.get("outbound_depth").is_none());
        assert!(parsed.get("stores").is_some());
        assert!(parsed["stores"].get("session_count").is_none());
        assert!(parsed.get("personality").is_some());
        assert!(parsed.get("continuity_tooling").is_some());
        assert!(parsed.get("task_execution").is_some());
        assert!(parsed.get("memory_len").is_some());
        assert!(parsed.get("long_term_count").is_some());
        assert!(parsed["task_execution"]["learning"]
            .get("candidate_observed")
            .is_some());
        assert!(parsed["task_execution"]["learning"]
            .get("candidate_promoted")
            .is_some());
        assert!(parsed["task_execution"]["learning"]
            .get("candidate_rejected")
            .is_some());
        assert!(parsed["operator_surface"].get("inspect").is_some());
        assert!(parsed["operator_surface"].get("trace").is_some());
        assert!(parsed["operator_surface"].get("diff").is_some());
        assert!(parsed["operator_surface"].get("repair").is_some());
        assert!(parsed["operator_surface"].get("policy_view").is_some());
        assert_eq!(parsed["diagnosis"]["kind"].as_str(), Some("memory_runtime"));
        assert!(parsed["diagnosis"].get("summary").is_some());
    }

    #[test]
    fn memory_status_api_surfaces_idle_forge_latest_decode_errors() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let rel_path = "memory/idle_forge/latest.json";
        let previous = ctx.platform.state_fs().read(rel_path).unwrap();
        ctx.platform
            .state_fs()
            .write(rel_path, br#"{"not":"valid""#)
            .unwrap();

        let result = body(&ctx, "/api/memory/status");

        match previous {
            Some(bytes) => ctx.platform.state_fs().write(rel_path, &bytes).unwrap(),
            None => ctx.platform.state_fs().remove(rel_path).unwrap(),
        }

        let error = result.expect_err("corrupt idle forge summary must fail memory status");
        assert!(
            error
                .to_string()
                .contains("idle_memory_forge_latest_decode"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn memory_status_api_supports_targeted_inspection() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let unique = unique_suffix();
        let chat_id = format!("memory-status-chat-{unique}");
        let run_id = format!("r{unique}");
        let topic = format!("apply_release_patch_{unique}");
        let now_secs = crate::util::current_unix_secs();

        ctx.session_store
            .append(&chat_id, "user", "Please finish the release patch flow.")
            .unwrap();
        ctx.session_store
            .append(&chat_id, "assistant", "Working on the release patch now.")
            .unwrap();
        ctx.platform
            .session_summary_store()
            .set_with_count(&chat_id, "Release patch work is in progress.", 2)
            .unwrap();
        ctx.platform
            .execution_state_store()
            .set(
                &chat_id,
                &ExecutionState {
                    status: ExecutionStatus::Active,
                    goal: "ship release patch".to_string(),
                    progress: "workspace prepared".to_string(),
                    blocker: String::new(),
                    next_action: "apply patch".to_string(),
                    last_output: "diff staged".to_string(),
                    updated_at: now_secs,
                    ..ExecutionState::default()
                },
            )
            .unwrap();
        ctx.platform
            .self_authored_core_store()
            .set(
                crate::memory::board_subject_scope_id(),
                &SelfAuthoredCore {
                    revision: 3,
                    identity_anchor: "beetle".to_string(),
                    priority_constitution: vec!["complete operator-grade memory work".to_string()],
                    updated_at: now_secs,
                    ..SelfAuthoredCore::default()
                },
            )
            .unwrap();

        let decision = TaskPlannerDecision {
            route: TaskExecutionRoute::StartRun,
            reason: "operator inspection".to_string(),
            title: "Release patch".to_string(),
            goal: "Apply the release patch".to_string(),
            completion_definition: "Patch is applied and verified.".to_string(),
            risk_notes: vec!["Keep the task workspace coherent.".to_string()],
            steps: vec![TaskPlannerStepDraft {
                title: "Apply patch".to_string(),
                instruction: "Update the release files and verify the output.".to_string(),
                tool_budget: 2,
                retry_budget: 1,
                expected_artifacts: vec!["patch".to_string()],
                review_criteria: vec!["patch applies cleanly".to_string()],
            }],
        };
        let run = build_task_run_record(
            &run_id,
            "telegram",
            &chat_id,
            "Apply the release patch",
            &decision,
            now_secs,
        )
        .unwrap();
        ctx.platform.task_run_store().upsert(&run).unwrap();
        ctx.platform
            .task_artifact_store()
            .put(&TaskArtifactRecord {
                artifact: TaskArtifact {
                    artifact_id: format!("a{unique}"),
                    run_id: run_id.clone(),
                    step_id: run.plan.ordered_steps[0].step_id.clone(),
                    kind: TaskArtifactKind::StepResult,
                    summary: "Applied the release patch.".to_string(),
                    content_ref: "inline".to_string(),
                    provenance: "memory handler test".to_string(),
                    created_at: now_secs,
                },
                content: "Patch content".to_string(),
            })
            .unwrap();
        ctx.platform
            .task_execution_ledger_store()
            .append(
                &run_id,
                &TaskExecutionLedgerEntry {
                    sequence: 1,
                    run_id: run_id.clone(),
                    step_id: run.plan.ordered_steps[0].step_id.clone(),
                    kind: TaskLedgerKind::StepStarted,
                    run_status: TaskRunStatus::Running,
                    message: "Patch step started".to_string(),
                    recorded_at: now_secs,
                },
            )
            .unwrap();
        ctx.platform
            .task_learning_store()
            .upsert(&TaskLearningRecord {
                learning_id: format!("l{unique}"),
                source_channel: "telegram".to_string(),
                source_chat_id: chat_id.clone(),
                run_id: run_id.clone(),
                step_id: run.plan.ordered_steps[0].step_id.clone(),
                kind: TaskLearningKind::ReusableProcedure,
                route: TaskLearningRoute::RuntimeSkill,
                run_status: TaskRunStatus::Completed,
                topic: topic.clone(),
                summary: "Apply release patch after validating artifact output.".to_string(),
                content: "Validate diff, apply patch, verify output.".to_string(),
                memory_kind: None,
                review_summary: "Reusable".to_string(),
                source_artifact_ids: vec![format!("a{unique}")],
                provenance: "memory handler test".to_string(),
                archive_note_name: String::new(),
                route_detail: "exact topic match".to_string(),
                candidate_state: Some(TaskLearningCandidateState::Promoted),
                candidate_state_updated_at: now_secs,
                last_failure_reason: String::new(),
                observed_at: now_secs,
            })
            .unwrap();
        for ledger in [
            crate::memory::TurnLedger {
                req_id: format!("{run_id}-4"),
                channel: "telegram".to_string(),
                ingress: crate::bus::IngressKind::User,
                status: crate::memory::TurnLedgerStatus::Answered,
                observation: Some(crate::memory::TurnObservationLedger {
                    execution_class: crate::memory::TurnExecutionClass::ToolAssisted,
                    deliberation_class: crate::memory::TurnDeliberationClass::Standard,
                    final_outcome: "final_recovery".to_string(),
                    pressure: crate::memory::TurnPersonaPressureLevel::Cautious,
                    mode: crate::memory::TurnModeSnapshotLedger {
                        current_mode: "normal".to_string(),
                        allow_non_voice_outbound: true,
                        allow_idle_self_runtime: true,
                    },
                    tool_path: crate::memory::TurnToolPathLedger {
                        path: "tool_recovery".to_string(),
                        tool_calls: 2,
                        react_rounds: 2,
                        current_primary_delivered: false,
                        final_answer_recovered: true,
                    },
                    blocker: Some(crate::memory::TurnBlockerLedger {
                        kind: "retryable".to_string(),
                        failed_calls: 1,
                        total_calls: 1,
                    }),
                }),
                finished_at_ms: (now_secs + 4) * 1000,
                ..crate::memory::TurnLedger::default()
            },
            crate::memory::TurnLedger {
                req_id: format!("{run_id}-3"),
                channel: "telegram".to_string(),
                ingress: crate::bus::IngressKind::User,
                status: crate::memory::TurnLedgerStatus::Answered,
                subject_state: Some(crate::memory::TurnSubjectStateLedger {
                    governance_mode: "adaptive".to_string(),
                    response_mode: "protective_brief".to_string(),
                    task_scope: "narrow".to_string(),
                    ..crate::memory::TurnSubjectStateLedger::default()
                }),
                observation: Some(crate::memory::TurnObservationLedger {
                    execution_class: crate::memory::TurnExecutionClass::ToolAssisted,
                    deliberation_class: crate::memory::TurnDeliberationClass::Standard,
                    final_outcome: "final_answer".to_string(),
                    pressure: crate::memory::TurnPersonaPressureLevel::Cautious,
                    mode: crate::memory::TurnModeSnapshotLedger {
                        current_mode: "normal".to_string(),
                        allow_non_voice_outbound: true,
                        allow_idle_self_runtime: true,
                    },
                    tool_path: crate::memory::TurnToolPathLedger {
                        path: "tool_reply".to_string(),
                        tool_calls: 1,
                        react_rounds: 1,
                        current_primary_delivered: false,
                        final_answer_recovered: false,
                    },
                    blocker: Some(crate::memory::TurnBlockerLedger {
                        kind: "retryable".to_string(),
                        failed_calls: 1,
                        total_calls: 1,
                    }),
                }),
                finished_at_ms: (now_secs + 3) * 1000,
                ..crate::memory::TurnLedger::default()
            },
            crate::memory::TurnLedger {
                req_id: format!("{run_id}-2"),
                channel: "telegram".to_string(),
                ingress: crate::bus::IngressKind::User,
                status: crate::memory::TurnLedgerStatus::Answered,
                observation: Some(crate::memory::TurnObservationLedger {
                    execution_class: crate::memory::TurnExecutionClass::ToolAssisted,
                    deliberation_class: crate::memory::TurnDeliberationClass::Standard,
                    final_outcome: "final_recovery".to_string(),
                    pressure: crate::memory::TurnPersonaPressureLevel::Cautious,
                    mode: crate::memory::TurnModeSnapshotLedger {
                        current_mode: "normal".to_string(),
                        allow_non_voice_outbound: true,
                        allow_idle_self_runtime: true,
                    },
                    tool_path: crate::memory::TurnToolPathLedger {
                        path: "tool_recovery".to_string(),
                        tool_calls: 2,
                        react_rounds: 2,
                        current_primary_delivered: false,
                        final_answer_recovered: true,
                    },
                    blocker: Some(crate::memory::TurnBlockerLedger {
                        kind: "capability".to_string(),
                        failed_calls: 1,
                        total_calls: 1,
                    }),
                }),
                finished_at_ms: (now_secs + 2) * 1000,
                ..crate::memory::TurnLedger::default()
            },
            crate::memory::TurnLedger {
                req_id: format!("{run_id}-1"),
                channel: "telegram".to_string(),
                ingress: crate::bus::IngressKind::User,
                status: crate::memory::TurnLedgerStatus::Answered,
                subject_state: Some(crate::memory::TurnSubjectStateLedger {
                    governance_mode: "adaptive".to_string(),
                    response_mode: "steady".to_string(),
                    task_scope: "brief".to_string(),
                    ..crate::memory::TurnSubjectStateLedger::default()
                }),
                observation: Some(crate::memory::TurnObservationLedger {
                    execution_class: crate::memory::TurnExecutionClass::DirectReply,
                    deliberation_class: crate::memory::TurnDeliberationClass::FastInteractive,
                    final_outcome: "final_answer".to_string(),
                    pressure: crate::memory::TurnPersonaPressureLevel::Normal,
                    mode: crate::memory::TurnModeSnapshotLedger {
                        current_mode: "normal".to_string(),
                        allow_non_voice_outbound: true,
                        allow_idle_self_runtime: true,
                    },
                    tool_path: crate::memory::TurnToolPathLedger {
                        path: String::new(),
                        tool_calls: 0,
                        react_rounds: 1,
                        current_primary_delivered: true,
                        final_answer_recovered: false,
                    },
                    blocker: None,
                }),
                finished_at_ms: (now_secs + 1) * 1000,
                ..crate::memory::TurnLedger::default()
            },
        ] {
            ctx.platform
                .turn_ledger_store()
                .set(&chat_id, &ledger)
                .unwrap();
        }

        let uri = format!(
            "/api/memory/status?chat_id={chat_id}&channel=telegram&query={topic}&run_id={run_id}&snapshot_mode=full_restore&deep=1"
        );
        let payload = body(&ctx, &uri).unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        let inspection = &parsed["inspection"];
        assert_eq!(inspection["target"]["chat_id"], chat_id);
        assert_eq!(inspection["target"]["channel"], "telegram");
        assert_eq!(inspection["snapshot_preview"]["mode"], "full_restore");
        assert!(
            inspection["intelligence_replay"]["total_turns"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert_eq!(
            inspection["intelligence_replay"]["latest_response_mode"],
            "steady"
        );
        assert!(inspection["intelligence_replay"]["recent_turns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["req_id"] == format!("{run_id}-1")));
        assert_eq!(inspection["recall"]["chat_id"], chat_id);
        assert_eq!(inspection["hygiene"]["current_chat_id"], chat_id);
        assert_eq!(inspection["task_workspace"]["run_id"], run_id);
        let expected_backend = if cfg!(target_os = "linux") {
            "task_learning_sqlite_fts_hybrid"
        } else {
            "task_learning_heuristic"
        };
        assert_eq!(inspection["task_learning"]["backend"], expected_backend);
        assert_eq!(
            inspection["task_learning"]["route_counts"]["runtime_skill"],
            1
        );
        assert!(
            parsed["task_execution"]["learning"]["candidate_promoted"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(inspection["task_learning"]["scored_hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["topic"] == topic
                && item["candidate_state"] == "promoted"
                && item["reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reason| reason == "promoted procedure")));
        assert!(inspection["task_learning"]["related_hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["topic"] == topic));
    }

    #[test]
    fn memory_status_api_includes_learning_summary_and_metrics() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let unique = unique_suffix();
        let now_secs: u64 = 4_102_444_800;
        let topic = format!("release_patch_flow_{unique}");
        let skill_name = format!("runtime_skill__{topic}");

        crate::skills::upsert_runtime_skill(
            ctx.skill_storage.as_ref(),
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Release patch flow".to_string(),
                summary: "Validated release patch flow.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: now_secs.saturating_sub(60),
            },
        )
        .unwrap();
        crate::skills::record_runtime_skill_outcomes(
            ctx.skill_storage.as_ref(),
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            now_secs,
            "final_answer",
        )
        .unwrap();
        ctx.platform
            .task_learning_store()
            .upsert(&TaskLearningRecord {
                learning_id: format!("learning-{unique}"),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-1".to_string(),
                run_id: format!("run-{unique}"),
                step_id: "s01".to_string(),
                kind: TaskLearningKind::ReusableProcedure,
                route: TaskLearningRoute::RuntimeSkill,
                run_status: TaskRunStatus::Completed,
                topic,
                summary: "Promoted release patch procedure.".to_string(),
                content: "Inspect diff, patch, verify.".to_string(),
                memory_kind: None,
                review_summary: "Reusable".to_string(),
                source_artifact_ids: Vec::new(),
                provenance: "memory status test".to_string(),
                archive_note_name: String::new(),
                route_detail: "promoted".to_string(),
                candidate_state: Some(TaskLearningCandidateState::Promoted),
                candidate_state_updated_at: now_secs,
                last_failure_reason: String::new(),
                observed_at: now_secs,
            })
            .unwrap();

        let payload = body(&ctx, "/api/memory/status").unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert!(
            parsed["learning"]["runtime_skills"]["validated"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert_eq!(
            parsed["learning"]["metrics"]["validated_runtime_skills"],
            parsed["learning"]["runtime_skills"]["validated"]
        );
        assert_eq!(
            parsed["learning"]["metrics"]["garbage_collectable_experience_crystals"],
            parsed["learning"]["experience_crystals"]["garbage_collectable"]
        );
        assert_eq!(
            parsed["learning"]["experience_crystals"]["promoted_candidates"],
            parsed["learning"]["task_candidates"]["candidate_promoted"]
        );
        assert_eq!(
            parsed["learning"]["experience_crystals"]["pending_candidates"],
            parsed["learning"]["task_candidates"]["candidate_observed"]
        );
        assert_eq!(
            parsed["learning"]["experience_crystals"]["rejected_candidates"],
            parsed["learning"]["task_candidates"]["candidate_rejected"]
        );
        assert!(
            parsed["learning"]["task_candidates"]["candidate_promoted"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );
        assert!(parsed["learning"]["runtime_skills"]["recent_records"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == skill_name
                && item["validated_success_count"] == 1
                && item["last_outcome_note"] == "final_answer"));
    }

    #[test]
    fn build_test_context_uses_isolated_runtime_skill_storage() {
        let _guard = memory_status_test_guard();
        let ctx_a = build_test_context();
        let ctx_b = build_test_context();
        let unique = unique_suffix();
        let skill_name = format!("runtime_skill__isolation_{unique}");

        crate::skills::write_skill(
            ctx_a.skill_storage.as_ref(),
            &skill_name,
            "## isolated\nonly visible in ctx_a",
        )
        .unwrap();

        assert!(
            crate::skills::get_skill_content(ctx_a.skill_storage.as_ref(), &skill_name).is_some()
        );
        assert!(
            crate::skills::get_skill_content(ctx_b.skill_storage.as_ref(), &skill_name).is_none()
        );
    }

    #[test]
    fn memory_status_api_includes_continuity_capsule_summary() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let now_secs = crate::util::current_unix_secs();
        ctx.platform
            .continuity_capsule_store()
            .upsert_many(
                &[
                    crate::memory::ContinuityCapsuleDraft {
                        scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                        scope_id: "chat-1".to_string(),
                        source_chat_id: "chat-1".to_string(),
                        topic: "resume release work".to_string(),
                        next_step: "apply final patch".to_string(),
                        source: crate::memory::ContinuityCapsuleSource::PostReplyMaintenance,
                        status: crate::memory::ContinuityCapsuleStatus::Active,
                        observed_at: now_secs,
                        ..Default::default()
                    },
                    crate::memory::ContinuityCapsuleDraft {
                        scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                        scope_id: "chat-2".to_string(),
                        source_chat_id: "chat-2".to_string(),
                        topic: "resume after reboot".to_string(),
                        next_step: "restore context".to_string(),
                        source: crate::memory::ContinuityCapsuleSource::RebootContinuity,
                        status: crate::memory::ContinuityCapsuleStatus::Active,
                        observed_at: now_secs.saturating_sub(10),
                        ..Default::default()
                    },
                    crate::memory::ContinuityCapsuleDraft {
                        scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                        scope_id: "chat-3".to_string(),
                        source_chat_id: "chat-3".to_string(),
                        topic: "completed release".to_string(),
                        outcome: "done".to_string(),
                        source: crate::memory::ContinuityCapsuleSource::TaskCompletion,
                        status: crate::memory::ContinuityCapsuleStatus::Done,
                        observed_at: now_secs.saturating_sub(20),
                        ..Default::default()
                    },
                ],
                now_secs,
            )
            .unwrap();

        let payload = body(&ctx, "/api/memory/status").unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(parsed["continuity_capsules"]["total"], 3);
        assert_eq!(parsed["continuity_capsules"]["active"], 2);
        assert_eq!(parsed["continuity_capsules"]["done"], 1);
        assert_eq!(parsed["continuity_capsules"]["post_reply"], 1);
        assert_eq!(parsed["continuity_capsules"]["reboot_continuity"], 1);
        assert_eq!(parsed["continuity_capsules"]["task_completion"], 1);
        assert!(parsed["continuity_capsules"]["recent_capsules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["topic"] == "resume release work"));
    }

    #[test]
    fn memory_status_api_regression_keeps_optional_inspection_boundary() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let unique = unique_suffix();
        let chat_id = format!("memory-status-boundary-{unique}");

        ctx.session_store
            .append(&chat_id, "user", "Show me the memory diagnosis.")
            .unwrap();
        ctx.session_store
            .append(&chat_id, "assistant", "Memory diagnosis is ready.")
            .unwrap();
        ctx.platform
            .session_summary_store()
            .set_with_count(&chat_id, "Memory diagnosis chat.", 2)
            .unwrap();

        let default_payload = body(&ctx, "/api/memory/status").unwrap();
        let default_parsed: Value = serde_json::from_str(&default_payload).unwrap();
        assert!(default_parsed.get("inspection").is_none());

        let targeted_without_deep = body(
            &ctx,
            &format!("/api/memory/status?chat_id={chat_id}&channel=telegram&query=memory"),
        )
        .unwrap();
        let targeted_without_deep_parsed: Value =
            serde_json::from_str(&targeted_without_deep).unwrap();
        assert!(targeted_without_deep_parsed.get("inspection").is_none());

        let targeted_payload = body(
            &ctx,
            &format!("/api/memory/status?chat_id={chat_id}&channel=telegram&query=memory&deep=1"),
        )
        .unwrap();
        let targeted_parsed: Value = serde_json::from_str(&targeted_payload).unwrap();
        assert_eq!(targeted_parsed["inspection"]["target"]["chat_id"], chat_id);
        assert_eq!(
            targeted_parsed["inspection"]["target"]["summary_present"],
            Value::Bool(true)
        );
        assert_eq!(
            targeted_parsed["operator_surface"]["trace"]["inspection_target"]["chat_id"],
            chat_id
        );
        assert!(targeted_parsed["operator_surface"]["trace"]
            .get("recall")
            .is_some());
    }

    #[test]
    fn parse_request_skips_default_channel_when_chat_id_is_absent() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        {
            let mut config = ctx.cached_config.write().unwrap_or_else(|e| e.into_inner());
            config.enabled_channel = "telegram".to_string();
        }

        let request = super::parse_request(&ctx, "/api/memory/status");

        assert!(request.chat_id.is_none());
        assert!(request.channel.is_none());
    }

    #[test]
    fn parse_request_keeps_target_channel_unset_without_deep() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        {
            let mut config = ctx.cached_config.write().unwrap_or_else(|e| e.into_inner());
            config.enabled_channel = "telegram".to_string();
        }

        let request = super::parse_request(&ctx, "/api/memory/status?chat_id=chat-1");

        assert_eq!(request.chat_id.as_deref(), Some("chat-1"));
        assert!(!request.deep);
        assert!(request.channel.is_none());
    }

    #[test]
    fn parse_request_defaults_target_channel_when_deep_inspection_is_enabled() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        {
            let mut config = ctx.cached_config.write().unwrap_or_else(|e| e.into_inner());
            config.enabled_channel = "telegram".to_string();
        }

        let request = super::parse_request(&ctx, "/api/memory/status?chat_id=chat-1&deep=1");

        assert_eq!(request.chat_id.as_deref(), Some("chat-1"));
        assert!(request.deep);
        assert_eq!(request.channel.as_deref(), Some("telegram"));
    }

    #[test]
    fn embedded_memory_status_requires_explicit_operator_window_for_deep_inspection() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let unique = unique_suffix();
        let chat_id = format!("memory-status-embedded-{unique}");

        ctx.session_store
            .append(&chat_id, "user", "Inspect the embedded memory state.")
            .unwrap();
        ctx.session_store
            .append(&chat_id, "assistant", "Embedded memory state is ready.")
            .unwrap();

        let error = body(
            &ctx,
            &format!(
                "/api/memory/status?chat_id={chat_id}&channel=telegram&query=memory&memory_system_kind=esp_compact&deep=1"
            ),
        )
        .expect_err("embedded deep inspection should require operator window");

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn legacy_profile_param_no_longer_overrides_memory_system_kind() {
        let _guard = memory_status_test_guard();
        let ctx = build_test_context();
        let unique = unique_suffix();
        let chat_id = format!("memory-status-legacy-profile-{unique}");

        ctx.session_store
            .append(
                &chat_id,
                "user",
                "Inspect memory without legacy profile override.",
            )
            .unwrap();
        ctx.session_store
            .append(&chat_id, "assistant", "Memory inspection is ready.")
            .unwrap();

        let payload = body(
            &ctx,
            &format!(
                "/api/memory/status?chat_id={chat_id}&channel=telegram&query=memory&profile=embedded&deep=1"
            ),
        )
        .expect("legacy profile should be ignored");
        let parsed: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(parsed["memory_system_kind"], "linux_full");
    }

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let config_store = platform.config_store();
        let skill_storage: Arc<dyn SkillStorage + Send + Sync> =
            Arc::new(TestSkillStorage::default());
        crate::platform::http_server::handlers::build_test_handler_context(
            config,
            platform,
            config_store,
            skill_storage,
            crate::platform::http_server::handlers::ControlPlaneRouteContract::FULL,
            "test",
        )
    }

    fn unique_suffix() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", nanos % 0xffff_ffff)
    }

    fn memory_status_test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::platform::http_server::handlers::default_test_handler_context_guard()
    }
}
