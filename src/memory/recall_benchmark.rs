//! Deterministic recall-contract benchmark and ground-truth evaluation.
//! 统一 recall 合同基准：用稳定 ground truth 验证 recall@k / precision@k / MRR / nDCG。

use std::collections::HashSet;

use super::{RecallPlane, RecallSelectionReport};

#[derive(Clone, Debug, PartialEq)]
pub struct RecallBenchmarkCase {
    pub name: &'static str,
    pub plane: RecallPlane,
    pub report: RecallSelectionReport,
    pub relevant_candidate_ids: Vec<String>,
    pub expected_top_candidate_id: Option<String>,
    pub top_k: usize,
    pub min_recall_at_k: f32,
    pub min_precision_at_k: f32,
    pub min_mrr: f32,
    pub min_ndcg: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecallBenchmarkMetrics {
    pub recall_at_k: f32,
    pub precision_at_k: f32,
    pub mrr: f32,
    pub ndcg: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecallBenchmarkResult {
    pub case_name: &'static str,
    pub plane: RecallPlane,
    pub metrics: RecallBenchmarkMetrics,
    pub top_candidate_id: Option<String>,
    pub matched_relevant_ids: Vec<String>,
    pub passed: bool,
}

fn dcg_for_binary_relevance(flags: &[bool]) -> f32 {
    flags
        .iter()
        .enumerate()
        .map(|(index, relevant)| {
            if !relevant {
                return 0.0;
            }
            let rank = index as f32 + 2.0;
            1.0 / rank.log2()
        })
        .sum()
}

pub fn compute_recall_benchmark_metrics(
    report: &RecallSelectionReport,
    relevant_candidate_ids: &[String],
    top_k: usize,
) -> RecallBenchmarkMetrics {
    if relevant_candidate_ids.is_empty() {
        return RecallBenchmarkMetrics::default();
    }
    let relevant = relevant_candidate_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let k = top_k.max(1).min(report.candidates.len().max(1));
    let top_candidates = report.candidates.iter().take(k).collect::<Vec<_>>();
    let relevant_in_top_k = top_candidates
        .iter()
        .filter(|candidate| relevant.contains(candidate.candidate_id.as_str()))
        .count();
    let recall_at_k = relevant_in_top_k as f32 / relevant.len() as f32;
    let precision_at_k = relevant_in_top_k as f32 / k as f32;
    let mrr = report
        .candidates
        .iter()
        .position(|candidate| relevant.contains(candidate.candidate_id.as_str()))
        .map(|index| 1.0 / (index as f32 + 1.0))
        .unwrap_or(0.0);
    let relevance_flags = top_candidates
        .iter()
        .map(|candidate| relevant.contains(candidate.candidate_id.as_str()))
        .collect::<Vec<_>>();
    let dcg = dcg_for_binary_relevance(&relevance_flags);
    let ideal_flags = (0..k)
        .map(|index| index < relevant.len())
        .collect::<Vec<_>>();
    let idcg = dcg_for_binary_relevance(&ideal_flags);
    RecallBenchmarkMetrics {
        recall_at_k,
        precision_at_k,
        mrr,
        ndcg: if idcg > 0.0 { dcg / idcg } else { 0.0 },
    }
}

pub fn run_recall_benchmark_case(case: &RecallBenchmarkCase) -> RecallBenchmarkResult {
    let metrics =
        compute_recall_benchmark_metrics(&case.report, &case.relevant_candidate_ids, case.top_k);
    let top_candidate_id = case
        .report
        .candidates
        .first()
        .map(|candidate| candidate.candidate_id.clone());
    let matched_relevant_ids = case
        .report
        .candidates
        .iter()
        .filter(|candidate| {
            case.relevant_candidate_ids
                .iter()
                .any(|relevant_id| relevant_id == &candidate.candidate_id)
        })
        .map(|candidate| candidate.candidate_id.clone())
        .collect::<Vec<_>>();
    let top_match = case
        .expected_top_candidate_id
        .as_ref()
        .is_none_or(|expected| top_candidate_id.as_ref() == Some(expected));
    let passed = top_match
        && metrics.recall_at_k >= case.min_recall_at_k
        && metrics.precision_at_k >= case.min_precision_at_k
        && metrics.mrr >= case.min_mrr
        && metrics.ndcg >= case.min_ndcg;
    RecallBenchmarkResult {
        case_name: case.name,
        plane: case.plane,
        metrics,
        top_candidate_id,
        matched_relevant_ids,
        passed,
    }
}

pub fn run_recall_benchmark_suite(cases: &[RecallBenchmarkCase]) -> Vec<RecallBenchmarkResult> {
    cases.iter().map(run_recall_benchmark_case).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        inspect_archive_recall, inspect_runtime_skill_recall, inspect_shared_factual_recall,
        inspect_task_recall, ArchiveRecordSource, LongTermMemoryConfidence, LongTermMemoryEntry,
        LongTermMemoryFreshness, LongTermMemoryKind, LongTermMemorySourceScope,
        LongTermMemorySourceType, MemoryProfile, MemoryStore, SessionMessage, SessionStore,
        TurnLedger, TurnLedgerStatus, TurnLedgerStore,
    };
    use crate::platform::SkillStorage;
    use crate::skills::{
        record_runtime_skill_outcomes, upsert_runtime_skill, RuntimeSkillReuseOutcome,
        RuntimeSkillWrite,
    };
    use crate::task_execution::{
        TaskLearningKind, TaskLearningRecord, TaskLearningRoute, TaskLearningStore, TaskPlan,
        TaskRun, TaskRunKind, TaskRunRecord, TaskRunStatus, TaskRunStore, TaskStep, TaskStepStatus,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        chats: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
            self.chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let items = self
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned()
                .unwrap_or_default();
            let start = items.len().saturating_sub(n);
            Ok(items.into_iter().skip(start).collect())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(self
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
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
        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            let mut names = self
                .notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            names.sort_by(|a, b| b.cmp(a));
            names.truncate(recent_n);
            Ok(names)
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

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(Some(TurnLedger {
                status: TurnLedgerStatus::Answered,
                ..TurnLedger::default()
            }))
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
    }

    impl crate::memory::LongTermMemoryStore for StubLongTermMemoryStore {
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
            limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.lock().unwrap_or_else(|e| e.into_inner()).len())
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
    struct StubTaskRunStore {
        runs: Mutex<Vec<TaskRunRecord>>,
    }

    impl TaskRunStore for StubTaskRunStore {
        fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
            Ok(self
                .runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|record| record.run.run_id == run_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskRunRecord) -> Result<()> {
            self.runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
            Ok(self
                .runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_active_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<TaskRunRecord>> {
            Ok(self
                .runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|record| {
                    record.run.source_channel == channel
                        && record.run.source_chat_id == chat_id
                        && record.run.status.is_active()
                })
                .take(limit)
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct StubTaskLearningStore {
        records: Mutex<Vec<TaskLearningRecord>>,
    }

    impl TaskLearningStore for StubTaskLearningStore {
        fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|record| record.learning_id == learning_id)
                .cloned())
        }

        fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
            self.records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(record.clone());
            Ok(())
        }

        fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_for_chat(
            &self,
            channel: &str,
            chat_id: &str,
            limit: usize,
        ) -> Result<Vec<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|record| {
                    record.source_channel == channel && record.source_chat_id == chat_id
                })
                .take(limit)
                .cloned()
                .collect())
        }

        fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
            Ok(self
                .records
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|record| record.run_id == run_id)
                .take(limit)
                .cloned()
                .collect())
        }
    }

    fn sample_task_run() -> TaskRunRecord {
        TaskRunRecord {
            run: TaskRun {
                run_id: "run_net".to_string(),
                kind: TaskRunKind::TaskExecution,
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-a".to_string(),
                user_request: "fix network setup".to_string(),
                title: "Fix network setup".to_string(),
                status: TaskRunStatus::Running,
                current_step_id: "step_1".to_string(),
                planner_reason: String::new(),
                final_summary: String::new(),
                failure_reason: String::new(),
                plan_revision: 1,
                created_at: 100,
                updated_at: 100,
                finished_at: 0,
            },
            plan: TaskPlan {
                goal: "restore network setup".to_string(),
                completion_definition: "network passes smoke test".to_string(),
                risk_notes: Vec::new(),
                ordered_steps: vec![TaskStep {
                    step_id: "step_1".to_string(),
                    title: "inspect network config".to_string(),
                    instruction: "check network".to_string(),
                    status: TaskStepStatus::Running,
                    tool_budget: 3,
                    retry_budget: 1,
                    expected_artifacts: Vec::new(),
                    review_criteria: Vec::new(),
                    attempt_count: 0,
                    last_result_summary: String::new(),
                    last_review_summary: String::new(),
                    started_at: 100,
                    finished_at: 0,
                }],
            },
        }
    }

    #[test]
    fn recall_benchmark_suite_scores_all_working_recall_planes() {
        let session_store = StubSessionStore::default();
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore;
        let long_term_store = StubLongTermMemoryStore::default();
        let skill_storage = StubSkillStorage::default();
        let task_run_store = StubTaskRunStore::default();
        let task_learning_store = StubTaskLearningStore::default();

        session_store
            .append("chat-a", "user", "please fix my network setup")
            .unwrap();
        memory_store
            .write_daily_note(
                "2026-04-06.md",
                "Network setup issue traced to stale DHCP lease on chat-a.",
            )
            .unwrap();

        long_term_store
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(LongTermMemoryEntry {
                id: "fact:network_setup".to_string(),
                kind: LongTermMemoryKind::Fact,
                topic: "network setup".to_string(),
                content: "The user's network setup issue is usually a stale DHCP lease."
                    .to_string(),
                keywords: vec!["network".to_string(), "dhcp".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                source_type: LongTermMemorySourceType::Conversation,
                source_scope: LongTermMemorySourceScope::User,
                confidence: LongTermMemoryConfidence::High,
                freshness: LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::ReviewBeforeUse,
                supporting_citations: vec!["daily_note:2026-04-06.md".to_string()],
                evidence_count: 2,
                created_at: 100,
                updated_at: 120,
                observed_at: 120,
                last_confirmed_at: 120,
                source_revision: 1,
                last_used_at: 0,
            });

        upsert_runtime_skill(
            &skill_storage,
            &RuntimeSkillWrite {
                name: "runtime_skill__network_setup".to_string(),
                topic: "network setup".to_string(),
                title: "Network setup checklist".to_string(),
                summary: "Use the DHCP lease reset checklist before deeper debugging.".to_string(),
                content: "1. Check lease 2. Renew DHCP".to_string(),
                citations: vec!["archive:network".to_string()],
                source_chat_id: Some("chat-a".to_string()),
                observed_at: 120,
            },
        )
        .unwrap();
        record_runtime_skill_outcomes(
            &skill_storage,
            &[String::from("runtime_skill__network_setup")],
            RuntimeSkillReuseOutcome::Succeeded,
            180,
            "final_answer",
        )
        .unwrap();

        let run = sample_task_run();
        task_run_store.upsert(&run).unwrap();
        task_learning_store
            .upsert(&TaskLearningRecord {
                learning_id: "learning_network_setup".to_string(),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-a".to_string(),
                run_id: "run_net".to_string(),
                step_id: "step_1".to_string(),
                kind: TaskLearningKind::ReusableProcedure,
                route: TaskLearningRoute::RuntimeSkill,
                run_status: TaskRunStatus::Running,
                topic: "network setup".to_string(),
                summary: "Reuse the DHCP lease reset checklist for network setup failures."
                    .to_string(),
                content: "Reset lease, verify interface, retest.".to_string(),
                memory_kind: None,
                review_summary: String::new(),
                source_artifact_ids: Vec::new(),
                provenance: String::new(),
                archive_note_name: "2026-04-06-task-network".to_string(),
                route_detail: String::new(),
                candidate_state: Some(crate::task_execution::TaskLearningCandidateState::Promoted),
                candidate_state_updated_at: 120,
                last_failure_reason: String::new(),
                observed_at: 120,
            })
            .unwrap();

        let shared_report = inspect_shared_factual_recall(
            &long_term_store,
            "chat-a",
            "network setup",
            None,
            &session_store.load_recent("chat-a", 2).unwrap(),
            800,
            MemoryProfile::Standard,
            200,
        );
        let archive_report = inspect_archive_recall(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            "chat-a",
            "network setup",
            None,
            &session_store.load_recent("chat-a", 2).unwrap(),
            700,
            MemoryProfile::Standard,
        );
        let runtime_report = inspect_runtime_skill_recall(
            &skill_storage,
            "network setup",
            Some("chat-a"),
            None,
            &session_store.load_recent("chat-a", 2).unwrap(),
            200,
            420,
        );
        assert_eq!(
            runtime_report.selection_note.as_deref(),
            Some("stable_validated_runtime_skill")
        );
        let task_report = inspect_task_recall(
            Some(&run),
            &task_learning_store,
            "telegram",
            "chat-a",
            "network setup",
            None,
            &session_store.load_recent("chat-a", 2).unwrap(),
            420,
        );

        let cases = vec![
            RecallBenchmarkCase {
                name: "shared factual network setup",
                plane: RecallPlane::SharedFactual,
                report: shared_report,
                relevant_candidate_ids: vec!["fact:network_setup".to_string()],
                expected_top_candidate_id: Some("fact:network_setup".to_string()),
                top_k: 1,
                min_recall_at_k: 1.0,
                min_precision_at_k: 1.0,
                min_mrr: 1.0,
                min_ndcg: 1.0,
            },
            RecallBenchmarkCase {
                name: "archive network setup",
                plane: RecallPlane::Archive,
                report: archive_report,
                relevant_candidate_ids: vec![
                    format!(
                        "transcript|chat-a|msg|{}",
                        crate::memory::synthesize_session_message_id(
                            "chat-a",
                            "user",
                            "please fix my network setup",
                            1,
                        )
                    ),
                    format!("daily_note|2026-04-06.md"),
                ],
                expected_top_candidate_id: None,
                top_k: 2,
                min_recall_at_k: 0.5,
                min_precision_at_k: 0.5,
                min_mrr: 0.5,
                min_ndcg: 0.5,
            },
            RecallBenchmarkCase {
                name: "runtime skill network setup",
                plane: RecallPlane::RuntimeSkill,
                report: runtime_report,
                relevant_candidate_ids: vec!["runtime_skill__network_setup".to_string()],
                expected_top_candidate_id: Some("runtime_skill__network_setup".to_string()),
                top_k: 1,
                min_recall_at_k: 1.0,
                min_precision_at_k: 1.0,
                min_mrr: 1.0,
                min_ndcg: 1.0,
            },
            RecallBenchmarkCase {
                name: "task recall network setup",
                plane: RecallPlane::TaskRecall,
                report: task_report,
                relevant_candidate_ids: vec!["learning_network_setup".to_string()],
                expected_top_candidate_id: Some("learning_network_setup".to_string()),
                top_k: 1,
                min_recall_at_k: 1.0,
                min_precision_at_k: 1.0,
                min_mrr: 1.0,
                min_ndcg: 1.0,
            },
        ];

        let results = run_recall_benchmark_suite(&cases);
        for result in results {
            assert!(result.passed, "recall benchmark failed: {:?}", result);
        }
    }

    #[test]
    fn benchmark_metrics_zero_when_nothing_matches() {
        let report = RecallSelectionReport {
            plane: RecallPlane::Archive,
            query: crate::memory::RecallQuery {
                plane: RecallPlane::Archive,
                ..crate::memory::RecallQuery::default()
            },
            backend: "archive_search".to_string(),
            candidate_count: 1,
            selected_count: 1,
            selected_ids: vec!["a".to_string()],
            miss_reason: None,
            selection_note: None,
            candidates: vec![crate::memory::RecallCandidate {
                plane: RecallPlane::Archive,
                candidate_id: "a".to_string(),
                title: "A".to_string(),
                excerpt: String::new(),
                citation: None,
                source: ArchiveRecordSource::Transcript.label().to_string(),
                observed_at: None,
                selected: true,
                score: crate::memory::RecallScoreBreakdown::default(),
            }],
        };
        let metrics = compute_recall_benchmark_metrics(&report, &[String::from("z")], 1);
        assert_eq!(metrics.recall_at_k, 0.0);
        assert_eq!(metrics.precision_at_k, 0.0);
        assert_eq!(metrics.mrr, 0.0);
        assert_eq!(metrics.ndcg, 0.0);
    }
}
