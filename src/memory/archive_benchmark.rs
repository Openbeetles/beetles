//! Deterministic archive retrieval benchmark and regression harness.

use crate::error::Result;

use super::archive_search::ArchiveSearchBackendKind;
use super::{
    search_archive_records_detailed, select_archive_hits_for_prompt_with_report,
    ArchiveRecordSource, ArchiveSearchQuery, MemoryProfile, MemoryStore, SessionStore,
    TurnLedgerStore,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveBenchmarkCase {
    pub name: &'static str,
    pub query: &'static str,
    pub preferred_chat_id: Option<&'static str>,
    pub sources: Vec<ArchiveRecordSource>,
    pub limit: usize,
    pub selector_max_chars: usize,
    pub expected_top_citation_fragment: &'static str,
    pub expected_top_source: ArchiveRecordSource,
    pub min_selector_items: usize,
    pub min_matched_terms: usize,
    pub expected_backend: Option<ArchiveSearchBackendKind>,
    pub expected_source_reason_fragment: Option<&'static str>,
    pub expected_selector_reason_fragment: Option<&'static str>,
    pub require_recency_reason: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveBenchmarkResult {
    pub case_name: &'static str,
    pub total_hits: usize,
    pub candidate_count: usize,
    pub selector_hits: usize,
    pub top_citation: Option<String>,
    pub top_source: Option<ArchiveRecordSource>,
    pub top_score: Option<u32>,
    pub backend_label: Option<String>,
    pub matched_terms: Vec<String>,
    pub ranking_reason_present: bool,
    pub source_reason_present: bool,
    pub recency_reason_present: bool,
    pub selector_reason_present: bool,
    pub query_miss_reason: Option<String>,
    pub selector_note: Option<String>,
    pub passed: bool,
}

pub fn run_archive_benchmark_case(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    profile: MemoryProfile,
    case: &ArchiveBenchmarkCase,
) -> Result<ArchiveBenchmarkResult> {
    let result = search_archive_records_detailed(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query: case.query,
            preferred_chat_id: case.preferred_chat_id,
            chat_id_filter: None,
            sources: &case.sources,
            limit: case.limit,
        },
    )?;
    let selected = select_archive_hits_for_prompt_with_report(
        result.hits.clone(),
        profile,
        case.selector_max_chars,
    );
    let top_hit = result.hits.first();
    let top_trace = top_hit.and_then(|hit| hit.retrieval_trace.as_ref());
    let ranking_reason_present = top_hit
        .and_then(|hit| hit.retrieval_trace.as_ref())
        .and_then(|trace| trace.ranking_reason.as_deref())
        .is_some();
    let source_reason_present = top_trace
        .and_then(|trace| trace.source_reason.as_deref())
        .is_some();
    let recency_reason_present = top_trace
        .and_then(|trace| trace.recency_reason.as_deref())
        .is_some();
    let selector_reason_present = selected.hits.iter().all(|hit| {
        hit.retrieval_trace
            .as_ref()
            .and_then(|trace| trace.selector_reason.as_deref())
            .is_some()
    });
    let passed = top_hit.is_some_and(|hit| {
        let trace = hit.retrieval_trace.as_ref();
        hit.citation.contains(case.expected_top_citation_fragment)
            && hit.source == case.expected_top_source
            && selected.hits.len() >= case.min_selector_items
            && trace
                .map(|trace| trace.matched_terms.len() >= case.min_matched_terms)
                .unwrap_or(case.min_matched_terms == 0)
            && case
                .expected_backend
                .is_none_or(|backend| trace.is_some_and(|trace| trace.backend == backend))
            && case.expected_source_reason_fragment.is_none_or(|fragment| {
                trace
                    .and_then(|trace| trace.source_reason.as_deref())
                    .is_some_and(|reason| reason.contains(fragment))
            })
            && case
                .expected_selector_reason_fragment
                .is_none_or(|fragment| {
                    selected.hits.iter().any(|selected_hit| {
                        selected_hit
                            .retrieval_trace
                            .as_ref()
                            .and_then(|trace| trace.selector_reason.as_deref())
                            .is_some_and(|reason| reason.contains(fragment))
                    })
                })
            && (!case.require_recency_reason || recency_reason_present)
            && ranking_reason_present
            && source_reason_present
            && selector_reason_present
    });
    Ok(ArchiveBenchmarkResult {
        case_name: case.name,
        total_hits: result.hits.len(),
        candidate_count: result.report.candidate_count,
        selector_hits: selected.hits.len(),
        top_citation: top_hit.map(|hit| hit.citation.clone()),
        top_source: top_hit.map(|hit| hit.source),
        top_score: top_hit.map(|hit| hit.score),
        backend_label: top_trace.map(|trace| format!("{:?}", trace.backend)),
        matched_terms: top_trace
            .map(|trace| trace.matched_terms.clone())
            .unwrap_or_default(),
        ranking_reason_present,
        source_reason_present,
        recency_reason_present,
        selector_reason_present,
        query_miss_reason: result.report.miss_reason,
        selector_note: selected.report.selection_note,
        passed,
    })
}

pub fn run_archive_benchmark_suite(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    profile: MemoryProfile,
    cases: &[ArchiveBenchmarkCase],
) -> Result<Vec<ArchiveBenchmarkResult>> {
    cases
        .iter()
        .map(|case| {
            run_archive_benchmark_case(
                session_store,
                memory_store,
                turn_ledger_store,
                profile,
                case,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{SessionMessage, TurnLedger, TurnLedgerStatus};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        chats: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let guard = self.chats.lock().unwrap_or_else(|e| e.into_inner());
            let items = guard.get(chat_id).cloned().unwrap_or_default();
            let start = items.len().saturating_sub(n);
            Ok(items.into_iter().skip(start).collect())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            let mut ids = self
                .chats
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            ids.sort();
            Ok(ids)
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
    struct StubTurnLedgerStore {
        ledgers: Mutex<HashMap<String, TurnLedger>>,
    }

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self
                .ledgers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
            self.ledgers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.ledgers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    fn seed_corpus() -> (StubSessionStore, StubMemoryStore, StubTurnLedgerStore) {
        let session_store = StubSessionStore::default();
        session_store
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "chat-a".to_string(),
                vec![
                    SessionMessage {
                        role: "user".to_string(),
                        content: "今晚先把家庭网络 setup checklist 收一下".to_string(),
                    },
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "先配 Wi-Fi，再核对 archive retrieval trace。".to_string(),
                    },
                ],
            );
        session_store
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "chat-b".to_string(),
                vec![SessionMessage {
                    role: "user".to_string(),
                    content: "今天主要在讨论咖啡豆".to_string(),
                }],
            );

        let memory_store = StubMemoryStore::default();
        memory_store
            .write_daily_note(
                "2026-04-02.md",
                "Durable setup note: home network setup checklist includes router placement and verification pass.",
            )
            .unwrap();
        memory_store
            .write_daily_note("2026-04-01.md", "Coffee archive only, unrelated to setup.")
            .unwrap();

        let turn_store = StubTurnLedgerStore::default();
        turn_store
            .set(
                "chat-a",
                &TurnLedger {
                    status: TurnLedgerStatus::Answered,
                    req_id: "req-1".to_string(),
                    reason: "archive retrieval trace verification".to_string(),
                    user_preview: "setup checklist".to_string(),
                    reply_preview: "verify trace and finalize".to_string(),
                    ..TurnLedger::default()
                },
            )
            .unwrap();

        (session_store, memory_store, turn_store)
    }

    #[test]
    fn benchmark_suite_catches_ranking_and_selector_regressions() {
        let (session_store, memory_store, turn_store) = seed_corpus();
        let cases = vec![
            ArchiveBenchmarkCase {
                name: "same-chat transcript wins on direct setup request",
                query: "家庭网络 setup checklist 收一下",
                preferred_chat_id: Some("chat-a"),
                sources: vec![],
                limit: 6,
                selector_max_chars: 800,
                expected_top_citation_fragment: "transcript:chat-a",
                expected_top_source: ArchiveRecordSource::Transcript,
                min_selector_items: 2,
                min_matched_terms: 2,
                expected_backend: None,
                expected_source_reason_fragment: Some("transcript"),
                expected_selector_reason_fragment: Some("primary quota pass"),
                require_recency_reason: false,
            },
            ArchiveBenchmarkCase {
                name: "daily note stays retrievable for durable setup phrase",
                query: "durable setup note router placement verification",
                preferred_chat_id: Some("chat-a"),
                sources: vec![ArchiveRecordSource::DailyNote],
                limit: 4,
                selector_max_chars: 600,
                expected_top_citation_fragment: "daily_note:2026-04-02.md",
                expected_top_source: ArchiveRecordSource::DailyNote,
                min_selector_items: 1,
                min_matched_terms: 2,
                expected_backend: None,
                expected_source_reason_fragment: Some("requested source preference"),
                expected_selector_reason_fragment: Some("primary quota pass"),
                require_recency_reason: true,
            },
            ArchiveBenchmarkCase {
                name: "turn log remains traceable for verification query",
                query: "retrieval trace verification",
                preferred_chat_id: Some("chat-a"),
                sources: vec![
                    ArchiveRecordSource::TurnLog,
                    ArchiveRecordSource::Transcript,
                ],
                limit: 4,
                selector_max_chars: 700,
                expected_top_citation_fragment: "turn_log:chat-a#req=req-1",
                expected_top_source: ArchiveRecordSource::TurnLog,
                min_selector_items: 1,
                min_matched_terms: 1,
                expected_backend: None,
                expected_source_reason_fragment: Some("requested source preference"),
                expected_selector_reason_fragment: Some("primary quota pass"),
                require_recency_reason: false,
            },
        ];
        let reports = run_archive_benchmark_suite(
            &session_store,
            &memory_store,
            &turn_store,
            MemoryProfile::Standard,
            &cases,
        )
        .unwrap();
        for report in reports {
            assert!(report.passed, "archive benchmark failed: {:?}", report);
            assert!(report.top_score.unwrap_or_default() > 0);
            assert!(!report.matched_terms.is_empty());
        }
    }
}
