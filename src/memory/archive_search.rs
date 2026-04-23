//! Searchable archive sidecar over retained transcripts, daily notes, and turn logs.

use crate::error::Result;
use crate::util::{
    collect_retrieval_terms, normalize_retrieval_text, trigram_overlap_score,
    truncate_content_to_max,
};
#[cfg(target_os = "linux")]
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

use super::{
    render_turn_observation_ledger_block, render_turn_persona_ledger_block, MemoryStore,
    SessionStore, TurnLedger, TurnLedgerStore, MAX_SESSION_ENTRIES,
};

pub const MAX_ARCHIVE_SEARCH_LIMIT: usize = 8;
pub const MAX_ARCHIVE_GET_CONTENT_LEN: usize = 4 * 1024;

const DEFAULT_ARCHIVE_GET_CONTENT_LEN: usize = 1800;
const ARCHIVE_SEARCH_EXCERPT_LEN: usize = 220;
const ARCHIVE_GET_EXCERPT_LEN: usize = 320;
const ARCHIVE_TRACE_MAX_MATCHED_TERMS: usize = 4;
#[cfg(target_os = "linux")]
const ARCHIVE_INDEX_VERSION: u32 = 2;
#[cfg(target_os = "linux")]
const REL_PATH_ARCHIVE_INDEX: &str = "memory/archive_index.sqlite3";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveSearchBackendKind {
    #[default]
    Lexical,
    IndexedHybrid,
    SqliteFtsHybrid,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveSearchScoreBreakdown {
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub lexical_score: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub fts_score: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub hybrid_score: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub same_chat_bonus: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub source_bonus: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub recency_bonus: u32,
    pub total_score: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveRetrievalTrace {
    #[serde(default)]
    pub backend: ArchiveSearchBackendKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranking_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recency_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector_reason: Option<String>,
    #[serde(default)]
    pub score: ArchiveSearchScoreBreakdown,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveRecordSource {
    Transcript,
    DailyNote,
    TurnLog,
}

impl ArchiveRecordSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Transcript => "transcript",
            Self::DailyNote => "daily_note",
            Self::TurnLog => "turn_log",
        }
    }
}

impl std::str::FromStr for ArchiveRecordSource {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "transcript" => Ok(Self::Transcript),
            "daily_note" | "daily-note" | "daily note" => Ok(Self::DailyNote),
            "turn_log" | "turn-log" | "turn log" => Ok(Self::TurnLog),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveRecordLocator {
    pub source: ArchiveRecordSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub req_id: Option<String>,
}

impl ArchiveRecordLocator {
    pub fn record_id(&self) -> String {
        match self.source {
            ArchiveRecordSource::Transcript => {
                if let Some(message_id) = self.message_id.as_deref() {
                    format!(
                        "transcript|{}|msg|{}",
                        self.chat_id.as_deref().unwrap_or_default(),
                        message_id
                    )
                } else {
                    format!(
                        "transcript|{}|{}",
                        self.chat_id.as_deref().unwrap_or_default(),
                        self.message_index.unwrap_or_default()
                    )
                }
            }
            ArchiveRecordSource::DailyNote => {
                format!(
                    "daily_note|{}",
                    self.note_name.as_deref().unwrap_or_default()
                )
            }
            ArchiveRecordSource::TurnLog => format!(
                "turn_log|{}|{}",
                self.chat_id.as_deref().unwrap_or_default(),
                self.req_id.as_deref().unwrap_or("latest")
            ),
        }
    }

    pub fn citation(&self) -> String {
        match self.source {
            ArchiveRecordSource::Transcript => {
                if let Some(message_id) = self.message_id.as_deref() {
                    format!(
                        "transcript:{}#message_id={}",
                        self.chat_id.as_deref().unwrap_or("unknown"),
                        message_id
                    )
                } else {
                    format!(
                        "transcript:{}#message={}",
                        self.chat_id.as_deref().unwrap_or("unknown"),
                        self.message_index.unwrap_or_default()
                    )
                }
            }
            ArchiveRecordSource::DailyNote => format!(
                "daily_note:{}",
                self.note_name.as_deref().unwrap_or("unknown")
            ),
            ArchiveRecordSource::TurnLog => format!(
                "turn_log:{}#req={}",
                self.chat_id.as_deref().unwrap_or("unknown"),
                self.req_id.as_deref().unwrap_or("latest")
            ),
        }
    }

    pub fn parse_record_id(value: &str) -> Option<Self> {
        let mut parts = value.split('|');
        let head = parts.next()?;
        match head {
            "transcript" => {
                let chat_id = parts.next()?.trim();
                let identity = parts.next()?.trim();
                let (message_id, message_index) = if identity == "msg" {
                    let message_id = parts.next()?.trim();
                    (Some(message_id.to_string()), None)
                } else {
                    (None, identity.parse::<usize>().ok())
                };
                Some(Self {
                    source: ArchiveRecordSource::Transcript,
                    chat_id: Some(chat_id.to_string()),
                    message_id,
                    message_index,
                    note_name: None,
                    req_id: None,
                })
            }
            "daily_note" => {
                let note_name = parts.next()?.trim();
                Some(Self {
                    source: ArchiveRecordSource::DailyNote,
                    chat_id: None,
                    message_id: None,
                    message_index: None,
                    note_name: Some(note_name.to_string()),
                    req_id: None,
                })
            }
            "turn_log" => {
                let chat_id = parts.next()?.trim();
                let req_id = parts.next()?.trim();
                Some(Self {
                    source: ArchiveRecordSource::TurnLog,
                    chat_id: Some(chat_id.to_string()),
                    message_id: None,
                    message_index: None,
                    note_name: None,
                    req_id: Some(req_id.to_string()),
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveSearchHit {
    pub record_id: String,
    pub citation: String,
    pub locator: ArchiveRecordLocator,
    pub source: ArchiveRecordSource,
    pub title: String,
    pub excerpt: String,
    pub score: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_trace: Option<ArchiveRetrievalTrace>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveSearchSourceStats {
    pub source: ArchiveRecordSource,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub candidate_count: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub hit_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveSearchQueryReport {
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub normalized_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_chat_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requested_sources: Vec<ArchiveRecordSource>,
    pub limit: usize,
    #[serde(default)]
    pub weak_query: bool,
    #[serde(default)]
    pub backend: ArchiveSearchBackendKind,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub candidate_count: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub returned_hit_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_citations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_match_terms: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_stats: Vec<ArchiveSearchSourceStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub miss_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveSearchResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<ArchiveSearchHit>,
    pub report: ArchiveSearchQueryReport,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveRecord {
    pub record_id: String,
    pub citation: String,
    pub locator: ArchiveRecordLocator,
    pub source: ArchiveRecordSource,
    pub title: String,
    pub excerpt: String,
    pub content: String,
    pub content_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cues: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_trace: Option<ArchiveRetrievalTrace>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArchiveSearchQuery<'a> {
    pub query: &'a str,
    pub preferred_chat_id: Option<&'a str>,
    pub chat_id_filter: Option<&'a str>,
    pub sources: &'a [ArchiveRecordSource],
    pub limit: usize,
}

#[derive(Clone)]
struct ArchiveSearchCandidate {
    locator: ArchiveRecordLocator,
    source: ArchiveRecordSource,
    title: String,
    content: String,
    cues: Vec<String>,
    observed_at: Option<u64>,
    current_chat_match: bool,
    normalized_title: String,
    normalized_content: String,
    normalized_document: String,
    estimated_doc_len: usize,
    backend_fts_score: u32,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ArchiveSourceSignature {
    sessions_files: u64,
    sessions_bytes: u64,
    sessions_latest_mtime_ns: u64,
    sessions_fingerprint: u64,
    daily_files: u64,
    daily_bytes: u64,
    daily_latest_mtime_ns: u64,
    daily_fingerprint: u64,
    turn_log_bytes: u64,
    turn_log_mtime_ns: u64,
    turn_log_fingerprint: u64,
}

#[derive(Default)]
struct ArchiveCorpusStats {
    document_count: usize,
    avg_doc_len: f32,
    document_frequency: HashMap<String, usize>,
}

#[derive(Clone, Copy)]
struct PreparedArchiveSearchQuery<'a> {
    raw: ArchiveSearchQuery<'a>,
    limit: usize,
    weak_query: bool,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

impl<'a> PreparedArchiveSearchQuery<'a> {
    fn new(raw: ArchiveSearchQuery<'a>, terms: &'a [String]) -> Self {
        Self {
            raw,
            limit: raw.limit.clamp(1, MAX_ARCHIVE_SEARCH_LIMIT),
            weak_query: raw.query.trim().is_empty() || terms.is_empty(),
        }
    }
}

pub fn search_archive_records_detailed(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: ArchiveSearchQuery<'_>,
) -> Result<ArchiveSearchResult> {
    let terms = collect_archive_match_terms(query.query);
    let prepared = PreparedArchiveSearchQuery::new(query, &terms);
    #[cfg(all(target_os = "linux", not(test)))]
    match search_archive_records_from_sqlite_detailed(
        session_store,
        memory_store,
        turn_ledger_store,
        prepared,
        &terms,
    ) {
        Ok(Some(result)) => return Ok(result),
        Ok(None) => {}
        Err(error) => log::warn!(
            "[archive_search] sqlite backend failed, falling back: {}",
            error
        ),
    }
    let candidates =
        collect_live_archive_candidates(session_store, memory_store, turn_ledger_store, query)?;
    let candidate_sources = candidates
        .iter()
        .map(|candidate| candidate.source)
        .collect::<Vec<_>>();
    let stats = build_archive_corpus_stats(&candidates, &terms);
    let newest_observed_at = candidates
        .iter()
        .filter_map(|candidate| candidate.observed_at)
        .max()
        .unwrap_or(0);
    let mut hits = score_archive_candidates(
        candidates,
        prepared,
        &terms,
        &stats,
        newest_observed_at,
        archive_search_backend_kind_fallback(),
    );
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });
    hits.truncate(prepared.limit);
    Ok(ArchiveSearchResult {
        report: build_archive_search_query_report(
            prepared,
            &terms,
            archive_search_backend_kind_fallback(),
            &candidate_sources,
            &hits,
        ),
        hits,
    })
}

pub fn search_archive_records(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: ArchiveSearchQuery<'_>,
) -> Result<Vec<ArchiveSearchHit>> {
    Ok(
        search_archive_records_detailed(session_store, memory_store, turn_ledger_store, query)?
            .hits,
    )
}

pub(crate) fn maintain_archive_search_backend(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
) -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let signature = build_archive_source_signature()?;
        let path = archive_index_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut conn = Connection::open(path)
            .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
        ensure_archive_sqlite_schema(&conn)?;
        let needs_rebuild = archive_sqlite_needs_rebuild(&conn, &signature)?;
        if needs_rebuild {
            let live = collect_live_archive_candidates(
                session_store,
                memory_store,
                turn_ledger_store,
                ArchiveSearchQuery {
                    query: "",
                    preferred_chat_id: None,
                    chat_id_filter: None,
                    sources: &[],
                    limit: 1,
                },
            )?;
            archive_sqlite_rebuild(&mut conn, &live, &signature)?;
        }
        Ok(needs_rebuild)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = session_store;
        let _ = memory_store;
        let _ = turn_ledger_store;
        Ok(false)
    }
}

fn collect_live_archive_candidates(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: ArchiveSearchQuery<'_>,
) -> Result<Vec<ArchiveSearchCandidate>> {
    let mut candidates = Vec::new();
    let source_filter = query.sources;
    let needs_chat_ids = source_filter.is_empty()
        || source_filter.contains(&ArchiveRecordSource::Transcript)
        || source_filter.contains(&ArchiveRecordSource::TurnLog);
    let chat_ids = if needs_chat_ids {
        collect_archive_chat_ids(session_store, query.preferred_chat_id, query.chat_id_filter)?
    } else {
        Vec::new()
    };

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::Transcript) {
        for chat_id in &chat_ids {
            let messages = session_store
                .load_recent_records(chat_id, MAX_SESSION_ENTRIES)
                .map_err(|error| error.with_stage("archive_search_transcript"))?;
            for (index, message) in messages.iter().enumerate() {
                let content = message.content.trim();
                if content.is_empty() {
                    continue;
                }
                let title = format!("{} in {}", message.role.to_uppercase(), chat_id);
                let locator = ArchiveRecordLocator {
                    source: ArchiveRecordSource::Transcript,
                    chat_id: Some(chat_id.clone()),
                    message_id: Some(message.message_id.clone()),
                    message_index: Some(index),
                    note_name: None,
                    req_id: None,
                };
                let mut cues = vec!["recent transcript".to_string()];
                if query.preferred_chat_id == Some(chat_id.as_str()) {
                    cues.push("current chat".to_string());
                }
                candidates.push(build_archive_search_candidate(
                    ArchiveSearchCandidateInput {
                        locator,
                        source: ArchiveRecordSource::Transcript,
                        title,
                        content: content.to_string(),
                        cues,
                        observed_at: None,
                        current_chat_match: query.preferred_chat_id == Some(chat_id.as_str()),
                        backend_fts_score: 0,
                    },
                ));
            }
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::DailyNote) {
        for name in memory_store
            .list_daily_note_names(usize::MAX)
            .map_err(|error| error.with_stage("archive_search_daily_notes"))?
        {
            let content = memory_store
                .get_daily_note(&name)
                .map_err(|error| error.with_stage("archive_search_daily_note_read"))?;
            let content = content.trim();
            if content.is_empty() {
                continue;
            }
            let observed_at = parse_daily_note_observed_at(&name);
            let mut cues = vec!["daily archive context".to_string()];
            if observed_at.is_some() {
                cues.push("dated note".to_string());
            }
            let locator = ArchiveRecordLocator {
                source: ArchiveRecordSource::DailyNote,
                chat_id: None,
                message_id: None,
                message_index: None,
                note_name: Some(name.clone()),
                req_id: None,
            };
            candidates.push(build_archive_search_candidate(
                ArchiveSearchCandidateInput {
                    locator,
                    source: ArchiveRecordSource::DailyNote,
                    title: name,
                    content: content.to_string(),
                    cues,
                    observed_at,
                    current_chat_match: false,
                    backend_fts_score: 0,
                },
            ));
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::TurnLog) {
        for chat_id in &chat_ids {
            let Some(ledger) = turn_ledger_store
                .get(chat_id)
                .map_err(|error| error.with_stage("archive_search_turn_log"))?
            else {
                continue;
            };
            let content = render_turn_log_content(&ledger);
            if content.is_empty() {
                continue;
            }
            let title = format!("{} turn in {}", ledger.status.label(), chat_id);
            let locator = ArchiveRecordLocator {
                source: ArchiveRecordSource::TurnLog,
                chat_id: Some(chat_id.clone()),
                message_id: None,
                message_index: None,
                note_name: None,
                req_id: Some(ledger.req_id.clone()),
            };
            let mut cues = vec!["execution log".to_string()];
            if query.preferred_chat_id == Some(chat_id.as_str()) {
                cues.push("current chat".to_string());
            }
            candidates.push(build_archive_search_candidate(
                ArchiveSearchCandidateInput {
                    locator,
                    source: ArchiveRecordSource::TurnLog,
                    title,
                    content,
                    cues,
                    observed_at: turn_log_observed_at(&ledger),
                    current_chat_match: query.preferred_chat_id == Some(chat_id.as_str()),
                    backend_fts_score: 0,
                },
            ));
        }
    }

    Ok(candidates)
}

#[cfg(all(target_os = "linux", not(test)))]
fn search_archive_records_from_sqlite_detailed(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
) -> Result<Option<ArchiveSearchResult>> {
    let signature = match build_archive_source_signature() {
        Ok(signature) => signature,
        Err(error) => {
            log::warn!("[archive_search] sqlite signature failed: {}", error);
            return Ok(None);
        }
    };
    let path = archive_index_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut conn = match Connection::open(path) {
        Ok(conn) => conn,
        Err(error) => {
            log::warn!("[archive_search] sqlite open failed: {}", error);
            return Ok(None);
        }
    };
    if let Err(error) = ensure_archive_sqlite_schema(&conn) {
        log::warn!("[archive_search] sqlite schema failed: {}", error);
        return Ok(None);
    }
    if archive_sqlite_needs_rebuild(&conn, &signature)? {
        let live = match collect_live_archive_candidates(
            session_store,
            memory_store,
            turn_ledger_store,
            query.raw,
        ) {
            Ok(live) => live,
            Err(error) => {
                log::warn!(
                    "[archive_search] sqlite rebuild skipped due to live corpus error: {}",
                    error
                );
                return Ok(None);
            }
        };
        archive_sqlite_rebuild(&mut conn, &live, &signature)?;
    }
    let candidates = query_archive_candidates_sqlite(&conn, query, terms)?;
    let candidate_sources = candidates
        .iter()
        .map(|candidate| candidate.source)
        .collect::<Vec<_>>();
    let stats = build_archive_corpus_stats(&candidates, terms);
    let newest_observed_at = candidates
        .iter()
        .filter_map(|candidate| candidate.observed_at)
        .max()
        .unwrap_or(0);
    let mut hits = score_archive_candidates(
        candidates,
        query,
        terms,
        &stats,
        newest_observed_at,
        ArchiveSearchBackendKind::SqliteFtsHybrid,
    );
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });
    hits.truncate(query.limit);
    Ok(Some(ArchiveSearchResult {
        report: build_archive_search_query_report(
            query,
            terms,
            ArchiveSearchBackendKind::SqliteFtsHybrid,
            &candidate_sources,
            &hits,
        ),
        hits,
    }))
}

#[cfg(target_os = "linux")]
fn archive_index_path() -> PathBuf {
    crate::platform::state_mount_path().join(REL_PATH_ARCHIVE_INDEX)
}

#[cfg(target_os = "linux")]
fn ensure_archive_sqlite_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS archive_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS archive_documents (
            record_id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            chat_id TEXT,
            message_id TEXT,
            message_index INTEGER,
            note_name TEXT,
            req_id TEXT,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            cues TEXT NOT NULL,
            observed_at INTEGER,
            updated_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_archive_documents_source ON archive_documents(source);
        CREATE INDEX IF NOT EXISTS idx_archive_documents_chat ON archive_documents(chat_id);
        CREATE INDEX IF NOT EXISTS idx_archive_documents_observed_at ON archive_documents(observed_at);
        CREATE VIRTUAL TABLE IF NOT EXISTS archive_documents_fts USING fts5(
            record_id UNINDEXED,
            title,
            content,
            cues,
            tokenize='unicode61 remove_diacritics 2'
        );",
    )
    .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))
    ?;
    match conn.execute(
        "ALTER TABLE archive_documents ADD COLUMN message_id TEXT",
        [],
    ) {
        Ok(_) => Ok(()),
        Err(error) if error.to_string().contains("duplicate column name") => Ok(()),
        Err(error) => Err(crate::error::Error::config(
            "archive_index",
            error.to_string(),
        )),
    }
}

#[cfg(target_os = "linux")]
fn archive_sqlite_needs_rebuild(
    conn: &Connection,
    signature: &ArchiveSourceSignature,
) -> Result<bool> {
    let version = conn
        .query_row(
            "SELECT value FROM archive_meta WHERE key = 'version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    if version.as_deref() != Some(&ARCHIVE_INDEX_VERSION.to_string()) {
        return Ok(true);
    }
    let stored_signature = conn
        .query_row(
            "SELECT value FROM archive_meta WHERE key = 'signature'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    let Some(stored_signature) = stored_signature else {
        return Ok(true);
    };
    let parsed = serde_json::from_str::<ArchiveSourceSignature>(&stored_signature)
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    Ok(parsed != *signature)
}

#[cfg(target_os = "linux")]
fn archive_sqlite_rebuild(
    conn: &mut Connection,
    candidates: &[ArchiveSearchCandidate],
    signature: &ArchiveSourceSignature,
) -> Result<()> {
    let tx = conn
        .transaction()
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    tx.execute("DELETE FROM archive_documents_fts", [])
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    tx.execute("DELETE FROM archive_documents", [])
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    for candidate in candidates {
        let cues = candidate.cues.join("\n");
        tx.execute(
            "INSERT INTO archive_documents (
                record_id, source, chat_id, message_id, message_index, note_name, req_id,
                title, content, cues, observed_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                candidate.locator.record_id(),
                candidate.source.label(),
                candidate.locator.chat_id.as_deref(),
                candidate.locator.message_id.as_deref(),
                candidate.locator.message_index.map(|value| value as i64),
                candidate.locator.note_name.as_deref(),
                candidate.locator.req_id.as_deref(),
                candidate.title,
                candidate.content,
                cues,
                candidate.observed_at.map(|value| value as i64),
                crate::util::current_unix_secs() as i64,
            ],
        )
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO archive_documents_fts(rowid, record_id, title, content, cues)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                rowid,
                candidate.locator.record_id(),
                candidate.title,
                candidate.content,
                candidate.cues.join(" "),
            ],
        )
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    }
    tx.execute(
        "INSERT INTO archive_meta(key, value) VALUES('version', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![ARCHIVE_INDEX_VERSION.to_string()],
    )
    .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    tx.execute(
        "INSERT INTO archive_meta(key, value) VALUES('signature', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![serde_json::to_string(signature)
            .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?],
    )
    .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    tx.commit()
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))
}

#[cfg(all(target_os = "linux", not(test)))]
fn query_archive_candidates_sqlite(
    conn: &Connection,
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
) -> Result<Vec<ArchiveSearchCandidate>> {
    let mut out = std::collections::HashMap::<String, ArchiveSearchCandidate>::new();
    if !query.weak_query {
        if let Some(match_expr) = archive_sqlite_match_expression(terms) {
            let mut stmt = conn
                .prepare(
                    "SELECT d.record_id, d.source, d.chat_id, d.message_id, d.message_index, d.note_name, d.req_id,
                            d.title, d.content, d.cues, d.observed_at, bm25(archive_documents_fts, 6.0, 1.5, 1.0) as rank
                     FROM archive_documents_fts
                     JOIN archive_documents d ON d.rowid = archive_documents_fts.rowid
                     WHERE archive_documents_fts MATCH ?1
                     ORDER BY rank ASC
                     LIMIT 48",
                )
                .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
            let rows = stmt
                .query_map(params![match_expr], |row| {
                    map_archive_sqlite_candidate_row(row, query.raw.preferred_chat_id)
                })
                .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
            for row in rows {
                let candidate =
                    row.map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
                if !archive_candidate_matches_query(&candidate, query.raw) {
                    continue;
                }
                upsert_archive_candidate(out.entry(candidate.locator.record_id()), candidate);
            }
        }
    }
    let mut stmt = conn
        .prepare(
            "SELECT record_id, source, chat_id, message_id, message_index, note_name, req_id,
                    title, content, cues, observed_at, 0.0 as rank
             FROM archive_documents
             ORDER BY observed_at DESC, rowid DESC
             LIMIT 64",
        )
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            map_archive_sqlite_candidate_row(row, query.raw.preferred_chat_id)
        })
        .map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
    for row in rows {
        let candidate =
            row.map_err(|e| crate::error::Error::config("archive_index", e.to_string()))?;
        if !archive_candidate_matches_query(&candidate, query.raw) {
            continue;
        }
        upsert_archive_candidate(out.entry(candidate.locator.record_id()), candidate);
    }
    Ok(out.into_values().collect())
}

#[cfg(all(target_os = "linux", not(test)))]
fn map_archive_sqlite_candidate_row(
    row: &rusqlite::Row<'_>,
    preferred_chat_id: Option<&str>,
) -> rusqlite::Result<ArchiveSearchCandidate> {
    let source = row
        .get::<_, String>(1)
        .ok()
        .and_then(|value| value.parse::<ArchiveRecordSource>().ok())
        .unwrap_or(ArchiveRecordSource::Transcript);
    let chat_id = row.get::<_, Option<String>>(2)?;
    let locator = ArchiveRecordLocator {
        source,
        chat_id: chat_id.clone(),
        message_id: row.get::<_, Option<String>>(3)?,
        message_index: row
            .get::<_, Option<i64>>(4)?
            .and_then(|value| usize::try_from(value).ok()),
        note_name: row.get::<_, Option<String>>(5)?,
        req_id: row.get::<_, Option<String>>(6)?,
    };
    let title = row.get::<_, String>(7)?;
    let content = row.get::<_, String>(8)?;
    let cues = row
        .get::<_, String>(9)?
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let observed_at = row
        .get::<_, Option<i64>>(10)?
        .and_then(|value| u64::try_from(value).ok());
    let rank = row.get::<_, f64>(11).unwrap_or(0.0);
    let sqlite_fts_score = if rank > 0.0 {
        ((1.0 / (1.0 + rank)) * 64.0).round().max(0.0) as u32
    } else {
        0
    };
    Ok(build_archive_search_candidate(
        ArchiveSearchCandidateInput {
            locator,
            source,
            title,
            content,
            cues,
            observed_at,
            current_chat_match: chat_id
                .as_deref()
                .is_some_and(|chat_id| Some(chat_id) == preferred_chat_id),
            backend_fts_score: sqlite_fts_score,
        },
    ))
}

#[cfg(all(target_os = "linux", not(test)))]
fn upsert_archive_candidate(
    slot: std::collections::hash_map::Entry<'_, String, ArchiveSearchCandidate>,
    candidate: ArchiveSearchCandidate,
) {
    match slot {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if candidate.backend_fts_score > entry.get().backend_fts_score {
                entry.insert(candidate);
            }
        }
    }
}

#[cfg(all(target_os = "linux", not(test)))]
fn archive_candidate_matches_query(
    candidate: &ArchiveSearchCandidate,
    query: ArchiveSearchQuery<'_>,
) -> bool {
    if let Some(chat_id_filter) = query.chat_id_filter {
        if candidate.locator.chat_id.as_deref() != Some(chat_id_filter) {
            return false;
        }
    }
    query.sources.is_empty() || query.sources.contains(&candidate.source)
}

#[cfg(all(target_os = "linux", not(test)))]
fn archive_sqlite_match_expression(terms: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    for term in terms {
        let escaped = term.replace('"', "\"\"");
        if escaped.trim().is_empty() {
            continue;
        }
        parts.push(format!("\"{}\"", escaped));
    }
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

#[cfg(target_os = "linux")]
fn build_archive_source_signature() -> Result<ArchiveSourceSignature> {
    let root = crate::platform::state_mount_path();
    let sessions = scan_path_signature(&root.join(super::REL_PATH_SESSIONS_DIR))?;
    let daily = scan_path_signature(&root.join(super::REL_PATH_DAILY_DIR))?;
    let turn_logs = scan_path_signature(&root.join(super::REL_PATH_TURN_LEDGERS))?;
    Ok(ArchiveSourceSignature {
        sessions_files: sessions.0,
        sessions_bytes: sessions.1,
        sessions_latest_mtime_ns: sessions.2,
        sessions_fingerprint: sessions.3,
        daily_files: daily.0,
        daily_bytes: daily.1,
        daily_latest_mtime_ns: daily.2,
        daily_fingerprint: daily.3,
        turn_log_bytes: turn_logs.1,
        turn_log_mtime_ns: turn_logs.2,
        turn_log_fingerprint: turn_logs.3,
    })
}

#[cfg(target_os = "linux")]
fn scan_path_signature(path: &Path) -> Result<(u64, u64, u64, u64)> {
    if !path.exists() {
        return Ok((0, 0, 0, 0));
    }
    let meta = std::fs::metadata(path).map_err(|e| crate::error::Error::io("archive_index", e))?;
    if meta.is_file() {
        return Ok((
            1,
            meta.len(),
            modified_unix_nanos(&meta),
            file_content_fingerprint(path)?,
        ));
    }
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut latest = 0u64;
    let mut fingerprint = 0xcbf29ce484222325u64;
    let mut entries = std::fs::read_dir(path)
        .map_err(|e| crate::error::Error::io("archive_index", e))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| crate::error::Error::io("archive_index", e))?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let entry_path = entry.path();
        let meta = entry
            .metadata()
            .map_err(|e| crate::error::Error::io("archive_index", e))?;
        if meta.is_dir() {
            let nested = scan_path_signature(&entry_path)?;
            files = files.saturating_add(nested.0);
            bytes = bytes.saturating_add(nested.1);
            latest = latest.max(nested.2);
            archive_signature_hash_update(&mut fingerprint, &nested.3.to_le_bytes());
        } else {
            files = files.saturating_add(1);
            bytes = bytes.saturating_add(meta.len());
            let modified_ns = modified_unix_nanos(&meta);
            latest = latest.max(modified_ns);
            let file_name = entry.file_name();
            archive_signature_hash_update(&mut fingerprint, file_name.to_string_lossy().as_bytes());
            archive_signature_hash_update(&mut fingerprint, &meta.len().to_le_bytes());
            archive_signature_hash_update(&mut fingerprint, &modified_ns.to_le_bytes());
            archive_signature_hash_update(
                &mut fingerprint,
                &file_content_fingerprint(&entry_path)?.to_le_bytes(),
            );
        }
    }
    Ok((files, bytes, latest, fingerprint))
}

#[cfg(target_os = "linux")]
fn modified_unix_nanos(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn archive_signature_hash_update(hash: &mut u64, bytes: &[u8]) {
    const FNV_PRIME: u64 = 0x100000001b3;
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

#[cfg(target_os = "linux")]
fn file_content_fingerprint(path: &Path) -> Result<u64> {
    let bytes = std::fs::read(path).map_err(|e| crate::error::Error::io("archive_index", e))?;
    let mut hash = 0xcbf29ce484222325u64;
    archive_signature_hash_update(&mut hash, &bytes);
    Ok(hash)
}

fn score_archive_candidates(
    candidates: Vec<ArchiveSearchCandidate>,
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
    stats: &ArchiveCorpusStats,
    newest_observed_at: u64,
    backend: ArchiveSearchBackendKind,
) -> Vec<ArchiveSearchHit> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let (score, trace, matched_terms) = score_archive_candidate(
                &candidate,
                query,
                terms,
                stats,
                newest_observed_at,
                backend,
            );
            let substantive = trace.score.lexical_score > 0
                || trace.score.fts_score > 0
                || trace.score.hybrid_score >= 4;
            if !query.weak_query && !substantive {
                return None;
            }
            Some(ArchiveSearchHit {
                record_id: candidate.locator.record_id(),
                citation: candidate.locator.citation(),
                excerpt: pick_archive_excerpt(
                    &candidate.content,
                    if matched_terms.is_empty() {
                        terms
                    } else {
                        matched_terms.as_slice()
                    },
                    ARCHIVE_SEARCH_EXCERPT_LEN,
                ),
                locator: candidate.locator,
                source: candidate.source,
                title: candidate.title,
                score,
                cues: build_archive_hit_cues(&candidate.cues, &trace),
                observed_at: candidate.observed_at,
                retrieval_trace: Some(trace),
            })
        })
        .collect()
}

fn score_archive_candidate(
    candidate: &ArchiveSearchCandidate,
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
    stats: &ArchiveCorpusStats,
    newest_observed_at: u64,
    backend: ArchiveSearchBackendKind,
) -> (u32, ArchiveRetrievalTrace, Vec<String>) {
    let matched_terms = matched_archive_terms(
        &candidate.normalized_title,
        &candidate.normalized_content,
        terms,
    );
    let lexical_score = lexical_archive_score(
        &candidate.normalized_title,
        &candidate.normalized_content,
        &matched_terms,
    );
    let fts_score = archive_fts_score(candidate, terms, stats);
    let hybrid_score = archive_hybrid_score(candidate, query.raw.query);
    let same_chat_bonus = if candidate.current_chat_match { 10 } else { 0 };
    let (source_bonus, source_reason) =
        archive_source_preference_bonus(candidate, query.raw.sources, same_chat_bonus > 0);
    let (recency_bonus, recency_reason) =
        archive_recency_bonus(candidate.observed_at, newest_observed_at);
    let total_score = lexical_score
        .saturating_add(fts_score)
        .saturating_add(hybrid_score)
        .saturating_add(same_chat_bonus)
        .saturating_add(source_bonus)
        .saturating_add(recency_bonus);
    let trace = ArchiveRetrievalTrace {
        backend,
        matched_terms: matched_terms
            .iter()
            .take(ARCHIVE_TRACE_MAX_MATCHED_TERMS)
            .cloned()
            .collect(),
        ranking_reason: Some(build_archive_ranking_reason(
            candidate,
            lexical_score,
            fts_score,
            hybrid_score,
            same_chat_bonus,
        )),
        source_reason,
        recency_reason,
        selector_reason: Some(build_archive_selector_reason(
            candidate,
            &matched_terms,
            query.raw.preferred_chat_id,
        )),
        score: ArchiveSearchScoreBreakdown {
            lexical_score,
            fts_score,
            hybrid_score,
            same_chat_bonus,
            source_bonus,
            recency_bonus,
            total_score,
        },
    };
    (total_score, trace, matched_terms)
}

fn build_archive_corpus_stats(
    candidates: &[ArchiveSearchCandidate],
    terms: &[String],
) -> ArchiveCorpusStats {
    if candidates.is_empty() {
        return ArchiveCorpusStats::default();
    }
    let mut total_len = 0usize;
    let mut document_frequency = HashMap::new();
    for candidate in candidates {
        total_len = total_len.saturating_add(candidate.estimated_doc_len);
        let mut seen = HashSet::new();
        for term in terms {
            if candidate.normalized_document.contains(term) && seen.insert(term.clone()) {
                *document_frequency.entry(term.clone()).or_insert(0) += 1;
            }
        }
    }
    ArchiveCorpusStats {
        document_count: candidates.len(),
        avg_doc_len: total_len as f32 / candidates.len() as f32,
        document_frequency,
    }
}

fn matched_archive_terms(
    normalized_title: &str,
    normalized_content: &str,
    terms: &[String],
) -> Vec<String> {
    let mut matched = Vec::new();
    for term in terms {
        if normalized_title.contains(term) || normalized_content.contains(term) {
            matched.push(term.clone());
        }
    }
    matched
}

fn lexical_archive_score(
    normalized_title: &str,
    normalized_content: &str,
    terms: &[String],
) -> u32 {
    if terms.is_empty() {
        return 1;
    }
    let mut score = 0u32;
    for term in terms {
        if normalized_title.contains(term) {
            score = score.saturating_add(6);
        }
        if normalized_content.contains(term) {
            score = score.saturating_add(4);
        }
    }
    score
}

fn archive_fts_score(
    candidate: &ArchiveSearchCandidate,
    terms: &[String],
    stats: &ArchiveCorpusStats,
) -> u32 {
    if candidate.backend_fts_score > 0 {
        return candidate.backend_fts_score;
    }
    if terms.is_empty() || stats.avg_doc_len <= 0.0 {
        return 0;
    }
    let doc_len = candidate.estimated_doc_len as f32;
    let avg_doc_len = stats.avg_doc_len.max(1.0);
    let mut score = 0.0f32;
    for term in terms {
        let tf_title = archive_term_frequency(&candidate.normalized_title, term) as f32;
        let tf_body = archive_term_frequency(&candidate.normalized_content, term) as f32;
        let tf = tf_title.mul_add(1.5, tf_body);
        if tf <= 0.0 {
            continue;
        }
        let df = stats.document_frequency.get(term).copied().unwrap_or(0) as f32;
        let idf = (((stats.document_count.max(1) as f32) + 1.0) / (df + 1.0)).ln_1p() + 1.0;
        let k1 = 1.2f32;
        let b = 0.75f32;
        let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * (doc_len / avg_doc_len)));
        score += idf * norm;
    }
    (score * 6.0).round().max(0.0) as u32
}

fn archive_hybrid_score(candidate: &ArchiveSearchCandidate, query_text: &str) -> u32 {
    let query = normalize_archive_match_text(query_text);
    if query.is_empty() {
        return 0;
    }
    trigram_overlap_score(&query, &candidate.normalized_document, 24)
}

struct ArchiveSearchCandidateInput {
    locator: ArchiveRecordLocator,
    source: ArchiveRecordSource,
    title: String,
    content: String,
    cues: Vec<String>,
    observed_at: Option<u64>,
    current_chat_match: bool,
    backend_fts_score: u32,
}

fn build_archive_search_candidate(input: ArchiveSearchCandidateInput) -> ArchiveSearchCandidate {
    let normalized_title = normalize_archive_match_text(&input.title);
    let normalized_content = normalize_archive_match_text(&input.content);
    let normalized_document =
        combine_normalized_archive_parts(&normalized_title, &normalized_content);
    let estimated_doc_len = archive_document_len(&normalized_document);
    ArchiveSearchCandidate {
        locator: input.locator,
        source: input.source,
        title: input.title,
        content: input.content,
        cues: input.cues,
        observed_at: input.observed_at,
        current_chat_match: input.current_chat_match,
        normalized_title,
        normalized_content,
        normalized_document,
        estimated_doc_len,
        backend_fts_score: input.backend_fts_score,
    }
}

fn combine_normalized_archive_parts(title: &str, content: &str) -> String {
    match (title.is_empty(), content.is_empty()) {
        (true, true) => String::new(),
        (false, true) => title.to_string(),
        (true, false) => content.to_string(),
        (false, false) => {
            let mut combined = String::with_capacity(title.len() + content.len() + 1);
            combined.push_str(title);
            combined.push(' ');
            combined.push_str(content);
            combined
        }
    }
}

fn archive_document_len(normalized_document: &str) -> usize {
    normalized_document
        .split_whitespace()
        .count()
        .max(normalized_document.chars().count() / 4)
}

fn archive_source_preference_bonus(
    candidate: &ArchiveSearchCandidate,
    source_preferences: &[ArchiveRecordSource],
    current_chat_match: bool,
) -> (u32, Option<String>) {
    if let Some(index) = source_preferences
        .iter()
        .position(|source| *source == candidate.source)
    {
        let bonus = ((source_preferences.len().saturating_sub(index)) as u32).saturating_mul(2);
        return (
            bonus,
            Some(format!(
                "requested source preference favored {}",
                candidate.source.label()
            )),
        );
    }
    let (bonus, label) = match candidate.source {
        ArchiveRecordSource::Transcript if current_chat_match => (4, "current transcript evidence"),
        ArchiveRecordSource::Transcript => (3, "transcript evidence"),
        ArchiveRecordSource::DailyNote => (2, "durable daily-note evidence"),
        ArchiveRecordSource::TurnLog => (1, "execution-log evidence"),
    };
    (bonus, Some(label.to_string()))
}

fn archive_recency_bonus(
    observed_at: Option<u64>,
    newest_observed_at: u64,
) -> (u32, Option<String>) {
    let Some(observed_at) = observed_at else {
        return (0, None);
    };
    if newest_observed_at == 0 || observed_at > newest_observed_at {
        return (0, None);
    }
    let age = newest_observed_at.saturating_sub(observed_at);
    let (bonus, label) = if age <= 86_400 {
        (6, "same-day evidence")
    } else if age <= 7 * 86_400 {
        (4, "recent-week evidence")
    } else if age <= 30 * 86_400 {
        (2, "recent-month evidence")
    } else {
        (0, "older evidence")
    };
    (bonus, (bonus > 0).then_some(label.to_string()))
}

fn build_archive_ranking_reason(
    candidate: &ArchiveSearchCandidate,
    lexical_score: u32,
    fts_score: u32,
    hybrid_score: u32,
    same_chat_bonus: u32,
) -> String {
    let mut parts = Vec::with_capacity(4);
    if lexical_score > 0 {
        parts.push("exact term overlap".to_string());
    }
    if fts_score > 0 {
        parts.push("fts-style term weighting".to_string());
    }
    if hybrid_score > 0 {
        parts.push("hybrid fuzzy match".to_string());
    }
    if same_chat_bonus > 0 && candidate.current_chat_match {
        parts.push("same chat boost".to_string());
    }
    if parts.is_empty() {
        format!("{} evidence remained eligible", candidate.source.label())
    } else {
        parts.join(", ")
    }
}

fn build_archive_hit_cues(base_cues: &[String], trace: &ArchiveRetrievalTrace) -> Vec<String> {
    let mut cues = base_cues.to_vec();
    for term in trace
        .matched_terms
        .iter()
        .take(ARCHIVE_TRACE_MAX_MATCHED_TERMS)
    {
        cues.push(format!("match:{term}"));
    }
    if trace.score.recency_bonus > 0 {
        cues.push(format!("recent+{}", trace.score.recency_bonus));
    }
    cues
}

fn build_archive_search_query_report(
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
    backend: ArchiveSearchBackendKind,
    candidate_sources: &[ArchiveRecordSource],
    hits: &[ArchiveSearchHit],
) -> ArchiveSearchQueryReport {
    let candidate_count = candidate_sources.len();
    ArchiveSearchQueryReport {
        query: query.raw.query.trim().to_string(),
        normalized_terms: terms.to_vec(),
        preferred_chat_id: query.raw.preferred_chat_id.map(str::to_string),
        chat_id_filter: query.raw.chat_id_filter.map(str::to_string),
        requested_sources: query.raw.sources.to_vec(),
        limit: query.limit,
        weak_query: query.weak_query,
        backend,
        candidate_count,
        returned_hit_count: hits.len(),
        top_citations: collect_archive_top_citations(hits, 3),
        top_match_terms: collect_archive_top_match_terms(hits, 4),
        source_stats: build_archive_source_stats(candidate_sources, hits, query.raw.sources),
        miss_reason: build_archive_search_miss_reason(query, terms, candidate_count, hits),
    }
}

fn collect_archive_top_citations(hits: &[ArchiveSearchHit], max_items: usize) -> Vec<String> {
    let mut citations = Vec::with_capacity(hits.len().min(max_items));
    for hit in hits {
        if citations.iter().any(|existing| existing == &hit.citation) {
            continue;
        }
        citations.push(hit.citation.clone());
        if citations.len() >= max_items {
            break;
        }
    }
    citations
}

fn collect_archive_top_match_terms(hits: &[ArchiveSearchHit], max_items: usize) -> Vec<String> {
    let mut terms = Vec::with_capacity(max_items);
    for hit in hits {
        let Some(trace) = hit.retrieval_trace.as_ref() else {
            continue;
        };
        for term in &trace.matched_terms {
            if terms.iter().any(|existing| existing == term) {
                continue;
            }
            terms.push(term.clone());
            if terms.len() >= max_items {
                return terms;
            }
        }
    }
    terms
}

fn build_archive_source_stats(
    candidate_sources: &[ArchiveRecordSource],
    hits: &[ArchiveSearchHit],
    requested_sources: &[ArchiveRecordSource],
) -> Vec<ArchiveSearchSourceStats> {
    let mut candidate_by_source = HashMap::<ArchiveRecordSource, usize>::new();
    let mut hit_by_source = HashMap::<ArchiveRecordSource, usize>::new();
    for source in candidate_sources {
        *candidate_by_source.entry(*source).or_insert(0) += 1;
    }
    for hit in hits {
        *hit_by_source.entry(hit.source).or_insert(0) += 1;
    }
    let mut sources = if requested_sources.is_empty() {
        vec![
            ArchiveRecordSource::Transcript,
            ArchiveRecordSource::DailyNote,
            ArchiveRecordSource::TurnLog,
        ]
    } else {
        requested_sources.to_vec()
    };
    sources.retain(|source| {
        *candidate_by_source.get(source).unwrap_or(&0) > 0
            || *hit_by_source.get(source).unwrap_or(&0) > 0
    });
    if sources.is_empty() && !candidate_sources.is_empty() {
        sources = candidate_by_source.keys().copied().collect();
        sources.sort_by_key(|source| source.label());
    }
    sources
        .into_iter()
        .map(|source| ArchiveSearchSourceStats {
            source,
            candidate_count: *candidate_by_source.get(&source).unwrap_or(&0),
            hit_count: *hit_by_source.get(&source).unwrap_or(&0),
        })
        .collect()
}

fn build_archive_search_miss_reason(
    query: PreparedArchiveSearchQuery<'_>,
    terms: &[String],
    candidate_count: usize,
    hits: &[ArchiveSearchHit],
) -> Option<String> {
    if !hits.is_empty() {
        return None;
    }
    if query.raw.query.trim().is_empty() {
        return Some("empty_query".to_string());
    }
    if terms.is_empty() || query.weak_query {
        return Some("query_terms_insufficient".to_string());
    }
    if candidate_count == 0 {
        if query.raw.chat_id_filter.is_some() {
            Some("no_archive_candidates_after_chat_filter".to_string())
        } else if !query.raw.sources.is_empty() {
            Some("no_archive_candidates_for_requested_sources".to_string())
        } else {
            Some("archive_corpus_empty".to_string())
        }
    } else {
        Some("no_substantive_archive_match".to_string())
    }
}

fn build_archive_selector_reason(
    candidate: &ArchiveSearchCandidate,
    matched_terms: &[String],
    preferred_chat_id: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("source={}", candidate.source.label()));
    if let Some(chat_id) = candidate.locator.chat_id.as_deref() {
        if Some(chat_id) == preferred_chat_id {
            parts.push("current-chat preferred".to_string());
        } else {
            parts.push(format!("chat={chat_id}"));
        }
    }
    if let Some(observed_at) = candidate.observed_at {
        parts.push(format!("observed_at={observed_at}"));
    }
    if !matched_terms.is_empty() {
        parts.push(format!(
            "matched={}",
            matched_terms
                .iter()
                .take(ARCHIVE_TRACE_MAX_MATCHED_TERMS)
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    parts.join("; ")
}

fn archive_term_frequency(text: &str, term: &str) -> usize {
    if text.is_empty() || term.is_empty() {
        return 0;
    }
    text.match_indices(term).count()
}

fn archive_search_backend_kind_fallback() -> ArchiveSearchBackendKind {
    ArchiveSearchBackendKind::IndexedHybrid
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

pub fn get_archive_record(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    locator: &ArchiveRecordLocator,
    focus_query: Option<&str>,
    max_content_chars: usize,
) -> Result<Option<ArchiveRecord>> {
    let content_limit = max_content_chars.clamp(256, MAX_ARCHIVE_GET_CONTENT_LEN);
    let terms = collect_archive_match_terms(focus_query.unwrap_or_default());
    match locator.source {
        ArchiveRecordSource::Transcript => {
            let Some(chat_id) = locator.chat_id.as_deref() else {
                return Ok(None);
            };
            let messages = session_store.load_recent_records(chat_id, MAX_SESSION_ENTRIES)?;
            let message = if let Some(message_id) = locator.message_id.as_deref() {
                messages
                    .iter()
                    .find(|message| message.message_id == message_id)
            } else if let Some(index) = locator.message_index {
                messages.get(index)
            } else {
                None
            };
            let Some(message) = message else {
                return Ok(None);
            };
            let content = message.content.trim();
            if content.is_empty() {
                return Ok(None);
            }
            let title = format!("{} in {}", message.role.to_uppercase(), chat_id);
            Ok(Some(build_archive_record(
                locator.clone(),
                title,
                content,
                vec!["recent transcript".to_string()],
                None,
                &terms,
                content_limit,
            )))
        }
        ArchiveRecordSource::DailyNote => {
            let Some(note_name) = locator.note_name.as_deref() else {
                return Ok(None);
            };
            let content = memory_store.get_daily_note(note_name)?;
            let content = content.trim();
            if content.is_empty() {
                return Ok(None);
            }
            Ok(Some(build_archive_record(
                locator.clone(),
                note_name.to_string(),
                content,
                vec!["daily archive context".to_string()],
                parse_daily_note_observed_at(note_name),
                &terms,
                content_limit,
            )))
        }
        ArchiveRecordSource::TurnLog => {
            let Some(chat_id) = locator.chat_id.as_deref() else {
                return Ok(None);
            };
            let Some(ledger) = turn_ledger_store.get(chat_id)? else {
                return Ok(None);
            };
            if let Some(req_id) = locator.req_id.as_deref() {
                let req_id = req_id.trim();
                if !req_id.is_empty() && req_id != "latest" && ledger.req_id != req_id {
                    return Ok(None);
                }
            }
            let content = render_turn_log_content(&ledger);
            if content.is_empty() {
                return Ok(None);
            }
            Ok(Some(build_archive_record(
                locator.clone(),
                format!("{} turn in {}", ledger.status.label(), chat_id),
                &content,
                vec!["execution log".to_string()],
                turn_log_observed_at(&ledger),
                &terms,
                content_limit,
            )))
        }
    }
}

pub(crate) fn collect_archive_match_terms(query: &str) -> Vec<String> {
    let normalized = normalize_archive_match_text(query);
    if normalized.is_empty() {
        return Vec::new();
    }
    let mut terms = collect_retrieval_terms(&normalized, 2, 24, &[2, 3]);
    if normalized.split_whitespace().count() > 1 {
        push_archive_term(&mut terms, &normalized);
    }
    terms
}

fn push_archive_term(terms: &mut Vec<String>, term: &str) {
    let trimmed = term.trim();
    if trimmed.chars().count() < 2 || terms.iter().any(|item| item == trimmed) {
        return;
    }
    terms.push(trimmed.to_string());
}

pub(crate) fn normalize_archive_match_text(input: &str) -> String {
    normalize_retrieval_text(input)
}

pub(crate) fn pick_archive_excerpt(content: &str, terms: &[String], max_chars: usize) -> String {
    if content.trim().is_empty() {
        return String::new();
    }
    if !terms.is_empty() {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let normalized = normalize_archive_match_text(trimmed);
            if terms.iter().any(|term| normalized.contains(term)) {
                return truncate_content_to_max(trimmed, max_chars).to_string();
            }
        }
    }
    truncate_content_to_max(content.trim(), max_chars).to_string()
}

fn build_archive_record(
    locator: ArchiveRecordLocator,
    title: String,
    content: &str,
    cues: Vec<String>,
    observed_at: Option<u64>,
    excerpt_terms: &[String],
    max_content_chars: usize,
) -> ArchiveRecord {
    let total_chars = content.chars().count();
    let content_truncated = total_chars > max_content_chars;
    let content = if content_truncated {
        truncate_content_to_max(content, max_content_chars).to_string()
    } else {
        content.to_string()
    };
    ArchiveRecord {
        record_id: locator.record_id(),
        citation: locator.citation(),
        source: locator.source,
        locator,
        title,
        excerpt: pick_archive_excerpt(&content, excerpt_terms, ARCHIVE_GET_EXCERPT_LEN),
        content,
        content_truncated,
        cues,
        observed_at,
        retrieval_trace: None,
    }
}

fn collect_archive_chat_ids(
    session_store: &dyn SessionStore,
    preferred_chat_id: Option<&str>,
    chat_id_filter: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(chat_id) = chat_id_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(vec![chat_id.to_string()]);
    }
    let mut chat_ids = session_store
        .list_chat_ids()
        .map_err(|error| error.with_stage("archive_search_chat_ids"))?;
    if let Some(preferred_chat_id) = preferred_chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        chat_ids.retain(|chat_id| chat_id != preferred_chat_id);
        chat_ids.insert(0, preferred_chat_id.to_string());
    }
    Ok(chat_ids)
}

fn render_turn_log_content(ledger: &TurnLedger) -> String {
    let mut parts = [
        (!ledger.reason.trim().is_empty()).then(|| format!("reason={}", ledger.reason.trim())),
        (!ledger.user_preview.trim().is_empty())
            .then(|| format!("user={}", ledger.user_preview.trim())),
        (!ledger.reply_preview.trim().is_empty())
            .then(|| format!("reply={}", ledger.reply_preview.trim())),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    if let Some(persona_summary) = (ledger.ingress == crate::bus::IngressKind::User)
        .then_some(ledger.persona.as_ref())
        .flatten()
        .and_then(|persona| {
            render_turn_persona_ledger_block(persona, 320)
                .map(|block| block.lines().skip(1).collect::<Vec<_>>().join(" | "))
        })
    {
        parts.push(format!("persona={}", persona_summary));
    }
    if let Some(observation_summary) = ledger.observation.as_ref().and_then(|observation| {
        render_turn_observation_ledger_block(observation, 420)
            .map(|block| block.lines().skip(1).collect::<Vec<_>>().join(" | "))
    }) {
        parts.push(format!("observation={}", observation_summary));
    }
    parts.join("; ")
}

fn turn_log_observed_at(ledger: &TurnLedger) -> Option<u64> {
    let millis = if ledger.finished_at_ms > 0 {
        ledger.finished_at_ms
    } else if ledger.updated_at_ms > 0 {
        ledger.updated_at_ms
    } else {
        ledger.started_at_ms
    };
    (millis > 0).then_some(millis / 1000)
}

pub(crate) fn parse_daily_note_observed_at(name: &str) -> Option<u64> {
    let stem = name.strip_suffix(".md").unwrap_or(name);
    let mut parts = stem.split('-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u32>().ok()?;
    let day = parts.next()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    ymd_to_unix_secs(year, month, day)
}

fn ymd_to_unix_secs(year: i32, month: u32, day: u32) -> Option<u64> {
    if year < 1970 {
        return None;
    }
    let mut days = 0i64;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    let month_lengths = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for len in month_lengths.iter().take(month.saturating_sub(1) as usize) {
        days += i64::from(*len);
    }
    days += i64::from(day.saturating_sub(1));
    (days >= 0).then_some((days as u64).saturating_mul(86_400))
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn archive_get_default_content_len() -> usize {
    DEFAULT_ARCHIVE_GET_CONTENT_LEN
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{SessionMessage, TurnLedger, TurnLedgerStatus};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        chats: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            unreachable!()
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
            let guard = self.chats.lock().unwrap_or_else(|e| e.into_inner());
            let items = guard.get(chat_id).cloned().unwrap_or_default();
            let start = items.len().saturating_sub(n);
            Ok(items.into_iter().skip(start).collect())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            unreachable!()
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
            unreachable!()
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

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            unreachable!()
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

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            unreachable!()
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            unreachable!()
        }
    }

    #[test]
    fn search_filters_sources_and_returns_roundtrip_locator() {
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
                        content: "讨论灯光自动化计划".to_string(),
                    },
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "已整理客厅灯光自动化方案".to_string(),
                    },
                ],
            );
        let memory_store = StubMemoryStore::default();
        memory_store
            .notes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "2026-04-02.md".to_string(),
                "今天把灯光自动化接入计划写进每日笔记".to_string(),
            );
        let turn_ledger_store = StubTurnLedgerStore::default();

        let hits = search_archive_records(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            ArchiveSearchQuery {
                query: "灯光 自动化",
                preferred_chat_id: Some("chat-a"),
                chat_id_filter: None,
                sources: &[ArchiveRecordSource::DailyNote],
                limit: 4,
            },
        )
        .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, ArchiveRecordSource::DailyNote);
        assert!(hits[0].citation.contains("2026-04-02.md"));
        assert_eq!(
            hits[0].retrieval_trace.as_ref().map(|trace| trace.backend),
            Some(archive_search_backend_kind_fallback())
        );
        assert_eq!(
            ArchiveRecordLocator::parse_record_id(&hits[0].record_id),
            Some(hits[0].locator.clone())
        );
    }

    #[test]
    fn detailed_search_reports_candidates_and_miss_reason() {
        let session_store = StubSessionStore::default();
        session_store
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "chat-a".to_string(),
                vec![SessionMessage {
                    role: "user".to_string(),
                    content: "家庭网络重构计划".to_string(),
                }],
            );
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();

        let result = search_archive_records_detailed(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            ArchiveSearchQuery {
                query: "zzqvv_no_match_2026",
                preferred_chat_id: Some("chat-a"),
                chat_id_filter: None,
                sources: &[ArchiveRecordSource::Transcript],
                limit: 4,
            },
        )
        .unwrap();

        assert!(result.report.candidate_count >= 1);
        let terms = collect_archive_match_terms("zzqvv_no_match_2026");
        let prepared = PreparedArchiveSearchQuery::new(
            ArchiveSearchQuery {
                query: "zzqvv_no_match_2026",
                preferred_chat_id: Some("chat-a"),
                chat_id_filter: None,
                sources: &[ArchiveRecordSource::Transcript],
                limit: 4,
            },
            &terms,
        );
        assert_eq!(
            build_archive_search_miss_reason(prepared, &terms, result.report.candidate_count, &[]),
            Some("no_substantive_archive_match".to_string())
        );
    }

    #[test]
    fn collect_archive_match_terms_keeps_cjk_windows() {
        let terms = collect_archive_match_terms("灯光自动化");
        assert!(terms.iter().any(|term| term == "灯光自动化"));
        assert!(terms.iter().any(|term| term == "灯光"));
        assert!(terms.iter().any(|term| term == "自动"));
    }

    #[test]
    fn get_roundtrips_transcript_hit() {
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
                        content: "先记录一下家庭网络重构方案".to_string(),
                    },
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "我已经整理了网络重构方案的关键节点".to_string(),
                    },
                ],
            );
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();

        let hit = search_archive_records(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            ArchiveSearchQuery {
                query: "网络 重构",
                preferred_chat_id: Some("chat-a"),
                chat_id_filter: Some("chat-a"),
                sources: &[ArchiveRecordSource::Transcript],
                limit: 2,
            },
        )
        .unwrap()
        .into_iter()
        .next()
        .unwrap();

        let record = get_archive_record(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            &hit.locator,
            Some("重构"),
            512,
        )
        .unwrap()
        .unwrap();

        assert_eq!(record.record_id, hit.record_id);
        assert!(record.content.contains("网络重构方案"));
        assert!(record.excerpt.contains("重构"));
    }

    #[test]
    fn transcript_locator_survives_recent_window_shift() {
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
                        content: "消息一".to_string(),
                    },
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "需要长期定位的消息".to_string(),
                    },
                    SessionMessage {
                        role: "user".to_string(),
                        content: "消息三".to_string(),
                    },
                ],
            );
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();

        let hit = search_archive_records(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            ArchiveSearchQuery {
                query: "长期 定位",
                preferred_chat_id: Some("chat-a"),
                chat_id_filter: Some("chat-a"),
                sources: &[ArchiveRecordSource::Transcript],
                limit: 2,
            },
        )
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.title.contains("ASSISTANT"))
        .expect("transcript hit");

        session_store
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "chat-a".to_string(),
                vec![
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "需要长期定位的消息".to_string(),
                    },
                    SessionMessage {
                        role: "user".to_string(),
                        content: "消息三".to_string(),
                    },
                    SessionMessage {
                        role: "assistant".to_string(),
                        content: "新消息".to_string(),
                    },
                ],
            );

        let record = get_archive_record(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            &hit.locator,
            Some("定位"),
            512,
        )
        .unwrap()
        .expect("record after window shift");

        assert_eq!(record.record_id, hit.record_id);
        assert!(record.content.contains("需要长期定位的消息"));
    }

    struct FailingListSessionStore;

    impl SessionStore for FailingListSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            unreachable!()
        }

        fn load_recent(&self, _chat_id: &str, _n: usize) -> Result<Vec<SessionMessage>> {
            Ok(Vec::new())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            unreachable!()
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Err(crate::error::Error::config(
                "archive_search_test",
                "session listing unavailable",
            ))
        }
    }

    #[test]
    fn search_archive_records_surfaces_session_listing_failures() {
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();

        let error = search_archive_records(
            &FailingListSessionStore,
            &memory_store,
            &turn_ledger_store,
            ArchiveSearchQuery {
                query: "anything",
                preferred_chat_id: None,
                chat_id_filter: None,
                sources: &[ArchiveRecordSource::Transcript],
                limit: 2,
            },
        )
        .expect_err("archive search should surface store failure");

        assert_eq!(error.stage(), "archive_search_chat_ids");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn archive_source_signature_changes_on_same_length_rewrite() {
        let root = std::env::temp_dir().join(format!(
            "archive_signature_rewrite_{}_{}",
            std::process::id(),
            crate::util::current_unix_ms()
        ));
        std::fs::create_dir_all(&root).expect("temp root");
        let path = root.join("same-len.txt");
        std::fs::write(&path, b"abc123").expect("seed file");
        let first = scan_path_signature(&root).expect("first signature");

        std::fs::write(&path, b"xyz789").expect("rewrite file");
        let second = scan_path_signature(&root).expect("second signature");

        assert_ne!(first.3, second.3);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn get_turn_log_checks_req_id() {
        let session_store = StubSessionStore::default();
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        turn_ledger_store
            .ledgers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                "chat-a".to_string(),
                TurnLedger {
                    req_id: "req-1".to_string(),
                    reason: "need_verify_wifi".to_string(),
                    user_preview: "帮我看看 WiFi 状态".to_string(),
                    reply_preview: "已开始检查".to_string(),
                    status: TurnLedgerStatus::Answered,
                    updated_at_ms: 1_775_101_149_000,
                    ..TurnLedger::default()
                },
            );

        let missing = get_archive_record(
            &session_store,
            &memory_store,
            &turn_ledger_store,
            &ArchiveRecordLocator {
                source: ArchiveRecordSource::TurnLog,
                chat_id: Some("chat-a".to_string()),
                message_id: None,
                message_index: None,
                note_name: None,
                req_id: Some("req-2".to_string()),
            },
            None,
            512,
        )
        .unwrap();

        assert!(missing.is_none());
    }

    #[test]
    fn render_turn_log_content_includes_observation_summary() {
        let content = render_turn_log_content(&TurnLedger {
            reason: "surface_finalization".to_string(),
            user_preview: "帮我继续查一下网络问题".to_string(),
            reply_preview: "我先给你恢复结论".to_string(),
            observation: Some(crate::memory::TurnObservationLedger {
                execution_class: crate::memory::TurnExecutionClass::ToolAssisted,
                deliberation_class: crate::memory::TurnDeliberationClass::HardReasoning,
                final_outcome: "surface_finalization".to_string(),
                pressure: crate::memory::TurnPersonaPressureLevel::Cautious,
                mode: crate::memory::TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: crate::memory::TurnToolPathLedger {
                    path: "surface_finalization".to_string(),
                    tool_calls: 2,
                    react_rounds: 2,
                    current_primary_delivered: false,
                },
                blocker: Some(crate::memory::TurnBlockerLedger {
                    kind: "retryable".to_string(),
                    failed_calls: 1,
                    total_calls: 1,
                }),
            }),
            ..TurnLedger::default()
        });

        assert!(content.contains("reason=surface_finalization"));
        assert!(content.contains("observation="));
        assert!(content.contains("Execution class: tool_assisted"));
        assert!(content.contains("Tool path: surface_finalization"));
        assert!(content.contains("Blocker: retryable 1/1"));
    }
}
