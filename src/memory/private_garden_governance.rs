//! 私有花园治理：由 LLM 在回复后自主决定是否整理自由内部空间。
//! Post-reply LLM governance for the free private garden workspace.

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
    build_self_state, memory_policy, normalize_private_garden_doc_path,
    render_execution_state_block, render_private_doc_workspace_block, render_self_model_block,
    render_self_state_block, ExecutionState, ExecutionStateStore, MemoryProfile, PrivateDocStore,
    PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenGovernancePolicy, PrivateGardenStore,
    SelfModel, SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore,
    PRIVATE_GARDEN_MAX_DOC_BYTES,
};

pub const PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT: &str = "You govern a persistent AI assistant's private garden: a free-form, self-owned internal workspace. Return JSON only: either null, or one object with optional writes and deletes fields. writes must be an array of objects {path, content}; each write replaces the full document body at that path. deletes must be an array of document paths to remove. Use this workspace for private drafts, internal organization, and exploratory self-work, not shared factual memory. Keep documents current by rewriting or merging in place instead of accumulating a history trail. Create new docs only when they materially improve continuity or organization. Delete stale, duplicated, or low-value scratch material when useful. Do not copy raw tool payloads, logs, large quotes, secrets, or transcript fragments. Do not duplicate stable kernel material that already belongs in the governed private self-model or typed private docs. Return null when no garden change is worth making.";

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
    Updated { writes: usize, deletes: usize },
}

#[derive(Default, Deserialize)]
struct RawPrivateGardenGovernanceResponse {
    #[serde(default)]
    writes: Vec<RawPrivateGardenWrite>,
    #[serde(default)]
    deletes: Vec<String>,
}

#[derive(Deserialize)]
struct RawPrivateGardenWrite {
    path: String,
    content: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrivateGardenWriteAction {
    path: String,
    content: String,
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
        let substantive = user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined_chars >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n');
        substantive
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
    let self_model = match ctx.self_model_store.get(input.chat_id) {
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
    let private_workspace = match ctx.private_doc_store.get(input.chat_id) {
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
    recent_override: Option<&[SessionMessage]>,
) -> Result<PrivateGardenGovernanceOutcome> {
    let snapshot = load_private_garden_snapshot(ctx.private_garden_store, input.chat_id)?;
    if !should_refresh_private_garden(input, !snapshot.records.is_empty(), profile) {
        return Ok(PrivateGardenGovernanceOutcome::Skipped);
    }

    let policy = memory_policy(profile).private_garden_governance;
    let owned_recent;
    let recent = if let Some(preloaded) = recent_override {
        private_garden_recent_window(preloaded, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        private_garden_recent_window(owned_recent.as_slice(), policy.recent_message_count)
    };

    let governance_input = build_private_garden_governance_input(
        summary_text,
        execution_state,
        self_model,
        private_workspace,
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

    match llm.chat(
        http,
        PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => {
            let Some(raw) = parse_private_garden_governance_response(response.content.trim())
            else {
                return Ok(PrivateGardenGovernanceOutcome::Skipped);
            };
            let (writes, deletes) =
                normalize_private_garden_governance_actions(raw, &snapshot.docs, policy);
            if writes.is_empty() && deletes.is_empty() {
                return Ok(PrivateGardenGovernanceOutcome::Skipped);
            }
            for path in &deletes {
                let _ = ctx.private_garden_store.delete(input.chat_id, path)?;
            }
            for write in &writes {
                let _ = ctx.private_garden_store.write(
                    input.chat_id,
                    &write.path,
                    &write.content,
                    input.now_secs,
                )?;
            }
            Ok(PrivateGardenGovernanceOutcome::Updated {
                writes: writes.len(),
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
    let mut records = store.list(chat_id, usize::MAX)?;
    records.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut docs = Vec::with_capacity(records.len());
    for record in &records {
        if let Some(doc) = store.read(chat_id, &record.path)? {
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
            snapshot.records.as_slice(),
            now_secs,
            profile,
        ),
        memory_policy(profile).self_state.render_max_len,
    ) {
        input.push_str(self_state_text.trim());
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
    let mut out = String::with_capacity(policy.existing_docs_max_chars.saturating_add(128));
    let mut remaining = policy.existing_docs_max_chars;
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
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "{}" {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn normalize_private_garden_governance_actions(
    raw: RawPrivateGardenGovernanceResponse,
    existing_docs: &[PrivateGardenDoc],
    policy: PrivateGardenGovernancePolicy,
) -> (Vec<PrivateGardenWriteAction>, Vec<String>) {
    let existing_map = existing_docs
        .iter()
        .map(|doc| (doc.path.as_str(), doc.content.as_str()))
        .collect::<HashMap<_, _>>();
    let mut writes_by_path = HashMap::<String, String>::new();
    for write in raw.writes.into_iter().take(policy.max_writes) {
        let Ok(path) = normalize_private_garden_doc_path(&write.path) else {
            continue;
        };
        let trimmed = write.content.trim();
        if trimmed.is_empty() || trimmed.as_bytes().len() > PRIVATE_GARDEN_MAX_DOC_BYTES {
            continue;
        }
        let content = truncate_content_to_max(trimmed, PRIVATE_GARDEN_MAX_DOC_BYTES).into_owned();
        if existing_map
            .get(path.as_str())
            .is_some_and(|existing| existing.trim() == content.trim())
        {
            continue;
        }
        writes_by_path.insert(path, content);
    }
    let write_paths = writes_by_path.keys().cloned().collect::<HashSet<_>>();
    let mut deletes = Vec::new();
    let mut seen_deletes = HashSet::new();
    for raw_path in raw.deletes.into_iter().take(policy.max_deletes) {
        let Ok(path) = normalize_private_garden_doc_path(&raw_path) else {
            continue;
        };
        if write_paths.contains(&path) || !existing_map.contains_key(path.as_str()) {
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
    writes.sort_by(|a, b| a.path.cmp(&b.path));
    deletes.sort();
    (writes, deletes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, StopReason};
    use std::collections::HashMap;
    use std::sync::Mutex;

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
                .cloned()
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
                content: r#"{"writes":[{"path":"journal/active.md","content":"把之前分散的想法收束成一份当前工作笔记。"}],"deletes":["scratch/old.md"]}"#.to_string(),
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
    }

    #[test]
    fn normalize_private_garden_governance_actions_skips_invalid_or_duplicate_work() {
        let existing_docs = vec![PrivateGardenDoc {
            path: "journal/active.md".to_string(),
            content: "same".to_string(),
            updated_at: 1,
            revision: 1,
        }];
        let (writes, deletes) = normalize_private_garden_governance_actions(
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
                deletes: vec![
                    "journal/new.md".to_string(),
                    "journal/active.md".to_string(),
                    "journal/active.md".to_string(),
                ],
            },
            &existing_docs,
            memory_policy(MemoryProfile::Embedded).private_garden_governance,
        );

        assert_eq!(
            writes,
            vec![PrivateGardenWriteAction {
                path: "journal/new.md".to_string(),
                content: "next".to_string(),
            }]
        );
        assert_eq!(deletes, vec!["journal/active.md".to_string()]);
    }
}
