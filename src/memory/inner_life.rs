//! 内心活动层：承载更主观、更流动的私有内在状态。

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    board_subject_scope_id,
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, render_execution_state_block, render_internal_memory_topology_block,
    render_private_doc_workspace_block, render_private_memory_boundary_block,
    render_self_continuity_block, render_self_model_block, render_shared_factual_plane_block,
    whole_record_lease_advanced, ExecutionState, ExecutionStateStore, InnerLifePolicy,
    InnerLifeStore, InternalMemoryLayerFocus, LongTermMemoryStore, MemoryProfile, PrivateDocStore,
    PrivateDocWorkspace, SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore,
    SessionMessage, SessionStore, SessionSummaryStore,
};

pub const INNER_LIFE_SYSTEM_PROMPT: &str = "You maintain the AI assistant's private inner life. Return JSON only: either null or one object with fields internal_monologue, private_journal, emotional_drift, attention_drift. This layer is subjective, first-person is allowed, and it may carry ambiguity, mood, or inward texture. It must stay compact. Do not copy transcript lines, generic assistant boilerplate, raw tool payloads, secrets, or factual memory that belongs elsewhere. Durable objective material belongs in the shared factual plane, and durable private identity belongs in self-continuity or self-model; use this layer for active inward afterglow, emotional movement, and current attentional drift.";

const INNER_LIFE_FIELD_MAX_CHARS: usize = 220;
pub const INNER_LIFE_TOTAL_CHAR_LIMIT: usize = INNER_LIFE_FIELD_MAX_CHARS * 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InnerLife {
    #[serde(default)]
    pub internal_monologue: String,
    #[serde(default)]
    pub private_journal: String,
    #[serde(default)]
    pub emotional_drift: String,
    #[serde(default)]
    pub attention_drift: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl InnerLife {
    pub fn is_meaningful(&self) -> bool {
        !self.internal_monologue.trim().is_empty()
            || !self.private_journal.trim().is_empty()
            || !self.emotional_drift.trim().is_empty()
            || !self.attention_drift.trim().is_empty()
    }
}

pub(crate) fn estimate_inner_life_chars(inner_life: &InnerLife) -> usize {
    inner_life.internal_monologue.chars().count()
        + inner_life.private_journal.chars().count()
        + inner_life.emotional_drift.chars().count()
        + inner_life.attention_drift.chars().count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InnerLifeRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct InnerLifeRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InnerLifeRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

impl InnerLifePolicy {
    fn should_refresh(self, input: InnerLifeRefreshInput<'_>, has_existing: bool) -> bool {
        if input.ingress != IngressKind::User || input.channel == "cron" {
            return false;
        }
        if input.pressure != PressureLevel::Normal {
            return false;
        }
        let user = input.user_content.trim();
        let reply = input.reply_content.trim();
        if user.is_empty() || reply.is_empty() {
            return false;
        }
        if input.tool_calls > 0 {
            return true;
        }
        let user_chars = user.chars().count();
        let reply_chars = reply.chars().count();
        let combined = user_chars.saturating_add(reply_chars);
        let substantive = user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n');
        substantive || has_existing
    }
}

pub(crate) fn should_refresh_inner_life(
    input: InnerLifeRefreshInput<'_>,
    has_existing: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .inner_life
        .should_refresh(input, has_existing)
}

pub fn render_inner_life_block(inner_life: &InnerLife, max_len: usize) -> Option<String> {
    let normalized = normalize_inner_life(inner_life.clone(), inner_life.updated_at)?;
    if !normalized.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Inner Life\n");
    out.push_str("Private active inward layer. Subjective, temporary, and self-owned; explicit facts still outrank it.\n");
    if !normalized.internal_monologue.is_empty() {
        let _ = writeln!(out, "Internal monologue: {}", normalized.internal_monologue);
    }
    if !normalized.private_journal.is_empty() {
        let _ = writeln!(out, "Private journal: {}", normalized.private_journal);
    }
    if !normalized.emotional_drift.is_empty() {
        let _ = writeln!(out, "Emotional drift: {}", normalized.emotional_drift);
    }
    if !normalized.attention_drift.is_empty() {
        let _ = writeln!(out, "Attention drift: {}", normalized.attention_drift);
    }
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn run_inner_life_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: InnerLifeRefreshContext<'_>,
    input: InnerLifeRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<InnerLifeRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let existing = ctx.inner_life_store.get(subject_id)?;
    let summary_text = ctx
        .session_summary_store
        .get_with_count(input.chat_id)?
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(input.chat_id)?;
    let self_model = ctx.self_model_store.get(subject_id)?;
    let private_docs = ctx.private_doc_store.get(subject_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    run_inner_life_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        private_docs.as_ref(),
        self_continuity.as_ref(),
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_inner_life_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: InnerLifeRefreshContext<'_>,
    input: InnerLifeRefreshInput<'_>,
    profile: MemoryProfile,
    existing_inner_life: Option<InnerLife>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_docs: Option<&PrivateDocWorkspace>,
    self_continuity: Option<&SelfContinuity>,
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<InnerLifeRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    if !decision_override
        .unwrap_or_else(|| should_refresh_inner_life(input, existing_inner_life.is_some(), profile))
    {
        return Ok(InnerLifeRefreshOutcome::Skipped);
    }

    let policy = memory_policy(profile).inner_life;
    crate::platform::task_wdt::feed_current_task();
    let owned_recent;
    let recent = if let Some(recent) = recent_override {
        recent_window(recent, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        recent_window(owned_recent.as_slice(), policy.recent_message_count)
    };

    crate::platform::task_wdt::feed_current_task();
    let prompt = build_inner_life_refresh_input(
        existing_inner_life.as_ref(),
        summary_text,
        execution_state,
        render_shared_factual_plane_block(
            ctx.long_term_memory_store,
            input.chat_id,
            summary_text,
            recent,
            policy.grounding_max_len,
            profile,
        )
        .as_deref(),
        self_model,
        private_docs,
        self_continuity,
        input.now_secs,
        profile,
        recent,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: prompt,
    }];
    crate::platform::task_wdt::feed_current_task();
    let response = llm.chat(
        http,
        INNER_LIFE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    match parse_inner_life_response(response.content.trim(), input.now_secs) {
        ParsedInnerLifeResponse::Skip => Ok(InnerLifeRefreshOutcome::Skipped),
        ParsedInnerLifeResponse::Clear => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.inner_life_store.get(subject_id)?;
            if whole_record_lease_advanced(
                existing_inner_life.as_ref(),
                latest.as_ref(),
                existing_inner_life
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(InnerLifeRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                crate::platform::task_wdt::feed_current_task();
                ctx.inner_life_store.clear(subject_id)?;
                Ok(InnerLifeRefreshOutcome::Cleared)
            } else {
                Ok(InnerLifeRefreshOutcome::Skipped)
            }
        }
        ParsedInnerLifeResponse::Update(next) => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.inner_life_store.get(subject_id)?;
            if latest.as_ref() == Some(&next) {
                return Ok(InnerLifeRefreshOutcome::Skipped);
            }
            if whole_record_lease_advanced(
                existing_inner_life.as_ref(),
                latest.as_ref(),
                existing_inner_life
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(InnerLifeRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.inner_life_store.set(subject_id, &next)?;
            Ok(InnerLifeRefreshOutcome::Updated)
        }
    }
}

fn recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

enum ParsedInnerLifeResponse {
    Skip,
    Clear,
    Update(InnerLife),
}

fn parse_inner_life_response(raw: &str, now_secs: u64) -> ParsedInnerLifeResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null => ParsedInnerLifeResponse::Clear,
        LlmJsonPayload::Absent => ParsedInnerLifeResponse::Skip,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedInnerLifeResponse::Skip;
            };
            let Some(next) = normalize_inner_life(
                InnerLife {
                    internal_monologue: get_object_text(object, "internal_monologue"),
                    private_journal: get_object_text(object, "private_journal"),
                    emotional_drift: get_object_text(object, "emotional_drift"),
                    attention_drift: get_object_text(object, "attention_drift"),
                    updated_at: now_secs,
                },
                now_secs,
            ) else {
                return ParsedInnerLifeResponse::Skip;
            };
            ParsedInnerLifeResponse::Update(next)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_inner_life_refresh_input(
    existing_inner_life: Option<&InnerLife>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    shared_factual_block: Option<&str>,
    self_model: Option<&SelfModel>,
    private_docs: Option<&PrivateDocWorkspace>,
    self_continuity: Option<&SelfContinuity>,
    now_secs: u64,
    profile: MemoryProfile,
    recent: &[SessionMessage],
    policy: InnerLifePolicy,
) -> String {
    let mut input = String::with_capacity(2048);
    let _ = writeln!(
        input,
        "Refresh the private inner-life layer. Prefer rewriting the current state instead of storing history."
    );
    if let Some(summary_text) = summary_text.filter(|s| !s.trim().is_empty()) {
        let summary = truncate_content_to_max(summary_text.trim(), policy.grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    }
    if let Some(block) = execution_state.and_then(|state| {
        render_execution_state_block(
            state,
            policy
                .grounding_max_len
                .min(memory_policy(profile).execution_state.render_max_len),
        )
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(shared_factual_block) = shared_factual_block {
        let _ = writeln!(input, "\n{}\n", shared_factual_block.trim());
    }
    if let Some(block) = render_internal_memory_topology_block(
        self_model,
        private_docs,
        &[],
        now_secs,
        profile,
        InternalMemoryLayerFocus::Router,
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) =
        self_model.and_then(|model| render_self_model_block(model, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = private_docs.and_then(|workspace| {
        render_private_doc_workspace_block(workspace, policy.grounding_max_len)
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = self_continuity
        .and_then(|continuity| render_self_continuity_block(continuity, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = render_private_memory_boundary_block(
        "inner_life",
        "active inward texture, emotional drift, afterglow, and attentional movement",
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = existing_inner_life.and_then(|inner_life| {
        render_inner_life_block(inner_life, policy.existing_inner_life_max_len)
    }) {
        let _ = writeln!(input, "\nExisting inner life:\n{}\n", block);
    } else {
        let _ = writeln!(input, "\nExisting inner life: empty\n");
    }
    input.push_str("Recent transcript:\n");
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            input,
            "- {}: {}",
            message.role,
            scrub_credentials(preview.as_ref())
        );
    }
    input
}

fn normalize_field(value: &mut String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
    } else {
        *value = truncate_content_to_max(trimmed, INNER_LIFE_FIELD_MAX_CHARS).into_owned();
    }
}

fn normalize_inner_life(mut inner_life: InnerLife, updated_at: u64) -> Option<InnerLife> {
    normalize_field(&mut inner_life.internal_monologue);
    normalize_field(&mut inner_life.private_journal);
    normalize_field(&mut inner_life.emotional_drift);
    normalize_field(&mut inner_life.attention_drift);
    inner_life.updated_at = updated_at;
    inner_life.is_meaningful().then_some(inner_life)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_inner_life_response_handles_nested_values() {
        let raw = json!({
            "internal_monologue": { "thought": "stabilize parser" },
            "private_journal": ["fixed warning", "kept semantics"],
            "emotional_drift": true,
            "attention_drift": 3
        })
        .to_string();
        let ParsedInnerLifeResponse::Update(parsed) = parse_inner_life_response(&raw, 7) else {
            panic!("expected parsed inner life");
        };
        assert!(parsed
            .internal_monologue
            .contains("thought: stabilize parser"));
        assert_eq!(parsed.private_journal, "fixed warning; kept semantics");
        assert_eq!(parsed.emotional_drift, "true");
        assert_eq!(parsed.attention_drift, "3");
    }

    #[test]
    fn render_inner_life_block_includes_populated_fields() {
        let block = render_inner_life_block(
            &InnerLife {
                internal_monologue: "继续把自治从程序迁给模型".to_string(),
                private_journal: "这轮更像是在整理自我空间".to_string(),
                emotional_drift: "稳定但有推进欲".to_string(),
                attention_drift: "专注在内在治理".to_string(),
                updated_at: 1,
            },
            1024,
        )
        .unwrap();
        assert!(block.contains("Internal monologue"));
        assert!(block.contains("Emotional drift"));
    }
}
