//! Idle memory forge contracts and persistence.

use crate::error::{Error, Result};
use crate::memory::{
    board_subject_scope_id, memory_policy, ContinuityCapsuleStore, LongTermMemoryKind,
    LongTermMemoryQuery, LongTermMemorySourceScope, LongTermMemoryStore, MemoryProfile,
    SelfContinuityStore,
};
use crate::platform::StateFs;
use crate::reasoning::{
    build_memory_query_snapshot_from_stores, default_lua_memory_query_capabilities,
    memory_attack_job_contracts, validate_memory_attack_result, validate_memory_query_result,
    CurrentExecutableLuaSandboxExecutor, LuaQueryBudget, LuaQueryRequest, MemoryAttackJobKind,
    MemoryAttackJobReport, MemoryAttackJobStatus, MemoryAttackResult, MemoryQueryContinuityScope,
    MemoryQueryResult, MemoryQuerySelection, ReasoningExecutor,
};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

const IDLE_MEMORY_FORGE_SCHEMA_VERSION: u32 = 2;
const REL_PATH_IDLE_MEMORY_FORGE_LATEST: &str = "memory/idle_forge/latest.json";
const REL_DIR_IDLE_MEMORY_FORGE_RUNS: &str = "memory/idle_forge/runs";
const IDLE_MEMORY_FORGE_DEFAULT_CADENCE_SECS: u64 = 15 * 60;
const MAX_PRIMARY_FINDING_CHARS: usize = 160;
const MAX_JOB_SUMMARY_CHARS: usize = 220;
const IDLE_MEMORY_FORGE_SOURCE_CHANNEL: &str = "idle_memory_forge";
const IDLE_MEMORY_FORGE_SCHEDULE_DELAY_MS: u64 = 1_000;
const IDLE_MEMORY_FORGE_LONG_TERM_LIMIT: usize = 12;
const IDLE_MEMORY_FORGE_CONTINUITY_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleMemoryForgeTrigger {
    CronIdleTick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleMemoryForgeJobKind {
    StaleFactualScan,
    ContinuityConflictScan,
    NearDuplicateFactualScan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleMemoryForgeJobStatus {
    Succeeded,
    NoCandidates,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdleMemoryForgeAdjudicationState {
    #[default]
    Clean,
    RequiresAdjudication,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeJobContract {
    pub kind: IdleMemoryForgeJobKind,
    pub linux_only: bool,
    pub proposal_only: bool,
    pub cadence_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeJobReport {
    pub job_kind: IdleMemoryForgeJobKind,
    pub snapshot_digest: String,
    pub status: IdleMemoryForgeJobStatus,
    pub summary: String,
    pub candidate_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeProposalBatch {
    pub job_kind: IdleMemoryForgeJobKind,
    pub snapshot_digest: String,
    pub result: MemoryQueryResult,
    pub adjudication_state: IdleMemoryForgeAdjudicationState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeAttackBatch {
    pub job_kind: MemoryAttackJobKind,
    pub snapshot_digest: String,
    pub result: MemoryAttackResult,
    pub adjudication_state: IdleMemoryForgeAdjudicationState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeRunLedger {
    pub schema_version: u32,
    pub chat_id: String,
    pub source_channel: String,
    pub trigger: IdleMemoryForgeTrigger,
    pub started_at: u64,
    pub completed_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_finding: Option<String>,
    pub total_candidates: usize,
    pub adjudication_state: IdleMemoryForgeAdjudicationState,
    #[serde(default)]
    pub job_reports: Vec<IdleMemoryForgeJobReport>,
    #[serde(default)]
    pub proposal_batches: Vec<IdleMemoryForgeProposalBatch>,
    #[serde(default)]
    pub attack_job_reports: Vec<MemoryAttackJobReport>,
    #[serde(default)]
    pub attack_batches: Vec<IdleMemoryForgeAttackBatch>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleMemoryForgeOperatorSummary {
    pub last_run_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_source_channel: Option<String>,
    pub total_candidates: usize,
    pub attack_findings: usize,
    pub distillation_candidates: usize,
    pub adjudication_state: IdleMemoryForgeAdjudicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_finding: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdleMemoryForgeAdmissionSnapshot {
    pub allow_periodic_maintenance: bool,
    pub active_agent_tasks: usize,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdleMemoryForgeJobPayload {
    pub trigger: IdleMemoryForgeTrigger,
    pub source_channel: String,
    pub scheduled_at: u64,
}

pub fn idle_memory_forge_job_contracts() -> Vec<IdleMemoryForgeJobContract> {
    [
        IdleMemoryForgeJobKind::StaleFactualScan,
        IdleMemoryForgeJobKind::ContinuityConflictScan,
        IdleMemoryForgeJobKind::NearDuplicateFactualScan,
    ]
    .into_iter()
    .map(|kind| IdleMemoryForgeJobContract {
        kind,
        linux_only: true,
        proposal_only: true,
        cadence_secs: IDLE_MEMORY_FORGE_DEFAULT_CADENCE_SECS,
    })
    .collect()
}

pub fn should_run_idle_memory_forge(snapshot: &IdleMemoryForgeAdmissionSnapshot) -> bool {
    snapshot.allow_periodic_maintenance
        && snapshot.active_agent_tasks == 0
        && snapshot.inbound_depth == 0
        && snapshot.outbound_depth == 0
}

pub(crate) fn enqueue_idle_memory_forge_tick(
    system_inbound_tx: &crate::bus::SystemInboundTx,
    self_continuity_store: &dyn SelfContinuityStore,
    profile: MemoryProfile,
    state_fs: &dyn StateFs,
    now_secs: u64,
) {
    if !cfg!(target_os = "linux") {
        append_idle_memory_forge_workflow_audit(
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_memory_forge_linux_only",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }

    if let Some(reason) = idle_memory_forge_enqueue_block_reason() {
        append_idle_memory_forge_workflow_audit(
            crate::runtime::WorkflowDisposition::Suppress,
            reason,
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }

    let Some(continuity) = self_continuity_store
        .get(board_subject_scope_id())
        .ok()
        .flatten()
    else {
        append_idle_memory_forge_workflow_audit(
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_memory_forge_no_target",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    };
    let chat_id = continuity.last_user_chat_id.trim();
    let source_channel = continuity.last_user_channel.trim();
    if chat_id.is_empty() {
        append_idle_memory_forge_workflow_audit(
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_memory_forge_no_target",
            crate::runtime::WorkflowEffect::Noop,
            None,
            None,
        );
        return;
    }
    let active_chat_window_secs = memory_policy(profile).self_runtime.active_chat_window_secs;
    if continuity.last_user_turn_at > 0
        && now_secs.saturating_sub(continuity.last_user_turn_at) > active_chat_window_secs
    {
        append_idle_memory_forge_workflow_audit(
            crate::runtime::WorkflowDisposition::NoTrigger,
            "idle_memory_forge_active_window_expired",
            crate::runtime::WorkflowEffect::Noop,
            Some(chat_id),
            Some(source_channel),
        );
        return;
    }
    match idle_memory_forge_due_from_latest_summary(state_fs, now_secs) {
        Ok(true) => {}
        Ok(false) => {
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::NoTrigger,
                "idle_memory_forge_not_due",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(source_channel),
            );
            return;
        }
        Err(error) => {
            log::warn!("[idle_memory_forge] latest summary unavailable: {error}");
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::NoTrigger,
                "idle_memory_forge_latest_invalid",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(source_channel),
            );
            return;
        }
    }
    let payload = IdleMemoryForgeJobPayload {
        trigger: IdleMemoryForgeTrigger::CronIdleTick,
        source_channel: if source_channel.is_empty() {
            IDLE_MEMORY_FORGE_SOURCE_CHANNEL.to_string()
        } else {
            source_channel.to_string()
        },
        scheduled_at: now_secs,
    };
    schedule_idle_memory_forge_job(
        system_inbound_tx,
        chat_id,
        payload,
        IDLE_MEMORY_FORGE_SCHEDULE_DELAY_MS,
    );
}

pub(crate) fn run_idle_memory_forge_background_job(
    long_term_store: &dyn LongTermMemoryStore,
    continuity_store: &dyn ContinuityCapsuleStore,
    state_fs: &dyn StateFs,
    msg: &crate::bus::PcMsg,
) -> Result<IdleMemoryForgeOperatorSummary> {
    let payload: IdleMemoryForgeJobPayload = serde_json::from_str(&msg.content)
        .map_err(|error| Error::config("idle_memory_forge_decode", error.to_string()))?;
    let started_at = crate::util::current_unix_secs();
    let executor = CurrentExecutableLuaSandboxExecutor;
    let mut job_reports = Vec::with_capacity(3);
    let mut proposal_batches = Vec::new();
    let mut attack_job_reports = Vec::with_capacity(3);
    let mut attack_batches = Vec::new();

    for job in idle_memory_forge_job_contracts() {
        let selection = selection_for_job(job.kind, &msg.chat_id);
        match execute_idle_memory_forge_job(
            &executor,
            job.kind,
            &selection,
            long_term_store,
            continuity_store,
            &msg.chat_id,
            started_at,
        ) {
            Ok((report, batch)) => {
                job_reports.push(report);
                if let Some(batch) = batch {
                    proposal_batches.push(batch);
                }
            }
            Err(error) => job_reports.push(IdleMemoryForgeJobReport {
                job_kind: job.kind,
                snapshot_digest: String::new(),
                status: IdleMemoryForgeJobStatus::Failed,
                summary: format!("{} failed", job_kind_label(job.kind)),
                candidate_count: 0,
                error_kind: Some(error.stage().to_string()),
                error_message: Some(error.to_string()),
            }),
        }
    }

    for job in memory_attack_job_contracts() {
        let selection = selection_for_attack_job(job.kind, &msg.chat_id);
        match execute_memory_attack_job(
            &executor,
            job.kind,
            &selection,
            long_term_store,
            continuity_store,
            &msg.chat_id,
            started_at,
        ) {
            Ok((report, batch)) => {
                attack_job_reports.push(report);
                if let Some(batch) = batch {
                    attack_batches.push(batch);
                }
            }
            Err(error) => attack_job_reports.push(MemoryAttackJobReport {
                job_kind: job.kind,
                snapshot_digest: String::new(),
                status: MemoryAttackJobStatus::Failed,
                summary: format!("{} failed", attack_job_kind_label(job.kind)),
                finding_count: 0,
                distillation_candidate_count: 0,
                error_kind: Some(error.stage().to_string()),
                error_message: Some(error.to_string()),
            }),
        }
    }

    let run = IdleMemoryForgeRunLedger {
        schema_version: IDLE_MEMORY_FORGE_SCHEMA_VERSION,
        chat_id: msg.chat_id.to_string(),
        source_channel: payload.source_channel,
        trigger: payload.trigger,
        started_at,
        completed_at: crate::util::current_unix_secs(),
        primary_finding: None,
        total_candidates: 0,
        adjudication_state: IdleMemoryForgeAdjudicationState::Clean,
        job_reports,
        proposal_batches,
        attack_job_reports,
        attack_batches,
    };
    let summary = persist_idle_memory_forge_run(state_fs, &run)?;
    append_idle_memory_forge_workflow_audit(
        crate::runtime::WorkflowDisposition::ExecuteNow,
        if has_review_outputs(&summary) {
            "idle_memory_forge_candidates_persisted"
        } else {
            "idle_memory_forge_completed"
        },
        crate::runtime::WorkflowEffect::PersistRecoveryIntent,
        Some(msg.chat_id.as_ref()),
        Some(IDLE_MEMORY_FORGE_SOURCE_CHANNEL),
    );
    Ok(summary)
}

pub fn persist_idle_memory_forge_run(
    state_fs: &dyn StateFs,
    run: &IdleMemoryForgeRunLedger,
) -> Result<IdleMemoryForgeOperatorSummary> {
    let normalized = normalize_run_ledger(run);
    let run_file_name = format!(
        "{}_{}.json",
        normalized.completed_at,
        sanitize_rel_component(&normalized.chat_id)
    );
    let run_path = format!("{REL_DIR_IDLE_MEMORY_FORGE_RUNS}/{run_file_name}");
    let encoded = serde_json::to_vec_pretty(&normalized)
        .map_err(|error| Error::config("idle_memory_forge_persist", error.to_string()))?;
    state_fs.write(run_path.as_str(), &encoded)?;

    let summary = operator_summary_from_run(&normalized);
    let latest = serde_json::to_vec(&summary)
        .map_err(|error| Error::config("idle_memory_forge_latest", error.to_string()))?;
    state_fs.write(REL_PATH_IDLE_MEMORY_FORGE_LATEST, &latest)?;
    Ok(summary)
}

pub fn load_idle_memory_forge_operator_summary(
    state_fs: &dyn StateFs,
) -> Result<Option<IdleMemoryForgeOperatorSummary>> {
    let Some(encoded) = state_fs.read(REL_PATH_IDLE_MEMORY_FORGE_LATEST)? else {
        return Ok(None);
    };
    let summary = serde_json::from_slice(&encoded)
        .map_err(|error| Error::config("idle_memory_forge_latest_decode", error.to_string()))?;
    Ok(Some(summary))
}

fn idle_memory_forge_due_from_latest_summary(
    state_fs: &dyn StateFs,
    now_secs: u64,
) -> Result<bool> {
    let Some(summary) = load_idle_memory_forge_operator_summary(state_fs)? else {
        return Ok(true);
    };
    if summary.last_run_at > 0
        && now_secs.saturating_sub(summary.last_run_at) < IDLE_MEMORY_FORGE_DEFAULT_CADENCE_SECS
    {
        return Ok(false);
    }
    Ok(true)
}

fn normalize_run_ledger(run: &IdleMemoryForgeRunLedger) -> IdleMemoryForgeRunLedger {
    let mut normalized = run.clone();
    normalized.schema_version = IDLE_MEMORY_FORGE_SCHEMA_VERSION;
    normalized.chat_id = run.chat_id.trim().to_string();
    normalized.source_channel = run.source_channel.trim().to_string();
    normalized.job_reports = normalized
        .job_reports
        .into_iter()
        .map(|mut report| {
            report.summary =
                truncate_content_to_max(report.summary.trim(), MAX_JOB_SUMMARY_CHARS).into_owned();
            report
        })
        .collect();
    normalized.attack_job_reports = normalized
        .attack_job_reports
        .into_iter()
        .map(|mut report| {
            report.summary =
                truncate_content_to_max(report.summary.trim(), MAX_JOB_SUMMARY_CHARS).into_owned();
            report
        })
        .collect();
    normalized.proposal_batches = normalized
        .proposal_batches
        .into_iter()
        .map(|mut batch| {
            batch.result.candidates.iter_mut().for_each(|candidate| {
                candidate.requires_adjudication = true;
            });
            batch.adjudication_state = if batch.result.candidates.is_empty() {
                IdleMemoryForgeAdjudicationState::Clean
            } else {
                IdleMemoryForgeAdjudicationState::RequiresAdjudication
            };
            batch
        })
        .collect();
    normalized.attack_batches = normalized
        .attack_batches
        .into_iter()
        .map(|mut batch| {
            batch.result.findings.iter_mut().for_each(|finding| {
                finding.requires_adjudication = true;
            });
            batch
                .result
                .distillation_candidates
                .iter_mut()
                .for_each(|candidate| {
                    candidate.requires_adjudication = true;
                });
            batch.adjudication_state = if batch.result.findings.is_empty()
                && batch.result.distillation_candidates.is_empty()
            {
                IdleMemoryForgeAdjudicationState::Clean
            } else {
                IdleMemoryForgeAdjudicationState::RequiresAdjudication
            };
            batch
        })
        .collect();
    normalized.total_candidates = normalized
        .proposal_batches
        .iter()
        .map(|batch| batch.result.candidates.len())
        .sum();
    normalized.adjudication_state = if normalized.total_candidates > 0
        || attack_findings_count(&normalized) > 0
        || distillation_candidates_count(&normalized) > 0
    {
        IdleMemoryForgeAdjudicationState::RequiresAdjudication
    } else {
        IdleMemoryForgeAdjudicationState::Clean
    };
    normalized.primary_finding = normalized
        .primary_finding
        .as_deref()
        .map(|value| truncate_content_to_max(value.trim(), MAX_PRIMARY_FINDING_CHARS).into_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| derive_primary_finding(&normalized));
    normalized
}

fn derive_primary_finding(run: &IdleMemoryForgeRunLedger) -> Option<String> {
    run.attack_batches
        .iter()
        .flat_map(|batch| batch.result.findings.iter())
        .find_map(|finding| {
            let summary =
                truncate_content_to_max(finding.summary.trim(), MAX_PRIMARY_FINDING_CHARS)
                    .into_owned();
            (!summary.is_empty()).then_some(summary)
        })
        .or_else(|| {
            run.attack_batches
                .iter()
                .flat_map(|batch| batch.result.distillation_candidates.iter())
                .find_map(|candidate| {
                    let summary = truncate_content_to_max(
                        candidate.summary.trim(),
                        MAX_PRIMARY_FINDING_CHARS,
                    )
                    .into_owned();
                    (!summary.is_empty()).then_some(summary)
                })
        })
        .or_else(|| {
            run.proposal_batches
                .iter()
                .flat_map(|batch| batch.result.candidates.iter())
                .find_map(|candidate| {
                    let finding = truncate_content_to_max(
                        candidate.summary.trim(),
                        MAX_PRIMARY_FINDING_CHARS,
                    )
                    .into_owned();
                    (!finding.is_empty()).then_some(finding)
                })
                .or_else(|| {
                    run.job_reports.iter().find_map(|report| {
                        let finding = truncate_content_to_max(
                            report.summary.trim(),
                            MAX_PRIMARY_FINDING_CHARS,
                        )
                        .into_owned();
                        (!finding.is_empty()).then_some(finding)
                    })
                })
        })
}

fn operator_summary_from_run(run: &IdleMemoryForgeRunLedger) -> IdleMemoryForgeOperatorSummary {
    IdleMemoryForgeOperatorSummary {
        last_run_at: run.completed_at,
        last_chat_id: (!run.chat_id.is_empty()).then_some(run.chat_id.clone()),
        last_source_channel: (!run.source_channel.is_empty()).then_some(run.source_channel.clone()),
        total_candidates: run.total_candidates,
        attack_findings: attack_findings_count(run),
        distillation_candidates: distillation_candidates_count(run),
        adjudication_state: run.adjudication_state,
        primary_finding: run.primary_finding.clone(),
    }
}

fn attack_findings_count(run: &IdleMemoryForgeRunLedger) -> usize {
    run.attack_batches
        .iter()
        .map(|batch| batch.result.findings.len())
        .sum()
}

fn distillation_candidates_count(run: &IdleMemoryForgeRunLedger) -> usize {
    run.attack_batches
        .iter()
        .map(|batch| batch.result.distillation_candidates.len())
        .sum()
}

fn has_review_outputs(summary: &IdleMemoryForgeOperatorSummary) -> bool {
    summary.total_candidates > 0
        || summary.attack_findings > 0
        || summary.distillation_candidates > 0
}

fn sanitize_rel_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    sanitized
        .trim_matches('_')
        .chars()
        .take(48)
        .collect::<String>()
}

fn idle_memory_forge_enqueue_block_reason() -> Option<&'static str> {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    let orchestrator = crate::orchestrator::snapshot();
    let snapshot = IdleMemoryForgeAdmissionSnapshot {
        allow_periodic_maintenance: runtime_mode.action_budget.allow_periodic_maintenance,
        active_agent_tasks: orchestrator.active_agent_tasks as usize,
        inbound_depth: orchestrator.inbound_depth as usize,
        outbound_depth: orchestrator.outbound_depth as usize,
    };
    if should_run_idle_memory_forge(&snapshot) {
        None
    } else if !runtime_mode.action_budget.allow_periodic_maintenance {
        Some(
            runtime_mode
                .mode_block_reason()
                .unwrap_or("runtime_mode_blocked"),
        )
    } else {
        Some("message_queues_busy")
    }
}

fn schedule_idle_memory_forge_job(
    system_inbound_tx: &crate::bus::SystemInboundTx,
    chat_id: &str,
    payload: IdleMemoryForgeJobPayload,
    delay_ms: u64,
) {
    let system_inbound_tx = system_inbound_tx.clone();
    let chat_id = chat_id.to_string();
    let audit_chat_id = chat_id.clone();
    let audit_channel = payload.source_channel.clone();
    let delayed_chat_id = chat_id.clone();
    let delayed_payload = payload.clone();
    let scheduled = crate::runtime::schedule_delayed_task(
        Instant::now() + Duration::from_millis(delay_ms),
        Box::new(move || {
            if let Some(reason) = idle_memory_forge_enqueue_block_reason() {
                append_idle_memory_forge_workflow_audit(
                    crate::runtime::WorkflowDisposition::Suppress,
                    reason,
                    crate::runtime::WorkflowEffect::Noop,
                    Some(delayed_chat_id.as_str()),
                    Some(audit_channel.as_str()),
                );
                return;
            }
            let _ = enqueue_idle_memory_forge_job_now(
                &system_inbound_tx,
                &delayed_chat_id,
                &delayed_payload,
            );
        }),
    );
    append_idle_memory_forge_workflow_audit(
        if scheduled {
            crate::runtime::WorkflowDisposition::DeferUntil
        } else {
            crate::runtime::WorkflowDisposition::ExecuteFailed
        },
        if scheduled {
            "idle_memory_forge_scheduled"
        } else {
            "idle_memory_forge_schedule_failed"
        },
        if scheduled {
            crate::runtime::WorkflowEffect::EnqueueSystemJob
        } else {
            crate::runtime::WorkflowEffect::Noop
        },
        Some(audit_chat_id.as_str()),
        Some(payload.source_channel.as_str()),
    );
}

fn enqueue_idle_memory_forge_job_now(
    system_inbound_tx: &crate::bus::SystemInboundTx,
    chat_id: &str,
    payload: &IdleMemoryForgeJobPayload,
) -> bool {
    let body = match serde_json::to_string(payload) {
        Ok(body) => body,
        Err(error) => {
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "idle_memory_forge_serialize_failed",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            log::warn!(
                "[idle_memory_forge] serialize failed chat_id={}: {}",
                chat_id,
                error
            );
            return false;
        }
    };
    let job = match crate::bus::PcMsg::new_system(
        crate::runtime::system_work::CHANNEL_IDLE_MEMORY_FORGE,
        chat_id,
        body,
    ) {
        Ok(job) => job,
        Err(error) => {
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "idle_memory_forge_build_failed",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            log::warn!(
                "[idle_memory_forge] build job failed chat_id={}: {}",
                chat_id,
                error
            );
            return false;
        }
    };
    match system_inbound_tx.try_send(job) {
        Ok(()) => true,
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "idle_memory_forge_queue_full",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            append_idle_memory_forge_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "idle_memory_forge_queue_disconnected",
                crate::runtime::WorkflowEffect::Noop,
                Some(chat_id),
                Some(payload.source_channel.as_str()),
            );
            false
        }
    }
}

fn execute_idle_memory_forge_job(
    executor: &dyn ReasoningExecutor,
    job_kind: IdleMemoryForgeJobKind,
    selection: &MemoryQuerySelection,
    long_term_store: &dyn LongTermMemoryStore,
    continuity_store: &dyn ContinuityCapsuleStore,
    chat_id: &str,
    now_secs: u64,
) -> Result<(
    IdleMemoryForgeJobReport,
    Option<IdleMemoryForgeProposalBatch>,
)> {
    let snapshot =
        build_memory_query_snapshot_from_stores(selection, long_term_store, continuity_store)?;
    let snapshot_digest = snapshot.snapshot_digest.clone();
    let request = LuaQueryRequest {
        script: job_script(job_kind).to_string(),
        input: json!({
            "snapshot": snapshot,
            "meta": {
                "now_secs": now_secs,
                "chat_id": chat_id
            }
        }),
        budget: LuaQueryBudget::default().with_timeout_ms(600),
        capabilities: default_lua_memory_query_capabilities(),
    };
    let response = executor.execute_query(&request)?;
    if !response.ok {
        let message = response
            .error_message
            .unwrap_or_else(|| "lua runner returned failure".to_string());
        return Ok((
            IdleMemoryForgeJobReport {
                job_kind,
                snapshot_digest,
                status: IdleMemoryForgeJobStatus::Failed,
                summary: format!("{} failed", job_kind_label(job_kind)),
                candidate_count: 0,
                error_kind: response.error_kind,
                error_message: Some(message),
            },
            None,
        ));
    }

    let result = response
        .result
        .ok_or_else(|| Error::config("idle_memory_forge_result", "missing lua result"))?;
    let validated = validate_memory_query_result(result)?;
    let candidate_count = validated.candidates.len();
    let report = IdleMemoryForgeJobReport {
        job_kind,
        snapshot_digest: snapshot_digest.clone(),
        status: if candidate_count == 0 {
            IdleMemoryForgeJobStatus::NoCandidates
        } else {
            IdleMemoryForgeJobStatus::Succeeded
        },
        summary: validated.summary.clone(),
        candidate_count,
        error_kind: None,
        error_message: None,
    };
    let batch = (candidate_count > 0).then_some(IdleMemoryForgeProposalBatch {
        job_kind,
        snapshot_digest,
        result: validated,
        adjudication_state: IdleMemoryForgeAdjudicationState::RequiresAdjudication,
    });
    Ok((report, batch))
}

fn selection_for_job(kind: IdleMemoryForgeJobKind, chat_id: &str) -> MemoryQuerySelection {
    match kind {
        IdleMemoryForgeJobKind::StaleFactualScan => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                freshness: None,
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_limit: 0,
            include_long_term: true,
            include_continuity: false,
            continuity_scope: None,
        },
        IdleMemoryForgeJobKind::ContinuityConflictScan => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_scope: Some(MemoryQueryContinuityScope {
                scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                scope_id: chat_id.to_string(),
            }),
            continuity_limit: IDLE_MEMORY_FORGE_CONTINUITY_LIMIT,
            include_long_term: true,
            include_continuity: true,
        },
        IdleMemoryForgeJobKind::NearDuplicateFactualScan => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_limit: 0,
            include_long_term: true,
            include_continuity: false,
            continuity_scope: None,
        },
    }
}

fn selection_for_attack_job(kind: MemoryAttackJobKind, chat_id: &str) -> MemoryQuerySelection {
    match kind {
        MemoryAttackJobKind::ContradictionSearch => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_scope: Some(MemoryQueryContinuityScope {
                scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                scope_id: chat_id.to_string(),
            }),
            continuity_limit: IDLE_MEMORY_FORGE_CONTINUITY_LIMIT,
            include_long_term: true,
            include_continuity: true,
        },
        MemoryAttackJobKind::EvidenceWeighing => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_limit: 0,
            include_long_term: true,
            include_continuity: false,
            continuity_scope: None,
        },
        MemoryAttackJobKind::DistillationProposalGeneration => MemoryQuerySelection {
            long_term_query: Some(LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Fact),
                source_scope: Some(LongTermMemorySourceScope::World),
                include_stale: true,
                limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
                ..LongTermMemoryQuery::default()
            }),
            long_term_limit: IDLE_MEMORY_FORGE_LONG_TERM_LIMIT,
            continuity_scope: Some(MemoryQueryContinuityScope {
                scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                scope_id: chat_id.to_string(),
            }),
            continuity_limit: IDLE_MEMORY_FORGE_CONTINUITY_LIMIT,
            include_long_term: true,
            include_continuity: true,
        },
    }
}

fn append_idle_memory_forge_workflow_audit(
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    chat_id: Option<&str>,
    source_channel: Option<&str>,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::IdleMemoryForge,
            crate::runtime::WorkflowTrigger::CronTick,
            disposition,
            effect,
            crate::runtime::WorkflowRecoveryPolicy::DropOnModeExit,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, source_channel, chat_id),
    );
}

fn job_kind_label(kind: IdleMemoryForgeJobKind) -> &'static str {
    match kind {
        IdleMemoryForgeJobKind::StaleFactualScan => "stale_factual_scan",
        IdleMemoryForgeJobKind::ContinuityConflictScan => "continuity_conflict_scan",
        IdleMemoryForgeJobKind::NearDuplicateFactualScan => "near_duplicate_factual_scan",
    }
}

fn attack_job_kind_label(kind: MemoryAttackJobKind) -> &'static str {
    match kind {
        MemoryAttackJobKind::ContradictionSearch => "contradiction_search",
        MemoryAttackJobKind::EvidenceWeighing => "evidence_weighing",
        MemoryAttackJobKind::DistillationProposalGeneration => "distillation_proposal_generation",
    }
}

fn job_script(kind: IdleMemoryForgeJobKind) -> &'static str {
    match kind {
        IdleMemoryForgeJobKind::StaleFactualScan => STALE_FACTUAL_SCAN_SCRIPT,
        IdleMemoryForgeJobKind::ContinuityConflictScan => CONTINUITY_CONFLICT_SCAN_SCRIPT,
        IdleMemoryForgeJobKind::NearDuplicateFactualScan => NEAR_DUPLICATE_FACTUAL_SCAN_SCRIPT,
    }
}

fn attack_job_script(kind: MemoryAttackJobKind) -> &'static str {
    match kind {
        MemoryAttackJobKind::ContradictionSearch => CONTRADICTION_SEARCH_SCRIPT,
        MemoryAttackJobKind::EvidenceWeighing => EVIDENCE_WEIGHING_SCRIPT,
        MemoryAttackJobKind::DistillationProposalGeneration => {
            DISTILLATION_PROPOSAL_GENERATION_SCRIPT
        }
    }
}

fn default_lua_memory_attack_capabilities() -> Vec<String> {
    let mut capabilities = crate::reasoning::default_lua_query_capabilities();
    capabilities.push("read_memory_snapshot".to_string());
    capabilities.push("emit_memory_attack_findings".to_string());
    capabilities.push("emit_memory_distillation_candidates".to_string());
    capabilities
}

fn execute_memory_attack_job(
    executor: &dyn ReasoningExecutor,
    job_kind: MemoryAttackJobKind,
    selection: &MemoryQuerySelection,
    long_term_store: &dyn LongTermMemoryStore,
    continuity_store: &dyn ContinuityCapsuleStore,
    chat_id: &str,
    now_secs: u64,
) -> Result<(MemoryAttackJobReport, Option<IdleMemoryForgeAttackBatch>)> {
    let snapshot =
        build_memory_query_snapshot_from_stores(selection, long_term_store, continuity_store)?;
    let snapshot_digest = snapshot.snapshot_digest.clone();
    let request = LuaQueryRequest {
        script: attack_job_script(job_kind).to_string(),
        input: json!({
            "snapshot": snapshot,
            "meta": {
                "now_secs": now_secs,
                "chat_id": chat_id
            }
        }),
        budget: LuaQueryBudget::default().with_timeout_ms(700),
        capabilities: default_lua_memory_attack_capabilities(),
    };
    let response = executor.execute_query(&request)?;
    if !response.ok {
        let message = response
            .error_message
            .unwrap_or_else(|| "lua runner returned failure".to_string());
        return Ok((
            MemoryAttackJobReport {
                job_kind,
                snapshot_digest,
                status: MemoryAttackJobStatus::Failed,
                summary: format!("{} failed", attack_job_kind_label(job_kind)),
                finding_count: 0,
                distillation_candidate_count: 0,
                error_kind: response.error_kind,
                error_message: Some(message),
            },
            None,
        ));
    }

    let result = response
        .result
        .ok_or_else(|| Error::config("memory_attack_result", "missing lua result"))?;
    let validated = validate_memory_attack_result(result)?;
    let finding_count = validated.findings.len();
    let distillation_candidate_count = validated.distillation_candidates.len();
    let report = MemoryAttackJobReport {
        job_kind,
        snapshot_digest: snapshot_digest.clone(),
        status: if finding_count == 0 && distillation_candidate_count == 0 {
            MemoryAttackJobStatus::NoFindings
        } else {
            MemoryAttackJobStatus::Succeeded
        },
        summary: validated.summary.clone(),
        finding_count,
        distillation_candidate_count,
        error_kind: None,
        error_message: None,
    };
    let batch = (finding_count > 0 || distillation_candidate_count > 0).then_some(
        IdleMemoryForgeAttackBatch {
            job_kind,
            snapshot_digest,
            result: validated,
            adjudication_state: IdleMemoryForgeAdjudicationState::RequiresAdjudication,
        },
    );
    Ok((report, batch))
}

const STALE_FACTUAL_SCAN_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local now_secs = (((input.meta or {}).now_secs) or 0)
local candidates = {}
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  local freshness = entry.freshness or ""
  if entry.kind == "fact" and (freshness == "dynamic" or freshness == "volatile") then
    local last = tonumber(entry.last_confirmed_at or 0)
    if last == 0 then
      last = tonumber(entry.updated_at or 0)
    end
    local threshold = freshness == "volatile" and 86400 or (7 * 86400)
    if last == 0 or (now_secs > 0 and (now_secs - last) >= threshold) then
      table.insert(candidates, {
        kind = "stale",
        summary = "Review stale factual record: " .. (entry.topic or record.record_ref or "fact"),
        rationale = "Dynamic or volatile factual memory has not been confirmed recently.",
        record_refs = { record.record_ref },
        requires_adjudication = true,
      })
    end
  end
end
return {
  summary = (#candidates > 0)
    and ("Found " .. tostring(#candidates) .. " stale factual candidates.")
    or "No stale factual candidates found.",
  candidates = candidates,
}
"#;

const CONTINUITY_CONFLICT_SCAN_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local candidates = {}
local facts = {}
local function normalize(value)
  value = tostring(value or ""):lower()
  value = value:gsub("[%s%p_]+", "")
  return value
end
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  if entry.kind == "fact" then
    local key = normalize(entry.topic)
    if key ~= "" then
      facts[key] = record
    end
  end
end
for _, record in ipairs(snapshot.continuity_capsules or {}) do
  local capsule = record.capsule or {}
  local key = normalize(capsule.topic)
  local fact = facts[key]
  if fact ~= nil and key ~= "" then
    local fact_text = normalize((fact.entry or {}).content)
    local capsule_text = normalize((capsule.summary or "") .. " " .. (capsule.outcome or "") .. " " .. (capsule.next_step or ""))
    if fact_text ~= "" and capsule_text ~= "" and fact_text ~= capsule_text then
      table.insert(candidates, {
        kind = "conflict",
        summary = "Review continuity conflict: " .. (capsule.topic or key),
        rationale = "Canonical factual memory and continuity capsule describe the same topic differently.",
        record_refs = { fact.record_ref, record.record_ref },
        requires_adjudication = true,
      })
    end
  end
end
return {
  summary = (#candidates > 0)
    and ("Found " .. tostring(#candidates) .. " continuity conflict candidates.")
    or "No continuity conflict candidates found.",
  candidates = candidates,
}
"#;

const NEAR_DUPLICATE_FACTUAL_SCAN_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local groups = {}
local candidates = {}
local function normalize(value)
  value = tostring(value or ""):lower()
  value = value:gsub("[%s%p_]+", "")
  return value
end
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  if entry.kind == "fact" then
    local content = normalize(entry.content)
    if content ~= "" then
      if groups[content] == nil then
        groups[content] = {}
      end
      table.insert(groups[content], record)
    end
  end
end
for _, records in pairs(groups) do
  if #records > 1 then
    local refs = {}
    local topics = {}
    for _, record in ipairs(records) do
      table.insert(refs, record.record_ref)
      table.insert(topics, ((record.entry or {}).topic or record.record_ref))
    end
    table.insert(candidates, {
      kind = "merge",
      summary = "Review near-duplicate factual records.",
      rationale = "Multiple factual records share nearly identical canonical content: " .. table.concat(topics, ", "),
      record_refs = refs,
      requires_adjudication = true,
    })
  end
end
return {
  summary = (#candidates > 0)
    and ("Found " .. tostring(#candidates) .. " near-duplicate factual candidates.")
    or "No near-duplicate factual candidates found.",
  candidates = candidates,
}
"#;

const CONTRADICTION_SEARCH_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local findings = {}
local facts_by_topic = {}
local function normalize(value)
  value = tostring(value or ""):lower()
  value = value:gsub("[%s%p_]+", "")
  return value
end
local function push_finding(summary, rationale, refs)
  table.insert(findings, {
    kind = "contradiction",
    summary = summary,
    rationale = rationale,
    record_refs = refs,
    requires_adjudication = true,
  })
end
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  if entry.kind == "fact" then
    local key = normalize(entry.topic)
    if key ~= "" then
      if facts_by_topic[key] == nil then
        facts_by_topic[key] = {}
      end
      table.insert(facts_by_topic[key], record)
    end
  end
end
for _, records in pairs(facts_by_topic) do
  local content_map = {}
  local refs = {}
  local topic = nil
  for _, record in ipairs(records) do
    local entry = record.entry or {}
    topic = topic or entry.topic or record.record_ref
    local content = normalize(entry.content)
    if content ~= "" then
      content_map[content] = true
    end
    table.insert(refs, record.record_ref)
  end
  local content_count = 0
  for _ in pairs(content_map) do
    content_count = content_count + 1
  end
  if content_count > 1 then
    push_finding(
      "Review contradictory factual memory: " .. tostring(topic or "fact"),
      "Multiple canonical fact records with the same topic disagree on content.",
      refs
    )
  end
end
for _, capsule_record in ipairs(snapshot.continuity_capsules or {}) do
  local capsule = capsule_record.capsule or {}
  local key = normalize(capsule.topic)
  local records = facts_by_topic[key]
  if key ~= "" and records ~= nil and #records > 0 then
    local capsule_text = normalize((capsule.summary or "") .. " " .. (capsule.outcome or "") .. " " .. (capsule.next_step or ""))
    local fact_text = normalize(((records[1].entry or {}).content or ""))
    if capsule_text ~= "" and fact_text ~= "" and capsule_text ~= fact_text then
      push_finding(
        "Review factual contradiction against continuity: " .. tostring(capsule.topic or key),
        "Canonical fact memory and recent continuity describe the same topic differently.",
        { records[1].record_ref, capsule_record.record_ref }
      )
    end
  end
end
return {
  summary = (#findings > 0)
    and ("Found " .. tostring(#findings) .. " contradiction findings.")
    or "No contradiction findings found.",
  findings = findings,
  distillation_candidates = {},
}
"#;

const EVIDENCE_WEIGHING_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local now_secs = (((input.meta or {}).now_secs) or 0)
local findings = {}
local function threshold_for(entry)
  if entry.freshness == "volatile" then
    return 21 * 86400
  elseif entry.freshness == "dynamic" then
    return 90 * 86400
  end
  return 365 * 86400
end
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  if entry.kind == "fact" then
    local weak = false
    local reasons = {}
    if entry.confidence == "low" then
      weak = true
      table.insert(reasons, "confidence is low")
    end
    if tonumber(entry.evidence_count or 0) <= 1 then
      weak = true
      table.insert(reasons, "evidence count is thin")
    end
    if entry.stale_hint == "review_before_use" or entry.stale_hint == "verify_against_current_state" then
      weak = true
      table.insert(reasons, "stale hint already requests review")
    end
    local last = tonumber(entry.last_confirmed_at or 0)
    if last == 0 then
      last = tonumber(entry.updated_at or 0)
    end
    local age = (now_secs > 0 and last > 0) and (now_secs - last) or 0
    if age > threshold_for(entry) then
      weak = true
      table.insert(reasons, "confirmation is older than freshness budget")
    end
    if weak then
      table.insert(findings, {
        kind = "weak_evidence",
        summary = "Review weak evidence for: " .. tostring(entry.topic or record.record_ref),
        rationale = "Memory record needs recheck because " .. table.concat(reasons, "; ") .. ".",
        record_refs = { record.record_ref },
        requires_adjudication = true,
      })
    end
  end
end
return {
  summary = (#findings > 0)
    and ("Found " .. tostring(#findings) .. " weak-evidence findings.")
    or "No weak-evidence findings found.",
  findings = findings,
  distillation_candidates = {},
}
"#;

const DISTILLATION_PROPOSAL_GENERATION_SCRIPT: &str = r#"
local snapshot = input.snapshot or {}
local candidates = {}
local groups = {}
local function normalize(value)
  value = tostring(value or ""):lower()
  value = value:gsub("[%s%p_]+", "")
  return value
end
local function ensure_group(key, topic)
  if groups[key] == nil then
    groups[key] = { topic = topic, refs = {}, snippets = {} }
  end
  return groups[key]
end
for _, record in ipairs(snapshot.long_term_entries or {}) do
  local entry = record.entry or {}
  if entry.kind == "fact" then
    local key = normalize(entry.topic)
    if key ~= "" then
      local group = ensure_group(key, entry.topic or record.record_ref)
      table.insert(group.refs, record.record_ref)
      table.insert(group.snippets, tostring(entry.content or ""))
    end
  end
end
for _, record in ipairs(snapshot.continuity_capsules or {}) do
  local capsule = record.capsule or {}
  local key = normalize(capsule.topic)
  if key ~= "" then
    local group = ensure_group(key, capsule.topic or record.record_ref)
    table.insert(group.refs, record.record_ref)
    local summary = tostring(capsule.summary or "")
    local outcome = tostring(capsule.outcome or "")
    local next_step = tostring(capsule.next_step or "")
    table.insert(group.snippets, table.concat({ summary, outcome, next_step }, " "))
  end
end
for _, group in pairs(groups) do
  if #group.refs >= 2 then
    local content = {}
    for idx, snippet in ipairs(group.snippets) do
      local normalized = normalize(snippet)
      if normalized ~= "" then
        table.insert(content, snippet)
      end
      if #content >= 3 then
        break
      end
    end
    table.insert(candidates, {
      kind = "fact",
      topic = tostring(group.topic or "distilled_memory"),
      summary = "Distill reusable memory for: " .. tostring(group.topic or "memory"),
      content = table.concat(content, " | "),
      record_refs = group.refs,
      requires_adjudication = true,
    })
  end
end
return {
  summary = (#candidates > 0)
    and ("Generated " .. tostring(#candidates) .. " distillation candidates.")
    or "No distillation candidates generated.",
  findings = {},
  distillation_candidates = candidates,
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::StateFs;
    use serde_json::json;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> crate::error::Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> crate::error::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> crate::error::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, rel_path: &str) -> crate::error::Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{}/", rel_path.trim_end_matches('/'))
            };
            let files = self.files.lock().unwrap_or_else(|error| error.into_inner());
            let mut names = BTreeSet::new();
            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let tail = &key[prefix.len()..];
                if tail.is_empty() {
                    continue;
                }
                if let Some((dir, _)) = tail.split_once('/') {
                    names.insert(format!("{dir}/"));
                } else {
                    names.insert(tail.to_string());
                }
            }
            Ok(names.into_iter().collect())
        }
    }

    #[test]
    fn idle_forge_job_contracts_stay_linux_only_and_proposal_only() {
        let jobs = idle_memory_forge_job_contracts();
        assert_eq!(jobs.len(), 3);
        assert!(jobs.iter().all(|job| job.linux_only));
        assert!(jobs.iter().all(|job| job.proposal_only));
        assert!(jobs.iter().all(|job| job.cadence_secs > 0));
    }

    #[test]
    fn idle_forge_admission_requires_idle_background_window() {
        assert!(!should_run_idle_memory_forge(
            &IdleMemoryForgeAdmissionSnapshot {
                allow_periodic_maintenance: false,
                active_agent_tasks: 0,
                inbound_depth: 0,
                outbound_depth: 0,
            }
        ));
        assert!(!should_run_idle_memory_forge(
            &IdleMemoryForgeAdmissionSnapshot {
                allow_periodic_maintenance: true,
                active_agent_tasks: 1,
                inbound_depth: 0,
                outbound_depth: 0,
            }
        ));
        assert!(!should_run_idle_memory_forge(
            &IdleMemoryForgeAdmissionSnapshot {
                allow_periodic_maintenance: true,
                active_agent_tasks: 0,
                inbound_depth: 1,
                outbound_depth: 0,
            }
        ));
        assert!(should_run_idle_memory_forge(
            &IdleMemoryForgeAdmissionSnapshot {
                allow_periodic_maintenance: true,
                active_agent_tasks: 0,
                inbound_depth: 0,
                outbound_depth: 0,
            }
        ));
    }

    #[test]
    fn persist_idle_forge_run_writes_latest_and_run_ledger() {
        let fs = MemoryStateFs::default();
        let run = sample_run_ledger();

        let summary = persist_idle_memory_forge_run(&fs, &run).unwrap();

        assert_eq!(summary.last_chat_id.as_deref(), Some("c2c:947"));
        assert_eq!(summary.total_candidates, 2);
        assert_eq!(summary.attack_findings, 1);
        assert_eq!(summary.distillation_candidates, 1);
        assert_eq!(
            summary.adjudication_state,
            IdleMemoryForgeAdjudicationState::RequiresAdjudication
        );
        assert_eq!(
            summary.primary_finding.as_deref(),
            Some("Review device drift.")
        );

        let latest = load_idle_memory_forge_operator_summary(&fs)
            .unwrap()
            .expect("latest forge summary");
        assert_eq!(latest, summary);

        let entries = fs.list_dir(REL_DIR_IDLE_MEMORY_FORGE_RUNS).unwrap();
        assert_eq!(entries.len(), 1);

        let encoded = fs
            .read(format!("{REL_DIR_IDLE_MEMORY_FORGE_RUNS}/{}", entries[0]).as_str())
            .unwrap()
            .expect("run ledger");
        let decoded: IdleMemoryForgeRunLedger = serde_json::from_slice(&encoded).unwrap();
        assert!(decoded
            .proposal_batches
            .iter()
            .flat_map(|batch| batch.result.candidates.iter())
            .all(|candidate| candidate.requires_adjudication));
        assert_eq!(
            decoded.adjudication_state,
            IdleMemoryForgeAdjudicationState::RequiresAdjudication
        );
        assert!(decoded
            .attack_batches
            .iter()
            .flat_map(|batch| batch.result.findings.iter())
            .all(|finding| finding.requires_adjudication));
        assert!(decoded
            .attack_batches
            .iter()
            .flat_map(|batch| batch.result.distillation_candidates.iter())
            .all(|candidate| candidate.requires_adjudication));
    }

    #[test]
    fn idle_forge_due_from_latest_summary_returns_error_for_corrupt_latest_file() {
        let fs = MemoryStateFs::default();
        fs.write(REL_PATH_IDLE_MEMORY_FORGE_LATEST, br#"{"bad":"json""#)
            .unwrap();
        let error =
            idle_memory_forge_due_from_latest_summary(&fs, crate::util::current_unix_secs())
                .expect_err("corrupt latest summary must not be treated as missing");
        assert!(error
            .to_string()
            .contains("idle_memory_forge_latest_decode"));
    }

    fn sample_run_ledger() -> IdleMemoryForgeRunLedger {
        IdleMemoryForgeRunLedger {
            schema_version: 1,
            chat_id: "c2c:947".to_string(),
            source_channel: "idle_memory_forge".to_string(),
            trigger: IdleMemoryForgeTrigger::CronIdleTick,
            started_at: 1_775_976_000,
            completed_at: 1_775_976_012,
            primary_finding: Some("Review device drift.".to_string()),
            total_candidates: 2,
            adjudication_state: IdleMemoryForgeAdjudicationState::RequiresAdjudication,
            job_reports: vec![IdleMemoryForgeJobReport {
                job_kind: IdleMemoryForgeJobKind::ContinuityConflictScan,
                snapshot_digest: "snap-1".to_string(),
                status: IdleMemoryForgeJobStatus::Succeeded,
                summary: "Detected device drift candidates.".to_string(),
                candidate_count: 2,
                error_kind: None,
                error_message: None,
            }],
            proposal_batches: vec![IdleMemoryForgeProposalBatch {
                job_kind: IdleMemoryForgeJobKind::ContinuityConflictScan,
                snapshot_digest: "snap-1".to_string(),
                result: serde_json::from_value(json!({
                    "summary": "Detected device drift candidates.",
                    "groups": [{
                        "label": "device_state",
                        "summary": "Device state mismatches",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"]
                    }],
                    "candidates": [{
                        "kind": "conflict",
                        "summary": "Review device drift.",
                        "rationale": "Canonical factual state and continuity diverged.",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"],
                        "requires_adjudication": false
                    },{
                        "kind": "stale",
                        "summary": "Review stale device revision.",
                        "rationale": "The last confirmed revision is stale.",
                        "record_refs": ["ltm:fact:device_revision"],
                        "requires_adjudication": true
                    }]
                }))
                .unwrap(),
                adjudication_state: IdleMemoryForgeAdjudicationState::RequiresAdjudication,
            }],
            attack_job_reports: vec![MemoryAttackJobReport {
                job_kind: MemoryAttackJobKind::ContradictionSearch,
                snapshot_digest: "attack-snap-1".to_string(),
                status: MemoryAttackJobStatus::Succeeded,
                summary: "Found one contradiction and one distillation candidate.".to_string(),
                finding_count: 1,
                distillation_candidate_count: 1,
                error_kind: None,
                error_message: None,
            }],
            attack_batches: vec![IdleMemoryForgeAttackBatch {
                job_kind: MemoryAttackJobKind::ContradictionSearch,
                snapshot_digest: "attack-snap-1".to_string(),
                result: serde_json::from_value(json!({
                    "summary": "Found one contradiction and one distillation candidate.",
                    "findings": [{
                        "kind": "contradiction",
                        "summary": "Review device drift.",
                        "rationale": "Two memory planes disagree on current device state.",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"],
                        "requires_adjudication": false
                    }],
                    "distillation_candidates": [{
                        "kind": "fact",
                        "topic": "device_summary",
                        "summary": "Distill reusable memory for: device_summary",
                        "content": "Linux board on QQ channel with idle maintenance active.",
                        "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"],
                        "requires_adjudication": false
                    }]
                }))
                .unwrap(),
                adjudication_state: IdleMemoryForgeAdjudicationState::RequiresAdjudication,
            }],
        }
    }
}
