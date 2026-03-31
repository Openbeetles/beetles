//! 长期记忆提取调度与轻量状态。
//! Long-term memory extraction scheduling and lightweight state.

use crate::bus::IngressKind;
use crate::error::Error;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;

use super::{
    render_long_term_memory_block, LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySlot,
    LongTermMemoryStore, SessionMessage, SessionStore, SessionSummaryStore,
};

/// 长期记忆提取状态存储路径（相对状态根）。
pub const REL_PATH_LONG_TERM_EXTRACTION_STATES: &str = "memory/long_term_extraction_states.json";
pub const LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT: &str = "You extract durable long-term memory for a personal AI assistant. Return JSON only: an array of objects. Each object must contain op, kind, topic. op must be upsert or delete. kind must be one of preference, profile, relationship, project, task, constraint, fact. topic must be a short stable slot key identifying the same memory across future updates, for example response_style, user_name, current_project, timezone, partner_name. Reuse an existing topic whenever the conversation updates, completes, or corrects that same durable slot. Prefer updating an existing slot over inventing a nearby new topic. For op=upsert, also provide content and optional keywords. For op=delete, omit content and keywords. Use delete when the conversation clearly invalidates or completes an existing durable slot, for example a task is finished, a temporary project focus is no longer active, or a prior fact is explicitly corrected. Use the provided session summary and existing long-term memory as grounding when deciding whether to upsert, delete, or ignore. Keep only the highest-value durable changes, at most 4 items. If there is nothing durable to add, update, or delete, return []. Do not store greetings, one-off troubleshooting steps, transient status, or assistant-only claims.";
pub const LONG_TERM_MEMORY_EXTRACTION_RECENT_N: usize = 8;
pub const LONG_TERM_MEMORY_EXTRACTION_BATCH: usize = 4;

const LONG_TERM_MEMORY_EXTRACTION_TRANSCRIPT_PREVIEW_CHARS: usize = 180;
const LONG_TERM_MEMORY_EXTRACTION_EXISTING_MEMORY_MAX_LEN: usize = 768;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongTermMemoryExtractionTurnDecision {
    pub next_state: LongTermMemoryExtractionState,
    pub should_enqueue: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LongTermMemoryExtractionPolicy {
    first_process_min_messages: usize,
    min_messages_between_requests: usize,
    force_process_after_messages: usize,
    low_signal_user_chars: usize,
    low_signal_user_words: usize,
    low_signal_reply_chars: usize,
    substantive_user_chars: usize,
    substantive_reply_chars: usize,
    substantive_combined_chars: usize,
}

const DEFAULT_LONG_TERM_MEMORY_EXTRACTION_POLICY: LongTermMemoryExtractionPolicy =
    LongTermMemoryExtractionPolicy {
        first_process_min_messages: 8,
        min_messages_between_requests: 6,
        force_process_after_messages: 12,
        low_signal_user_chars: 6,
        low_signal_user_words: 2,
        low_signal_reply_chars: 48,
        substantive_user_chars: 10,
        substantive_reply_chars: 32,
        substantive_combined_chars: 72,
    };

impl LongTermMemoryExtractionPolicy {
    fn is_eligible_turn(self, input: LongTermMemoryExtractionTurnInput<'_>) -> bool {
        if input.ingress != IngressKind::User || input.channel == "cron" {
            return false;
        }
        if input.pressure != PressureLevel::Normal {
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
) -> LongTermMemoryExtractionTurnDecision {
    let policy = DEFAULT_LONG_TERM_MEMORY_EXTRACTION_POLICY;
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
}

#[derive(Deserialize)]
struct LongTermMemoryExtractionItem {
    #[serde(default = "default_long_term_memory_extraction_op")]
    op: String,
    kind: LongTermMemoryKind,
    topic: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    source_chat_id: Option<String>,
}

enum ParsedLongTermMemoryAction {
    Upsert(LongTermMemoryDraft),
    Delete(LongTermMemorySlot),
}

fn default_long_term_memory_extraction_op() -> String {
    "upsert".to_string()
}

pub fn build_long_term_memory_extraction_input(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    recent: &[SessionMessage],
    session_summary: Option<&str>,
) -> String {
    let transcript = build_long_term_memory_extraction_transcript(recent);
    let existing_memory = store
        .recall(
            &transcript,
            Some(chat_id),
            LONG_TERM_MEMORY_EXTRACTION_BATCH,
        )
        .ok()
        .and_then(|entries| {
            render_long_term_memory_block(
                &entries,
                LONG_TERM_MEMORY_EXTRACTION_EXISTING_MEMORY_MAX_LEN,
            )
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

    input.push_str("## Recent conversation\n");
    input.push_str(transcript.trim());
    input
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
        let action = match parsed_item.op.trim().to_ascii_lowercase().as_str() {
            "delete" => ParsedLongTermMemoryAction::Delete(LongTermMemorySlot {
                kind: parsed_item.kind,
                topic: parsed_item.topic,
            }),
            "upsert" => {
                if parsed_item.source_chat_id.is_none() {
                    parsed_item.source_chat_id = Some(chat_id.to_string());
                }
                ParsedLongTermMemoryAction::Upsert(LongTermMemoryDraft {
                    kind: parsed_item.kind,
                    topic: parsed_item.topic,
                    content: parsed_item.content,
                    keywords: parsed_item.keywords,
                    source_chat_id: parsed_item.source_chat_id,
                })
            }
            _ => continue,
        };
        let slot_id = match &action {
            ParsedLongTermMemoryAction::Upsert(draft) => draft.stable_id(),
            ParsedLongTermMemoryAction::Delete(slot) => slot.stable_id(),
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
    for action in actions {
        match action {
            ParsedLongTermMemoryAction::Upsert(draft) => upserts.push(draft),
            ParsedLongTermMemoryAction::Delete(slot) => deletes.push(slot),
        }
    }
    ParsedLongTermMemoryExtraction { upserts, deletes }
}

pub fn apply_long_term_memory_extraction(
    store: &dyn LongTermMemoryStore,
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
        changed += store.upsert_many(&extraction.upserts, now_secs)?;
    }
    Ok(changed)
}

pub struct LongTermMemoryRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub extraction_state_store: &'a dyn LongTermMemoryExtractionStateStore,
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
) -> LongTermMemoryRefreshOutcome {
    let previous_state = ctx.extraction_state_store.get(chat_id).ok().flatten();
    if pressure != PressureLevel::Normal {
        return LongTermMemoryRefreshOutcome::Deferred {
            next_state: mark_long_term_memory_extraction_deferred(previous_state.as_ref()),
            previous_state,
        };
    }

    match extract_long_term_memory(http, llm, &ctx, chat_id) {
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

fn build_long_term_memory_extraction_transcript(recent: &[SessionMessage]) -> String {
    let mut transcript = String::with_capacity(1536);
    for message in recent {
        let preview = truncate_content_to_max(
            &message.content,
            LONG_TERM_MEMORY_EXTRACTION_TRANSCRIPT_PREVIEW_CHARS,
        );
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
) -> Result<usize> {
    let recent = ctx
        .session_store
        .load_recent(chat_id, LONG_TERM_MEMORY_EXTRACTION_RECENT_N)?;
    if recent.len() < 2 {
        return Ok(0);
    }
    let session_summary = ctx.session_summary_store.get(chat_id).ok().flatten();
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: build_long_term_memory_extraction_input(
            ctx.long_term_memory_store,
            chat_id,
            &recent,
            session_summary.as_deref(),
        ),
    }];
    let response = llm.chat(
        http,
        LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    let extraction = parse_long_term_memory_extraction_response(response.content.trim(), chat_id);
    if extraction.upserts.is_empty() && extraction.deletes.is_empty() {
        return Ok(0);
    }
    apply_long_term_memory_extraction(
        ctx.long_term_memory_store,
        &extraction,
        crate::util::current_unix_secs(),
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
    use crate::memory::LongTermMemoryEntry;
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
        }
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
            },
            None,
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
            },
            None,
        );
        assert!(!decision.should_enqueue);
        assert_eq!(
            decision.next_state,
            LongTermMemoryExtractionState::default()
        );
    }

    #[test]
    fn substantive_turn_eventually_enqueues_and_sets_pending() {
        let first = evaluate_long_term_memory_extraction_turn(substantive_turn_input(4), None);
        assert!(!first.should_enqueue);
        assert_eq!(first.next_state.dirty_turns, 1);

        let second = evaluate_long_term_memory_extraction_turn(
            substantive_turn_input(10),
            Some(&first.next_state),
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
        let decision =
            evaluate_long_term_memory_extraction_turn(substantive_turn_input(16), Some(&state));
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
            recall_entries: vec![LongTermMemoryEntry {
                id: "pref:response_style".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "response_style".to_string(),
                content: "User prefers concise, direct answers.".to_string(),
                keywords: vec!["concise".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                created_at: 10,
                updated_at: 20,
            }],
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

        let input = build_long_term_memory_extraction_input(
            &store,
            "chat-1",
            &recent,
            Some("当前重点是 memory pipeline 收口。"),
        );

        assert!(input.contains("## Session summary"));
        assert!(input.contains("当前重点是 memory pipeline 收口。"));
        assert!(input.contains("## Long-term memory"));
        assert!(input.contains("response_style"));
        assert!(input.contains("## Recent conversation"));
        assert!(input.contains("USER: 最近我们在做长期记忆重构。"));
        assert!(input.contains("ASSISTANT: 这轮先把提取输入和解析从 agent loop 里拆出去。"));
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
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![LongTermMemoryDraft {
                kind: LongTermMemoryKind::Project,
                topic: "current_project".to_string(),
                content: "Rebuild the memory pipeline.".to_string(),
                keywords: vec!["memory".to_string()],
                source_chat_id: Some("chat-1".to_string()),
            }],
            deletes: vec![LongTermMemorySlot {
                kind: LongTermMemoryKind::Task,
                topic: "old_focus".to_string(),
            }],
        };

        let changed = apply_long_term_memory_extraction(&store, &extraction, 100).unwrap();

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
        let extraction = ParsedLongTermMemoryExtraction {
            upserts: vec![LongTermMemoryDraft {
                kind: LongTermMemoryKind::Fact,
                topic: "release_phase".to_string(),
                content: "Long-term extraction pipeline is shared.".to_string(),
                keywords: vec![],
                source_chat_id: Some("chat-1".to_string()),
            }],
            deletes: vec![],
        };

        let changed = apply_long_term_memory_extraction(&store, &extraction, 100).unwrap();

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
        let ctx = LongTermMemoryRefreshContext {
            session_store: &session_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            extraction_state_store: &extraction_state_store,
        };
        let mut http = DummyHttpClient;
        let outcome = run_long_term_memory_refresh(
            &mut http,
            &PanicLlmClient,
            ctx,
            "chat-1",
            PressureLevel::Cautious,
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
