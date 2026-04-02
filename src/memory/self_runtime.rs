//! 自治运行层：由 LLM 决定是否经营自己的内在空间。

use crate::bus::{IngressKind, PcMsg, SystemInboundTx};
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::task::TaskStore;
use crate::util::{current_unix_secs, scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    autonomy_idle_interval_secs, build_self_state, build_world_snapshot, memory_policy,
    render_autonomy_strategy_block, render_execution_state_block, render_inner_life_block,
    render_internal_memory_topology_block, render_private_doc_workspace_block,
    render_private_garden_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_world_sense_block, render_world_snapshot_block,
    run_autonomy_strategy_refresh_with_state, run_inner_life_refresh_with_state,
    run_private_doc_workspace_refresh_with_state, run_private_garden_governance_with_state,
    run_self_continuity_refresh_with_state, run_world_sense_refresh_with_state,
    touch_self_continuity_runtime, AutonomyStrategyRefreshContext, AutonomyStrategyRefreshInput,
    AutonomyStrategyRefreshOutcome, AutonomyStrategyStore, ExecutionStateStore,
    InnerLifeRefreshContext, InnerLifeRefreshInput, InnerLifeRefreshOutcome, InnerLifeStore,
    InternalMemoryLayerFocus, MemoryProfile, PrivateDocStore, PrivateDocWorkspaceRefreshContext,
    PrivateDocWorkspaceRefreshInput, PrivateDocWorkspaceRefreshOutcome,
    PrivateGardenGovernanceContext, PrivateGardenGovernanceInput, PrivateGardenGovernanceOutcome,
    PrivateGardenStore, RemindAtStore, SelfContinuityRefreshContext, SelfContinuityRefreshInput,
    SelfContinuityRefreshOutcome, SelfContinuityStore, SelfModelStore, SessionStore,
    SessionSummaryStore, WorldSenseRefreshContext, WorldSenseRefreshInput,
    WorldSenseRefreshOutcome, WorldSenseStore, WorldSnapshotContext,
};

pub const SELF_RUNTIME_SYSTEM_PROMPT: &str = "You govern the assistant's private inward space. Respect the current autonomy strategy unless the latest world state or self-state clearly requires a different emphasis. Return JSON only: one object with fields refresh_inner_life, inner_life_intent, refresh_private_docs, private_docs_intent, refresh_self_continuity, self_continuity_intent, refresh_private_garden, private_garden_intent. Use true only when that layer should change now. private_docs is the governed inner workspace; private_garden is the free-form private workspace. Use self-state capacity, world-sense, and current autonomy strategy to decide whether to write, compress, reorganize, or leave memory untouched. Keep intents short and concrete. Favor autonomy, but do not churn memory without gain.";
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
    pub refresh_self_continuity: bool,
    #[serde(default)]
    pub self_continuity_intent: String,
    #[serde(default)]
    pub refresh_private_garden: bool,
    #[serde(default)]
    pub private_garden_intent: String,
}

pub struct SelfRuntimeContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
}

pub struct SelfRuntimeOutcome {
    pub decision: Option<SelfRuntimeDecision>,
    pub world_sense_result: Result<WorldSenseRefreshOutcome>,
    pub autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    pub inner_life_result: Result<InnerLifeRefreshOutcome>,
    pub private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    pub self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    pub private_garden_result: Result<PrivateGardenGovernanceOutcome>,
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
    let chat_ids = match session_store.list_chat_ids() {
        Ok(chat_ids) => chat_ids,
        Err(error) => {
            log::warn!("[self_runtime] failed to list chat ids: {}", error);
            return;
        }
    };
    let mut enqueued = 0usize;
    for chat_id in chat_ids {
        if enqueued >= policy.max_jobs_per_tick {
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
    let world_sense = ctx.world_sense_store.get(chat_id).ok().flatten();
    let autonomy_strategy = ctx.autonomy_strategy_store.get(chat_id).ok().flatten();
    if payload.trigger == SelfRuntimeTrigger::PostReply {
        let _ = touch_self_continuity_runtime(
            ctx.self_continuity_store,
            chat_id,
            payload.now_secs,
            true,
            false,
        );
    }
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
                .max(
                    memory_policy(profile)
                        .private_garden_governance
                        .recent_message_count,
                ),
        )
        .unwrap_or_default();
    let world_policy = memory_policy(profile).world_sense;
    let world_snapshot_changed = world_sense.as_ref().is_some_and(|existing| {
        existing.source_fingerprint != crate::memory::world_snapshot_fingerprint(&world_snapshot)
    });
    let world_sense_should_refresh = world_sense.is_none()
        || world_snapshot_changed
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && world_policy.should_refresh(
                WorldSenseRefreshInput {
                    chat_id,
                    ingress: IngressKind::User,
                    channel: &payload.source_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                world_sense.is_some(),
            ))
        || world_sense.as_ref().is_some_and(|world_sense| {
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
            ingress: match payload.trigger {
                SelfRuntimeTrigger::PostReply => IngressKind::User,
                SelfRuntimeTrigger::IdleTick => IngressKind::System,
            },
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        world_sense.clone(),
        &world_snapshot,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_continuity.as_ref(),
        autonomy_strategy.as_ref(),
        Some(world_sense_should_refresh),
        Some(recent.as_slice()),
    );
    let refreshed_world_sense = ctx
        .world_sense_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(world_sense.clone());
    let autonomy_policy = memory_policy(profile).autonomy_strategy;
    let autonomy_strategy_should_refresh = autonomy_strategy.is_none()
        || (payload.trigger == SelfRuntimeTrigger::PostReply
            && autonomy_policy.should_refresh(
                AutonomyStrategyRefreshInput {
                    chat_id,
                    ingress: IngressKind::User,
                    channel: &payload.source_channel,
                    user_content: &payload.user_content,
                    reply_content: &payload.reply_content,
                    pressure: PressureLevel::Normal,
                    tool_calls: payload.tool_calls,
                    now_secs: payload.now_secs,
                },
                autonomy_strategy.is_some(),
            ))
        || autonomy_strategy.as_ref().is_some_and(|strategy| {
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
            ingress: match payload.trigger {
                SelfRuntimeTrigger::PostReply => IngressKind::User,
                SelfRuntimeTrigger::IdleTick => IngressKind::System,
            },
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: PressureLevel::Normal,
            tool_calls: payload.tool_calls,
            now_secs: payload.now_secs,
        },
        profile,
        autonomy_strategy.clone(),
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        inner_life.as_ref(),
        self_continuity.as_ref(),
        private_docs.as_ref(),
        &private_garden_docs,
        refreshed_world_sense.as_ref(),
        Some(&world_snapshot),
        Some(autonomy_strategy_should_refresh),
        Some(recent.as_slice()),
    );
    let refreshed_autonomy_strategy = ctx
        .autonomy_strategy_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(autonomy_strategy.clone());
    let decision = match decide_self_runtime(
        http,
        llm,
        payload,
        summary_text.as_deref(),
        execution_state.as_ref(),
        self_model.as_ref(),
        private_docs.as_ref(),
        &private_garden_docs,
        inner_life.as_ref(),
        self_continuity.as_ref(),
        refreshed_world_sense.as_ref(),
        &world_snapshot,
        refreshed_autonomy_strategy.as_ref(),
        profile,
        ctx.session_store,
        chat_id,
    ) {
        Ok(decision) => Some(decision),
        Err(error) => {
            return SelfRuntimeOutcome {
                decision: None,
                world_sense_result,
                autonomy_strategy_result,
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
            inner_life.clone(),
            summary_text.as_deref(),
            execution_state.as_ref(),
            self_model.as_ref(),
            private_docs.as_ref(),
            self_continuity.as_ref(),
            Some(true),
            Some(recent.as_slice()),
        )
    } else {
        Ok(InnerLifeRefreshOutcome::Skipped)
    };

    let refreshed_inner_life = ctx
        .inner_life_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(inner_life);
    let private_doc_result = if decision_ref.is_some_and(|d| d.refresh_private_docs) {
        run_private_doc_workspace_refresh_with_state(
            http,
            llm,
            PrivateDocWorkspaceRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
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
            private_docs.clone(),
            summary_text.as_deref(),
            execution_state.as_ref(),
            self_model.as_ref(),
            &private_garden_docs,
            decision_ref.and_then(|d| {
                (!d.private_docs_intent.trim().is_empty()).then_some(d.private_docs_intent.as_str())
            }),
            &[],
            refreshed_autonomy_strategy.as_ref(),
            self_continuity.as_ref(),
            refreshed_inner_life.as_ref(),
            refreshed_world_sense.as_ref(),
            Some(true),
            Some(recent.as_slice()),
        )
    } else {
        Ok(PrivateDocWorkspaceRefreshOutcome::Skipped)
    };
    let refreshed_private_docs = ctx
        .private_doc_store
        .get(chat_id)
        .ok()
        .flatten()
        .or(private_docs.clone());
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
            self_continuity.clone(),
            summary_text.as_deref(),
            execution_state.as_ref(),
            self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            refreshed_inner_life.as_ref(),
            Some(true),
            Some(recent.as_slice()),
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
            summary_text.as_deref(),
            execution_state.as_ref(),
            self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            decision_ref.and_then(|d| {
                (!d.private_garden_intent.trim().is_empty())
                    .then_some(d.private_garden_intent.as_str())
            }),
            &[],
            Some(true),
            Some(recent.as_slice()),
        )
    } else {
        Ok(PrivateGardenGovernanceOutcome::Skipped)
    };

    let _ = touch_self_continuity_runtime(
        ctx.self_continuity_store,
        chat_id,
        payload.now_secs,
        false,
        true,
    );

    SelfRuntimeOutcome {
        decision,
        world_sense_result,
        autonomy_strategy_result,
        inner_life_result,
        private_doc_result,
        self_continuity_result,
        private_garden_result,
    }
}

#[allow(clippy::too_many_arguments)]
fn decide_self_runtime(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
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
    session_store: &dyn SessionStore,
    chat_id: &str,
) -> Result<SelfRuntimeDecision> {
    let policy = memory_policy(profile).self_runtime;
    let recent = session_store.load_recent(chat_id, policy.recent_message_count)?;
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
    serde_json::from_str(response.content.trim())
        .map_err(|error| crate::error::Error::config("self_runtime_parse", error.to_string()))
}
