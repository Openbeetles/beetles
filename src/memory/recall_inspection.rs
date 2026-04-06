//! Working-recall inspection for operator diagnostics.
//! 工作级 recall 巡检：统一查看 canonical factual recall、archive query 报告与 prompt selector 结果。

use crate::platform::SkillStorage;
use crate::task_execution::{
    active_task_run_for_chat, build_task_recall_bundle, TaskLearningStore, TaskRunStore,
};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{
    build_archive_evidence_block, build_shared_factual_plane_snapshot, inspect_archive_recall,
    inspect_runtime_skill_recall, inspect_shared_factual_recall, inspect_task_recall,
    memory_policy, parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    render_exact_long_term_memory_block, search_archive_records_detailed,
    select_archive_hits_for_prompt_with_report, ArchivePromptSelectionReport, ArchiveSearchHit,
    ArchiveSearchQuery, ArchiveSearchQueryReport, LongTermMemoryStore, MemoryProfile, MemoryStore,
    RecallPlane, RecallQuery, RecallSelectionReport, SessionMessage, SessionStore,
    SharedFactualPlaneSnapshot, TurnLedgerStore,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingRecallInspection {
    pub chat_id: String,
    pub query: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_term_memory_text: Option<String>,
    pub shared_factual_plane: SharedFactualPlaneSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_evidence_text: Option<String>,
    #[serde(default)]
    pub shared_factual_report: RecallSelectionReport,
    #[serde(default)]
    pub archive_recall_report: RecallSelectionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_skill_text: Option<String>,
    #[serde(default)]
    pub runtime_skill_report: RecallSelectionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_recall_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_recall_report: Option<RecallSelectionReport>,
    pub archive_query_report: ArchiveSearchQueryReport,
    pub archive_selector_report: ArchivePromptSelectionReport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive_hits: Vec<ArchiveSearchHit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_archive_hits: Vec<ArchiveSearchHit>,
}

pub struct WorkingRecallInspectionInput<'a> {
    pub chat_id: &'a str,
    pub query: &'a str,
    pub summary_text: Option<&'a str>,
    pub recent: &'a [SessionMessage],
    pub system_max_len: usize,
    pub profile: MemoryProfile,
    pub current_channel: Option<&'a str>,
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: Option<&'a dyn SkillStorage>,
    pub task_run_store: Option<&'a dyn TaskRunStore>,
    pub task_learning_store: Option<&'a dyn TaskLearningStore>,
}

pub fn inspect_working_recall(input: WorkingRecallInspectionInput<'_>) -> WorkingRecallInspection {
    let recall_policy = memory_policy(input.profile).long_term_recall;
    let recall_budget = input.system_max_len.min(recall_policy.block_max_len_cap);
    let long_term_memory_text = parse_explicit_long_term_slot_query(input.query)
        .and_then(|slot| {
            render_exact_long_term_memory_block(input.long_term_memory_store, &slot, recall_budget)
        })
        .or_else(|| {
            recall_long_term_memory_block(
                input.long_term_memory_store,
                input.chat_id,
                input.query,
                input.summary_text,
                input.recent,
                recall_budget,
                input.profile,
            )
        });
    let shared_factual_plane = build_shared_factual_plane_snapshot(
        input.session_store,
        input.long_term_memory_store,
        input.memory_store,
        input.turn_ledger_store,
        input.chat_id,
        input.query,
        input.summary_text,
        input.recent,
        input
            .system_max_len
            .min(recall_policy.block_max_len_cap.saturating_add(384)),
        input.profile,
    );
    let shared_factual_report = inspect_shared_factual_recall(
        input.long_term_memory_store,
        input.chat_id,
        input.query,
        input.summary_text,
        input.recent,
        input
            .system_max_len
            .min(recall_policy.block_max_len_cap.saturating_add(384)),
        input.profile,
        crate::util::current_unix_secs(),
    );
    let archive_result = search_archive_records_detailed(
        input.session_store,
        input.memory_store,
        input.turn_ledger_store,
        ArchiveSearchQuery {
            query: input.query,
            preferred_chat_id: Some(input.chat_id),
            chat_id_filter: None,
            sources: &[],
            limit: super::MAX_ARCHIVE_SEARCH_LIMIT,
        },
    )
    .unwrap_or_default();
    let archive_recall_report = inspect_archive_recall(
        input.session_store,
        input.memory_store,
        input.turn_ledger_store,
        input.chat_id,
        input.query,
        input.summary_text,
        input.recent,
        input.system_max_len.min(768),
        input.profile,
    );
    let selection = select_archive_hits_for_prompt_with_report(
        archive_result.hits.clone(),
        input.profile,
        input.system_max_len.min(768),
    );
    let archive_evidence_text = build_archive_evidence_block(
        input.session_store,
        input.memory_store,
        input.turn_ledger_store,
        input.chat_id,
        input.query,
        input.system_max_len.min(768),
        input.profile,
    );
    let runtime_skill_text = input.skill_storage.and_then(|storage| {
        crate::skills::build_runtime_skill_recall_block(
            storage,
            input.query,
            Some(input.chat_id),
            crate::util::current_unix_secs(),
            input.system_max_len.min(420),
        )
    });
    let runtime_skill_report = input.skill_storage.map_or_else(
        || RecallSelectionReport {
            plane: RecallPlane::RuntimeSkill,
            query: RecallQuery {
                plane: RecallPlane::RuntimeSkill,
                ..RecallQuery::default()
            },
            backend: "runtime_skill_hybrid".to_string(),
            candidate_count: 0,
            selected_count: 0,
            selected_ids: Vec::new(),
            miss_reason: Some("skill_storage_unavailable".to_string()),
            selection_note: None,
            candidates: Vec::new(),
        },
        |storage| {
            inspect_runtime_skill_recall(
                storage,
                input.query,
                Some(input.chat_id),
                input.summary_text,
                input.recent,
                crate::util::current_unix_secs(),
                input.system_max_len.min(420),
            )
        },
    );
    let active_task_run = match (input.current_channel, input.task_run_store) {
        (Some(channel), Some(task_run_store)) => {
            active_task_run_for_chat(task_run_store, channel, input.chat_id)
                .ok()
                .flatten()
        }
        _ => None,
    };
    let task_recall_text = match (
        active_task_run.as_ref(),
        input.current_channel,
        input.task_learning_store,
    ) {
        (Some(run), Some(channel), Some(task_learning_store)) => build_task_recall_bundle(
            run,
            task_learning_store,
            channel,
            input.chat_id,
            input.query,
            input.system_max_len.min(520),
        ),
        _ => None,
    };
    let task_recall_report = match (
        active_task_run.as_ref(),
        input.current_channel,
        input.task_learning_store,
    ) {
        (run, Some(channel), Some(task_learning_store)) => Some(inspect_task_recall(
            run,
            task_learning_store,
            channel,
            input.chat_id,
            input.query,
            input.summary_text,
            input.recent,
            input.system_max_len.min(520),
        )),
        _ => None,
    };
    WorkingRecallInspection {
        chat_id: input.chat_id.to_string(),
        query: input.query.trim().to_string(),
        profile: match input.profile {
            MemoryProfile::Embedded => "embedded".to_string(),
            MemoryProfile::Standard => "standard".to_string(),
        },
        summary_text: input
            .summary_text
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        long_term_memory_text,
        shared_factual_plane,
        archive_evidence_text,
        shared_factual_report,
        archive_recall_report,
        runtime_skill_text,
        runtime_skill_report,
        task_recall_text,
        task_recall_report,
        archive_query_report: archive_result.report,
        archive_selector_report: selection.report,
        archive_hits: archive_result.hits,
        selected_archive_hits: selection.hits,
    }
}

pub fn render_working_recall_inspection_markdown(inspection: &WorkingRecallInspection) -> String {
    let mut out = String::from("# Working Recall Inspection\n\n");
    out.push_str(&format!(
        "- chat_id: {}\n- profile: {}\n- query: {}\n",
        inspection.chat_id,
        inspection.profile,
        if inspection.query.is_empty() {
            "<empty>"
        } else {
            inspection.query.as_str()
        }
    ));
    if let Some(summary) = inspection.summary_text.as_deref() {
        out.push_str(&format!(
            "- summary: {}\n",
            truncate_content_to_max(summary, 220)
        ));
    }

    out.push_str("\n## Canonical Recall\n");
    if let Some(text) = inspection.long_term_memory_text.as_deref() {
        out.push_str(text.trim());
        out.push('\n');
    } else {
        out.push_str("- No canonical long-term recall block.\n");
    }

    out.push_str("\n## Shared Factual Plane\n");
    if let Some(block) = inspection.shared_factual_plane.block.as_deref() {
        out.push_str(block.trim());
        out.push('\n');
    } else {
        out.push_str("- No shared factual plane block.\n");
    }
    out.push_str(&format!(
        "- shared_factual_report: backend={}; candidates={}; selected={}\n",
        inspection.shared_factual_report.backend,
        inspection.shared_factual_report.candidate_count,
        inspection.shared_factual_report.selected_count,
    ));
    if let Some(reason) = inspection.shared_factual_report.miss_reason.as_deref() {
        out.push_str(&format!("- shared_factual_miss_reason: {}\n", reason));
    }

    out.push_str("\n## Archive Query Report\n");
    out.push_str(&format!(
        "- backend: {:?}\n- candidates: {}\n- hits: {}\n- selected: {}\n",
        inspection.archive_query_report.backend,
        inspection.archive_query_report.candidate_count,
        inspection.archive_query_report.returned_hit_count,
        inspection.archive_selector_report.selected_hits
    ));
    if !inspection.archive_query_report.normalized_terms.is_empty() {
        out.push_str(&format!(
            "- terms: {}\n",
            inspection.archive_query_report.normalized_terms.join(", ")
        ));
    }
    if let Some(reason) = inspection.archive_query_report.miss_reason.as_deref() {
        out.push_str(&format!("- miss_reason: {}\n", reason));
    }
    if let Some(note) = inspection.archive_selector_report.selection_note.as_deref() {
        out.push_str(&format!("- selector_note: {}\n", note));
    }
    if !inspection.archive_query_report.source_stats.is_empty() {
        out.push_str("- source_stats:\n");
        for stats in &inspection.archive_query_report.source_stats {
            out.push_str(&format!(
                "  - {}: candidates={}, hits={}\n",
                stats.source.label(),
                stats.candidate_count,
                stats.hit_count
            ));
        }
    }

    out.push_str("\n## Selected Archive Evidence\n");
    if inspection.selected_archive_hits.is_empty() {
        out.push_str("- No archive hits selected for prompt injection.\n");
    } else {
        for hit in &inspection.selected_archive_hits {
            let selector_reason = hit
                .retrieval_trace
                .as_ref()
                .and_then(|trace| trace.selector_reason.as_deref())
                .unwrap_or("no selector reason");
            out.push_str(&format!(
                "- [{}] {} | {} | why={}\n",
                hit.source.label(),
                hit.citation,
                truncate_content_to_max(hit.excerpt.trim(), 140),
                truncate_content_to_max(selector_reason, 140)
            ));
        }
    }

    if let Some(block) = inspection.archive_evidence_text.as_deref() {
        out.push_str("\n## Prompt Archive Block\n");
        out.push_str(block.trim());
        out.push('\n');
    }

    out.push_str("\n## Runtime Skill Recall\n");
    out.push_str(&format!(
        "- backend: {}\n- candidates: {}\n- selected: {}\n",
        inspection.runtime_skill_report.backend,
        inspection.runtime_skill_report.candidate_count,
        inspection.runtime_skill_report.selected_count
    ));
    if let Some(reason) = inspection.runtime_skill_report.miss_reason.as_deref() {
        out.push_str(&format!("- miss_reason: {}\n", reason));
    }
    if let Some(block) = inspection.runtime_skill_text.as_deref() {
        out.push_str(block.trim());
        out.push('\n');
    } else {
        out.push_str("- No runtime skill recall block.\n");
    }

    out.push_str("\n## Task Recall\n");
    if let Some(report) = inspection.task_recall_report.as_ref() {
        out.push_str(&format!(
            "- backend: {}\n- candidates: {}\n- selected: {}\n",
            report.backend, report.candidate_count, report.selected_count
        ));
        if let Some(reason) = report.miss_reason.as_deref() {
            out.push_str(&format!("- miss_reason: {}\n", reason));
        }
    } else {
        out.push_str("- Task recall inspection unavailable for this request.\n");
    }
    if let Some(block) = inspection.task_recall_text.as_deref() {
        out.push_str(block.trim());
        out.push('\n');
    } else {
        out.push_str("- No task recall block.\n");
    }

    out.trim_end().to_string()
}
