//! Task-learning governance layered on top of the formal task workspace.
//! 任务学习治理层：把任务产物分流到 canonical / runtime skill / archive / workspace。

use crate::error::{Error, Result};
use crate::memory::{
    write_governed_shared_memory, LongTermMemoryConfidence, LongTermMemoryDraft,
    LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
    LongTermMemorySourceType, LongTermMemoryStore, MemoryStore, SharedMemoryWriteSource,
};
use crate::skills::{runtime_skill_name_for_topic, upsert_runtime_skill, RuntimeSkillWrite};
use crate::util::{epoch_to_ymdhms, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use super::{
    current_or_next_step, summarize_task_artifact_content, TaskArtifactRecord, TaskArtifactStore,
    TaskExecutionLedgerEntry, TaskExecutionLedgerStore, TaskRunRecord, TaskRunStatus, TaskRunStore,
    MAX_TASK_ARTIFACT_CONTENT_CHARS, MAX_TASK_ARTIFACT_ID_CHARS, MAX_TASK_ARTIFACT_SUMMARY_CHARS,
    MAX_TASK_OPERATOR_ARTIFACT_PREVIEW, MAX_TASK_OPERATOR_RECENT_RUNS, MAX_TASK_PROVENANCE_CHARS,
    MAX_TASK_REASON_CHARS, MAX_TASK_STEP_LIST_ITEMS, MAX_TASK_TITLE_CHARS,
};

pub const REL_DIR_TASK_LEARNING: &str = "memory/task_learning";

const MAX_TASK_LEARNING_RECORDS_PER_CHAT: usize = 64;
const MAX_TASK_LEARNING_HITS: usize = 4;
const MIN_TASK_RECALL_BLOCK_LEN: usize = 180;
const PROCEDURE_PROMOTION_MIN_DISTINCT_RUNS: usize = 2;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLearningKind {
    DurableFact,
    ReusableProcedure,
    EvidenceOnly,
    TransientArtifact,
}

impl TaskLearningKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::DurableFact => "durable_fact",
            Self::ReusableProcedure => "reusable_procedure",
            Self::EvidenceOnly => "evidence_only",
            Self::TransientArtifact => "transient_artifact",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskLearningRoute {
    #[default]
    Pending,
    CanonicalFactual,
    RuntimeSkill,
    ArchivedEvidence,
    WorkspacePruned,
    Rejected,
}

impl TaskLearningRoute {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::CanonicalFactual => "canonical_factual",
            Self::RuntimeSkill => "runtime_skill",
            Self::ArchivedEvidence => "archived_evidence",
            Self::WorkspacePruned => "workspace_pruned",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLearningDraft {
    pub topic: String,
    #[serde(default)]
    pub summary: String,
    pub content: String,
    #[serde(default)]
    pub memory_kind: Option<LongTermMemoryKind>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLearningRecord {
    pub learning_id: String,
    pub source_channel: String,
    pub source_chat_id: String,
    pub run_id: String,
    #[serde(default)]
    pub step_id: String,
    pub kind: TaskLearningKind,
    pub route: TaskLearningRoute,
    pub run_status: TaskRunStatus,
    pub topic: String,
    pub summary: String,
    pub content: String,
    #[serde(default)]
    pub memory_kind: Option<LongTermMemoryKind>,
    #[serde(default)]
    pub review_summary: String,
    #[serde(default)]
    pub source_artifact_ids: Vec<String>,
    #[serde(default)]
    pub provenance: String,
    #[serde(default)]
    pub archive_note_name: String,
    #[serde(default)]
    pub route_detail: String,
    #[serde(default)]
    pub observed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLearningHit {
    pub record: TaskLearningRecord,
    pub score: u32,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLearningOperatorRecord {
    pub learning_id: String,
    pub run_id: String,
    pub kind: TaskLearningKind,
    pub route: TaskLearningRoute,
    pub topic: String,
    pub summary: String,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskLearningOperatorSnapshot {
    #[serde(default)]
    pub pending: usize,
    #[serde(default)]
    pub runtime_skill_promoted: usize,
    #[serde(default)]
    pub canonical_facts_written: usize,
    #[serde(default)]
    pub archived_evidence: usize,
    #[serde(default)]
    pub workspace_pruned: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_records: Vec<TaskLearningOperatorRecord>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskLearningMaintenanceOutcome {
    pub considered: usize,
    pub updated: usize,
    pub canonical_writes: usize,
    pub runtime_skill_promotions: usize,
    pub archived_records: usize,
    pub pruned_artifacts: usize,
    pub rejected: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskLearningInspection {
    pub channel: String,
    pub chat_id: String,
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_hits: Vec<TaskLearningRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_records: Vec<TaskLearningRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskWorkspaceInspection {
    pub channel: String,
    pub chat_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<TaskRunRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<TaskArtifactRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<TaskExecutionLedgerEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub learning_records: Vec<TaskLearningRecord>,
}

pub trait TaskLearningStore: Send + Sync {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>>;
    fn upsert(&self, record: &TaskLearningRecord) -> Result<()>;
    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>>;
    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>>;
    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>>;
}

pub struct TaskLearningMaintenanceContext<'a> {
    pub task_run_store: &'a dyn TaskRunStore,
    pub task_artifact_store: &'a dyn TaskArtifactStore,
    pub task_learning_store: &'a dyn TaskLearningStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub skill_storage: &'a dyn crate::platform::SkillStorage,
    pub memory_store: &'a dyn MemoryStore,
}

pub struct TaskLearningMaintenanceInput<'a> {
    pub channel: &'a str,
    pub chat_id: &'a str,
    pub now_secs: u64,
}

pub fn normalize_task_learning_drafts(
    drafts: &mut Vec<TaskLearningDraft>,
    stage: &'static str,
) -> Result<()> {
    let mut normalized = Vec::with_capacity(drafts.len().min(MAX_TASK_STEP_LIST_ITEMS));
    for (index, draft) in drafts.drain(..).take(MAX_TASK_STEP_LIST_ITEMS).enumerate() {
        normalized.push(normalize_task_learning_draft(draft, stage, index)?);
    }
    *drafts = normalized;
    Ok(())
}

pub fn normalize_task_learning_artifact_ids(values: &mut Vec<String>) {
    *values = values
        .drain(..)
        .filter_map(|value| {
            let normalized = normalize_inline(&value, MAX_TASK_ARTIFACT_ID_CHARS);
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(MAX_TASK_STEP_LIST_ITEMS)
        .collect();
}

pub fn build_task_learning_records(
    record: &TaskRunRecord,
    step_id: &str,
    step_artifact: &TaskArtifactRecord,
    review_artifact: &TaskArtifactRecord,
    durable_facts: &[TaskLearningDraft],
    reusable_procedures: &[TaskLearningDraft],
    evidence_only: &[TaskLearningDraft],
    transient_artifact_ids: &[String],
    review_summary: &str,
    now_secs: u64,
) -> Vec<TaskLearningRecord> {
    let mut out = Vec::new();
    let mut sequence = 1usize;
    for draft in durable_facts {
        out.push(build_task_learning_record(
            record,
            step_id,
            TaskLearningKind::DurableFact,
            draft,
            &[
                step_artifact.artifact.artifact_id.clone(),
                review_artifact.artifact.artifact_id.clone(),
            ],
            review_summary,
            sequence,
            now_secs,
        ));
        sequence = sequence.saturating_add(1);
    }
    for draft in reusable_procedures {
        out.push(build_task_learning_record(
            record,
            step_id,
            TaskLearningKind::ReusableProcedure,
            draft,
            &[
                step_artifact.artifact.artifact_id.clone(),
                review_artifact.artifact.artifact_id.clone(),
            ],
            review_summary,
            sequence,
            now_secs,
        ));
        sequence = sequence.saturating_add(1);
    }
    for draft in evidence_only {
        out.push(build_task_learning_record(
            record,
            step_id,
            TaskLearningKind::EvidenceOnly,
            draft,
            &[
                step_artifact.artifact.artifact_id.clone(),
                review_artifact.artifact.artifact_id.clone(),
            ],
            review_summary,
            sequence,
            now_secs,
        ));
        sequence = sequence.saturating_add(1);
    }
    for artifact_id in transient_artifact_ids {
        let draft = TaskLearningDraft {
            topic: format!("transient_{}_{}", record.run.run_id, artifact_id),
            summary: format!(
                "Transient artifact {} should be pruned after review",
                artifact_id
            ),
            content: format!(
                "Artifact {} from run {} / step {} was marked transient by the reviewer and should not be promoted into durable memory.",
                artifact_id, record.run.run_id, step_id
            ),
            memory_kind: None,
        };
        out.push(build_task_learning_record(
            record,
            step_id,
            TaskLearningKind::TransientArtifact,
            &draft,
            std::slice::from_ref(artifact_id),
            review_summary,
            sequence,
            now_secs,
        ));
        sequence = sequence.saturating_add(1);
    }
    out
}

pub fn run_task_learning_maintenance(
    ctx: TaskLearningMaintenanceContext<'_>,
    input: TaskLearningMaintenanceInput<'_>,
) -> Result<TaskLearningMaintenanceOutcome> {
    let all_chat_records = ctx.task_learning_store.list_for_chat(
        input.channel,
        input.chat_id,
        MAX_TASK_LEARNING_RECORDS_PER_CHAT,
    )?;
    if all_chat_records.is_empty() {
        return Ok(TaskLearningMaintenanceOutcome::default());
    }

    let pending = all_chat_records
        .iter()
        .filter(|record| record.route == TaskLearningRoute::Pending)
        .cloned()
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(TaskLearningMaintenanceOutcome::default());
    }

    let mut outcome = TaskLearningMaintenanceOutcome {
        considered: pending.len(),
        ..TaskLearningMaintenanceOutcome::default()
    };
    let mut by_run = HashMap::<String, Vec<TaskLearningRecord>>::new();
    for record in pending {
        by_run
            .entry(record.run_id.clone())
            .or_default()
            .push(record);
    }

    for (run_id, records) in by_run {
        let Some(run) = ctx.task_run_store.get(&run_id)? else {
            continue;
        };
        if !run.run.status.is_terminal() {
            continue;
        }
        let all_run_records = ctx
            .task_learning_store
            .list_for_run(&run_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)?;
        let archive_note_name = if all_run_records
            .iter()
            .any(|record| record.kind != TaskLearningKind::TransientArtifact)
        {
            resolve_task_learning_archive_note_name(&run, &all_run_records)
        } else {
            String::new()
        };
        let archive_citation = if archive_note_name.is_empty() {
            String::new()
        } else {
            format!("daily_note:{archive_note_name}")
        };
        let mut promoted_topics = HashSet::<String>::new();

        for mut record in records {
            let route_before = record.route;
            match record.kind {
                TaskLearningKind::DurableFact => {
                    let draft = build_task_learning_factual_draft(&record, &archive_citation);
                    let write = write_governed_shared_memory(
                        ctx.long_term_memory_store,
                        &[draft],
                        input.now_secs,
                        SharedMemoryWriteSource::TaskLearning,
                    )?;
                    if write.accepted > 0 {
                        record.route = TaskLearningRoute::CanonicalFactual;
                        record.route_detail = "accepted by shared factual governance".to_string();
                        outcome.canonical_writes = outcome
                            .canonical_writes
                            .saturating_add(write.changed.max(1));
                    } else {
                        record.route = TaskLearningRoute::Rejected;
                        record.route_detail = write
                            .reports
                            .first()
                            .map(|report| report.detail.clone())
                            .unwrap_or_else(|| {
                                "shared factual governance rejected the draft".to_string()
                            });
                        outcome.rejected = outcome.rejected.saturating_add(1);
                    }
                }
                TaskLearningKind::ReusableProcedure => {
                    let distinct_runs = count_distinct_procedure_runs(&all_chat_records, &record);
                    if distinct_runs >= PROCEDURE_PROMOTION_MIN_DISTINCT_RUNS {
                        let write = build_runtime_skill_write(&record, &archive_citation);
                        if upsert_runtime_skill(ctx.skill_storage, &write)? {
                            outcome.runtime_skill_promotions =
                                outcome.runtime_skill_promotions.saturating_add(1);
                        }
                        record.route = TaskLearningRoute::RuntimeSkill;
                        record.route_detail = format!(
                            "promoted after {} distinct successful task runs",
                            distinct_runs
                        );
                        promoted_topics
                            .insert(normalize_learning_match_key(&record.topic, &record.summary));
                    } else {
                        record.route = TaskLearningRoute::ArchivedEvidence;
                        record.route_detail = format!(
                            "archived as procedural evidence; waiting for repeated success ({}/{})",
                            distinct_runs, PROCEDURE_PROMOTION_MIN_DISTINCT_RUNS
                        );
                        outcome.archived_records = outcome.archived_records.saturating_add(1);
                    }
                }
                TaskLearningKind::EvidenceOnly => {
                    record.route = TaskLearningRoute::ArchivedEvidence;
                    record.route_detail = "retained as archive evidence only".to_string();
                    outcome.archived_records = outcome.archived_records.saturating_add(1);
                }
                TaskLearningKind::TransientArtifact => {
                    let mut pruned = 0usize;
                    for artifact_id in &record.source_artifact_ids {
                        pruned += usize::from(
                            ctx.task_artifact_store
                                .delete(&record.run_id, artifact_id)?,
                        );
                    }
                    record.route = TaskLearningRoute::WorkspacePruned;
                    record.route_detail = if pruned > 0 {
                        format!("pruned {} transient workspace artifact(s)", pruned)
                    } else {
                        "transient artifact already absent from workspace".to_string()
                    };
                    outcome.pruned_artifacts = outcome.pruned_artifacts.saturating_add(pruned);
                }
            }
            record.run_status = run.run.status;
            if record.kind != TaskLearningKind::TransientArtifact {
                record.archive_note_name = archive_note_name.clone();
            }
            if route_before != record.route || record.archive_note_name.is_empty() {
                outcome.updated = outcome.updated.saturating_add(1);
            }
            ctx.task_learning_store.upsert(&record)?;
        }

        if !promoted_topics.is_empty() {
            for mut existing in all_chat_records.clone() {
                if existing.kind != TaskLearningKind::ReusableProcedure {
                    continue;
                }
                let key = normalize_learning_match_key(&existing.topic, &existing.summary);
                if !promoted_topics.contains(&key)
                    || existing.route == TaskLearningRoute::RuntimeSkill
                {
                    continue;
                }
                existing.route = TaskLearningRoute::RuntimeSkill;
                if existing.route_detail.is_empty() {
                    existing.route_detail =
                        "matched a later promoted procedure for the same topic".to_string();
                }
                if existing.archive_note_name.is_empty() {
                    existing.archive_note_name = archive_note_name.clone();
                }
                ctx.task_learning_store.upsert(&existing)?;
            }
        }

        if !archive_note_name.is_empty() {
            let refreshed_run_records = ctx
                .task_learning_store
                .list_for_run(&run_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)?;
            write_task_learning_archive_note(
                ctx.memory_store,
                &run,
                &refreshed_run_records,
                &archive_note_name,
            )?;
        }
    }

    Ok(outcome)
}

pub fn retrieve_task_learning_hits(
    store: &dyn TaskLearningStore,
    channel: &str,
    chat_id: &str,
    active_run_id: Option<&str>,
    query: &str,
    limit: usize,
) -> Vec<TaskLearningHit> {
    let normalized_query = normalize_match_text(query);
    let terms = collect_terms(&normalized_query);
    let mut hits = store
        .list_for_chat(channel, chat_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.route != TaskLearningRoute::Rejected)
        .filter_map(|record| {
            score_task_learning_record(record, active_run_id, &normalized_query, &terms)
        })
        .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.record.observed_at.cmp(&a.record.observed_at))
            .then_with(|| a.record.learning_id.cmp(&b.record.learning_id))
    });
    hits.truncate(limit.min(MAX_TASK_LEARNING_HITS));
    hits
}

pub fn build_task_recall_bundle(
    active_run: &TaskRunRecord,
    store: &dyn TaskLearningStore,
    channel: &str,
    chat_id: &str,
    query: &str,
    max_len: usize,
) -> Option<String> {
    if max_len < MIN_TASK_RECALL_BLOCK_LEN {
        return None;
    }
    let step_title = current_or_next_step(active_run)
        .map(|step| step.title.as_str())
        .unwrap_or("");
    let composed_query = [query.trim(), active_run.plan.goal.trim(), step_title.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let hits = retrieve_task_learning_hits(
        store,
        channel,
        chat_id,
        Some(&active_run.run.run_id),
        &composed_query,
        3,
    );
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Task Recall Bundle\nUse prior governed task-learning results only when they fit the current run. Prefer promoted procedures and canonical facts; treat archived evidence as support, not as a direct conclusion.\n",
    );
    out.push_str(&format!(
        "Active run: {} | goal: {}\n",
        active_run.run.run_id, active_run.plan.goal
    ));
    for hit in hits {
        let line = format!(
            "- [{} / {} / {}] {} (why: {}; route={})",
            hit.record.kind.label(),
            hit.record.topic,
            hit.record.run_id,
            truncate_content_to_max(hit.record.summary.trim(), 120),
            truncate_content_to_max(&hit.reasons.join(", "), 120),
            hit.record.route.label(),
        );
        let remaining = max_len.saturating_sub(out.len()).saturating_sub(1);
        if remaining < 64 {
            break;
        }
        if line.len() > remaining {
            out.push_str(&truncate_content_to_max(&line, remaining));
            out.push('\n');
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    Some(out.trim_end().to_string())
}

pub fn build_task_learning_operator_snapshot(
    task_learning_store: &dyn TaskLearningStore,
) -> Result<TaskLearningOperatorSnapshot> {
    let mut snapshot = TaskLearningOperatorSnapshot::default();
    for record in task_learning_store.list_recent(MAX_TASK_LEARNING_RECORDS_PER_CHAT)? {
        match record.route {
            TaskLearningRoute::Pending => snapshot.pending = snapshot.pending.saturating_add(1),
            TaskLearningRoute::RuntimeSkill => {
                snapshot.runtime_skill_promoted = snapshot.runtime_skill_promoted.saturating_add(1);
            }
            TaskLearningRoute::CanonicalFactual => {
                snapshot.canonical_facts_written =
                    snapshot.canonical_facts_written.saturating_add(1);
            }
            TaskLearningRoute::ArchivedEvidence => {
                snapshot.archived_evidence = snapshot.archived_evidence.saturating_add(1);
            }
            TaskLearningRoute::WorkspacePruned => {
                snapshot.workspace_pruned = snapshot.workspace_pruned.saturating_add(1);
            }
            TaskLearningRoute::Rejected => {}
        }
        snapshot.recent_records.push(TaskLearningOperatorRecord {
            learning_id: record.learning_id,
            run_id: record.run_id,
            kind: record.kind,
            route: record.route,
            topic: record.topic,
            summary: record.summary,
            observed_at: record.observed_at,
        });
    }
    snapshot
        .recent_records
        .sort_by_key(|record| Reverse(record.observed_at));
    snapshot
        .recent_records
        .truncate(MAX_TASK_OPERATOR_ARTIFACT_PREVIEW);
    Ok(snapshot)
}

pub fn render_task_learning_operator_text(snapshot: &TaskLearningOperatorSnapshot) -> String {
    let mut out = String::from("task_learning:\n");
    out.push_str(&format!(
        "  pending: {}\n  runtime_skill_promoted: {}\n  canonical_facts_written: {}\n  archived_evidence: {}\n  workspace_pruned: {}\n",
        snapshot.pending,
        snapshot.runtime_skill_promoted,
        snapshot.canonical_facts_written,
        snapshot.archived_evidence,
        snapshot.workspace_pruned,
    ));
    if snapshot.recent_records.is_empty() {
        out.push_str("  recent_records: none\n");
        return out;
    }
    out.push_str("  recent_records:\n");
    for record in &snapshot.recent_records {
        out.push_str(&format!(
            "    - {} | {} | {} | {} | {}\n",
            record.learning_id,
            record.run_id,
            record.kind.label(),
            record.route.label(),
            record.summary
        ));
    }
    out
}

pub fn inspect_task_learning(
    store: &dyn TaskLearningStore,
    channel: &str,
    chat_id: &str,
    query: &str,
) -> TaskLearningInspection {
    let recent_records = store
        .list_for_chat(channel, chat_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)
        .unwrap_or_default();
    let related_hits = retrieve_task_learning_hits(store, channel, chat_id, None, query, 6)
        .into_iter()
        .map(|hit| hit.record)
        .collect::<Vec<_>>();
    TaskLearningInspection {
        channel: channel.to_string(),
        chat_id: chat_id.to_string(),
        query: query.trim().to_string(),
        related_hits,
        recent_records,
    }
}

pub fn render_task_learning_inspection_markdown(inspection: &TaskLearningInspection) -> String {
    let mut out = String::from("# Task Learning Inspection\n\n");
    out.push_str(&format!(
        "- channel: {}\n- chat_id: {}\n- query: {}\n",
        inspection.channel,
        inspection.chat_id,
        if inspection.query.is_empty() {
            "<empty>"
        } else {
            inspection.query.as_str()
        }
    ));
    out.push_str("\n## Related Hits\n");
    if inspection.related_hits.is_empty() {
        out.push_str("- No related task-learning hits.\n");
    } else {
        for record in &inspection.related_hits {
            out.push_str(&format!(
                "- [{} / {}] {} | route={} | run={}\n",
                record.kind.label(),
                record.topic,
                truncate_content_to_max(record.summary.trim(), 140),
                record.route.label(),
                record.run_id
            ));
        }
    }
    out.push_str("\n## Recent Records\n");
    if inspection.recent_records.is_empty() {
        out.push_str("- No task-learning records for this chat.\n");
    } else {
        for record in &inspection.recent_records {
            out.push_str(&format!(
                "- {} | {} | {} | route={} | artifacts={}\n",
                record.learning_id,
                record.kind.label(),
                truncate_content_to_max(record.summary.trim(), 120),
                record.route.label(),
                if record.source_artifact_ids.is_empty() {
                    "-".to_string()
                } else {
                    record.source_artifact_ids.join(", ")
                }
            ));
            if !record.route_detail.is_empty() {
                out.push_str(&format!("  why: {}\n", record.route_detail));
            }
        }
    }
    out.trim_end().to_string()
}

pub fn inspect_task_workspace(
    task_run_store: &dyn TaskRunStore,
    task_artifact_store: &dyn TaskArtifactStore,
    task_execution_ledger_store: &dyn TaskExecutionLedgerStore,
    task_learning_store: &dyn TaskLearningStore,
    channel: &str,
    chat_id: &str,
    run_id: Option<&str>,
) -> TaskWorkspaceInspection {
    let run = run_id
        .and_then(|run_id| task_run_store.get(run_id).ok().flatten())
        .or_else(|| {
            task_run_store
                .list_active_for_chat(channel, chat_id, 1)
                .ok()
                .and_then(|mut runs| runs.drain(..).next())
        })
        .or_else(|| {
            task_run_store
                .list_recent(MAX_TASK_OPERATOR_RECENT_RUNS)
                .ok()
                .and_then(|runs| {
                    runs.into_iter().find(|record| {
                        record.run.source_channel == channel && record.run.source_chat_id == chat_id
                    })
                })
        });
    let resolved_run_id = run
        .as_ref()
        .map(|record| record.run.run_id.clone())
        .unwrap_or_default();
    let artifacts = if resolved_run_id.is_empty() {
        Vec::new()
    } else {
        task_artifact_store
            .list_for_run(&resolved_run_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)
            .unwrap_or_default()
    };
    let ledger = if resolved_run_id.is_empty() {
        Vec::new()
    } else {
        task_execution_ledger_store
            .list(&resolved_run_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)
            .unwrap_or_default()
    };
    let learning_records = if resolved_run_id.is_empty() {
        Vec::new()
    } else {
        task_learning_store
            .list_for_run(&resolved_run_id, MAX_TASK_LEARNING_RECORDS_PER_CHAT)
            .unwrap_or_default()
    };
    TaskWorkspaceInspection {
        channel: channel.to_string(),
        chat_id: chat_id.to_string(),
        run_id: resolved_run_id,
        run,
        artifacts,
        ledger,
        learning_records,
    }
}

pub fn render_task_workspace_inspection_markdown(inspection: &TaskWorkspaceInspection) -> String {
    let mut out = String::from("# Task Workspace Inspection\n\n");
    out.push_str(&format!(
        "- channel: {}\n- chat_id: {}\n- run_id: {}\n",
        inspection.channel,
        inspection.chat_id,
        if inspection.run_id.is_empty() {
            "<none>"
        } else {
            inspection.run_id.as_str()
        }
    ));
    out.push_str("\n## Workspace\n");
    if let Some(run) = inspection.run.as_ref() {
        if let Some(block) = super::render_task_workspace_block(run, &inspection.artifacts, 1400) {
            out.push_str(block.trim());
            out.push('\n');
        }
    } else {
        out.push_str("- No matching task run.\n");
    }
    out.push_str("\n## Ledger\n");
    if inspection.ledger.is_empty() {
        out.push_str("- No ledger entries.\n");
    } else {
        for entry in &inspection.ledger {
            out.push_str(&format!(
                "- #{} {:?} [{}] {}\n",
                entry.sequence, entry.kind, entry.step_id, entry.message
            ));
        }
    }
    out.push_str("\n## Task Learning\n");
    if inspection.learning_records.is_empty() {
        out.push_str("- No task-learning records.\n");
    } else {
        for record in &inspection.learning_records {
            out.push_str(&format!(
                "- {} | {} | route={} | {}\n",
                record.learning_id,
                record.kind.label(),
                record.route.label(),
                record.summary
            ));
        }
    }
    out.trim_end().to_string()
}

fn normalize_task_learning_draft(
    mut draft: TaskLearningDraft,
    stage: &'static str,
    index: usize,
) -> Result<TaskLearningDraft> {
    draft.topic = normalize_inline(&draft.topic, MAX_TASK_TITLE_CHARS);
    if draft.topic.is_empty() {
        return Err(Error::config(
            stage,
            format!("learning draft {} topic must not be empty", index + 1),
        ));
    }
    draft.summary = normalize_multiline(&draft.summary, MAX_TASK_ARTIFACT_SUMMARY_CHARS);
    draft.content = normalize_multiline(&draft.content, MAX_TASK_ARTIFACT_CONTENT_CHARS);
    if draft.content.is_empty() {
        return Err(Error::config(
            stage,
            format!("learning draft {} content must not be empty", index + 1),
        ));
    }
    if draft.summary.is_empty() {
        draft.summary = summarize_task_artifact_content(&draft.content);
    }
    Ok(draft)
}

fn build_task_learning_record(
    record: &TaskRunRecord,
    step_id: &str,
    kind: TaskLearningKind,
    draft: &TaskLearningDraft,
    source_artifact_ids: &[String],
    review_summary: &str,
    sequence: usize,
    now_secs: u64,
) -> TaskLearningRecord {
    TaskLearningRecord {
        learning_id: format!("{}_{}_l{:02}", record.run.run_id, step_id, sequence),
        source_channel: record.run.source_channel.clone(),
        source_chat_id: record.run.source_chat_id.clone(),
        run_id: record.run.run_id.clone(),
        step_id: step_id.to_string(),
        kind,
        route: TaskLearningRoute::Pending,
        run_status: record.run.status,
        topic: draft.topic.clone(),
        summary: draft.summary.clone(),
        content: draft.content.clone(),
        memory_kind: draft.memory_kind.clone(),
        review_summary: truncate_content_to_max(review_summary.trim(), MAX_TASK_REASON_CHARS)
            .into_owned(),
        source_artifact_ids: source_artifact_ids.to_vec(),
        provenance: truncate_content_to_max(
            &format!(
                "run={} step={} artifacts={}",
                record.run.run_id,
                step_id,
                source_artifact_ids.join(",")
            ),
            MAX_TASK_PROVENANCE_CHARS,
        )
        .into_owned(),
        archive_note_name: String::new(),
        route_detail: String::new(),
        observed_at: now_secs,
    }
}

fn build_task_learning_factual_draft(
    record: &TaskLearningRecord,
    archive_citation: &str,
) -> LongTermMemoryDraft {
    LongTermMemoryDraft {
        kind: record
            .memory_kind
            .clone()
            .unwrap_or(LongTermMemoryKind::Task),
        topic: record.topic.clone(),
        content: record.content.clone(),
        keywords: collect_terms(&normalize_match_text(&format!(
            "{} {}",
            record.topic, record.summary
        ))),
        source_chat_id: Some(record.source_chat_id.clone()),
        source_type: Some(LongTermMemorySourceType::SystemRuntime),
        source_scope: Some(LongTermMemorySourceScope::User),
        confidence: Some(LongTermMemoryConfidence::High),
        freshness: Some(LongTermMemoryFreshness::Dynamic),
        stale_hint: None,
        supporting_citations: if archive_citation.trim().is_empty() {
            Vec::new()
        } else {
            vec![archive_citation.to_string()]
        },
        evidence_count: Some(record.source_artifact_ids.len().max(1) as u32),
        observed_at: Some(record.observed_at),
        last_confirmed_at: Some(record.observed_at),
        source_revision: None,
    }
}

fn build_runtime_skill_write(
    record: &TaskLearningRecord,
    archive_citation: &str,
) -> RuntimeSkillWrite {
    RuntimeSkillWrite {
        name: runtime_skill_name_for_topic(&record.topic),
        topic: record.topic.clone(),
        title: record.topic.replace('_', " "),
        summary: record.summary.clone(),
        content: record.content.clone(),
        citations: if archive_citation.trim().is_empty() {
            Vec::new()
        } else {
            vec![archive_citation.to_string()]
        },
        source_chat_id: Some(record.source_chat_id.clone()),
        observed_at: record.observed_at,
    }
}

fn resolve_task_learning_archive_note_name(
    run: &TaskRunRecord,
    all_run_records: &[TaskLearningRecord],
) -> String {
    all_run_records
        .iter()
        .find_map(|record| {
            (!record.archive_note_name.trim().is_empty()).then(|| record.archive_note_name.clone())
        })
        .unwrap_or_else(|| task_learning_note_name(run.run.created_at, &run.run.run_id))
}

fn write_task_learning_archive_note(
    memory_store: &dyn MemoryStore,
    run: &TaskRunRecord,
    all_run_records: &[TaskLearningRecord],
    note_name: &str,
) -> Result<()> {
    let content = render_task_learning_archive_note(run, all_run_records);
    memory_store.write_daily_note(note_name, &content)
}

fn render_task_learning_archive_note(
    run: &TaskRunRecord,
    records: &[TaskLearningRecord],
) -> String {
    let mut out = String::new();
    out.push_str("<!-- beetle:task-learning-archive -->\n");
    out.push_str(&format!(
        "# Task learning archive for {}\n\nRun id: {}\nChannel/chat: {}/{}\nStatus: {:?}\nTitle: {}\nGoal: {}\n\n",
        run.run.run_id,
        run.run.run_id,
        run.run.source_channel,
        run.run.source_chat_id,
        run.run.status,
        run.run.title,
        run.plan.goal
    ));
    for record in records {
        if record.kind == TaskLearningKind::TransientArtifact {
            continue;
        }
        out.push_str(&format!(
            "## {} [{}]\nSummary: {}\nRoute: {}\nArtifacts: {}\n\n{}\n\n",
            record.topic,
            record.kind.label(),
            record.summary,
            record.route.label(),
            if record.source_artifact_ids.is_empty() {
                "-".to_string()
            } else {
                record.source_artifact_ids.join(", ")
            },
            record.content
        ));
    }
    out.trim_end().to_string()
}

fn task_learning_note_name(observed_at: u64, run_id: &str) -> String {
    let (year, month, day, _, _, _) = epoch_to_ymdhms(observed_at);
    format!("{year:04}-{month:02}-{day:02}-task-{run_id}.md")
}

fn count_distinct_procedure_runs(
    records: &[TaskLearningRecord],
    target: &TaskLearningRecord,
) -> usize {
    let key = normalize_learning_match_key(&target.topic, &target.summary);
    records
        .iter()
        .filter(|record| {
            record.kind == TaskLearningKind::ReusableProcedure
                && record.route != TaskLearningRoute::Rejected
                && normalize_learning_match_key(&record.topic, &record.summary) == key
        })
        .map(|record| record.run_id.clone())
        .collect::<HashSet<_>>()
        .len()
}

fn score_task_learning_record(
    record: TaskLearningRecord,
    active_run_id: Option<&str>,
    normalized_query: &str,
    terms: &[String],
) -> Option<TaskLearningHit> {
    if record.summary.trim().is_empty() && record.content.trim().is_empty() {
        return None;
    }
    let corpus = normalize_match_text(&format!(
        "{} {} {}",
        record.topic, record.summary, record.content
    ));
    let mut score = 0u32;
    let mut reasons = Vec::new();
    if let Some(run_id) = active_run_id {
        if run_id == record.run_id {
            score = score.saturating_add(8);
            reasons.push("same active run".to_string());
        }
    }
    if !normalized_query.is_empty() {
        let overlap = terms
            .iter()
            .filter(|term| corpus.contains(term.as_str()))
            .count() as u32;
        if overlap == 0 {
            return None;
        }
        score = score.saturating_add(overlap.saturating_mul(4));
        reasons.push(format!("term_overlap={overlap}"));
    } else {
        score = score.saturating_add(1);
        reasons.push("recent task learning".to_string());
    }
    match record.route {
        TaskLearningRoute::RuntimeSkill => {
            score = score.saturating_add(6);
            reasons.push("promoted procedure".to_string());
        }
        TaskLearningRoute::CanonicalFactual => {
            score = score.saturating_add(5);
            reasons.push("canonical factual write".to_string());
        }
        TaskLearningRoute::ArchivedEvidence => {
            score = score.saturating_add(2);
            reasons.push("archived evidence".to_string());
        }
        TaskLearningRoute::Pending
        | TaskLearningRoute::WorkspacePruned
        | TaskLearningRoute::Rejected => {}
    }
    Some(TaskLearningHit {
        record,
        score,
        reasons,
    })
}

fn normalize_learning_match_key(topic: &str, summary: &str) -> String {
    normalize_match_text(&format!("{topic} {summary}"))
}

fn normalize_match_text(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&ch) {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn collect_terms(normalized: &str) -> Vec<String> {
    let mut terms = normalized
        .split_whitespace()
        .filter(|term| term.len() >= 2 || term.chars().count() >= 2)
        .map(str::to_string)
        .collect::<Vec<_>>();
    terms.truncate(12);
    terms
}

fn normalize_multiline(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn normalize_inline(value: &str, max_chars: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryKind, LongTermMemorySlot,
        LongTermMemoryStore, MemoryStore,
    };
    use crate::platform::SkillStorage;
    use crate::task_execution::{TaskArtifactRecord, TaskRun, TaskRunStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubTaskLearningStore {
        records: Mutex<HashMap<String, TaskLearningRecord>>,
    }

    impl StubTaskLearningStore {
        fn with_records(records: Vec<TaskLearningRecord>) -> Self {
            let map = records
                .into_iter()
                .map(|record| (record.learning_id.clone(), record))
                .collect();
            Self {
                records: Mutex::new(map),
            }
        }
    }

    impl TaskLearningStore for StubTaskLearningStore {
        fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(learning_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
            self.records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(record.learning_id.clone(), record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|record| {
                    record.source_channel == channel && record.source_chat_id == chat_id
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
            let mut records = self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .filter(|record| record.run_id == run_id)
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| Reverse(record.observed_at));
            records.truncate(limit);
            Ok(records)
        }
    }

    struct StubTaskRunStore {
        records: HashMap<String, TaskRunRecord>,
    }

    impl StubTaskRunStore {
        fn new(records: Vec<TaskRunRecord>) -> Self {
            Self {
                records: records
                    .into_iter()
                    .map(|record| (record.run.run_id.clone(), record))
                    .collect(),
            }
        }
    }

    impl TaskRunStore for StubTaskRunStore {
        fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
            Ok(self.records.get(run_id).cloned())
        }

        fn upsert(&self, _record: &TaskRunRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
            let mut records = self.records.values().cloned().collect::<Vec<_>>();
            records.sort_by_key(|record| Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<TaskRunRecord>> {
            let mut records = self
                .records
                .values()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && record.run.status.is_active()
                })
                .cloned()
                .collect::<Vec<_>>();
            records.sort_by_key(|record| Reverse(record.run.updated_at));
            records.truncate(limit);
            Ok(records)
        }
    }

    #[derive(Default)]
    struct StubTaskArtifactStore {
        deleted: Mutex<Vec<(String, String)>>,
    }

    impl TaskArtifactStore for StubTaskArtifactStore {
        fn put(&self, _record: &TaskArtifactRecord) -> Result<()> {
            Ok(())
        }

        fn list_for_run(&self, _run_id: &str, _limit: usize) -> Result<Vec<TaskArtifactRecord>> {
            Ok(Vec::new())
        }

        fn delete(&self, run_id: &str, artifact_id: &str) -> Result<bool> {
            self.deleted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((run_id.to_string(), artifact_id.to_string()));
            Ok(true)
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        drafts: Mutex<Vec<LongTermMemoryDraft>>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.drafts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(drafts);
            Ok(drafts.len())
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.drafts.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubMemoryStore {
        notes: Mutex<HashMap<String, String>>,
    }

    impl MemoryStore for StubMemoryStore {
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
            Ok(self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write_daily_note(&self, name: &str, content: &str) -> Result<()> {
            self.notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_string());
            Ok(())
        }
    }

    fn make_run_record(run_id: &str, status: TaskRunStatus, now_secs: u64) -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: run_id.to_string(),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-1".to_string(),
                user_request: "Finish the migration".to_string(),
                title: "Migration task".to_string(),
                status,
                current_step_id: "s01".to_string(),
                planner_reason: "complex task".to_string(),
                final_summary: String::new(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: now_secs,
                updated_at: now_secs,
                finished_at: now_secs,
            },
            plan: crate::task_execution::TaskPlan {
                goal: "Finish the migration".to_string(),
                completion_definition: "All required artifacts are produced".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![crate::task_execution::TaskStep {
                    step_id: "s01".to_string(),
                    title: "Inspect previous outcomes".to_string(),
                    instruction: "Review prior work".to_string(),
                    status: crate::task_execution::TaskStepStatus::Passed,
                    tool_budget: 2,
                    retry_budget: 1,
                    expected_artifacts: Vec::new(),
                    review_criteria: Vec::new(),
                    attempt_count: 1,
                    last_result_summary: String::new(),
                    last_review_summary: String::new(),
                    started_at: now_secs,
                    finished_at: now_secs,
                }],
            },
        }
    }

    fn make_learning_record(
        learning_id: &str,
        run_id: &str,
        kind: TaskLearningKind,
        route: TaskLearningRoute,
        topic: &str,
        summary: &str,
        content: &str,
        observed_at: u64,
    ) -> TaskLearningRecord {
        TaskLearningRecord {
            learning_id: learning_id.to_string(),
            source_channel: "telegram".to_string(),
            source_chat_id: "chat-1".to_string(),
            run_id: run_id.to_string(),
            step_id: "s01".to_string(),
            kind,
            route,
            run_status: TaskRunStatus::Completed,
            topic: topic.to_string(),
            summary: summary.to_string(),
            content: content.to_string(),
            memory_kind: Some(LongTermMemoryKind::Fact),
            review_summary: "reviewed".to_string(),
            source_artifact_ids: vec!["a01".to_string()],
            provenance: "run=tr001 step=s01 artifacts=a01".to_string(),
            archive_note_name: String::new(),
            route_detail: String::new(),
            observed_at,
        }
    }

    #[test]
    fn task_learning_draft_requires_topic_and_content() {
        let err = normalize_task_learning_drafts(
            &mut vec![TaskLearningDraft {
                topic: String::new(),
                summary: String::new(),
                content: String::new(),
                memory_kind: None,
            }],
            "task_learning",
        )
        .unwrap_err();
        assert_eq!(err.stage(), "task_learning");
    }

    #[test]
    fn task_learning_note_name_uses_run_date() {
        assert_eq!(
            task_learning_note_name(crate::util::ymdhms_to_epoch(2026, 4, 6, 8, 0, 0), "tr001"),
            "2026-04-06-task-tr001.md"
        );
    }

    #[test]
    fn task_recall_bundle_includes_ranked_learning_hits() {
        let active_run = make_run_record("tr_active", TaskRunStatus::Running, 1_800_000_000);
        let store = StubTaskLearningStore::with_records(vec![
            make_learning_record(
                "tl1",
                "tr_old_1",
                TaskLearningKind::ReusableProcedure,
                TaskLearningRoute::RuntimeSkill,
                "apply_release_patch",
                "Previous successful release fix path",
                "1. inspect release diff\n2. patch rollback guards\n3. verify logs",
                1_800_000_010,
            ),
            make_learning_record(
                "tl2",
                "tr_old_2",
                TaskLearningKind::EvidenceOnly,
                TaskLearningRoute::ArchivedEvidence,
                "release_blocker",
                "Previous blocker evidence",
                "The last release failed because the guard missed a missing artifact.",
                1_800_000_000,
            ),
        ]);

        let bundle = build_task_recall_bundle(
            &active_run,
            &store,
            "telegram",
            "chat-1",
            "Need the release fix path",
            520,
        )
        .expect("task recall bundle should be built");

        assert!(bundle.contains("## Task Recall Bundle"));
        assert!(bundle.contains("apply_release_patch"));
        assert!(bundle.contains("runtime_skill"));
        assert!(bundle.contains("release_blocker"));
    }

    #[test]
    fn task_learning_maintenance_routes_pending_records_across_all_destinations() {
        let now_secs = crate::util::ymdhms_to_epoch(2026, 4, 6, 9, 0, 0);
        let run = make_run_record("tr001", TaskRunStatus::Completed, now_secs);
        let prior_procedure = make_learning_record(
            "tl_old_proc",
            "tr000",
            TaskLearningKind::ReusableProcedure,
            TaskLearningRoute::ArchivedEvidence,
            "apply_release_patch",
            "Stable release patch sequence",
            "1. inspect release diff\n2. patch rollback guards\n3. verify logs",
            now_secs.saturating_sub(60),
        );
        let pending_fact = make_learning_record(
            "tl_fact",
            "tr001",
            TaskLearningKind::DurableFact,
            TaskLearningRoute::Pending,
            "release_root_cause",
            "The failure came from a missing artifact guard",
            "Root cause: the release pipeline skipped an artifact presence guard.",
            now_secs,
        );
        let pending_procedure = make_learning_record(
            "tl_proc",
            "tr001",
            TaskLearningKind::ReusableProcedure,
            TaskLearningRoute::Pending,
            "apply_release_patch",
            "Stable release patch sequence",
            "1. inspect release diff\n2. patch rollback guards\n3. verify logs",
            now_secs,
        );
        let pending_evidence = make_learning_record(
            "tl_ev",
            "tr001",
            TaskLearningKind::EvidenceOnly,
            TaskLearningRoute::Pending,
            "release_observation",
            "Observed deployment output",
            "Observed warning logs and artifact mismatches during the failed rollout.",
            now_secs,
        );
        let mut pending_transient = make_learning_record(
            "tl_transient",
            "tr001",
            TaskLearningKind::TransientArtifact,
            TaskLearningRoute::Pending,
            "transient_tr001_a09",
            "Scratch output that should be pruned",
            "Temporary scratch content.",
            now_secs,
        );
        pending_transient.memory_kind = None;
        pending_transient.source_artifact_ids = vec!["a09".to_string()];

        let learning_store = StubTaskLearningStore::with_records(vec![
            prior_procedure.clone(),
            pending_fact.clone(),
            pending_procedure.clone(),
            pending_evidence.clone(),
            pending_transient.clone(),
        ]);
        let task_run_store = StubTaskRunStore::new(vec![
            make_run_record(
                "tr000",
                TaskRunStatus::Completed,
                now_secs.saturating_sub(60),
            ),
            run.clone(),
        ]);
        let task_artifact_store = StubTaskArtifactStore::default();
        let long_term_memory_store = StubLongTermMemoryStore::default();
        let skill_storage = StubSkillStorage::default();
        let memory_store = StubMemoryStore::default();

        let outcome = run_task_learning_maintenance(
            TaskLearningMaintenanceContext {
                task_run_store: &task_run_store,
                task_artifact_store: &task_artifact_store,
                task_learning_store: &learning_store,
                long_term_memory_store: &long_term_memory_store,
                skill_storage: &skill_storage,
                memory_store: &memory_store,
            },
            TaskLearningMaintenanceInput {
                channel: "telegram",
                chat_id: "chat-1",
                now_secs,
            },
        )
        .expect("task learning maintenance should succeed");

        assert_eq!(outcome.considered, 4);
        assert_eq!(outcome.canonical_writes, 1);
        assert_eq!(outcome.runtime_skill_promotions, 1);
        assert_eq!(outcome.archived_records, 1);
        assert_eq!(outcome.pruned_artifacts, 1);

        let fact = learning_store
            .get("tl_fact")
            .expect("fact read")
            .expect("fact exists");
        assert_eq!(fact.route, TaskLearningRoute::CanonicalFactual);
        assert!(!fact.archive_note_name.is_empty());

        let procedure = learning_store
            .get("tl_proc")
            .expect("proc read")
            .expect("proc exists");
        assert_eq!(procedure.route, TaskLearningRoute::RuntimeSkill);

        let prior_promoted = learning_store
            .get("tl_old_proc")
            .expect("prior proc read")
            .expect("prior proc exists");
        assert_eq!(prior_promoted.route, TaskLearningRoute::RuntimeSkill);

        let evidence = learning_store
            .get("tl_ev")
            .expect("evidence read")
            .expect("evidence exists");
        assert_eq!(evidence.route, TaskLearningRoute::ArchivedEvidence);

        let transient = learning_store
            .get("tl_transient")
            .expect("transient read")
            .expect("transient exists");
        assert_eq!(transient.route, TaskLearningRoute::WorkspacePruned);

        let deleted = task_artifact_store
            .deleted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert_eq!(deleted, vec![("tr001".to_string(), "a09".to_string())]);

        let stored_drafts = long_term_memory_store
            .drafts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert_eq!(stored_drafts.len(), 1);
        assert_eq!(stored_drafts[0].topic, "release_root_cause");

        let skill_names = skill_storage.list_names().expect("skill names");
        assert!(skill_names
            .iter()
            .any(|name| name == &runtime_skill_name_for_topic("apply_release_patch")));

        let note_names = memory_store.list_daily_note_names(8).expect("note names");
        assert_eq!(note_names.len(), 1);
        let note_body = memory_store
            .get_daily_note(&note_names[0])
            .expect("note body should load");
        assert!(note_body.contains("Task learning archive for tr001"));
        assert!(note_body.contains("release_root_cause"));
        assert!(note_body.contains("apply_release_patch"));
    }
}
