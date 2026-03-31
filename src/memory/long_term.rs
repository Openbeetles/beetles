//! 结构化长期记忆抽象与轻量召回辅助。
//! Structured long-term memory abstractions and lightweight recall helpers.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 结构化长期记忆存储路径（相对状态根）。
pub const REL_PATH_LONG_TERM_MEMORIES: &str = "memory/long_term_memories.json";
/// 长期记忆条目上限；两端平台先共用同一预算，后续可按实现单独扩展。
pub const MAX_LONG_TERM_MEMORY_ITEMS: usize = 96;
/// 单条记忆内容字节上限。
pub const MAX_LONG_TERM_MEMORY_CONTENT_LEN: usize = 240;
/// 单条记忆关键词个数上限。
pub const MAX_LONG_TERM_MEMORY_KEYWORDS: usize = 8;
/// 单个关键词字节上限。
pub const MAX_LONG_TERM_MEMORY_KEYWORD_LEN: usize = 24;
/// 单个主题槽位字节上限。
pub const MAX_LONG_TERM_MEMORY_TOPIC_LEN: usize = 40;
/// 单次召回默认条数上限。
pub const DEFAULT_LONG_TERM_MEMORY_RECALL_LIMIT: usize = 4;
/// 注入 prompt 的长期记忆块上限。
pub const MAX_LONG_TERM_MEMORY_BLOCK_LEN: usize = 1024;

/// 长期记忆类别。只保留当前 beetle 真实会用到的 durable 类型。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryKind {
    Preference,
    Profile,
    Relationship,
    Project,
    Task,
    Constraint,
    Fact,
}

impl LongTermMemoryKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Profile => "profile",
            Self::Relationship => "relationship",
            Self::Project => "project",
            Self::Task => "task",
            Self::Constraint => "constraint",
            Self::Fact => "fact",
        }
    }
}

/// 持久化后的长期记忆条目。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemoryEntry {
    pub id: String,
    pub kind: LongTermMemoryKind,
    #[serde(default)]
    pub topic: String,
    pub content: String,
    pub keywords: Vec<String>,
    pub source_chat_id: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

/// 待写入的长期记忆草稿。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemoryDraft {
    pub kind: LongTermMemoryKind,
    pub topic: String,
    pub content: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub source_chat_id: Option<String>,
}

impl LongTermMemoryDraft {
    /// 规范化草稿：裁剪长度、去重关键词、忽略空内容。
    pub fn normalized(&self) -> Option<Self> {
        let topic = normalize_topic(self.topic.trim());
        if topic.is_empty() {
            return None;
        }
        let content = truncate_utf8_bytes(self.content.trim(), MAX_LONG_TERM_MEMORY_CONTENT_LEN);
        if content.is_empty() {
            return None;
        }
        let mut keywords =
            Vec::with_capacity(self.keywords.len().min(MAX_LONG_TERM_MEMORY_KEYWORDS));
        for raw in &self.keywords {
            let normalized = truncate_utf8_bytes(
                raw.trim().to_lowercase().as_str(),
                MAX_LONG_TERM_MEMORY_KEYWORD_LEN,
            );
            if normalized.len() < 2 || keywords.iter().any(|item| item == &normalized) {
                continue;
            }
            keywords.push(normalized);
            if keywords.len() >= MAX_LONG_TERM_MEMORY_KEYWORDS {
                break;
            }
        }
        Some(Self {
            kind: self.kind.clone(),
            topic,
            content,
            keywords,
            source_chat_id: self
                .source_chat_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        })
    }

    pub fn stable_id(&self) -> Option<String> {
        let normalized = self.normalized()?;
        let mut hasher = DefaultHasher::new();
        normalized.kind.hash(&mut hasher);
        0x517c_c1b7_u32.hash(&mut hasher);
        normalized.topic.hash(&mut hasher);
        let mut id = String::with_capacity(20);
        id.push_str("ltm-");
        id.push_str(&format!("{:016x}", hasher.finish()));
        Some(id)
    }
}

/// 结构化长期记忆存储接口。实现负责持久化、去重与轻量召回。
pub trait LongTermMemoryStore: Send + Sync {
    fn upsert_many(&self, drafts: &[LongTermMemoryDraft], now_secs: u64) -> Result<usize>;
    fn recall(
        &self,
        query: &str,
        source_chat_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LongTermMemoryEntry>>;
    fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>>;
    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>>;
    fn delete(&self, id: &str) -> Result<bool>;
    fn count(&self) -> Result<usize>;
}

pub fn canonicalize_long_term_memory_entry(
    mut entry: LongTermMemoryEntry,
) -> Option<LongTermMemoryEntry> {
    let topic = {
        let normalized = normalize_topic(&entry.topic);
        if !normalized.is_empty() {
            normalized
        } else {
            let fallback = entry
                .keywords
                .first()
                .map(|keyword| normalize_topic(keyword))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| normalize_topic(&entry.content));
            if fallback.is_empty() {
                return None;
            }
            fallback
        }
    };
    if topic.is_empty() {
        return None;
    }
    entry.topic = topic;
    entry.content = truncate_utf8_bytes(entry.content.trim(), MAX_LONG_TERM_MEMORY_CONTENT_LEN);
    if entry.content.is_empty() {
        return None;
    }
    let mut keywords = Vec::with_capacity(entry.keywords.len().min(MAX_LONG_TERM_MEMORY_KEYWORDS));
    for keyword in entry.keywords {
        let normalized = truncate_utf8_bytes(
            keyword.trim().to_lowercase().as_str(),
            MAX_LONG_TERM_MEMORY_KEYWORD_LEN,
        );
        if normalized.len() < 2 || keywords.iter().any(|item| item == &normalized) {
            continue;
        }
        keywords.push(normalized);
        if keywords.len() >= MAX_LONG_TERM_MEMORY_KEYWORDS {
            break;
        }
    }
    entry.keywords = keywords;
    if entry.updated_at == 0 {
        entry.updated_at = entry.created_at;
    }
    Some(entry)
}

pub fn merge_long_term_memory_entry(
    existing: &mut LongTermMemoryEntry,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> bool {
    let Some(normalized) = draft.normalized() else {
        return false;
    };
    let mut changed = false;
    let mut merged_keywords = normalized.keywords.clone();
    for keyword in &existing.keywords {
        if merged_keywords.iter().any(|item| item == keyword) {
            continue;
        }
        merged_keywords.push(keyword.clone());
    }
    merged_keywords.truncate(MAX_LONG_TERM_MEMORY_KEYWORDS);

    if existing.content != normalized.content {
        existing.content = normalized.content;
        changed = true;
    }
    if existing.keywords != merged_keywords {
        existing.keywords = merged_keywords;
        changed = true;
    }
    if let Some(source_chat_id) = normalized.source_chat_id {
        if existing.source_chat_id.as_deref() != Some(source_chat_id.as_str()) {
            existing.source_chat_id = Some(source_chat_id);
            changed = true;
        }
    }
    if existing.updated_at != now_secs {
        existing.updated_at = now_secs;
        changed = true;
    }
    changed
}

/// 渲染注入 prompt 的长期记忆块。
pub fn render_long_term_memory_block(
    entries: &[LongTermMemoryEntry],
    max_len: usize,
) -> Option<String> {
    if entries.is_empty() || max_len < 32 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(MAX_LONG_TERM_MEMORY_BLOCK_LEN));
    out.push_str("## Long-term memory\n");
    for entry in entries {
        let line = if entry.keywords.is_empty() {
            format!(
                "- [{}:{}] {}",
                entry.kind.label(),
                entry.topic,
                entry.content
            )
        } else {
            format!(
                "- [{}:{}] {} (keywords: {})",
                entry.kind.label(),
                entry.topic,
                entry.content,
                entry.keywords.join(", ")
            )
        };
        let next_len = if out.is_empty() {
            line.len()
        } else {
            out.len().saturating_add(1).saturating_add(line.len())
        };
        if next_len > max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    if out.trim() == "## Long-term memory" {
        None
    } else {
        Some(out.trim_end().to_string())
    }
}

pub(crate) fn score_long_term_memory_recall(
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    entry: &LongTermMemoryEntry,
) -> u32 {
    let normalized_query = normalize_for_match(query);
    if normalized_query.len() < 2 {
        return 0;
    }

    let mut score = 0u32;
    let normalized_content = normalize_for_match(&entry.content);
    let normalized_topic = normalize_for_match(&entry.topic);
    if normalized_content.contains(&normalized_query) {
        score = score.saturating_add(8);
    }
    if normalized_topic.contains(&normalized_query) {
        score = score.saturating_add(10);
    }

    let terms = collect_match_terms(query);
    for term in terms {
        if normalized_topic.contains(&term) {
            score = score.saturating_add(4);
        }
        if normalized_content.contains(&term) {
            score = score.saturating_add(2);
        }
        for keyword in &entry.keywords {
            let normalized_keyword = normalize_for_match(keyword);
            if normalized_keyword.contains(&term) || term.contains(&normalized_keyword) {
                score = score.saturating_add(3);
            }
        }
    }

    if score == 0 {
        return 0;
    }

    if source_chat_id.is_some_and(|chat_id| entry.source_chat_id.as_deref() == Some(chat_id)) {
        score = score.saturating_add(recall_chat_affinity_bonus(&entry.kind));
    }
    score.saturating_add(recall_recency_bonus(now_secs, entry.updated_at))
}

fn truncate_utf8_bytes(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].trim().to_string()
}

fn normalize_topic(input: &str) -> String {
    let mut out = String::with_capacity(input.len().min(MAX_LONG_TERM_MEMORY_TOPIC_LEN));
    let mut prev_sep = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() || is_cjk(ch) {
            for lower in ch.to_lowercase() {
                if out.len().saturating_add(lower.len_utf8()) > MAX_LONG_TERM_MEMORY_TOPIC_LEN {
                    break;
                }
                out.push(lower);
            }
            prev_sep = false;
        } else if !prev_sep && !out.is_empty() {
            out.push('_');
            prev_sep = true;
        }
        if out.len() >= MAX_LONG_TERM_MEMORY_TOPIC_LEN {
            break;
        }
    }
    out.trim_matches('_').to_string()
}

fn normalize_for_match(input: &str) -> String {
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

fn collect_match_terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_has_cjk = false;

    let flush_run = |out: &mut Vec<String>, run: &mut String, has_cjk: &mut bool| -> () {
        if run.is_empty() {
            return;
        }
        let term = run.clone();
        if *has_cjk {
            push_unique_term(out, &term);
            let chars: Vec<char> = term.chars().collect();
            for window in [2usize, 3usize] {
                if chars.len() < window {
                    continue;
                }
                for slice in chars.windows(window) {
                    let candidate: String = slice.iter().collect();
                    push_unique_term(out, &candidate);
                    if out.len() >= 24 {
                        break;
                    }
                }
            }
        } else {
            push_unique_term(out, &term);
        }
        run.clear();
        *has_cjk = false;
    };

    for ch in normalize_for_match(query).chars() {
        if ch.is_ascii_alphanumeric() {
            current.push(ch);
        } else if is_cjk(ch) {
            current.push(ch);
            current_has_cjk = true;
        } else {
            flush_run(&mut out, &mut current, &mut current_has_cjk);
            if out.len() >= 24 {
                break;
            }
        }
    }
    flush_run(&mut out, &mut current, &mut current_has_cjk);
    out
}

fn push_unique_term(out: &mut Vec<String>, term: &str) {
    let term = term.trim();
    if term.len() < 2 || out.iter().any(|item| item == term) {
        return;
    }
    out.push(term.to_string());
}

fn recall_chat_affinity_bonus(kind: &LongTermMemoryKind) -> u32 {
    match kind {
        LongTermMemoryKind::Task | LongTermMemoryKind::Project => 4,
        LongTermMemoryKind::Constraint | LongTermMemoryKind::Preference => 3,
        LongTermMemoryKind::Profile | LongTermMemoryKind::Relationship => 2,
        LongTermMemoryKind::Fact => 1,
    }
}

fn recall_recency_bonus(now_secs: u64, updated_at: u64) -> u32 {
    if now_secs == 0 || updated_at == 0 || updated_at > now_secs {
        return 0;
    }
    match now_secs - updated_at {
        0..=86_400 => 4,
        86_401..=604_800 => 3,
        604_801..=2_592_000 => 2,
        2_592_001..=7_776_000 => 1,
        _ => 0,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_long_term_memory_draft() {
        let draft = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Preference,
            topic: "response_style".to_string(),
            content: "  User prefers concise answers.  ".to_string(),
            keywords: vec![
                " concise ".to_string(),
                "STYLE".to_string(),
                "STYLE".to_string(),
            ],
            source_chat_id: Some(" chat ".to_string()),
        };

        let normalized = draft.normalized().unwrap();
        assert_eq!(normalized.topic, "response_style");
        assert_eq!(normalized.content, "User prefers concise answers.");
        assert_eq!(normalized.keywords, vec!["concise", "style"]);
        assert_eq!(normalized.source_chat_id.as_deref(), Some("chat"));
    }

    #[test]
    fn stable_id_is_deterministic() {
        let draft = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Project,
            topic: "current_project".to_string(),
            content: "We are building Beetle on ESP and Linux".to_string(),
            keywords: vec!["beetle".to_string()],
            source_chat_id: None,
        };

        assert_eq!(draft.stable_id(), draft.stable_id());
    }

    #[test]
    fn recall_score_matches_cjk_terms() {
        let entry = LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Project,
            topic: "长期记忆设计".to_string(),
            content: "当前项目重点是长期记忆与 Linux 体验".to_string(),
            keywords: vec!["长期记忆".to_string(), "linux".to_string()],
            source_chat_id: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(score_long_term_memory_recall("记忆这块怎么设计", None, 0, &entry) > 0);
    }

    #[test]
    fn renders_long_term_memory_block() {
        let block = render_long_term_memory_block(
            &[LongTermMemoryEntry {
                id: "ltm-1".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "response_style".to_string(),
                content: "User prefers direct technical answers.".to_string(),
                keywords: vec!["direct".to_string(), "technical".to_string()],
                source_chat_id: None,
                created_at: 0,
                updated_at: 0,
            }],
            256,
        )
        .unwrap();

        assert!(block.contains("Long-term memory"));
        assert!(block.contains("preference:response_style"));
    }

    #[test]
    fn canonicalize_long_term_memory_entry_fills_updated_at() {
        let entry = canonicalize_long_term_memory_entry(LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Profile,
            topic: " user name ".to_string(),
            content: "甲壳虫".to_string(),
            keywords: vec!["名字".to_string()],
            source_chat_id: None,
            created_at: 42,
            updated_at: 0,
        })
        .unwrap();

        assert_eq!(entry.topic, "user_name");
        assert_eq!(entry.updated_at, 42);
    }

    #[test]
    fn canonicalize_long_term_memory_entry_derives_topic_for_legacy_entries() {
        let entry = canonicalize_long_term_memory_entry(LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Fact,
            topic: String::new(),
            content: "User lives in Shenzhen".to_string(),
            keywords: vec!["location".to_string()],
            source_chat_id: None,
            created_at: 1,
            updated_at: 0,
        })
        .unwrap();

        assert_eq!(entry.topic, "location");
    }

    #[test]
    fn stable_id_uses_topic_so_same_slot_can_update() {
        let a = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Preference,
            topic: "response_style".to_string(),
            content: "User prefers concise answers.".to_string(),
            keywords: vec!["concise".to_string()],
            source_chat_id: None,
        };
        let b = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Preference,
            topic: "response_style".to_string(),
            content: "User now prefers detailed answers.".to_string(),
            keywords: vec!["detailed".to_string()],
            source_chat_id: None,
        };

        assert_eq!(a.stable_id(), b.stable_id());
    }

    #[test]
    fn merge_long_term_memory_entry_overwrites_same_slot_content() {
        let mut entry = LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Preference,
            topic: "response_style".to_string(),
            content: "User prefers concise answers.".to_string(),
            keywords: vec!["concise".to_string()],
            source_chat_id: Some("chat-a".to_string()),
            created_at: 10,
            updated_at: 10,
        };
        let draft = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Preference,
            topic: "response_style".to_string(),
            content: "User now prefers detailed answers.".to_string(),
            keywords: vec!["detailed".to_string()],
            source_chat_id: Some("chat-b".to_string()),
        };

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.content, "User now prefers detailed answers.");
        assert_eq!(entry.keywords, vec!["detailed", "concise"]);
        assert_eq!(entry.source_chat_id.as_deref(), Some("chat-b"));
        assert_eq!(entry.created_at, 10);
        assert_eq!(entry.updated_at, 20);
    }

    #[test]
    fn merge_long_term_memory_entry_preserves_source_chat_when_draft_has_none() {
        let mut entry = LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Project,
            topic: "current_project".to_string(),
            content: "Current project is Beetle memory.".to_string(),
            keywords: vec!["beetle".to_string()],
            source_chat_id: Some("chat-a".to_string()),
            created_at: 10,
            updated_at: 10,
        };
        let draft = LongTermMemoryDraft {
            kind: LongTermMemoryKind::Project,
            topic: "current_project".to_string(),
            content: "Current project is Beetle runtime.".to_string(),
            keywords: vec!["runtime".to_string()],
            source_chat_id: None,
        };

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.source_chat_id.as_deref(), Some("chat-a"));
    }

    #[test]
    fn recall_score_prefers_same_chat_and_recent_updates_after_match() {
        let base = LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Project,
            topic: "current_project".to_string(),
            content: "Current project is Beetle long-term memory.".to_string(),
            keywords: vec!["beetle".to_string(), "memory".to_string()],
            source_chat_id: Some("chat-a".to_string()),
            created_at: 10,
            updated_at: 90,
        };
        let mut older = base.clone();
        older.source_chat_id = Some("chat-b".to_string());
        older.updated_at = 10;

        let preferred = score_long_term_memory_recall("memory project", Some("chat-a"), 100, &base);
        let fallback = score_long_term_memory_recall("memory project", Some("chat-a"), 100, &older);
        assert!(preferred > fallback);
    }
}
