//! Searchable archive sidecar over retained transcripts, daily notes, and turn logs.

use crate::error::Result;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

use super::{MemoryStore, SessionStore, TurnLedger, TurnLedgerStore, MAX_SESSION_ENTRIES};

pub const MAX_ARCHIVE_SEARCH_LIMIT: usize = 8;
pub const MAX_ARCHIVE_GET_CONTENT_LEN: usize = 4 * 1024;

const DEFAULT_ARCHIVE_GET_CONTENT_LEN: usize = 1800;
const ARCHIVE_SEARCH_EXCERPT_LEN: usize = 220;
const ARCHIVE_GET_EXCERPT_LEN: usize = 320;

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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArchiveSearchQuery<'a> {
    pub query: &'a str,
    pub preferred_chat_id: Option<&'a str>,
    pub chat_id_filter: Option<&'a str>,
    pub sources: &'a [ArchiveRecordSource],
    pub limit: usize,
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
    let mut hits = Vec::new();
    let source_filter = query.sources;

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::Transcript) {
        for chat_id in
            collect_archive_chat_ids(session_store, query.preferred_chat_id, query.chat_id_filter)
        {
            let messages = session_store
                .load_recent(&chat_id, MAX_SESSION_ENTRIES)
                .unwrap_or_default();
            for (index, message) in messages.iter().enumerate() {
                let content = message.content.trim();
                if content.is_empty() {
                    continue;
                }
                let title = format!("{} in {}", message.role.to_uppercase(), chat_id);
                let match_score = archive_match_score(content, &title, &terms);
                if !weak_query && match_score == 0 {
                    continue;
                }
                let mut cues = vec!["recent transcript".to_string()];
                let mut score = 30u32
                    .saturating_add(match_score)
                    .saturating_add((index + 1) as u32);
                if query.preferred_chat_id == Some(chat_id.as_str()) {
                    score = score.saturating_add(10);
                    cues.push("current chat".to_string());
                }
                let locator = ArchiveRecordLocator {
                    source: ArchiveRecordSource::Transcript,
                    chat_id: Some(chat_id.clone()),
                    message_index: Some(index),
                    note_name: None,
                    req_id: None,
                };
                hits.push(ArchiveSearchHit {
                    record_id: locator.record_id(),
                    citation: locator.citation(),
                    locator,
                    source: ArchiveRecordSource::Transcript,
                    title,
                    excerpt: pick_archive_excerpt(content, &terms, ARCHIVE_SEARCH_EXCERPT_LEN),
                    score,
                    cues,
                    observed_at: None,
                });
            }
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::DailyNote) {
        for (order, name) in memory_store
            .list_daily_note_names(usize::MAX)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let Ok(content) = memory_store.get_daily_note(&name) else {
                continue;
            };
            let content = content.trim();
            if content.is_empty() {
                continue;
            }
            let match_score = archive_match_score(content, &name, &terms);
            if !weak_query && match_score == 0 {
                continue;
            }
            let mut cues = vec!["daily archive context".to_string()];
            let recency_bonus = 24u32.saturating_sub(order as u32).max(1);
            let observed_at = parse_daily_note_observed_at(&name);
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
            hits.push(ArchiveSearchHit {
                record_id: locator.record_id(),
                citation: locator.citation(),
                locator,
                source: ArchiveRecordSource::DailyNote,
                title: name,
                excerpt: pick_archive_excerpt(content, &terms, ARCHIVE_SEARCH_EXCERPT_LEN),
                score: 24u32
                    .saturating_add(match_score)
                    .saturating_add(recency_bonus),
                cues,
                observed_at,
            });
        }
    }

    if source_filter.is_empty() || source_filter.contains(&ArchiveRecordSource::TurnLog) {
        for chat_id in
            collect_archive_chat_ids(session_store, query.preferred_chat_id, query.chat_id_filter)
        {
            let Ok(Some(ledger)) = turn_ledger_store.get(&chat_id) else {
                continue;
            };
            let content = render_turn_log_content(&ledger);
            if content.is_empty() {
                continue;
            }
            let title = format!("{} turn in {}", ledger.status.label(), chat_id);
            let match_score = archive_match_score(&content, &title, &terms);
            if !weak_query && match_score == 0 {
                continue;
            }
            let mut cues = vec!["execution log".to_string()];
            let mut score = 18u32.saturating_add(match_score);
            if query.preferred_chat_id == Some(chat_id.as_str()) {
                score = score.saturating_add(8);
                cues.push("current chat".to_string());
            }
            let locator = ArchiveRecordLocator {
                source: ArchiveRecordSource::TurnLog,
                chat_id: Some(chat_id.clone()),
                message_index: None,
                note_name: None,
                req_id: Some(ledger.req_id.clone()),
            };
            hits.push(ArchiveSearchHit {
                record_id: locator.record_id(),
                citation: locator.citation(),
                locator,
                source: ArchiveRecordSource::TurnLog,
                title,
                excerpt: pick_archive_excerpt(&content, &terms, ARCHIVE_SEARCH_EXCERPT_LEN),
                score,
                cues,
                observed_at: turn_log_observed_at(&ledger),
            });
        }
    }

    apply_archive_recency_bonus(&mut hits);
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.observed_at.cmp(&a.observed_at))
            .then_with(|| a.citation.cmp(&b.citation))
    });
    hits.truncate(limit);
    Ok(hits)
}

fn apply_archive_recency_bonus(hits: &mut [ArchiveSearchHit]) {
    let newest = hits
        .iter()
        .filter_map(|hit| hit.observed_at)
        .max()
        .unwrap_or(0);
    if newest == 0 {
        return;
    }
    for hit in hits {
        let Some(observed_at) = hit.observed_at else {
            continue;
        };
        let age = newest.saturating_sub(observed_at);
        let bonus = if age <= 86_400 {
            6
        } else if age <= 7 * 86_400 {
            4
        } else if age <= 30 * 86_400 {
            2
        } else {
            0
        };
        hit.score = hit.score.saturating_add(bonus);
        if bonus > 0 {
            hit.cues.push(format!("recent+{}", bonus));
        }
    }
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
        if part.chars().count() < 2 || terms.iter().any(|item| item == part) {
            continue;
        }
        terms.push(part.to_string());
    }
    if terms.is_empty() {
        terms.push(normalized);
    }
    terms
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

pub(crate) fn archive_match_score(content: &str, title: &str, terms: &[String]) -> u32 {
    if terms.is_empty() {
        return 1;
    }
    let normalized_content = normalize_archive_match_text(content);
    let normalized_title = normalize_archive_match_text(title);
    let mut score = 0u32;
    for term in terms {
        if normalized_title.contains(term) {
            score = score.saturating_add(4);
        }
        if normalized_content.contains(term) {
            score = score.saturating_add(3);
        }
    }
    score
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
            ArchiveRecordLocator::parse_record_id(&hits[0].record_id),
            Some(hits[0].locator.clone())
        );
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
