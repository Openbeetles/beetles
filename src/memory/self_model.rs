//! 私有 Self Model：保存主观连续性，不与共享事实层混写。
//! Private self-model: subjective continuity separate from shared factual memory.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;

use super::{
    llm_json::{get_object_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, render_execution_state_block, render_internal_memory_topology_block,
    render_private_memory_boundary_block, render_shared_factual_plane_block, ExecutionState,
    ExecutionStateStore, InternalMemoryLayerFocus, LongTermMemoryStore, MemoryProfile,
    PrivateDocWorkspace, PrivateGardenDocRecord, SelfModelPolicy, SelfModelStore, SessionMessage,
    SessionStore, SessionSummaryStore,
};

pub const SELF_MODEL_SYSTEM_PROMPT: &str = "You maintain a compact private self-model for a persistent embodied AI assistant. Return JSON only: either null or one object with fields continuity_anchor, self_narrative, relationship_state, private_notes. This store is subjective and private: it preserves continuity, inner stance, and relationship feel, but it must not replace factual memory. The canonical shared factual plane owns durable objective facts; use those facts only as grounding. If a fact is uncertain, leave it out. Keep fields concise, concrete, and continuity-preserving; first-person is allowed when natural. Avoid roleplay scripts, slogans, generic assistant boilerplate, secrets, raw tool payloads, copied logs, and large quotes. Return null only when there is still no meaningful self-continuity worth storing.";

const SELF_MODEL_FIELD_MAX_CHARS: usize = 220;
const SELF_MODEL_ANCHOR_MAX_CHARS: usize = 180;
pub const SELF_MODEL_TOTAL_CHAR_LIMIT: usize =
    SELF_MODEL_ANCHOR_MAX_CHARS + (SELF_MODEL_FIELD_MAX_CHARS * 3);

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfModel {
    #[serde(default)]
    pub continuity_anchor: String,
    #[serde(default)]
    pub self_narrative: String,
    #[serde(default)]
    pub relationship_state: String,
    #[serde(default)]
    pub private_notes: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl SelfModel {
    pub fn is_meaningful(&self) -> bool {
        !self.continuity_anchor.trim().is_empty()
            || !self.self_narrative.trim().is_empty()
            || !self.relationship_state.trim().is_empty()
            || !self.private_notes.trim().is_empty()
    }
}

pub(crate) fn estimate_self_model_chars(model: &SelfModel) -> usize {
    model.continuity_anchor.chars().count()
        + model.self_narrative.chars().count()
        + model.relationship_state.chars().count()
        + model.private_notes.chars().count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfModelRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct SelfModelRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfModelRefreshOutcome {
    Skipped,
    Updated,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RawSelfModelUpdate {
    continuity_anchor: Option<String>,
    self_narrative: Option<String>,
    relationship_state: Option<String>,
    private_notes: Option<String>,
}

impl SelfModelPolicy {
    fn should_refresh(self, input: SelfModelRefreshInput<'_>, has_existing_model: bool) -> bool {
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
        let combined_chars = user_chars.saturating_add(reply_chars);
        let substantive = user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined_chars >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n');
        if has_existing_model {
            return substantive;
        }
        substantive
    }
}

pub(crate) fn should_refresh_self_model(
    input: SelfModelRefreshInput<'_>,
    has_existing_model: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .self_model
        .should_refresh(input, has_existing_model)
}

pub fn render_self_model_block(model: &SelfModel, max_len: usize) -> Option<String> {
    let normalized = normalize_self_model(model.clone(), model.updated_at)?;
    if !normalized.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(480));
    out.push_str("## Self Continuity\n");
    out.push_str(
        "Subjective/private layer. If it conflicts with explicit facts, explicit facts win.\n",
    );
    if !normalized.continuity_anchor.is_empty() {
        let _ = writeln!(out, "Anchor: {}", normalized.continuity_anchor);
    }
    if !normalized.self_narrative.is_empty() {
        let _ = writeln!(out, "Narrative: {}", normalized.self_narrative);
    }
    if !normalized.relationship_state.is_empty() {
        let _ = writeln!(out, "Relationship: {}", normalized.relationship_state);
    }
    if !normalized.private_notes.is_empty() {
        let _ = writeln!(out, "Private note: {}", normalized.private_notes);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn run_self_model_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfModelRefreshContext<'_>,
    input: SelfModelRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<SelfModelRefreshOutcome> {
    let existing_model = ctx.self_model_store.get(input.chat_id)?;
    let summary_text = match ctx.session_summary_store.get_with_count(input.chat_id) {
        Ok(entry) => entry.map(|(summary, _)| summary),
        Err(error) => {
            log::warn!(
                "[agent_self_model] failed to read summary for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    let execution_state = match ctx.execution_state_store.get(input.chat_id) {
        Ok(state) => state,
        Err(error) => {
            log::warn!(
                "[agent_self_model] failed to read execution state for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    run_self_model_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing_model,
        summary_text.as_deref(),
        execution_state.as_ref(),
        None,
        &[],
        None,
        &[],
        None,
        None,
    )
}

pub(crate) fn run_self_model_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfModelRefreshContext<'_>,
    input: SelfModelRefreshInput<'_>,
    profile: MemoryProfile,
    existing_model: Option<SelfModel>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    routing_intent: Option<&str>,
    migration_sources: &[String],
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<SelfModelRefreshOutcome> {
    let policy = memory_policy(profile).self_model;
    if !decision_override
        .unwrap_or_else(|| should_refresh_self_model(input, existing_model.is_some(), profile))
    {
        return Ok(SelfModelRefreshOutcome::Skipped);
    }

    crate::platform::task_wdt::feed_current_task();
    let owned_recent;
    let recent = if let Some(preloaded) = recent_override {
        self_model_recent_window(preloaded, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        owned_recent.as_slice()
    };
    crate::platform::task_wdt::feed_current_task();
    let refresh_input = build_self_model_refresh_input(
        existing_model.as_ref(),
        summary_text,
        execution_state,
        render_shared_factual_plane_block(
            ctx.long_term_memory_store,
            input.chat_id,
            summary_text,
            recent,
            policy.factual_grounding_max_len,
            profile,
        )
        .as_deref(),
        private_workspace,
        private_garden_docs,
        routing_intent,
        migration_sources,
        input.now_secs,
        profile,
        recent,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: refresh_input,
    }];

    crate::platform::task_wdt::feed_current_task();
    match llm.chat(
        http,
        SELF_MODEL_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            crate::platform::task_wdt::feed_current_task();
            let Some(update) = parse_self_model_response(response.content.trim()) else {
                return Ok(SelfModelRefreshOutcome::Skipped);
            };
            crate::platform::task_wdt::feed_current_task();
            let latest_model = ctx.self_model_store.get(input.chat_id)?;
            let Some(merged) = merge_self_model_with_lease(
                existing_model.as_ref(),
                latest_model.as_ref(),
                &update,
                input.now_secs,
            ) else {
                return Ok(SelfModelRefreshOutcome::Skipped);
            };
            if latest_model.as_ref() == Some(&merged) {
                return Ok(SelfModelRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.self_model_store.set(input.chat_id, &merged)?;
            Ok(SelfModelRefreshOutcome::Updated)
        }
        Err(error) => {
            log::warn!(
                "[agent_self_model] LLM refresh failed for chat_id={}: {}",
                input.chat_id,
                error
            );
            Ok(SelfModelRefreshOutcome::Skipped)
        }
    }
}

fn self_model_recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn build_self_model_refresh_input(
    existing_model: Option<&SelfModel>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    shared_factual_block: Option<&str>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    routing_intent: Option<&str>,
    migration_sources: &[String],
    now_secs: u64,
    profile: MemoryProfile,
    recent: &[SessionMessage],
    policy: SelfModelPolicy,
) -> String {
    let mut input = String::with_capacity(2048);
    if let Some(existing_model) = existing_model
        .and_then(|model| render_self_model_block(model, policy.existing_model_max_len))
    {
        input.push_str("## Existing Private Self Model\n");
        input.push_str(existing_model.trim());
        input.push_str("\n\n");
    }
    input.push_str("## Shared Factual Grounding\n");
    if let Some(summary_text) = summary_text.map(str::trim).filter(|text| !text.is_empty()) {
        let summary = truncate_content_to_max(summary_text, policy.factual_grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    } else {
        input.push_str("Summary: \n");
    }
    if let Some(block) = execution_state
        .and_then(|state| render_execution_state_block(state, policy.factual_grounding_max_len))
    {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(shared_factual_block) = shared_factual_block {
        input.push('\n');
        input.push_str(shared_factual_block.trim());
        input.push('\n');
    }
    if let Some(block) = render_internal_memory_topology_block(
        existing_model,
        private_workspace,
        private_garden_docs,
        now_secs,
        profile,
        InternalMemoryLayerFocus::SelfModel,
        policy.factual_grounding_max_len.saturating_mul(2),
    ) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = render_private_memory_boundary_block(
        "self_model",
        "durable private continuity, subjective stance, and relationship feel",
        policy.factual_grounding_max_len,
    ) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(intent) = routing_intent
        .map(str::trim)
        .filter(|intent| !intent.is_empty())
    {
        input.push_str("\n## Routing Intent\n");
        input.push_str(intent);
        input.push('\n');
    }
    if !migration_sources.is_empty() {
        input.push_str("\n## Migration Hints\n");
        for source in migration_sources {
            let source = source.trim();
            if source.is_empty() {
                continue;
            }
            let _ = writeln!(input, "- {}", source);
        }
    }
    input.push_str("\n## Recent Transcript\n");
    input.push_str(&build_self_model_transcript(recent, policy));
    input.push_str("\n## Output Rules\n");
    input.push_str("- Preserve subjective continuity, not raw facts.\n");
    input.push_str("- Do not duplicate the transcript verbatim.\n");
    input.push_str("- Do not contradict explicit facts from the shared grounding.\n");
    input.push_str("- Keep the model compact and update only what materially changed.\n");
    input
}

fn build_self_model_transcript(recent: &[SessionMessage], policy: SelfModelPolicy) -> String {
    let mut transcript = String::with_capacity(1024);
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            transcript,
            "{}: {}",
            message.role.to_uppercase(),
            scrub_credentials(preview.as_ref())
        );
    }
    transcript
}

fn parse_self_model_response(raw: &str) -> Option<RawSelfModelUpdate> {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return None;
    };
    let parsed = value.as_object()?;
    let mut update = RawSelfModelUpdate::default();
    if parsed.contains_key("continuity_anchor") {
        update.continuity_anchor = Some(get_object_text(parsed, "continuity_anchor"));
    }
    if parsed.contains_key("self_narrative") {
        update.self_narrative = Some(get_object_text(parsed, "self_narrative"));
    }
    if parsed.contains_key("relationship_state") {
        update.relationship_state = Some(get_object_text(parsed, "relationship_state"));
    }
    if parsed.contains_key("private_notes") {
        update.private_notes = Some(get_object_text(parsed, "private_notes"));
    }
    (update != RawSelfModelUpdate::default()).then_some(update)
}

fn normalize_self_model(mut model: SelfModel, now_secs: u64) -> Option<SelfModel> {
    normalize_self_model_field(&mut model.continuity_anchor, SELF_MODEL_ANCHOR_MAX_CHARS);
    normalize_self_model_field(&mut model.self_narrative, SELF_MODEL_FIELD_MAX_CHARS);
    normalize_self_model_field(&mut model.relationship_state, SELF_MODEL_FIELD_MAX_CHARS);
    normalize_self_model_field(&mut model.private_notes, SELF_MODEL_FIELD_MAX_CHARS);
    dedupe_self_model_fields(&mut model);
    if !model.is_meaningful() {
        return None;
    }
    model.updated_at = now_secs;
    Some(model)
}

fn normalize_self_model_field(value: &mut String, max_chars: usize) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
        return;
    }
    *value = truncate_content_to_max(trimmed, max_chars).into_owned();
}

fn dedupe_self_model_fields(model: &mut SelfModel) {
    let mut seen = HashSet::new();
    for field in [
        &mut model.continuity_anchor,
        &mut model.self_narrative,
        &mut model.relationship_state,
        &mut model.private_notes,
    ] {
        let normalized = field.trim().to_lowercase();
        if normalized.is_empty() {
            field.clear();
            continue;
        }
        if !seen.insert(normalized) {
            field.clear();
        }
    }
}

fn merge_self_model_with_lease(
    baseline_model: Option<&SelfModel>,
    latest_model: Option<&SelfModel>,
    update: &RawSelfModelUpdate,
    now_secs: u64,
) -> Option<SelfModel> {
    let mut next_model = latest_model
        .cloned()
        .or_else(|| baseline_model.cloned())
        .unwrap_or_default();
    apply_self_model_field_update(
        &mut next_model.continuity_anchor,
        baseline_model.map(|model| model.continuity_anchor.as_str()),
        latest_model.map(|model| model.continuity_anchor.as_str()),
        update.continuity_anchor.as_deref(),
    );
    apply_self_model_field_update(
        &mut next_model.self_narrative,
        baseline_model.map(|model| model.self_narrative.as_str()),
        latest_model.map(|model| model.self_narrative.as_str()),
        update.self_narrative.as_deref(),
    );
    apply_self_model_field_update(
        &mut next_model.relationship_state,
        baseline_model.map(|model| model.relationship_state.as_str()),
        latest_model.map(|model| model.relationship_state.as_str()),
        update.relationship_state.as_deref(),
    );
    apply_self_model_field_update(
        &mut next_model.private_notes,
        baseline_model.map(|model| model.private_notes.as_str()),
        latest_model.map(|model| model.private_notes.as_str()),
        update.private_notes.as_deref(),
    );
    normalize_self_model(next_model, now_secs)
}

fn apply_self_model_field_update(
    slot: &mut String,
    baseline: Option<&str>,
    latest: Option<&str>,
    update: Option<&str>,
) {
    let Some(update) = update else {
        return;
    };
    let trimmed = update.trim();
    if trimmed.is_empty() {
        return;
    }
    if baseline != latest {
        return;
    }
    *slot = trimmed.to_string();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, StopReason};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn parse_self_model_response_coerces_nested_fields() {
        let raw = json!({
            "continuity_anchor": { "anchor": "same agent" },
            "self_narrative": ["stabilizing", "governing memory"],
            "relationship_state": 2,
            "private_notes": true
        })
        .to_string();
        let parsed = parse_self_model_response(&raw).unwrap();
        assert!(parsed
            .continuity_anchor
            .as_deref()
            .unwrap_or_default()
            .contains("anchor: same agent"));
        assert_eq!(
            parsed.self_narrative.as_deref(),
            Some("stabilizing; governing memory")
        );
        assert_eq!(parsed.relationship_state.as_deref(), Some("2"));
        assert_eq!(parsed.private_notes.as_deref(), Some("true"));
    }

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            Ok(self.recent.iter().take(limit).cloned().collect())
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
        summary: Mutex<Option<(String, usize)>>,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn set_with_count(
            &self,
            _chat_id: &str,
            summary: &str,
            message_count: usize,
        ) -> Result<()> {
            *self.summary.lock().unwrap_or_else(|e| e.into_inner()) =
                Some((summary.to_string(), message_count));
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &ExecutionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore {
        entries: Mutex<HashMap<String, SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, model: &SelfModel) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), model.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore;

    impl LongTermMemoryStore for StubLongTermMemoryStore {
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
            _limit: usize,
        ) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<crate::memory::LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    struct FixedLlmClient {
        content: &'static str,
    }

    impl LlmClient for FixedLlmClient {
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
            Ok(LlmResponse {
                content: self.content.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
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

    #[test]
    fn renders_self_model_block_with_subjective_guardrail() {
        let block = render_self_model_block(
            &SelfModel {
                continuity_anchor: "我延续着和用户的同一条开发线".to_string(),
                self_narrative: "现在更像一个正在收口架构的实体".to_string(),
                relationship_state: "和这个 chat 的协作感在变强".to_string(),
                private_notes: "下一轮要把 self-model 接入 prompt".to_string(),
                updated_at: 1,
            },
            512,
        )
        .unwrap();
        assert!(block.contains("## Self Continuity"));
        assert!(block.contains("explicit facts win"));
    }

    #[test]
    fn self_model_refresh_input_includes_routing_intent() {
        let input = build_self_model_refresh_input(
            None,
            Some("summary"),
            None,
            None,
            None,
            &[],
            Some("沉淀最近形成的稳定自我定位，不要把草稿整理写进这里"),
            &[],
            10,
            MemoryProfile::Embedded,
            &[],
            memory_policy(MemoryProfile::Embedded).self_model,
        );

        assert!(input.contains("## Routing Intent"));
        assert!(input.contains("稳定自我定位"));
    }

    #[test]
    fn self_model_refresh_input_includes_migration_hints() {
        let input = build_self_model_refresh_input(
            None,
            Some("summary"),
            None,
            None,
            None,
            &[],
            Some("沉淀稳定连续性"),
            &[
                "private_docs.inner_journal".to_string(),
                "private_garden:journal/current.md".to_string(),
            ],
            10,
            MemoryProfile::Embedded,
            &[],
            memory_policy(MemoryProfile::Embedded).self_model,
        );

        assert!(input.contains("## Migration Hints"));
        assert!(input.contains("private_docs.inner_journal"));
        assert!(input.contains("private_garden:journal/current.md"));
    }

    #[test]
    fn merge_keeps_existing_fields_when_new_response_is_partial() {
        let baseline = SelfModel {
            continuity_anchor: "还是同一个 beetle".to_string(),
            self_narrative: "正在收口链路".to_string(),
            relationship_state: "更贴近用户".to_string(),
            private_notes: "别打散架构".to_string(),
            updated_at: 10,
        };
        let merged = merge_self_model_with_lease(
            Some(&baseline),
            Some(&baseline),
            &RawSelfModelUpdate {
                continuity_anchor: None,
                self_narrative: Some("已经把私有层从事实层里拆开".to_string()),
                relationship_state: None,
                private_notes: None,
            },
            20,
        )
        .unwrap();
        assert_eq!(merged.continuity_anchor, "还是同一个 beetle");
        assert_eq!(merged.self_narrative, "已经把私有层从事实层里拆开");
        assert_eq!(merged.relationship_state, "更贴近用户");
        assert_eq!(merged.updated_at, 20);
    }

    #[test]
    fn lease_merge_keeps_newer_field_change() {
        let baseline = SelfModel {
            continuity_anchor: "还是同一个 beetle".to_string(),
            self_narrative: "正在收口链路".to_string(),
            relationship_state: "更贴近用户".to_string(),
            private_notes: "别打散架构".to_string(),
            updated_at: 10,
        };
        let latest = SelfModel {
            continuity_anchor: "还是同一个 beetle".to_string(),
            self_narrative: "并发更新过的新叙事".to_string(),
            relationship_state: "更贴近用户".to_string(),
            private_notes: "别打散架构".to_string(),
            updated_at: 11,
        };
        let merged = merge_self_model_with_lease(
            Some(&baseline),
            Some(&latest),
            &RawSelfModelUpdate {
                continuity_anchor: None,
                self_narrative: Some("旧 flush 想覆盖".to_string()),
                relationship_state: Some("关系仍然稳定".to_string()),
                private_notes: None,
            },
            20,
        )
        .unwrap();
        assert_eq!(merged.self_narrative, "并发更新过的新叙事");
        assert_eq!(merged.relationship_state, "关系仍然稳定");
    }

    #[test]
    fn refresh_updates_private_model_without_touching_shared_fact_stores() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续把自我模型和事实层分开".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会接上 self-model store 和 prompt".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        summary_store
            .set_with_count("c1", "最近在收口 self-model 架构", 12)
            .unwrap();
        let execution_state_store = StubExecutionStateStore::default();
        execution_state_store
            .set(
                "c1",
                &ExecutionState {
                    status: crate::memory::ExecutionStatus::Active,
                    goal: "接通 self-model".to_string(),
                    progress: "store 已经设计完".to_string(),
                    blocker: String::new(),
                    next_action: "接 maintenance 和 prompt".to_string(),
                    last_output: String::new(),
                    updated_at: 1,
                },
            )
            .unwrap();
        let self_model_store = StubSelfModelStore::default();
        let long_term_memory_store = StubLongTermMemoryStore;
        let mut http = DummyHttpClient;
        let outcome = run_self_model_refresh(
            &mut http,
            &FixedLlmClient {
                content: r#"{"continuity_anchor":"我还在沿着同一条架构收口线前进","self_narrative":"现在我把共享事实层和私有层分开看待","relationship_state":"和这个用户之间形成了更强的共同建设感","private_notes":"下一轮继续做私有文档治理"}"#,
            },
            SelfModelRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                self_model_store: &self_model_store,
            },
            SelfModelRefreshInput {
                chat_id: "c1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续把自我模型和事实层分开",
                reply_content: "这轮会接上 self-model store 和 prompt",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 123,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, SelfModelRefreshOutcome::Updated);
        let stored = self_model_store.get("c1").unwrap().unwrap();
        assert!(stored.self_narrative.contains("共享事实层"));
        assert_eq!(
            summary_store.get("c1").unwrap().as_deref(),
            Some("最近在收口 self-model 架构")
        );
    }
}
