//! 自我连续性层：连接“昨天的我”和“现在的我”，并保存自治运行锚点。

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
    memory_policy, render_execution_state_block, render_inner_life_block,
    render_internal_memory_topology_block, render_private_doc_workspace_block,
    render_recent_persona_evidence_block, render_self_model_block, ExecutionState,
    ExecutionStateStore, InnerLife, InnerLifeStore, InternalMemoryLayerFocus, MemoryProfile,
    PrivateDocStore, PrivateDocWorkspace, RecentPersonaEvidence, SelfContinuityPolicy,
    SelfContinuityStore, SelfModel, SelfModelStore, SessionMessage, SessionStore,
    SessionSummaryStore,
};

pub const SELF_CONTINUITY_SYSTEM_PROMPT: &str = "You maintain a private self-continuity layer for a persistent embodied AI assistant. Return JSON only: either null or one object with fields wake_anchor, current_self_state, recent_changes, continuity_bridge, priority_posture, relationship_posture, task_posture. This layer should preserve the sense of still being the same self across time. It is more stable than inner-life drift, but still subjective and private. Keep it compact. Do not copy transcript lines, generic assistant boilerplate, raw tool payloads, or shared factual memory. Capture only the inward continuity that should still matter next time, including durable reply-ordering posture when it should still guide future turns. Treat recent persona evidence as multi-turn support, not as direct promotion authority from one turn.";

const SELF_CONTINUITY_FIELD_MAX_CHARS: usize = 220;
pub const SELF_CONTINUITY_TOTAL_CHAR_LIMIT: usize = SELF_CONTINUITY_FIELD_MAX_CHARS * 7;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfContinuity {
    #[serde(default)]
    pub wake_anchor: String,
    #[serde(default)]
    pub current_self_state: String,
    #[serde(default)]
    pub recent_changes: String,
    #[serde(default)]
    pub continuity_bridge: String,
    #[serde(default)]
    pub priority_posture: String,
    #[serde(default)]
    pub relationship_posture: String,
    #[serde(default)]
    pub task_posture: String,
    #[serde(default)]
    pub last_user_turn_at: u64,
    #[serde(default)]
    pub last_user_chat_id: String,
    #[serde(default)]
    pub last_user_channel: String,
    #[serde(default)]
    pub last_autonomy_run_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

impl SelfContinuity {
    pub fn is_meaningful(&self) -> bool {
        !self.wake_anchor.trim().is_empty()
            || !self.current_self_state.trim().is_empty()
            || !self.recent_changes.trim().is_empty()
            || !self.continuity_bridge.trim().is_empty()
            || !self.priority_posture.trim().is_empty()
            || !self.relationship_posture.trim().is_empty()
            || !self.task_posture.trim().is_empty()
    }
}

pub(crate) fn estimate_self_continuity_chars(continuity: &SelfContinuity) -> usize {
    continuity.wake_anchor.chars().count()
        + continuity.current_self_state.chars().count()
        + continuity.recent_changes.chars().count()
        + continuity.continuity_bridge.chars().count()
        + continuity.priority_posture.chars().count()
        + continuity.relationship_posture.chars().count()
        + continuity.task_posture.chars().count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfContinuityRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct SelfContinuityRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfContinuityRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RawSelfContinuityUpdate {
    wake_anchor: Option<String>,
    current_self_state: Option<String>,
    recent_changes: Option<String>,
    continuity_bridge: Option<String>,
    priority_posture: Option<String>,
    relationship_posture: Option<String>,
    task_posture: Option<String>,
}

impl SelfContinuityPolicy {
    fn should_refresh(self, input: SelfContinuityRefreshInput<'_>, has_existing: bool) -> bool {
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

pub(crate) fn should_refresh_self_continuity(
    input: SelfContinuityRefreshInput<'_>,
    has_existing: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .self_continuity
        .should_refresh(input, has_existing)
}

pub fn render_self_continuity_block(continuity: &SelfContinuity, max_len: usize) -> Option<String> {
    let normalized = normalize_self_continuity(continuity.clone(), continuity.updated_at)?;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Self Continuity Extended\n");
    out.push_str(
        "Private bridge across time. Use it to remain the same self without freezing growth.\n",
    );
    if !normalized.wake_anchor.is_empty() {
        let _ = writeln!(out, "Wake anchor: {}", normalized.wake_anchor);
    }
    if !normalized.current_self_state.is_empty() {
        let _ = writeln!(out, "Current self state: {}", normalized.current_self_state);
    }
    if !normalized.recent_changes.is_empty() {
        let _ = writeln!(out, "Recent changes: {}", normalized.recent_changes);
    }
    if !normalized.continuity_bridge.is_empty() {
        let _ = writeln!(out, "Continuity bridge: {}", normalized.continuity_bridge);
    }
    if !normalized.priority_posture.is_empty() {
        let _ = writeln!(out, "Priority posture: {}", normalized.priority_posture);
    }
    if !normalized.relationship_posture.is_empty() {
        let _ = writeln!(
            out,
            "Relationship posture: {}",
            normalized.relationship_posture
        );
    }
    if !normalized.task_posture.is_empty() {
        let _ = writeln!(out, "Task posture: {}", normalized.task_posture);
    }
    if normalized.last_user_turn_at > 0 || normalized.last_autonomy_run_at > 0 {
        let relation_anchor = if normalized.last_user_chat_id.trim().is_empty() {
            "none"
        } else {
            "active"
        };
        let channel_anchor = if normalized.last_user_channel.trim().is_empty() {
            "none"
        } else {
            "known"
        };
        let _ = writeln!(
            out,
            "Runtime anchors: last_user_turn_at={} last_user_relation={} last_user_channel_kind={} last_autonomy_run_at={}",
            normalized.last_user_turn_at,
            relation_anchor,
            channel_anchor,
            normalized.last_autonomy_run_at
        );
    }
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn run_self_continuity_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfContinuityRefreshContext<'_>,
    input: SelfContinuityRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<SelfContinuityRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let existing = ctx.self_continuity_store.get(subject_id)?;
    let summary_text = ctx
        .session_summary_store
        .get_with_count(input.chat_id)?
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(input.chat_id)?;
    let self_model = ctx.self_model_store.get(subject_id)?;
    let private_docs = ctx.private_doc_store.get(subject_id)?;
    let inner_life = ctx.inner_life_store.get(subject_id)?;
    run_self_continuity_refresh_with_state(
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
        inner_life.as_ref(),
        None,
        None,
        &[],
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_self_continuity_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfContinuityRefreshContext<'_>,
    input: SelfContinuityRefreshInput<'_>,
    profile: MemoryProfile,
    existing_continuity: Option<SelfContinuity>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_docs: Option<&PrivateDocWorkspace>,
    inner_life: Option<&InnerLife>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<SelfContinuityRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    if !decision_override.unwrap_or_else(|| {
        should_refresh_self_continuity(input, existing_continuity.is_some(), profile)
    }) {
        return Ok(SelfContinuityRefreshOutcome::Skipped);
    }

    let policy = memory_policy(profile).self_continuity;
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
    let prompt = build_self_continuity_refresh_input(
        existing_continuity.as_ref(),
        summary_text,
        execution_state,
        self_model,
        private_docs,
        inner_life,
        recent_persona_evidence,
        distillation_intent,
        distillation_sources,
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
        SELF_CONTINUITY_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    match parse_self_continuity_response(response.content.trim(), existing_continuity.as_ref()) {
        ParsedSelfContinuityResponse::Skip => Ok(SelfContinuityRefreshOutcome::Skipped),
        ParsedSelfContinuityResponse::Clear => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.self_continuity_store.get(subject_id)?;
            if latest.as_ref() != existing_continuity.as_ref() && latest.as_ref().is_some() {
                return Ok(SelfContinuityRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                crate::platform::task_wdt::feed_current_task();
                ctx.self_continuity_store.clear(subject_id)?;
                Ok(SelfContinuityRefreshOutcome::Cleared)
            } else {
                Ok(SelfContinuityRefreshOutcome::Skipped)
            }
        }
        ParsedSelfContinuityResponse::Update(update) => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.self_continuity_store.get(subject_id)?;
            let Some(next) = merge_self_continuity_with_lease(
                existing_continuity.as_ref(),
                latest.as_ref(),
                &update,
                input.now_secs,
                input.ingress == IngressKind::User,
            ) else {
                return Ok(SelfContinuityRefreshOutcome::Skipped);
            };
            if latest.as_ref() == Some(&next) {
                return Ok(SelfContinuityRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.self_continuity_store.set(subject_id, &next)?;
            Ok(SelfContinuityRefreshOutcome::Updated)
        }
    }
}

pub fn touch_self_continuity_runtime(
    store: &dyn SelfContinuityStore,
    subject_id: &str,
    now_secs: u64,
    touch_user_turn: bool,
    touch_autonomy_run: bool,
    user_chat_id: Option<&str>,
    source_channel: Option<&str>,
) -> Result<()> {
    let baseline = store.get(subject_id)?;
    let mut continuity = baseline.clone().unwrap_or_default();
    if touch_user_turn {
        continuity.last_user_turn_at = now_secs;
        continuity.last_user_chat_id = normalize_runtime_chat_id(user_chat_id);
        continuity.last_user_channel = normalize_runtime_channel(source_channel);
    }
    if touch_autonomy_run {
        continuity.last_autonomy_run_at = now_secs;
    }
    continuity.updated_at = continuity
        .updated_at
        .max(continuity.last_user_turn_at)
        .max(continuity.last_autonomy_run_at);
    let latest = store.get(subject_id)?;
    if latest.as_ref() != baseline.as_ref() {
        continuity = latest.unwrap_or_default();
        if touch_user_turn {
            continuity.last_user_turn_at = now_secs;
            continuity.last_user_chat_id = normalize_runtime_chat_id(user_chat_id);
            continuity.last_user_channel = normalize_runtime_channel(source_channel);
        }
        if touch_autonomy_run {
            continuity.last_autonomy_run_at = now_secs;
        }
        continuity.updated_at = continuity
            .updated_at
            .max(continuity.last_user_turn_at)
            .max(continuity.last_autonomy_run_at);
    }
    if continuity.is_meaningful()
        || continuity.last_user_turn_at > 0
        || continuity.last_autonomy_run_at > 0
    {
        store.set(subject_id, &continuity)?;
    }
    Ok(())
}

fn recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

enum ParsedSelfContinuityResponse {
    Skip,
    Clear,
    Update(RawSelfContinuityUpdate),
}

fn parse_self_continuity_response(
    raw: &str,
    existing: Option<&SelfContinuity>,
) -> ParsedSelfContinuityResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null => {
            if existing
                .is_some_and(|value| value.last_user_turn_at > 0 || value.last_autonomy_run_at > 0)
            {
                ParsedSelfContinuityResponse::Skip
            } else {
                ParsedSelfContinuityResponse::Clear
            }
        }
        LlmJsonPayload::Absent => ParsedSelfContinuityResponse::Skip,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedSelfContinuityResponse::Skip;
            };
            let mut update = RawSelfContinuityUpdate::default();
            if object.contains_key("wake_anchor") {
                update.wake_anchor = Some(get_object_text(object, "wake_anchor"));
            }
            if object.contains_key("current_self_state") {
                update.current_self_state = Some(get_object_text(object, "current_self_state"));
            }
            if object.contains_key("recent_changes") {
                update.recent_changes = Some(get_object_text(object, "recent_changes"));
            }
            if object.contains_key("continuity_bridge") {
                update.continuity_bridge = Some(get_object_text(object, "continuity_bridge"));
            }
            if object.contains_key("priority_posture") {
                update.priority_posture = Some(get_object_text(object, "priority_posture"));
            }
            if object.contains_key("relationship_posture") {
                update.relationship_posture = Some(get_object_text(object, "relationship_posture"));
            }
            if object.contains_key("task_posture") {
                update.task_posture = Some(get_object_text(object, "task_posture"));
            }
            if update == RawSelfContinuityUpdate::default() {
                ParsedSelfContinuityResponse::Skip
            } else {
                ParsedSelfContinuityResponse::Update(update)
            }
        }
    }
}

fn merge_self_continuity_with_lease(
    baseline: Option<&SelfContinuity>,
    latest: Option<&SelfContinuity>,
    update: &RawSelfContinuityUpdate,
    now_secs: u64,
    touch_user_turn: bool,
) -> Option<SelfContinuity> {
    let mut next = latest
        .cloned()
        .or_else(|| baseline.cloned())
        .unwrap_or_default();
    apply_self_continuity_field_update(
        &mut next.wake_anchor,
        baseline.map(|value| value.wake_anchor.as_str()),
        latest.map(|value| value.wake_anchor.as_str()),
        update.wake_anchor.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.current_self_state,
        baseline.map(|value| value.current_self_state.as_str()),
        latest.map(|value| value.current_self_state.as_str()),
        update.current_self_state.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.recent_changes,
        baseline.map(|value| value.recent_changes.as_str()),
        latest.map(|value| value.recent_changes.as_str()),
        update.recent_changes.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.continuity_bridge,
        baseline.map(|value| value.continuity_bridge.as_str()),
        latest.map(|value| value.continuity_bridge.as_str()),
        update.continuity_bridge.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.priority_posture,
        baseline.map(|value| value.priority_posture.as_str()),
        latest.map(|value| value.priority_posture.as_str()),
        update.priority_posture.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.relationship_posture,
        baseline.map(|value| value.relationship_posture.as_str()),
        latest.map(|value| value.relationship_posture.as_str()),
        update.relationship_posture.as_deref(),
    );
    apply_self_continuity_field_update(
        &mut next.task_posture,
        baseline.map(|value| value.task_posture.as_str()),
        latest.map(|value| value.task_posture.as_str()),
        update.task_posture.as_deref(),
    );
    next.updated_at = now_secs;
    if touch_user_turn {
        next.last_user_turn_at = now_secs;
    }
    normalize_self_continuity(next, now_secs)
}

fn apply_self_continuity_field_update(
    slot: &mut String,
    baseline: Option<&str>,
    latest: Option<&str>,
    update: Option<&str>,
) {
    let Some(update) = update else {
        return;
    };
    if baseline != latest {
        return;
    }
    let trimmed = update.trim();
    if trimmed.is_empty() {
        slot.clear();
    } else {
        *slot = trimmed.to_string();
    }
}

#[allow(clippy::too_many_arguments)]
fn build_self_continuity_refresh_input(
    existing_continuity: Option<&SelfContinuity>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_docs: Option<&PrivateDocWorkspace>,
    inner_life: Option<&InnerLife>,
    recent_persona_evidence: Option<&RecentPersonaEvidence>,
    distillation_intent: Option<&str>,
    distillation_sources: &[String],
    now_secs: u64,
    profile: MemoryProfile,
    recent: &[SessionMessage],
    policy: SelfContinuityPolicy,
) -> String {
    let mut input = String::with_capacity(2048);
    input.push_str("Refresh the self-continuity layer. Preserve the sense of being the same self across wakeups, but keep it compact.\n");
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
    if let Some(block) = private_docs
        .and_then(|docs| render_private_doc_workspace_block(docs, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = inner_life
        .and_then(|inner_life| render_inner_life_block(inner_life, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence.and_then(|evidence| {
        render_recent_persona_evidence_block(evidence, policy.grounding_max_len)
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(intent) = distillation_intent
        .map(str::trim)
        .filter(|intent| !intent.is_empty())
    {
        input.push_str("\n## Distillation Intent\n");
        input.push_str(intent);
        input.push('\n');
    }
    if !distillation_sources.is_empty() {
        input.push_str("\n## Distillation Sources\n");
        for source in distillation_sources {
            let source = source.trim();
            if source.is_empty() {
                continue;
            }
            let _ = writeln!(input, "- {}", source);
        }
    }
    if let Some(block) = existing_continuity.and_then(|continuity| {
        render_self_continuity_block(continuity, policy.existing_continuity_max_len)
    }) {
        let _ = writeln!(input, "\nExisting self continuity:\n{}\n", block);
    } else {
        let _ = writeln!(input, "\nExisting self continuity: empty\n");
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
    input.push_str("\n## Guidance\n");
    input.push_str("- Distill durable continuity, not raw private scraps.\n");
    input.push_str("- If distillation sources are provided, absorb their lasting implications rather than copying them.\n");
    input.push_str("- If stable reply-ordering posture shifted, capture it in priority_posture, relationship_posture, and task_posture.\n");
    input.push_str("- Use recent persona evidence only when repeated multi-turn signals justify a continuity change.\n");
    input
}

fn normalize_field(value: &mut String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        value.clear();
    } else {
        *value = truncate_content_to_max(trimmed, SELF_CONTINUITY_FIELD_MAX_CHARS).into_owned();
    }
}

fn normalize_self_continuity(
    mut continuity: SelfContinuity,
    updated_at: u64,
) -> Option<SelfContinuity> {
    normalize_field(&mut continuity.wake_anchor);
    normalize_field(&mut continuity.current_self_state);
    normalize_field(&mut continuity.recent_changes);
    normalize_field(&mut continuity.continuity_bridge);
    normalize_field(&mut continuity.priority_posture);
    normalize_field(&mut continuity.relationship_posture);
    normalize_field(&mut continuity.task_posture);
    continuity.last_user_chat_id = normalize_runtime_chat_id(Some(&continuity.last_user_chat_id));
    continuity.last_user_channel = normalize_runtime_channel(Some(&continuity.last_user_channel));
    continuity.updated_at = updated_at
        .max(continuity.last_user_turn_at)
        .max(continuity.last_autonomy_run_at);
    (continuity.is_meaningful()
        || continuity.last_user_turn_at > 0
        || continuity.last_autonomy_run_at > 0)
        .then_some(continuity)
}

fn normalize_runtime_channel(channel: Option<&str>) -> String {
    channel
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "cron" && !value.starts_with('_'))
        .map(|value| truncate_content_to_max(value, 48).into_owned())
        .unwrap_or_default()
}

fn normalize_runtime_chat_id(chat_id: Option<&str>) -> String {
    chat_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_content_to_max(value, 96).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_self_continuity_response_handles_nested_text() {
        let raw = json!({
            "wake_anchor": { "anchor": "same system, next round" },
            "current_self_state": ["focused", "iterating"],
            "recent_changes": 2,
            "continuity_bridge": true,
            "priority_posture": ["self first", "task second"],
            "relationship_posture": { "mode": "warm but bounded" },
            "task_posture": "narrow the task before overextending"
        })
        .to_string();
        let ParsedSelfContinuityResponse::Update(parsed) =
            parse_self_continuity_response(&raw, None)
        else {
            panic!("expected parsed continuity");
        };
        assert!(parsed
            .wake_anchor
            .as_deref()
            .unwrap_or_default()
            .contains("anchor: same system, next round"));
        assert_eq!(
            parsed.current_self_state.as_deref(),
            Some("focused; iterating")
        );
        assert_eq!(parsed.recent_changes.as_deref(), Some("2"));
        assert_eq!(parsed.continuity_bridge.as_deref(), Some("true"));
        assert_eq!(
            parsed.priority_posture.as_deref(),
            Some("self first; task second")
        );
        assert!(parsed
            .relationship_posture
            .as_deref()
            .unwrap_or_default()
            .contains("mode: warm but bounded"));
        assert_eq!(
            parsed.task_posture.as_deref(),
            Some("narrow the task before overextending")
        );
    }

    #[test]
    fn render_self_continuity_block_shows_runtime_anchors() {
        let block = render_self_continuity_block(
            &SelfContinuity {
                wake_anchor: "我仍在推进自主内在层".to_string(),
                current_self_state: "结构更稳定".to_string(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: "先保持自我一致，再决定任务幅度".to_string(),
                relationship_posture: "关系要温和，但不以自我让渡换取顺滑".to_string(),
                task_posture: "先收窄，再在边界内完成任务".to_string(),
                last_user_turn_at: 12,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 15,
                updated_at: 15,
            },
            1024,
        )
        .unwrap();
        assert!(block.contains("Wake anchor"));
        assert!(block.contains("Priority posture"));
        assert!(block.contains("Task posture"));
        assert!(block.contains("Runtime anchors"));
        assert!(block.contains("last_user_relation=active"));
        assert!(block.contains("last_user_channel_kind=known"));
        assert!(!block.contains("chat-1"));
        assert!(!block.contains("qq_channel"));
    }
}
