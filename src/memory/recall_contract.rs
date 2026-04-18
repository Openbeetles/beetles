//! Unified recall contract and inspection reports for working memory planes.
//! 统一 recall 合同：为 shared factual / archive / runtime skill / task recall 提供同构查询与报告。
#![allow(clippy::too_many_arguments)]

use crate::platform::SkillStorage;
use crate::skills::retrieve_runtime_skill_hits_with_backend;
use crate::task_execution::{TaskLearningStore, TaskRunRecord};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{
    parse_explicit_long_term_slot_query, score_long_term_memory_recall_breakdown,
    search_archive_records_detailed, select_archive_hits_for_prompt_with_report,
    select_long_term_recall_entries, ArchiveSearchQuery, LongTermMemoryEntry, LongTermMemorySlot,
    LongTermMemoryStore, MemoryProfile, MemoryStore, SessionMessage, SessionStore, TurnLedgerStore,
};

const RECALL_REPORT_RECENT_GROUNDING_LIMIT: usize = 2;
const RECALL_REPORT_REASON_LIMIT: usize = 6;
const RECALL_REPORT_CANDIDATE_LIMIT: usize = 12;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RecallPlane {
    #[default]
    SharedFactual,
    ContinuityCapsule,
    Archive,
    RuntimeSkill,
    TaskRecall,
}

impl RecallPlane {
    pub fn label(self) -> &'static str {
        match self {
            Self::SharedFactual => "shared_factual",
            Self::ContinuityCapsule => "continuity_capsule",
            Self::Archive => "archive",
            Self::RuntimeSkill => "runtime_skill",
            Self::TaskRecall => "task_recall",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecallQuery {
    pub plane: RecallPlane,
    #[serde(default)]
    pub raw_query: String,
    #[serde(default)]
    pub normalized_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_grounding: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_lookup: Option<String>,
    #[serde(default)]
    pub requested_limit: usize,
    #[serde(default)]
    pub max_chars: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecallScoreBreakdown {
    #[serde(default)]
    pub lexical_score: u32,
    #[serde(default)]
    pub semantic_score: u32,
    #[serde(default)]
    pub exact_match_score: u32,
    #[serde(default)]
    pub recency_score: u32,
    #[serde(default)]
    pub confidence_score: u32,
    #[serde(default)]
    pub importance_score: u32,
    #[serde(default)]
    pub scope_affinity_score: u32,
    #[serde(default)]
    pub governance_score: u32,
    #[serde(default)]
    pub source_score: u32,
    #[serde(default)]
    pub total_score: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_fragments: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecallCandidate {
    pub plane: RecallPlane,
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub score: RecallScoreBreakdown,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecallSelectionReport {
    pub plane: RecallPlane,
    pub query: RecallQuery,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub candidate_count: usize,
    #[serde(default)]
    pub selected_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<RecallCandidate>,
}

fn normalize_recall_text(value: &str, max_len: usize) -> String {
    truncate_content_to_max(value.trim(), max_len).into_owned()
}

fn normalize_reason_fragments(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .filter_map(|value| {
            let normalized = normalize_recall_text(&value, 96);
            (!normalized.is_empty()).then_some(normalized)
        })
        .take(RECALL_REPORT_REASON_LIMIT)
        .collect()
}

fn recent_grounding(messages: &[SessionMessage]) -> Vec<String> {
    messages
        .iter()
        .rev()
        .take(RECALL_REPORT_RECENT_GROUNDING_LIMIT)
        .filter_map(|message| {
            let content = normalize_recall_text(&message.content, 120);
            (!content.is_empty()).then_some(format!("{}: {}", message.role, content))
        })
        .collect()
}

fn base_recall_query(
    plane: RecallPlane,
    raw_query: &str,
    preferred_chat_id: Option<&str>,
    current_channel: Option<&str>,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    requested_limit: usize,
    max_chars: usize,
) -> RecallQuery {
    RecallQuery {
        plane,
        raw_query: raw_query.trim().to_string(),
        normalized_query: normalize_recall_text(raw_query, 240),
        preferred_chat_id: preferred_chat_id.map(str::to_string),
        current_channel: current_channel.map(str::to_string),
        summary_text: summary_text
            .map(|value| normalize_recall_text(value, 220))
            .filter(|value| !value.is_empty()),
        recent_grounding: recent_grounding(recent),
        active_run_id: None,
        exact_lookup: None,
        requested_limit,
        max_chars,
        notes: Vec::new(),
    }
}

fn runtime_skill_selection_note(hits: &[crate::skills::RuntimeSkillHit]) -> Option<String> {
    let top = hits.first()?;
    Some(if top.record.revision_pending {
        "fallback_revision_pending_runtime_skill".to_string()
    } else if top.record.validated_success_count > 0 {
        "stable_validated_runtime_skill".to_string()
    } else {
        "fallback_promoted_runtime_skill".to_string()
    })
}

fn build_shared_factual_candidate(
    entry: &LongTermMemoryEntry,
    selected: bool,
    chat_id: &str,
    query: &str,
    exact_lookup: Option<&LongTermMemorySlot>,
    now_secs: u64,
) -> RecallCandidate {
    let mut breakdown =
        score_long_term_memory_recall_breakdown(query, Some(chat_id), now_secs, entry);
    let exact_lookup_bonus = exact_lookup
        .and_then(LongTermMemorySlot::stable_id)
        .filter(|slot_id| slot_id == &entry.id)
        .map(|_| 24)
        .unwrap_or(0);
    if exact_lookup_bonus > 0 {
        breakdown.exact_match_score = breakdown
            .exact_match_score
            .saturating_add(exact_lookup_bonus);
        breakdown.total_score = breakdown.total_score.saturating_add(exact_lookup_bonus);
        breakdown
            .reason_fragments
            .push("explicit slot lookup".to_string());
    }
    let reasons = normalize_reason_fragments(vec![
        breakdown.reason_fragments.join(", "),
        format!("kind={}", entry.kind.label()),
        format!("confidence={}", entry.confidence.label()),
        format!("freshness={}", entry.freshness.label()),
        format!("evidence_count={}", entry.evidence_count),
        entry
            .source_chat_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(|value| format!("source_chat={value}"))
            .unwrap_or_default(),
    ]);
    RecallCandidate {
        plane: RecallPlane::SharedFactual,
        candidate_id: entry.id.clone(),
        title: entry.topic.clone(),
        excerpt: normalize_recall_text(&entry.content, 160),
        citation: entry.supporting_citations.first().cloned(),
        source: format!("canonical_{}", entry.kind.label()),
        observed_at: Some(entry.observed_at),
        selected,
        score: RecallScoreBreakdown {
            lexical_score: breakdown
                .lexical_score
                .saturating_add(breakdown.keyword_score),
            semantic_score: breakdown.semantic_score,
            exact_match_score: breakdown.exact_match_score,
            recency_score: breakdown
                .recency_score
                .saturating_add(breakdown.last_used_score),
            confidence_score: breakdown.confidence_score,
            importance_score: entry.evidence_count.min(4),
            scope_affinity_score: breakdown.scope_affinity_score,
            governance_score: breakdown.governance_score,
            source_score: 0,
            total_score: breakdown.total_score,
            reason_fragments: reasons,
        },
    }
}

pub fn inspect_shared_factual_recall(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    max_chars: usize,
    profile: MemoryProfile,
    now_secs: u64,
) -> RecallSelectionReport {
    let exact_lookup = parse_explicit_long_term_slot_query(user_query);
    let mut report = RecallSelectionReport {
        plane: RecallPlane::SharedFactual,
        query: base_recall_query(
            RecallPlane::SharedFactual,
            user_query,
            Some(chat_id),
            None,
            summary_text,
            recent_messages,
            0,
            max_chars,
        ),
        backend: "hybrid_canonical".to_string(),
        candidate_count: 0,
        selected_count: 0,
        selected_ids: Vec::new(),
        miss_reason: None,
        selection_note: None,
        candidates: Vec::new(),
    };
    if let Some(slot) = exact_lookup.as_ref() {
        report.backend = "exact_slot".to_string();
        report.query.exact_lookup = slot.stable_id();
        report.query.notes.push("explicit_slot_lookup".to_string());
        match store.get_slot(slot).ok().flatten() {
            Some(entry) => {
                report.candidate_count = 1;
                report.selected_count = 1;
                report.selected_ids.push(entry.id.clone());
                report.candidates.push(build_shared_factual_candidate(
                    &entry,
                    true,
                    chat_id,
                    user_query,
                    exact_lookup.as_ref(),
                    now_secs,
                ));
            }
            None => {
                report.miss_reason = Some("exact_slot_not_found".to_string());
            }
        }
        return report;
    }

    let selection = select_long_term_recall_entries(
        store,
        chat_id,
        user_query,
        summary_text,
        recent_messages,
        profile,
    );
    let selected_ids = selection
        .selected
        .iter()
        .map(|entry| entry.id.clone())
        .collect::<Vec<_>>();
    report.query.normalized_query = selection.recall_query.clone();
    report.query.requested_limit = selection.desired;
    if selection.used_fallback {
        report.query.notes.push("fallback_list_used".to_string());
    }
    report.candidate_count = selection.candidates.len();
    report.selected_count = selection.selected.len();
    report.selected_ids = selected_ids.clone();
    report.selection_note = selection
        .used_fallback
        .then(|| "fallback_list_contributed_candidates".to_string());
    report.miss_reason = selection
        .candidates
        .is_empty()
        .then(|| "no_canonical_fact_candidates".to_string());
    report.candidates = selection
        .candidates
        .iter()
        .take(RECALL_REPORT_CANDIDATE_LIMIT)
        .map(|entry| {
            build_shared_factual_candidate(
                entry,
                selected_ids
                    .iter()
                    .any(|selected_id| selected_id == &entry.id),
                chat_id,
                &selection.recall_query,
                None,
                now_secs,
            )
        })
        .collect();
    report
}

pub fn inspect_archive_recall(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    max_chars: usize,
    profile: MemoryProfile,
) -> RecallSelectionReport {
    let mut report = RecallSelectionReport {
        plane: RecallPlane::Archive,
        query: base_recall_query(
            RecallPlane::Archive,
            user_query,
            Some(chat_id),
            None,
            summary_text,
            recent_messages,
            super::MAX_ARCHIVE_SEARCH_LIMIT,
            max_chars,
        ),
        backend: "archive_search".to_string(),
        candidate_count: 0,
        selected_count: 0,
        selected_ids: Vec::new(),
        miss_reason: None,
        selection_note: None,
        candidates: Vec::new(),
    };
    let Ok(search_result) = search_archive_records_detailed(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query: user_query,
            preferred_chat_id: Some(chat_id),
            chat_id_filter: None,
            sources: &[],
            limit: super::MAX_ARCHIVE_SEARCH_LIMIT,
        },
    ) else {
        report.miss_reason = Some("archive_search_failed".to_string());
        return report;
    };
    let selection =
        select_archive_hits_for_prompt_with_report(search_result.hits.clone(), profile, max_chars);
    let selected_ids = selection
        .hits
        .iter()
        .map(|hit| hit.record_id.clone())
        .collect::<Vec<_>>();
    report.backend = format!("{:?}", search_result.report.backend);
    report.candidate_count = search_result.hits.len();
    report.selected_count = selection.hits.len();
    report.selected_ids = selected_ids.clone();
    report.miss_reason = search_result.report.miss_reason.clone();
    report.selection_note = selection.report.selection_note.clone();
    report.candidates = search_result
        .hits
        .iter()
        .take(RECALL_REPORT_CANDIDATE_LIMIT)
        .map(|hit| {
            let trace = hit.retrieval_trace.as_ref();
            let reasons = normalize_reason_fragments(vec![
                trace
                    .and_then(|value| value.ranking_reason.clone())
                    .unwrap_or_default(),
                trace
                    .and_then(|value| value.source_reason.clone())
                    .unwrap_or_default(),
                trace
                    .and_then(|value| value.recency_reason.clone())
                    .unwrap_or_default(),
                trace
                    .and_then(|value| value.selector_reason.clone())
                    .unwrap_or_default(),
            ]);
            RecallCandidate {
                plane: RecallPlane::Archive,
                candidate_id: hit.record_id.clone(),
                title: hit.title.clone(),
                excerpt: normalize_recall_text(&hit.excerpt, 180),
                citation: Some(hit.citation.clone()),
                source: hit.source.label().to_string(),
                observed_at: hit.observed_at,
                selected: selected_ids
                    .iter()
                    .any(|selected_id| selected_id == &hit.record_id),
                score: RecallScoreBreakdown {
                    lexical_score: trace
                        .map(|value| value.score.lexical_score)
                        .unwrap_or(hit.score),
                    semantic_score: trace.map(|value| value.score.hybrid_score).unwrap_or(0),
                    exact_match_score: 0,
                    recency_score: trace.map(|value| value.score.recency_bonus).unwrap_or(0),
                    confidence_score: 0,
                    importance_score: 0,
                    scope_affinity_score: trace
                        .map(|value| value.score.same_chat_bonus)
                        .unwrap_or(0),
                    governance_score: 0,
                    source_score: trace.map(|value| value.score.source_bonus).unwrap_or(0),
                    total_score: trace
                        .map(|value| value.score.total_score)
                        .unwrap_or(hit.score),
                    reason_fragments: reasons,
                },
            }
        })
        .collect();
    report
}

pub fn inspect_runtime_skill_recall(
    storage: &dyn SkillStorage,
    query: &str,
    preferred_chat_id: Option<&str>,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    now_secs: u64,
    max_chars: usize,
) -> RecallSelectionReport {
    let recall =
        retrieve_runtime_skill_hits_with_backend(storage, query, preferred_chat_id, now_secs, 4);
    let hits = recall.hits;
    let selected_ids = hits
        .iter()
        .map(|hit| hit.record.name.clone())
        .collect::<Vec<_>>();
    let miss_reason = hits
        .is_empty()
        .then(|| "no_runtime_skill_candidates".to_string());
    RecallSelectionReport {
        plane: RecallPlane::RuntimeSkill,
        query: base_recall_query(
            RecallPlane::RuntimeSkill,
            query,
            preferred_chat_id,
            None,
            summary_text,
            recent_messages,
            4,
            max_chars,
        ),
        backend: recall.backend.label().to_string(),
        candidate_count: hits.len(),
        selected_count: hits.len(),
        selected_ids,
        miss_reason,
        selection_note: runtime_skill_selection_note(&hits),
        candidates: hits
            .into_iter()
            .map(|hit| RecallCandidate {
                plane: RecallPlane::RuntimeSkill,
                candidate_id: hit.record.name.clone(),
                title: hit.record.title.clone(),
                excerpt: normalize_recall_text(&hit.record.summary, 180),
                citation: hit.record.citations.first().cloned(),
                source: "runtime_skill".to_string(),
                observed_at: Some(hit.record.observed_at),
                selected: true,
                score: RecallScoreBreakdown {
                    lexical_score: hit.score_breakdown.lexical_score,
                    semantic_score: hit.score_breakdown.semantic_score,
                    exact_match_score: hit.score_breakdown.exact_match_score,
                    recency_score: hit.score_breakdown.recency_score,
                    confidence_score: hit.score_breakdown.confidence_score,
                    importance_score: hit.score_breakdown.importance_score,
                    scope_affinity_score: hit.score_breakdown.scope_affinity_score,
                    governance_score: hit.score_breakdown.governance_score,
                    source_score: hit.score_breakdown.source_score,
                    total_score: hit.score_breakdown.total_score,
                    reason_fragments: normalize_reason_fragments(hit.reasons),
                },
            })
            .collect(),
    }
}

pub fn inspect_task_recall(
    active_run: Option<&TaskRunRecord>,
    store: &dyn TaskLearningStore,
    channel: &str,
    chat_id: &str,
    query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    max_chars: usize,
) -> RecallSelectionReport {
    let mut report = RecallSelectionReport {
        plane: RecallPlane::TaskRecall,
        query: base_recall_query(
            RecallPlane::TaskRecall,
            query,
            Some(chat_id),
            Some(channel),
            summary_text,
            recent_messages,
            3,
            max_chars,
        ),
        backend: "task_learning_heuristic".to_string(),
        candidate_count: 0,
        selected_count: 0,
        selected_ids: Vec::new(),
        miss_reason: None,
        selection_note: None,
        candidates: Vec::new(),
    };
    let Some(active_run) = active_run else {
        report.miss_reason = Some("no_active_task_run".to_string());
        return report;
    };
    report.query.active_run_id = Some(active_run.run.run_id.clone());
    let step_title = crate::task_execution::current_or_next_step(active_run)
        .map(|step| step.title.as_str())
        .unwrap_or("");
    let composed_query = [query.trim(), active_run.plan.goal.trim(), step_title.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    report.query.normalized_query = normalize_recall_text(&composed_query, 240);
    let (hits, backend) = crate::task_execution::retrieve_task_learning_hits_with_backend(
        store,
        channel,
        chat_id,
        Some(&active_run.run.run_id),
        &composed_query,
        3,
    );
    report.backend = backend.label().to_string();
    report.candidate_count = hits.len();
    report.selected_count = hits.len();
    report.selected_ids = hits
        .iter()
        .map(|hit| hit.record.learning_id.clone())
        .collect::<Vec<_>>();
    report.miss_reason = hits
        .is_empty()
        .then(|| "no_task_learning_candidates".to_string());
    report.selection_note = if hits.is_empty() {
        None
    } else {
        Some("active_task_recall_bundle".to_string())
    };
    report.candidates = hits
        .into_iter()
        .map(|hit| RecallCandidate {
            plane: RecallPlane::TaskRecall,
            candidate_id: hit.record.learning_id.clone(),
            title: hit.record.topic.clone(),
            excerpt: normalize_recall_text(&hit.record.summary, 180),
            citation: (!hit.record.archive_note_name.trim().is_empty())
                .then(|| hit.record.archive_note_name.clone()),
            source: hit.record.route.label().to_string(),
            observed_at: Some(hit.record.observed_at),
            selected: true,
            score: RecallScoreBreakdown {
                lexical_score: hit.score_breakdown.lexical_score,
                semantic_score: hit.score_breakdown.semantic_score,
                exact_match_score: hit.score_breakdown.exact_match_score,
                recency_score: hit.score_breakdown.recency_score,
                confidence_score: 0,
                importance_score: 0,
                scope_affinity_score: hit.score_breakdown.scope_affinity_score,
                governance_score: hit.score_breakdown.governance_score,
                source_score: hit.score_breakdown.source_score,
                total_score: hit.score,
                reason_fragments: normalize_reason_fragments(hit.reasons),
            },
        })
        .collect();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};
    use crate::skills::{
        record_runtime_skill_outcomes, upsert_runtime_skill, RuntimeSkillReuseOutcome,
        RuntimeSkillWrite,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

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
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill", "missing"))
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

    #[test]
    fn inspect_runtime_skill_recall_surfaces_stable_and_fallback_selection_notes() {
        let stable_storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &stable_storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Validated release patch flow.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();
        record_runtime_skill_outcomes(
            &stable_storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Succeeded,
            200,
            "final_answer",
        )
        .unwrap();

        let stable_report = inspect_runtime_skill_recall(
            &stable_storage,
            "继续按 release patch flow 做",
            Some("chat-1"),
            None,
            &[],
            300,
            420,
        );
        assert_eq!(
            stable_report.selection_note.as_deref(),
            Some("stable_validated_runtime_skill")
        );

        let fallback_storage = StubSkillStorage::default();
        upsert_runtime_skill(
            &fallback_storage,
            &RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Fresh promoted release patch flow.".to_string(),
                content: "1. inspect diff\n2. patch\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            },
        )
        .unwrap();

        let fallback_report = inspect_runtime_skill_recall(
            &fallback_storage,
            "继续按 release patch flow 做",
            Some("chat-1"),
            None,
            &[],
            300,
            420,
        );
        assert_eq!(
            fallback_report.selection_note.as_deref(),
            Some("fallback_promoted_runtime_skill")
        );

        record_runtime_skill_outcomes(
            &fallback_storage,
            &[String::from("runtime_skill__release_patch_flow")],
            RuntimeSkillReuseOutcome::Mismatch,
            320,
            "surface_finalization",
        )
        .unwrap();
        let revision_pending_report = inspect_runtime_skill_recall(
            &fallback_storage,
            "继续按 release patch flow 做",
            Some("chat-1"),
            None,
            &[],
            360,
            420,
        );
        assert_eq!(
            revision_pending_report.selection_note.as_deref(),
            Some("fallback_revision_pending_runtime_skill")
        );
    }
}
