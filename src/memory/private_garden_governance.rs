//! 私有花园治理：由 LLM 在回复后自主决定是否整理自由内部空间。
//! Post-reply LLM governance for the free private garden workspace.
#![allow(clippy::too_many_arguments)]

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use super::{
    board_subject_scope_id, build_private_garden_preview, build_private_garden_usage,
    build_self_state,
    llm_json::{coerce_json_string_list, coerce_json_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, normalize_private_garden_doc_path, private_garden_scope_id,
    render_autonomy_strategy_block, render_execution_state_block,
    render_internal_memory_topology_block, render_private_doc_workspace_block,
    render_self_model_block, render_self_state_block, summarize_private_garden_directories,
    AutonomyStrategy, ExecutionState, ExecutionStateStore, InternalMemoryLayerFocus, MemoryProfile,
    PrivateDocStore, PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord,
    PrivateGardenGovernancePolicy, PrivateGardenStore, SelfModel, SelfModelStore, SessionMessage,
    SessionStore, SessionSummaryStore, PRIVATE_GARDEN_MAX_DOC_BYTES,
};

pub const PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT: &str = "You govern a persistent AI assistant's private garden: a free-form, self-owned internal workspace. Return JSON only: either null, or one object with optional writes, moves, and deletes fields. writes must be an array of objects {path, content}; each write replaces the full document body at that path. moves must be an array of objects {from_path, to_path} for reorganizing or renaming existing documents. deletes must be an array of document paths to remove. Use this workspace for private drafts, internal organization, and exploratory self-work, not shared factual memory. Keep documents current by rewriting, merging, or relocating in place instead of accumulating a history trail. Create new docs only when they materially improve continuity or organization. Delete stale, duplicated, or low-value scratch material when useful. Do not copy raw tool payloads, logs, large quotes, secrets, or transcript fragments. Do not duplicate stable kernel material that already belongs in the governed private self-model or typed private docs. Return null when no garden change is worth making.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateGardenGovernanceInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct PrivateGardenGovernanceContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrivateGardenGovernanceOutcome {
    Skipped,
    Updated {
        writes: usize,
        moves: usize,
        deletes: usize,
    },
}

#[derive(Default, Deserialize)]
struct RawPrivateGardenGovernanceResponse {
    #[serde(default)]
    writes: Vec<RawPrivateGardenWrite>,
    #[serde(default)]
    moves: Vec<RawPrivateGardenMove>,
    #[serde(default)]
    deletes: Vec<String>,
}

#[derive(Deserialize)]
struct RawPrivateGardenWrite {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct RawPrivateGardenMove {
    from_path: String,
    to_path: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrivateGardenWriteAction {
    path: String,
    content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrivateGardenMoveAction {
    from_path: String,
    to_path: String,
}

struct PrivateGardenSnapshot {
    records: Vec<super::PrivateGardenDocRecord>,
    docs: Vec<PrivateGardenDoc>,
}

impl PrivateGardenGovernancePolicy {
    fn should_govern(
        self,
        input: PrivateGardenGovernanceInput<'_>,
        _has_existing_docs: bool,
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
        user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined_chars >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n')
    }
}

pub(crate) fn should_refresh_private_garden(
    input: PrivateGardenGovernanceInput<'_>,
    has_existing_docs: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .private_garden_governance
        .should_govern(input, has_existing_docs)
}

pub fn run_private_garden_governance(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PrivateGardenGovernanceContext<'_>,
    input: PrivateGardenGovernanceInput<'_>,
    profile: MemoryProfile,
) -> Result<PrivateGardenGovernanceOutcome> {
    let subject_id = board_subject_scope_id();
    let summary_text = match ctx.session_summary_store.get_with_count(input.chat_id) {
        Ok(entry) => entry.map(|(summary, _)| summary),
        Err(error) => {
            log::warn!(
                "[agent_private_garden] failed to read summary for chat_id={}: {}",
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
                "[agent_private_garden] failed to read execution state for chat_id={}: {}",
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
                "[agent_private_garden] failed to read self model for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    let private_workspace = match ctx.private_doc_store.get(subject_id) {
        Ok(workspace) => workspace,
        Err(error) => {
            log::warn!(
                "[agent_private_garden] failed to read private docs for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    run_private_garden_governance_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        private_workspace.as_ref(),
        None,
        None,
        &[],
        None,
        None,
    )
}

pub(crate) fn run_private_garden_governance_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PrivateGardenGovernanceContext<'_>,
    input: PrivateGardenGovernanceInput<'_>,
    profile: MemoryProfile,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    routing_intent: Option<&str>,
    upstream_cleanup_paths: &[String],
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<PrivateGardenGovernanceOutcome> {
    crate::platform::task_wdt::feed_current_task();
    let snapshot = load_private_garden_snapshot(ctx.private_garden_store, input.chat_id)?;
    if !decision_override.unwrap_or_else(|| {
        should_refresh_private_garden(input, !snapshot.records.is_empty(), profile)
    }) {
        return Ok(PrivateGardenGovernanceOutcome::Skipped);
    }

    let policy = memory_policy(profile).private_garden_governance;
    crate::platform::task_wdt::feed_current_task();
    let owned_recent;
    let recent = if let Some(preloaded) = recent_override {
        private_garden_recent_window(preloaded, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        private_garden_recent_window(owned_recent.as_slice(), policy.recent_message_count)
    };

    crate::platform::task_wdt::feed_current_task();
    let governance_input = build_private_garden_governance_input(
        summary_text,
        execution_state,
        self_model,
        private_workspace,
        autonomy_strategy,
        routing_intent,
        upstream_cleanup_paths,
        &snapshot,
        recent,
        input.now_secs,
        profile,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: governance_input,
    }];

    crate::platform::task_wdt::feed_current_task();
    match llm.chat(
        http,
        PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            crate::platform::task_wdt::feed_current_task();
            let Some(raw) = parse_private_garden_governance_response(response.content.trim())
            else {
                return Ok(PrivateGardenGovernanceOutcome::Skipped);
            };
            crate::platform::task_wdt::feed_current_task();
            let latest_snapshot =
                load_private_garden_snapshot(ctx.private_garden_store, input.chat_id)?;
            let (writes, moves, deletes) = normalize_private_garden_governance_actions(
                raw,
                &snapshot,
                &latest_snapshot,
                policy,
            );
            if writes.is_empty() && moves.is_empty() && deletes.is_empty() {
                return Ok(PrivateGardenGovernanceOutcome::Skipped);
            }
            for path in &deletes {
                crate::platform::task_wdt::feed_current_task();
                let _ = ctx
                    .private_garden_store
                    .delete(private_garden_scope_id(), path)?;
            }
            for move_action in &moves {
                crate::platform::task_wdt::feed_current_task();
                let _ = ctx.private_garden_store.move_doc(
                    private_garden_scope_id(),
                    &move_action.from_path,
                    &move_action.to_path,
                    input.now_secs,
                )?;
            }
            for write in &writes {
                crate::platform::task_wdt::feed_current_task();
                let _ = ctx.private_garden_store.write(
                    private_garden_scope_id(),
                    &write.path,
                    &write.content,
                    input.now_secs,
                )?;
            }
            Ok(PrivateGardenGovernanceOutcome::Updated {
                writes: writes.len(),
                moves: moves.len(),
                deletes: deletes.len(),
            })
        }
        Err(error) => {
            log::warn!(
                "[agent_private_garden] LLM governance failed for chat_id={}: {}",
                input.chat_id,
                error
            );
            Ok(PrivateGardenGovernanceOutcome::Skipped)
        }
    }
}

fn load_private_garden_snapshot(
    store: &dyn PrivateGardenStore,
    chat_id: &str,
) -> Result<PrivateGardenSnapshot> {
    crate::platform::task_wdt::feed_current_task();
    let _ = chat_id;
    let mut records = store.list(private_garden_scope_id(), usize::MAX)?;
    records.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut docs = Vec::with_capacity(records.len());
    for record in &records {
        crate::platform::task_wdt::feed_current_task();
        if let Some(doc) = store.read(private_garden_scope_id(), &record.path)? {
            docs.push(doc);
        }
    }
    docs.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(PrivateGardenSnapshot { records, docs })
}

fn private_garden_recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn build_private_garden_governance_input(
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    routing_intent: Option<&str>,
    upstream_cleanup_paths: &[String],
    snapshot: &PrivateGardenSnapshot,
    recent: &[SessionMessage],
    now_secs: u64,
    profile: MemoryProfile,
    policy: PrivateGardenGovernancePolicy,
) -> String {
    let mut input = String::with_capacity(4096);
    if let Some(self_state_text) = render_self_state_block(
        &build_self_state(
            self_model,
            private_workspace,
            autonomy_strategy,
            None,
            None,
            snapshot.records.as_slice(),
            now_secs,
            profile,
        ),
        memory_policy(profile).self_state.render_max_len,
    ) {
        input.push_str(self_state_text.trim());
        input.push_str("\n\n");
    }
    if let Some(topology_text) = render_internal_memory_topology_block(
        self_model,
        private_workspace,
        snapshot.records.as_slice(),
        now_secs,
        profile,
        InternalMemoryLayerFocus::PrivateGarden,
        policy.grounding_max_len.saturating_mul(2),
    ) {
        input.push_str(topology_text.trim());
        input.push_str("\n\n");
    }
    input.push_str("## Shared Grounding\n");
    if let Some(summary_text) = summary_text.map(str::trim).filter(|text| !text.is_empty()) {
        let summary = truncate_content_to_max(summary_text, policy.grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    } else {
        input.push_str("Summary: \n");
    }
    if let Some(block) = execution_state
        .and_then(|state| render_execution_state_block(state, policy.grounding_max_len))
    {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) =
        self_model.and_then(|model| render_self_model_block(model, policy.grounding_max_len))
    {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = private_workspace.and_then(|workspace| {
        render_private_doc_workspace_block(workspace, policy.grounding_max_len)
    }) {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(block) = autonomy_strategy
        .and_then(|strategy| render_autonomy_strategy_block(strategy, policy.grounding_max_len))
    {
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
    if !upstream_cleanup_paths.is_empty() {
        input.push_str("\n## Upstream Promotions Already Applied\n");
        input.push_str("These garden docs were already promoted upstream and deleted before this governance pass:\n");
        for path in upstream_cleanup_paths {
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            let _ = writeln!(input, "- {}", path);
        }
    }
    input.push_str("\n## Existing Private Garden\n");
    input.push_str(&render_private_garden_docs_snapshot(
        snapshot.docs.as_slice(),
        policy,
    ));
    input.push_str("\n## Recent Transcript\n");
    input.push_str(&build_private_garden_transcript(recent, policy));
    input.push_str("\n## Governance Rules\n");
    input.push_str(
        "- Keep the garden current; rewrite or merge in place instead of storing a timeline.\n",
    );
    input.push_str("- Prefer stable paths when updating existing working material.\n");
    input.push_str(
        "- Use moves when renaming or regrouping existing docs would keep the workspace cleaner.\n",
    );
    input.push_str("- Delete stale or overlapping scratch docs when they no longer help.\n");
    input.push_str(
        "- Only create a new doc when it materially improves private continuity or organization.\n",
    );
    input.push_str(
        "- Do not duplicate stable kernel material or copy raw transcript/tool payloads.\n",
    );
    input.push_str("- Return null if no meaningful garden change is needed.\n");
    input
}

fn render_private_garden_docs_snapshot(
    docs: &[PrivateGardenDoc],
    policy: PrivateGardenGovernancePolicy,
) -> String {
    if docs.is_empty() {
        return "None.\n".to_string();
    }
    let records = docs
        .iter()
        .map(|doc| PrivateGardenDocRecord {
            path: doc.path.clone(),
            updated_at: doc.updated_at,
            revision: doc.revision,
            bytes: doc.content.len(),
            preview: build_private_garden_preview(&doc.content),
        })
        .collect::<Vec<_>>();
    let usage = build_private_garden_usage(&records);
    let directories = summarize_private_garden_directories(&records, 4);
    let mut out = String::with_capacity(policy.existing_docs_max_chars.saturating_add(128));
    let _ = writeln!(
        out,
        "Workspace: {}/{} docs used ({} free), {}/{} bytes used ({} free).",
        usage.docs_used,
        usage.docs_limit,
        usage.docs_free,
        usage.bytes_used,
        usage.bytes_limit,
        usage.bytes_free
    );
    if !directories.is_empty() {
        out.push_str("Folders: ");
        for (idx, dir) in directories.iter().enumerate() {
            if idx > 0 {
                out.push_str("; ");
            }
            let _ = write!(
                out,
                "{} ({} docs, {} bytes)",
                dir.path, dir.doc_count, dir.bytes
            );
        }
        out.push_str("\n\n");
    }
    let mut remaining = policy.existing_docs_max_chars.saturating_sub(out.len());
    for doc in docs.iter().take(policy.existing_doc_count) {
        if remaining == 0 {
            break;
        }
        let content = truncate_content_to_max(&doc.content, policy.existing_doc_max_chars);
        let rendered = format!(
            "Path: {}\nRevision: {}\nUpdated: {}\nContent:\n{}\n\n",
            doc.path,
            doc.revision,
            doc.updated_at,
            scrub_credentials(content.as_ref())
        );
        let clipped = truncate_content_to_max(rendered.trim_end(), remaining);
        if clipped.trim().is_empty() {
            break;
        }
        out.push_str(clipped.as_ref());
        out.push_str("\n\n");
        remaining = remaining.saturating_sub(clipped.len().saturating_add(2));
    }
    if out.trim().is_empty() {
        "None.\n".to_string()
    } else {
        out
    }
}

fn build_private_garden_transcript(
    recent: &[SessionMessage],
    policy: PrivateGardenGovernancePolicy,
) -> String {
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

fn parse_private_garden_governance_response(
    raw: &str,
) -> Option<RawPrivateGardenGovernanceResponse> {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return None;
    };
    let object = value.as_object()?;
    let writes = object
        .get("writes")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(parse_private_garden_write_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let moves = object
        .get("moves")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(parse_private_garden_move_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let deletes = object
        .get("deletes")
        .map(coerce_json_string_list)
        .unwrap_or_default();
    (!writes.is_empty() || !moves.is_empty() || !deletes.is_empty()).then_some(
        RawPrivateGardenGovernanceResponse {
            writes,
            moves,
            deletes,
        },
    )
}

fn parse_private_garden_write_value(value: &serde_json::Value) -> Option<RawPrivateGardenWrite> {
    let object = value.as_object()?;
    let path = object
        .get("path")
        .and_then(|value| coerce_json_string_list(value).into_iter().next())
        .unwrap_or_default();
    let content = object
        .get("content")
        .map(coerce_json_text)
        .unwrap_or_default();
    (!path.trim().is_empty() && !content.trim().is_empty())
        .then_some(RawPrivateGardenWrite { path, content })
}

fn parse_private_garden_move_value(value: &serde_json::Value) -> Option<RawPrivateGardenMove> {
    let object = value.as_object()?;
    let from_path = object
        .get("from_path")
        .and_then(|value| coerce_json_string_list(value).into_iter().next())
        .unwrap_or_default();
    let to_path = object
        .get("to_path")
        .and_then(|value| coerce_json_string_list(value).into_iter().next())
        .unwrap_or_default();
    (!from_path.trim().is_empty() && !to_path.trim().is_empty())
        .then_some(RawPrivateGardenMove { from_path, to_path })
}

fn normalize_private_garden_governance_actions(
    raw: RawPrivateGardenGovernanceResponse,
    baseline_snapshot: &PrivateGardenSnapshot,
    latest_snapshot: &PrivateGardenSnapshot,
    policy: PrivateGardenGovernancePolicy,
) -> (
    Vec<PrivateGardenWriteAction>,
    Vec<PrivateGardenMoveAction>,
    Vec<String>,
) {
    let baseline_revisions = baseline_snapshot
        .docs
        .iter()
        .map(|doc| (doc.path.as_str(), doc.revision))
        .collect::<HashMap<_, _>>();
    let latest_map = latest_snapshot
        .docs
        .iter()
        .map(|doc| (doc.path.as_str(), doc))
        .collect::<HashMap<_, _>>();
    let mut writes_by_path = HashMap::<String, String>::new();
    for write in raw.writes.into_iter().take(policy.max_writes) {
        let Ok(path) = normalize_private_garden_doc_path(&write.path) else {
            continue;
        };
        let trimmed = write.content.trim();
        if trimmed.is_empty() || trimmed.len() > PRIVATE_GARDEN_MAX_DOC_BYTES {
            continue;
        }
        let content = truncate_content_to_max(trimmed, PRIVATE_GARDEN_MAX_DOC_BYTES).into_owned();
        if latest_map
            .get(path.as_str())
            .is_some_and(|existing| existing.content.trim() == content.trim())
        {
            continue;
        }
        if baseline_revisions.get(path.as_str()).copied()
            != latest_map.get(path.as_str()).map(|doc| doc.revision)
        {
            continue;
        }
        writes_by_path.insert(path, content);
    }
    let mut moves = Vec::new();
    let mut claimed_sources = HashSet::new();
    let mut claimed_targets = HashSet::new();
    for raw_move in raw.moves.into_iter().take(policy.max_moves) {
        let Ok(from_path) = normalize_private_garden_doc_path(&raw_move.from_path) else {
            continue;
        };
        let Ok(to_path) = normalize_private_garden_doc_path(&raw_move.to_path) else {
            continue;
        };
        let latest_source_revision = latest_map.get(from_path.as_str()).map(|doc| doc.revision);
        let baseline_source_revision = baseline_revisions.get(from_path.as_str()).copied();
        if from_path == to_path
            || baseline_source_revision.is_none()
            || latest_source_revision != baseline_source_revision
            || claimed_sources.contains(&from_path)
            || claimed_targets.contains(&to_path)
            || writes_by_path.contains_key(&from_path)
            || writes_by_path.contains_key(&to_path)
            || latest_map.get(to_path.as_str()).map(|doc| doc.revision)
                != baseline_revisions.get(to_path.as_str()).copied()
        {
            continue;
        }
        claimed_sources.insert(from_path.clone());
        claimed_targets.insert(to_path.clone());
        moves.push(PrivateGardenMoveAction { from_path, to_path });
    }
    let move_sources = moves
        .iter()
        .map(|action| action.from_path.clone())
        .collect::<HashSet<_>>();
    let write_paths = writes_by_path.keys().cloned().collect::<HashSet<_>>();
    let mut deletes = Vec::new();
    let mut seen_deletes = HashSet::new();
    for raw_path in raw.deletes.into_iter().take(policy.max_deletes) {
        let Ok(path) = normalize_private_garden_doc_path(&raw_path) else {
            continue;
        };
        if write_paths.contains(&path)
            || move_sources.contains(&path)
            || baseline_revisions.get(path.as_str()).copied()
                != latest_map.get(path.as_str()).map(|doc| doc.revision)
        {
            continue;
        }
        if seen_deletes.insert(path.clone()) {
            deletes.push(path);
        }
    }
    let mut writes = writes_by_path
        .into_iter()
        .map(|(path, content)| PrivateGardenWriteAction { path, content })
        .collect::<Vec<_>>();
    moves.sort_by(|a, b| {
        a.from_path
            .cmp(&b.from_path)
            .then_with(|| a.to_path.cmp(&b.to_path))
    });
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    deletes.sort();
    (writes, moves, deletes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, StopReason};
    use crate::memory::PrivateGardenDocRecord;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn parse_private_garden_governance_response_coerces_nested_shapes() {
        let raw = json!({
            "writes": [
                {
                    "path": { "value": "journal/today.md" },
                    "content": ["line one", "line two"]
                }
            ],
            "moves": [
                {
                    "from_path": { "path": "scratch/idea.md" },
                    "to_path": "journal/idea.md"
                }
            ],
            "deletes": [{ "path": "scratch/old.md" }]
        })
        .to_string();
        let parsed = parse_private_garden_governance_response(&raw).unwrap();
        assert_eq!(parsed.writes.len(), 1);
        assert_eq!(parsed.writes[0].path, "journal/today.md");
        assert_eq!(parsed.writes[0].content, "line one; line two");
        assert_eq!(parsed.moves.len(), 1);
        assert_eq!(parsed.moves[0].from_path, "scratch/idea.md");
        assert_eq!(parsed.moves[0].to_path, "journal/idea.md");
        assert_eq!(parsed.deletes, vec!["scratch/old.md".to_string()]);
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
    struct StubSessionSummaryStore;

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore;

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore;

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _model: &SelfModel) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore;

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _workspace: &PrivateDocWorkspace) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<HashMap<String, PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(
            &self,
            _chat_id: &str,
            limit: usize,
        ) -> Result<Vec<super::super::PrivateGardenDocRecord>> {
            let mut docs = self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .map(|doc| super::super::PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: super::super::build_private_garden_preview(&doc.content),
                })
                .collect::<Vec<_>>();
            docs.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.path.cmp(&b.path))
            });
            docs.truncate(limit);
            Ok(docs)
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(doc_path)
                .cloned())
        }

        fn write(
            &self,
            _chat_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> Result<super::super::PrivateGardenDocRecord> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let revision = docs
                .get(doc_path)
                .map(|doc| doc.revision.saturating_add(1))
                .unwrap_or(1);
            let doc = PrivateGardenDoc {
                path: doc_path.to_string(),
                content: content.to_string(),
                updated_at: now_secs,
                revision,
            };
            docs.insert(doc_path.to_string(), doc.clone());
            Ok(super::super::PrivateGardenDocRecord {
                path: doc.path,
                updated_at: doc.updated_at,
                revision: doc.revision,
                bytes: doc.content.len(),
                preview: super::super::build_private_garden_preview(&doc.content),
            })
        }

        fn delete(&self, _chat_id: &str, doc_path: &str) -> Result<bool> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(doc_path)
                .is_some())
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            from_path: &str,
            to_path: &str,
            now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let Some(doc) = docs.remove(from_path) else {
                return Ok(None);
            };
            let moved = PrivateGardenDoc {
                path: to_path.to_string(),
                content: doc.content,
                updated_at: now_secs,
                revision: doc.revision.saturating_add(1),
            };
            docs.insert(to_path.to_string(), moved.clone());
            Ok(Some(PrivateGardenDocRecord {
                path: moved.path,
                updated_at: moved.updated_at,
                revision: moved.revision,
                bytes: moved.content.len(),
                preview: super::super::build_private_garden_preview(&moved.content),
            }))
        }
    }

    struct FixedLlmClient;

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
                content: r#"{"writes":[{"path":"journal/active.md","content":"把之前分散的想法收束成一份当前工作笔记。"}],"moves":[{"from_path":"drafts/live.md","to_path":"journal/live.md"}],"deletes":["scratch/old.md"]}"#.to_string(),
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
    fn private_garden_governance_writes_and_deletes_docs() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续整理你的内部空间".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我会把零散草稿收束掉".to_string(),
                },
            ],
        };
        let private_garden_store = StubPrivateGardenStore::default();
        private_garden_store
            .write("chat-1", "scratch/old.md", "过时草稿", 1)
            .unwrap();
        private_garden_store
            .write("chat-1", "drafts/live.md", "活跃草稿", 2)
            .unwrap();
        let mut http = DummyHttpClient;
        let outcome = run_private_garden_governance(
            &mut http,
            &FixedLlmClient,
            PrivateGardenGovernanceContext {
                session_store: &session_store,
                session_summary_store: &StubSessionSummaryStore,
                execution_state_store: &StubExecutionStateStore,
                self_model_store: &StubSelfModelStore,
                private_doc_store: &StubPrivateDocStore,
                private_garden_store: &private_garden_store,
            },
            PrivateGardenGovernanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续整理你的内部空间",
                reply_content: "我会把零散草稿收束掉",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 10,
            },
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert_eq!(
            outcome,
            PrivateGardenGovernanceOutcome::Updated {
                writes: 1,
                moves: 1,
                deletes: 1
            }
        );
        assert!(private_garden_store
            .read("chat-1", "scratch/old.md")
            .unwrap()
            .is_none());
        assert!(private_garden_store
            .read("chat-1", "journal/active.md")
            .unwrap()
            .is_some());
        assert!(private_garden_store
            .read("chat-1", "journal/live.md")
            .unwrap()
            .is_some());
    }

    #[test]
    fn private_garden_governance_input_includes_routing_intent() {
        let input = build_private_garden_governance_input(
            Some("summary"),
            None,
            None,
            None,
            None,
            Some("把探索性内容继续留在 garden，并顺手整理目录结构"),
            &[],
            &PrivateGardenSnapshot {
                records: vec![PrivateGardenDocRecord {
                    path: "journal/active.md".to_string(),
                    updated_at: 1,
                    revision: 1,
                    bytes: 16,
                    preview: "preview".to_string(),
                }],
                docs: vec![PrivateGardenDoc {
                    path: "journal/active.md".to_string(),
                    content: "活跃草稿".to_string(),
                    updated_at: 1,
                    revision: 1,
                }],
            },
            &[],
            10,
            MemoryProfile::Embedded,
            memory_policy(MemoryProfile::Embedded).private_garden_governance,
        );

        assert!(input.contains("## Routing Intent"));
        assert!(input.contains("继续留在 garden"));
    }

    #[test]
    fn private_garden_governance_input_includes_upstream_cleanup_context() {
        let input = build_private_garden_governance_input(
            Some("summary"),
            None,
            None,
            None,
            None,
            Some("继续整理剩余探索内容"),
            &[
                "journal/promoted.md".to_string(),
                "notes/merged.md".to_string(),
            ],
            &PrivateGardenSnapshot {
                records: vec![PrivateGardenDocRecord {
                    path: "journal/active.md".to_string(),
                    updated_at: 1,
                    revision: 1,
                    bytes: 16,
                    preview: "preview".to_string(),
                }],
                docs: vec![PrivateGardenDoc {
                    path: "journal/active.md".to_string(),
                    content: "活跃草稿".to_string(),
                    updated_at: 1,
                    revision: 1,
                }],
            },
            &[],
            10,
            MemoryProfile::Embedded,
            memory_policy(MemoryProfile::Embedded).private_garden_governance,
        );

        assert!(input.contains("## Upstream Promotions Already Applied"));
        assert!(input.contains("journal/promoted.md"));
        assert!(input.contains("notes/merged.md"));
    }

    #[test]
    fn normalize_private_garden_governance_actions_skips_invalid_or_duplicate_work() {
        let existing_docs = vec![PrivateGardenDoc {
            path: "journal/active.md".to_string(),
            content: "same".to_string(),
            updated_at: 1,
            revision: 1,
        }];
        let baseline_snapshot = PrivateGardenSnapshot {
            records: vec![PrivateGardenDocRecord {
                path: "journal/active.md".to_string(),
                updated_at: 1,
                revision: 1,
                bytes: 4,
                preview: "same".to_string(),
            }],
            docs: existing_docs.clone(),
        };
        let latest_snapshot = PrivateGardenSnapshot {
            records: baseline_snapshot.records.clone(),
            docs: existing_docs,
        };
        let (writes, moves, deletes) = normalize_private_garden_governance_actions(
            RawPrivateGardenGovernanceResponse {
                writes: vec![
                    RawPrivateGardenWrite {
                        path: "journal/new.md".to_string(),
                        content: "next".to_string(),
                    },
                    RawPrivateGardenWrite {
                        path: "journal/active.md".to_string(),
                        content: "same".to_string(),
                    },
                    RawPrivateGardenWrite {
                        path: "../escape".to_string(),
                        content: "bad".to_string(),
                    },
                ],
                moves: vec![
                    RawPrivateGardenMove {
                        from_path: "journal/active.md".to_string(),
                        to_path: "journal/renamed.md".to_string(),
                    },
                    RawPrivateGardenMove {
                        from_path: "journal/missing.md".to_string(),
                        to_path: "journal/skip.md".to_string(),
                    },
                ],
                deletes: vec![
                    "journal/new.md".to_string(),
                    "journal/active.md".to_string(),
                    "journal/active.md".to_string(),
                ],
            },
            &baseline_snapshot,
            &latest_snapshot,
            memory_policy(MemoryProfile::Embedded).private_garden_governance,
        );

        assert_eq!(
            writes,
            vec![PrivateGardenWriteAction {
                path: "journal/new.md".to_string(),
                content: "next".to_string(),
            }]
        );
        assert_eq!(
            moves,
            vec![PrivateGardenMoveAction {
                from_path: "journal/active.md".to_string(),
                to_path: "journal/renamed.md".to_string(),
            }]
        );
        assert!(deletes.is_empty());
    }
}
