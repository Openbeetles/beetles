//! GET /api/memory/status: operator-facing memory snapshot and optional deep inspection.

use super::HandlerContext;
use crate::memory::{
    board_subject_scope_id, compute_core_revision_governance_digest, export_continuity_snapshot,
    inspect_intelligence_replay, inspect_memory_hygiene, inspect_working_recall,
    ContinuitySnapshotExportContext, ContinuitySnapshotManifest, ContinuitySnapshotMode,
    IntelligenceReplayInspection, MemoryHygieneContext, MemoryHygieneInspection, MemoryProfile,
    WorkingRecallInspection, WorkingRecallInspectionInput,
};
use crate::skills::is_runtime_skill_name;
use crate::task_execution::{
    build_task_execution_operator_snapshot, inspect_task_learning, inspect_task_workspace,
    TaskExecutionOperatorSnapshot, TaskLearningInspection, TaskWorkspaceInspection,
};
use crate::util::{current_unix_secs, percent_decode_query};
use serde::Serialize;
use std::sync::atomic::Ordering;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";
const MEMORY_STATUS_RECALL_SYSTEM_MAX_LEN: usize = 2_400;
const MEMORY_STATUS_RECENT_MESSAGES: usize = 24;

#[derive(Debug, Serialize)]
struct MemoryStoreStatus {
    session_count: usize,
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
    profile: String,
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
struct MemoryStatusBody {
    memory_profile: String,
    inbound_depth: usize,
    outbound_depth: usize,
    memory_len: usize,
    soul_len: usize,
    user_len: usize,
    long_term_count: usize,
    continuity_capsule_count: usize,
    stores: MemoryStoreStatus,
    personality: MemoryPersonalityStatus,
    continuity_tooling: MemoryContinuityTooling,
    task_execution: TaskExecutionOperatorSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    inspection: Option<MemoryDeepInspection>,
}

struct MemoryStatusRequest {
    chat_id: Option<String>,
    channel: Option<String>,
    deep: bool,
    query: String,
    run_id: Option<String>,
    profile: MemoryProfile,
    snapshot_mode: ContinuitySnapshotMode,
}

/// Generate a structured memory/operator JSON body.
pub fn body(ctx: &HandlerContext, uri: &str) -> Result<String, std::io::Error> {
    let request = parse_request(ctx, uri);
    let subject_id = board_subject_scope_id();
    let chat_ids = ctx
        .session_store
        .list_chat_ids()
        .map_err(std::io::Error::other)?;
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
    let inspection = request
        .deep
        .then_some(request.chat_id.as_deref())
        .flatten()
        .map(|chat_id| build_deep_inspection(ctx, &request, chat_id))
        .transpose()?;
    let payload = MemoryStatusBody {
        memory_profile: memory_profile_label(ctx.platform.memory_profile()).to_string(),
        inbound_depth: ctx.inbound_depth.load(Ordering::Relaxed),
        outbound_depth: ctx.outbound_depth.load(Ordering::Relaxed),
        memory_len,
        soul_len,
        user_len,
        long_term_count,
        continuity_capsule_count,
        stores: MemoryStoreStatus {
            session_count: chat_ids.len(),
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
        task_execution,
        inspection,
    };
    serde_json::to_string(&payload).map_err(std::io::Error::other)
}

fn build_deep_inspection(
    ctx: &HandlerContext,
    request: &MemoryStatusRequest,
    chat_id: &str,
) -> Result<MemoryDeepInspection, std::io::Error> {
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
        profile: request.profile,
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
        request.profile,
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
            profile: memory_profile_label(request.profile).to_string(),
            run_id: request.run_id.clone(),
            message_count,
            recent_message_count: recent.len(),
            summary_present: summary_text.is_some(),
            summary_message_count,
            execution_state_present,
        },
        snapshot_preview: MemorySnapshotPreview {
            mode: continuity_snapshot_mode_label(request.snapshot_mode).to_string(),
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
        profile: parse_memory_profile(
            query_param_from_uri(uri, "profile").as_deref(),
            ctx.platform.memory_profile(),
        ),
        snapshot_mode: parse_snapshot_mode(query_param_from_uri(uri, "snapshot_mode").as_deref()),
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

fn parse_memory_profile(value: Option<&str>, fallback: MemoryProfile) -> MemoryProfile {
    match value.map(str::trim) {
        Some("embedded") => MemoryProfile::Embedded,
        Some("standard") => MemoryProfile::Standard,
        _ => fallback,
    }
}

fn parse_snapshot_mode(value: Option<&str>) -> ContinuitySnapshotMode {
    match value.map(str::trim) {
        Some("full_restore") => ContinuitySnapshotMode::FullRestore,
        _ => ContinuitySnapshotMode::Bootstrap,
    }
}

fn memory_profile_label(profile: MemoryProfile) -> &'static str {
    match profile {
        MemoryProfile::Embedded => "embedded",
        MemoryProfile::Standard => "standard",
    }
}

fn continuity_snapshot_mode_label(mode: ContinuitySnapshotMode) -> &'static str {
    match mode {
        ContinuitySnapshotMode::Bootstrap => "bootstrap",
        ContinuitySnapshotMode::FullRestore => "full_restore",
    }
}

#[cfg(test)]
mod tests {
    use super::body;
    use crate::config::AppConfig;
    use crate::memory::{ExecutionState, ExecutionStatus, SelfAuthoredCore};
    use crate::platform::Platform;
    use crate::task_execution::{
        build_task_run_record, TaskArtifact, TaskArtifactKind, TaskArtifactRecord,
        TaskExecutionLedgerEntry, TaskExecutionRoute, TaskLearningKind, TaskLearningRecord,
        TaskLearningRoute, TaskLedgerKind, TaskPlannerDecision, TaskPlannerStepDraft,
        TaskRunStatus,
    };
    use serde_json::Value;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn memory_status_api_returns_structured_operator_snapshot() {
        let ctx = build_test_context();
        let payload = body(&ctx, "/api/memory/status").unwrap();
        let parsed: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["memory_profile"], "standard");
        assert!(parsed.get("stores").is_some());
        assert!(parsed.get("personality").is_some());
        assert!(parsed.get("continuity_tooling").is_some());
        assert!(parsed.get("task_execution").is_some());
        assert!(parsed.get("memory_len").is_some());
        assert!(parsed.get("long_term_count").is_some());
    }

    #[test]
    fn memory_status_api_supports_targeted_inspection() {
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
        assert_eq!(
            inspection["task_learning"]["backend"],
            "task_learning_heuristic"
        );
        assert_eq!(
            inspection["task_learning"]["route_counts"]["runtime_skill"],
            1
        );
        assert!(inspection["task_learning"]["scored_hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["topic"] == topic
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
    fn memory_status_api_regression_keeps_optional_inspection_boundary() {
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
    }

    #[test]
    fn parse_request_skips_default_channel_when_chat_id_is_absent() {
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

    fn build_test_context() -> crate::platform::http_server::handlers::HandlerContext {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let (registry, _) = crate::tools::build_default_registry(
            &config,
            crate::tools::DefaultRegistryDeps {
                platform: Arc::clone(&platform),
                remind_at_store: platform.remind_at_store(),
                session_store: platform.session_store(),
                memory_store: platform.memory_store(),
                long_term_memory_store: platform.long_term_memory_store(),
                turn_ledger_store: platform.turn_ledger_store(),
                private_garden_store: platform.private_garden_store(),
                config_store: platform.config_store(),
            },
        );
        let channel_capability_registry =
            Arc::new(crate::build_channel_capability_registry(&config, false));
        let skill_storage = platform.skill_storage();
        let skill_meta_store = platform.skill_meta_store();
        let skill_prompt_cache = Arc::new(crate::skills::SkillPromptCache::new(
            Arc::clone(&skill_meta_store),
            Arc::clone(&skill_storage),
            8192,
        ));
        crate::platform::http_server::handlers::HandlerContext {
            config_store: platform.config_store(),
            config_file_store: Arc::new(crate::config::PlatformConfigFileStore(Arc::clone(
                &platform,
            ))),
            platform: Arc::clone(&platform),
            memory_store: platform.memory_store(),
            session_store: platform.session_store(),
            skill_storage,
            skill_meta_store,
            skill_prompt_cache,
            tool_registry: Arc::new(registry),
            channel_capability_registry: Arc::clone(&channel_capability_registry),
            capability_package_runtime_capabilities: Arc::new(
                crate::build_capability_package_runtime_capabilities(
                    channel_capability_registry.as_ref(),
                    false,
                ),
            ),
            inbound_depth: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            outbound_depth: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            version: Arc::from("0.0.0"),
            board_id: Arc::from("test"),
            cached_config: Arc::new(std::sync::RwLock::new(config)),
            llm_stream_enabled: false,
        }
    }

    fn unique_suffix() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", nanos % 0xffff_ffff)
    }
}
