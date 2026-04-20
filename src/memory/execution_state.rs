//! 对话级执行状态：当前目标、进展、阻塞与下一步。
//! Live execution state separate from long-term memory and session summary.
#![allow(clippy::too_many_arguments)]

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::Write as _;

use super::{
    llm_json::{coerce_json_text, parse_llm_json_payload, LlmJsonPayload},
    memory_policy, relationship_scope_id, render_turn_observation_ledger_block,
    ExecutionStatePolicy, MemoryProfile, SessionMessage, SessionStore, SessionSummaryStore,
    TurnLedgerStore, TurnObservationLedger,
};

pub const REL_PATH_EXECUTION_STATES: &str = "memory/execution_states.json";
pub const EXECUTION_STATE_SYSTEM_PROMPT: &str = "You maintain a compact live execution state for a personal AI assistant. Return JSON only: either null or one object with fields status, goal, progress, blocker, next_action, last_output, active_constraints, open_questions, latest_observations, next_best_actions. status must be active, blocked, or done. Capture only the current task/project execution context that should guide the next turn: the current goal, latest concrete progress, blocker, next action, latest meaningful output, and the compact working set that will improve the next decision. Replace old state when the focus changes instead of keeping parallel tasks. Prefer concrete task names, changed progress, real constraints, unanswered questions, grounded observations, and actionable next steps. Use short arrays for working-set fields when useful; omit noise instead of filling placeholders. Do not return vague placeholders such as continue, keep going, processing, current task, or done unless paired with concrete task detail. Do not store greetings, chit-chat, durable user profile facts, stable preferences, or general long-term memory. Return null when there is no active execution context worth carrying to the next turn. Keep fields short and concrete.";
const EXECUTION_STATE_REFRESH_RULES: &str = concat!(
    "## Extraction Rules\n",
    "- Goal must name the concrete task/project, not a vague placeholder.\n",
    "- Progress must describe a real change, not just say it is ongoing.\n",
    "- Next action must be an actionable next step when one exists.\n",
    "- active_constraints should list the real constraints currently shaping execution.\n",
    "- open_questions should list unresolved questions that affect the next step.\n",
    "- latest_observations should capture grounded new observations from this turn.\n",
    "- next_best_actions should list the best concrete next moves when there are multiple plausible steps.\n",
    "- Return null if this turn contains no durable execution context worth carrying.\n\n",
);

const EXECUTION_STATE_GOAL_MAX_CHARS: usize = 120;
const EXECUTION_STATE_FIELD_MAX_CHARS: usize = 180;
const EXECUTION_STATE_LIST_ITEM_MAX_CHARS: usize = 120;
const EXECUTION_STATE_LIST_MAX_ITEMS: usize = 4;
const MIN_FOCUS_MATCH_CHARS: usize = 4;
const MIN_FIELD_SPECIFICITY_SCORE: u32 = 2;
const MIN_LAST_OUTPUT_SPECIFICITY_SCORE: u32 = 4;
const MIN_STRONG_STATE_FIELD_SCORE: u32 = 3;
const STRONG_SINGLE_FIELD_SCORE: u32 = 5;

#[derive(Default)]
struct RecentObservationWorkingSet {
    latest_observations: Vec<String>,
    next_best_actions: Vec<String>,
}

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
    pub active_constraints: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub latest_observations: Vec<String>,
    #[serde(default)]
    pub next_best_actions: Vec<String>,
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
            || !self.active_constraints.is_empty()
            || !self.open_questions.is_empty()
            || !self.latest_observations.is_empty()
            || !self.next_best_actions.is_empty()
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
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProvisionalExecutionStateInput<'a> {
    pub(crate) chat_id: &'a str,
    pub(crate) ingress: IngressKind,
    pub(crate) channel: &'a str,
    pub(crate) user_content: &'a str,
    pub(crate) reply_content: &'a str,
    pub(crate) reply_requests_input: bool,
    pub(crate) tool_calls: u32,
    pub(crate) now_secs: u64,
    pub(crate) turn_observation: Option<&'a TurnObservationLedger>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionStateRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
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

pub(crate) fn should_refresh_execution_state(
    input: ExecutionStateRefreshInput<'_>,
    has_existing_state: bool,
    profile: MemoryProfile,
) -> bool {
    memory_policy(profile)
        .execution_state
        .should_refresh(input, has_existing_state)
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
    if !normalized.active_constraints.is_empty() {
        let _ = writeln!(
            out,
            "Constraints: {}",
            normalized.active_constraints.join(" | ")
        );
    }
    if !normalized.open_questions.is_empty() {
        let _ = writeln!(
            out,
            "Open questions: {}",
            normalized.open_questions.join(" | ")
        );
    }
    if !normalized.latest_observations.is_empty() {
        let _ = writeln!(
            out,
            "Observations: {}",
            normalized.latest_observations.join(" | ")
        );
    }
    if !normalized.next_best_actions.is_empty() {
        let _ = writeln!(
            out,
            "Next best actions: {}",
            normalized.next_best_actions.join(" | ")
        );
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub(crate) fn seed_execution_state_from_turn(
    store: &dyn ExecutionStateStore,
    input: ProvisionalExecutionStateInput<'_>,
) -> Result<bool> {
    if input.ingress != IngressKind::User || input.channel == "cron" {
        return Ok(false);
    }
    let user_content = input.user_content.trim();
    let reply_content = input.reply_content.trim();
    if user_content.is_empty() || reply_content.is_empty() {
        return Ok(false);
    }
    let reply_requests_input = input.reply_requests_input;
    let mut candidate = ExecutionState {
        status: if reply_requests_input
            || input
                .turn_observation
                .and_then(|observation| observation.blocker.as_ref())
                .is_some()
        {
            ExecutionStatus::Blocked
        } else {
            ExecutionStatus::Active
        },
        goal: normalize_field(user_content, EXECUTION_STATE_GOAL_MAX_CHARS),
        blocker: if reply_requests_input {
            normalize_field(reply_content, EXECUTION_STATE_FIELD_MAX_CHARS)
        } else {
            String::new()
        },
        next_action: if reply_requests_input {
            normalize_field(reply_content, EXECUTION_STATE_FIELD_MAX_CHARS)
        } else {
            String::new()
        },
        last_output: if should_capture_last_output(reply_content) {
            normalize_field(reply_content, EXECUTION_STATE_FIELD_MAX_CHARS)
        } else {
            String::new()
        },
        updated_at: input.now_secs,
        ..ExecutionState::default()
    };
    if let Some(observation) = input.turn_observation {
        tighten_execution_state_with_recent_observation(&mut candidate, Some(observation));
    }
    if input.tool_calls == 0
        && !reply_requests_input
        && !execution_state_has_pending_work(&candidate)
        && field_specificity_score(&candidate.goal) < MIN_STRONG_STATE_FIELD_SCORE
    {
        return Ok(false);
    }
    let existing = store.get(input.chat_id)?;
    let Some(merged) = merge_execution_state(existing.as_ref(), candidate, input.now_secs) else {
        return Ok(false);
    };
    if !should_persist_execution_state(&merged) {
        return Ok(false);
    }
    store.set(input.chat_id, &merged)?;
    Ok(true)
}

pub fn run_execution_state_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: ExecutionStateRefreshContext<'_>,
    input: ExecutionStateRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<ExecutionStateRefreshOutcome> {
    let existing_state = ctx.execution_state_store.get(input.chat_id)?;
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
    run_execution_state_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing_state,
        summary_text.as_deref(),
        None,
    )
}

pub(crate) fn run_execution_state_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: ExecutionStateRefreshContext<'_>,
    input: ExecutionStateRefreshInput<'_>,
    profile: MemoryProfile,
    existing_state: Option<ExecutionState>,
    summary_text: Option<&str>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<ExecutionStateRefreshOutcome> {
    let policy = memory_policy(profile).execution_state;
    if !should_refresh_execution_state(input, existing_state.is_some(), profile) {
        return Ok(ExecutionStateRefreshOutcome::Skipped);
    }

    let owned_recent;
    let recent = if let Some(preloaded) = recent_override {
        execution_state_recent_window(preloaded, policy.recent_message_count)
    } else {
        owned_recent = ctx
            .session_store
            .load_recent(input.chat_id, policy.recent_message_count)?;
        owned_recent.as_slice()
    };
    let recent_observation = ctx
        .turn_ledger_store
        .get(&relationship_scope_id(input.channel, input.chat_id))?
        .and_then(|ledger| ledger.observation);
    let refresh_input = build_execution_state_refresh_input(
        existing_state.as_ref(),
        if existing_state.is_some() {
            None
        } else {
            summary_text
        },
        recent,
        recent_observation.as_ref(),
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
            tighten_execution_state_with_recent_observation(
                &mut state,
                recent_observation.as_ref(),
            );
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

fn execution_state_recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn should_capture_last_output(reply_content: &str) -> bool {
    let trimmed = reply_content.trim();
    let reply_chars = trimmed.chars().count();
    !trimmed.is_empty()
        && !is_low_value_last_output(trimmed)
        && (reply_chars >= 24 || trimmed.contains('\n'))
}

fn build_execution_state_refresh_input(
    existing_state: Option<&ExecutionState>,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    recent_observation: Option<&TurnObservationLedger>,
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
    if let Some(observation) = recent_observation.and_then(|observation| {
        render_turn_observation_ledger_block(
            observation,
            policy.existing_state_max_len.clamp(160, 320),
        )
    }) {
        input.push_str(&observation);
        input.push_str("\n\n");
    }
    input.push_str(EXECUTION_STATE_REFRESH_RULES);
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

fn tighten_execution_state_with_recent_observation(
    state: &mut ExecutionState,
    recent_observation: Option<&TurnObservationLedger>,
) {
    let Some(observation) = recent_observation else {
        return;
    };
    let grounding = build_recent_observation_working_set(observation);
    if !grounding.latest_observations.is_empty() {
        state.latest_observations = merge_execution_state_list_prefix(
            grounding.latest_observations,
            std::mem::take(&mut state.latest_observations),
        );
    }
    if !grounding.next_best_actions.is_empty() {
        state.next_best_actions = merge_execution_state_list_prefix(
            grounding.next_best_actions,
            std::mem::take(&mut state.next_best_actions),
        );
    }
}

fn build_recent_observation_working_set(
    observation: &TurnObservationLedger,
) -> RecentObservationWorkingSet {
    let mut grounding = RecentObservationWorkingSet::default();
    if !observation.tool_path.path.trim().is_empty() || !observation.final_outcome.trim().is_empty()
    {
        let path = if observation.tool_path.path.trim().is_empty() {
            "direct_reply"
        } else {
            observation.tool_path.path.trim()
        };
        let outcome = if observation.final_outcome.trim().is_empty() {
            "unknown"
        } else {
            observation.final_outcome.trim()
        };
        grounding
            .latest_observations
            .push(format!("last_turn path={path} outcome={outcome}"));
    }
    if let Some(blocker) = observation.blocker.as_ref() {
        grounding.latest_observations.push(format!(
            "last_turn blocker={} {}/{}",
            blocker.kind.trim(),
            blocker.failed_calls,
            blocker.total_calls
        ));
    }
    if observation.deliberation_class.label() != "standard"
        || observation.pressure.as_str() != "normal"
    {
        grounding.latest_observations.push(format!(
            "last_turn deliberation={} pressure={}",
            observation.deliberation_class.label(),
            observation.pressure.as_str()
        ));
    }
    if observation.tool_path.tool_calls > 0 && !observation.tool_path.current_primary_delivered {
        grounding
            .next_best_actions
            .push("deliver current primary answer before more tool work".to_string());
    }
    if let Some(blocker) = observation.blocker.as_ref() {
        let hint = crate::agent::parse_workflow_outcome_kind(&blocker.kind)
            .map(crate::agent::WorkflowOutcomeKind::next_action_hint)
            .unwrap_or("state the blocker clearly before continuing");
        grounding.next_best_actions.push(hint.to_string());
    }
    grounding.latest_observations = normalize_execution_state_list(grounding.latest_observations);
    grounding.next_best_actions = normalize_execution_state_list(grounding.next_best_actions);
    grounding
}

fn merge_execution_state_list_prefix(prefix: Vec<String>, existing: Vec<String>) -> Vec<String> {
    let mut combined = prefix;
    combined.extend(existing);
    normalize_execution_state_list(combined)
}

fn parse_execution_state_response(raw: &str, now_secs: u64) -> Option<ExecutionState> {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return None;
    };
    let parsed = value.as_object()?;
    normalize_execution_state(
        ExecutionState {
            status: parsed
                .get("status")
                .and_then(parse_execution_status)
                .unwrap_or_default(),
            goal: parsed.get("goal").map(coerce_json_text).unwrap_or_default(),
            progress: parsed
                .get("progress")
                .map(coerce_json_text)
                .unwrap_or_default(),
            blocker: parsed
                .get("blocker")
                .map(coerce_json_text)
                .unwrap_or_default(),
            next_action: parsed
                .get("next_action")
                .map(coerce_json_text)
                .unwrap_or_default(),
            last_output: parsed
                .get("last_output")
                .map(coerce_json_text)
                .unwrap_or_default(),
            active_constraints: parse_execution_state_list(parsed.get("active_constraints")),
            open_questions: parse_execution_state_list(parsed.get("open_questions")),
            latest_observations: parse_execution_state_list(parsed.get("latest_observations")),
            next_best_actions: parse_execution_state_list(parsed.get("next_best_actions")),
            updated_at: now_secs,
        },
        now_secs,
    )
}

fn parse_execution_status(value: &serde_json::Value) -> Option<ExecutionStatus> {
    let normalized = coerce_json_text(value).to_ascii_lowercase();
    if normalized.contains("block") {
        Some(ExecutionStatus::Blocked)
    } else if normalized.contains("done") || normalized.contains("complete") {
        Some(ExecutionStatus::Done)
    } else if normalized.contains("active")
        || normalized.contains("doing")
        || normalized.contains("progress")
    {
        Some(ExecutionStatus::Active)
    } else {
        None
    }
}

fn normalize_execution_state(mut state: ExecutionState, now_secs: u64) -> Option<ExecutionState> {
    state.goal = sanitize_goal_field(&normalize_field(
        &state.goal,
        EXECUTION_STATE_GOAL_MAX_CHARS,
    ));
    state.progress = sanitize_progress_field(&normalize_field(
        &state.progress,
        EXECUTION_STATE_FIELD_MAX_CHARS,
    ));
    state.blocker = sanitize_blocker_field(&normalize_field(
        &state.blocker,
        EXECUTION_STATE_FIELD_MAX_CHARS,
    ));
    state.next_action = sanitize_next_action_field(&normalize_field(
        &state.next_action,
        EXECUTION_STATE_FIELD_MAX_CHARS,
    ));
    state.last_output = sanitize_last_output_field(&normalize_field(
        &state.last_output,
        EXECUTION_STATE_FIELD_MAX_CHARS,
    ));
    state.active_constraints = normalize_execution_state_list(state.active_constraints);
    state.open_questions = normalize_execution_state_list(state.open_questions);
    state.latest_observations = normalize_execution_state_list(state.latest_observations);
    state.next_best_actions = normalize_execution_state_list(state.next_best_actions);
    if state.progress.is_empty() {
        state.progress = state
            .latest_observations
            .first()
            .cloned()
            .unwrap_or_default();
    }
    if state.next_action.is_empty() {
        state.next_action = state.next_best_actions.first().cloned().unwrap_or_default();
    }
    if state.blocker.is_empty() && state.status == ExecutionStatus::Blocked {
        state.blocker = state
            .active_constraints
            .first()
            .cloned()
            .unwrap_or_default();
    }
    dedupe_execution_state_fields(&mut state);
    if !state.is_meaningful() {
        return None;
    }
    let goal_score = field_specificity_score(&state.goal);
    let progress_score = field_specificity_score(&state.progress);
    let blocker_score = field_specificity_score(&state.blocker);
    let next_action_score = field_specificity_score(&state.next_action);
    let strongest_score = [
        goal_score,
        progress_score,
        blocker_score,
        next_action_score,
        field_specificity_score(&state.last_output),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    if state.goal.is_empty() || goal_score < MIN_STRONG_STATE_FIELD_SCORE {
        if progress_score >= MIN_STRONG_STATE_FIELD_SCORE {
            state.goal = state.progress.clone();
        } else if next_action_score >= MIN_STRONG_STATE_FIELD_SCORE {
            state.goal = state.next_action.clone();
        } else if blocker_score >= MIN_STRONG_STATE_FIELD_SCORE {
            state.goal = state.blocker.clone();
        } else if state.goal.is_empty() {
            return None;
        }
    }
    dedupe_execution_state_fields(&mut state);
    if strongest_score < MIN_STRONG_STATE_FIELD_SCORE && state.status == ExecutionStatus::Active {
        return None;
    }
    if !state.blocker.is_empty()
        && state.next_action.is_empty()
        && state.status == ExecutionStatus::Active
    {
        state.status = ExecutionStatus::Blocked;
    }
    if state.status == ExecutionStatus::Blocked && state.blocker.is_empty() {
        state.status = ExecutionStatus::Active;
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
        if next.goal.is_empty()
            || field_specificity_score(&next.goal) < MIN_STRONG_STATE_FIELD_SCORE
        {
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
        if next.active_constraints.is_empty() {
            next.active_constraints = existing.active_constraints.clone();
        }
        if next.open_questions.is_empty() {
            next.open_questions = existing.open_questions.clone();
        }
        if next.latest_observations.is_empty() {
            next.latest_observations = existing.latest_observations.clone();
        }
        if next.next_best_actions.is_empty() && next.status != ExecutionStatus::Done {
            next.next_best_actions = existing.next_best_actions.clone();
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
    next.active_constraints = if next.active_constraints.is_empty() {
        Vec::new()
    } else {
        next.active_constraints
    };
    next.open_questions = if next.open_questions.is_empty() {
        Vec::new()
    } else {
        next.open_questions
    };
    next.latest_observations = if next.latest_observations.is_empty() {
        Vec::new()
    } else {
        next.latest_observations
    };
    next.next_best_actions = if next.next_best_actions.is_empty() {
        Vec::new()
    } else {
        next.next_best_actions
    };
    normalize_execution_state(next, now_secs)
}

pub(crate) fn execution_state_has_pending_work(state: &ExecutionState) -> bool {
    !state.next_action.is_empty()
        || !state.blocker.is_empty()
        || !state.active_constraints.is_empty()
        || !state.open_questions.is_empty()
        || !state.next_best_actions.is_empty()
}

fn should_persist_execution_state(state: &ExecutionState) -> bool {
    if !state.is_meaningful() {
        return false;
    }
    let field_scores = [
        field_specificity_score(&state.goal),
        field_specificity_score(&state.progress),
        field_specificity_score(&state.blocker),
        field_specificity_score(&state.next_action),
        field_specificity_score(&state.last_output),
        execution_state_list_specificity(&state.active_constraints),
        execution_state_list_specificity(&state.open_questions),
        execution_state_list_specificity(&state.latest_observations),
        execution_state_list_specificity(&state.next_best_actions),
    ];
    let non_empty_fields = [
        &state.goal,
        &state.progress,
        &state.blocker,
        &state.next_action,
        &state.last_output,
    ]
    .iter()
    .filter(|value| !value.trim().is_empty())
    .count();
    let strongest_field = field_scores.into_iter().max().unwrap_or(0);
    let informative_fields = field_scores
        .into_iter()
        .filter(|score| *score >= MIN_FIELD_SPECIFICITY_SCORE)
        .count();
    if informative_fields == 0 {
        return false;
    }
    if strongest_field < MIN_STRONG_STATE_FIELD_SCORE {
        return false;
    }
    if non_empty_fields == 1
        && strongest_field < STRONG_SINGLE_FIELD_SCORE
        && state.status == ExecutionStatus::Active
    {
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
    focus_strings_match(&left.goal, &right.goal)
        || focus_strings_match(&left.goal, &right.next_action)
        || focus_strings_match(&left.next_action, &right.goal)
        || focus_strings_match(&left.next_action, &right.next_action)
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

fn compact_focus_text(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn focus_bigrams(value: &str) -> Vec<String> {
    let chars: Vec<char> = compact_focus_text(value).chars().collect();
    if chars.len() < 2 {
        return Vec::new();
    }
    chars
        .windows(2)
        .map(|pair| pair.iter().collect::<String>())
        .collect()
}

fn focus_strings_match(left: &str, right: &str) -> bool {
    let left_goal = normalize_focus_text(left);
    let right_goal = normalize_focus_text(right);
    if left_goal.is_empty() || right_goal.is_empty() {
        return false;
    }
    if left_goal == right_goal {
        return true;
    }

    let left_compact = compact_focus_text(&left_goal);
    let right_compact = compact_focus_text(&right_goal);
    let min_chars = left_compact
        .chars()
        .count()
        .min(right_compact.chars().count());
    if min_chars >= MIN_FOCUS_MATCH_CHARS
        && (left_compact.contains(&right_compact) || right_compact.contains(&left_compact))
    {
        return true;
    }

    let left_terms = focus_terms(&left_goal);
    let right_terms = focus_terms(&right_goal);
    let overlap = left_terms
        .iter()
        .filter(|term| right_terms.iter().any(|candidate| candidate == *term))
        .count();
    if overlap > 0 && overlap * 2 >= left_terms.len().min(right_terms.len()) {
        return true;
    }

    let left_bigrams = focus_bigrams(&left_goal);
    let right_bigrams = focus_bigrams(&right_goal);
    if left_bigrams.len() < 2 || right_bigrams.len() < 2 {
        return false;
    }
    let shared = left_bigrams
        .iter()
        .filter(|gram| right_bigrams.iter().any(|candidate| candidate == *gram))
        .count();
    shared >= 2 && shared * 2 >= left_bigrams.len().min(right_bigrams.len())
}

fn dedupe_execution_state_fields(state: &mut ExecutionState) {
    if !state.goal.is_empty() && state.progress == state.goal {
        state.progress.clear();
    }
    if !state.goal.is_empty() && state.next_action == state.goal {
        state.next_action.clear();
    }
    if !state.progress.is_empty() && state.next_action == state.progress {
        state.next_action.clear();
    }
    if !state.progress.is_empty() && state.blocker == state.progress {
        state.blocker.clear();
    }
    state
        .active_constraints
        .retain(|item| item != &state.blocker && item != &state.goal && item != &state.progress);
    state
        .latest_observations
        .retain(|item| item != &state.progress && item != &state.last_output);
    state
        .next_best_actions
        .retain(|item| item != &state.next_action && item != &state.goal);
}

fn sanitize_goal_field(value: &str) -> String {
    is_low_value_focus_field(value)
        .then(String::new)
        .unwrap_or_else(|| value.to_string())
}

fn sanitize_progress_field(value: &str) -> String {
    is_low_value_focus_field(value)
        .then(String::new)
        .unwrap_or_else(|| value.to_string())
}

fn sanitize_blocker_field(value: &str) -> String {
    is_low_value_blocker_field(value)
        .then(String::new)
        .unwrap_or_else(|| value.to_string())
}

fn sanitize_next_action_field(value: &str) -> String {
    is_low_value_focus_field(value)
        .then(String::new)
        .unwrap_or_else(|| value.to_string())
}

fn sanitize_last_output_field(value: &str) -> String {
    is_low_value_last_output(value)
        .then(String::new)
        .unwrap_or_else(|| value.to_string())
}

fn field_specificity_score(value: &str) -> u32 {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let normalized = normalize_focus_text(trimmed);
    let compact = compact_focus_text(&normalized);
    let char_count = compact.chars().count();
    let mut score = 0u32;
    score += match char_count {
        0..=2 => 0,
        3..=4 => 1,
        5..=7 => 2,
        8..=11 => 3,
        _ => 4,
    };
    let bigrams = focus_bigrams(&normalized);
    let unique_bigrams = bigrams.iter().collect::<HashSet<_>>().len();
    if unique_bigrams >= 2 {
        score += 1;
    }
    if unique_bigrams >= 4 {
        score += 1;
    }
    let has_digit = trimmed.chars().any(|ch| ch.is_ascii_digit());
    let has_structural_marker = trimmed.chars().any(|ch| {
        matches!(
            ch,
            '/' | '\\' | '_' | '.' | ':' | '#' | '(' | ')' | '[' | ']' | '`'
        )
    });
    let has_ascii_word = trimmed
        .split_whitespace()
        .any(|token| token.chars().filter(|ch| ch.is_ascii_alphabetic()).count() >= 3);
    let has_mixed_script = trimmed.chars().any(|ch| ch.is_ascii_alphabetic())
        && trimmed
            .chars()
            .any(|ch| !ch.is_ascii() && !ch.is_whitespace());
    let multi_part = normalized.split_whitespace().count() >= 2;
    let has_sentence_shape = trimmed.chars().any(|ch| {
        matches!(
            ch,
            ',' | '，' | '.' | '。' | ';' | '；' | ':' | '：' | '(' | ')' | '[' | ']'
        )
    });
    score
        + u32::from(has_digit)
        + u32::from(has_structural_marker)
        + u32::from(has_ascii_word)
        + u32::from(has_mixed_script)
        + u32::from(multi_part)
        + u32::from(has_sentence_shape)
}

fn is_low_value_focus_field(value: &str) -> bool {
    !value.trim().is_empty() && field_specificity_score(value) < MIN_FIELD_SPECIFICITY_SCORE
}

fn is_low_value_blocker_field(value: &str) -> bool {
    !value.trim().is_empty() && field_specificity_score(value) < MIN_FIELD_SPECIFICITY_SCORE
}

fn is_low_value_last_output(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && field_specificity_score(trimmed) < MIN_LAST_OUTPUT_SPECIFICITY_SCORE
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn parse_execution_state_list(value: Option<&serde_json::Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let mut items = Vec::new();
    match value {
        serde_json::Value::Array(values) => {
            for item in values {
                extend_execution_state_list_text(&mut items, &coerce_json_text(item));
            }
        }
        _ => extend_execution_state_list_text(&mut items, &coerce_json_text(value)),
    }
    normalize_execution_state_list(items)
}

fn extend_execution_state_list_text(items: &mut Vec<String>, text: &str) {
    for segment in text.split(['\n', ';', '；']) {
        let trimmed = segment
            .trim()
            .trim_start_matches(['-', '*', '•', ' ', '\t'])
            .trim();
        if trimmed.is_empty() {
            continue;
        }
        items.push(trimmed.to_string());
    }
}

fn normalize_execution_state_list(items: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::with_capacity(items.len().min(EXECUTION_STATE_LIST_MAX_ITEMS));
    for item in items {
        let item = normalize_field(&item, EXECUTION_STATE_LIST_ITEM_MAX_CHARS);
        if item.is_empty() || normalized.iter().any(|existing| existing == &item) {
            continue;
        }
        normalized.push(item);
        if normalized.len() >= EXECUTION_STATE_LIST_MAX_ITEMS {
            break;
        }
    }
    normalized
}

fn execution_state_list_specificity(items: &[String]) -> u32 {
    items
        .iter()
        .map(|item| field_specificity_score(item))
        .max()
        .unwrap_or(0)
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
    use crate::memory::{
        relationship_scope_id, TurnBlockerLedger, TurnDeliberationClass, TurnExecutionClass,
        TurnLedger, TurnModeSnapshotLedger, TurnObservationLedger, TurnPersonaPressureLevel,
        TurnToolPathLedger,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn parse_execution_state_response_coerces_nested_fields() {
        let raw = json!({
            "status": { "value": "blocked" },
            "goal": ["reduce esp size", "then move on"],
            "progress": { "step": "fixed parser warnings" },
            "blocker": false,
            "next_action": { "step": "run size diff" },
            "last_output": { "note": "tests green" }
        })
        .to_string();
        let parsed = parse_execution_state_response(&raw, 11).unwrap();
        assert_eq!(parsed.status, ExecutionStatus::Blocked);
        assert_eq!(parsed.goal, "reduce esp size; then move on");
        assert!(parsed.progress.contains("step: fixed parser warnings"));
        assert_eq!(parsed.blocker, "false");
        assert!(parsed.next_action.contains("step: run size diff"));
        assert!(parsed.last_output.contains("note: tests green"));
    }

    #[test]
    fn parse_execution_state_response_collects_working_set_fields() {
        let raw = json!({
            "status": "active",
            "goal": "收口 execution state",
            "active_constraints": [
                "必须保持 Linux / ESP 同物种语义",
                "不能新开 store"
            ],
            "open_questions": "是否需要把 working set 头项直接映射给 next_action",
            "latest_observations": [
                "turn ledger observation 已接入 archive replay",
                "CLI status 也能看到 observation"
            ],
            "next_best_actions": [
                "补 execution state working set 回归测试",
                "更新 render block"
            ]
        })
        .to_string();

        let parsed = parse_execution_state_response(&raw, 12).unwrap();

        assert_eq!(parsed.goal, "收口 execution state");
        assert_eq!(parsed.active_constraints.len(), 2);
        assert!(parsed
            .active_constraints
            .iter()
            .any(|item| item.contains("Linux / ESP")));
        assert_eq!(parsed.open_questions.len(), 1);
        assert_eq!(
            parsed.progress,
            "turn ledger observation 已接入 archive replay"
        );
        assert_eq!(
            parsed.latest_observations,
            vec!["CLI status 也能看到 observation".to_string()]
        );
        assert_eq!(
            parsed.next_best_actions,
            vec!["更新 render block".to_string()]
        );
        assert_eq!(
            parsed.next_action,
            "补 execution state working set 回归测试"
        );
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

    #[derive(Default)]
    struct StubTurnLedgerStore {
        entries: Mutex<HashMap<String, TurnLedger>>,
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

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), ledger.clone());
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
                active_constraints: vec!["不能新开 store".to_string()],
                open_questions: vec!["是否需要保留 next_action 头项".to_string()],
                latest_observations: vec!["turn ledger 已有 replay substrate".to_string()],
                next_best_actions: vec!["接入 prompt".to_string(), "补回归测试".to_string()],
                updated_at: 1,
            },
            512,
        )
        .unwrap();
        assert!(block.contains("## Execution State"));
        assert!(block.contains("Goal: 推进 execution state"));
        assert!(block.contains("Next: 接入 prompt"));
        assert!(block.contains("Constraints: 不能新开 store"));
        assert!(block.contains("Open questions: 是否需要保留 next_action 头项"));
        assert!(block.contains("Observations: turn ledger 已有 replay substrate"));
        assert!(block.contains("Next best actions: 补回归测试"));
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
                active_constraints: vec!["不能新开 store".to_string()],
                open_questions: vec!["现在是否该扩 ExecutionState".to_string()],
                latest_observations: vec!["subject state 已落账".to_string()],
                next_best_actions: vec!["接 prompt".to_string(), "补测试".to_string()],
                updated_at: 1,
            }),
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 execution state".to_string(),
                progress: "prompt 已接入".to_string(),
                blocker: String::new(),
                next_action: String::new(),
                last_output: String::new(),
                active_constraints: Vec::new(),
                open_questions: vec!["还要不要扩 render block".to_string()],
                latest_observations: vec!["prompt block 已经接上".to_string()],
                next_best_actions: vec!["补回归测试".to_string()],
                updated_at: 2,
            },
            3,
        )
        .unwrap();

        assert_eq!(merged.goal, "收口 execution state");
        assert_eq!(merged.progress, "prompt 已接入");
        assert_eq!(merged.next_action, "接 prompt");
        assert_eq!(merged.last_output, "store ok");
        assert_eq!(
            merged.active_constraints,
            vec!["不能新开 store".to_string()]
        );
        assert_eq!(
            merged.open_questions,
            vec!["还要不要扩 render block".to_string()]
        );
        assert_eq!(
            merged.latest_observations,
            vec!["prompt block 已经接上".to_string()]
        );
        assert_eq!(merged.next_best_actions, vec!["补回归测试".to_string()]);
    }

    #[test]
    fn same_focus_matches_extended_goal_text() {
        let merged = merge_execution_state(
            Some(&ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 execution state".to_string(),
                progress: "store 已完成".to_string(),
                blocker: String::new(),
                next_action: "补测试".to_string(),
                last_output: String::new(),
                updated_at: 1,
                ..ExecutionState::default()
            }),
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "继续收口 execution state 并补回归测试".to_string(),
                progress: "开始整理回归项".to_string(),
                blocker: String::new(),
                next_action: String::new(),
                last_output: String::new(),
                updated_at: 2,
                ..ExecutionState::default()
            },
            3,
        )
        .unwrap();

        assert_eq!(merged.progress, "开始整理回归项");
        assert_eq!(merged.next_action, "补测试");
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
                ..ExecutionState::default()
            }),
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "推进 linux 任务面".to_string(),
                progress: "开始梳理链路".to_string(),
                blocker: String::new(),
                next_action: "补测试".to_string(),
                last_output: String::new(),
                updated_at: 2,
                ..ExecutionState::default()
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
                ..ExecutionState::default()
            },
            2,
        )
        .unwrap();
        assert!(!should_persist_execution_state(&state));
    }

    #[test]
    fn generic_state_without_concrete_fields_is_dropped() {
        let state = normalize_execution_state(
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "继续处理".to_string(),
                progress: "推进中".to_string(),
                blocker: "无".to_string(),
                next_action: "继续".to_string(),
                last_output: "好的".to_string(),
                updated_at: 1,
                ..ExecutionState::default()
            },
            2,
        );
        assert!(state.is_none());
    }

    #[test]
    fn generic_goal_falls_back_to_concrete_next_action() {
        let state = normalize_execution_state(
            ExecutionState {
                status: ExecutionStatus::Active,
                goal: "继续处理".to_string(),
                progress: "已完成 tool round 去重".to_string(),
                blocker: String::new(),
                next_action: "补 execution state 回归测试".to_string(),
                last_output: String::new(),
                updated_at: 1,
                ..ExecutionState::default()
            },
            2,
        )
        .unwrap();
        assert_eq!(state.goal, "已完成 tool round 去重");
        assert_eq!(state.next_action, "补 execution state 回归测试");
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
                ..ExecutionState::default()
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
                ..ExecutionState::default()
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
        let turn_ledger_store = StubTurnLedgerStore::default();
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
                turn_ledger_store: &turn_ledger_store,
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
        let turn_ledger_store = StubTurnLedgerStore::default();
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
                turn_ledger_store: &turn_ledger_store,
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
                    ..ExecutionState::default()
                },
            )])),
            clears: Mutex::new(0),
        };
        let turn_ledger_store = StubTurnLedgerStore::default();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient { content: "null" };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
                turn_ledger_store: &turn_ledger_store,
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
                    ..ExecutionState::default()
                },
            )])),
            clears: Mutex::new(0),
        };
        let turn_ledger_store = StubTurnLedgerStore::default();
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
                turn_ledger_store: &turn_ledger_store,
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

    #[test]
    fn provisional_seed_persists_concrete_tool_backed_state() {
        let execution_store = StubExecutionStateStore::default();
        let seeded = seed_execution_state_from_turn(
            &execution_store,
            ProvisionalExecutionStateInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "帮我配置 QQ 邮箱账户",
                reply_content: "我先检查当前邮件状态，然后继续配置。",
                reply_requests_input: false,
                tool_calls: 1,
                now_secs: 77,
                turn_observation: Some(&TurnObservationLedger {
                    execution_class: TurnExecutionClass::ToolAssisted,
                    deliberation_class: TurnDeliberationClass::Standard,
                    final_outcome: "final_answer".to_string(),
                    pressure: TurnPersonaPressureLevel::Normal,
                    mode: TurnModeSnapshotLedger {
                        current_mode: "normal".to_string(),
                        allow_non_voice_outbound: true,
                        allow_idle_self_runtime: true,
                    },
                    tool_path: TurnToolPathLedger {
                        path: "tool_round".to_string(),
                        tool_calls: 1,
                        react_rounds: 1,
                        current_primary_delivered: false,
                    },
                    blocker: None,
                }),
            },
        )
        .unwrap();

        assert!(seeded);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(stored.goal, "帮我配置 QQ 邮箱账户");
        assert_eq!(
            stored.next_action,
            "deliver current primary answer before more tool work"
        );
        assert_eq!(stored.updated_at, 77);
    }

    #[test]
    fn provisional_seed_does_not_overwrite_existing_state_for_low_signal_turn() {
        let execution_store = StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-1".to_string(),
                ExecutionState {
                    status: ExecutionStatus::Active,
                    goal: "配置 QQ 邮箱账户".to_string(),
                    next_action: "补认证信息并继续配置".to_string(),
                    updated_at: 11,
                    ..ExecutionState::default()
                },
            )])),
            clears: Mutex::new(0),
        };
        let seeded = seed_execution_state_from_turn(
            &execution_store,
            ProvisionalExecutionStateInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "好",
                reply_content: "继续处理中。",
                reply_requests_input: false,
                tool_calls: 0,
                now_secs: 12,
                turn_observation: None,
            },
        )
        .unwrap();

        assert!(!seeded);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(stored.goal, "配置 QQ 邮箱账户");
        assert_eq!(stored.next_action, "补认证信息并继续配置");
    }

    #[test]
    fn provisional_seed_persists_blocked_state_for_input_request_reply() {
        let execution_store = StubExecutionStateStore {
            entries: Mutex::new(HashMap::from([(
                "chat-1".to_string(),
                ExecutionState {
                    status: ExecutionStatus::Active,
                    goal: "配置 QQ 邮箱账户".to_string(),
                    progress: "已经确认 IMAP/SMTP 入口".to_string(),
                    next_action: "继续补账户凭据".to_string(),
                    updated_at: 11,
                    ..ExecutionState::default()
                },
            )])),
            clears: Mutex::new(0),
        };
        let seeded = seed_execution_state_from_turn(
            &execution_store,
            ProvisionalExecutionStateInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续",
                reply_content: "请先提供 QQ 邮箱的授权码，我才能继续配置。",
                reply_requests_input: true,
                tool_calls: 0,
                now_secs: 12,
                turn_observation: None,
            },
        )
        .unwrap();

        assert!(seeded);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert_eq!(stored.status, ExecutionStatus::Blocked);
        assert_eq!(stored.goal, "配置 QQ 邮箱账户");
        assert_eq!(stored.blocker, "请先提供 QQ 邮箱的授权码，我才能继续配置。");
        assert_eq!(
            stored.next_action,
            "请先提供 QQ 邮箱的授权码，我才能继续配置。"
        );
    }

    #[test]
    fn refresh_input_omits_summary_when_existing_state_is_present() {
        let input = build_execution_state_refresh_input(
            Some(&ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 execution state".to_string(),
                progress: "已经接上 store".to_string(),
                blocker: String::new(),
                next_action: "整理上下文预算".to_string(),
                last_output: String::new(),
                updated_at: 1,
                ..ExecutionState::default()
            }),
            None,
            &[SessionMessage {
                role: "user".to_string(),
                content: "继续处理 execution state".to_string(),
            }],
            None,
            memory_policy(MemoryProfile::Embedded).execution_state,
        );
        assert!(input.contains("## Execution State"));
        assert!(!input.contains("## Session Summary"));
        assert!(input.contains("## Extraction Rules"));
    }

    #[test]
    fn refresh_input_includes_recent_turn_observation_block() {
        let input = build_execution_state_refresh_input(
            None,
            Some("continue closing the execution state loop"),
            &[SessionMessage {
                role: "user".to_string(),
                content: "继续".to_string(),
            }],
            Some(&TurnObservationLedger {
                execution_class: TurnExecutionClass::ToolAssisted,
                deliberation_class: TurnDeliberationClass::HardReasoning,
                final_outcome: "surface_finalization".to_string(),
                pressure: TurnPersonaPressureLevel::Cautious,
                mode: TurnModeSnapshotLedger {
                    current_mode: "normal".to_string(),
                    allow_non_voice_outbound: true,
                    allow_idle_self_runtime: true,
                },
                tool_path: TurnToolPathLedger {
                    path: "surface_finalization".to_string(),
                    tool_calls: 2,
                    react_rounds: 2,
                    current_primary_delivered: false,
                },
                blocker: Some(TurnBlockerLedger {
                    kind: "retryable".to_string(),
                    failed_calls: 1,
                    total_calls: 1,
                }),
            }),
            memory_policy(MemoryProfile::Embedded).execution_state,
        );

        assert!(input.contains("## Latest Turn Observation"));
        assert!(input.contains("Tool path: surface_finalization"));
        assert!(input.contains("## Recent Conversation"));
    }

    #[test]
    fn generic_reply_is_not_captured_as_last_output() {
        assert!(!should_capture_last_output("好的，这轮继续处理。"));
        assert!(should_capture_last_output(
            "我已经把 execution state 的 merge 规则改成按具体目标优先了。"
        ));
    }

    #[test]
    fn refresh_enriches_working_set_with_recent_turn_observation() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续收口当前这一轮".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我先基于上轮执行结果继续收敛。".to_string(),
                },
            ],
        };
        let summary_store = StubSessionSummaryStore::default();
        let execution_store = StubExecutionStateStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        turn_ledger_store
            .set(
                &relationship_scope_id("qq_channel", "chat-1"),
                &TurnLedger {
                    req_id: "run-execution".to_string(),
                    observation: Some(TurnObservationLedger {
                        execution_class: TurnExecutionClass::ToolAssisted,
                        deliberation_class: TurnDeliberationClass::HardReasoning,
                        final_outcome: "surface_finalization".to_string(),
                        pressure: TurnPersonaPressureLevel::Cautious,
                        mode: TurnModeSnapshotLedger {
                            current_mode: "normal".to_string(),
                            allow_non_voice_outbound: true,
                            allow_idle_self_runtime: true,
                        },
                        tool_path: TurnToolPathLedger {
                            path: "surface_finalization".to_string(),
                            tool_calls: 2,
                            react_rounds: 2,
                            current_primary_delivered: false,
                        },
                        blocker: Some(TurnBlockerLedger {
                            kind: "retryable".to_string(),
                            failed_calls: 1,
                            total_calls: 1,
                        }),
                    }),
                    ..TurnLedger::default()
                },
            )
            .unwrap();
        let mut http = DummyHttpClient;
        let llm = FixedLlmClient {
            content: r#"{"status":"active","goal":"收口 execution state","progress":"把 replay substrate 继续收紧","next_action":"补 execution state 回归测试"}"#,
        };

        let outcome = run_execution_state_refresh(
            &mut http,
            &llm,
            ExecutionStateRefreshContext {
                session_store: &session_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_store,
                turn_ledger_store: &turn_ledger_store,
            },
            ExecutionStateRefreshInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续收口当前这一轮",
                reply_content: "我先基于上轮执行结果继续收敛。",
                pressure: PressureLevel::Normal,
                tool_calls: 1,
                now_secs: 79,
            },
            MemoryProfile::Standard,
        )
        .unwrap();

        assert_eq!(outcome, ExecutionStateRefreshOutcome::Updated);
        let stored = execution_store.get("chat-1").unwrap().unwrap();
        assert!(stored
            .latest_observations
            .iter()
            .any(|item| item.contains("surface_finalization")));
        assert!(stored
            .latest_observations
            .iter()
            .any(|item| item.contains("retryable")));
        assert!(stored
            .next_best_actions
            .iter()
            .any(|item| item.contains("primary answer")));
        assert!(stored
            .next_best_actions
            .iter()
            .any(|item| item.contains("retryable blocker")));
    }
}
