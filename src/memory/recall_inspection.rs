//! Working-recall inspection for operator diagnostics.
//! 工作级 recall 巡检：统一查看 canonical factual recall、archive query 报告与 prompt selector 结果。

use crate::agent::{load_active_work_for_chat, ActiveWorkStore};
use crate::platform::SkillStorage;
use crate::task_execution::{
    active_task_run_for_chat, build_task_recall_bundle, TaskLearningStore, TaskRunStore,
};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{
    build_archive_evidence_block, build_continuity_recall_query, build_cross_plane_rerank_result,
    build_shared_factual_plane_snapshot, build_work_continuity_record, decide_prompt_recall_route,
    inspect_archive_recall, inspect_continuity_capsule_recall, inspect_runtime_skill_recall,
    inspect_shared_factual_recall, inspect_task_recall, memory_policy,
    parse_explicit_long_term_slot_query, recall_long_term_memory_block,
    render_continuity_capsule_block, render_exact_long_term_memory_block,
    render_work_continuity_block, search_archive_records_detailed,
    select_archive_hits_for_prompt_with_report, ArchivePromptSelectionReport, ArchiveSearchHit,
    ArchiveSearchQuery, ArchiveSearchQueryReport, ContinuityCapsule,
    ContinuityCapsuleRecallInspectionInput, ContinuityCapsuleScopeKind, ContinuityCapsuleStore,
    CrossPlaneRerankInput, CrossPlaneRerankResult, LongTermMemoryStore, MemoryProfile, MemoryStore,
    PromptRecallIntent, RecallPlane, RecallQuery, RecallSelectionReport, SessionMessage,
    SessionStore, SharedFactualPlaneSnapshot, TurnLedgerStore, MAX_WORK_CONTINUITY_BLOCK_LEN,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkingRecallInspection {
    pub chat_id: String,
    pub query: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_continuity_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_term_memory_text: Option<String>,
    pub shared_factual_plane: SharedFactualPlaneSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_evidence_text: Option<String>,
    #[serde(default)]
    pub shared_factual_report: RecallSelectionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuity_capsule_text: Option<String>,
    #[serde(default)]
    pub continuity_capsule_report: RecallSelectionReport,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continuity_capsules: Vec<ContinuityCapsule>,
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
    #[serde(default)]
    pub prompt_recall_intent: PromptRecallIntent,
    #[serde(default)]
    pub cross_plane_rerank: CrossPlaneRerankResult,
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
    pub active_work_store: Option<&'a dyn ActiveWorkStore>,
    pub continuity_capsule_store: &'a dyn ContinuityCapsuleStore,
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
    let active_task_run = match (input.current_channel, input.task_run_store) {
        (Some(channel), Some(task_run_store)) => {
            active_task_run_for_chat(task_run_store, channel, input.chat_id)
                .ok()
                .flatten()
        }
        _ => None,
    };
    let active_work = input.active_work_store.and_then(|store| {
        load_active_work_for_chat(store, active_task_run.as_ref(), input.chat_id)
            .ok()
            .flatten()
    });
    let work_continuity_text =
        build_work_continuity_record(active_work.as_ref(), input.summary_text).and_then(|record| {
            render_work_continuity_block(
                &record,
                input.system_max_len.min(MAX_WORK_CONTINUITY_BLOCK_LEN),
            )
        });
    let continuity_query = build_continuity_recall_query(
        input.query,
        input.summary_text,
        input.recent,
        active_work.as_ref(),
        active_task_run.as_ref(),
    );
    let (continuity_capsule_report, continuity_capsules) =
        inspect_continuity_capsule_recall(ContinuityCapsuleRecallInspectionInput {
            store: input.continuity_capsule_store,
            scope_kind: ContinuityCapsuleScopeKind::Chat,
            scope_id: input.chat_id,
            preferred_chat_id: Some(input.chat_id),
            query: &continuity_query,
            summary_text: input.summary_text,
            recent_messages: input.recent,
            max_chars: input.system_max_len.min(480),
            now_secs: crate::util::current_unix_secs(),
        });
    let continuity_capsule_text =
        render_continuity_capsule_block(&continuity_capsules, input.system_max_len.min(480));
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
    let prompt_recall_intent =
        decide_prompt_recall_route(super::recall_router::PromptRecallRouterInput {
            user_query: input.query,
            has_active_continuity: active_work.is_some(),
            has_active_task_run: active_task_run.is_some(),
            shared_factual_report: &shared_factual_report,
            continuity_capsule_report: &continuity_capsule_report,
            archive_report: &archive_recall_report,
            runtime_skill_report: &runtime_skill_report,
            task_recall_report: task_recall_report.as_ref(),
        })
        .intent;
    let cross_plane_rerank = build_cross_plane_rerank_result(CrossPlaneRerankInput {
        intent: prompt_recall_intent,
        shared_factual_report: &shared_factual_report,
        continuity_capsule_report: &continuity_capsule_report,
        archive_report: &archive_recall_report,
        runtime_skill_report: &runtime_skill_report,
        task_recall_report: task_recall_report.as_ref(),
    });
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
        work_continuity_text,
        long_term_memory_text,
        shared_factual_plane,
        archive_evidence_text,
        shared_factual_report,
        continuity_capsule_text,
        continuity_capsule_report,
        continuity_capsules,
        archive_recall_report,
        runtime_skill_text,
        runtime_skill_report,
        task_recall_text,
        task_recall_report,
        prompt_recall_intent,
        cross_plane_rerank,
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
    out.push_str(&format!(
        "- prompt_intent: {}\n",
        inspection.prompt_recall_intent.label()
    ));

    out.push_str("\n## Work Continuity\n");
    if let Some(text) = inspection.work_continuity_text.as_deref() {
        out.push_str(text.trim());
        out.push('\n');
    } else {
        out.push_str("- No work continuity projection.\n");
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

    out.push_str("\n## Continuity Capsules\n");
    if let Some(block) = inspection.continuity_capsule_text.as_deref() {
        out.push_str(block.trim());
        out.push('\n');
    } else {
        out.push_str("- No continuity capsule block.\n");
    }
    out.push_str(&format!(
        "- continuity_capsule_report: backend={}; candidates={}; selected={}\n",
        inspection.continuity_capsule_report.backend,
        inspection.continuity_capsule_report.candidate_count,
        inspection.continuity_capsule_report.selected_count,
    ));
    if let Some(reason) = inspection.continuity_capsule_report.miss_reason.as_deref() {
        out.push_str(&format!("- continuity_capsule_miss_reason: {}\n", reason));
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

    out.push_str("\n## Cross-Plane Rerank\n");
    if inspection.cross_plane_rerank.plane_signals.is_empty() {
        out.push_str("- No cross-plane rerank signals.\n");
    } else {
        out.push_str(&format!(
            "- intent: {}\n",
            inspection.cross_plane_rerank.intent.label()
        ));
        for signal in &inspection.cross_plane_rerank.plane_signals {
            out.push_str(&format!(
                "- plane={} signal={} top={} candidates={} selected={}\n",
                signal.plane.label(),
                signal.signal_score,
                signal.top_rerank_score,
                signal.candidate_count,
                signal.selected_count
            ));
            if let Some(reason) = signal.top_reason.as_deref() {
                out.push_str(&format!(
                    "  why: {}\n",
                    truncate_content_to_max(reason, 140)
                ));
            }
        }
        if !inspection.cross_plane_rerank.top_candidates.is_empty() {
            out.push_str("- top_candidates:\n");
            for candidate in &inspection.cross_plane_rerank.top_candidates {
                out.push_str(&format!(
                    "  - {} {} score={} selected={} source={}\n",
                    candidate.plane.label(),
                    truncate_content_to_max(&candidate.title, 72),
                    candidate.rerank_score,
                    candidate.selected,
                    candidate.source
                ));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        apply_continuity_capsule_drafts, ContinuityCapsule, ContinuityCapsuleDraft,
        ContinuityCapsuleStore, CrossPlanePlaneSignal, CrossPlaneRerankCandidate, MemoryStore,
        SessionStore, SharedFactualPlaneSnapshot, TurnLedger, TurnLedgerStore,
    };
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            Ok(self.recent.iter().take(limit).cloned().collect())
        }

        fn message_count(&self, _chat_id: &str) -> Result<usize> {
            Ok(self.recent.len())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubMemoryStore {
        daily_notes: Vec<(String, String)>,
    }

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            Ok(self
                .daily_notes
                .iter()
                .take(recent_n)
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .daily_notes
                .iter()
                .find(|(note_name, _)| note_name == name)
                .map(|(_, content)| content.clone())
                .unwrap_or_default())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
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

    #[derive(Default)]
    struct StubLongTermMemoryStore;

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<crate::memory::LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct StubContinuityCapsuleStore {
        entries: Mutex<Vec<ContinuityCapsule>>,
    }

    impl ContinuityCapsuleStore for StubContinuityCapsuleStore {
        fn upsert_many(
            &self,
            drafts: &[ContinuityCapsuleDraft],
            now_secs: u64,
        ) -> Result<crate::memory::ContinuityCapsuleWriteOutcome> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            Ok(apply_continuity_capsule_drafts(
                &mut entries,
                drafts,
                now_secs,
            ))
        }

        fn get(&self, capsule_id: &str) -> Result<Option<ContinuityCapsule>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.capsule_id == capsule_id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<ContinuityCapsule>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
        }
    }

    #[test]
    fn inspection_markdown_includes_cross_plane_rerank_section() {
        let markdown = render_working_recall_inspection_markdown(&WorkingRecallInspection {
            chat_id: "chat-1".to_string(),
            query: "继续 release patch".to_string(),
            profile: "standard".to_string(),
            summary_text: Some("summary".to_string()),
            work_continuity_text: Some(
                "## Work Continuity\nFocus: release patch\nStatus: active".to_string(),
            ),
            long_term_memory_text: None,
            shared_factual_plane: SharedFactualPlaneSnapshot {
                block: None,
                observations: Vec::new(),
            },
            archive_evidence_text: None,
            shared_factual_report: RecallSelectionReport::default(),
            continuity_capsule_text: None,
            continuity_capsule_report: RecallSelectionReport::default(),
            continuity_capsules: Vec::new(),
            archive_recall_report: RecallSelectionReport::default(),
            runtime_skill_text: None,
            runtime_skill_report: RecallSelectionReport::default(),
            task_recall_text: None,
            task_recall_report: None,
            prompt_recall_intent: PromptRecallIntent::Procedural,
            cross_plane_rerank: CrossPlaneRerankResult {
                intent: PromptRecallIntent::Procedural,
                plane_signals: vec![CrossPlanePlaneSignal {
                    plane: RecallPlane::RuntimeSkill,
                    candidate_count: 2,
                    selected_count: 1,
                    top_rerank_score: 48,
                    signal_score: 60,
                    top_candidate_id: Some("runtime-1".to_string()),
                    top_reason: Some("intent=procedural".to_string()),
                }],
                top_candidates: vec![CrossPlaneRerankCandidate {
                    plane: RecallPlane::RuntimeSkill,
                    candidate_id: "runtime-1".to_string(),
                    title: "Release patch flow".to_string(),
                    source: "runtime_skill".to_string(),
                    selected: true,
                    original_total_score: 32,
                    rerank_score: 48,
                    reasons: vec!["intent=procedural".to_string()],
                }],
            },
            archive_query_report: ArchiveSearchQueryReport {
                query: "继续 release patch".to_string(),
                normalized_terms: Vec::new(),
                preferred_chat_id: Some("chat-1".to_string()),
                chat_id_filter: None,
                requested_sources: Vec::new(),
                limit: 4,
                weak_query: false,
                backend: Default::default(),
                candidate_count: 0,
                returned_hit_count: 0,
                top_citations: Vec::new(),
                top_match_terms: Vec::new(),
                source_stats: Vec::new(),
                miss_reason: None,
            },
            archive_selector_report: ArchivePromptSelectionReport {
                input_hits: 0,
                selected_hits: 0,
                max_items: 0,
                max_chars: 0,
                skipped_by_chars: 0,
                skipped_by_similarity: 0,
                deferred_by_quota: 0,
                relaxed_quota_selected: 0,
                source_stats: Vec::new(),
                selection_note: None,
            },
            archive_hits: Vec::new(),
            selected_archive_hits: Vec::new(),
        });

        assert!(markdown.contains("## Cross-Plane Rerank"));
        assert!(markdown.contains("## Work Continuity"));
        assert!(markdown.contains("intent: procedural"));
        assert!(markdown.contains("plane=runtime_skill"));
        assert!(markdown.contains("Release patch flow"));
    }

    #[test]
    fn weak_continuity_inspection_prefers_capsule_before_archive_fallback() {
        let session_store = StubSessionStore {
            recent: vec![SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }],
        };
        let memory_store = StubMemoryStore {
            daily_notes: vec![(
                "2026-04-09.md".to_string(),
                "Archive evidence: the raw router diary still exists, but it should only be fallback."
                    .to_string(),
            )],
        };
        let continuity_capsule_store = StubContinuityCapsuleStore::default();
        continuity_capsule_store
            .upsert_many(
                &[ContinuityCapsuleDraft {
                    scope_kind: crate::memory::ContinuityCapsuleScopeKind::Chat,
                    scope_id: "chat-1".to_string(),
                    source_chat_id: "chat-1".to_string(),
                    topic: "memory router".to_string(),
                    summary: "Continue the memory router work without reopening archive notes."
                        .to_string(),
                    next_step: "Route continuity capsule before archive fallback.".to_string(),
                    ..Default::default()
                }],
                100,
            )
            .expect("seed continuity capsule");

        let inspection = inspect_working_recall(WorkingRecallInspectionInput {
            chat_id: "chat-1",
            query: "继续",
            summary_text: Some("continue the memory router work"),
            recent: &session_store.recent,
            system_max_len: 1024,
            profile: MemoryProfile::Standard,
            current_channel: None,
            session_store: &session_store,
            memory_store: &memory_store,
            long_term_memory_store: &StubLongTermMemoryStore,
            active_work_store: None,
            continuity_capsule_store: &continuity_capsule_store,
            turn_ledger_store: &StubTurnLedgerStore,
            skill_storage: None,
            task_run_store: None,
            task_learning_store: None,
        });

        assert_eq!(
            inspection.prompt_recall_intent,
            PromptRecallIntent::Continuity
        );
        assert!(inspection
            .continuity_capsule_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Continuity Capsules"));
        assert!(inspection.continuity_capsule_report.selected_count > 0);
    }
}
