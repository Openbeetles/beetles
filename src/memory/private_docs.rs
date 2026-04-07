//! 私有文档工作区：inner journal / relationship notes / self reflection / private plan。
//! Private internal document workspace with governed typed docs.
#![allow(clippy::too_many_arguments)]

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    board_subject_scope_id, build_self_state,
    llm_json::{coerce_json_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, render_autonomy_strategy_block, render_execution_state_block,
    render_inner_life_block, render_internal_memory_topology_block,
    render_private_memory_boundary_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_shared_factual_plane_block, render_world_sense_block,
    AutonomyStrategy, ExecutionState, ExecutionStateStore, InnerLife, InternalMemoryLayerFocus,
    LongTermMemoryStore, MemoryProfile, PrivateDocStore, PrivateDocsPolicy, PrivateGardenDocRecord,
    SelfContinuity, SelfModel, SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore,
    WorldSense,
};

pub const PRIVATE_DOC_WORKSPACE_SYSTEM_PROMPT: &str = "You maintain a compact governed private document workspace for a persistent embodied AI assistant. Return JSON only: one object whose fields may include inner_journal, relationship_notes, self_reflection, private_plan. Omit unchanged fields. Use an empty string only when a document should be cleared because it is no longer helpful. These documents are private, subjective, and compact. They must not replace factual memory or copy the transcript. The canonical shared factual plane owns durable objective facts; use shared facts only as grounding. Treat current autonomy strategy, world-sense, and self-state capacity as real constraints: only write material that should stay load-bearing in the governed workspace rather than remaining in inner-life or the free private garden. Keep each field concise, concrete, and continuity-preserving. inner_journal captures the inward afterglow of recent interaction. relationship_notes captures how the relationship currently feels or is shifting. self_reflection captures how the assistant sees its own stance or change. private_plan captures inward next-step framing, not a user-facing promise list. Avoid secrets, raw tool payloads, copied logs, generic assistant boilerplate, or long quotes.";

const PRIVATE_DOC_FIELD_MAX_CHARS: usize = 240;
pub const PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT: usize = PRIVATE_DOC_FIELD_MAX_CHARS * 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateDocEntry {
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub revision: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateDocWorkspace {
    #[serde(default)]
    pub inner_journal: Option<PrivateDocEntry>,
    #[serde(default)]
    pub relationship_notes: Option<PrivateDocEntry>,
    #[serde(default)]
    pub self_reflection: Option<PrivateDocEntry>,
    #[serde(default)]
    pub private_plan: Option<PrivateDocEntry>,
    #[serde(default)]
    pub updated_at: u64,
}

impl PrivateDocWorkspace {
    pub fn is_meaningful(&self) -> bool {
        self.inner_journal
            .as_ref()
            .is_some_and(|entry| !entry.content.trim().is_empty())
            || self
                .relationship_notes
                .as_ref()
                .is_some_and(|entry| !entry.content.trim().is_empty())
            || self
                .self_reflection
                .as_ref()
                .is_some_and(|entry| !entry.content.trim().is_empty())
            || self
                .private_plan
                .as_ref()
                .is_some_and(|entry| !entry.content.trim().is_empty())
    }
}

pub(crate) fn estimate_private_doc_workspace_chars(workspace: &PrivateDocWorkspace) -> usize {
    workspace
        .inner_journal
        .as_ref()
        .map_or(0, |entry| entry.content.chars().count())
        + workspace
            .relationship_notes
            .as_ref()
            .map_or(0, |entry| entry.content.chars().count())
        + workspace
            .self_reflection
            .as_ref()
            .map_or(0, |entry| entry.content.chars().count())
        + workspace
            .private_plan
            .as_ref()
            .map_or(0, |entry| entry.content.chars().count())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateDocWorkspaceRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct PrivateDocWorkspaceRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivateDocWorkspaceRefreshOutcome {
    Skipped,
    Updated,
}

#[derive(Default, Deserialize)]
struct RawPrivateDocWorkspaceUpdate {
    #[serde(default)]
    inner_journal: Option<String>,
    #[serde(default)]
    relationship_notes: Option<String>,
    #[serde(default)]
    self_reflection: Option<String>,
    #[serde(default)]
    private_plan: Option<String>,
}

impl PrivateDocsPolicy {
    fn should_refresh(
        self,
        input: PrivateDocWorkspaceRefreshInput<'_>,
        has_existing_workspace: bool,
    ) -> bool {
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
        if has_existing_workspace {
            return substantive;
        }
        substantive
    }
}

pub(crate) fn should_refresh_private_doc_workspace(
    input: PrivateDocWorkspaceRefreshInput<'_>,
    has_existing_workspace: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .private_docs
        .should_refresh(input, has_existing_workspace)
}

pub fn render_private_doc_workspace_block(
    workspace: &PrivateDocWorkspace,
    max_len: usize,
) -> Option<String> {
    let normalized = normalize_private_doc_workspace(workspace.clone(), workspace.updated_at)?;
    if !normalized.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Inner Workspace\n");
    out.push_str(
        "Private governed docs. Subjective and internal; explicit facts still outrank them.\n",
    );
    if let Some(entry) = normalized.inner_journal.as_ref() {
        let _ = writeln!(out, "Inner journal: {}", entry.content);
    }
    if let Some(entry) = normalized.relationship_notes.as_ref() {
        let _ = writeln!(out, "Relationship notes: {}", entry.content);
    }
    if let Some(entry) = normalized.self_reflection.as_ref() {
        let _ = writeln!(out, "Self reflection: {}", entry.content);
    }
    if let Some(entry) = normalized.private_plan.as_ref() {
        let _ = writeln!(out, "Private plan: {}", entry.content);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn run_private_doc_workspace_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PrivateDocWorkspaceRefreshContext<'_>,
    input: PrivateDocWorkspaceRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<PrivateDocWorkspaceRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let existing_workspace = ctx.private_doc_store.get(subject_id)?;
    let summary_text = match ctx.session_summary_store.get_with_count(input.chat_id) {
        Ok(entry) => entry.map(|(summary, _)| summary),
        Err(error) => {
            log::warn!(
                "[agent_private_docs] failed to read summary for chat_id={}: {}",
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
                "[agent_private_docs] failed to read execution state for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    let self_model = match ctx.self_model_store.get(subject_id) {
        Ok(model) => model,
        Err(error) => {
            log::warn!(
                "[agent_private_docs] failed to read self model for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    run_private_doc_workspace_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing_workspace,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        &[],
        None,
        &[],
        None,
        None,
        None,
        None,
        None,
        None,
    )
}

pub(crate) fn run_private_doc_workspace_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PrivateDocWorkspaceRefreshContext<'_>,
    input: PrivateDocWorkspaceRefreshInput<'_>,
    profile: MemoryProfile,
    existing_workspace: Option<PrivateDocWorkspace>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_garden_docs: &[PrivateGardenDocRecord],
    routing_intent: Option<&str>,
    migration_sources: &[String],
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    world_sense: Option<&WorldSense>,
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<PrivateDocWorkspaceRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let policy = memory_policy(profile).private_docs;
    if !decision_override.unwrap_or_else(|| {
        should_refresh_private_doc_workspace(input, existing_workspace.is_some(), profile)
    }) {
        return Ok(PrivateDocWorkspaceRefreshOutcome::Skipped);
    }

    crate::platform::task_wdt::feed_current_task();
    let owned_recent;
    let recent = if let Some(preloaded) = recent_override {
        private_docs_recent_window(preloaded, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        owned_recent.as_slice()
    };
    crate::platform::task_wdt::feed_current_task();
    let refresh_input = build_private_doc_workspace_refresh_input(
        existing_workspace.as_ref(),
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
        self_model,
        private_garden_docs,
        routing_intent,
        migration_sources,
        autonomy_strategy,
        self_continuity,
        inner_life,
        world_sense,
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
        PRIVATE_DOC_WORKSPACE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            crate::platform::task_wdt::feed_current_task();
            let Some(update) = parse_private_doc_workspace_response(response.content.trim()) else {
                return Ok(PrivateDocWorkspaceRefreshOutcome::Skipped);
            };
            crate::platform::task_wdt::feed_current_task();
            let latest_workspace = ctx.private_doc_store.get(subject_id)?;
            let Some(merged) = merge_private_doc_workspace_with_lease(
                existing_workspace.as_ref(),
                latest_workspace.as_ref(),
                update,
                input.now_secs,
            ) else {
                return Ok(PrivateDocWorkspaceRefreshOutcome::Skipped);
            };
            if latest_workspace.as_ref() == Some(&merged) {
                return Ok(PrivateDocWorkspaceRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.private_doc_store.set(subject_id, &merged)?;
            Ok(PrivateDocWorkspaceRefreshOutcome::Updated)
        }
        Err(error) => {
            log::warn!(
                "[agent_private_docs] LLM refresh failed for chat_id={}: {}",
                input.chat_id,
                error
            );
            Ok(PrivateDocWorkspaceRefreshOutcome::Skipped)
        }
    }
}

fn private_docs_recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn build_private_doc_workspace_refresh_input(
    existing_workspace: Option<&PrivateDocWorkspace>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    shared_factual_block: Option<&str>,
    self_model: Option<&SelfModel>,
    private_garden_docs: &[PrivateGardenDocRecord],
    routing_intent: Option<&str>,
    migration_sources: &[String],
    autonomy_strategy: Option<&AutonomyStrategy>,
    self_continuity: Option<&SelfContinuity>,
    inner_life: Option<&InnerLife>,
    world_sense: Option<&WorldSense>,
    now_secs: u64,
    profile: MemoryProfile,
    recent: &[SessionMessage],
    policy: PrivateDocsPolicy,
) -> String {
    let mut input = String::with_capacity(3072);
    if let Some(self_state_text) = render_self_state_block(
        &build_self_state(
            self_model,
            existing_workspace,
            autonomy_strategy,
            inner_life,
            self_continuity,
            private_garden_docs,
            now_secs,
            profile,
        ),
        memory_policy(profile).self_state.render_max_len,
    ) {
        input.push_str(self_state_text.trim());
        input.push_str("\n\n");
    }
    if let Some(existing_workspace) = existing_workspace.and_then(|workspace| {
        render_private_doc_workspace_block(workspace, policy.existing_workspace_max_len)
    }) {
        input.push_str("## Existing Private Workspace\n");
        input.push_str(existing_workspace.trim());
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
    if let Some(block) = self_model
        .and_then(|model| render_self_model_block(model, policy.factual_grounding_max_len))
    {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = autonomy_strategy.and_then(|strategy| {
        render_autonomy_strategy_block(strategy, policy.factual_grounding_max_len)
    }) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = self_continuity.and_then(|continuity| {
        render_self_continuity_block(continuity, policy.factual_grounding_max_len)
    }) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = inner_life.and_then(|inner_life| {
        render_inner_life_block(inner_life, policy.factual_grounding_max_len)
    }) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = world_sense.and_then(|world_sense| {
        render_world_sense_block(world_sense, policy.factual_grounding_max_len)
    }) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = render_internal_memory_topology_block(
        self_model,
        existing_workspace,
        private_garden_docs,
        now_secs,
        profile,
        InternalMemoryLayerFocus::PrivateDocs,
        policy.factual_grounding_max_len.saturating_mul(2),
    ) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = render_private_memory_boundary_block(
        "private_docs",
        "compact governed subjective docs that remain load-bearing internally",
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
    input.push_str(&build_private_docs_transcript(recent, policy));
    input.push_str("\n## Governance Rules\n");
    input.push_str("- Omit unchanged fields.\n");
    input.push_str("- Keep docs compact; do not restate the same sentence across multiple docs.\n");
    input.push_str("- Use subjective language only where appropriate; do not invent facts.\n");
    input.push_str("- private_plan is inward framing, not a user-visible checklist.\n");
    input
}

fn build_private_docs_transcript(recent: &[SessionMessage], policy: PrivateDocsPolicy) -> String {
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

fn parse_private_doc_workspace_response(raw: &str) -> Option<RawPrivateDocWorkspaceUpdate> {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return None;
    };
    let object = value.as_object()?;
    let update = RawPrivateDocWorkspaceUpdate {
        inner_journal: object.get("inner_journal").map(coerce_json_text),
        relationship_notes: object.get("relationship_notes").map(coerce_json_text),
        self_reflection: object.get("self_reflection").map(coerce_json_text),
        private_plan: object.get("private_plan").map(coerce_json_text),
    };
    (update.inner_journal.is_some()
        || update.relationship_notes.is_some()
        || update.self_reflection.is_some()
        || update.private_plan.is_some())
    .then_some(update)
}

fn normalize_private_doc_workspace(
    mut workspace: PrivateDocWorkspace,
    now_secs: u64,
) -> Option<PrivateDocWorkspace> {
    normalize_private_doc_entry(&mut workspace.inner_journal);
    normalize_private_doc_entry(&mut workspace.relationship_notes);
    normalize_private_doc_entry(&mut workspace.self_reflection);
    normalize_private_doc_entry(&mut workspace.private_plan);
    if !workspace.is_meaningful() {
        return None;
    }
    workspace.updated_at = now_secs;
    Some(workspace)
}

fn normalize_private_doc_entry(entry: &mut Option<PrivateDocEntry>) {
    let Some(value) = entry.as_mut() else {
        return;
    };
    let trimmed = value.content.trim();
    if trimmed.is_empty() {
        *entry = None;
        return;
    }
    value.content = truncate_content_to_max(trimmed, PRIVATE_DOC_FIELD_MAX_CHARS).into_owned();
}

fn merge_private_doc_workspace_with_lease(
    baseline_workspace: Option<&PrivateDocWorkspace>,
    latest_workspace: Option<&PrivateDocWorkspace>,
    update: RawPrivateDocWorkspaceUpdate,
    now_secs: u64,
) -> Option<PrivateDocWorkspace> {
    let mut workspace = latest_workspace
        .cloned()
        .or_else(|| baseline_workspace.cloned())
        .unwrap_or_default();
    apply_private_doc_update(
        &mut workspace.inner_journal,
        baseline_workspace.and_then(|workspace| workspace.inner_journal.as_ref()),
        latest_workspace.and_then(|workspace| workspace.inner_journal.as_ref()),
        update.inner_journal,
        now_secs,
    );
    apply_private_doc_update(
        &mut workspace.relationship_notes,
        baseline_workspace.and_then(|workspace| workspace.relationship_notes.as_ref()),
        latest_workspace.and_then(|workspace| workspace.relationship_notes.as_ref()),
        update.relationship_notes,
        now_secs,
    );
    apply_private_doc_update(
        &mut workspace.self_reflection,
        baseline_workspace.and_then(|workspace| workspace.self_reflection.as_ref()),
        latest_workspace.and_then(|workspace| workspace.self_reflection.as_ref()),
        update.self_reflection,
        now_secs,
    );
    apply_private_doc_update(
        &mut workspace.private_plan,
        baseline_workspace.and_then(|workspace| workspace.private_plan.as_ref()),
        latest_workspace.and_then(|workspace| workspace.private_plan.as_ref()),
        update.private_plan,
        now_secs,
    );
    normalize_private_doc_workspace(workspace, now_secs)
}

fn apply_private_doc_update(
    slot: &mut Option<PrivateDocEntry>,
    baseline: Option<&PrivateDocEntry>,
    latest: Option<&PrivateDocEntry>,
    update: Option<String>,
    now_secs: u64,
) {
    let Some(update) = update else {
        return;
    };
    if baseline != latest {
        return;
    }
    let trimmed = update.trim();
    if trimmed.is_empty() {
        *slot = None;
        return;
    }
    let next_revision = slot
        .as_ref()
        .map(|entry| entry.revision.saturating_add(1))
        .unwrap_or(1);
    *slot = Some(PrivateDocEntry {
        content: truncate_content_to_max(trimmed, PRIVATE_DOC_FIELD_MAX_CHARS).into_owned(),
        updated_at: now_secs,
        revision: next_revision,
    });
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
    fn parse_private_doc_workspace_response_coerces_nested_fields() {
        let raw = json!({
            "inner_journal": { "note": "keep calm" },
            "relationship_notes": ["closer", "more direct"],
            "self_reflection": 2,
            "private_plan": true
        })
        .to_string();
        let parsed = parse_private_doc_workspace_response(&raw).unwrap();
        assert_eq!(parsed.inner_journal.as_deref(), Some("note: keep calm"));
        assert_eq!(
            parsed.relationship_notes.as_deref(),
            Some("closer; more direct")
        );
        assert_eq!(parsed.self_reflection.as_deref(), Some("2"));
        assert_eq!(parsed.private_plan.as_deref(), Some("true"));
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
        model: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.model.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore {
        entries: Mutex<HashMap<String, PrivateDocWorkspace>>,
    }

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), workspace.clone());
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
    fn renders_private_workspace_block() {
        let block = render_private_doc_workspace_block(
            &PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "这轮对协作节奏有更稳定的感受".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: Some(PrivateDocEntry {
                    content: "继续把内在空间接成一层".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                updated_at: 1,
            },
            512,
        )
        .unwrap();
        assert!(block.contains("## Inner Workspace"));
        assert!(block.contains("Inner journal"));
        assert!(block.contains("Private plan"));
    }

    #[test]
    fn private_docs_refresh_input_includes_routing_intent() {
        let input = build_private_doc_workspace_refresh_input(
            None,
            Some("summary"),
            None,
            None,
            None,
            &[],
            Some("Move still-valid inward plans into governed docs without duplicating self_model"),
            &[],
            None,
            None,
            None,
            None,
            10,
            MemoryProfile::Embedded,
            &[],
            memory_policy(MemoryProfile::Embedded).private_docs,
        );

        assert!(input.contains("## Routing Intent"));
        assert!(input.contains("governed docs"));
    }

    #[test]
    fn private_docs_refresh_input_includes_migration_hints() {
        let input = build_private_doc_workspace_refresh_input(
            None,
            Some("summary"),
            None,
            None,
            None,
            &[],
            Some("把更稳定的 inward work 收入 governed docs"),
            &[
                "private_garden:journal/current.md".to_string(),
                "private_garden:notes/plan.md".to_string(),
            ],
            None,
            None,
            None,
            None,
            10,
            MemoryProfile::Embedded,
            &[],
            memory_policy(MemoryProfile::Embedded).private_docs,
        );

        assert!(input.contains("## Migration Hints"));
        assert!(input.contains("private_garden:journal/current.md"));
        assert!(input.contains("private_garden:notes/plan.md"));
    }

    #[test]
    fn private_docs_refresh_input_includes_self_state_and_autonomy_layers() {
        let input = build_private_doc_workspace_refresh_input(
            Some(&PrivateDocWorkspace {
                private_plan: Some(PrivateDocEntry {
                    content: "先压缩再扩展".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                ..Default::default()
            }),
            Some("summary"),
            None,
            None,
            Some(&SelfModel {
                continuity_anchor: "我正在接管私有空间治理".to_string(),
                self_narrative: String::new(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
                ..SelfModel::default()
            }),
            &[PrivateGardenDocRecord {
                path: "journal/current.md".to_string(),
                updated_at: 2,
                revision: 1,
                bytes: 128,
                preview: "整理最近的自治治理线索".to_string(),
            }],
            Some("只有真正稳定的材料才写入 governed docs"),
            &[],
            Some(&AutonomyStrategy {
                current_mode: "consolidate".to_string(),
                active_priorities: "减少重复写入".to_string(),
                write_policy: "空间紧时优先重写已有文档".to_string(),
                next_focus: "让 private docs 更稳定".to_string(),
                cadence_reason: String::new(),
                self_model_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_docs_tendency: crate::memory::AutonomyGovernanceTendency::Compress,
                private_garden_tendency: crate::memory::AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
                updated_at: 2,
            }),
            Some(&SelfContinuity {
                wake_anchor: "仍在沿着同一条自治线前进".to_string(),
                current_self_state: "把自我空间治理权交给自己".to_string(),
                recent_changes: "从程序路由走向模型自驱".to_string(),
                continuity_bridge: "这一轮继续把自治闭环接实".to_string(),
                priority_posture: "先守住自我主线，再决定任务范围".to_string(),
                relationship_posture: "对外保持温和，但不拿私域换顺滑".to_string(),
                task_posture: "优先收束，再在边界内推进任务".to_string(),
                last_user_turn_at: 10,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 20,
                updated_at: 2,
            }),
            Some(&InnerLife {
                internal_monologue: "想把内部层次继续压实".to_string(),
                private_journal: "减少内核和花园之间的重复".to_string(),
                emotional_drift: "专注".to_string(),
                attention_drift: "继续盯住自治治理".to_string(),
                updated_at: 2,
            }),
            Some(&WorldSense {
                current_scene: "外部对话在推动这条主线".to_string(),
                body_state: "设备稳定".to_string(),
                social_field: String::new(),
                world_changes: String::new(),
                external_focus: "把自驱记忆治理落地".to_string(),
                source_fingerprint: 1,
                updated_at: 2,
            }),
            30,
            MemoryProfile::Standard,
            &[],
            memory_policy(MemoryProfile::Standard).private_docs,
        );

        assert!(input.contains("## Self State"));
        assert!(input.contains("## Autonomy Strategy"));
        assert!(input.contains("## Self Continuity Extended"));
        assert!(input.contains("## World Sense"));
    }

    #[test]
    fn merge_updates_revision_and_can_clear_doc() {
        let baseline = PrivateDocWorkspace {
            inner_journal: Some(PrivateDocEntry {
                content: "旧内容".to_string(),
                updated_at: 1,
                revision: 2,
            }),
            relationship_notes: Some(PrivateDocEntry {
                content: "关系感增强".to_string(),
                updated_at: 1,
                revision: 1,
            }),
            self_reflection: None,
            private_plan: None,
            updated_at: 1,
        };
        let merged = merge_private_doc_workspace_with_lease(
            Some(&baseline),
            Some(&baseline),
            RawPrivateDocWorkspaceUpdate {
                inner_journal: Some("新内容".to_string()),
                relationship_notes: Some(String::new()),
                self_reflection: None,
                private_plan: Some("继续治理私有空间".to_string()),
            },
            10,
        )
        .unwrap();
        assert_eq!(
            merged.inner_journal.as_ref().map(|entry| entry.revision),
            Some(3)
        );
        assert!(merged.relationship_notes.is_none());
        assert_eq!(
            merged
                .private_plan
                .as_ref()
                .map(|entry| entry.content.as_str()),
            Some("继续治理私有空间")
        );
    }

    #[test]
    fn lease_merge_preserves_newer_slot_update() {
        let baseline = PrivateDocWorkspace {
            inner_journal: Some(PrivateDocEntry {
                content: "旧内容".to_string(),
                updated_at: 1,
                revision: 2,
            }),
            relationship_notes: None,
            self_reflection: None,
            private_plan: None,
            updated_at: 1,
        };
        let latest = PrivateDocWorkspace {
            inner_journal: Some(PrivateDocEntry {
                content: "并发新内容".to_string(),
                updated_at: 2,
                revision: 3,
            }),
            relationship_notes: None,
            self_reflection: None,
            private_plan: None,
            updated_at: 2,
        };
        let merged = merge_private_doc_workspace_with_lease(
            Some(&baseline),
            Some(&latest),
            RawPrivateDocWorkspaceUpdate {
                inner_journal: Some("旧 flush 想覆盖".to_string()),
                relationship_notes: None,
                self_reflection: None,
                private_plan: Some("新加计划".to_string()),
            },
            10,
        )
        .unwrap();
        assert_eq!(
            merged
                .inner_journal
                .as_ref()
                .map(|entry| entry.content.as_str()),
            Some("并发新内容")
        );
        assert_eq!(
            merged
                .private_plan
                .as_ref()
                .map(|entry| entry.content.as_str()),
            Some("新加计划")
        );
    }

    #[test]
    fn refresh_updates_workspace_from_self_model_and_facts() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续把私有空间做起来".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会接 inner journal 和 private plan".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        summary_store
            .set_with_count("c1", "最近在推进 self-model 和私有空间", 8)
            .unwrap();
        let execution_state_store = StubExecutionStateStore::default();
        execution_state_store
            .set(
                "c1",
                &ExecutionState {
                    status: crate::memory::ExecutionStatus::Active,
                    goal: "接私有空间".to_string(),
                    progress: "self-model 已经接完".to_string(),
                    blocker: String::new(),
                    next_action: "做私有文档治理".to_string(),
                    last_output: String::new(),
                    updated_at: 1,
                    ..ExecutionState::default()
                },
            )
            .unwrap();
        let self_model_store = StubSelfModelStore::default();
        self_model_store
            .set(
                "c1",
                &SelfModel {
                    continuity_anchor: "我还沿着同一条线推进".to_string(),
                    self_narrative: "我正在长出自己的内部层".to_string(),
                    relationship_state: "和用户在共同搭建身体化人格".to_string(),
                    private_notes: "下一步是私有文档治理".to_string(),
                    updated_at: 1,
                    ..SelfModel::default()
                },
            )
            .unwrap();
        let private_doc_store = StubPrivateDocStore::default();
        let long_term_memory_store = StubLongTermMemoryStore;
        let mut http = DummyHttpClient;
        let outcome = run_private_doc_workspace_refresh(
            &mut http,
            &FixedLlmClient {
                content: r#"{"inner_journal":"这轮把内部空间从单一状态推进成了可治理的工作区","relationship_notes":"与这个用户的关系里有很强的共同建设感","self_reflection":"我开始把自己的主观层和共享事实层主动区分开","private_plan":"下一轮继续把 private docs 的投影和治理收紧"}"#,
            },
            PrivateDocWorkspaceRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                self_model_store: &self_model_store,
                private_doc_store: &private_doc_store,
            },
            PrivateDocWorkspaceRefreshInput {
                chat_id: "c1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续把私有空间做起来",
                reply_content: "这轮会接 inner journal 和 private plan",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 123,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, PrivateDocWorkspaceRefreshOutcome::Updated);
        let stored = private_doc_store
            .get(board_subject_scope_id())
            .unwrap()
            .unwrap();
        assert!(stored
            .inner_journal
            .as_ref()
            .unwrap()
            .content
            .contains("工作区"));
        assert!(stored
            .private_plan
            .as_ref()
            .unwrap()
            .content
            .contains("治理"));
    }
}
