//! 对话级执行状态：当前目标、进展、阻塞与下一步。
//! Live execution state separate from long-term memory and session summary.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    memory_policy, ExecutionStatePolicy, MemoryProfile, SessionMessage, SessionStore,
    SessionSummaryStore,
};

pub const REL_PATH_EXECUTION_STATES: &str = "memory/execution_states.json";
pub const EXECUTION_STATE_SYSTEM_PROMPT: &str = "You maintain a compact live execution state for a personal AI assistant. Return JSON only: either null or one object with fields status, goal, progress, blocker, next_action, last_output. status must be active, blocked, or done. Capture only the current task/project execution context that should guide the next turn: the current goal, latest concrete progress, blocker, next action, and latest meaningful output. Replace old state when the focus changes instead of keeping parallel tasks. Do not store greetings, chit-chat, durable user profile facts, stable preferences, or general long-term memory. Return null when there is no active execution context worth carrying to the next turn. Keep fields short and concrete.";

const EXECUTION_STATE_GOAL_MAX_CHARS: usize = 120;
const EXECUTION_STATE_FIELD_MAX_CHARS: usize = 180;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    #[default]
    Active,
    Blocked,
    Done,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionState {
    #[serde(default)]
    pub status: ExecutionStatus,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub progress: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub last_output: String,
    #[serde(default)]
    pub updated_at: u64,
}

impl ExecutionState {
    pub fn is_meaningful(&self) -> bool {
        !self.goal.trim().is_empty()
            || !self.progress.trim().is_empty()
            || !self.blocker.trim().is_empty()
            || !self.next_action.trim().is_empty()
            || !self.last_output.trim().is_empty()
    }
}

pub trait ExecutionStateStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>>;
    fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()>;
    fn clear(&self, chat_id: &str) -> Result<()>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecutionStateRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct ExecutionStateRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionStateRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

#[derive(Deserialize)]
struct RawExecutionState {
    #[serde(default)]
    status: Option<ExecutionStatus>,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    progress: String,
    #[serde(default)]
    blocker: String,
    #[serde(default)]
    next_action: String,
    #[serde(default)]
    last_output: String,
}

impl ExecutionStatePolicy {
    fn should_refresh(
        self,
        input: ExecutionStateRefreshInput<'_>,
        has_existing_state: bool,
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
        if has_existing_state {
            return substantive;
        }
        substantive
    }
}

pub fn render_execution_state_block(state: &ExecutionState, max_len: usize) -> Option<String> {
    let normalized = normalize_execution_state(state.clone(), state.updated_at)?;
    if !should_persist_execution_state(&normalized) {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(384));
    out.push_str("## Execution State\n");
    let _ = writeln!(out, "Status: {}", execution_status_label(normalized.status));
    if !normalized.goal.is_empty() {
        let _ = writeln!(out, "Goal: {}", normalized.goal);
    }
    if !normalized.progress.is_empty() {
        let _ = writeln!(out, "Progress: {}", normalized.progress);
    }
    if !normalized.blocker.is_empty() {
        let _ = writeln!(out, "Blocker: {}", normalized.blocker);
    }
    if !normalized.next_action.is_empty() {
        let _ = writeln!(out, "Next: {}", normalized.next_action);
    }
    if !normalized.last_output.is_empty() {
        let _ = writeln!(out, "Latest output: {}", normalized.last_output);
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub fn run_execution_state_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: ExecutionStateRefreshContext<'_>,
    input: ExecutionStateRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<ExecutionStateRefreshOutcome> {
    let policy = memory_policy(profile).execution_state;
    let existing_state = ctx.execution_state_store.get(input.chat_id)?;
    if !policy.should_refresh(input, existing_state.is_some()) {
        return Ok(ExecutionStateRefreshOutcome::Skipped);
    }

    let summary_text = match ctx.session_summary_store.get_with_count(input.chat_id) {
        Ok(entry) => entry.map(|(summary, _)| summary),
        Err(error) => {
            log::warn!(
                "[agent_execution_state] failed to read summary for chat_id={}: {}",
                input.chat_id,
                error
            );
            None
        }
    };
    let recent = ctx
        .session_store
        .load_recent(input.chat_id, policy.recent_message_count)?;
    let refresh_input = build_execution_state_refresh_input(
        existing_state.as_ref(),
        summary_text.as_deref(),
        &recent,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: refresh_input,
    }];
    let response = llm.chat(
        http,
        EXECUTION_STATE_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    match parse_execution_state_response(response.content.trim(), input.now_secs) {
        Some(mut state) => {
            if state.last_output.is_empty() && should_capture_last_output(input.reply_content) {
                state.last_output =
                    normalize_field(input.reply_content, EXECUTION_STATE_FIELD_MAX_CHARS);
            }
            if let Some(merged) =
                merge_execution_state(existing_state.as_ref(), state, input.now_secs)
            {
                if should_persist_execution_state(&merged) {
                    ctx.execution_state_store.set(input.chat_id, &merged)?;
                    Ok(ExecutionStateRefreshOutcome::Updated)
                } else {
                    ctx.execution_state_store.clear(input.chat_id)?;
                    Ok(ExecutionStateRefreshOutcome::Cleared)
                }
            } else {
                ctx.execution_state_store.clear(input.chat_id)?;
                Ok(ExecutionStateRefreshOutcome::Cleared)
            }
        }
        None => {
            ctx.execution_state_store.clear(input.chat_id)?;
            Ok(ExecutionStateRefreshOutcome::Cleared)
        }
    }
}

fn should_capture_last_output(reply_content: &str) -> bool {
    let trimmed = reply_content.trim();
    let reply_chars = trimmed.chars().count();
    !trimmed.is_empty() && (reply_chars >= 24 || trimmed.contains('\n'))
}

fn build_execution_state_refresh_input(
    existing_state: Option<&ExecutionState>,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    policy: ExecutionStatePolicy,
) -> String {
    let mut input = String::with_capacity(2048);
    if let Some(existing) = existing_state
        .and_then(|state| render_execution_state_block(state, policy.existing_state_max_len))
    {
        input.push_str(&existing);
        input.push_str("\n\n");
    }
    if let Some(summary) = summary_text
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
    {
        input.push_str("## Session Summary\n");
        input.push_str(summary);
        input.push_str("\n\n");
    }
    input.push_str("## Recent Conversation\n");
    input.push_str(&build_execution_state_transcript(recent, policy));
    input
}

fn build_execution_state_transcript(
    recent: &[SessionMessage],
    policy: ExecutionStatePolicy,
) -> String {
    let mut transcript = String::with_capacity(1024);
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            transcript,
            "{}: {}",
            message.role.to_uppercase(),
            preview.as_ref()
        );
    }
    transcript
}

fn parse_execution_state_response(raw: &str, now_secs: u64) -> Option<ExecutionState> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        return None;
    }
    let json_slice = if trimmed.starts_with('{') {
        trimmed
    } else {
        match (trimmed.find('{'), trimmed.rfind('}')) {
            (Some(start), Some(end)) if start < end => &trimmed[start..=end],
            _ => return None,
        }
    };
    let parsed = serde_json::from_str::<RawExecutionState>(json_slice).ok()?;
    normalize_execution_state(
        ExecutionState {
            status: parsed.status.unwrap_or_default(),
            goal: parsed.goal,
            progress: parsed.progress,
            blocker: parsed.blocker,
            next_action: parsed.next_action,
            last_output: parsed.last_output,
            updated_at: now_secs,
        },
        now_secs,
    )
}

fn normalize_execution_state(mut state: ExecutionState, now_secs: u64) -> Option<ExecutionState> {
    state.goal = normalize_field(&state.goal, EXECUTION_STATE_GOAL_MAX_CHARS);
    state.progress = normalize_field(&state.progress, EXECUTION_STATE_FIELD_MAX_CHARS);
    state.blocker = normalize_field(&state.blocker, EXECUTION_STATE_FIELD_MAX_CHARS);
    state.next_action = normalize_field(&state.next_action, EXECUTION_STATE_FIELD_MAX_CHARS);
    state.last_output = normalize_field(&state.last_output, EXECUTION_STATE_FIELD_MAX_CHARS);
    if !state.is_meaningful() {
        return None;
    }
    if state.goal.is_empty() {
        if !state.progress.is_empty() {
            state.goal = state.progress.clone();
        } else if !state.next_action.is_empty() {
            state.goal = state.next_action.clone();
        } else if !state.blocker.is_empty() {
            state.goal = state.blocker.clone();
        } else {
            return None;
        }
    }
    if !state.blocker.is_empty()
        && state.next_action.is_empty()
        && state.status == ExecutionStatus::Active
    {
        state.status = ExecutionStatus::Blocked;
    }
    if state.status == ExecutionStatus::Done {
        state.blocker.clear();
        if !state.next_action.is_empty() {
            state.status = ExecutionStatus::Active;
        }
    }
    state.updated_at = now_secs;
    Some(state)
}

fn merge_execution_state(
    existing: Option<&ExecutionState>,
    mut next: ExecutionState,
    now_secs: u64,
) -> Option<ExecutionState> {
    let Some(existing) = existing else {
        return normalize_execution_state(next, now_secs);
    };
    if same_execution_focus(existing, &next) {
        if next.goal.is_empty() {
            next.goal = existing.goal.clone();
        }
        if next.progress.is_empty() {
            next.progress = existing.progress.clone();
        }
        if next.blocker.is_empty() && next.status == ExecutionStatus::Blocked {
            next.blocker = existing.blocker.clone();
        }
        if next.next_action.is_empty() && next.status != ExecutionStatus::Done {
            next.next_action = existing.next_action.clone();
        }
        if next.last_output.is_empty() {
            next.last_output = existing.last_output.clone();
        }
        return normalize_execution_state(next, now_secs);
    }

    next.progress = if next.progress.is_empty() {
        String::new()
    } else {
        next.progress
    };
    next.blocker = if next.blocker.is_empty() {
        String::new()
    } else {
        next.blocker
    };
    next.next_action = if next.next_action.is_empty() {
        String::new()
    } else {
        next.next_action
    };
    next.last_output = if next.last_output.is_empty() {
        String::new()
    } else {
        next.last_output
    };
    normalize_execution_state(next, now_secs)
}

fn should_persist_execution_state(state: &ExecutionState) -> bool {
    if !state.is_meaningful() {
        return false;
    }
    if state.status == ExecutionStatus::Done
        && state.next_action.is_empty()
        && state.blocker.is_empty()
    {
        return false;
    }
    true
}

fn same_execution_focus(left: &ExecutionState, right: &ExecutionState) -> bool {
    let left_goal = normalize_focus_text(&left.goal);
    let right_goal = normalize_focus_text(&right.goal);
    if left_goal.is_empty() || right_goal.is_empty() {
        return false;
    }
    if left_goal == right_goal {
        return true;
    }
    let left_terms = focus_terms(&left_goal);
    let right_terms = focus_terms(&right_goal);
    if left_terms.is_empty() || right_terms.is_empty() {
        return false;
    }
    let overlap = left_terms
        .iter()
        .filter(|term| right_terms.iter().any(|candidate| candidate == *term))
        .count();
    overlap > 0 && overlap * 2 >= left_terms.len().min(right_terms.len())
}

fn normalize_focus_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for ch in value.trim().chars() {
        if ch.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.extend(ch.to_lowercase());
        } else if !out.is_empty() {
            pending_space = true;
        }
    }
    out.trim().to_string()
}

fn focus_terms(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect()
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn execution_status_label(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Active => "active",
        ExecutionStatus::Blocked => "blocked",
        ExecutionStatus::Done => "done",
    }
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
        entries: Mutex<HashMap<String, ExecutionState>>,
        clears: Mutex<u32>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, state: &ExecutionState) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), state.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
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
    fn parses_json_object_and_normalizes_fields() {
        let parsed = parse_execution_state_response(
            r#"{"status":"blocked","goal":"收口 execution state","progress":"已经接上 store","blocker":"还没接 prompt","next_action":"改 build_context","last_output":"store ok"}"#,
            123,
        )
        .unwrap();
        assert_eq!(parsed.status, ExecutionStatus::Blocked);
        assert_eq!(parsed.goal, "收口 execution state");
        assert_eq!(parsed.updated_at, 123);
    }

    #[test]
    fn renders_execution_state_block() {
        let block = render_execution_state_block(
            &ExecutionState {
                status: ExecutionStatus::Active,
                goal: "推进 execution state".to_string(),
                progress: "store 已持久化".to_string(),
                blocker: String::new(),
                next_action: "接入 prompt".to_string(),
                last_output: String::new(),
                updated_at: 1,
            },
            512,
        )
        .unwrap();
        assert!(block.contains("## Execution State"));
        assert!(block.contains("Goal: 推进 execution state"));
        assert!(block.contains("Next: 接入 prompt"));
    }

    #[test]
    fn same_focus_merge_preserves_missing_fields() {
        let merged = merge_execution_state(
            Some(&ExecutionState {
                status: ExecutionStatus::Blocked,
                goal: "收口 execution state".to_string(),
                progress: "store 已完成".to_string(),
                blocker: "还没接 prompt".to_string(),
                next_action: "接 prompt".to_string(),
                last_output: "store ok".to_string(),
                updated_at: 1,
            }),
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 execution state".to_string(),
                progress: "prompt 已接入".to_string(),
                blocker: String::new(),
                next_action: String::new(),
                last_output: String::new(),
                updated_at: 2,
            },
            3,
        )
        .unwrap();

        assert_eq!(merged.goal, "收口 execution state");
        assert_eq!(merged.progress, "prompt 已接入");
        assert_eq!(merged.next_action, "接 prompt");
        assert_eq!(merged.last_output, "store ok");
    }

    #[test]
    fn focus_switch_drops_old_parallel_fields() {
        let merged = merge_execution_state(
            Some(&ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 execution state".to_string(),
                progress: "store 已完成".to_string(),
                blocker: "还没接 prompt".to_string(),
                next_action: "接 prompt".to_string(),
                last_output: "store ok".to_string(),
                updated_at: 1,
            }),
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "推进 linux 任务面".to_string(),
                progress: "开始梳理链路".to_string(),
                blocker: String::new(),
                next_action: "补测试".to_string(),
                last_output: String::new(),
                updated_at: 2,
            },
            3,
        )
        .unwrap();

        assert_eq!(merged.goal, "推进 linux 任务面");
        assert_eq!(merged.progress, "开始梳理链路");
        assert!(merged.blocker.is_empty());
        assert_eq!(merged.next_action, "补测试");
        assert!(merged.last_output.is_empty());
    }

    #[test]
    fn done_without_followup_is_not_persisted() {
        let state = normalize_execution_state(
            ExecutionState {
                status: ExecutionStatus::Done,
                goal: "收口 execution state".to_string(),
                progress: "已经完成".to_string(),
                blocker: "none".to_string(),
                next_action: String::new(),
                last_output: "done".to_string(),
                updated_at: 1,
            },
            2,
        )
        .unwrap();
        assert!(!should_persist_execution_state(&state));
    }

    #[test]
    fn done_with_next_action_becomes_active() {
        let state = normalize_execution_state(
            ExecutionState {
                status: ExecutionStatus::Done,
                goal: "收口 execution state".to_string(),
                progress: "当前子步骤完成".to_string(),
                blocker: String::new(),
                next_action: "补剩余测试".to_string(),
                last_output: String::new(),
                updated_at: 1,
            },
            2,
        )
        .unwrap();
        assert_eq!(state.status, ExecutionStatus::Active);
        assert_eq!(state.next_action, "补剩余测试");
    }

    #[test]
    fn render_hides_done_state_without_followup() {
        let block = render_execution_state_block(
            &ExecutionState {
                status: ExecutionStatus::Done,
                goal: "收口 execution state".to_string(),
                progress: "已经完成".to_string(),
                blocker: String::new(),
                next_action: String::new(),
                last_output: "done".to_string(),
                updated_at: 1,
            },
            512,
        );
        assert!(block.is_none());
    }

    #[test]
    fn refresh_updates_store_when_llm_returns_state() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续收 execution state".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我先把 store 接好".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        let execution_store = StubExecutionStateStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            content: r#"{"status":"active","goal":"收口 execution state","progress":"store 已接好","next_action":"接 prompt"}"#,
        };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
            },
            ExecutionStateRefreshInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续收 execution state",
                reply_content: "我先把 store 接好",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 77,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, ExecutionStateRefreshOutcome::Updated);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(stored.goal, "收口 execution state");
        assert_eq!(stored.next_action, "接 prompt");
        assert_eq!(stored.updated_at, 77);
    }

    #[test]
    fn refresh_backfills_last_output_from_reply_when_model_omits_it() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "帮我把 execution state 这轮收掉".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我已经把 task continuation 逻辑删掉并切到 execution state"
                        .to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        let execution_store = StubExecutionStateStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            content: r#"{"status":"active","goal":"收口 execution state","progress":"删除 dead continuation path","next_action":"补回归测试"}"#,
        };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
            },
            ExecutionStateRefreshInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "帮我把 execution state 这轮收掉",
                reply_content: "我已经把 task continuation 逻辑删掉并切到 execution state。",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 78,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, ExecutionStateRefreshOutcome::Updated);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(
            stored.last_output,
            "我已经把 task continuation 逻辑删掉并切到 execution state。"
        );
    }

    #[test]
    fn refresh_clears_store_when_llm_returns_null() {
        let session_store = StubSessionStore {
            recent: vec![SessionMessage {
                role: "user".to_string(),
                content: "好了".to_string(),
            }],
        };
        let summary_store = StubSessionSummaryStore::default();
        let execution_store = StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-1".to_string(),
                ExecutionState {
                    status: ExecutionStatus::Active,
                    goal: "旧任务".to_string(),
                    progress: String::new(),
                    blocker: String::new(),
                    next_action: String::new(),
                    last_output: String::new(),
                    updated_at: 1,
                },
            )])),
            clears: Mutex::new(0),
        };
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient { content: "null" };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
            },
            ExecutionStateRefreshInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "好了",
                reply_content: "这轮结束。",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 88,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, ExecutionStateRefreshOutcome::Cleared);
        assert!(execution_store.get("chat-1").unwrap().is_none());
    }

    #[test]
    fn existing_state_low_signal_turn_without_tools_is_skipped() {
        let session_store = StubSessionStore {
            recent: vec![SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }],
        };
        let summary_store = StubSessionSummaryStore::default();
        let execution_store = StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-1".to_string(),
                ExecutionState {
                    status: ExecutionStatus::Active,
                    goal: "收口 execution state".to_string(),
                    progress: "store 已完成".to_string(),
                    blocker: String::new(),
                    next_action: "接 prompt".to_string(),
                    last_output: String::new(),
                    updated_at: 1,
                },
            )])),
            clears: Mutex::new(0),
        };
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            content: r#"{"status":"active","goal":"should not be used"}"#,
        };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
            },
            ExecutionStateRefreshInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续",
                reply_content: "好。",
                pressure: PressureLevel::Normal,
                tool_calls: 0,
                now_secs: 2,
            },
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert_eq!(outcome, ExecutionStateRefreshOutcome::Skipped);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(stored.goal, "收口 execution state");
    }
}
