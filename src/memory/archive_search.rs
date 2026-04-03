//! Searchable archive sidecar over retained transcripts, daily notes, and turn logs.

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::{MemoryStore, SessionStore, TurnLedger, TurnLedgerStore, MAX_SESSION_ENTRIES};

pub const MAX_ARCHIVE_SEARCH_LIMIT: usize = 8;
pub const MAX_ARCHIVE_GET_CONTENT_LEN: usize = 4 * 1024;

const DEFAULT_ARCHIVE_GET_CONTENT_LEN: usize = 1800;
const ARCHIVE_SEARCH_EXCERPT_LEN: usize = 220;
const ARCHIVE_GET_EXCERPT_LEN: usize = 320;
const ARCHIVE_TRACE_MAX_MATCHED_TERMS: usize = 4;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveSearchBackendKind {
    #[default]
    Lexical,
    IndexedHybrid,
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

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "transcript" => Some(Self::Transcript),
            "daily_note" | "daily-note" | "daily note" => Some(Self::DailyNote),
            "turn_log" | "turn-log" | "turn log" => Some(Self::TurnLog),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveRecordLocator {
    pub source: ArchiveRecordSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
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
            ArchiveRecordSource::Transcript => format!(
                "transcript|{}|{}",
                self.chat_id.as_deref().unwrap_or_default(),
                self.message_index.unwrap_or_default()
            ),
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
            ArchiveRecordSource::Transcript => format!(
                "transcript:{}#message={}",
                self.chat_id.as_deref().unwrap_or("unknown"),
                self.message_index.unwrap_or_default()
            ),
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
                let message_index = parts.next()?.trim().parse::<usize>().ok()?;
                Some(Self {
                    source: ArchiveRecordSource::Transcript,
                    chat_id: Some(chat_id.to_string()),
                    message_index: Some(message_index),
                    note_name: None,
                    req_id: None,
                })
            }
            "daily_note" => {
                let note_name = parts.next()?.trim();
                Some(Self {
                    source: ArchiveRecordSource::DailyNote,
                    chat_id: None,
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
}

#[derive(Default)]
struct ArchiveCorpusStats {
    document_count: usize,
    avg_doc_len: f32,
    document_frequency: HashMap<String, usize>,
}

pub fn search_archive_records(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: ArchiveSearchQuery<'_>,
) -> Result<Vec<ArchiveSearchHit>> {
    let limit = query.limit.clamp(1, MAX_ARCHIVE_SEARCH_LIMIT);
    let terms = collect_archive_match_terms(query.query);
    let weak_query = query.query.trim().is_empty() || terms.is_empty();
    let candidates =
        collect_archive_candidates(session_store, memory_store, turn_ledger_store, query);
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let stats = build_archive_corpus_stats(&candidates, &terms);
    let newest_observed_at = candidates
        .iter()
        .filter_map(|candidate| candidate.observed_at)
        .max()
        .unwrap_or(0);
    let mut hits = score_archive_candidates(
        candidates,
        query,
        &terms,
        weak_query,
        &stats,
        newest_observed_at,
    );
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn collect_archive_candidates(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    query: ArchiveSearchQuery<'_>,
) -> Vec<ArchiveSearchCandidate> {
    let mut candidates = Vec::new();
    let source_filter = query.sources;
    let chat_ids =
        collect_archive_chat_ids(session_store, query.preferred_chat_id, query.chat_id_filter);

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::Transcript) {
        for chat_id in &chat_ids {
            let messages = session_store
                .load_recent(chat_id, MAX_SESSION_ENTRIES)
                .unwrap_or_default();
            for (index, message) in messages.iter().enumerate() {
                let content = message.content.trim();
                if content.is_empty() {
                    continue;
                }
                let title = format!("{} in {}", message.role.to_uppercase(), chat_id);
                let locator = ArchiveRecordLocator {
                    source: ArchiveRecordSource::Transcript,
                    chat_id: Some(chat_id.clone()),
                    message_index: Some(index),
                    note_name: None,
                    req_id: None,
                };
                let mut cues = vec!["recent transcript".to_string()];
                if query.preferred_chat_id == Some(chat_id.as_str()) {
                    cues.push("current chat".to_string());
                }
                candidates.push(ArchiveSearchCandidate {
                    normalized_title: normalize_archive_match_text(&title),
                    normalized_content: normalize_archive_match_text(content),
                    locator,
                    source: ArchiveRecordSource::Transcript,
                    title,
                    content: content.to_string(),
                    cues,
                    observed_at: None,
                    current_chat_match: query.preferred_chat_id == Some(chat_id.as_str()),
                });
            }
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::DailyNote) {
        for name in memory_store
            .list_daily_note_names(usize::MAX)
            .unwrap_or_default()
        {
            let Ok(content) = memory_store.get_daily_note(&name) else {
                continue;
            };
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
                message_index: None,
                note_name: Some(name.clone()),
                req_id: None,
            };
            candidates.push(ArchiveSearchCandidate {
                normalized_title: normalize_archive_match_text(&name),
                normalized_content: normalize_archive_match_text(content),
                locator,
                source: ArchiveRecordSource::DailyNote,
                title: name,
                content: content.to_string(),
                cues,
                observed_at,
                current_chat_match: false,
            });
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::TurnLog) {
        for chat_id in &chat_ids {
            let Ok(Some(ledger)) = turn_ledger_store.get(chat_id) else {
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
                message_index: None,
                note_name: None,
                req_id: Some(ledger.req_id.clone()),
            };
            let mut cues = vec!["execution log".to_string()];
            if query.preferred_chat_id == Some(chat_id.as_str()) {
                cues.push("current chat".to_string());
            }
            candidates.push(ArchiveSearchCandidate {
                normalized_title: normalize_archive_match_text(&title),
                normalized_content: normalize_archive_match_text(&content),
                locator,
                source: ArchiveRecordSource::TurnLog,
                title,
                content,
                cues,
                observed_at: turn_log_observed_at(&ledger),
                current_chat_match: query.preferred_chat_id == Some(chat_id.as_str()),
            });
        }
    }

    candidates
}

fn score_archive_candidates(
    candidates: Vec<ArchiveSearchCandidate>,
    query: ArchiveSearchQuery<'_>,
    terms: &[String],
    weak_query: bool,
    stats: &ArchiveCorpusStats,
    newest_observed_at: u64,
) -> Vec<ArchiveSearchHit> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            let (score, trace, matched_terms) =
                score_archive_candidate(&candidate, query, terms, stats, newest_observed_at);
            let substantive = trace.score.lexical_score > 0
                || trace.score.fts_score > 0
                || trace.score.hybrid_score > 0;
            if !weak_query && !substantive {
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
    query: ArchiveSearchQuery<'_>,
    terms: &[String],
    stats: &ArchiveCorpusStats,
    newest_observed_at: u64,
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
    let hybrid_score = archive_hybrid_score(candidate, query.query);
    let same_chat_bonus = if candidate.current_chat_match { 10 } else { 0 };
    let (source_bonus, source_reason) =
        archive_source_preference_bonus(candidate, query.sources, same_chat_bonus > 0);
    let (recency_bonus, recency_reason) =
        archive_recency_bonus(candidate.observed_at, newest_observed_at);
    let total_score = lexical_score
        .saturating_add(fts_score)
        .saturating_add(hybrid_score)
        .saturating_add(same_chat_bonus)
        .saturating_add(source_bonus)
        .saturating_add(recency_bonus);
    let trace = ArchiveRetrievalTrace {
        backend: archive_search_backend_kind(),
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
        selector_reason: None,
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
        total_len = total_len.saturating_add(
            candidate
                .normalized_content
                .split_whitespace()
                .count()
                .max(candidate.normalized_content.chars().count() / 4),
        );
        let mut seen = HashSet::new();
        let combined = format!(
            "{} {}",
            candidate.normalized_title, candidate.normalized_content
        );
        for term in terms {
            if combined.contains(term) && seen.insert(term.clone()) {
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
    if terms.is_empty() || stats.avg_doc_len <= 0.0 {
        return 0;
    }
    let combined = format!(
        "{} {}",
        candidate.normalized_title, candidate.normalized_content
    );
    let doc_len = combined
        .split_whitespace()
        .count()
        .max(combined.chars().count() / 4) as f32;
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
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let _ = candidate;
        let _ = query_text;
        0
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        let query = normalize_archive_match_text(query_text);
        if query.is_empty() {
            return 0;
        }
        let doc = format!(
            "{} {}",
            candidate.normalized_title, candidate.normalized_content
        );
        trigram_overlap_score(&query, &doc)
    }
}

fn trigram_overlap_score(left: &str, right: &str) -> u32 {
    let left = archive_trigrams(left);
    let right = archive_trigrams(right);
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let overlap = left
        .iter()
        .filter(|gram| right.iter().any(|candidate| candidate == *gram))
        .count();
    let ratio = overlap as f32 / left.len().max(right.len()) as f32;
    (ratio * 24.0).round().max(0.0) as u32
}

fn archive_trigrams(value: &str) -> Vec<String> {
    let compact: Vec<char> = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return Vec::new();
    }
    if compact.len() < 3 {
        return vec![compact.iter().collect()];
    }
    let mut grams = Vec::new();
    for slice in compact.windows(3) {
        let gram: String = slice.iter().collect();
        if !grams.iter().any(|existing| existing == &gram) {
            grams.push(gram);
        }
    }
    grams
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

fn archive_term_frequency(text: &str, term: &str) -> usize {
    if text.is_empty() || term.is_empty() {
        return 0;
    }
    text.match_indices(term).count()
}

fn archive_search_backend_kind() -> ArchiveSearchBackendKind {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        ArchiveSearchBackendKind::Lexical
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        ArchiveSearchBackendKind::IndexedHybrid
    }
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
            let Some(index) = locator.message_index else {
                return Ok(None);
            };
            let messages = session_store.load_recent(chat_id, MAX_SESSION_ENTRIES)?;
            let Some(message) = messages.get(index) else {
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
    let mut terms = Vec::new();
    for part in normalized.split_whitespace() {
        push_archive_term(&mut terms, part);
        if part.chars().all(is_cjk) {
            let chars: Vec<char> = part.chars().collect();
            for width in [2usize, 3usize] {
                if chars.len() < width {
                    continue;
                }
                for window in chars.windows(width) {
                    let candidate: String = window.iter().collect();
                    push_archive_term(&mut terms, &candidate);
                }
            }
        }
    }
    if terms.is_empty() {
        terms.push(normalized);
    } else if normalized.split_whitespace().count() > 1 {
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
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
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
) -> Vec<String> {
    if let Some(chat_id) = chat_id_filter
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return vec![chat_id.to_string()];
    }
    let mut chat_ids = session_store.list_chat_ids().unwrap_or_default();
    if let Some(preferred_chat_id) = preferred_chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        chat_ids.retain(|chat_id| chat_id != preferred_chat_id);
        chat_ids.insert(0, preferred_chat_id.to_string());
    }
    chat_ids
}

fn render_turn_log_content(ledger: &TurnLedger) -> String {
    [
        (!ledger.reason.trim().is_empty()).then(|| format!("reason={}", ledger.reason.trim())),
        (!ledger.user_preview.trim().is_empty())
            .then(|| format!("user={}", ledger.user_preview.trim())),
        (!ledger.reply_preview.trim().is_empty())
            .then(|| format!("reply={}", ledger.reply_preview.trim())),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
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

fn parse_daily_note_observed_at(name: &str) -> Option<u64> {
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

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
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

        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_soul(&self, _content: &str) -> Result<()> {
            unreachable!()
        }

        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_user(&self, _content: &str) -> Result<()> {
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
            Some(archive_search_backend_kind())
        );
        assert_eq!(
            ArchiveRecordLocator::parse_record_id(&hits[0].record_id),
            Some(hits[0].locator.clone())
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
}
