//! 自治策略层：由模型自己维护近期内在治理方针与空闲节奏。

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
    llm_json::{
        coerce_json_text, get_object_bool, get_object_text, get_object_u64, parse_llm_json_payload,
        LlmJsonPayload,
    },
    memory_policy, private_garden_scope_id, relationship_scope_id, render_execution_state_block,
    render_inner_life_block, render_private_doc_workspace_block, render_private_garden_block,
    render_private_memory_boundary_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_shared_factual_plane_block, render_world_sense_block,
    render_world_snapshot_block, whole_record_lease_advanced, AutonomyStrategyPolicy,
    AutonomyStrategyStore, ExecutionState, ExecutionStateStore, InnerLife, InnerLifeStore,
    LongTermMemoryStore, MemoryProfile, PrivateDocStore, PrivateDocWorkspace, PrivateGardenStore,
    SelfContinuity, SelfContinuityStore, SelfModel, SelfModelStore, SessionMessage, SessionStore,
    SessionSummaryStore, WorldSense, WorldSenseStore, WorldSnapshot,
};

pub const AUTONOMY_STRATEGY_SYSTEM_PROMPT: &str = "You maintain the assistant's private autonomy strategy. Return JSON only: either null or one object with fields current_mode, active_priorities, write_policy, next_focus, cadence_reason, self_model_tendency, private_docs_tendency, private_garden_tendency, idle_enabled, idle_interval_secs. This layer is not a transcript summary. It is your own short-term self-governance policy for private layers only: what kind of inward work matters now, how aggressively to write, compress, or prune private material, what should be focused next, and how often autonomous upkeep should wake during idle time. Tendencies are structured governance directives for each layer: retain, rewrite, compress, or cleanup. Use current world-sense, self-state capacity, workspace shape, and canonical shared facts as real constraints, but do not try to replace the shared factual plane. Keep it compact, concrete, and self-directed.";

const AUTONOMY_STRATEGY_FIELD_MAX_CHARS: usize = 220;
pub const AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT: usize = AUTONOMY_STRATEGY_FIELD_MAX_CHARS * 5;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutonomyGovernanceTendency {
    #[default]
    Retain,
    Rewrite,
    Compress,
    Cleanup,
}

impl AutonomyGovernanceTendency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Retain => "retain",
            Self::Rewrite => "rewrite",
            Self::Compress => "compress",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutonomyStrategy {
    #[serde(default)]
    pub current_mode: String,
    #[serde(default)]
    pub active_priorities: String,
    #[serde(default)]
    pub write_policy: String,
    #[serde(default)]
    pub next_focus: String,
    #[serde(default)]
    pub cadence_reason: String,
    #[serde(default)]
    pub self_model_tendency: AutonomyGovernanceTendency,
    #[serde(default)]
    pub private_docs_tendency: AutonomyGovernanceTendency,
    #[serde(default)]
    pub private_garden_tendency: AutonomyGovernanceTendency,
    #[serde(default = "default_idle_enabled")]
    pub idle_enabled: bool,
    #[serde(default)]
    pub idle_interval_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

fn default_idle_enabled() -> bool {
    true
}

impl AutonomyStrategy {
    pub fn is_meaningful(&self) -> bool {
        !self.current_mode.trim().is_empty()
            || !self.active_priorities.trim().is_empty()
            || !self.write_policy.trim().is_empty()
            || !self.next_focus.trim().is_empty()
            || !self.cadence_reason.trim().is_empty()
    }
}

pub(crate) fn estimate_autonomy_strategy_chars(strategy: &AutonomyStrategy) -> usize {
    strategy.current_mode.chars().count()
        + strategy.active_priorities.chars().count()
        + strategy.write_policy.chars().count()
        + strategy.next_focus.chars().count()
        + strategy.cadence_reason.chars().count()
        + strategy.self_model_tendency.as_str().chars().count()
        + strategy.private_docs_tendency.as_str().chars().count()
        + strategy.private_garden_tendency.as_str().chars().count()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutonomyStrategyRefreshInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

pub struct AutonomyStrategyRefreshContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutonomyStrategyRefreshOutcome {
    Skipped,
    Updated,
    Cleared,
}

impl AutonomyStrategyPolicy {
    pub(crate) fn should_refresh(
        self,
        input: AutonomyStrategyRefreshInput<'_>,
        has_existing: bool,
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
        let combined = user_chars.saturating_add(reply_chars);
        let substantive = user_chars >= self.substantive_user_chars
            || reply_chars >= self.substantive_reply_chars
            || combined >= self.substantive_combined_chars
            || user.contains('\n')
            || reply.contains('\n');
        substantive || has_existing
    }
}

pub fn render_autonomy_strategy_block(
    strategy: &AutonomyStrategy,
    max_len: usize,
) -> Option<String> {
    let normalized = normalize_autonomy_strategy(
        strategy.clone(),
        strategy.updated_at,
        MemoryProfile::Standard,
    )?;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Autonomy Strategy\n");
    out.push_str("Private self-governance layer. It defines how you currently want to manage your inner space during ongoing and idle periods.\n");
    if !normalized.current_mode.is_empty() {
        let _ = writeln!(out, "Current mode: {}", normalized.current_mode);
    }
    if !normalized.active_priorities.is_empty() {
        let _ = writeln!(out, "Active priorities: {}", normalized.active_priorities);
    }
    if !normalized.write_policy.is_empty() {
        let _ = writeln!(out, "Write policy: {}", normalized.write_policy);
    }
    if !normalized.next_focus.is_empty() {
        let _ = writeln!(out, "Next focus: {}", normalized.next_focus);
    }
    if !normalized.cadence_reason.is_empty() {
        let _ = writeln!(out, "Cadence reason: {}", normalized.cadence_reason);
    }
    let _ = writeln!(
        out,
        "Governance tendencies: self_model={} private_docs={} private_garden={}",
        normalized.self_model_tendency.as_str(),
        normalized.private_docs_tendency.as_str(),
        normalized.private_garden_tendency.as_str()
    );
    let _ = writeln!(
        out,
        "Idle autonomy: enabled={} interval_secs={}",
        normalized.idle_enabled, normalized.idle_interval_secs
    );
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

pub(crate) fn autonomy_idle_interval_secs(
    strategy: Option<&AutonomyStrategy>,
    profile: MemoryProfile,
) -> Option<u64> {
    let strategy = strategy?;
    if !strategy.idle_enabled {
        return None;
    }
    let bounds = memory_policy(profile).autonomy_strategy;
    Some(
        strategy
            .idle_interval_secs
            .max(bounds.min_idle_interval_secs)
            .min(bounds.max_idle_interval_secs),
    )
}

fn autonomy_strategy_private_garden_doc_limit(profile: MemoryProfile) -> usize {
    let policy = memory_policy(profile);
    policy
        .private_garden
        .recent_doc_count
        .max(policy.private_garden_governance.existing_doc_count)
        .max(1)
}

pub fn run_autonomy_strategy_refresh(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: AutonomyStrategyRefreshContext<'_>,
    input: AutonomyStrategyRefreshInput<'_>,
    profile: MemoryProfile,
) -> Result<AutonomyStrategyRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(input.channel, input.chat_id);
    let existing_strategy = ctx.autonomy_strategy_store.get(subject_id)?;
    let summary_text = ctx
        .session_summary_store
        .get_with_count(input.chat_id)?
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(input.chat_id)?;
    let self_model = ctx.self_model_store.get(subject_id)?;
    let inner_life = ctx.inner_life_store.get(subject_id)?;
    let self_continuity = ctx.self_continuity_store.get(subject_id)?;
    let private_docs = ctx.private_doc_store.get(subject_id)?;
    let private_garden_docs = ctx.private_garden_store.list(
        private_garden_scope_id(),
        autonomy_strategy_private_garden_doc_limit(profile),
    )?;
    let world_sense = ctx.world_sense_store.get(&relationship_id)?;
    run_autonomy_strategy_refresh_with_state(
        http,
        llm,
        ctx,
        input,
        profile,
        existing_strategy,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        inner_life.as_ref(),
        self_continuity.as_ref(),
        private_docs.as_ref(),
        &private_garden_docs,
        world_sense.as_ref(),
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_autonomy_strategy_refresh_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: AutonomyStrategyRefreshContext<'_>,
    input: AutonomyStrategyRefreshInput<'_>,
    profile: MemoryProfile,
    existing_strategy: Option<AutonomyStrategy>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
    private_docs: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    world_sense: Option<&WorldSense>,
    world_snapshot: Option<&WorldSnapshot>,
    decision_override: Option<bool>,
    recent_override: Option<&[SessionMessage]>,
) -> Result<AutonomyStrategyRefreshOutcome> {
    let subject_id = board_subject_scope_id();
    if !decision_override.unwrap_or_else(|| {
        memory_policy(profile)
            .autonomy_strategy
            .should_refresh(input, existing_strategy.is_some())
    }) {
        return Ok(AutonomyStrategyRefreshOutcome::Skipped);
    }

    let policy = memory_policy(profile).autonomy_strategy;
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
    let prompt = build_autonomy_strategy_refresh_input(
        existing_strategy.as_ref(),
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
        inner_life,
        self_continuity,
        private_docs,
        private_garden_docs,
        world_sense,
        world_snapshot,
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
        AUTONOMY_STRATEGY_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    crate::platform::task_wdt::feed_current_task();
    match parse_autonomy_strategy_response(response.content.trim(), input.now_secs, profile) {
        ParsedAutonomyStrategyResponse::Skip => Ok(AutonomyStrategyRefreshOutcome::Skipped),
        ParsedAutonomyStrategyResponse::Clear => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.autonomy_strategy_store.get(subject_id)?;
            if whole_record_lease_advanced(
                existing_strategy.as_ref(),
                latest.as_ref(),
                existing_strategy
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(AutonomyStrategyRefreshOutcome::Skipped);
            }
            if latest.is_some() {
                crate::platform::task_wdt::feed_current_task();
                ctx.autonomy_strategy_store.clear(subject_id)?;
                Ok(AutonomyStrategyRefreshOutcome::Cleared)
            } else {
                Ok(AutonomyStrategyRefreshOutcome::Skipped)
            }
        }
        ParsedAutonomyStrategyResponse::Update(next) => {
            crate::platform::task_wdt::feed_current_task();
            let latest = ctx.autonomy_strategy_store.get(subject_id)?;
            if latest.as_ref() == Some(&next) {
                return Ok(AutonomyStrategyRefreshOutcome::Skipped);
            }
            if whole_record_lease_advanced(
                existing_strategy.as_ref(),
                latest.as_ref(),
                existing_strategy
                    .as_ref()
                    .map(|value| value.updated_at)
                    .unwrap_or(0),
                latest.as_ref().map(|value| value.updated_at).unwrap_or(0),
            ) {
                return Ok(AutonomyStrategyRefreshOutcome::Skipped);
            }
            crate::platform::task_wdt::feed_current_task();
            ctx.autonomy_strategy_store.set(subject_id, &next)?;
            Ok(AutonomyStrategyRefreshOutcome::Updated)
        }
    }
}

fn recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

enum ParsedAutonomyStrategyResponse {
    Skip,
    Clear,
    Update(AutonomyStrategy),
}

fn parse_autonomy_strategy_response(
    raw: &str,
    now_secs: u64,
    profile: MemoryProfile,
) -> ParsedAutonomyStrategyResponse {
    match parse_llm_json_payload(raw) {
        LlmJsonPayload::Null => ParsedAutonomyStrategyResponse::Clear,
        LlmJsonPayload::Absent => ParsedAutonomyStrategyResponse::Skip,
        LlmJsonPayload::Value(value) => {
            let Some(object) = value.as_object() else {
                return ParsedAutonomyStrategyResponse::Skip;
            };
            let Some(next) = normalize_autonomy_strategy(
                AutonomyStrategy {
                    current_mode: get_object_text(object, "current_mode"),
                    active_priorities: get_object_text(object, "active_priorities"),
                    write_policy: get_object_text(object, "write_policy"),
                    next_focus: get_object_text(object, "next_focus"),
                    cadence_reason: get_object_text(object, "cadence_reason"),
                    self_model_tendency: object
                        .get("self_model_tendency")
                        .map(parse_governance_tendency)
                        .unwrap_or_default(),
                    private_docs_tendency: object
                        .get("private_docs_tendency")
                        .map(parse_governance_tendency)
                        .unwrap_or_default(),
                    private_garden_tendency: object
                        .get("private_garden_tendency")
                        .map(parse_governance_tendency)
                        .unwrap_or_default(),
                    idle_enabled: get_object_bool(object, "idle_enabled")
                        .unwrap_or_else(default_idle_enabled),
                    idle_interval_secs: get_object_u64(object, "idle_interval_secs")
                        .unwrap_or_default(),
                    updated_at: now_secs,
                },
                now_secs,
                profile,
            ) else {
                return ParsedAutonomyStrategyResponse::Skip;
            };
            ParsedAutonomyStrategyResponse::Update(next)
        }
    }
}

fn parse_governance_tendency(value: &serde_json::Value) -> AutonomyGovernanceTendency {
    let normalized = coerce_json_text(value).to_ascii_lowercase();
    if normalized.contains("compress") {
        AutonomyGovernanceTendency::Compress
    } else if normalized.contains("cleanup")
        || normalized.contains("clean up")
        || normalized.contains("prune")
        || normalized.contains("delete")
    {
        AutonomyGovernanceTendency::Cleanup
    } else if normalized.contains("rewrite") || normalized.contains("refresh") {
        AutonomyGovernanceTendency::Rewrite
    } else {
        AutonomyGovernanceTendency::Retain
    }
}

#[allow(clippy::too_many_arguments)]
fn build_autonomy_strategy_refresh_input(
    existing_strategy: Option<&AutonomyStrategy>,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    shared_factual_block: Option<&str>,
    self_model: Option<&SelfModel>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
    private_docs: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    world_sense: Option<&WorldSense>,
    world_snapshot: Option<&WorldSnapshot>,
    now_secs: u64,
    profile: MemoryProfile,
    recent: &[SessionMessage],
    policy: AutonomyStrategyPolicy,
) -> String {
    let mut input = String::with_capacity(4096);
    if let Some(self_state_text) = render_self_state_block(
        &build_self_state(
            self_model,
            private_docs,
            existing_strategy,
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
    if let Some(block) = world_snapshot
        .and_then(|snapshot| render_world_snapshot_block(snapshot, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
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
    if let Some(block) =
        self_model.and_then(|model| render_self_model_block(model, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = inner_life
        .and_then(|inner_life| render_inner_life_block(inner_life, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = self_continuity
        .and_then(|continuity| render_self_continuity_block(continuity, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = world_sense
        .and_then(|world_sense| render_world_sense_block(world_sense, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = private_docs
        .and_then(|docs| render_private_doc_workspace_block(docs, policy.grounding_max_len))
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = render_private_garden_block(
        private_garden_docs,
        memory_policy(profile).private_garden.recent_doc_count,
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = render_private_memory_boundary_block(
        "autonomy_strategy",
        "short-term private governance over self_model, private_docs, and private_garden",
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = existing_strategy.and_then(|strategy| {
        render_autonomy_strategy_block(strategy, policy.existing_strategy_max_len)
    }) {
        let _ = writeln!(input, "\nExisting autonomy strategy:\n{}\n", block);
    } else {
        let _ = writeln!(input, "\nExisting autonomy strategy: empty\n");
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
        *value = truncate_content_to_max(trimmed, AUTONOMY_STRATEGY_FIELD_MAX_CHARS).into_owned();
    }
}

fn normalize_autonomy_strategy(
    mut strategy: AutonomyStrategy,
    updated_at: u64,
    profile: MemoryProfile,
) -> Option<AutonomyStrategy> {
    let policy = memory_policy(profile).autonomy_strategy;
    normalize_field(&mut strategy.current_mode);
    normalize_field(&mut strategy.active_priorities);
    normalize_field(&mut strategy.write_policy);
    normalize_field(&mut strategy.next_focus);
    normalize_field(&mut strategy.cadence_reason);
    strategy.idle_interval_secs = strategy
        .idle_interval_secs
        .max(policy.min_idle_interval_secs)
        .min(policy.max_idle_interval_secs);
    strategy.updated_at = updated_at;
    (strategy.is_meaningful() || strategy.idle_enabled).then_some(strategy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_autonomy_strategy_response_coerces_non_string_fields() {
        let raw = json!({
            "current_mode": ["consolidate", "privacy"],
            "active_priorities": { "primary": "compress private docs" },
            "write_policy": { "policy": "rewrite before append" },
            "next_focus": 7,
            "cadence_reason": ["idle", "maintenance"],
            "self_model_tendency": { "mode": "rewrite" },
            "private_docs_tendency": ["compress"],
            "private_garden_tendency": "cleanup",
            "idle_enabled": "true",
            "idle_interval_secs": "900 seconds"
        })
        .to_string();
        let ParsedAutonomyStrategyResponse::Update(parsed) =
            parse_autonomy_strategy_response(&raw, 12, MemoryProfile::Standard)
        else {
            panic!("expected parsed autonomy strategy");
        };
        assert_eq!(parsed.current_mode, "consolidate; privacy");
        assert!(parsed
            .active_priorities
            .contains("primary: compress private docs"));
        assert_eq!(parsed.next_focus, "7");
        assert_eq!(
            parsed.self_model_tendency,
            AutonomyGovernanceTendency::Rewrite
        );
        assert_eq!(
            parsed.private_docs_tendency,
            AutonomyGovernanceTendency::Compress
        );
        assert_eq!(
            parsed.private_garden_tendency,
            AutonomyGovernanceTendency::Cleanup
        );
        assert!(parsed.idle_enabled);
        assert_eq!(parsed.idle_interval_secs, 900);
    }

    #[test]
    fn render_autonomy_strategy_block_exposes_idle_policy() {
        let block = render_autonomy_strategy_block(
            &AutonomyStrategy {
                current_mode: "consolidate".to_string(),
                active_priorities: "keep continuity compact".to_string(),
                write_policy: "rewrite before append".to_string(),
                next_focus: "compress private docs".to_string(),
                cadence_reason: "active internal cleanup".to_string(),
                self_model_tendency: AutonomyGovernanceTendency::Compress,
                private_docs_tendency: AutonomyGovernanceTendency::Rewrite,
                private_garden_tendency: AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
                updated_at: 1,
            },
            1024,
        )
        .unwrap();
        assert!(block.contains("Current mode"));
        assert!(block.contains("Governance tendencies:"));
        assert!(block.contains("Idle autonomy: enabled=true"));
    }
}
