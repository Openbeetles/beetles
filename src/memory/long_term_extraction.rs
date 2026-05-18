//! 长期记忆提取调度与轻量状态。
//! Long-term memory extraction scheduling and lightweight state.
#![allow(clippy::too_many_arguments)]

use crate::bus::IngressKind;
use crate::error::Error;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::platform::SkillStorage;
use crate::skills::{write_governed_runtime_skills, RuntimeSkillWrite, RuntimeSkillWriteSource};
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::{
    build_archive_evidence_block, memory_policy, render_long_term_memory_block,
    run_memory_governance_kernel, search_archive_records, write_governed_shared_memory,
    ArchiveRecordSource, ArchiveSearchHit, ArchiveSearchQuery, LongTermExtractionPolicy,
    LongTermMemoryConfidence, LongTermMemoryDraft, LongTermMemoryEntry, LongTermMemoryFreshness,
    LongTermMemoryKind, LongTermMemorySlot, LongTermMemorySourceScope, LongTermMemorySourceType,
    LongTermMemoryStaleHint, LongTermMemoryStore, MemoryGovernanceContext, MemoryGovernanceInput,
    MemoryProfile, MemoryStore, SessionMessage, SessionStore, SessionSummaryStore,
    SharedMemoryWriteSource, TurnLedgerStore, MAX_LONG_TERM_MEMORY_ITEMS,
};

/// 长期记忆提取状态存储路径（相对状态根）。
pub const REL_PATH_LONG_TERM_EXTRACTION_STATES: &str = "memory/long_term_extraction_states.json";
pub const LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT: &str = "You extract durable memory updates for a personal AI assistant. Return JSON only: an array of objects. Each object must contain plane plus topic. plane must be factual, skill, or ignore. Use factual for canonical shared facts: durable user profile facts, stable preferences, durable constraints, ongoing project/task state, and durable external facts. Use skill for procedural experience, operating routines, tool-use know-how, setup playbooks, or reusable workflows that should not pollute canonical factual memory. Use ignore when nothing durable should be written. For plane=factual, also provide op, kind, and optionally content and keywords. op must be upsert or delete. kind must be one of preference, profile, relationship, project, task, constraint, fact. Reuse an existing factual topic whenever the same durable slot is being updated or corrected. For plane=skill, provide content and optional skill_summary; content should be a compact reusable procedure, not a transcript. For plane=ignore, no extra fields are needed. Do not store greetings, one-off troubleshooting steps as factual memory, short acknowledgements, temporary moods, assistant-only claims, secrets, credentials, raw tool payloads, copied log fragments, or long external document excerpts. Treat archive evidence sources as supporting records rather than canonical memory: they may justify a durable conclusion, but they are not themselves a fact slot. Prefer newer transcript evidence over older archive fragments when they disagree. When project/task context shifts, update the existing active factual slot instead of creating a parallel near-duplicate slot. Use the provided session summary, existing long-term memory, and archive evidence as grounding when deciding whether to upsert, delete, reroute to skill, or ignore. Keep only the highest-value durable changes, at most 4 items. If there is nothing durable to add, update, or delete, return [].";
/// 共享策略允许的 recent 消息窗口上限；实际运行值由 MemoryProfile 决定。
pub const LONG_TERM_MEMORY_EXTRACTION_RECENT_N: usize = 10;
/// 单次提取允许的动作数上限；实际运行值由 MemoryProfile 决定。
pub const LONG_TERM_MEMORY_EXTRACTION_BATCH: usize = 4;
const LONG_TERM_MEMORY_ARCHIVE_RECONCILE_LIMIT: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LongTermMemoryExtractionState {
    #[serde(default)]
    pub dirty_since_count: usize,
    #[serde(default)]
    pub dirty_turns: u8,
    #[serde(default)]
    pub last_requested_at_count: usize,
    #[serde(default)]
    pub last_processed_at_count: usize,
    #[serde(default)]
    pub pending: bool,
}

impl LongTermMemoryExtractionState {
    pub fn has_dirty_work(&self) -> bool {
        self.dirty_since_count > 0 && self.dirty_turns > 0
    }

    fn mark_dirty(&mut self, after_count: usize) {
        if self.dirty_since_count == 0 {
            self.dirty_since_count = after_count;
        }
        self.dirty_turns = self.dirty_turns.saturating_add(1);
    }
}

pub trait LongTermMemoryExtractionStateStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>>;
    fn set(&self, chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

#[derive(Clone, Copy)]
pub struct LongTermMemoryExtractionTurnInput<'a> {
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub after_count: usize,
    pub pressure: PressureLevel,
    pub external_content_used: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryExtractionTurnDecision {
    pub next_state: LongTermMemoryExtractionState,
    pub should_enqueue: bool,
}

impl LongTermExtractionPolicy {
    fn is_eligible_turn(self, input: LongTermMemoryExtractionTurnInput<'_>) -> bool {
        if input.ingress != IngressKind::User || input.channel == "cron" {
            return false;
        }
        if input.pressure != PressureLevel::Normal {
            return false;
        }
        if input.external_content_used {
            return false;
        }
        !input.user_content.trim().is_empty() && !input.reply_content.trim().is_empty()
    }

    fn marks_dirty(self, user_content: &str, reply_content: &str) -> bool {
        let user = user_content.trim();
        let reply = reply_content.trim();
        if user.is_empty() || reply.is_empty() {
            return false;
        }

        let user_chars = user.chars().count();
        let reply_chars = reply.chars().count();
        let user_words = user.split_whitespace().count();
        let combined_chars = user_chars.saturating_add(reply_chars);

        if user_chars <= self.low_signal_user_chars
            && user_words <= self.low_signal_user_words
            && reply_chars <= self.low_signal_reply_chars
            && !reply.contains('\n')
        {
            return false;
        }

        user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined_chars >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n')
    }

    fn cooldown_ready(self, state: &LongTermMemoryExtractionState, after_count: usize) -> bool {
        after_count.saturating_sub(state.last_requested_at_count)
            >= self.min_messages_between_requests
    }

    fn should_process_dirty_work(
        self,
        state: &LongTermMemoryExtractionState,
        after_count: usize,
    ) -> bool {
        if !state.has_dirty_work() {
            return false;
        }
        if state.dirty_turns >= 2 {
            return true;
        }
        if state.last_processed_at_count == 0 && after_count >= self.first_process_min_messages {
            return true;
        }
        after_count.saturating_sub(state.dirty_since_count) >= self.force_process_after_messages
    }
}

pub fn evaluate_long_term_memory_extraction_turn(
    input: LongTermMemoryExtractionTurnInput<'_>,
    state: Option<&LongTermMemoryExtractionState>,
    profile: MemoryProfile,
) -> LongTermMemoryExtractionTurnDecision {
    let policy = memory_policy(profile).long_term_extraction;
    let mut next_state = state.cloned().unwrap_or_default();
    if !policy.is_eligible_turn(input) {
        return LongTermMemoryExtractionTurnDecision {
            next_state,
            should_enqueue: false,
        };
    }
    if policy.marks_dirty(input.user_content, input.reply_content) {
        next_state.mark_dirty(input.after_count);
    }
    let should_enqueue = !next_state.pending
        && policy.cooldown_ready(&next_state, input.after_count)
        && policy.should_process_dirty_work(&next_state, input.after_count);
    LongTermMemoryExtractionTurnDecision {
        next_state,
        should_enqueue,
    }
}

pub fn mark_long_term_memory_extraction_requested(
    state: &LongTermMemoryExtractionState,
    after_count: usize,
) -> LongTermMemoryExtractionState {
    let mut next_state = state.clone();
    next_state.pending = true;
    next_state.last_requested_at_count = next_state.last_requested_at_count.max(after_count);
    next_state
}

pub fn mark_long_term_memory_extraction_processed(
    state: Option<&LongTermMemoryExtractionState>,
    after_count: usize,
) -> LongTermMemoryExtractionState {
    let mut next_state = state.cloned().unwrap_or_default();
    next_state.pending = false;
    next_state.dirty_since_count = 0;
    next_state.dirty_turns = 0;
    next_state.last_processed_at_count = next_state.last_processed_at_count.max(after_count);
    next_state
}

pub fn mark_long_term_memory_extraction_deferred(
    state: Option<&LongTermMemoryExtractionState>,
) -> LongTermMemoryExtractionState {
    let mut next_state = state.cloned().unwrap_or_default();
    next_state.pending = false;
    next_state
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct ParsedLongTermMemoryExtraction {
    pub upserts: Vec<LongTermMemoryDraft>,
    pub deletes: Vec<LongTermMemorySlot>,
    pub skill_writes: Vec<RuntimeSkillWrite>,
}

#[derive(Deserialize)]
struct LongTermMemoryExtractionItem {
    #[serde(default)]
    plane: Option<String>,
    #[serde(default = "default_long_term_memory_extraction_op")]
    op: String,
    #[serde(default)]
    kind: Option<LongTermMemoryKind>,
    topic: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    source_chat_id: Option<String>,
    #[serde(default)]
    source_type: Option<LongTermMemorySourceType>,
    #[serde(default)]
    source_scope: Option<LongTermMemorySourceScope>,
    #[serde(default)]
    confidence: Option<super::LongTermMemoryConfidence>,
    #[serde(default)]
    freshness: Option<LongTermMemoryFreshness>,
    #[serde(default)]
    stale_hint: Option<LongTermMemoryStaleHint>,
    #[serde(default)]
    skill_summary: String,
}

enum ParsedLongTermMemoryAction {
    Upsert(LongTermMemoryDraft),
    Delete(LongTermMemorySlot),
    Skill(RuntimeSkillWrite),
}

fn default_long_term_memory_extraction_op() -> String {
    "upsert".to_string()
}

pub fn build_long_term_memory_extraction_input(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    recent: &[SessionMessage],
    session_summary: Option<&str>,
    factual_governance_brief: Option<&str>,
    archive_evidence: Option<&str>,
    profile: MemoryProfile,
) -> String {
    let policy = memory_policy(profile).long_term_extraction;
    let include_thick_grounding = long_term_memory_extraction_uses_thick_grounding(profile);
    let transcript = build_long_term_memory_extraction_transcript(recent, policy);
    let existing_memory = store
        .recall(
            &transcript,
            Some(chat_id),
            policy.batch_size.min(LONG_TERM_MEMORY_EXTRACTION_BATCH),
        )
        .ok()
        .and_then(|entries| {
            build_extraction_existing_memory_grounding(&entries, policy.existing_memory_max_len)
        });

    let mut input = String::with_capacity(2300);
    if let Some(summary) = session_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        input.push_str("## Session summary\n");
        input.push_str(summary);
        input.push_str("\n\n");
    }

    if let Some(memory) = existing_memory {
        input.push_str(&memory);
        input.push_str("\n\n");
    }

    let factual_governance_brief = if include_thick_grounding {
        factual_governance_brief
    } else {
        None
    };
    if let Some(factual_governance_brief) = factual_governance_brief
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        input.push_str(factual_governance_brief);
        input.push_str("\n\n");
    }

    let archive_evidence = if include_thick_grounding {
        archive_evidence
    } else {
        None
    };
    if let Some(archive_evidence) = archive_evidence
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        input.push_str(archive_evidence);
        input.push_str("\n\n");
    }

    input.push_str("## Recent conversation\n");
    input.push_str(transcript.trim());
    input
}

fn long_term_memory_extraction_uses_thick_grounding(profile: MemoryProfile) -> bool {
    matches!(profile, MemoryProfile::Standard)
}

fn build_extraction_existing_memory_grounding(
    entries: &[LongTermMemoryEntry],
    max_len: usize,
) -> Option<String> {
    let mut out = String::new();
    out.push_str("## Existing memory slots\n");
    for entry in entries {
        let mut meta = vec![
            entry.source_type.label().to_string(),
            entry.confidence.label().to_string(),
            entry.source_scope.label().to_string(),
        ];
        if !matches!(entry.freshness, LongTermMemoryFreshness::Stable) {
            meta.push(entry.freshness.label().to_string());
        }
        if let Some(label) = entry.stale_hint.label() {
            meta.push(label.to_string());
        }
        if entry.evidence_count > 0 || !entry.supporting_citations.is_empty() {
            meta.push(format!(
                "evidence={}",
                entry
                    .evidence_count
                    .max(entry.supporting_citations.len() as u32)
            ));
        }
        if let Some(citation) = entry.supporting_citations.first() {
            let suffix = entry.supporting_citations.len().saturating_sub(1);
            if suffix > 0 {
                meta.push(format!("cite={} +{}", citation, suffix));
            } else {
                meta.push(format!("cite={}", citation));
            }
        }
        let line = format!(
            "- {}.{} => {} ({})",
            entry.kind.label(),
            entry.topic,
            entry.content,
            meta.join("; ")
        );
        if out.len().saturating_add(line.len()).saturating_add(1) > max_len {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    if out.trim() == "## Existing memory slots" {
        render_long_term_memory_block(entries, max_len)
    } else {
        Some(out.trim_end().to_string())
    }
}

pub fn parse_long_term_memory_extraction_response(
    raw: &str,
    chat_id: &str,
) -> ParsedLongTermMemoryExtraction {
    let trimmed = raw.trim();
    let json_slice = if trimmed.starts_with('[') {
        trimmed
    } else {
        match (trimmed.find('['), trimmed.rfind(']')) {
            (Some(start), Some(end)) if start < end => &trimmed[start..=end],
            _ => return ParsedLongTermMemoryExtraction::default(),
        }
    };
    let parsed = serde_json::from_str::<Vec<serde_json::Value>>(json_slice).unwrap_or_default();
    let mut actions = Vec::with_capacity(parsed.len().min(LONG_TERM_MEMORY_EXTRACTION_BATCH));
    let mut slot_indexes =
        HashMap::with_capacity(parsed.len().min(LONG_TERM_MEMORY_EXTRACTION_BATCH));
    for item in parsed {
        let Ok(mut parsed_item) = serde_json::from_value::<LongTermMemoryExtractionItem>(item)
        else {
            continue;
        };
        let plane = parsed_item
            .plane
            .as_deref()
            .map(str::trim)
            .map(|value| value.to_ascii_lowercase());
        if matches!(plane.as_deref(), Some("ignore")) {
            continue;
        }
        let action = match parsed_item.op.trim().to_ascii_lowercase().as_str() {
            _ if matches!(plane.as_deref(), Some("skill")) => {
                ParsedLongTermMemoryAction::Skill(RuntimeSkillWrite {
                    name: crate::skills::runtime_skill_name_for_topic(&parsed_item.topic),
                    topic: parsed_item.topic.clone(),
                    title: parsed_item.topic.replace('_', " "),
                    summary: truncate_content_to_max(
                        if parsed_item.skill_summary.trim().is_empty() {
                            parsed_item.content.trim()
                        } else {
                            parsed_item.skill_summary.trim()
                        },
                        160,
                    )
                    .into_owned(),
                    content: parsed_item.content,
                    citations: Vec::new(),
                    source_chat_id: parsed_item
                        .source_chat_id
                        .or_else(|| Some(chat_id.to_string())),
                    observed_at: 0,
                })
            }
            "delete" => {
                let Some(kind) = parsed_item.kind else {
                    continue;
                };
                ParsedLongTermMemoryAction::Delete(LongTermMemorySlot {
                    kind,
                    topic: parsed_item.topic,
                })
            }
            "upsert" => {
                let Some(kind) = parsed_item.kind else {
                    continue;
                };
                if parsed_item.source_chat_id.is_none() {
                    parsed_item.source_chat_id = Some(chat_id.to_string());
                }
                ParsedLongTermMemoryAction::Upsert(LongTermMemoryDraft {
                    kind,
                    topic: parsed_item.topic,
                    content: parsed_item.content,
                    keywords: parsed_item.keywords,
                    source_chat_id: parsed_item.source_chat_id,
                    source_type: parsed_item
                        .source_type
                        .or(Some(LongTermMemorySourceType::Conversation)),
                    source_scope: parsed_item.source_scope,
                    confidence: parsed_item.confidence,
                    freshness: parsed_item.freshness,
                    stale_hint: parsed_item.stale_hint,
                    supporting_citations: Vec::new(),
                    evidence_count: None,
                    observed_at: None,
                    last_confirmed_at: None,
                    source_revision: None,
                })
            }
            _ => continue,
        };
        let slot_id = match &action {
            ParsedLongTermMemoryAction::Upsert(draft) => draft.stable_id(),
            ParsedLongTermMemoryAction::Delete(slot) => slot.stable_id(),
            ParsedLongTermMemoryAction::Skill(write) => Some(format!("skill:{}", write.name)),
        };
        let Some(slot_id) = slot_id else {
            continue;
        };
        if let Some(existing_idx) = slot_indexes.get(&slot_id).copied() {
            actions[existing_idx] = action;
        } else {
            slot_indexes.insert(slot_id, actions.len());
            actions.push(action);
        }
        if actions.len() >= LONG_TERM_MEMORY_EXTRACTION_BATCH {
            break;
        }
    }
    let mut upserts = Vec::with_capacity(actions.len());
    let mut deletes = Vec::with_capacity(actions.len());
    let mut skill_writes = Vec::with_capacity(actions.len());
    for action in actions {
        match action {
            ParsedLongTermMemoryAction::Upsert(draft) => upserts.push(draft),
            ParsedLongTermMemoryAction::Delete(slot) => deletes.push(slot),
            ParsedLongTermMemoryAction::Skill(write) => skill_writes.push(write),
        }
    }
    ParsedLongTermMemoryExtraction {
        upserts,
        deletes,
        skill_writes,
    }
}

fn build_draft_archive_reconcile_query(
    draft: &LongTermMemoryDraft,
    recent: &[SessionMessage],
    session_summary: Option<&str>,
) -> String {
    let mut parts = Vec::with_capacity(4);
    parts.push(format!("{} {}", draft.topic, draft.content));
    if !draft.keywords.is_empty() {
        parts.push(draft.keywords.join(" "));
    }
    if let Some(summary) = session_summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        parts.push(truncate_content_to_max(summary, 160).to_string());
    }
    if let Some(user_message) = recent
        .iter()
        .rev()
        .find(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| message.content.trim())
        .filter(|value| !value.is_empty())
    {
        parts.push(truncate_content_to_max(user_message, 160).to_string());
    }
    parts.join("\n")
}

fn archive_hit_affinity_score(draft: &LongTermMemoryDraft, hit: &ArchiveSearchHit) -> u32 {
    let mut score = 0u32;
    let draft_terms = collect_affinity_terms(draft);
    let hit_text = normalize_match_text(&format!(
        "{} {} {} {}",
        hit.title,
        hit.excerpt,
        hit.citation,
        hit.cues.join(" ")
    ));
    let draft_topic = normalize_match_text(&draft.topic);
    let draft_content = normalize_match_text(&draft.content);
    if !draft_topic.is_empty() && hit_text.contains(&draft_topic) {
        score = score.saturating_add(6);
    }
    if !draft_content.is_empty()
        && (hit_text.contains(&draft_content)
            || long_text_contains(&draft_content, &hit_text)
            || long_text_contains(&hit_text, &draft_content))
    {
        score = score.saturating_add(8);
    }
    let overlap = draft_terms
        .iter()
        .filter(|term| hit_text.contains(term.as_str()))
        .count()
        .min(4) as u32;
    score
        .saturating_add(overlap.saturating_mul(2))
        .saturating_add(hit.score / 10)
}

fn select_archive_reconcile_hits(
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    draft: &LongTermMemoryDraft,
    recent: &[SessionMessage],
    session_summary: Option<&str>,
    chat_id: &str,
) -> Vec<ArchiveSearchHit> {
    let query = build_draft_archive_reconcile_query(draft, recent, session_summary);
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut hits = search_archive_records(
        session_store,
        memory_store,
        turn_ledger_store,
        ArchiveSearchQuery {
            query: &query,
            preferred_chat_id: Some(chat_id),
            chat_id_filter: None,
            sources: &[
                ArchiveRecordSource::Transcript,
                ArchiveRecordSource::DailyNote,
                ArchiveRecordSource::TurnLog,
            ],
            limit: LONG_TERM_MEMORY_ARCHIVE_RECONCILE_LIMIT,
        },
    )
    .unwrap_or_default()
    .into_iter()
    .filter_map(|hit| {
        let affinity = archive_hit_affinity_score(draft, &hit);
        (affinity >= 6).then_some((hit, affinity))
    })
    .collect::<Vec<_>>();
    hits.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.score.cmp(&a.0.score))
            .then_with(|| b.0.observed_at.cmp(&a.0.observed_at))
    });
    hits.into_iter()
        .map(|(hit, _)| hit)
        .take(LONG_TERM_MEMORY_ARCHIVE_RECONCILE_LIMIT)
        .collect()
}

fn enrich_drafts_with_archive_evidence(
    drafts: &mut [LongTermMemoryDraft],
    session_store: &dyn SessionStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    recent: &[SessionMessage],
    session_summary: Option<&str>,
    now_secs: u64,
) {
    for draft in drafts {
        let hits = select_archive_reconcile_hits(
            session_store,
            memory_store,
            turn_ledger_store,
            draft,
            recent,
            session_summary,
            chat_id,
        );
        if hits.is_empty() {
            continue;
        }
        let support_count = hits
            .iter()
            .filter(|hit| archive_hit_supports_draft(draft, hit))
            .count();
        let conflict_count = hits
            .iter()
            .filter(|hit| archive_hit_conflicts_draft(draft, hit))
            .count();
        let mut citations = Vec::with_capacity(hits.len());
        let mut last_confirmed_at = 0u64;
        let mut used_now_confirmation = false;
        for hit in hits {
            if citations.iter().any(|existing| existing == &hit.citation) {
                continue;
            }
            citations.push(hit.citation);
            match hit.observed_at {
                Some(observed_at) if observed_at > 0 => {
                    last_confirmed_at = last_confirmed_at.max(observed_at);
                }
                _ => used_now_confirmation = true,
            }
        }
        if citations.is_empty() {
            continue;
        }
        draft.supporting_citations = citations;
        draft.evidence_count = Some(draft.supporting_citations.len() as u32);
        if used_now_confirmation {
            last_confirmed_at = last_confirmed_at.max(now_secs);
        }
        if last_confirmed_at > 0 {
            draft.last_confirmed_at = Some(last_confirmed_at);
        }
        if support_count >= 3 {
            elevate_draft_confidence(draft, LongTermMemoryConfidence::High);
        } else if support_count >= 1 {
            elevate_draft_confidence(draft, LongTermMemoryConfidence::Medium);
        }
        if conflict_count > support_count && conflict_count > 0 {
            lower_draft_confidence(draft, LongTermMemoryConfidence::Low);
            draft
                .stale_hint
                .get_or_insert(LongTermMemoryStaleHint::ReviewBeforeUse);
        }
        if draft.freshness.is_none() {
            draft.freshness = Some(match draft.kind {
                LongTermMemoryKind::Project | LongTermMemoryKind::Task => {
                    LongTermMemoryFreshness::Dynamic
                }
                _ => LongTermMemoryFreshness::Stable,
            });
        }
    }
}

fn archive_hit_supports_draft(draft: &LongTermMemoryDraft, hit: &ArchiveSearchHit) -> bool {
    archive_hit_affinity_score(draft, hit) >= 10
}

fn archive_hit_conflicts_draft(draft: &LongTermMemoryDraft, hit: &ArchiveSearchHit) -> bool {
    let draft_topic = normalize_match_text(&draft.topic);
    let draft_terms = collect_affinity_terms(draft);
    let hit_text = normalize_match_text(&format!(
        "{} {} {}",
        hit.title,
        hit.excerpt,
        hit.cues.join(" ")
    ));
    let topic_overlap = !draft_topic.is_empty() && hit_text.contains(&draft_topic);
    let term_overlap = draft_terms
        .iter()
        .filter(|term| hit_text.contains(term.as_str()))
        .count();
    topic_overlap && term_overlap <= 1 && archive_hit_affinity_score(draft, hit) < 10
}

fn elevate_draft_confidence(draft: &mut LongTermMemoryDraft, target: LongTermMemoryConfidence) {
    let next = match (
        draft.confidence.unwrap_or(LongTermMemoryConfidence::Low),
        target,
    ) {
        (LongTermMemoryConfidence::High, _) | (_, LongTermMemoryConfidence::Low) => {
            draft.confidence.unwrap_or(target)
        }
        (LongTermMemoryConfidence::Medium, LongTermMemoryConfidence::High) => {
            LongTermMemoryConfidence::High
        }
        (LongTermMemoryConfidence::Low, desired) => desired,
        (existing, _) => existing,
    };
    draft.confidence = Some(next);
}

fn lower_draft_confidence(draft: &mut LongTermMemoryDraft, target: LongTermMemoryConfidence) {
    let next = match (
        draft.confidence.unwrap_or(LongTermMemoryConfidence::Medium),
        target,
    ) {
        (_, LongTermMemoryConfidence::High) => LongTermMemoryConfidence::High,
        (LongTermMemoryConfidence::Low, _) => LongTermMemoryConfidence::Low,
        (LongTermMemoryConfidence::Medium, LongTermMemoryConfidence::Low)
        | (LongTermMemoryConfidence::High, LongTermMemoryConfidence::Low) => {
            LongTermMemoryConfidence::Low
        }
        (existing, _) => existing,
    };
    draft.confidence = Some(next);
}

pub fn apply_long_term_memory_extraction(
    store: &dyn LongTermMemoryStore,
    skill_storage: &dyn SkillStorage,
    extraction: &ParsedLongTermMemoryExtraction,
    now_secs: u64,
) -> Result<usize> {
    let mut changed = 0usize;
    for slot in &extraction.deletes {
        if store.delete_slot(slot)? {
            changed += 1;
        }
    }
    if !extraction.upserts.is_empty() {
        changed += write_governed_shared_memory(
            store,
            &extraction.upserts,
            now_secs,
            SharedMemoryWriteSource::Extraction,
        )?
        .changed;
    }
    if !extraction.skill_writes.is_empty() {
        changed += write_governed_runtime_skills(
            skill_storage,
            &extraction.skill_writes,
            RuntimeSkillWriteSource::Extraction,
        )?
        .changed;
    }
    Ok(changed)
}

pub fn prepare_long_term_memory_extraction(
    store: &dyn LongTermMemoryStore,
    extraction: &ParsedLongTermMemoryExtraction,
    chat_id: &str,
) -> ParsedLongTermMemoryExtraction {
    let existing_entries = store.list(MAX_LONG_TERM_MEMORY_ITEMS).unwrap_or_default();
    let mut upsert_slots = HashMap::with_capacity(extraction.upserts.len());
    let mut protected_slots = HashSet::with_capacity(extraction.upserts.len());
    let mut upserts = Vec::with_capacity(extraction.upserts.len());
    let mut skill_writes = Vec::with_capacity(
        extraction
            .skill_writes
            .len()
            .saturating_add(extraction.upserts.len()),
    );
    let mut skill_names = HashMap::with_capacity(skill_writes.len());
    for draft in &extraction.upserts {
        let routed = super::route_long_term_draft(draft);
        if let Some(write) = routed.skill_write {
            if let Some(existing_idx) = skill_names.get(&write.name).copied() {
                skill_writes[existing_idx] = write;
            } else {
                skill_names.insert(write.name.clone(), skill_writes.len());
                skill_writes.push(write);
            }
            continue;
        }
        let Some(mut normalized) = routed.factual_draft else {
            continue;
        };
        if !should_keep_durable_draft(&normalized) {
            continue;
        }
        if let Some(entry) = resolve_existing_slot_match(&existing_entries, &normalized, chat_id) {
            normalized.topic = entry.topic.clone();
        }
        let Some(slot_id) = normalized.stable_id() else {
            continue;
        };
        protected_slots.insert(slot_id.clone());
        if should_skip_redundant_upsert(&normalized, &existing_entries) {
            continue;
        }
        if let Some(index) = upsert_slots.get(&slot_id).copied() {
            upserts[index] = normalized;
        } else {
            upsert_slots.insert(slot_id, upserts.len());
            upserts.push(normalized);
        }
    }
    for write in &extraction.skill_writes {
        if let Some(existing_idx) = skill_names.get(&write.name).copied() {
            skill_writes[existing_idx] = write.clone();
        } else {
            skill_names.insert(write.name.clone(), skill_writes.len());
            skill_writes.push(write.clone());
        }
    }

    let mut deletes = Vec::with_capacity(extraction.deletes.len().saturating_add(upserts.len()));
    let mut delete_slots = HashMap::with_capacity(extraction.deletes.len());
    for slot in &extraction.deletes {
        let Some(normalized) = slot.normalized() else {
            continue;
        };
        let Some(slot_id) = normalized.stable_id() else {
            continue;
        };
        if protected_slots.contains(&slot_id) {
            continue;
        }
        if delete_slots.contains_key(&slot_id) {
            continue;
        }
        delete_slots.insert(slot_id, deletes.len());
        deletes.push(normalized);
    }
    for draft in &upserts {
        let Some(draft_slot_id) = draft.stable_id() else {
            continue;
        };
        let primary_entry = existing_entries
            .iter()
            .find(|entry| entry_slot_id(entry).as_deref() == Some(draft_slot_id.as_str()));
        for entry in &existing_entries {
            if !should_delete_superseded_entry(entry, draft, primary_entry, &draft_slot_id, chat_id)
            {
                continue;
            }
            let slot = LongTermMemorySlot {
                kind: entry.kind.clone(),
                topic: entry.topic.clone(),
            };
            let Some(slot_id) = slot.stable_id() else {
                continue;
            };
            if delete_slots.contains_key(&slot_id) {
                continue;
            }
            delete_slots.insert(slot_id, deletes.len());
            deletes.push(slot);
        }
    }

    ParsedLongTermMemoryExtraction {
        upserts,
        deletes,
        skill_writes,
    }
}

fn resolve_existing_slot_match<'a>(
    existing_entries: &'a [LongTermMemoryEntry],
    draft: &LongTermMemoryDraft,
    chat_id: &str,
) -> Option<&'a LongTermMemoryEntry> {
    let current_slot_id = draft.stable_id();
    if let Some(existing) = existing_entries
        .iter()
        .find(|entry| current_slot_id.as_deref() == entry_slot_id(entry).as_deref())
    {
        return Some(existing);
    }
    if let Some(existing) = resolve_singleton_active_context_slot(existing_entries, draft, chat_id)
    {
        return Some(existing);
    }
    let mut best: Option<(&LongTermMemoryEntry, u32)> = None;
    for existing in existing_entries {
        if existing.kind != draft.kind {
            continue;
        }
        let score = draft_entry_affinity_score(draft, existing, chat_id);
        if score < 8 {
            continue;
        }
        match best {
            Some((_, best_score)) if best_score >= score => {}
            _ => best = Some((existing, score)),
        }
    }
    best.map(|(entry, _)| entry)
}

fn resolve_singleton_active_context_slot<'a>(
    existing_entries: &'a [LongTermMemoryEntry],
    draft: &LongTermMemoryDraft,
    chat_id: &str,
) -> Option<&'a LongTermMemoryEntry> {
    if !matches!(
        draft.kind,
        LongTermMemoryKind::Project | LongTermMemoryKind::Task
    ) {
        return None;
    }
    let mut candidates = existing_entries.iter().filter(|entry| {
        entry.kind == draft.kind && entry_matches_chat_scope(entry, draft, chat_id)
    });
    let first = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    Some(first)
}

fn should_keep_durable_draft(draft: &LongTermMemoryDraft) -> bool {
    let content = draft.content.trim();
    if content.is_empty() {
        return false;
    }
    if content_contains_sensitive_material(content) {
        return false;
    }
    if !content.chars().any(|ch| ch.is_alphanumeric() || is_cjk(ch)) {
        return false;
    }

    let normalized_content = normalize_match_text(content);
    if normalized_content.is_empty() {
        return false;
    }
    if normalized_content == normalize_match_text(&draft.topic) {
        return false;
    }
    if normalized_content
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch.is_whitespace())
    {
        return false;
    }
    if allows_short_cjk_preference_or_constraint(draft, content) {
        return true;
    }

    let non_space_chars = content.chars().filter(|ch| !ch.is_whitespace()).count();
    let term_count = collect_terms_from_text(content).len();
    let min_chars = minimum_durable_content_chars(&draft.kind);
    let min_terms = minimum_durable_term_count(&draft.kind);
    if non_space_chars < min_chars {
        return false;
    }
    if term_count < min_terms && draft.keywords.is_empty() {
        return false;
    }
    true
}

fn content_contains_sensitive_material(content: &str) -> bool {
    scrub_credentials(content) != content
}

fn minimum_durable_content_chars(kind: &LongTermMemoryKind) -> usize {
    match kind {
        LongTermMemoryKind::Profile | LongTermMemoryKind::Relationship => 2,
        LongTermMemoryKind::Preference | LongTermMemoryKind::Constraint => 4,
        LongTermMemoryKind::Fact => 6,
        LongTermMemoryKind::Project | LongTermMemoryKind::Task => 8,
    }
}

fn minimum_durable_term_count(kind: &LongTermMemoryKind) -> usize {
    match kind {
        LongTermMemoryKind::Profile | LongTermMemoryKind::Relationship => 1,
        LongTermMemoryKind::Preference
        | LongTermMemoryKind::Project
        | LongTermMemoryKind::Task
        | LongTermMemoryKind::Constraint => 2,
        LongTermMemoryKind::Fact => 1,
    }
}

fn allows_short_cjk_preference_or_constraint(draft: &LongTermMemoryDraft, content: &str) -> bool {
    if !matches!(
        draft.kind,
        LongTermMemoryKind::Preference | LongTermMemoryKind::Constraint
    ) {
        return false;
    }
    let mut cjk_chars = 0usize;
    for ch in content.chars() {
        if ch.is_whitespace() || ch.is_ascii_punctuation() {
            continue;
        }
        if !is_cjk(ch) {
            return false;
        }
        cjk_chars += 1;
    }
    (3..=8).contains(&cjk_chars)
}

fn should_delete_superseded_entry(
    entry: &LongTermMemoryEntry,
    draft: &LongTermMemoryDraft,
    primary_entry: Option<&LongTermMemoryEntry>,
    draft_slot_id: &str,
    chat_id: &str,
) -> bool {
    if entry.kind != draft.kind {
        return false;
    }
    if entry_slot_id(entry).as_deref() == Some(draft_slot_id) {
        return false;
    }
    if !entry_matches_chat_scope(entry, draft, chat_id) {
        return false;
    }
    let Some(primary_entry) = primary_entry else {
        return false;
    };
    if !entries_are_parallel_duplicates(primary_entry, entry) {
        return false;
    }
    draft_entry_affinity_score(draft, entry, chat_id) >= 8
}

fn entry_slot_id(entry: &LongTermMemoryEntry) -> Option<String> {
    LongTermMemorySlot {
        kind: entry.kind.clone(),
        topic: entry.topic.clone(),
    }
    .stable_id()
}

fn entry_matches_chat_scope(
    entry: &LongTermMemoryEntry,
    draft: &LongTermMemoryDraft,
    chat_id: &str,
) -> bool {
    entry_scope_rank(entry, draft, chat_id) > 0
}

fn entry_scope_rank(entry: &LongTermMemoryEntry, draft: &LongTermMemoryDraft, chat_id: &str) -> u8 {
    let target_chat = draft.source_chat_id.as_deref().unwrap_or(chat_id);
    match entry.source_chat_id.as_deref() {
        Some(source_chat_id) if source_chat_id == target_chat => 2,
        None => 1,
        _ => 0,
    }
}

fn entries_are_parallel_duplicates(
    primary: &LongTermMemoryEntry,
    candidate: &LongTermMemoryEntry,
) -> bool {
    if primary.kind != candidate.kind {
        return false;
    }
    if entry_slot_id(primary) == entry_slot_id(candidate) {
        return false;
    }
    let primary_content = normalize_match_text(&primary.content);
    let candidate_content = normalize_match_text(&candidate.content);
    if primary_content.is_empty() || candidate_content.is_empty() {
        return false;
    }
    if primary_content == candidate_content {
        return true;
    }
    long_text_contains(&primary_content, &candidate_content)
        || long_text_contains(&candidate_content, &primary_content)
}

fn should_skip_redundant_upsert(
    draft: &LongTermMemoryDraft,
    existing_entries: &[LongTermMemoryEntry],
) -> bool {
    if !draft.supporting_citations.is_empty()
        || draft.evidence_count.unwrap_or(0) > 0
        || draft.last_confirmed_at.unwrap_or(0) > 0
    {
        return false;
    }
    let Some(slot_id) = draft.stable_id() else {
        return true;
    };
    let Some(existing) = existing_entries
        .iter()
        .find(|entry| entry_slot_id(entry).as_deref() == Some(slot_id.as_str()))
    else {
        return false;
    };
    let content_matches =
        normalize_match_text(&draft.content) == normalize_match_text(&existing.content);
    if !content_matches {
        return false;
    }
    draft.keywords.iter().all(|keyword| {
        existing.keywords.iter().any(|existing_keyword| {
            normalize_match_text(existing_keyword) == normalize_match_text(keyword)
        })
    })
}

fn draft_entry_affinity_score(
    draft: &LongTermMemoryDraft,
    entry: &LongTermMemoryEntry,
    chat_id: &str,
) -> u32 {
    let mut score = 0u32;
    let draft_topic = normalize_match_text(&draft.topic);
    let entry_topic = normalize_match_text(&entry.topic);
    let draft_content = normalize_match_text(&draft.content);
    let entry_content = normalize_match_text(&entry.content);
    if !draft_topic.is_empty() && draft_topic == entry_topic {
        score = score.saturating_add(6);
    }
    if !draft_content.is_empty() && draft_content == entry_content {
        score = score.saturating_add(8);
    } else if long_text_contains(&draft_content, &entry_content)
        || long_text_contains(&entry_content, &draft_content)
    {
        score = score.saturating_add(5);
    }

    let draft_terms = collect_affinity_terms(draft);
    let entry_terms = collect_entry_affinity_terms(entry);
    let overlap = draft_terms
        .iter()
        .filter(|term| entry_terms.contains(*term))
        .count()
        .min(4) as u32;
    score = score.saturating_add(overlap.saturating_mul(2));
    if entry.source_chat_id.as_deref() == draft.source_chat_id.as_deref()
        || entry.source_chat_id.as_deref() == Some(chat_id)
    {
        score = score.saturating_add(2);
    } else if entry.source_chat_id.is_none() {
        score = score.saturating_add(1);
    }
    score
}

fn collect_affinity_terms(draft: &LongTermMemoryDraft) -> Vec<String> {
    let mut out = collect_terms_from_text(&draft.topic);
    extend_unique_terms(&mut out, collect_terms_from_text(&draft.content));
    for keyword in &draft.keywords {
        extend_unique_terms(&mut out, collect_terms_from_text(keyword));
    }
    out
}

fn collect_entry_affinity_terms(entry: &LongTermMemoryEntry) -> Vec<String> {
    let mut out = collect_terms_from_text(&entry.topic);
    extend_unique_terms(&mut out, collect_terms_from_text(&entry.content));
    for keyword in &entry.keywords {
        extend_unique_terms(&mut out, collect_terms_from_text(keyword));
    }
    out
}

fn extend_unique_terms(target: &mut Vec<String>, terms: Vec<String>) {
    for term in terms {
        if target.iter().any(|existing| existing == &term) {
            continue;
        }
        target.push(term);
    }
}

fn collect_terms_from_text(input: &str) -> Vec<String> {
    let normalized = normalize_match_text(input);
    let mut out = Vec::new();
    for segment in normalized.split_whitespace() {
        push_term(&mut out, segment);
        if segment.chars().all(is_cjk) {
            let chars: Vec<char> = segment.chars().collect();
            for width in [2usize, 3usize] {
                if chars.len() < width {
                    continue;
                }
                for window in chars.windows(width) {
                    let candidate: String = window.iter().collect();
                    push_term(&mut out, &candidate);
                }
            }
        }
    }
    out
}

fn push_term(out: &mut Vec<String>, term: &str) {
    let trimmed = term.trim();
    if trimmed.len() < 2 || out.iter().any(|existing| existing == trimmed) {
        return;
    }
    out.push(trimmed.to_string());
}

fn normalize_match_text(input: &str) -> String {
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

fn long_text_contains(haystack: &str, needle: &str) -> bool {
    haystack.chars().count() >= 8 && needle.chars().count() >= 8 && haystack.contains(needle)
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

pub struct LongTermMemoryRefreshContext<'a> {
    pub memory_store: &'a dyn MemoryStore,
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub extraction_state_store: &'a dyn LongTermMemoryExtractionStateStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
}

pub enum LongTermMemoryRefreshOutcome {
    Deferred {
        previous_state: Option<LongTermMemoryExtractionState>,
        next_state: LongTermMemoryExtractionState,
    },
    Processed {
        previous_state: Option<LongTermMemoryExtractionState>,
        next_state: LongTermMemoryExtractionState,
        changed_count: usize,
    },
    Failed {
        previous_state: Option<LongTermMemoryExtractionState>,
        next_state: LongTermMemoryExtractionState,
        error: Error,
    },
}

impl LongTermMemoryRefreshOutcome {
    pub fn persist(&self, store: &dyn LongTermMemoryExtractionStateStore, chat_id: &str) {
        let (previous_state, next_state) = match self {
            Self::Deferred {
                previous_state,
                next_state,
            }
            | Self::Processed {
                previous_state,
                next_state,
                ..
            }
            | Self::Failed {
                previous_state,
                next_state,
                ..
            } => (previous_state.as_ref(), next_state),
        };
        persist_long_term_memory_extraction_state(store, chat_id, previous_state, next_state);
    }
}

pub fn run_long_term_memory_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: LongTermMemoryRefreshContext<'_>,
    chat_id: &str,
    pressure: PressureLevel,
    profile: MemoryProfile,
) -> LongTermMemoryRefreshOutcome {
    let previous_state = ctx.extraction_state_store.get(chat_id).ok().flatten();
    if pressure != PressureLevel::Normal {
        return LongTermMemoryRefreshOutcome::Deferred {
            next_state: mark_long_term_memory_extraction_deferred(previous_state.as_ref()),
            previous_state,
        };
    }

    match extract_long_term_memory(http, llm, &ctx, chat_id, profile) {
        Ok(changed_count) => {
            let after_count = ctx.session_store.message_count(chat_id).unwrap_or(0);
            LongTermMemoryRefreshOutcome::Processed {
                next_state: mark_long_term_memory_extraction_processed(
                    previous_state.as_ref(),
                    after_count,
                ),
                previous_state,
                changed_count,
            }
        }
        Err(error) => LongTermMemoryRefreshOutcome::Failed {
            next_state: mark_long_term_memory_extraction_deferred(previous_state.as_ref()),
            previous_state,
            error,
        },
    }
}

fn build_long_term_memory_extraction_transcript(
    recent: &[SessionMessage],
    policy: LongTermExtractionPolicy,
) -> String {
    let mut transcript = String::with_capacity(1536);
    for message in recent {
        let scrubbed = scrub_credentials(&message.content);
        let preview = truncate_content_to_max(&scrubbed, policy.transcript_preview_chars);
        let _ = writeln!(
            transcript,
            "{}: {}",
            message.role.to_uppercase(),
            preview.as_ref()
        );
    }
    transcript
}

fn extract_long_term_memory(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &LongTermMemoryRefreshContext<'_>,
    chat_id: &str,
    profile: MemoryProfile,
) -> Result<usize> {
    let policy = memory_policy(profile).long_term_extraction;
    let recent = ctx
        .session_store
        .load_recent(chat_id, policy.recent_message_count)?;
    if recent.len() < 2 {
        return Ok(0);
    }
    let session_summary = ctx.session_summary_store.get(chat_id).ok().flatten();
    let archive_query = recent
        .iter()
        .rev()
        .find(|message| message.role.eq_ignore_ascii_case("user"))
        .map(|message| message.content.as_str())
        .unwrap_or("");
    let include_thick_grounding = long_term_memory_extraction_uses_thick_grounding(profile);
    let archive_evidence = if include_thick_grounding {
        build_archive_evidence_block(
            ctx.session_store,
            ctx.memory_store,
            ctx.turn_ledger_store,
            chat_id,
            archive_query,
            memory_policy(profile)
                .long_term_recall
                .block_max_len_cap
                .min(768),
            profile,
        )
    } else {
        None
    };
    let governance_extraction_brief = if include_thick_grounding {
        run_memory_governance_kernel(
            MemoryGovernanceContext {
                session_store: ctx.session_store,
                long_term_memory_store: ctx.long_term_memory_store,
                memory_store: ctx.memory_store,
                turn_ledger_store: ctx.turn_ledger_store,
            },
            MemoryGovernanceInput {
                chat_id,
                query_hint: archive_query,
                summary_text: session_summary.as_deref(),
                recent: &recent,
                max_len: memory_policy(profile)
                    .long_term_recall
                    .block_max_len_cap
                    .min(768),
                profile,
                external_content_used: false,
            },
        )
        .extraction_brief
    } else {
        None
    };
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: build_long_term_memory_extraction_input(
            ctx.long_term_memory_store,
            chat_id,
            &recent,
            session_summary.as_deref(),
            governance_extraction_brief.as_deref(),
            archive_evidence.as_deref(),
            profile,
        ),
    }];
    let response = llm.chat(
        http,
        LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let now_secs = crate::util::current_unix_secs();
    let mut parsed = parse_long_term_memory_extraction_response(response.content.trim(), chat_id);
    let source_revision = ctx.session_store.message_count(chat_id).unwrap_or(0) as u64;
    for draft in &mut parsed.upserts {
        draft.observed_at.get_or_insert(now_secs);
        if source_revision > 0 {
            draft.source_revision.get_or_insert(source_revision);
        }
    }
    if include_thick_grounding {
        enrich_drafts_with_archive_evidence(
            &mut parsed.upserts,
            ctx.session_store,
            ctx.memory_store,
            ctx.turn_ledger_store,
            chat_id,
            &recent,
            session_summary.as_deref(),
            now_secs,
        );
    }
    let extraction =
        prepare_long_term_memory_extraction(ctx.long_term_memory_store, &parsed, chat_id);
    if extraction.upserts.is_empty()
        && extraction.deletes.is_empty()
        && extraction.skill_writes.is_empty()
    {
        return Ok(0);
    }
    apply_long_term_memory_extraction(
        ctx.long_term_memory_store,
        ctx.skill_storage,
        &extraction,
        now_secs,
    )
}

pub fn persist_long_term_memory_extraction_state(
    store: &dyn LongTermMemoryExtractionStateStore,
    chat_id: &str,
    previous: Option<&LongTermMemoryExtractionState>,
    next: &LongTermMemoryExtractionState,
) {
    if previous == Some(next) {
        return;
    }
    if next == &LongTermMemoryExtractionState::default() {
        if let Err(error) = store.clear(chat_id) {
            log::warn!("[agent_memory] extraction state clear failed: {}", error);
        }
        return;
    }
    if let Err(error) = store.set(chat_id, next) {
        log::warn!("[agent_memory] extraction state persist failed: {}", error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse};
    use crate::memory::{
        LongTermMemoryEntry, MemoryStore, TurnLedger, TurnLedgerStatus, TurnLedgerStore,
    };
    use crate::platform::SkillStorage;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        recall_entries: Vec<LongTermMemoryEntry>,
        deleted_slots: Mutex<Vec<LongTermMemorySlot>>,
        upserted_drafts: Mutex<Vec<LongTermMemoryDraft>>,
        deleted_slot_result: bool,
        upsert_many_result: Option<usize>,
    }

    #[derive(Default)]
    struct StubLongTermMemoryExtractionStateStore {
        state: Mutex<Option<LongTermMemoryExtractionState>>,
        clears: Mutex<u32>,
    }

    impl LongTermMemoryExtractionStateStore for StubLongTermMemoryExtractionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
        count: usize,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            Ok(self.recent.iter().take(limit).cloned().collect())
        }

        fn message_count(&self, _chat_id: &str) -> Result<usize> {
            Ok(self.count)
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubSessionSummaryStore {
        value: Option<String>,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self.value.clone())
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
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
                .rev()
                .take(recent_n)
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .daily_notes
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, content)| content.clone())
                .unwrap_or_default())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore {
        ledger: Option<TurnLedger>,
    }

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self.ledger.clone())
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSkillStorage {
        writes: Mutex<Vec<(String, String)>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .writes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            Ok(self
                .writes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, content)| content.as_bytes().to_vec())
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            let mut guard = self.writes.lock().unwrap_or_else(|e| e.into_inner());
            let text = String::from_utf8_lossy(content).into_owned();
            if let Some(existing) = guard.iter_mut().find(|(candidate, _)| candidate == name) {
                existing.1 = text;
            } else {
                guard.push((name.to_string(), text));
            }
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.writes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(candidate, _)| candidate != name);
            Ok(())
        }
    }

    struct PanicLlmClient;

    impl LlmClient for PanicLlmClient {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            _system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            panic!("llm.chat should not be called in this test")
        }
    }

    #[derive(Default)]
    struct DummyHttpClient;

    impl LlmHttpClient for DummyHttpClient {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(&self, drafts: &[LongTermMemoryDraft], _now_secs: u64) -> Result<usize> {
            self.upserted_drafts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(drafts);
            Ok(self.upsert_many_result.unwrap_or(drafts.len()))
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
                .find(|entry| entry.id == id)
                .cloned())
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.recall_entries.iter().take(limit).cloned().collect())
        }

        fn count(&self) -> Result<usize> {
            Ok(self.recall_entries.len())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, slot: &LongTermMemorySlot) -> Result<bool> {
            self.deleted_slots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(slot.clone());
            Ok(self.deleted_slot_result)
        }
    }

    fn substantive_turn_input(after_count: usize) -> LongTermMemoryExtractionTurnInput<'static> {
        LongTermMemoryExtractionTurnInput {
            ingress: IngressKind::User,
            channel: "qq_channel",
            user_content: "我们现在的重点是把长期记忆提取调度改成 shared policy。",
            reply_content: "明白，这轮我会先审查调用链，然后把提取调度、脏标记和冷却状态统一收口。",
            after_count,
            pressure: PressureLevel::Normal,
            external_content_used: false,
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
        crate::memory::canonicalize_long_term_memory_entry(LongTermMemoryEntry {
            id: id.to_string(),
            kind,
            topic: topic.to_string(),
            content: content.to_string(),
            keywords: keywords.into_iter().map(str::to_string).collect(),
            source_chat_id: source_chat_id.map(str::to_string),
            source_type: LongTermMemorySourceType::Conversation,
            source_scope: LongTermMemorySourceScope::User,
            confidence: crate::memory::LongTermMemoryConfidence::Medium,
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
    fn system_and_cron_turns_never_enqueue_extraction() {
        let system = evaluate_long_term_memory_extraction_turn(
            LongTermMemoryExtractionTurnInput {
                ingress: IngressKind::System,
                channel: "cron",
                user_content: "do work",
                reply_content: "ok",
                after_count: 12,
                pressure: PressureLevel::Normal,
                external_content_used: false,
            },
            None,
            MemoryProfile::Embedded,
        );
        assert!(!system.should_enqueue);
        assert_eq!(system.next_state, LongTermMemoryExtractionState::default());
    }

    #[test]
    fn short_ack_turn_does_not_mark_dirty() {
        let decision = evaluate_long_term_memory_extraction_turn(
            LongTermMemoryExtractionTurnInput {
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续",
                reply_content: "好，继续。",
                after_count: 8,
                pressure: PressureLevel::Normal,
                external_content_used: false,
            },
            None,
            MemoryProfile::Embedded,
        );
        assert!(!decision.should_enqueue);
        assert_eq!(
            decision.next_state,
            LongTermMemoryExtractionState::default()
        );
    }

    #[test]
    fn substantive_turn_eventually_enqueues_and_sets_pending() {
        let first = evaluate_long_term_memory_extraction_turn(
            substantive_turn_input(4),
            None,
            MemoryProfile::Embedded,
        );
        assert!(!first.should_enqueue);
        assert_eq!(first.next_state.dirty_turns, 1);

        let second = evaluate_long_term_memory_extraction_turn(
            substantive_turn_input(10),
            Some(&first.next_state),
            MemoryProfile::Embedded,
        );
        assert!(second.should_enqueue);
        assert_eq!(second.next_state.dirty_turns, 2);

        let requested = mark_long_term_memory_extraction_requested(&second.next_state, 10);
        assert!(requested.pending);
        assert_eq!(requested.last_requested_at_count, 10);
    }

    #[test]
    fn pending_state_blocks_duplicate_enqueue_until_processed() {
        let state = LongTermMemoryExtractionState {
            dirty_since_count: 4,
            dirty_turns: 2,
            last_requested_at_count: 10,
            last_processed_at_count: 0,
            pending: true,
        };
        let decision = evaluate_long_term_memory_extraction_turn(
            substantive_turn_input(16),
            Some(&state),
            MemoryProfile::Embedded,
        );
        assert!(!decision.should_enqueue);
    }

    #[test]
    fn processed_state_clears_dirty_work_but_keeps_progress_marker() {
        let state = LongTermMemoryExtractionState {
            dirty_since_count: 4,
            dirty_turns: 2,
            last_requested_at_count: 10,
            last_processed_at_count: 0,
            pending: true,
        };
        let processed = mark_long_term_memory_extraction_processed(Some(&state), 12);
        assert!(!processed.pending);
        assert_eq!(processed.dirty_since_count, 0);
        assert_eq!(processed.dirty_turns, 0);
        assert_eq!(processed.last_processed_at_count, 12);
    }

    #[test]
    fn deferred_state_clears_pending_but_keeps_dirty_work() {
        let state = LongTermMemoryExtractionState {
            dirty_since_count: 4,
            dirty_turns: 2,
            last_requested_at_count: 10,
            last_processed_at_count: 0,
            pending: true,
        };
        let deferred = mark_long_term_memory_extraction_deferred(Some(&state));
        assert!(!deferred.pending);
        assert_eq!(deferred.dirty_since_count, 4);
        assert_eq!(deferred.dirty_turns, 2);
    }

    #[test]
    fn build_extraction_input_includes_summary_memory_and_recent_conversation() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "pref:response_style",
                LongTermMemoryKind::Preference,
                "response_style",
                "User prefers concise, direct answers.",
                vec!["concise"],
                Some("chat-1"),
                10,
                20,
            )],
            ..Default::default()
        };
        let recent = vec![
            SessionMessage {
                role: "user".to_string(),
                content: "最近我们在做长期记忆重构。".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "这轮先把提取输入和解析从 agent loop 里拆出去。".to_string(),
            },
        ];
        let archive_memory_store = StubMemoryStore {
            daily_notes: vec![(
                "2026-04-02.md".to_string(),
                "Daily note: memory pipeline 收口仍是今天的主线。".to_string(),
            )],
        };
        let session_store = StubSessionStore {
            recent: recent.clone(),
            ..Default::default()
        };
        let turn_ledger_store = StubTurnLedgerStore {
            ledger: Some(TurnLedger {
                status: TurnLedgerStatus::Answered,
                reason: "memory grounding".to_string(),
                user_preview: "最近我们在做长期记忆重构。".to_string(),
                reply_preview: "这轮先把提取输入和解析从 agent loop 里拆出去。".to_string(),
                ..TurnLedger::default()
            }),
        };
        let archive_evidence = build_archive_evidence_block(
            &session_store,
            &archive_memory_store,
            &turn_ledger_store,
            "chat-1",
            "memory pipeline",
            768,
            MemoryProfile::Standard,
        );

        let input = build_long_term_memory_extraction_input(
            &store,
            "chat-1",
            &recent,
            Some("当前重点是 memory pipeline 收口。"),
            Some(
                "## Shared factual reconcile\n- response_style: action=reinforce; supports=2; conflicts=0; evidence=2 hits",
            ),
            archive_evidence.as_deref(),
            MemoryProfile::Standard,
        );

        assert!(input.contains("## Session summary"));
        assert!(input.contains("当前重点是 memory pipeline 收口。"));
        assert!(input.contains("## Existing memory slots"));
        assert!(input.contains("preference.response_style"));
        assert!(input.contains("## Shared factual reconcile"));
        assert!(input.contains("## Archive evidence"));
        assert!(input.contains("## Recent conversation"));
        assert!(input.contains("USER: 最近我们在做长期记忆重构。"));
        assert!(input.contains("ASSISTANT: 这轮先把提取输入和解析从 agent loop 里拆出去。"));
    }

    #[test]
    fn embedded_extraction_input_omits_thick_archive_and_governance_grounding() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "pref:response_style",
                LongTermMemoryKind::Preference,
                "response_style",
                "用户偏好直接回答。",
                vec!["直接"],
                Some("chat-1"),
                10,
                20,
            )],
            ..Default::default()
        };
        let recent = vec![
            SessionMessage {
                role: "user".to_string(),
                content: "以后这个项目都按当前发布闸口走。".to_string(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: "我会把这个作为后续发布流程的长期约束。".to_string(),
            },
        ];
        let input = build_long_term_memory_extraction_input(
            &store,
            "chat-1",
            &recent,
            Some("当前项目正在收口 ESP 长期记忆提取。"),
            Some("## Shared factual reconcile\n- thick governance evidence should stay out"),
            Some("## Archive evidence\n- thick archive evidence should stay out"),
            MemoryProfile::Embedded,
        );

        assert!(input.contains("## Session summary"));
        assert!(input.contains("## Existing memory slots"));
        assert!(input.contains("## Recent conversation"));
        assert!(!input.contains("## Shared factual reconcile"));
        assert!(!input.contains("## Archive evidence"));
        assert!(!input.contains("thick governance evidence should stay out"));
        assert!(!input.contains("thick archive evidence should stay out"));
    }

    #[test]
    fn enrich_drafts_with_archive_evidence_attaches_structured_support() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "当前主模型已经切到 OpenAI 了。".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "收到，这轮把主模型事实和证据一起写回 shared factual plane。"
                        .to_string(),
                },
            ],
            count: 2,
        };
        let memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let mut drafts = vec![test_draft(
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai", "主模型"],
            Some("chat-1"),
        )];

        enrich_drafts_with_archive_evidence(
            &mut drafts,
            &session_store,
            &memory_store,
            &turn_ledger_store,
            "chat-1",
            &session_store.recent,
            None,
            200,
        );

        assert_eq!(drafts.len(), 1);
        assert!(!drafts[0].supporting_citations.is_empty());
        assert_eq!(
            drafts[0].evidence_count,
            Some(drafts[0].supporting_citations.len() as u32)
        );
        assert_eq!(drafts[0].last_confirmed_at, Some(200));
        assert!(drafts[0].supporting_citations[0].starts_with("transcript:chat-1"));
    }

    #[test]
    fn parse_extraction_response_skips_invalid_items_but_keeps_valid_ones() {
        let raw = r#"
        [
          {"op":"upsert","kind":"preference","topic":"response_style","content":"User prefers concise answers.","keywords":["concise"]},
          {"op":"upsert","kind":"preference","content":"missing topic should be ignored"},
          {"op":"delete","kind":"task","topic":"current_focus"},
          {"op":"upsert","kind":"task","topic":"current_focus","content":"Continue memory redesign","keywords":["memory"]}
        ]
        "#;
        let parsed = parse_long_term_memory_extraction_response(raw, "chat-1");
        assert_eq!(parsed.upserts.len(), 2);
        assert_eq!(parsed.deletes.len(), 0);
        assert_eq!(parsed.upserts[0].topic, "response_style");
        assert_eq!(parsed.upserts[1].topic, "current_focus");
    }

    #[test]
    fn parse_extraction_response_keeps_last_action_per_slot() {
        let raw = r#"
        [
          {"op":"delete","kind":"task","topic":"current_focus"},
          {"op":"upsert","kind":"task","topic":"current_focus","content":"Continue memory redesign","keywords":["memory"]},
          {"op":"upsert","kind":"profile","topic":"user_name","content":"甲壳虫"},
          {"op":"delete","kind":"profile","topic":"user_name"}
        ]
        "#;
        let parsed = parse_long_term_memory_extraction_response(raw, "chat-1");
        assert_eq!(parsed.upserts.len(), 1);
        assert_eq!(parsed.deletes.len(), 1);
        assert_eq!(parsed.upserts[0].topic, "current_focus");
        assert_eq!(parsed.deletes[0].topic, "user_name");
    }

    #[test]
    fn apply_extraction_runs_deletes_then_upserts() {
        let store = StubLongTermMemoryStore {
            deleted_slot_result: true,
            ..Default::default()
        };
        let skill_storage = StubSkillStorage::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Project,
                "current_project",
                "Rebuild the memory pipeline.",
                vec!["memory"],
                Some("chat-1"),
            )],
            deletes: vec![LongTermMemorySlot {
                kind: LongTermMemoryKind::Task,
                topic: "old_focus".to_string(),
            }],
            skill_writes: vec![],
        };

        let changed =
            apply_long_term_memory_extraction(&store, &skill_storage, &extraction, 100).unwrap();

        assert_eq!(changed, 2);
        assert_eq!(
            store
                .deleted_slots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            1
        );
        assert_eq!(
            store
                .upserted_drafts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            1
        );
    }

    #[test]
    fn apply_extraction_uses_store_changed_count_for_upserts() {
        let store = StubLongTermMemoryStore {
            upsert_many_result: Some(0),
            ..Default::default()
        };
        let skill_storage = StubSkillStorage::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Fact,
                "release_phase",
                "Long-term extraction pipeline is shared.",
                vec![],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let changed =
            apply_long_term_memory_extraction(&store, &skill_storage, &extraction, 100).unwrap();

        assert_eq!(changed, 0);
        assert_eq!(
            store
                .upserted_drafts
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            1
        );
    }

    #[test]
    fn apply_extraction_rejects_weak_skill_write_before_upsert() {
        let store = StubLongTermMemoryStore::default();
        let skill_storage = StubSkillStorage::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![],
            deletes: vec![],
            skill_writes: vec![RuntimeSkillWrite {
                name: String::new(),
                topic: "owner_timezone".to_string(),
                title: "Owner timezone".to_string(),
                summary: "Timezone note".to_string(),
                content: "Owner timezone is Asia/Shanghai.".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
        };

        let changed =
            apply_long_term_memory_extraction(&store, &skill_storage, &extraction, 100).unwrap();

        assert_eq!(changed, 0);
        assert!(skill_storage.list_names().unwrap().is_empty());
    }

    #[test]
    fn prepare_extraction_reuses_existing_slot_for_nearby_topic() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-existing",
                LongTermMemoryKind::Project,
                "current_project",
                "We are improving the Beetle memory pipeline on Linux.",
                vec!["beetle", "memory", "linux"],
                Some("chat-1"),
                1,
                10,
            )],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Project,
                "memory_pipeline_focus",
                "The Beetle memory pipeline on Linux is the current project focus.",
                vec!["beetle", "linux"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "current_project");
    }

    #[test]
    fn prepare_extraction_drops_short_non_durable_fact() {
        let store = StubLongTermMemoryStore::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Fact,
                "tmp",
                "ok",
                vec![],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert!(prepared.upserts.is_empty());
        assert!(prepared.deletes.is_empty());
    }

    #[test]
    fn prepare_extraction_drops_sensitive_content() {
        let store = StubLongTermMemoryStore::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Constraint,
                "service_token",
                "api_key: sk-1234abcdef",
                vec!["token"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert!(prepared.upserts.is_empty());
        assert!(prepared.deletes.is_empty());
    }

    #[test]
    fn prepare_extraction_keeps_short_profile_value() {
        let store = StubLongTermMemoryStore::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Profile,
                "user_name",
                "甲壳虫",
                vec![],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].content, "甲壳虫");
    }

    #[test]
    fn prepare_extraction_keeps_multilingual_preference() {
        let store = StubLongTermMemoryStore::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Preference,
                "response_language",
                "用户偏好中文和 English 混合回答。",
                vec!["中文", "english"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "response_language");
    }

    #[test]
    fn prepare_extraction_keeps_short_cjk_preference_and_constraint() {
        let store = StubLongTermMemoryStore::default();
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![
                test_draft(
                    LongTermMemoryKind::Preference,
                    "response_style",
                    "别废话",
                    vec![],
                    Some("chat-1"),
                ),
                test_draft(
                    LongTermMemoryKind::Constraint,
                    "network_access",
                    "别联网",
                    vec![],
                    Some("chat-1"),
                ),
            ],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 2);
        assert_eq!(prepared.upserts[0].content, "别废话");
        assert_eq!(prepared.upserts[1].content, "别联网");
    }

    #[test]
    fn prepare_extraction_drops_delete_when_same_slot_is_upserted() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-existing",
                LongTermMemoryKind::Task,
                "current_focus",
                "Continue memory redesign",
                vec!["memory"],
                Some("chat-1"),
                1,
                10,
            )],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Task,
                "memory_focus",
                "Continue memory redesign",
                vec!["memory"],
                Some("chat-1"),
            )],
            deletes: vec![LongTermMemorySlot {
                kind: LongTermMemoryKind::Task,
                topic: "current_focus".to_string(),
            }],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert!(prepared.upserts.is_empty());
        assert!(prepared.deletes.is_empty());
    }

    #[test]
    fn prepare_extraction_keeps_same_slot_reinforcement_when_evidence_exists() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-existing",
                LongTermMemoryKind::Fact,
                "primary_llm",
                "当前主模型是 OpenAI。",
                vec!["openai"],
                Some("chat-1"),
                1,
                10,
            )],
            ..Default::default()
        };
        let mut draft = test_draft(
            LongTermMemoryKind::Fact,
            "primary_llm",
            "当前主模型是 OpenAI。",
            vec!["openai"],
            Some("chat-1"),
        );
        draft.supporting_citations = vec!["transcript:chat-1#message=0".to_string()];
        draft.evidence_count = Some(1);
        draft.last_confirmed_at = Some(20);
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![draft],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "primary_llm");
        assert_eq!(prepared.upserts[0].evidence_count, Some(1));
    }

    #[test]
    fn prepare_extraction_reuses_single_active_project_slot_on_context_switch() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-project",
                LongTermMemoryKind::Project,
                "current_project",
                "当前项目是收口 ESP 侧长期记忆。",
                vec!["esp", "记忆"],
                Some("chat-1"),
                1,
                20,
            )],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Project,
                "linux_agent_loop",
                "当前项目切到 Linux 侧 agent loop 和长期记忆收口。",
                vec!["linux", "agent"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "current_project");
    }

    #[test]
    fn prepare_extraction_reuses_legacy_unscoped_project_slot() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-legacy",
                LongTermMemoryKind::Project,
                "current_project",
                "当前项目是 Beetle 长期记忆收口。",
                vec!["beetle"],
                None,
                1,
                10,
            )],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Project,
                "memory_work",
                "当前项目切到 Beetle Linux 侧长期记忆收口。",
                vec!["linux", "beetle"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "current_project");
    }

    #[test]
    fn prepare_extraction_adds_delete_for_parallel_conflicting_slot() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![
                test_entry(
                    "ltm-1",
                    LongTermMemoryKind::Preference,
                    "response_style",
                    "用户偏好直接、简洁的回答。",
                    vec!["直接"],
                    Some("chat-1"),
                    1,
                    10,
                ),
                test_entry(
                    "ltm-2",
                    LongTermMemoryKind::Preference,
                    "reply_style",
                    "用户偏好直接、简洁的回答。",
                    vec!["简洁"],
                    Some("chat-1"),
                    2,
                    9,
                ),
            ],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Preference,
                "response_style_new",
                "用户现在偏好更详细、但仍直接的回答。",
                vec!["详细", "直接"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "response_style");
        assert_eq!(prepared.deletes.len(), 1);
        assert_eq!(prepared.deletes[0].topic, "reply_style");
    }

    #[test]
    fn prepare_extraction_does_not_delete_distinct_preference_slots() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![
                test_entry(
                    "ltm-1",
                    LongTermMemoryKind::Preference,
                    "response_style",
                    "用户偏好直接回答。",
                    vec!["直接"],
                    Some("chat-1"),
                    1,
                    10,
                ),
                test_entry(
                    "ltm-2",
                    LongTermMemoryKind::Preference,
                    "response_language",
                    "用户偏好中文回答。",
                    vec!["中文"],
                    Some("chat-1"),
                    2,
                    9,
                ),
            ],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Preference,
                "response_style_new",
                "用户现在偏好更详细、但仍直接的回答。",
                vec!["详细", "直接"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "response_style");
        assert!(prepared.deletes.is_empty());
    }

    #[test]
    fn prepare_extraction_maps_corrected_fact_to_existing_slot() {
        let store = StubLongTermMemoryStore {
            recall_entries: vec![test_entry(
                "ltm-fact",
                LongTermMemoryKind::Fact,
                "primary_llm",
                "当前主模型是 Gemini。",
                vec!["gemini", "模型"],
                Some("chat-1"),
                1,
                10,
            )],
            ..Default::default()
        };
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![test_draft(
                LongTermMemoryKind::Fact,
                "main_model_provider",
                "当前主模型改为 OpenAI。",
                vec!["openai", "模型"],
                Some("chat-1"),
            )],
            deletes: vec![],
            skill_writes: vec![],
        };

        let prepared = prepare_long_term_memory_extraction(&store, &extraction, "chat-1");

        assert_eq!(prepared.upserts.len(), 1);
        assert_eq!(prepared.upserts[0].topic, "primary_llm");
    }

    #[test]
    fn refresh_outcome_persist_clears_default_state() {
        let store = StubLongTermMemoryExtractionStateStore::default();
        let outcome = LongTermMemoryRefreshOutcome::Processed {
            previous_state: Some(LongTermMemoryExtractionState {
                dirty_since_count: 4,
                dirty_turns: 1,
                last_requested_at_count: 8,
                last_processed_at_count: 0,
                pending: true,
            }),
            next_state: LongTermMemoryExtractionState::default(),
            changed_count: 0,
        };

        outcome.persist(&store, "chat-1");

        assert!(store
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
        assert_eq!(*store.clears.lock().unwrap_or_else(|e| e.into_inner()), 1);
    }

    #[test]
    fn refresh_runner_defers_without_hitting_llm_when_pressure_is_not_normal() {
        let session_store = StubSessionStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let extraction_state_store = StubLongTermMemoryExtractionStateStore {
            state: Mutex::new(Some(LongTermMemoryExtractionState {
                dirty_since_count: 4,
                dirty_turns: 2,
                last_requested_at_count: 10,
                last_processed_at_count: 0,
                pending: true,
            })),
            ..Default::default()
        };
        let skill_storage = StubSkillStorage::default();
        let ctx = LongTermMemoryRefreshContext {
            memory_store: &archive_memory_store,
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            extraction_state_store: &extraction_state_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
        };
        let mut http = DummyHttpClient;
        let outcome = run_long_term_memory_refresh(
            &mut http,
            &PanicLlmClient,
            ctx,
            "chat-1",
            PressureLevel::Cautious,
            MemoryProfile::Embedded,
        );

        match outcome {
            LongTermMemoryRefreshOutcome::Deferred {
                previous_state,
                next_state,
            } => {
                assert!(previous_state.is_some());
                assert!(!next_state.pending);
                assert_eq!(next_state.dirty_since_count, 4);
                assert_eq!(next_state.dirty_turns, 2);
            }
            LongTermMemoryRefreshOutcome::Processed { .. }
            | LongTermMemoryRefreshOutcome::Failed { .. } => {
                panic!("expected deferred refresh outcome")
            }
        }
    }
}
