//! 结构化长期记忆抽象与轻量召回辅助。
//! Structured long-term memory abstractions and lightweight recall helpers.

use crate::error::Result;
use crate::util::{
    collect_retrieval_terms, is_cjk, normalize_retrieval_text, trigram_overlap_score,
};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use super::{
    memory_policy, shared_long_term_governance_policy, LongTermRecallPolicy, MemoryProfile,
    SessionMessage,
};

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
/// 单条记忆保留的支持性引用上限。
pub const MAX_LONG_TERM_MEMORY_SUPPORTING_CITATIONS: usize = 6;
/// 单条支持性引用字节上限。
pub const MAX_LONG_TERM_MEMORY_CITATION_LEN: usize = 96;
/// 注入 prompt 的长期记忆块上限。
pub const MAX_LONG_TERM_MEMORY_BLOCK_LEN: usize = 1024;
/// 长期记忆治理：任务超时后视为陈旧。
const LONG_TERM_MEMORY_TASK_TTL_SECS: u64 = 45 * 86_400;
/// 长期记忆治理：项目超时后视为陈旧。
const LONG_TERM_MEMORY_PROJECT_TTL_SECS: u64 = 180 * 86_400;

impl LongTermRecallPolicy {
    fn recall_block_max_len(self, system_max_len: usize) -> usize {
        let mut block_max_len = (system_max_len / 4).min(self.block_max_len_cap);
        if block_max_len < self.block_min_len {
            block_max_len = system_max_len.min(self.block_max_len_cap);
        }
        block_max_len
    }

    fn desired_entry_count(self, block_max_len: usize) -> usize {
        match block_max_len {
            0..=255 => 2,
            256..=511 => 3,
            512..=767 => 4,
            _ => 5,
        }
    }

    fn direct_recall_limit(self, desired: usize) -> usize {
        desired.saturating_mul(self.direct_recall_multiplier)
    }

    fn fallback_list_limit(self, desired: usize) -> usize {
        desired.saturating_mul(self.fallback_list_multiplier)
    }

    fn build_recall_query(
        self,
        user_query: &str,
        summary_text: Option<&str>,
        recent_messages: &[SessionMessage],
    ) -> String {
        let trimmed = user_query.trim();
        let summary = summary_text
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_utf8_bytes(value, self.summary_grounding_max_len));
        let recent_grounding = build_recent_recall_grounding(
            recent_messages,
            self.recent_grounding_message_count,
            self.recent_grounding_max_len,
        );
        if !self.is_weak_query(trimmed) {
            return trimmed.to_string();
        }
        let mut parts = Vec::with_capacity(3);
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
        if !recent_grounding.is_empty() {
            parts.push(recent_grounding);
        }
        if let Some(summary) = summary {
            parts.push(summary);
        }
        parts.join("\n\n")
    }

    fn is_weak_query(self, query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return true;
        }
        let chars = trimmed.chars().count();
        if chars <= self.weak_query_short_chars {
            return true;
        }
        chars <= self.weak_query_max_chars
            && trimmed.split_whitespace().count() <= self.weak_query_max_words
    }

    fn compare_fallback_entries(
        self,
        chat_id: &str,
        a: &LongTermMemoryEntry,
        b: &LongTermMemoryEntry,
    ) -> Ordering {
        self.fallback_entry_priority(chat_id, b)
            .cmp(&self.fallback_entry_priority(chat_id, a))
            .then_with(|| entry_observed_at(b).cmp(&entry_observed_at(a)))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| b.created_at.cmp(&a.created_at))
    }

    fn fallback_entry_priority(self, chat_id: &str, entry: &LongTermMemoryEntry) -> (u8, u8) {
        let same_chat = u8::from(entry.source_chat_id.as_deref() == Some(chat_id));
        let kind_priority = match entry.kind {
            LongTermMemoryKind::Task => 6,
            LongTermMemoryKind::Project => 5,
            LongTermMemoryKind::Constraint => 4,
            LongTermMemoryKind::Preference => 3,
            LongTermMemoryKind::Profile => 2,
            LongTermMemoryKind::Relationship => 1,
            LongTermMemoryKind::Fact => 0,
        };
        (same_chat, kind_priority)
    }

    fn select_entries(
        self,
        candidates: Vec<LongTermMemoryEntry>,
        desired: usize,
    ) -> Vec<LongTermMemoryEntry> {
        let mut selected = Vec::with_capacity(candidates.len().min(desired));
        let mut seen_ids = HashSet::with_capacity(candidates.len());
        let mut seen_topics = HashSet::with_capacity(candidates.len());
        let mut seen_kinds = HashSet::with_capacity(candidates.len());

        for pass in 0..3 {
            for entry in &candidates {
                if selected.len() >= desired || !seen_ids.insert(entry.id.clone()) {
                    continue;
                }
                let topic_key = format!("{}:{}", entry.kind.label(), entry.topic);
                let kind_key = entry.kind.label();
                let allow = match pass {
                    0 => !seen_topics.contains(&topic_key) && !seen_kinds.contains(kind_key),
                    1 => !seen_topics.contains(&topic_key),
                    _ => true,
                };
                if !allow {
                    seen_ids.remove(&entry.id);
                    continue;
                }
                seen_topics.insert(topic_key);
                seen_kinds.insert(kind_key.to_string());
                selected.push(entry.clone());
                if selected.len() >= desired {
                    break;
                }
            }
        }
        selected
    }

    fn kind_budget(self, kind: &LongTermMemoryKind) -> usize {
        match kind {
            LongTermMemoryKind::Preference => 18,
            LongTermMemoryKind::Profile => 12,
            LongTermMemoryKind::Relationship => 8,
            LongTermMemoryKind::Project => 16,
            LongTermMemoryKind::Task => 12,
            LongTermMemoryKind::Constraint => 12,
            LongTermMemoryKind::Fact => 18,
        }
    }

    fn is_stale(self, entry: &LongTermMemoryEntry, now_secs: u64) -> bool {
        let observed_at = entry_observed_at(entry);
        if now_secs == 0 || observed_at == 0 || observed_at > now_secs {
            return false;
        }
        let age_secs = now_secs - observed_at;
        (match entry.kind {
            LongTermMemoryKind::Task => age_secs > LONG_TERM_MEMORY_TASK_TTL_SECS,
            LongTermMemoryKind::Project => age_secs > LONG_TERM_MEMORY_PROJECT_TTL_SECS,
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Constraint
            | LongTermMemoryKind::Fact => false,
        }) || matches!(entry.freshness, LongTermMemoryFreshness::Volatile)
            && age_secs > LONG_TERM_MEMORY_PROJECT_TTL_SECS
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LongTermRecallSelection {
    pub recall_query: String,
    pub candidates: Vec<LongTermMemoryEntry>,
    pub selected: Vec<LongTermMemoryEntry>,
    pub desired: usize,
    pub used_fallback: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LongTermRecallScoreBreakdown {
    pub lexical_score: u32,
    pub semantic_score: u32,
    pub exact_match_score: u32,
    pub keyword_score: u32,
    pub recency_score: u32,
    pub last_used_score: u32,
    pub confidence_score: u32,
    pub scope_affinity_score: u32,
    pub governance_score: u32,
    pub total_score: u32,
    pub reason_fragments: Vec<String>,
}

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

/// 当前记忆内容的置信度。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryConfidence {
    Low,
    #[default]
    Medium,
    High,
}

impl LongTermMemoryConfidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "confidence=low",
            Self::Medium => "confidence=medium",
            Self::High => "confidence=high",
        }
    }

    fn recall_bonus(self) -> u32 {
        match self {
            Self::Low => 0,
            Self::Medium => 2,
            Self::High => 4,
        }
    }
}

/// 记忆来源类型：当前内容最近一次由什么来源形成。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemorySourceType {
    #[default]
    Conversation,
    ManualTool,
    SystemRuntime,
    ExternalObservation,
}

impl LongTermMemorySourceType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::ManualTool => "manual tool",
            Self::SystemRuntime => "system runtime",
            Self::ExternalObservation => "external observation",
        }
    }
}

/// 记忆适用范围：这条记录更应该被视为当前 chat、跨 chat 用户画像，还是外部世界事实。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemorySourceScope {
    Chat,
    #[default]
    User,
    World,
}

impl LongTermMemorySourceScope {
    pub fn label(self) -> &'static str {
        match self {
            Self::Chat => "scope=chat",
            Self::User => "scope=user",
            Self::World => "scope=world",
        }
    }
}

/// 记忆的新鲜度类别：决定召回时是否更应提示复核。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryFreshness {
    #[default]
    Stable,
    Dynamic,
    Volatile,
}

impl LongTermMemoryFreshness {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dynamic => "dynamic",
            Self::Volatile => "volatile",
        }
    }

    fn aging_after_secs(self) -> u64 {
        match self {
            Self::Stable => 90 * 86_400,
            Self::Dynamic => 14 * 86_400,
            Self::Volatile => 3 * 86_400,
        }
    }

    fn stale_after_secs(self) -> u64 {
        match self {
            Self::Stable => 365 * 86_400,
            Self::Dynamic => 90 * 86_400,
            Self::Volatile => 21 * 86_400,
        }
    }
}

/// 召回侧对模型的 stale 提示倾向。
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryStaleHint {
    #[default]
    None,
    ReviewBeforeUse,
    VerifyAgainstCurrentState,
}

impl LongTermMemoryStaleHint {
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::ReviewBeforeUse => Some("review"),
            Self::VerifyAgainstCurrentState => Some("verify current"),
        }
    }
}

/// 召回给主模型的主证据态标签。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LongTermMemoryEvidenceState {
    StableFact,
    RecentState,
    PossiblyStale,
    NeedsReview,
}

impl LongTermMemoryEvidenceState {
    pub fn label(self) -> &'static str {
        match self {
            Self::StableFact => "stable fact",
            Self::RecentState => "recent state",
            Self::PossiblyStale => "possibly stale",
            Self::NeedsReview => "needs review",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemoryEvidenceSummary {
    pub state: LongTermMemoryEvidenceState,
    pub confidence: LongTermMemoryConfidence,
    pub freshness: LongTermMemoryFreshness,
    pub stale_hint: LongTermMemoryStaleHint,
    pub source_type: LongTermMemorySourceType,
    pub source_scope: LongTermMemorySourceScope,
    pub evidence_count: u32,
    pub supporting_citations: Vec<String>,
    pub last_confirmed_at: u64,
    pub last_used_at: u64,
    pub age_summary: Option<String>,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LongTermMemoryAgeState {
    Current,
    Aging,
    Stale,
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
    #[serde(default)]
    pub source_type: LongTermMemorySourceType,
    #[serde(default)]
    pub source_scope: LongTermMemorySourceScope,
    #[serde(default)]
    pub confidence: LongTermMemoryConfidence,
    #[serde(default)]
    pub freshness: LongTermMemoryFreshness,
    #[serde(default)]
    pub stale_hint: LongTermMemoryStaleHint,
    #[serde(default)]
    pub supporting_citations: Vec<String>,
    #[serde(default)]
    pub evidence_count: u32,
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub observed_at: u64,
    #[serde(default)]
    pub last_confirmed_at: u64,
    #[serde(default)]
    pub source_revision: u64,
    #[serde(default)]
    pub last_used_at: u64,
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
    #[serde(default)]
    pub source_type: Option<LongTermMemorySourceType>,
    #[serde(default)]
    pub source_scope: Option<LongTermMemorySourceScope>,
    #[serde(default)]
    pub confidence: Option<LongTermMemoryConfidence>,
    #[serde(default)]
    pub freshness: Option<LongTermMemoryFreshness>,
    #[serde(default)]
    pub stale_hint: Option<LongTermMemoryStaleHint>,
    #[serde(default)]
    pub supporting_citations: Vec<String>,
    #[serde(default)]
    pub evidence_count: Option<u32>,
    #[serde(default)]
    pub observed_at: Option<u64>,
    #[serde(default)]
    pub last_confirmed_at: Option<u64>,
    #[serde(default)]
    pub source_revision: Option<u64>,
}

/// 长期记忆槽位键，用于更新/删除同一条结构化记忆。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemorySlot {
    pub kind: LongTermMemoryKind,
    pub topic: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemoryQuery {
    #[serde(default)]
    pub kind: Option<LongTermMemoryKind>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub source_scope: Option<LongTermMemorySourceScope>,
    #[serde(default)]
    pub source_chat_id: Option<String>,
    #[serde(default)]
    pub freshness: Option<LongTermMemoryFreshness>,
    #[serde(default)]
    pub include_stale: bool,
    #[serde(default)]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemorySlotLookup {
    pub slot: LongTermMemorySlot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<LongTermMemoryEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nearby_candidates: Vec<LongTermMemoryEntry>,
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
            source_type: self.source_type,
            source_scope: self.source_scope,
            confidence: self.confidence,
            freshness: self.freshness,
            stale_hint: self.stale_hint,
            supporting_citations: normalize_supporting_citations(&self.supporting_citations),
            evidence_count: self.evidence_count.filter(|value| *value > 0),
            observed_at: self.observed_at.filter(|value| *value > 0),
            last_confirmed_at: self.last_confirmed_at.filter(|value| *value > 0),
            source_revision: self.source_revision.filter(|value| *value > 0),
        })
    }

    pub fn stable_id(&self) -> Option<String> {
        let normalized = self.normalized()?;
        stable_id_for_kind_topic(&normalized.kind, &normalized.topic)
    }
}

impl LongTermMemorySlot {
    pub fn normalized(&self) -> Option<Self> {
        let topic = normalize_topic(self.topic.trim());
        if topic.is_empty() {
            return None;
        }
        Some(Self {
            kind: self.kind.clone(),
            topic,
        })
    }

    pub fn stable_id(&self) -> Option<String> {
        let normalized = self.normalized()?;
        stable_id_for_kind_topic(&normalized.kind, &normalized.topic)
    }
}

impl LongTermMemoryQuery {
    pub fn normalized(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            topic: self
                .topic
                .as_deref()
                .map(normalize_topic)
                .filter(|value| !value.is_empty()),
            source_scope: self.source_scope,
            source_chat_id: self
                .source_chat_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            freshness: self.freshness,
            include_stale: self.include_stale,
            limit: self.limit.clamp(1, MAX_LONG_TERM_MEMORY_ITEMS),
        }
    }
}

fn stable_id_for_kind_topic(kind: &LongTermMemoryKind, topic: &str) -> Option<String> {
    if topic.is_empty() {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    kind.hash(&mut hasher);
    0x517c_c1b7_u32.hash(&mut hasher);
    topic.hash(&mut hasher);
    let mut id = String::with_capacity(20);
    id.push_str("ltm-");
    id.push_str(&format!("{:016x}", hasher.finish()));
    Some(id)
}

fn normalize_supporting_citations(values: &[String]) -> Vec<String> {
    let mut citations =
        Vec::with_capacity(values.len().min(MAX_LONG_TERM_MEMORY_SUPPORTING_CITATIONS));
    for raw in values {
        let normalized = truncate_utf8_bytes(raw.trim(), MAX_LONG_TERM_MEMORY_CITATION_LEN);
        if normalized.is_empty() || citations.iter().any(|item| item == &normalized) {
            continue;
        }
        citations.push(normalized);
        if citations.len() >= MAX_LONG_TERM_MEMORY_SUPPORTING_CITATIONS {
            break;
        }
    }
    citations
}

fn effective_evidence_count(citation_count: usize, evidence_count: u32) -> u32 {
    evidence_count.max(citation_count as u32)
}

fn stale_hint_rank(hint: LongTermMemoryStaleHint) -> u8 {
    match hint {
        LongTermMemoryStaleHint::None => 0,
        LongTermMemoryStaleHint::ReviewBeforeUse => 1,
        LongTermMemoryStaleHint::VerifyAgainstCurrentState => 2,
    }
}

fn strictest_stale_hint(
    left: LongTermMemoryStaleHint,
    right: LongTermMemoryStaleHint,
) -> LongTermMemoryStaleHint {
    if stale_hint_rank(left) >= stale_hint_rank(right) {
        left
    } else {
        right
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LongTermMemoryResolvedMeta {
    source_type: LongTermMemorySourceType,
    source_scope: LongTermMemorySourceScope,
    confidence: LongTermMemoryConfidence,
    freshness: LongTermMemoryFreshness,
    stale_hint: LongTermMemoryStaleHint,
}

fn infer_long_term_memory_source_scope(
    kind: &LongTermMemoryKind,
    source_chat_id: Option<&str>,
    requested: Option<LongTermMemorySourceScope>,
) -> LongTermMemorySourceScope {
    match requested {
        Some(LongTermMemorySourceScope::Chat) if source_chat_id.is_some() => {
            LongTermMemorySourceScope::Chat
        }
        Some(LongTermMemorySourceScope::Chat) => match kind {
            LongTermMemoryKind::Project | LongTermMemoryKind::Task => {
                LongTermMemorySourceScope::User
            }
            LongTermMemoryKind::Fact => LongTermMemorySourceScope::World,
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Constraint => LongTermMemorySourceScope::User,
        },
        Some(scope) => scope,
        None => match kind {
            LongTermMemoryKind::Project | LongTermMemoryKind::Task if source_chat_id.is_some() => {
                LongTermMemorySourceScope::Chat
            }
            LongTermMemoryKind::Fact => LongTermMemorySourceScope::World,
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Constraint
            | LongTermMemoryKind::Project
            | LongTermMemoryKind::Task => LongTermMemorySourceScope::User,
        },
    }
}

fn infer_long_term_memory_confidence(
    kind: &LongTermMemoryKind,
    source_type: LongTermMemorySourceType,
    requested: Option<LongTermMemoryConfidence>,
) -> LongTermMemoryConfidence {
    requested.unwrap_or(match source_type {
        LongTermMemorySourceType::ManualTool => LongTermMemoryConfidence::High,
        LongTermMemorySourceType::ExternalObservation => LongTermMemoryConfidence::Medium,
        LongTermMemorySourceType::SystemRuntime => LongTermMemoryConfidence::Medium,
        LongTermMemorySourceType::Conversation => match kind {
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Constraint => LongTermMemoryConfidence::High,
            LongTermMemoryKind::Relationship
            | LongTermMemoryKind::Project
            | LongTermMemoryKind::Task
            | LongTermMemoryKind::Fact => LongTermMemoryConfidence::Medium,
        },
    })
}

fn infer_long_term_memory_freshness(
    kind: &LongTermMemoryKind,
    requested: Option<LongTermMemoryFreshness>,
) -> LongTermMemoryFreshness {
    requested.unwrap_or(match kind {
        LongTermMemoryKind::Task => LongTermMemoryFreshness::Volatile,
        LongTermMemoryKind::Project
        | LongTermMemoryKind::Relationship
        | LongTermMemoryKind::Fact => LongTermMemoryFreshness::Dynamic,
        LongTermMemoryKind::Preference
        | LongTermMemoryKind::Profile
        | LongTermMemoryKind::Constraint => LongTermMemoryFreshness::Stable,
    })
}

fn infer_long_term_memory_stale_hint(
    freshness: LongTermMemoryFreshness,
    requested: Option<LongTermMemoryStaleHint>,
) -> LongTermMemoryStaleHint {
    requested.unwrap_or(match freshness {
        LongTermMemoryFreshness::Stable => LongTermMemoryStaleHint::None,
        LongTermMemoryFreshness::Dynamic => LongTermMemoryStaleHint::ReviewBeforeUse,
        LongTermMemoryFreshness::Volatile => LongTermMemoryStaleHint::VerifyAgainstCurrentState,
    })
}

fn resolve_long_term_memory_meta(draft: &LongTermMemoryDraft) -> LongTermMemoryResolvedMeta {
    let source_type = draft
        .source_type
        .unwrap_or(LongTermMemorySourceType::Conversation);
    let source_scope = infer_long_term_memory_source_scope(
        &draft.kind,
        draft.source_chat_id.as_deref(),
        draft.source_scope,
    );
    let freshness = infer_long_term_memory_freshness(&draft.kind, draft.freshness);
    let confidence = infer_long_term_memory_confidence(&draft.kind, source_type, draft.confidence);
    let stale_hint = infer_long_term_memory_stale_hint(freshness, draft.stale_hint);
    LongTermMemoryResolvedMeta {
        source_type,
        source_scope,
        confidence,
        freshness,
        stale_hint,
    }
}

pub(crate) fn long_term_memory_entry_from_draft(
    draft: &LongTermMemoryDraft,
    id: String,
    now_secs: u64,
) -> Option<LongTermMemoryEntry> {
    let normalized = draft.normalized()?;
    let meta = resolve_long_term_memory_meta(&normalized);
    let observed_at = normalized.observed_at.unwrap_or(now_secs);
    let supporting_citations = normalize_supporting_citations(&normalized.supporting_citations);
    let evidence_count = effective_evidence_count(
        supporting_citations.len(),
        normalized.evidence_count.unwrap_or(0),
    );
    let last_confirmed_at = normalized
        .last_confirmed_at
        .unwrap_or(observed_at)
        .max(observed_at);
    Some(LongTermMemoryEntry {
        id,
        kind: normalized.kind,
        topic: normalized.topic,
        content: normalized.content,
        keywords: normalized.keywords,
        source_chat_id: normalized.source_chat_id,
        source_type: meta.source_type,
        source_scope: meta.source_scope,
        confidence: meta.confidence,
        freshness: meta.freshness,
        stale_hint: meta.stale_hint,
        supporting_citations,
        evidence_count,
        created_at: now_secs,
        updated_at: now_secs,
        observed_at,
        last_confirmed_at,
        source_revision: draft.source_revision.unwrap_or(0),
        last_used_at: 0,
    })
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
    fn get_slot(&self, slot: &LongTermMemorySlot) -> Result<Option<LongTermMemoryEntry>> {
        let Some(id) = slot.stable_id() else {
            return Ok(None);
        };
        self.get(&id)
    }
    fn query(&self, query: &LongTermMemoryQuery) -> Result<Vec<LongTermMemoryEntry>> {
        let normalized = query.normalized();
        let now_secs = crate::util::current_unix_secs();
        let mut entries = self.list(MAX_LONG_TERM_MEMORY_ITEMS)?;
        entries.retain(|entry| long_term_memory_matches_query(entry, &normalized, now_secs));
        entries.sort_by(|left, right| {
            compare_long_term_memory_query_results(left, right, &normalized)
        });
        entries.truncate(normalized.limit);
        Ok(entries)
    }
    fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>>;
    fn delete(&self, id: &str) -> Result<bool>;
    fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool>;
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
    entry.supporting_citations = normalize_supporting_citations(&entry.supporting_citations);
    entry.evidence_count =
        effective_evidence_count(entry.supporting_citations.len(), entry.evidence_count);
    if entry.updated_at == 0 {
        entry.updated_at = entry.created_at;
    }
    if entry.observed_at == 0 {
        entry.observed_at = entry.updated_at.max(entry.created_at);
    }
    if entry.last_confirmed_at == 0 {
        entry.last_confirmed_at = entry.observed_at;
    }
    let source_scope = infer_long_term_memory_source_scope(
        &entry.kind,
        entry.source_chat_id.as_deref(),
        Some(entry.source_scope),
    );
    let freshness = infer_long_term_memory_freshness(&entry.kind, Some(entry.freshness));
    let source_type = entry.source_type;
    let confidence =
        infer_long_term_memory_confidence(&entry.kind, source_type, Some(entry.confidence));
    entry.source_type = source_type;
    entry.source_scope = source_scope;
    entry.confidence = confidence;
    entry.freshness = freshness;
    entry.stale_hint = infer_long_term_memory_stale_hint(freshness, Some(entry.stale_hint));
    Some(entry)
}

fn age_state_for_entry(entry: &LongTermMemoryEntry, now_secs: u64) -> LongTermMemoryAgeState {
    let observed_at = entry_observed_at(entry);
    if now_secs == 0 || observed_at == 0 || observed_at > now_secs {
        return LongTermMemoryAgeState::Current;
    }
    let age_secs = now_secs - observed_at;
    if age_secs >= entry.freshness.stale_after_secs() {
        LongTermMemoryAgeState::Stale
    } else if age_secs >= entry.freshness.aging_after_secs() {
        LongTermMemoryAgeState::Aging
    } else {
        LongTermMemoryAgeState::Current
    }
}

fn effective_stale_hint(entry: &LongTermMemoryEntry, now_secs: u64) -> LongTermMemoryStaleHint {
    match age_state_for_entry(entry, now_secs) {
        LongTermMemoryAgeState::Current => entry.stale_hint,
        LongTermMemoryAgeState::Aging => match entry.stale_hint {
            LongTermMemoryStaleHint::None => LongTermMemoryStaleHint::ReviewBeforeUse,
            hint => hint,
        },
        LongTermMemoryAgeState::Stale => LongTermMemoryStaleHint::VerifyAgainstCurrentState,
    }
}

fn render_age_hint(entry: &LongTermMemoryEntry, now_secs: u64) -> Option<String> {
    let observed_at = entry_observed_at(entry);
    if now_secs == 0 || observed_at == 0 || observed_at > now_secs {
        return None;
    }
    let age_secs = now_secs - observed_at;
    let prefix = if entry.evidence_count > 0 || !entry.supporting_citations.is_empty() {
        "confirmed"
    } else {
        "updated"
    };
    let value = match age_secs {
        0..=86_400 => format!("{prefix} today"),
        86_401..=604_800 => format!("{prefix} {}d ago", age_secs / 86_400),
        604_801..=5_184_000 => format!("{prefix} {}w ago", age_secs / 604_800),
        _ => format!("{prefix} {}mo ago", age_secs / 2_592_000),
    };
    Some(value)
}

fn entry_observed_at(entry: &LongTermMemoryEntry) -> u64 {
    entry
        .last_confirmed_at
        .max(entry.observed_at)
        .max(entry.updated_at)
        .max(entry.created_at)
}

fn evidence_state_for_entry(
    entry: &LongTermMemoryEntry,
    now_secs: u64,
) -> LongTermMemoryEvidenceState {
    let age_state = age_state_for_entry(entry, now_secs);
    let effective_hint = effective_stale_hint(entry, now_secs);
    if matches!(
        effective_hint,
        LongTermMemoryStaleHint::VerifyAgainstCurrentState
    ) || matches!(age_state, LongTermMemoryAgeState::Stale)
    {
        return LongTermMemoryEvidenceState::NeedsReview;
    }
    if matches!(age_state, LongTermMemoryAgeState::Aging) {
        return LongTermMemoryEvidenceState::PossiblyStale;
    }
    if matches!(entry.freshness, LongTermMemoryFreshness::Stable)
        && matches!(
            entry.kind,
            LongTermMemoryKind::Preference
                | LongTermMemoryKind::Profile
                | LongTermMemoryKind::Relationship
                | LongTermMemoryKind::Constraint
                | LongTermMemoryKind::Fact
        )
    {
        return LongTermMemoryEvidenceState::StableFact;
    }
    LongTermMemoryEvidenceState::RecentState
}

pub(crate) fn long_term_memory_evidence_state(
    entry: &LongTermMemoryEntry,
    now_secs: u64,
) -> LongTermMemoryEvidenceState {
    evidence_state_for_entry(entry, now_secs)
}

pub(crate) fn long_term_memory_effective_stale_hint(
    entry: &LongTermMemoryEntry,
    now_secs: u64,
) -> LongTermMemoryStaleHint {
    effective_stale_hint(entry, now_secs)
}

pub fn long_term_memory_evidence_summary(
    entry: &LongTermMemoryEntry,
    now_secs: u64,
) -> LongTermMemoryEvidenceSummary {
    let state = evidence_state_for_entry(entry, now_secs);
    let stale_hint = effective_stale_hint(entry, now_secs);
    let evidence_count =
        effective_evidence_count(entry.supporting_citations.len(), entry.evidence_count);
    let age_summary = render_age_hint(entry, now_secs);
    let mut summary = String::from(state.label());
    summary.push_str("; ");
    summary.push_str(entry.confidence.label());
    summary.push_str("; ");
    summary.push_str(entry.source_scope.label());
    if !matches!(entry.freshness, LongTermMemoryFreshness::Stable) {
        summary.push_str("; ");
        summary.push_str(entry.freshness.label());
    }
    if let Some(label) = stale_hint.label() {
        summary.push_str("; stale_hint=");
        summary.push_str(label);
    }
    if let Some(age_summary) = age_summary.as_deref() {
        summary.push_str("; ");
        summary.push_str(age_summary);
    }
    if evidence_count > 0 {
        summary.push_str("; evidence=");
        summary.push_str(&evidence_count.to_string());
    }
    LongTermMemoryEvidenceSummary {
        state,
        confidence: entry.confidence,
        freshness: entry.freshness,
        stale_hint,
        source_type: entry.source_type,
        source_scope: entry.source_scope,
        evidence_count,
        supporting_citations: entry.supporting_citations.clone(),
        last_confirmed_at: entry.last_confirmed_at,
        last_used_at: entry.last_used_at,
        age_summary,
        summary,
    }
}

fn confidence_rank(confidence: LongTermMemoryConfidence) -> u8 {
    match confidence {
        LongTermMemoryConfidence::Low => 0,
        LongTermMemoryConfidence::Medium => 1,
        LongTermMemoryConfidence::High => 2,
    }
}

pub(crate) fn long_term_memory_matches_query(
    entry: &LongTermMemoryEntry,
    query: &LongTermMemoryQuery,
    now_secs: u64,
) -> bool {
    if let Some(kind) = query.kind.as_ref() {
        if &entry.kind != kind {
            return false;
        }
    }
    if let Some(topic) = query.topic.as_deref() {
        if entry.topic != topic {
            return false;
        }
    }
    if let Some(source_scope) = query.source_scope {
        if entry.source_scope != source_scope {
            return false;
        }
    }
    if let Some(source_chat_id) = query.source_chat_id.as_deref() {
        if entry.source_chat_id.as_deref() != Some(source_chat_id) {
            return false;
        }
    }
    if let Some(freshness) = query.freshness {
        if entry.freshness != freshness {
            return false;
        }
    }
    if !query.include_stale
        && matches!(
            age_state_for_entry(entry, now_secs),
            LongTermMemoryAgeState::Stale
        )
    {
        return false;
    }
    true
}

pub(crate) fn compare_long_term_memory_query_results(
    left: &LongTermMemoryEntry,
    right: &LongTermMemoryEntry,
    query: &LongTermMemoryQuery,
) -> Ordering {
    query_exactness_priority(right, query)
        .cmp(&query_exactness_priority(left, query))
        .then_with(|| confidence_rank(right.confidence).cmp(&confidence_rank(left.confidence)))
        .then_with(|| entry_observed_at(right).cmp(&entry_observed_at(left)))
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| right.created_at.cmp(&left.created_at))
}

fn query_exactness_priority(entry: &LongTermMemoryEntry, query: &LongTermMemoryQuery) -> u8 {
    let mut score = 0u8;
    if query
        .topic
        .as_deref()
        .is_some_and(|topic| entry.topic == topic)
    {
        score = score.saturating_add(4);
    }
    if query.kind.as_ref().is_some_and(|kind| &entry.kind == kind) {
        score = score.saturating_add(3);
    }
    if query
        .source_chat_id
        .as_deref()
        .is_some_and(|chat_id| entry.source_chat_id.as_deref() == Some(chat_id))
    {
        score = score.saturating_add(2);
    }
    if query
        .source_scope
        .is_some_and(|source_scope| entry.source_scope == source_scope)
    {
        score = score.saturating_add(1);
    }
    score
}

fn draft_is_older_than_existing(
    existing: &LongTermMemoryEntry,
    incoming_observed_at: u64,
    incoming_source_revision: u64,
) -> bool {
    if incoming_source_revision > 0
        && existing.source_revision > 0
        && incoming_source_revision < existing.source_revision
    {
        return true;
    }
    incoming_observed_at > 0
        && existing.observed_at > 0
        && incoming_observed_at < existing.observed_at
}

pub fn merge_long_term_memory_entry(
    existing: &mut LongTermMemoryEntry,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> bool {
    let Some(normalized) = draft.normalized() else {
        return false;
    };
    let meta = resolve_long_term_memory_meta(&normalized);
    let mut changed = false;
    let incoming_observed_at = normalized.observed_at.unwrap_or(now_secs);
    let incoming_last_confirmed_at = normalized
        .last_confirmed_at
        .unwrap_or(incoming_observed_at)
        .max(incoming_observed_at);
    let incoming_source_revision = normalized.source_revision.unwrap_or(0);
    let incoming_citations = normalize_supporting_citations(&normalized.supporting_citations);
    let incoming_evidence_count = effective_evidence_count(
        incoming_citations.len(),
        normalized.evidence_count.unwrap_or(0),
    );
    let incoming_is_older =
        draft_is_older_than_existing(existing, incoming_observed_at, incoming_source_revision);
    let content_changed = existing.content != normalized.content;
    let can_replace_content = !incoming_is_older
        && confidence_rank(meta.confidence) >= confidence_rank(existing.confidence);
    if content_changed && incoming_is_older {
        return false;
    }
    if content_changed && !can_replace_content {
        let next_hint = strictest_stale_hint(
            existing.stale_hint,
            LongTermMemoryStaleHint::VerifyAgainstCurrentState,
        );
        if existing.stale_hint != next_hint {
            existing.stale_hint = next_hint;
            changed = true;
        }
        if changed && existing.updated_at != now_secs {
            existing.updated_at = now_secs;
        }
        return changed;
    }
    if content_changed && can_replace_content {
        existing.content = normalized.content;
        existing.supporting_citations = incoming_citations.clone();
        existing.evidence_count = incoming_evidence_count;
        existing.last_confirmed_at = incoming_last_confirmed_at;
        changed = true;
    }

    let merged_keywords = if content_changed && can_replace_content {
        normalized.keywords.clone()
    } else {
        let mut merged_keywords = existing.keywords.clone();
        for keyword in &normalized.keywords {
            if merged_keywords.iter().any(|item| item == keyword) {
                continue;
            }
            merged_keywords.push(keyword.clone());
        }
        merged_keywords.truncate(MAX_LONG_TERM_MEMORY_KEYWORDS);
        merged_keywords
    };
    if existing.keywords != merged_keywords {
        existing.keywords = merged_keywords;
        changed = true;
    }
    if !content_changed {
        let base_evidence_count =
            effective_evidence_count(existing.supporting_citations.len(), existing.evidence_count);
        let mut merged_citations = existing.supporting_citations.clone();
        let mut new_citation_count = 0u32;
        for citation in &incoming_citations {
            if merged_citations.iter().any(|item| item == citation) {
                continue;
            }
            merged_citations.push(citation.clone());
            new_citation_count = new_citation_count.saturating_add(1);
            if merged_citations.len() >= MAX_LONG_TERM_MEMORY_SUPPORTING_CITATIONS {
                break;
            }
        }
        if existing.supporting_citations != merged_citations {
            existing.supporting_citations = merged_citations;
            changed = true;
        }
        let newer_confirmation = incoming_last_confirmed_at > existing.last_confirmed_at;
        let confirmation_bump =
            u32::from(new_citation_count == 0 && incoming_evidence_count > 0 && newer_confirmation);
        let merged_evidence_count = base_evidence_count
            .saturating_add(new_citation_count)
            .max(incoming_evidence_count)
            .saturating_add(confirmation_bump);
        if existing.evidence_count != merged_evidence_count {
            existing.evidence_count = merged_evidence_count;
            changed = true;
        }
        if existing.last_confirmed_at < incoming_last_confirmed_at {
            existing.last_confirmed_at = incoming_last_confirmed_at;
            changed = true;
        }
    }
    if let Some(source_chat_id) = normalized.source_chat_id.filter(|_| !incoming_is_older) {
        if existing.source_chat_id.as_deref() != Some(source_chat_id.as_str()) {
            existing.source_chat_id = Some(source_chat_id);
            changed = true;
        }
    }
    if !incoming_is_older && existing.source_type != meta.source_type {
        existing.source_type = meta.source_type;
        changed = true;
    }
    if !incoming_is_older && existing.source_scope != meta.source_scope {
        existing.source_scope = meta.source_scope;
        changed = true;
    }
    if !incoming_is_older && existing.confidence != meta.confidence {
        existing.confidence = meta.confidence;
        changed = true;
    }
    if !incoming_is_older && existing.freshness != meta.freshness {
        existing.freshness = meta.freshness;
        changed = true;
    }
    if !incoming_is_older && existing.stale_hint != meta.stale_hint {
        existing.stale_hint = meta.stale_hint;
        changed = true;
    }
    if !incoming_is_older && existing.observed_at != incoming_observed_at {
        existing.observed_at = incoming_observed_at;
        changed = true;
    }
    if !incoming_is_older && existing.source_revision != incoming_source_revision {
        existing.source_revision = incoming_source_revision;
        changed = true;
    }
    if changed && existing.updated_at != now_secs {
        existing.updated_at = now_secs;
        changed = true;
    }
    changed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LongTermMemoryMergeGuardDecision {
    Allow,
    RejectOlderObservation,
    RejectLowerConfidenceContent,
}

pub(crate) fn inspect_long_term_memory_merge_guard(
    existing: &LongTermMemoryEntry,
    draft: &LongTermMemoryDraft,
    now_secs: u64,
) -> LongTermMemoryMergeGuardDecision {
    let Some(normalized) = draft.normalized() else {
        return LongTermMemoryMergeGuardDecision::Allow;
    };
    let meta = resolve_long_term_memory_meta(&normalized);
    let incoming_observed_at = normalized.observed_at.unwrap_or(now_secs);
    let incoming_source_revision = normalized.source_revision.unwrap_or(0);
    let incoming_is_older =
        draft_is_older_than_existing(existing, incoming_observed_at, incoming_source_revision);
    let content_changed = existing.content != normalized.content;
    if !content_changed {
        return LongTermMemoryMergeGuardDecision::Allow;
    }
    if incoming_is_older {
        return LongTermMemoryMergeGuardDecision::RejectOlderObservation;
    }
    if confidence_rank(meta.confidence) < confidence_rank(existing.confidence) {
        return LongTermMemoryMergeGuardDecision::RejectLowerConfidenceContent;
    }
    LongTermMemoryMergeGuardDecision::Allow
}

pub(crate) fn govern_long_term_memory_entries(
    entries: &mut Vec<LongTermMemoryEntry>,
    now_secs: u64,
) -> bool {
    let policy = shared_long_term_governance_policy();
    let original_len = entries.len();
    entries.retain(|entry| !policy.is_stale(entry, now_secs));

    entries.sort_by(|a, b| {
        entry_observed_at(b)
            .cmp(&entry_observed_at(a))
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });

    let mut preference = 0usize;
    let mut profile = 0usize;
    let mut relationship = 0usize;
    let mut project = 0usize;
    let mut task = 0usize;
    let mut constraint = 0usize;
    let mut fact = 0usize;
    let mut kept = Vec::with_capacity(entries.len().min(MAX_LONG_TERM_MEMORY_ITEMS));
    for entry in entries.drain(..) {
        let used = match entry.kind {
            LongTermMemoryKind::Preference => &mut preference,
            LongTermMemoryKind::Profile => &mut profile,
            LongTermMemoryKind::Relationship => &mut relationship,
            LongTermMemoryKind::Project => &mut project,
            LongTermMemoryKind::Task => &mut task,
            LongTermMemoryKind::Constraint => &mut constraint,
            LongTermMemoryKind::Fact => &mut fact,
        };
        if *used >= policy.kind_budget(&entry.kind) {
            continue;
        }
        *used += 1;
        kept.push(entry);
    }

    if kept.len() > MAX_LONG_TERM_MEMORY_ITEMS {
        kept.truncate(MAX_LONG_TERM_MEMORY_ITEMS);
    }
    let changed = kept.len() != original_len;
    *entries = kept;
    changed
}

pub(crate) fn select_long_term_recall_entries(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    profile: MemoryProfile,
) -> LongTermRecallSelection {
    let policy = memory_policy(profile).long_term_recall;
    let desired = policy.desired_entry_count(policy.block_max_len_cap);
    let recall_query = policy.build_recall_query(user_query, summary_text, recent_messages);
    let mut candidates = store
        .recall(
            &recall_query,
            Some(chat_id),
            policy.direct_recall_limit(desired),
        )
        .unwrap_or_default();
    let direct_count = candidates.len();
    let mut used_fallback = false;
    if candidates.len() < desired {
        let mut fallback = store
            .list(policy.fallback_list_limit(desired))
            .unwrap_or_default();
        fallback.sort_by(|a, b| policy.compare_fallback_entries(chat_id, a, b));
        for entry in fallback {
            if candidates.iter().any(|existing| existing.id == entry.id) {
                continue;
            }
            candidates.push(entry);
            used_fallback = true;
        }
    }
    reorder_recall_candidates_for_chat(chat_id, &mut candidates);
    let selected = policy.select_entries(candidates.clone(), desired);
    LongTermRecallSelection {
        recall_query,
        candidates,
        selected,
        desired,
        used_fallback: used_fallback || direct_count < desired,
    }
}

pub(crate) fn recall_long_term_memory_entries(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    profile: MemoryProfile,
) -> Vec<LongTermMemoryEntry> {
    select_long_term_recall_entries(
        store,
        chat_id,
        user_query,
        summary_text,
        recent_messages,
        profile,
    )
    .selected
}

pub fn recall_long_term_memory_block(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    system_max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    let now_secs = crate::util::current_unix_secs();
    let policy = memory_policy(profile).long_term_recall;
    let block_max_len = policy.recall_block_max_len(system_max_len);
    let selected = recall_long_term_memory_entries(
        store,
        chat_id,
        user_query,
        summary_text,
        recent_messages,
        profile,
    );
    render_long_term_memory_block_with_now(&selected, block_max_len, now_secs)
}

pub fn render_exact_long_term_memory_block(
    store: &dyn LongTermMemoryStore,
    slot: &LongTermMemorySlot,
    max_len: usize,
) -> Option<String> {
    render_exact_long_term_memory_block_with_now(
        store,
        slot,
        max_len,
        crate::util::current_unix_secs(),
    )
}

fn render_exact_long_term_memory_block_with_now(
    store: &dyn LongTermMemoryStore,
    slot: &LongTermMemorySlot,
    max_len: usize,
    now_secs: u64,
) -> Option<String> {
    if max_len < 32 {
        return None;
    }
    let lookup = lookup_long_term_memory_slot(store, slot, 0).ok()?;
    let entry = lookup.entry?;
    let evidence = long_term_memory_evidence_summary(&entry, now_secs);
    let mut out = String::from("## Long-term memory (exact slot)\n");
    let body = render_exact_long_term_memory_body(&entry, &evidence);
    if out.len().saturating_add(body.len()) > max_len {
        return render_long_term_memory_block(&[entry], max_len);
    }
    out.push_str(&body);
    Some(out)
}

fn render_exact_long_term_memory_body(
    entry: &LongTermMemoryEntry,
    evidence: &LongTermMemoryEvidenceSummary,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "Slot: [{}:{}]", entry.kind.label(), entry.topic);
    let _ = writeln!(out, "Content: {}", entry.content);
    let _ = writeln!(out, "Evidence: {}", evidence.summary);
    let _ = write!(
        out,
        "Provenance: source={} scope={}",
        entry.source_type.label(),
        entry.source_scope.label()
    );
    if let Some(source_chat_id) = entry.source_chat_id.as_deref() {
        let _ = write!(out, " chat={}", source_chat_id);
    }
    if entry.source_revision > 0 {
        let _ = write!(out, " revision={}", entry.source_revision);
    }
    out.push('\n');
    if !entry.supporting_citations.is_empty() {
        let _ = writeln!(out, "Citations: {}", entry.supporting_citations.join(", "));
    }
    out
}

fn reorder_recall_candidates_for_chat(chat_id: &str, candidates: &mut [LongTermMemoryEntry]) {
    candidates.sort_by_key(|entry| recall_scope_priority(chat_id, entry));
}

fn recall_scope_priority(chat_id: &str, entry: &LongTermMemoryEntry) -> u8 {
    let same_chat = entry.source_chat_id.as_deref() == Some(chat_id);
    if matches!(entry.source_scope, LongTermMemorySourceScope::Chat) && !same_chat {
        return 6;
    }
    match (same_chat, &entry.kind) {
        (true, LongTermMemoryKind::Project)
        | (true, LongTermMemoryKind::Task)
        | (true, LongTermMemoryKind::Constraint) => 0,
        (true, LongTermMemoryKind::Preference)
        | (true, LongTermMemoryKind::Profile)
        | (true, LongTermMemoryKind::Relationship) => 1,
        (false, LongTermMemoryKind::Preference)
        | (false, LongTermMemoryKind::Profile)
        | (false, LongTermMemoryKind::Relationship) => 2,
        (true, LongTermMemoryKind::Fact) => 3,
        (false, LongTermMemoryKind::Fact) => 4,
        (false, LongTermMemoryKind::Project)
        | (false, LongTermMemoryKind::Task)
        | (false, LongTermMemoryKind::Constraint) => 5,
    }
}

/// 渲染注入 prompt 的长期记忆块。
pub fn render_long_term_memory_block(
    entries: &[LongTermMemoryEntry],
    max_len: usize,
) -> Option<String> {
    render_long_term_memory_block_with_now(entries, max_len, crate::util::current_unix_secs())
}

pub(crate) fn render_long_term_memory_block_with_now(
    entries: &[LongTermMemoryEntry],
    max_len: usize,
    now_secs: u64,
) -> Option<String> {
    if entries.is_empty() || max_len < 32 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(MAX_LONG_TERM_MEMORY_BLOCK_LEN));
    out.push_str("## Long-term memory\n");
    let mut active = Vec::new();
    let mut personal = Vec::new();
    let mut facts = Vec::new();
    for entry in entries {
        match entry.kind {
            LongTermMemoryKind::Project
            | LongTermMemoryKind::Task
            | LongTermMemoryKind::Constraint => active.push(entry),
            LongTermMemoryKind::Preference
            | LongTermMemoryKind::Profile
            | LongTermMemoryKind::Relationship => personal.push(entry),
            LongTermMemoryKind::Fact => facts.push(entry),
        }
    }
    render_long_term_memory_section(&mut out, "Active context", &active, max_len, now_secs);
    render_long_term_memory_section(&mut out, "User profile", &personal, max_len, now_secs);
    render_long_term_memory_section(&mut out, "Facts", &facts, max_len, now_secs);
    if out.trim() == "## Long-term memory" {
        None
    } else {
        Some(out.trim_end().to_string())
    }
}

fn render_long_term_memory_section(
    out: &mut String,
    title: &str,
    entries: &[&LongTermMemoryEntry],
    max_len: usize,
    now_secs: u64,
) {
    if entries.is_empty() {
        return;
    }
    let section_start = out.len();
    let header = format!("\n### {title}\n");
    if out.len().saturating_add(header.len()) > max_len {
        return;
    }
    out.push_str(&header);
    let mut appended = 0usize;
    for entry in entries {
        let line_with_keywords = render_long_term_memory_line(entry, true, now_secs);
        if out
            .len()
            .saturating_add(line_with_keywords.len())
            .saturating_add(1)
            <= max_len
        {
            out.push_str(&line_with_keywords);
            out.push('\n');
            appended += 1;
            continue;
        }
        let line_without_keywords = render_long_term_memory_line(entry, false, now_secs);
        if out
            .len()
            .saturating_add(line_without_keywords.len())
            .saturating_add(1)
            <= max_len
        {
            out.push_str(&line_without_keywords);
            out.push('\n');
            appended += 1;
            continue;
        }
        break;
    }
    if appended == 0 {
        out.truncate(section_start);
    }
}

fn render_long_term_memory_line(
    entry: &LongTermMemoryEntry,
    include_keywords: bool,
    now_secs: u64,
) -> String {
    let mut tags = vec![
        evidence_state_for_entry(entry, now_secs)
            .label()
            .to_string(),
        format!("source: {}", entry.source_type.label()),
        entry.source_scope.label().to_string(),
        entry.confidence.label().to_string(),
    ];
    if !matches!(entry.freshness, LongTermMemoryFreshness::Stable) {
        tags.push(entry.freshness.label().to_string());
    }
    if let Some(label) = effective_stale_hint(entry, now_secs).label() {
        tags.push(label.to_string());
    }
    if matches!(
        age_state_for_entry(entry, now_secs),
        LongTermMemoryAgeState::Aging | LongTermMemoryAgeState::Stale
    ) {
        if let Some(age_hint) = render_age_hint(entry, now_secs) {
            tags.push(age_hint);
        }
    }
    let evidence_count =
        effective_evidence_count(entry.supporting_citations.len(), entry.evidence_count);
    if evidence_count > 0 {
        tags.push(format!("evidence={evidence_count}"));
    }
    if !entry.supporting_citations.is_empty() {
        let preview_count = entry.supporting_citations.len().min(2);
        let mut preview = entry.supporting_citations[..preview_count].join(", ");
        if entry.supporting_citations.len() > preview_count {
            preview.push_str(&format!(
                " +{}",
                entry.supporting_citations.len() - preview_count
            ));
        }
        tags.push(format!("cites: {preview}"));
    }
    if include_keywords && !entry.keywords.is_empty() {
        format!(
            "- [{}:{}] {} ({}, keywords: {})",
            entry.kind.label(),
            entry.topic,
            entry.content,
            tags.join("; "),
            entry.keywords.join(", ")
        )
    } else {
        format!(
            "- [{}:{}] {} ({})",
            entry.kind.label(),
            entry.topic,
            entry.content,
            tags.join("; ")
        )
    }
}

fn build_recent_recall_grounding(
    recent_messages: &[SessionMessage],
    max_messages: usize,
    max_chars: usize,
) -> String {
    if recent_messages.is_empty() || max_messages == 0 || max_chars == 0 {
        return String::new();
    }
    let start = recent_messages.len().saturating_sub(max_messages);
    let mut out = String::new();
    for message in &recent_messages[start..] {
        let role = message.role.trim();
        let content = message.content.trim();
        if role.is_empty() || content.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&role.to_uppercase());
        out.push_str(": ");
        out.push_str(content);
    }
    truncate_utf8_bytes(out.trim(), max_chars)
}

#[cfg(test)]
pub(crate) fn score_long_term_memory_recall(
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    entry: &LongTermMemoryEntry,
) -> u32 {
    score_long_term_memory_recall_breakdown(query, source_chat_id, now_secs, entry).total_score
}

pub(crate) fn score_long_term_memory_recall_breakdown(
    query: &str,
    source_chat_id: Option<&str>,
    now_secs: u64,
    entry: &LongTermMemoryEntry,
) -> LongTermRecallScoreBreakdown {
    let normalized_query = normalize_for_match(query);
    if normalized_query.chars().count() < 2 {
        return LongTermRecallScoreBreakdown::default();
    }

    let normalized_content = normalize_for_match(&entry.content);
    let normalized_topic = normalize_for_match(&entry.topic);
    let normalized_document = match (normalized_topic.is_empty(), normalized_content.is_empty()) {
        (true, true) => String::new(),
        (false, true) => normalized_topic.clone(),
        (true, false) => normalized_content.clone(),
        (false, false) => format!("{} {}", normalized_topic, normalized_content),
    };
    let terms = collect_match_terms(query);
    let mut exact_match_score = 0u32;
    let mut lexical_score = 0u32;
    let mut keyword_score = 0u32;
    let mut reasons = Vec::new();

    if normalized_topic == normalized_query {
        exact_match_score = exact_match_score.saturating_add(14);
        reasons.push("exact topic match".to_string());
    }
    if normalized_content.contains(&normalized_query) {
        exact_match_score = exact_match_score.saturating_add(8);
    }
    if normalized_topic.contains(&normalized_query) {
        lexical_score = lexical_score.saturating_add(10);
    }
    for term in &terms {
        if normalized_topic.contains(term.as_str()) {
            lexical_score = lexical_score.saturating_add(4);
        }
        if normalized_content.contains(term.as_str()) {
            lexical_score = lexical_score.saturating_add(2);
        }
        for keyword in &entry.keywords {
            let normalized_keyword = normalize_for_match(keyword);
            if normalized_keyword.contains(term.as_str()) || term.contains(&normalized_keyword) {
                keyword_score = keyword_score.saturating_add(3);
            }
        }
    }
    if lexical_score > 0 && !reasons.iter().any(|reason| reason == "term overlap") {
        reasons.push("term overlap".to_string());
    }
    if keyword_score > 0 {
        reasons.push("keyword overlap".to_string());
    }
    let semantic_score = trigram_overlap_score(&normalized_query, &normalized_document, 18);
    if semantic_score > 0 {
        reasons.push("semantic overlap".to_string());
    }
    if lexical_score == 0 && exact_match_score == 0 && keyword_score == 0 && semantic_score == 0 {
        return LongTermRecallScoreBreakdown::default();
    }

    let scope_affinity_score = source_chat_id
        .filter(|chat_id| entry.source_chat_id.as_deref() == Some(*chat_id))
        .map(|_| recall_chat_affinity_bonus(&entry.kind))
        .unwrap_or(0);
    if scope_affinity_score > 0 {
        reasons.push("same-chat affinity".to_string());
    }
    let confidence_score = entry.confidence.recall_bonus();
    let recency_score = recall_recency_bonus(now_secs, entry_observed_at(entry));
    let last_used_score = recall_last_used_bonus(now_secs, entry.last_used_at);
    let governance_score = long_term_governance_recall_score(entry, now_secs);
    let total_score = lexical_score
        .saturating_add(semantic_score)
        .saturating_add(exact_match_score)
        .saturating_add(keyword_score)
        .saturating_add(recency_score)
        .saturating_add(last_used_score)
        .saturating_add(confidence_score)
        .saturating_add(scope_affinity_score)
        .saturating_add(governance_score);
    LongTermRecallScoreBreakdown {
        lexical_score,
        semantic_score,
        exact_match_score,
        keyword_score,
        recency_score,
        last_used_score,
        confidence_score,
        scope_affinity_score,
        governance_score,
        total_score,
        reason_fragments: reasons,
    }
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
    normalize_retrieval_text(input)
}

pub fn parse_explicit_long_term_slot_query(query: &str) -> Option<LongTermMemorySlot> {
    let trimmed = query
        .trim()
        .trim_matches(|ch| matches!(ch, '[' | ']' | '(' | ')'));
    if trimmed.is_empty() {
        return None;
    }
    let candidate = trimmed
        .strip_prefix("slot ")
        .or_else(|| trimmed.strip_prefix("slot="))
        .or_else(|| trimmed.strip_prefix("slot:"))
        .unwrap_or(trimmed)
        .trim();
    let separators = [":", "/", "."];
    for separator in separators {
        let mut parts = candidate.splitn(2, separator);
        let Some(kind) = parts.next() else {
            continue;
        };
        let Some(topic) = parts.next() else {
            continue;
        };
        let kind = match kind.trim().to_ascii_lowercase().as_str() {
            "preference" => LongTermMemoryKind::Preference,
            "profile" => LongTermMemoryKind::Profile,
            "relationship" => LongTermMemoryKind::Relationship,
            "project" => LongTermMemoryKind::Project,
            "task" => LongTermMemoryKind::Task,
            "constraint" => LongTermMemoryKind::Constraint,
            "fact" => LongTermMemoryKind::Fact,
            _ => continue,
        };
        let slot = LongTermMemorySlot {
            kind,
            topic: topic.to_string(),
        };
        if let Some(normalized) = slot.normalized() {
            return Some(normalized);
        }
    }
    None
}

pub fn lookup_long_term_memory_slot(
    store: &dyn LongTermMemoryStore,
    slot: &LongTermMemorySlot,
    nearby_limit: usize,
) -> Result<LongTermMemorySlotLookup> {
    let Some(normalized_slot) = slot.normalized() else {
        return Ok(LongTermMemorySlotLookup {
            slot: slot.clone(),
            entry: None,
            nearby_candidates: Vec::new(),
        });
    };
    let entry = store.get_slot(&normalized_slot)?;
    let nearby_candidates = if entry.is_none() && nearby_limit > 0 {
        find_nearby_long_term_memory_slot_candidates(store, &normalized_slot, nearby_limit)?
    } else {
        Vec::new()
    };
    Ok(LongTermMemorySlotLookup {
        slot: normalized_slot,
        entry,
        nearby_candidates,
    })
}

fn find_nearby_long_term_memory_slot_candidates(
    store: &dyn LongTermMemoryStore,
    slot: &LongTermMemorySlot,
    limit: usize,
) -> Result<Vec<LongTermMemoryEntry>> {
    let now_secs = crate::util::current_unix_secs();
    let normalized_topic = normalize_for_match(&slot.topic);
    let query_terms = collect_match_terms(&slot.topic);
    let mut scored = store
        .list(MAX_LONG_TERM_MEMORY_ITEMS)?
        .into_iter()
        .filter_map(|entry| {
            let score = nearby_slot_candidate_score(&entry, slot, &normalized_topic, &query_terms);
            (score > 0).then_some((score, entry))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| {
                u8::from(right.1.kind == slot.kind).cmp(&u8::from(left.1.kind == slot.kind))
            })
            .then_with(|| {
                confidence_rank(right.1.confidence).cmp(&confidence_rank(left.1.confidence))
            })
            .then_with(|| entry_observed_at(&right.1).cmp(&entry_observed_at(&left.1)))
            .then_with(|| {
                long_term_memory_evidence_state(&right.1, now_secs)
                    .label()
                    .cmp(long_term_memory_evidence_state(&left.1, now_secs).label())
            })
    });
    Ok(scored
        .into_iter()
        .take(limit.clamp(1, 8))
        .map(|(_, entry)| entry)
        .collect())
}

fn nearby_slot_candidate_score(
    entry: &LongTermMemoryEntry,
    slot: &LongTermMemorySlot,
    normalized_topic: &str,
    query_terms: &[String],
) -> u32 {
    let entry_topic = normalize_for_match(&entry.topic);
    let mut score = 0u32;
    if entry.kind == slot.kind {
        score = score.saturating_add(8);
    }
    if entry.topic == slot.topic {
        score = score.saturating_add(16);
    } else if entry_topic == normalized_topic {
        score = score.saturating_add(12);
    } else if entry_topic.starts_with(normalized_topic)
        || normalized_topic.starts_with(&entry_topic)
    {
        score = score.saturating_add(8);
    } else if entry_topic.contains(normalized_topic) || normalized_topic.contains(&entry_topic) {
        score = score.saturating_add(6);
    }
    for term in query_terms {
        if entry_topic.contains(term) {
            score = score.saturating_add(3);
        } else if normalize_for_match(&entry.content).contains(term) {
            score = score.saturating_add(1);
        }
    }
    score
}

fn collect_match_terms(query: &str) -> Vec<String> {
    collect_retrieval_terms(query, 2, 24, &[2, 3])
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

fn recall_last_used_bonus(now_secs: u64, last_used_at: u64) -> u32 {
    if now_secs == 0 || last_used_at == 0 || last_used_at > now_secs {
        return 0;
    }
    match now_secs - last_used_at {
        0..=604_800 => 2,
        604_801..=5_184_000 => 1,
        _ => 0,
    }
}

fn long_term_governance_recall_score(entry: &LongTermMemoryEntry, now_secs: u64) -> u32 {
    let scope_bonus: u32 = if matches!(entry.source_scope, LongTermMemorySourceScope::User)
        && matches!(
            entry.kind,
            LongTermMemoryKind::Preference
                | LongTermMemoryKind::Profile
                | LongTermMemoryKind::Relationship
                | LongTermMemoryKind::Constraint
        ) {
        2
    } else {
        0
    };
    let freshness_bonus = match age_state_for_entry(entry, now_secs) {
        LongTermMemoryAgeState::Current => 4u32,
        LongTermMemoryAgeState::Aging => 2u32,
        LongTermMemoryAgeState::Stale => 0u32,
    };
    scope_bonus
        .saturating_add(freshness_bonus)
        .saturating_add(entry.evidence_count.min(4))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;

    #[derive(Clone, Default)]
    struct StubLongTermMemoryStore {
        recall_entries: Vec<LongTermMemoryEntry>,
        list_entries: Vec<LongTermMemoryEntry>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, _drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.recall_entries.iter().take(limit).cloned().collect())
        }

        fn get(&self, id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(self
                .recall_entries
                .iter()
                .chain(self.list_entries.iter())
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.list_entries.iter().take(limit).cloned().collect())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self
                .recall_entries
                .len()
                .saturating_add(self.list_entries.len()))
        }
    }

    fn test_draft(
        kind: LongTermMemoryKind,
        topic: &str,
        content: &str,
        keywords: Vec<&str>,
        source_chat_id: Option<&str>,
    ) -> LongTermMemoryDraft {
        LongTermMemoryDraft {
            kind,
            topic: topic.to_string(),
            content: content.to_string(),
            keywords: keywords.into_iter().map(str::to_string).collect(),
            source_chat_id: source_chat_id.map(str::to_string),
            source_type: None,
            source_scope: None,
            confidence: None,
            freshness: None,
            stale_hint: None,
            supporting_citations: Vec::new(),
            evidence_count: None,
            observed_at: None,
            last_confirmed_at: None,
            source_revision: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn test_entry(
        id: &str,
        kind: LongTermMemoryKind,
        topic: &str,
        content: &str,
        keywords: Vec<&str>,
        source_chat_id: Option<&str>,
        created_at: u64,
        updated_at: u64,
    ) -> LongTermMemoryEntry {
        canonicalize_long_term_memory_entry(LongTermMemoryEntry {
            id: id.to_string(),
            kind,
            topic: topic.to_string(),
            content: content.to_string(),
            keywords: keywords.into_iter().map(str::to_string).collect(),
            source_chat_id: source_chat_id.map(str::to_string),
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::User,
            confidence: LongTermMemoryConfidence::Medium,
            freshness: LongTermMemoryFreshness::Stable,
            stale_hint: LongTermMemoryStaleHint::None,
            supporting_citations: Vec::new(),
            evidence_count: 0,
            created_at,
            updated_at,
            observed_at: updated_at.max(created_at),
            last_confirmed_at: updated_at.max(created_at),
            source_revision: 0,
            last_used_at: 0,
        })
        .unwrap()
    }

    #[test]
    fn normalizes_long_term_memory_draft() {
        let draft = test_draft(
            LongTermMemoryKind::Preference,
            "response_style",
            "  User prefers concise answers.  ",
            vec![" concise ", "STYLE", "STYLE"],
            Some(" chat "),
        );

        let normalized = draft.normalized().unwrap();
        assert_eq!(normalized.topic, "response_style");
        assert_eq!(normalized.content, "User prefers concise answers.");
        assert_eq!(normalized.keywords, vec!["concise", "style"]);
        assert_eq!(normalized.source_chat_id.as_deref(), Some("chat"));
    }

    #[test]
    fn stable_id_is_deterministic() {
        let draft = test_draft(
            LongTermMemoryKind::Project,
            "current_project",
            "We are building Beetle on ESP and Linux",
            vec!["beetle"],
            None,
        );

        assert_eq!(draft.stable_id(), draft.stable_id());
    }

    #[test]
    fn slot_stable_id_matches_draft_slot() {
        let draft = test_draft(
            LongTermMemoryKind::Profile,
            "user_name",
            "甲壳虫",
            vec![],
            None,
        );
        let slot = LongTermMemorySlot {
            kind: LongTermMemoryKind::Profile,
            topic: "user_name".to_string(),
        };

        assert_eq!(draft.stable_id(), slot.stable_id());
    }

    #[test]
    fn parses_explicit_slot_query_syntax() {
        let slot = parse_explicit_long_term_slot_query("project:Current Project").unwrap();
        assert_eq!(slot.kind, LongTermMemoryKind::Project);
        assert_eq!(slot.topic, "current_project");
    }

    #[test]
    fn parses_prefixed_and_dotted_slot_query_syntax() {
        let prefixed =
            parse_explicit_long_term_slot_query("slot relationship:owner relation").unwrap();
        assert_eq!(prefixed.kind, LongTermMemoryKind::Relationship);
        assert_eq!(prefixed.topic, "owner_relation");

        let dotted = parse_explicit_long_term_slot_query("[profile.user name]").unwrap();
        assert_eq!(dotted.kind, LongTermMemoryKind::Profile);
        assert_eq!(dotted.topic, "user_name");
    }

    #[test]
    fn default_query_filters_slot_scope_and_freshness() {
        let now_secs = crate::util::current_unix_secs();
        let mut project = test_entry(
            "ltm-1",
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle runtime.",
            vec!["runtime"],
            Some("chat-a"),
            now_secs.saturating_sub(60),
            now_secs.saturating_sub(30),
        );
        project.source_scope = LongTermMemorySourceScope::Chat;
        project.freshness = LongTermMemoryFreshness::Dynamic;
        let mut fact = test_entry(
            "ltm-2",
            LongTermMemoryKind::Fact,
            "primary_llm",
            "Current primary model is OpenAI.",
            vec!["openai"],
            None,
            now_secs.saturating_sub(60),
            now_secs.saturating_sub(20),
        );
        fact.source_scope = LongTermMemorySourceScope::World;
        let store = StubLongTermMemoryStore {
            recall_entries: Vec::new(),
            list_entries: vec![fact, project.clone()],
        };

        let items = store
            .query(&LongTermMemoryQuery {
                kind: Some(LongTermMemoryKind::Project),
                topic: Some("current_project".to_string()),
                source_scope: Some(LongTermMemorySourceScope::Chat),
                source_chat_id: Some("chat-a".to_string()),
                freshness: Some(LongTermMemoryFreshness::Dynamic),
                include_stale: false,
                limit: 4,
            })
            .unwrap();

        assert_eq!(items, vec![project]);
    }

    #[test]
    fn recall_score_matches_cjk_terms() {
        let entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Project,
            "长期记忆设计",
            "当前项目重点是长期记忆与 Linux 体验",
            vec!["长期记忆", "linux"],
            None,
            0,
            0,
        );

        assert!(score_long_term_memory_recall("记忆这块怎么设计", None, 0, &entry) > 0);
    }

    #[test]
    fn renders_long_term_memory_block() {
        let block = render_long_term_memory_block(
            &[test_entry(
                "ltm-1",
                LongTermMemoryKind::Preference,
                "response_style",
                "User prefers direct technical answers.",
                vec!["direct", "technical"],
                None,
                0,
                0,
            )],
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
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::User,
            confidence: LongTermMemoryConfidence::Medium,
            freshness: LongTermMemoryFreshness::Stable,
            stale_hint: LongTermMemoryStaleHint::None,
            supporting_citations: Vec::new(),
            evidence_count: 0,
            created_at: 42,
            updated_at: 0,
            observed_at: 0,
            last_confirmed_at: 0,
            source_revision: 0,
            last_used_at: 0,
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
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::User,
            confidence: LongTermMemoryConfidence::Medium,
            freshness: LongTermMemoryFreshness::Stable,
            stale_hint: LongTermMemoryStaleHint::None,
            supporting_citations: Vec::new(),
            evidence_count: 0,
            created_at: 1,
            updated_at: 0,
            observed_at: 0,
            last_confirmed_at: 0,
            source_revision: 0,
            last_used_at: 0,
        })
        .unwrap();

        assert_eq!(entry.topic, "location");
    }

    #[test]
    fn stable_id_uses_topic_so_same_slot_can_update() {
        let a = test_draft(
            LongTermMemoryKind::Preference,
            "response_style",
            "User prefers concise answers.",
            vec!["concise"],
            None,
        );
        let b = test_draft(
            LongTermMemoryKind::Preference,
            "response_style",
            "User now prefers detailed answers.",
            vec!["detailed"],
            None,
        );

        assert_eq!(a.stable_id(), b.stable_id());
    }

    #[test]
    fn merge_long_term_memory_entry_overwrites_same_slot_content() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Preference,
            "response_style",
            "User prefers concise answers.",
            vec!["concise"],
            Some("chat-a"),
            10,
            10,
        );
        let draft = test_draft(
            LongTermMemoryKind::Preference,
            "response_style",
            "User now prefers detailed answers.",
            vec!["detailed"],
            Some("chat-b"),
        );

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.content, "User now prefers detailed answers.");
        assert_eq!(entry.keywords, vec!["detailed"]);
        assert_eq!(entry.source_chat_id.as_deref(), Some("chat-b"));
        assert_eq!(entry.created_at, 10);
        assert_eq!(entry.updated_at, 20);
    }

    #[test]
    fn merge_long_term_memory_entry_preserves_source_chat_when_draft_has_none() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle memory.",
            vec!["beetle"],
            Some("chat-a"),
            10,
            10,
        );
        let draft = test_draft(
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle runtime.",
            vec!["runtime"],
            None,
        );

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.source_chat_id.as_deref(), Some("chat-a"));
    }

    #[test]
    fn merge_long_term_memory_entry_merges_keywords_when_content_is_unchanged() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai"],
            Some("chat-a"),
            10,
            10,
        );
        let draft = test_draft(
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["模型"],
            Some("chat-a"),
        );

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.keywords, vec!["openai", "模型"]);
        assert_eq!(entry.content, "当前主模型是 OpenAI。");
    }

    #[test]
    fn merge_long_term_memory_entry_reinforces_same_content_with_archive_evidence() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai"],
            Some("chat-a"),
            10,
            10,
        );
        entry.supporting_citations = vec!["transcript:chat-a#message=1".to_string()];
        entry.evidence_count = 1;
        entry.last_confirmed_at = 10;
        let mut draft = test_draft(
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["模型"],
            Some("chat-a"),
        );
        draft.supporting_citations = vec![
            "daily_note:2026-04-02.md".to_string(),
            "transcript:chat-a#message=1".to_string(),
        ];
        draft.evidence_count = Some(2);
        draft.last_confirmed_at = Some(30);

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 30));
        assert_eq!(entry.supporting_citations.len(), 2);
        assert_eq!(entry.evidence_count, 2);
        assert_eq!(entry.last_confirmed_at, 30);
    }

    #[test]
    fn merge_long_term_memory_entry_rejects_lower_confidence_overwrite() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Fact,
            "timezone",
            "User timezone is Asia/Shanghai.",
            vec!["timezone"],
            Some("chat-a"),
            10,
            10,
        );
        entry.confidence = LongTermMemoryConfidence::High;
        entry.source_revision = 8;
        entry.observed_at = 10;
        let mut draft = test_draft(
            LongTermMemoryKind::Fact,
            "timezone",
            "User timezone is UTC.",
            vec!["utc"],
            Some("chat-a"),
        );
        draft.confidence = Some(LongTermMemoryConfidence::Low);
        draft.observed_at = Some(20);
        draft.source_revision = Some(9);

        assert!(merge_long_term_memory_entry(&mut entry, &draft, 20));
        assert_eq!(entry.content, "User timezone is Asia/Shanghai.");
        assert_eq!(entry.confidence, LongTermMemoryConfidence::High);
        assert_eq!(entry.source_revision, 8);
        assert_eq!(
            entry.stale_hint,
            LongTermMemoryStaleHint::VerifyAgainstCurrentState
        );
    }

    #[test]
    fn merge_long_term_memory_entry_rejects_older_revision_overwrite() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle runtime.",
            vec!["runtime"],
            Some("chat-a"),
            10,
            10,
        );
        entry.source_revision = 12;
        entry.observed_at = 30;
        let mut draft = test_draft(
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle memory.",
            vec!["memory"],
            Some("chat-a"),
        );
        draft.confidence = Some(LongTermMemoryConfidence::High);
        draft.observed_at = Some(20);
        draft.source_revision = Some(10);

        assert!(!merge_long_term_memory_entry(&mut entry, &draft, 40));
        assert_eq!(entry.content, "Current project is Beetle runtime.");
        assert_eq!(entry.source_revision, 12);
        assert_eq!(entry.observed_at, 30);
    }

    #[test]
    fn render_long_term_memory_block_includes_evidence_summary() {
        let mut entry = test_entry(
            "ltm-1",
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai"],
            None,
            1,
            2,
        );
        entry.supporting_citations = vec![
            "transcript:chat-a#message=1".to_string(),
            "daily_note:2026-04-02.md".to_string(),
        ];
        entry.evidence_count = 2;
        entry.last_confirmed_at = 2;

        let block = render_long_term_memory_block_with_now(&[entry], 512, 2).unwrap();

        assert!(block.contains("evidence=2"));
        assert!(block.contains("cites: transcript:chat-a#message=1"));
    }

    #[test]
    fn exact_slot_block_surfaces_evidence_and_provenance() {
        let slot = LongTermMemorySlot {
            kind: LongTermMemoryKind::Fact,
            topic: "primary_llm".to_string(),
        };
        let mut entry = test_entry(
            slot.stable_id().as_deref().unwrap_or("ltm-1"),
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai"],
            Some("chat-a"),
            1,
            2,
        );
        entry.source_scope = LongTermMemorySourceScope::World;
        entry.source_revision = 3;
        entry.supporting_citations = vec!["transcript:chat-a#message=1".to_string()];
        let store = StubLongTermMemoryStore {
            recall_entries: vec![entry.clone()],
            list_entries: vec![entry],
        };

        let block = render_exact_long_term_memory_block(&store, &slot, 512).unwrap();

        assert!(block.contains("Evidence:"));
        assert!(block.contains("Provenance:"));
        assert!(block.contains("Citations:"));
    }

    #[test]
    fn slot_lookup_returns_nearby_candidates_when_exact_slot_missing() {
        let store = StubLongTermMemoryStore {
            recall_entries: Vec::new(),
            list_entries: vec![
                test_entry(
                    "ltm-1",
                    LongTermMemoryKind::Project,
                    "current_project_status",
                    "Current project status is memory coordination.",
                    vec!["project"],
                    Some("chat-a"),
                    10,
                    20,
                ),
                test_entry(
                    "ltm-2",
                    LongTermMemoryKind::Task,
                    "current_project_task",
                    "Current task is continuity export.",
                    vec!["task"],
                    Some("chat-a"),
                    10,
                    30,
                ),
            ],
        };

        let lookup = lookup_long_term_memory_slot(
            &store,
            &LongTermMemorySlot {
                kind: LongTermMemoryKind::Project,
                topic: "current_project".to_string(),
            },
            3,
        )
        .unwrap();

        assert!(lookup.entry.is_none());
        assert_eq!(lookup.nearby_candidates.len(), 2);
        assert_eq!(
            lookup.nearby_candidates[0].kind,
            LongTermMemoryKind::Project
        );
    }

    #[test]
    fn canonicalize_long_term_memory_entry_defaults_evidence_fields() {
        let entry = canonicalize_long_term_memory_entry(LongTermMemoryEntry {
            id: "ltm-1".to_string(),
            kind: LongTermMemoryKind::Fact,
            topic: "release_phase".to_string(),
            content: "Current phase is memory coordination.".to_string(),
            keywords: vec![],
            source_chat_id: None,
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::World,
            confidence: LongTermMemoryConfidence::Medium,
            freshness: LongTermMemoryFreshness::Dynamic,
            stale_hint: LongTermMemoryStaleHint::ReviewBeforeUse,
            supporting_citations: vec![
                " transcript:chat-a#message=3 ".to_string(),
                "transcript:chat-a#message=3".to_string(),
            ],
            evidence_count: 0,
            created_at: 10,
            updated_at: 10,
            observed_at: 12,
            last_confirmed_at: 0,
            source_revision: 0,
            last_used_at: 0,
        })
        .unwrap();

        assert_eq!(
            entry.supporting_citations,
            vec!["transcript:chat-a#message=3"]
        );
        assert_eq!(entry.evidence_count, 1);
        assert_eq!(entry.last_confirmed_at, 12);
    }

    #[test]
    fn recall_score_prefers_same_chat_and_recent_updates_after_match() {
        let base = test_entry(
            "ltm-1",
            LongTermMemoryKind::Project,
            "current_project",
            "Current project is Beetle long-term memory.",
            vec!["beetle", "memory"],
            Some("chat-a"),
            10,
            90,
        );
        let mut older = base.clone();
        older.source_chat_id = Some("chat-b".to_string());
        older.updated_at = 10;

        let preferred = score_long_term_memory_recall("memory project", Some("chat-a"), 100, &base);
        let fallback = score_long_term_memory_recall("memory project", Some("chat-a"), 100, &older);
        assert!(preferred > fallback);
    }

    #[test]
    fn governance_prunes_stale_task_entries() {
        let mut entries = vec![
            test_entry(
                "ltm-task",
                LongTermMemoryKind::Task,
                "current_task",
                "Continue old task",
                vec![],
                None,
                1,
                1,
            ),
            test_entry(
                "ltm-pref",
                LongTermMemoryKind::Preference,
                "response_style",
                "Prefer direct answers",
                vec![],
                None,
                1,
                1,
            ),
        ];

        assert!(govern_long_term_memory_entries(
            &mut entries,
            LONG_TERM_MEMORY_TASK_TTL_SECS + 2
        ));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "ltm-pref");
    }

    #[test]
    fn governance_keeps_newest_entries_within_kind_budget() {
        let mut entries = Vec::new();
        for idx in 0..14u64 {
            entries.push(test_entry(
                &format!("ltm-task-{idx}"),
                LongTermMemoryKind::Task,
                &format!("task_{idx}"),
                &format!("Task {idx}"),
                vec![],
                None,
                idx,
                idx,
            ));
        }

        assert!(govern_long_term_memory_entries(&mut entries, 0));
        assert_eq!(entries.len(), 12);
        assert_eq!(entries[0].id, "ltm-task-13");
        assert_eq!(entries[11].id, "ltm-task-2");
    }

    #[test]
    fn recall_block_uses_fallback_and_deduplicates_entries() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-task",
                LongTermMemoryKind::Task,
                "current_focus",
                "Continue memory redesign",
                vec!["memory"],
                Some("chat-1"),
                1,
                10,
            )],
            list_entries: vec![
                test_entry(
                    "ltm-task",
                    LongTermMemoryKind::Task,
                    "current_focus",
                    "Continue memory redesign",
                    vec!["memory"],
                    Some("chat-1"),
                    1,
                    10,
                ),
                test_entry(
                    "ltm-project",
                    LongTermMemoryKind::Project,
                    "platform_memory",
                    "Shared memory policy stays aligned.",
                    vec!["memory", "platform"],
                    Some("chat-1"),
                    2,
                    9,
                ),
                test_entry(
                    "ltm-pref",
                    LongTermMemoryKind::Preference,
                    "response_style",
                    "User prefers direct technical answers.",
                    vec!["direct"],
                    None,
                    3,
                    8,
                ),
            ],
        };

        let block = recall_long_term_memory_block(
            &store,
            "chat-1",
            "继续",
            Some("当前重点是长期记忆和 agent loop"),
            &[
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我们正在做长期记忆收口".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续按这个方向推进".to_string(),
                },
            ],
            4096,
            MemoryProfile::Standard,
        )
        .expect("rendered long-term memory block");

        assert_eq!(block.matches("[task:current_focus]").count(), 1);
        assert!(block.contains("### Active context"));
        assert!(block.contains("### User profile"));
        assert!(block.contains("[project:platform_memory]"));
        assert!(block.contains("[preference:response_style]"));
    }

    #[test]
    fn recall_query_uses_summary_and_recent_grounding_for_weak_queries() {
        let query = memory_policy(MemoryProfile::Standard)
            .long_term_recall
            .build_recall_query(
                "继续",
                Some("当前重点是长期记忆和 agent loop"),
                &[SessionMessage {
                    role: "assistant".to_string(),
                    content: "上一轮重点是 Linux 侧长期记忆".to_string(),
                }],
            );
        assert!(query.contains("继续"));
        assert!(query.contains("长期记忆"));
        assert!(query.contains("Linux 侧长期记忆"));
    }

    #[test]
    fn recall_block_prefers_same_chat_active_context_over_other_chat_projects() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![
                test_entry(
                    "ltm-other-project",
                    LongTermMemoryKind::Project,
                    "other_project",
                    "Other chat project with strong memory keywords.",
                    vec!["memory", "linux"],
                    Some("chat-2"),
                    1,
                    10,
                ),
                test_entry(
                    "ltm-current-task",
                    LongTermMemoryKind::Task,
                    "current_focus",
                    "Current chat is closing the memory pipeline.",
                    vec!["memory", "pipeline"],
                    Some("chat-1"),
                    2,
                    9,
                ),
                test_entry(
                    "ltm-pref",
                    LongTermMemoryKind::Preference,
                    "response_style",
                    "User prefers direct technical answers.",
                    vec!["direct"],
                    None,
                    3,
                    8,
                ),
            ],
            ..Default::default()
        };

        let block = recall_long_term_memory_block(
            &store,
            "chat-1",
            "memory",
            Some("当前重点是 memory pipeline"),
            &[SessionMessage {
                role: "user".to_string(),
                content: "继续把 memory pipeline 收掉".to_string(),
            }],
            220,
            MemoryProfile::Standard,
        )
        .expect("rendered block");

        assert!(block.contains("[task:current_focus]"));
        assert!(!block.contains("[project:other_project]"));
    }

    #[test]
    fn select_entries_prefers_diversity_first() {
        let candidates = vec![
            test_entry(
                "ltm-1",
                LongTermMemoryKind::Task,
                "current_focus",
                "Continue memory redesign",
                vec![],
                Some("chat-1"),
                1,
                10,
            ),
            test_entry(
                "ltm-2",
                LongTermMemoryKind::Task,
                "next_task",
                "Integrate prompt-guided runtime",
                vec![],
                Some("chat-1"),
                2,
                9,
            ),
            test_entry(
                "ltm-3",
                LongTermMemoryKind::Preference,
                "response_style",
                "User prefers direct technical answers.",
                vec![],
                None,
                3,
                8,
            ),
        ];

        let selected = memory_policy(MemoryProfile::Standard)
            .long_term_recall
            .select_entries(candidates, 2);
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].topic, "current_focus");
        assert_eq!(selected[1].topic, "response_style");
    }
}
