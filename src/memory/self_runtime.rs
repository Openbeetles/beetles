//! 自治运行层：由 LLM 决定是否经营自己的内在空间。

use crate::bus::{IngressKind, PcMsg, SystemInboundTx};
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::platform::SkillStorage;
use crate::task::TaskStore;
use crate::util::{current_unix_secs, scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    autonomy_idle_interval_secs, build_archive_evidence_block, build_self_state,
    build_world_snapshot,
    llm_json::{get_object_bool, get_object_text, parse_llm_json_payload, LlmJsonPayload},
    memory_capability_profile, memory_policy, render_autonomy_strategy_block,
    render_execution_state_block, render_inner_life_block, render_internal_memory_topology_block,
    render_private_doc_workspace_block, render_private_garden_block,
    render_private_memory_boundary_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_world_sense_block, render_world_snapshot_block,
    run_autonomy_strategy_refresh_with_state, run_inner_life_refresh_with_state,
    run_memory_governance_kernel, run_memory_hygiene_jobs, run_outer_voice_refresh_with_state,
    run_private_doc_workspace_refresh_with_state, run_private_garden_governance_with_state,
    run_self_continuity_refresh_with_state, run_world_sense_refresh_with_state,
    touch_self_continuity_runtime, AutonomyGovernanceTendency, AutonomyStrategyRefreshContext,
    AutonomyStrategyRefreshInput, AutonomyStrategyRefreshOutcome, AutonomyStrategyStore,
    ExecutionStateStore, InnerLifeRefreshContext, InnerLifeRefreshInput, InnerLifeRefreshOutcome,
    InnerLifeStore, InternalMemoryLayerFocus, LongTermMemoryStore, MemoryGovernanceContext,
    MemoryGovernanceInput, MemoryHygieneContext, MemoryProfile, MemoryStore, MentalPrivacyStore,
    OuterVoiceRefreshContext, OuterVoiceRefreshInput, OuterVoiceRefreshOutcome, OuterVoiceStore,
    PrivateDocStore, PrivateDocWorkspaceRefreshContext, PrivateDocWorkspaceRefreshInput,
    PrivateDocWorkspaceRefreshOutcome, PrivateGardenGovernanceContext,
    PrivateGardenGovernanceInput, PrivateGardenGovernanceOutcome, PrivateGardenStore,
    RemindAtStore, SelfContinuityRefreshContext, SelfContinuityRefreshInput,
    SelfContinuityRefreshOutcome, SelfContinuityStore, SelfMemorySpaceBottleneck,
    SelfMemorySpacePressure, SelfModelStore, SelfState, SessionStore, SessionSummaryStore,
    SharedFactualPlaneSnapshot, SharedFactualReconcileAction, TurnLedgerStore,
    WorldSenseRefreshContext, WorldSenseRefreshInput, WorldSenseRefreshOutcome, WorldSenseStore,
    WorldSnapshotContext,
};

pub const SELF_RUNTIME_SYSTEM_PROMPT: &str = "You govern the assistant's private inward space. Respect the current autonomy strategy unless the latest world state or self-state clearly requires a different emphasis. Return JSON only: one object with fields refresh_inner_life, inner_life_intent, refresh_private_docs, private_docs_intent, private_docs_action, refresh_self_continuity, self_continuity_intent, refresh_private_garden, private_garden_intent, private_garden_action, boundary_flush, boundary_flush_reason, request_factual_refresh, factual_reconcile_action, factual_reconcile_intent. Use true only when that layer should change now. Runtime governance actions are hold, rewrite, compress, or cleanup. factual_reconcile_action is hold, reinforce, correct, conflict, or stale. This runtime governs private layers directly, and may request a shared factual refresh when archive evidence suggests the canonical record should be reinforced, corrected, reconciled, or reviewed. private_docs is the governed inner workspace; private_garden is the free-form private workspace. Use self-state capacity, world-sense, current autonomy strategy, canonical shared facts, archive evidence, and boundary signals to decide whether to write, compress, reorganize, or leave memory untouched. Keep intents short and concrete. Favor autonomy, but do not churn memory without gain.";
pub const SELF_RUNTIME_CHANNEL: &str = "_self_runtime";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfRuntimeTrigger {
    PostReply,
    IdleTick,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfRuntimeJobPayload {
    pub trigger: SelfRuntimeTrigger,
    #[serde(default)]
    pub source_channel: String,
    #[serde(default)]
    pub user_content: String,
    #[serde(default)]
    pub reply_content: String,
    #[serde(default)]
    pub tool_calls: u32,
    #[serde(default)]
    pub external_content_used: bool,
    pub now_secs: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct SelfRuntimeDecision {
    #[serde(default)]
    pub refresh_inner_life: bool,
    #[serde(default)]
    pub inner_life_intent: String,
    #[serde(default)]
    pub refresh_private_docs: bool,
    #[serde(default)]
    pub private_docs_intent: String,
    #[serde(default)]
    pub private_docs_action: SelfRuntimeGovernanceAction,
    #[serde(default)]
    pub refresh_self_continuity: bool,
    #[serde(default)]
    pub self_continuity_intent: String,
    #[serde(default)]
    pub refresh_private_garden: bool,
    #[serde(default)]
    pub private_garden_intent: String,
    #[serde(default)]
    pub private_garden_action: SelfRuntimeGovernanceAction,
    #[serde(default)]
    pub boundary_flush: bool,
    #[serde(default)]
    pub boundary_flush_reason: String,
    #[serde(default)]
    pub request_factual_refresh: bool,
    #[serde(default)]
    pub factual_reconcile_action: SharedFactualReconcileAction,
    #[serde(default)]
    pub factual_reconcile_intent: String,
}

pub struct SelfRuntimeContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub outer_voice_store: &'a dyn OuterVoiceStore,
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
}

pub struct SelfRuntimeOutcome {
    pub decision: Option<SelfRuntimeDecision>,
    pub world_sense_result: Result<WorldSenseRefreshOutcome>,
    pub autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    pub outer_voice_result: Result<OuterVoiceRefreshOutcome>,
    pub inner_life_result: Result<InnerLifeRefreshOutcome>,
    pub private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    pub self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    pub private_garden_result: Result<PrivateGardenGovernanceOutcome>,
}

struct LoadedSelfRuntimeState {
    summary_text: Option<String>,
    execution_state: Option<crate::memory::ExecutionState>,
    self_model: Option<crate::memory::SelfModel>,
    private_docs: Option<crate::memory::PrivateDocWorkspace>,
    private_garden_docs: Vec<crate::memory::PrivateGardenDocRecord>,
    inner_life: Option<crate::memory::InnerLife>,
    self_continuity: Option<crate::memory::SelfContinuity>,
    world_sense: Option<crate::memory::WorldSense>,
    autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    outer_voice: Option<crate::memory::OuterVoice>,
    mental_privacy_state: Option<crate::memory::MentalPrivacyState>,
    prior_user_channel: String,
    world_snapshot: crate::memory::WorldSnapshot,
    recent: Vec<crate::memory::SessionMessage>,
}

struct SelfRuntimeRefreshPrelude {
    world_sense_result: Result<WorldSenseRefreshOutcome>,
    autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    outer_voice_result: Result<OuterVoiceRefreshOutcome>,
    refreshed_world_sense: Option<crate::memory::WorldSense>,
    refreshed_autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    runtime_self_state: SelfState,
}

struct SelfRuntimeActionResults {
    decision: Option<SelfRuntimeDecision>,
    inner_life_result: Result<InnerLifeRefreshOutcome>,
    private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    private_garden_result: Result<PrivateGardenGovernanceOutcome>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GovernedRuntimeLayer {
    PrivateDocs,
    PrivateGarden,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfRuntimeGovernanceAction {
    #[default]
    Hold,
    Rewrite,
    Compress,
    Cleanup,
}

impl SelfRuntimeGovernanceAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hold => "hold",
            Self::Rewrite => "rewrite",
            Self::Compress => "compress",
            Self::Cleanup => "cleanup",
        }
    }

    fn from_text(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "rewrite" => Self::Rewrite,
            "compress" => Self::Compress,
            "cleanup" | "clean_up" | "clean-up" | "prune" => Self::Cleanup,
            _ => Self::Hold,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelfRuntimeBoundaryReason {
    DailyBoundary,
    AutonomyShift,
    IdleSettlement,
    ChannelHandoff,
}

impl SelfRuntimeBoundaryReason {
    fn label(self) -> &'static str {
        match self {
            Self::DailyBoundary => "daily_boundary",
            Self::AutonomyShift => "autonomy_shift",
            Self::IdleSettlement => "idle_settlement",
            Self::ChannelHandoff => "channel_handoff",
        }
    }

    fn human_label(self) -> &'static str {
        match self {
            Self::DailyBoundary => "daily boundary",
            Self::AutonomyShift => "autonomy strategy shift",
            Self::IdleSettlement => "idle settlement",
            Self::ChannelHandoff => "channel handoff",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SelfRuntimeBoundarySignal {
    reasons: Vec<SelfRuntimeBoundaryReason>,
}

impl SelfRuntimeBoundarySignal {
    fn is_active(&self) -> bool {
        !self.reasons.is_empty()
    }

    fn summary(&self) -> String {
        self.reasons
            .iter()
            .map(|reason| reason.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn human_summary(&self) -> String {
        self.reasons
            .iter()
            .map(|reason| reason.human_label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn self_runtime_ingress(trigger: SelfRuntimeTrigger) -> IngressKind {
    match trigger {
        SelfRuntimeTrigger::PostReply => IngressKind::User,
        SelfRuntimeTrigger::IdleTick => IngressKind::System,
    }
}

fn load_self_runtime_state(
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
) -> LoadedSelfRuntimeState {
    let summary_text = ctx
        .session_summary_store
        .get_with_count(chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(chat_id).ok().flatten();
    let self_model = ctx.self_model_store.get(chat_id).ok().flatten();
    let private_docs = ctx.private_doc_store.get(chat_id).ok().flatten();
    let private_garden_docs = ctx
        .private_garden_store
        .list(chat_id, usize::MAX)
        .unwrap_or_default();
    let inner_life = ctx.inner_life_store.get(chat_id).ok().flatten();
    let self_continuity = ctx.self_continuity_store.get(chat_id).ok().flatten();
    let prior_user_channel = self_continuity
        .as_ref()
        .map(|continuity| continuity.last_user_channel.trim().to_string())
        .unwrap_or_default();
    let world_sense = ctx.world_sense_store.get(chat_id).ok().flatten();
    let autonomy_strategy = ctx.autonomy_strategy_store.get(chat_id).ok().flatten();
    let outer_voice = ctx.outer_voice_store.get(chat_id).ok().flatten();
    let mental_privacy_state = ctx.mental_privacy_store.get(chat_id).ok().flatten();
    let self_continuity = if payload.trigger == SelfRuntimeTrigger::PostReply {
        let mut continuity = self_continuity.unwrap_or_default();
        continuity.last_user_turn_at = payload.now_secs;
        continuity.last_user_channel = payload.source_channel.trim().to_string();
        Some(continuity)
    } else {
        self_continuity
    };
    let world_snapshot = build_world_snapshot(WorldSnapshotContext {
        chat_id,
        source_channel: &payload.source_channel,
        now_secs: payload.now_secs,
        self_continuity: self_continuity.as_ref(),
        remind_store: ctx.remind_store,
        task_store: ctx.task_store,
    });
    let recent = ctx
        .session_store
        .load_recent(
            chat_id,
            memory_policy(profile)
                .self_runtime
                .recent_message_count
                .max(memory_policy(profile).world_sense.recent_message_count)
                .max(
                    memory_policy(profile)
                        .autonomy_strategy
                        .recent_message_count,
                )
                .max(memory_policy(profile).inner_life.recent_message_count)
                .max(memory_policy(profile).self_continuity.recent_message_count)
                .max(memory_policy(profile).outer_voice.recent_message_count)
                .max(
                    memory_policy(profile)
                        .private_garden_governance
                        .recent_message_count,
                ),
        )
        .unwrap_or_default();
    LoadedSelfRuntimeState {
        summary_text,
        execution_state,
        self_model,
        private_docs,
        private_garden_docs,
        inner_life,
        self_continuity,
        world_sense,
        autonomy_strategy,
        outer_voice,
        mental_privacy_state,
        prior_user_channel,
        world_snapshot,
        recent,
    }
}

fn refresh_world_and_autonomy(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
    state: &LoadedSelfRuntimeState,
) -> SelfRuntimeRefreshPrelude {
    let ingress = self_runtime_ingress(payload.trigger);
    let world_policy = memory_policy(profile).world_sense;
    let world_snapshot_changed = state.world_sense.as_ref().is_some_and(|existing| {
        existing.source_fingerprint
            != crate::memory::world_snapshot_fingerprint(&state.world_snapshot)
    });
    let world_sense_should_refresh = state.world_sense.is_none()
        || world_snapshot_changed
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && world_policy.should_refresh(
                WorldSenseRefreshInput {
                    chat_id,
                    ingress,
                    channel: &payload.source_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                state.world_sense.is_some(),
            ))
        || state.world_sense.as_ref().is_some_and(|world_sense| {
            payload.now_secs.saturating_sub(world_sense.updated_at)
                >= world_policy.refresh_interval_secs
        });
    let world_sense_result = run_world_sense_refresh_with_state(
        http,
        llm,
        WorldSenseRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            execution_state_store: ctx.execution_state_store,
            self_continuity_store: ctx.self_continuity_store,
            autonomy_strategy_store: ctx.autonomy_strategy_store,
            world_sense_store: ctx.world_sense_store,
            remind_store: ctx.remind_store,
            task_store: ctx.task_store,
        },
        WorldSenseRefreshInput {
            chat_id,
            ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        state.world_sense.clone(),
        &state.world_snapshot,
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_continuity.as_ref(),
        state.autonomy_strategy.as_ref(),
        Some(world_sense_should_refresh),
        Some(state.recent.as_slice()),
    );
    let refreshed_world_sense = ctx
        .world_sense_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(state.world_sense.clone());
    let autonomy_policy = memory_policy(profile).autonomy_strategy;
    let autonomy_strategy_should_refresh = state.autonomy_strategy.is_none()
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && autonomy_policy.should_refresh(
                AutonomyStrategyRefreshInput {
                    chat_id,
                    ingress,
                    channel: &payload.source_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                state.autonomy_strategy.is_some(),
            ))
        || state.autonomy_strategy.as_ref().is_some_and(|strategy| {
            payload.now_secs.saturating_sub(strategy.updated_at)
                >= autonomy_policy.refresh_interval_secs
        });
    let autonomy_strategy_result = run_autonomy_strategy_refresh_with_state(
        http,
        llm,
        AutonomyStrategyRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            execution_state_store: ctx.execution_state_store,
            long_term_memory_store: ctx.long_term_memory_store,
            self_model_store: ctx.self_model_store,
            inner_life_store: ctx.inner_life_store,
            self_continuity_store: ctx.self_continuity_store,
            private_doc_store: ctx.private_doc_store,
            private_garden_store: ctx.private_garden_store,
            world_sense_store: ctx.world_sense_store,
            autonomy_strategy_store: ctx.autonomy_strategy_store,
        },
        AutonomyStrategyRefreshInput {
            chat_id,
            ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        state.autonomy_strategy.clone(),
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_model.as_ref(),
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        refreshed_world_sense.as_ref(),
        Some(&state.world_snapshot),
        Some(autonomy_strategy_should_refresh),
        Some(state.recent.as_slice()),
    );
    let refreshed_autonomy_strategy = ctx
        .autonomy_strategy_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(state.autonomy_strategy.clone());
    let outer_voice_policy = memory_policy(profile).outer_voice;
    let outer_voice_should_refresh = state.outer_voice.is_none()
        || world_snapshot_changed
        || matches!(
            &world_sense_result,
            Ok(WorldSenseRefreshOutcome::Updated | WorldSenseRefreshOutcome::Cleared)
        )
        || matches!(
            &autonomy_strategy_result,
            Ok(AutonomyStrategyRefreshOutcome::Updated | AutonomyStrategyRefreshOutcome::Cleared)
        )
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && outer_voice_policy.should_refresh(
                OuterVoiceRefreshInput {
                    chat_id,
                    ingress,
                    channel: &payload.source_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                state.outer_voice.is_some(),
            ))
        || state.outer_voice.as_ref().is_some_and(|outer_voice| {
            payload.now_secs.saturating_sub(outer_voice.updated_at)
                >= outer_voice_policy.refresh_interval_secs
        });
    let outer_voice_result = run_outer_voice_refresh_with_state(
        http,
        llm,
        OuterVoiceRefreshContext {
            outer_voice_store: ctx.outer_voice_store,
        },
        OuterVoiceRefreshInput {
            chat_id,
            ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        state.outer_voice.clone(),
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_model.as_ref(),
        &state.world_snapshot,
        refreshed_world_sense.as_ref(),
        refreshed_autonomy_strategy.as_ref(),
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        state.mental_privacy_state.as_ref(),
        Some(outer_voice_should_refresh),
        Some(state.recent.as_slice()),
    );
    let runtime_self_state = build_self_state(
        state.self_model.as_ref(),
        state.private_docs.as_ref(),
        refreshed_autonomy_strategy.as_ref(),
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        &state.private_garden_docs,
        payload.now_secs,
        profile,
    );
    SelfRuntimeRefreshPrelude {
        world_sense_result,
        autonomy_strategy_result,
        outer_voice_result,
        refreshed_world_sense,
        refreshed_autonomy_strategy,
        runtime_self_state,
    }
}

fn unix_day_bucket(now_secs: u64) -> u64 {
    now_secs / 86_400
}

fn detect_boundary_flush_signal(
    payload: &SelfRuntimeJobPayload,
    state: &LoadedSelfRuntimeState,
    prelude: &SelfRuntimeRefreshPrelude,
) -> SelfRuntimeBoundarySignal {
    let mut reasons = Vec::with_capacity(4);
    let current_channel = payload.source_channel.trim();
    if payload.trigger == SelfRuntimeTrigger::PostReply
        && !current_channel.is_empty()
        && !state.prior_user_channel.trim().is_empty()
        && state.prior_user_channel.trim() != current_channel
    {
        reasons.push(SelfRuntimeBoundaryReason::ChannelHandoff);
    }
    if payload.trigger == SelfRuntimeTrigger::IdleTick {
        let last_autonomy_run_at = state
            .self_continuity
            .as_ref()
            .map(|continuity| continuity.last_autonomy_run_at)
            .unwrap_or(0);
        if last_autonomy_run_at > 0
            && unix_day_bucket(last_autonomy_run_at) != unix_day_bucket(payload.now_secs)
        {
            reasons.push(SelfRuntimeBoundaryReason::DailyBoundary);
        }
        let last_user_turn_at = state
            .self_continuity
            .as_ref()
            .map(|continuity| continuity.last_user_turn_at)
            .unwrap_or(0);
        if last_user_turn_at > 0 && payload.now_secs.saturating_sub(last_user_turn_at) >= 30 * 60 {
            reasons.push(SelfRuntimeBoundaryReason::IdleSettlement);
        }
    }
    let previous_mode = state
        .autonomy_strategy
        .as_ref()
        .map(|strategy| strategy.current_mode.trim())
        .unwrap_or_default();
    let current_mode = prelude
        .refreshed_autonomy_strategy
        .as_ref()
        .map(|strategy| strategy.current_mode.trim())
        .unwrap_or_default();
    if !current_mode.is_empty() && !previous_mode.is_empty() && current_mode != previous_mode {
        reasons.push(SelfRuntimeBoundaryReason::AutonomyShift);
    }
    SelfRuntimeBoundarySignal { reasons }
}

fn execute_self_runtime_actions(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
    state: &LoadedSelfRuntimeState,
    prelude: &SelfRuntimeRefreshPrelude,
) -> SelfRuntimeActionResults {
    let boundary_signal = detect_boundary_flush_signal(payload, state, prelude);
    let query_hint = if !payload.user_content.trim().is_empty() {
        payload.user_content.as_str()
    } else {
        payload.reply_content.as_str()
    };
    let governance = run_memory_governance_kernel(
        MemoryGovernanceContext {
            session_store: ctx.session_store,
            long_term_memory_store: ctx.long_term_memory_store,
            memory_store: ctx.memory_store,
            turn_ledger_store: ctx.turn_ledger_store,
        },
        MemoryGovernanceInput {
            chat_id,
            query_hint,
            summary_text: state.summary_text.as_deref(),
            recent: state.recent.as_slice(),
            max_len: memory_policy(profile).self_runtime.grounding_max_len,
            profile,
            external_content_used: payload.external_content_used,
        },
    );
    let factual_snapshot = governance.factual_plane_snapshot;
    let decision = match decide_self_runtime(
        http,
        llm,
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        chat_id,
        payload,
        state.summary_text.as_deref(),
        state.execution_state.as_ref(),
        state.self_model.as_ref(),
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        prelude.refreshed_world_sense.as_ref(),
        &state.world_snapshot,
        prelude.refreshed_autonomy_strategy.as_ref(),
        profile,
        state.recent.as_slice(),
        &factual_snapshot,
        &boundary_signal,
    ) {
        Ok(decision) => Some(normalize_self_runtime_decision(
            decision,
            payload.trigger,
            prelude.refreshed_autonomy_strategy.as_ref(),
            &prelude.runtime_self_state,
            state.private_docs.is_some(),
            !state.private_garden_docs.is_empty(),
            &factual_snapshot,
            &boundary_signal,
        )),
        Err(error) => {
            return SelfRuntimeActionResults {
                decision: None,
                inner_life_result: Err(error),
                private_doc_result: Ok(PrivateDocWorkspaceRefreshOutcome::Skipped),
                self_continuity_result: Ok(SelfContinuityRefreshOutcome::Skipped),
                private_garden_result: Ok(PrivateGardenGovernanceOutcome::Skipped),
            };
        }
    };
    let decision_ref = decision.as_ref();
    let inner_life_result = if decision_ref.is_some_and(|d| d.refresh_inner_life) {
        run_inner_life_refresh_with_state(
            http,
            llm,
            InnerLifeRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                self_continuity_store: ctx.self_continuity_store,
                inner_life_store: ctx.inner_life_store,
            },
            InnerLifeRefreshInput {
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.inner_life.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            state.self_model.as_ref(),
            state.private_docs.as_ref(),
            state.self_continuity.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(InnerLifeRefreshOutcome::Skipped)
    };
    let refreshed_inner_life = ctx
        .inner_life_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(state.inner_life.clone());
    let private_doc_result = if decision_ref.is_some_and(|d| d.refresh_private_docs) {
        run_private_doc_workspace_refresh_with_state(
            http,
            llm,
            PrivateDocWorkspaceRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
            },
            PrivateDocWorkspaceRefreshInput {
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.private_docs.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            state.self_model.as_ref(),
            &state.private_garden_docs,
            decision_ref.and_then(|d| {
                (!d.private_docs_intent.trim().is_empty()).then_some(d.private_docs_intent.as_str())
            }),
            &[],
            prelude.refreshed_autonomy_strategy.as_ref(),
            state.self_continuity.as_ref(),
            refreshed_inner_life.as_ref(),
            prelude.refreshed_world_sense.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(PrivateDocWorkspaceRefreshOutcome::Skipped)
    };
    let refreshed_private_docs = ctx
        .private_doc_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(state.private_docs.clone());
    let self_continuity_result = if decision_ref.is_some_and(|d| d.refresh_self_continuity) {
        run_self_continuity_refresh_with_state(
            http,
            llm,
            SelfContinuityRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                inner_life_store: ctx.inner_life_store,
                self_continuity_store: ctx.self_continuity_store,
            },
            SelfContinuityRefreshInput {
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.self_continuity.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            state.self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            refreshed_inner_life.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(SelfContinuityRefreshOutcome::Skipped)
    };
    let private_garden_result = if decision_ref.is_some_and(|d| d.refresh_private_garden) {
        run_private_garden_governance_with_state(
            http,
            llm,
            PrivateGardenGovernanceContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                self_model_store: ctx.self_model_store,
                private_doc_store: ctx.private_doc_store,
                private_garden_store: ctx.private_garden_store,
            },
            PrivateGardenGovernanceInput {
                chat_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            state.self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            decision_ref.and_then(|d| {
                (!d.private_garden_intent.trim().is_empty())
                    .then_some(d.private_garden_intent.as_str())
            }),
            &[],
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(PrivateGardenGovernanceOutcome::Skipped)
    };
    SelfRuntimeActionResults {
        decision,
        inner_life_result,
        private_doc_result,
        self_continuity_result,
        private_garden_result,
    }
}

pub fn enqueue_self_runtime_post_reply(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    source_channel: &str,
    user_content: &str,
    reply_content: &str,
    tool_calls: u32,
    external_content_used: bool,
) -> bool {
    enqueue_self_runtime_job(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::PostReply,
            source_channel: source_channel.to_string(),
            user_content: truncate_content_to_max(user_content, 512).into_owned(),
            reply_content: truncate_content_to_max(reply_content, 768).into_owned(),
            tool_calls,
            external_content_used,
            now_secs: current_unix_secs(),
        },
    )
}

pub fn enqueue_self_runtime_idle_tick(system_inbound_tx: &SystemInboundTx, chat_id: &str) -> bool {
    enqueue_self_runtime_job(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: "self_runtime_idle".to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: current_unix_secs(),
        },
    )
}

fn enqueue_self_runtime_job(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    payload: SelfRuntimeJobPayload,
) -> bool {
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            log::warn!(
                "[self_runtime] serialize job failed chat_id={}: {}",
                chat_id,
                error
            );
            return false;
        }
    };
    let job = match PcMsg::new_system(SELF_RUNTIME_CHANNEL, chat_id, body) {
        Ok(job) => job,
        Err(error) => {
            log::warn!(
                "[self_runtime] build job failed chat_id={}: {}",
                chat_id,
                error
            );
            return false;
        }
    };
    match system_inbound_tx.try_send(job) {
        Ok(()) => true,
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
            log::debug!(
                "[self_runtime] skip enqueue because system queue is full chat_id={}",
                chat_id
            );
            false
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::warn!("[self_runtime] enqueue failed: system queue disconnected");
            false
        }
    }
}

pub fn self_runtime_tick(
    system_inbound_tx: &SystemInboundTx,
    session_store: &dyn SessionStore,
    self_continuity_store: &dyn SelfContinuityStore,
    autonomy_strategy_store: &dyn AutonomyStrategyStore,
    profile: MemoryProfile,
    now_secs: u64,
) {
    let policy = memory_policy(profile).self_runtime;
    let capability = memory_capability_profile(profile);
    let chat_ids = match session_store.list_chat_ids() {
        Ok(chat_ids) => chat_ids,
        Err(error) => {
            log::warn!("[self_runtime] failed to list chat ids: {}", error);
            return;
        }
    };
    let mut enqueued = 0usize;
    for chat_id in chat_ids {
        let max_jobs_per_tick = policy
            .max_jobs_per_tick
            .min(capability.runtime_max_jobs_per_tick);
        if enqueued >= max_jobs_per_tick {
            break;
        }
        let continuity = match self_continuity_store.get(&chat_id) {
            Ok(value) => value,
            Err(error) => {
                log::warn!(
                    "[self_runtime] failed to read continuity for {}: {}",
                    chat_id,
                    error
                );
                continue;
            }
        };
        let active = continuity
            .as_ref()
            .map(|c| c.last_user_turn_at)
            .unwrap_or(0);
        if active > 0 && now_secs.saturating_sub(active) > policy.active_chat_window_secs {
            continue;
        }
        let last_autonomy = continuity
            .as_ref()
            .map(|c| c.last_autonomy_run_at)
            .unwrap_or(0);
        let strategy = match autonomy_strategy_store.get(&chat_id) {
            Ok(value) => value,
            Err(error) => {
                log::warn!(
                    "[self_runtime] failed to read autonomy strategy for {}: {}",
                    chat_id,
                    error
                );
                None
            }
        };
        let idle_interval_secs = match autonomy_idle_interval_secs(strategy.as_ref(), profile) {
            Some(interval) => interval,
            None if strategy.is_some() => continue,
            None => policy.idle_tick_interval_secs,
        };
        if last_autonomy > 0 && now_secs.saturating_sub(last_autonomy) < idle_interval_secs {
            continue;
        }
        if enqueue_self_runtime_idle_tick(system_inbound_tx, &chat_id) {
            enqueued += 1;
        }
    }
}

pub fn run_self_runtime(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
) -> SelfRuntimeOutcome {
    let state = load_self_runtime_state(&ctx, chat_id, payload, profile);
    let prelude = refresh_world_and_autonomy(http, llm, &ctx, chat_id, payload, profile, &state);
    let action_results =
        execute_self_runtime_actions(http, llm, &ctx, chat_id, payload, profile, &state, &prelude);

    let _ = touch_self_continuity_runtime(
        ctx.self_continuity_store,
        chat_id,
        payload.now_secs,
        payload.trigger == SelfRuntimeTrigger::PostReply,
        true,
        Some(payload.source_channel.as_str()),
    );
    if matches!(payload.trigger, SelfRuntimeTrigger::IdleTick) {
        let _ = run_memory_hygiene_jobs(
            MemoryHygieneContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                memory_store: ctx.memory_store,
                turn_ledger_store: ctx.turn_ledger_store,
                long_term_memory_store: ctx.long_term_memory_store,
                skill_storage: ctx.skill_storage,
            },
            chat_id,
            profile,
            payload.now_secs,
        );
    }

    SelfRuntimeOutcome {
        decision: action_results.decision,
        world_sense_result: prelude.world_sense_result,
        autonomy_strategy_result: prelude.autonomy_strategy_result,
        outer_voice_result: prelude.outer_voice_result,
        inner_life_result: action_results.inner_life_result,
        private_doc_result: action_results.private_doc_result,
        self_continuity_result: action_results.self_continuity_result,
        private_garden_result: action_results.private_garden_result,
    }
}

fn normalize_self_runtime_decision(
    mut decision: SelfRuntimeDecision,
    trigger: SelfRuntimeTrigger,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    self_state: &SelfState,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> SelfRuntimeDecision {
    let Some(strategy) = autonomy_strategy else {
        if !decision.refresh_private_docs {
            decision.private_docs_intent.clear();
        }
        if !decision.refresh_private_garden {
            decision.private_garden_intent.clear();
        }
        normalize_boundary_and_factual_decisions(
            &mut decision,
            self_state,
            factual_snapshot,
            boundary_signal,
            has_private_docs,
            has_private_garden_docs,
        );
        return decision;
    };

    apply_runtime_governance_tendency(
        &mut decision.refresh_private_docs,
        &mut decision.private_docs_intent,
        &mut decision.private_docs_action,
        strategy.private_docs_tendency,
        GovernedRuntimeLayer::PrivateDocs,
        trigger,
        self_state,
        has_private_docs,
    );
    apply_runtime_governance_tendency(
        &mut decision.refresh_private_garden,
        &mut decision.private_garden_intent,
        &mut decision.private_garden_action,
        strategy.private_garden_tendency,
        GovernedRuntimeLayer::PrivateGarden,
        trigger,
        self_state,
        has_private_garden_docs,
    );
    normalize_boundary_and_factual_decisions(
        &mut decision,
        self_state,
        factual_snapshot,
        boundary_signal,
        has_private_docs,
        has_private_garden_docs,
    );
    decision
}

fn normalize_boundary_and_factual_decisions(
    decision: &mut SelfRuntimeDecision,
    self_state: &SelfState,
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
    has_private_docs: bool,
    has_private_garden_docs: bool,
) {
    if boundary_signal.is_active() {
        decision.boundary_flush = true;
        if decision.boundary_flush_reason.trim().is_empty() {
            decision.boundary_flush_reason = boundary_signal.summary();
        }
        decision.refresh_self_continuity = true;
        if decision.self_continuity_intent.trim().is_empty() {
            decision.self_continuity_intent =
                default_boundary_self_continuity_intent(boundary_signal);
        }
        if has_private_docs && !decision.refresh_private_docs {
            decision.refresh_private_docs = true;
            if matches!(
                decision.private_docs_action,
                SelfRuntimeGovernanceAction::Hold
            ) {
                decision.private_docs_action = default_boundary_governance_action(self_state, true);
            }
            if decision.private_docs_intent.trim().is_empty() {
                decision.private_docs_intent = default_boundary_private_intent(
                    decision.private_docs_action,
                    GovernedRuntimeLayer::PrivateDocs,
                    boundary_signal,
                );
            }
        }
        if has_private_garden_docs && !decision.refresh_private_garden {
            decision.refresh_private_garden = true;
            if matches!(
                decision.private_garden_action,
                SelfRuntimeGovernanceAction::Hold
            ) {
                decision.private_garden_action =
                    default_boundary_governance_action(self_state, false);
            }
            if decision.private_garden_intent.trim().is_empty() {
                decision.private_garden_intent = default_boundary_private_intent(
                    decision.private_garden_action,
                    GovernedRuntimeLayer::PrivateGarden,
                    boundary_signal,
                );
            }
        }
    }

    if let Some(action) = factual_snapshot.strongest_refresh_action() {
        if matches!(
            decision.factual_reconcile_action,
            SharedFactualReconcileAction::Hold
        ) {
            decision.factual_reconcile_action = action;
        }
        if decision.factual_reconcile_intent.trim().is_empty() {
            decision.factual_reconcile_intent =
                default_factual_refresh_intent(action, factual_snapshot);
        }
        if matches!(
            action,
            SharedFactualReconcileAction::Correct
                | SharedFactualReconcileAction::Conflict
                | SharedFactualReconcileAction::Stale
        ) {
            decision.request_factual_refresh = true;
        }
    } else if !decision.request_factual_refresh {
        decision.factual_reconcile_intent.clear();
    }
}

fn apply_runtime_governance_tendency(
    refresh: &mut bool,
    intent: &mut String,
    action: &mut SelfRuntimeGovernanceAction,
    tendency: AutonomyGovernanceTendency,
    layer: GovernedRuntimeLayer,
    trigger: SelfRuntimeTrigger,
    self_state: &SelfState,
    has_material: bool,
) {
    if !*refresh
        && should_force_runtime_governance_refresh(
            tendency,
            layer,
            trigger,
            self_state,
            has_material,
        )
    {
        *refresh = true;
    }
    if matches!(*action, SelfRuntimeGovernanceAction::Hold) {
        *action = runtime_action_from_tendency(tendency);
    }
    if *refresh && intent.trim().is_empty() {
        *intent = default_runtime_governance_intent(*action, layer, self_state);
    }
    if !*refresh {
        intent.clear();
        *action = SelfRuntimeGovernanceAction::Hold;
    }
}

fn runtime_action_from_tendency(
    tendency: AutonomyGovernanceTendency,
) -> SelfRuntimeGovernanceAction {
    match tendency {
        AutonomyGovernanceTendency::Retain => SelfRuntimeGovernanceAction::Hold,
        AutonomyGovernanceTendency::Rewrite => SelfRuntimeGovernanceAction::Rewrite,
        AutonomyGovernanceTendency::Compress => SelfRuntimeGovernanceAction::Compress,
        AutonomyGovernanceTendency::Cleanup => SelfRuntimeGovernanceAction::Cleanup,
    }
}

fn should_force_runtime_governance_refresh(
    tendency: AutonomyGovernanceTendency,
    layer: GovernedRuntimeLayer,
    trigger: SelfRuntimeTrigger,
    self_state: &SelfState,
    has_material: bool,
) -> bool {
    if trigger != SelfRuntimeTrigger::IdleTick || !has_material {
        return false;
    }
    let kernel_pressure = matches!(
        self_state.memory_space.pressure,
        SelfMemorySpacePressure::Cautious | SelfMemorySpacePressure::Tight
    ) || matches!(
        self_state.memory_space.bottleneck,
        SelfMemorySpaceBottleneck::Kernel
    );
    let garden_pressure = matches!(
        self_state.memory_space.pressure,
        SelfMemorySpacePressure::Cautious | SelfMemorySpacePressure::Tight
    ) || matches!(
        self_state.memory_space.bottleneck,
        SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes
    );
    match (layer, tendency) {
        (_, AutonomyGovernanceTendency::Retain) => false,
        (GovernedRuntimeLayer::PrivateDocs, AutonomyGovernanceTendency::Rewrite) => true,
        (GovernedRuntimeLayer::PrivateDocs, AutonomyGovernanceTendency::Compress) => {
            kernel_pressure
        }
        (GovernedRuntimeLayer::PrivateDocs, AutonomyGovernanceTendency::Cleanup) => matches!(
            self_state.memory_space.pressure,
            SelfMemorySpacePressure::Tight
        ),
        (GovernedRuntimeLayer::PrivateGarden, AutonomyGovernanceTendency::Rewrite) => true,
        (GovernedRuntimeLayer::PrivateGarden, AutonomyGovernanceTendency::Compress) => {
            garden_pressure
        }
        (GovernedRuntimeLayer::PrivateGarden, AutonomyGovernanceTendency::Cleanup) => {
            garden_pressure
        }
    }
}

fn default_runtime_governance_intent(
    action: SelfRuntimeGovernanceAction,
    layer: GovernedRuntimeLayer,
    self_state: &SelfState,
) -> String {
    let pressure_focus = match self_state.memory_space.bottleneck {
        SelfMemorySpaceBottleneck::Kernel => "降低内核空间中的重复与漂移",
        SelfMemorySpaceBottleneck::GardenDocs => "减少 garden 文档数量上的拥挤",
        SelfMemorySpaceBottleneck::GardenBytes => "压低 garden 总体体积",
        SelfMemorySpaceBottleneck::Balanced => "保持整体内在空间清晰",
    };
    match (layer, action) {
        (_, SelfRuntimeGovernanceAction::Hold) => String::new(),
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Rewrite) => {
            "重写 governed docs，只保留仍然承重的内在线索".to_string()
        }
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Compress) => {
            let mut out = String::with_capacity(24 + pressure_focus.len());
            out.push_str("压缩 governed docs，");
            out.push_str(pressure_focus);
            out
        }
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Cleanup) => {
            "清理低价值 governed docs 字段，只留下仍然有效的部分".to_string()
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Rewrite) => {
            "重写并重组 private garden 中仍然活跃的工作文档".to_string()
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Compress) => {
            let mut out = String::with_capacity(24 + pressure_focus.len());
            out.push_str("压缩 private garden，");
            out.push_str(pressure_focus);
            out
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Cleanup) => {
            "清理陈旧或重复的 private garden 草稿与路径".to_string()
        }
    }
}

fn default_boundary_governance_action(
    self_state: &SelfState,
    private_docs: bool,
) -> SelfRuntimeGovernanceAction {
    match self_state.memory_space.pressure {
        SelfMemorySpacePressure::Tight => SelfRuntimeGovernanceAction::Cleanup,
        SelfMemorySpacePressure::Cautious => SelfRuntimeGovernanceAction::Compress,
        SelfMemorySpacePressure::Normal => {
            if private_docs {
                SelfRuntimeGovernanceAction::Rewrite
            } else {
                SelfRuntimeGovernanceAction::Compress
            }
        }
    }
}

fn default_boundary_self_continuity_intent(boundary_signal: &SelfRuntimeBoundarySignal) -> String {
    format!(
        "在 {} 收束当前阶段，确保下一次苏醒时连续性不撕裂",
        boundary_signal.human_summary()
    )
}

fn default_boundary_private_intent(
    action: SelfRuntimeGovernanceAction,
    layer: GovernedRuntimeLayer,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> String {
    let layer_name = match layer {
        GovernedRuntimeLayer::PrivateDocs => "governed docs",
        GovernedRuntimeLayer::PrivateGarden => "private garden",
    };
    format!(
        "在 {} 时对 {} 执行 {} 收口",
        boundary_signal.human_summary(),
        layer_name,
        action.label()
    )
}

fn default_factual_refresh_intent(
    action: SharedFactualReconcileAction,
    snapshot: &SharedFactualPlaneSnapshot,
) -> String {
    match snapshot.refresh_summary() {
        Some(summary) => format!(
            "shared factual plane needs {} review: {}",
            action.label(),
            summary
        ),
        None => format!("shared factual plane needs {} review", action.label()),
    }
}

#[allow(clippy::too_many_arguments)]
fn decide_self_runtime(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    session_store: &dyn SessionStore,
    _long_term_memory_store: &dyn LongTermMemoryStore,
    memory_store: &dyn MemoryStore,
    turn_ledger_store: &dyn TurnLedgerStore,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    summary_text: Option<&str>,
    execution_state: Option<&crate::memory::ExecutionState>,
    self_model: Option<&crate::memory::SelfModel>,
    private_docs: Option<&crate::memory::PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    inner_life: Option<&crate::memory::InnerLife>,
    self_continuity: Option<&crate::memory::SelfContinuity>,
    world_sense: Option<&crate::memory::WorldSense>,
    world_snapshot: &crate::memory::WorldSnapshot,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    profile: MemoryProfile,
    recent: &[crate::memory::SessionMessage],
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> Result<SelfRuntimeDecision> {
    let policy = memory_policy(profile).self_runtime;
    let query_hint = if !payload.user_content.trim().is_empty() {
        payload.user_content.trim()
    } else {
        payload.reply_content.trim()
    };
    let mut input = String::with_capacity(2048);
    let _ = writeln!(input, "Trigger: {:?}", payload.trigger);
    if !payload.source_channel.trim().is_empty() {
        let _ = writeln!(input, "Source channel: {}", payload.source_channel.trim());
    }
    if !payload.user_content.trim().is_empty() {
        let _ = writeln!(
            input,
            "Latest user: {}",
            scrub_credentials(
                truncate_content_to_max(
                    payload.user_content.trim(),
                    policy.transcript_preview_chars
                )
                .as_ref()
            )
        );
    }
    if !payload.reply_content.trim().is_empty() {
        let _ = writeln!(
            input,
            "Latest reply: {}",
            scrub_credentials(
                truncate_content_to_max(
                    payload.reply_content.trim(),
                    policy.transcript_preview_chars
                )
                .as_ref()
            )
        );
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
    if let Some(block) = factual_snapshot.block.as_deref() {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = build_archive_evidence_block(
        session_store,
        memory_store,
        turn_ledger_store,
        chat_id,
        query_hint,
        policy.grounding_max_len,
        profile,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = render_world_snapshot_block(
        world_snapshot,
        memory_policy(profile).world_sense.snapshot_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(self_state_text) = render_self_state_block(
        &build_self_state(
            self_model,
            private_docs,
            autonomy_strategy,
            inner_life,
            self_continuity,
            private_garden_docs,
            payload.now_secs,
            profile,
        ),
        memory_policy(profile).self_state.render_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", self_state_text);
    }
    if let Some(block) = render_internal_memory_topology_block(
        self_model,
        private_docs,
        private_garden_docs,
        payload.now_secs,
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
    if let Some(block) = autonomy_strategy
        .and_then(|strategy| render_autonomy_strategy_block(strategy, policy.grounding_max_len))
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
        "self_runtime",
        "governing private inward writes while keeping objective facts in the shared plane",
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if boundary_signal.is_active() {
        let _ = writeln!(
            input,
            "\nBoundary flush signal: {}",
            boundary_signal.human_summary()
        );
    }
    if let Some(summary) = factual_snapshot.refresh_summary() {
        let _ = writeln!(input, "Shared factual reconcile summary: {}", summary);
    }
    if payload.external_content_used {
        let _ = writeln!(
            input,
            "Latest turn used external content/tools that may have changed what deserves inward organization."
        );
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
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: input,
    }];
    let response = llm.chat(
        http,
        SELF_RUNTIME_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(parse_self_runtime_decision(response.content.trim()))
}

fn parse_self_runtime_decision(raw: &str) -> SelfRuntimeDecision {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return SelfRuntimeDecision::default();
    };
    let Some(object) = value.as_object() else {
        return SelfRuntimeDecision::default();
    };
    SelfRuntimeDecision {
        refresh_inner_life: get_object_bool(object, "refresh_inner_life").unwrap_or(false),
        inner_life_intent: get_object_text(object, "inner_life_intent"),
        refresh_private_docs: get_object_bool(object, "refresh_private_docs").unwrap_or(false),
        private_docs_intent: get_object_text(object, "private_docs_intent"),
        private_docs_action: SelfRuntimeGovernanceAction::from_text(&get_object_text(
            object,
            "private_docs_action",
        )),
        refresh_self_continuity: get_object_bool(object, "refresh_self_continuity")
            .unwrap_or(false),
        self_continuity_intent: get_object_text(object, "self_continuity_intent"),
        refresh_private_garden: get_object_bool(object, "refresh_private_garden").unwrap_or(false),
        private_garden_intent: get_object_text(object, "private_garden_intent"),
        private_garden_action: SelfRuntimeGovernanceAction::from_text(&get_object_text(
            object,
            "private_garden_action",
        )),
        boundary_flush: get_object_bool(object, "boundary_flush").unwrap_or(false),
        boundary_flush_reason: get_object_text(object, "boundary_flush_reason"),
        request_factual_refresh: get_object_bool(object, "request_factual_refresh")
            .unwrap_or(false),
        factual_reconcile_action: parse_shared_factual_reconcile_action(&get_object_text(
            object,
            "factual_reconcile_action",
        )),
        factual_reconcile_intent: get_object_text(object, "factual_reconcile_intent"),
    }
}

fn parse_shared_factual_reconcile_action(value: &str) -> SharedFactualReconcileAction {
    match value.trim().to_ascii_lowercase().as_str() {
        "reinforce" => SharedFactualReconcileAction::Reinforce,
        "correct" => SharedFactualReconcileAction::Correct,
        "conflict" => SharedFactualReconcileAction::Conflict,
        "stale" => SharedFactualReconcileAction::Stale,
        _ => SharedFactualReconcileAction::Hold,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_self_state() -> SelfState {
        SelfState {
            memory_space: crate::memory::SelfMemorySpaceState {
                kernel_chars_used: 900,
                kernel_chars_limit: 1000,
                garden_docs_used: 6,
                garden_docs_limit: 8,
                garden_bytes_used: 800,
                garden_bytes_limit: 1024,
                bottleneck: SelfMemorySpaceBottleneck::Kernel,
                pressure: SelfMemorySpacePressure::Tight,
                governance_posture: crate::memory::SelfMemoryGovernancePosture::Prune,
                recent_activity: crate::memory::SelfMemorySpaceActivity::Growing,
                last_internal_change_at: 10,
            },
            inner_state: crate::memory::SelfInnerState {
                inner_life_chars_used: 80,
                inner_life_chars_limit: 240,
                self_continuity_chars_used: 80,
                self_continuity_chars_limit: 240,
            },
            autonomy: crate::memory::SelfAutonomyState {
                last_user_turn_at: 10,
                last_autonomy_run_at: 20,
                status: crate::memory::SelfAutonomyStatus::Active,
                health_score: 90,
                strategy_chars_used: 120,
                strategy_chars_limit: 512,
                strategy_mode: "consolidate".to_string(),
                strategy_focus: "trim drift".to_string(),
                self_model_tendency: AutonomyGovernanceTendency::Retain,
                private_docs_tendency: AutonomyGovernanceTendency::Compress,
                private_garden_tendency: AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
            },
        }
    }

    #[test]
    fn parse_self_runtime_decision_coerces_nested_fields() {
        let raw = json!({
            "refresh_inner_life": "true",
            "inner_life_intent": { "goal": "capture drift" },
            "refresh_private_docs": 1,
            "private_docs_intent": ["rewrite private notes"],
            "private_docs_action": "compress",
            "refresh_self_continuity": false,
            "self_continuity_intent": 0,
            "refresh_private_garden": { "enabled": true },
            "private_garden_intent": { "path": "journal/today.md" },
            "private_garden_action": "cleanup",
            "boundary_flush": true,
            "boundary_flush_reason": ["daily_boundary"],
            "request_factual_refresh": 1,
            "factual_reconcile_action": "conflict",
            "factual_reconcile_intent": { "why": "recent transcript diverges" }
        })
        .to_string();
        let parsed = parse_self_runtime_decision(&raw);
        assert!(parsed.refresh_inner_life);
        assert!(parsed.refresh_private_docs);
        assert!(parsed.refresh_private_garden);
        assert_eq!(
            parsed.private_docs_action,
            SelfRuntimeGovernanceAction::Compress
        );
        assert_eq!(
            parsed.private_garden_action,
            SelfRuntimeGovernanceAction::Cleanup
        );
        assert!(parsed.boundary_flush);
        assert!(parsed.request_factual_refresh);
        assert_eq!(
            parsed.factual_reconcile_action,
            SharedFactualReconcileAction::Conflict
        );
        assert!(parsed.inner_life_intent.contains("goal: capture drift"));
        assert_eq!(parsed.private_docs_intent, "rewrite private notes");
        assert_eq!(parsed.self_continuity_intent, "0");
        assert!(parsed
            .private_garden_intent
            .contains("path: journal/today.md"));
        assert!(parsed.boundary_flush_reason.contains("daily_boundary"));
        assert!(parsed
            .factual_reconcile_intent
            .contains("why: recent transcript diverges"));
    }

    #[test]
    fn idle_tick_tendency_can_force_private_docs_refresh_and_fill_intent() {
        let strategy = crate::memory::AutonomyStrategy {
            current_mode: "consolidate".to_string(),
            active_priorities: String::new(),
            write_policy: String::new(),
            next_focus: String::new(),
            cadence_reason: String::new(),
            self_model_tendency: AutonomyGovernanceTendency::Retain,
            private_docs_tendency: AutonomyGovernanceTendency::Compress,
            private_garden_tendency: AutonomyGovernanceTendency::Retain,
            idle_enabled: true,
            idle_interval_secs: 900,
            updated_at: 1,
        };
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            Some(&strategy),
            &sample_self_state(),
            true,
            false,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_docs);
        assert!(decision.private_docs_intent.contains("压缩 governed docs"));
        assert_eq!(
            decision.private_docs_action,
            SelfRuntimeGovernanceAction::Compress
        );
    }

    #[test]
    fn post_reply_tendency_does_not_force_refresh_but_can_fill_missing_intent() {
        let strategy = crate::memory::AutonomyStrategy {
            current_mode: "organize".to_string(),
            active_priorities: String::new(),
            write_policy: String::new(),
            next_focus: String::new(),
            cadence_reason: String::new(),
            self_model_tendency: AutonomyGovernanceTendency::Retain,
            private_docs_tendency: AutonomyGovernanceTendency::Retain,
            private_garden_tendency: AutonomyGovernanceTendency::Rewrite,
            idle_enabled: true,
            idle_interval_secs: 900,
            updated_at: 1,
        };
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision {
                refresh_private_garden: true,
                ..Default::default()
            },
            SelfRuntimeTrigger::PostReply,
            Some(&strategy),
            &sample_self_state(),
            false,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_garden);
        assert!(decision
            .private_garden_intent
            .contains("重写并重组 private garden"));
        assert!(!decision.refresh_private_docs);
        assert_eq!(
            decision.private_garden_action,
            SelfRuntimeGovernanceAction::Rewrite
        );
    }

    #[test]
    fn boundary_signal_forces_continuity_and_private_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal {
                reasons: vec![SelfRuntimeBoundaryReason::DailyBoundary],
            },
        );

        assert!(decision.boundary_flush);
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_private_docs);
        assert!(decision.refresh_private_garden);
    }
}
