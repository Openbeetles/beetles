//! 自治运行层：由 LLM 决定是否经营自己的内在空间。
#![allow(clippy::too_many_arguments)]

mod governance;
mod llm;
mod scheduler;
mod state;

use crate::bus::{IngressKind, PcMsg, SystemInboundTx};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::{
    TLS_ADMISSION_MIN_INTERNAL_BYTES, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
    TLS_ADMISSION_NO_PSRAM_MIN_BYTES,
};
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::platform::SkillStorage;
use crate::task::TaskStore;
use crate::util::{current_unix_secs, scrub_credentials, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::Write as _;

use self::governance::{
    apply_personality_runtime_governance_gate, detect_boundary_flush_signal,
    normalize_initial_self_runtime_decision, normalize_runtime_distillation_decisions,
    normalize_runtime_source_id, re_finalize_staged_self_runtime_decision,
    refresh_runtime_relationship_constitution, SelfRuntimeBoundarySignal,
};
#[cfg(test)]
use self::governance::{
    normalize_self_runtime_decision, PersonaDistillationSnapshot, SelfRuntimeBoundaryReason,
};
use self::llm::decide_self_runtime;
use self::scheduler::idle_memory_hygiene_budget_allows_run;
pub use self::scheduler::{
    enqueue_self_runtime_idle_tick, enqueue_self_runtime_post_reply, self_runtime_tick,
};
#[cfg(test)]
use self::scheduler::{idle_self_runtime_due, should_enqueue_self_runtime_post_reply_with_state};
use self::state::{
    load_self_runtime_state, sync_self_runtime_relationship_constitution,
    sync_self_runtime_relationship_portfolio, sync_self_runtime_relationship_topology,
};

use super::{
    autonomy_idle_interval_secs, board_subject_scope_id, build_archive_evidence_block,
    build_self_state, build_world_snapshot, compute_core_revision_governance_digest,
    derive_personality_runtime_governance_gate_from_inspection, inspect_personality_governance,
    llm_json::{
        get_object_bool, get_object_string_list, get_object_text, parse_llm_json_payload,
        LlmJsonPayload,
    },
    load_recent_persona_evidence, memory_capability_profile, memory_policy, relationship_scope_id,
    render_autonomy_strategy_block, render_core_revision_governance_block,
    render_execution_state_block, render_internal_memory_topology_block,
    render_mental_privacy_boundary_block, render_persistent_self_authored_core_block,
    render_private_memory_boundary_block, render_recent_persona_evidence_block,
    render_relationship_constitution_block, render_relationship_portfolio_block,
    render_relationship_topology_block, render_self_authored_core_block, render_self_state_block,
    render_world_sense_block, render_world_snapshot_block,
    run_autonomy_strategy_refresh_with_state, run_boundary_persona_refresh_with_state,
    run_inner_life_refresh_with_state, run_memory_governance_kernel, run_memory_hygiene_jobs,
    run_outer_voice_refresh_with_state, run_private_doc_workspace_refresh_with_state,
    run_private_garden_governance_with_state, run_self_authored_core_refresh_with_state,
    run_self_continuity_refresh_with_state, run_self_model_refresh_with_state,
    run_world_sense_refresh_with_state, select_relationship_portfolio_targets,
    sync_relationship_constitution, sync_relationship_portfolio,
    touch_relationship_portfolio_selection, touch_self_continuity_runtime,
    upsert_relationship_topology_entry, AutonomyGovernanceTendency, AutonomyStrategyRefreshContext,
    AutonomyStrategyRefreshInput, AutonomyStrategyRefreshOutcome, AutonomyStrategyStore,
    BoundaryPersonaRefreshContext, BoundaryPersonaRefreshInput, BoundaryPersonaRefreshOutcome,
    CoreRevisionGovernanceDigest, CoreRevisionLedgerStore, ExecutionStateStore,
    InnerLifeRefreshContext, InnerLifeRefreshInput, InnerLifeRefreshOutcome, InnerLifeStore,
    InternalMemoryLayerFocus, LongTermMemoryStore, MemoryGovernanceContext, MemoryGovernanceInput,
    MemoryHygieneContext, MemoryProfile, MemoryStore, MentalPrivacyStore, OuterVoiceRefreshContext,
    OuterVoiceRefreshInput, OuterVoiceRefreshOutcome, OuterVoiceStore,
    PersonalityGovernanceInspectionInput, PrivateDocStore, PrivateDocWorkspaceRefreshContext,
    PrivateDocWorkspaceRefreshInput, PrivateDocWorkspaceRefreshOutcome,
    PrivateGardenGovernanceContext, PrivateGardenGovernanceInput, PrivateGardenGovernanceOutcome,
    PrivateGardenStore, RelationshipConstitution, RelationshipConstitutionStore,
    RelationshipConstitutionSyncInput, RelationshipPortfolio, RelationshipPortfolioSelectorInput,
    RelationshipPortfolioStore, RelationshipTopology, RelationshipTopologyStore, RemindAtStore,
    SelfAuthoredCoreRefreshContext, SelfAuthoredCoreRefreshInput, SelfAuthoredCoreRefreshOutcome,
    SelfAuthoredCoreStore, SelfContinuityRefreshContext, SelfContinuityRefreshInput,
    SelfContinuityRefreshOutcome, SelfContinuityStore, SelfMemorySpaceBottleneck,
    SelfMemorySpacePressure, SelfModelRefreshContext, SelfModelRefreshInput,
    SelfModelRefreshOutcome, SelfModelStore, SelfState, SessionStore, SessionSummaryStore,
    SharedFactualPlaneSnapshot, SharedFactualReconcileAction, TurnLedgerStore,
    WorldSenseRefreshContext, WorldSenseRefreshInput, WorldSenseRefreshOutcome, WorldSenseStore,
    WorldSnapshotContext,
};

pub const SELF_RUNTIME_SYSTEM_PROMPT: &str = "You govern the assistant's inward autonomy runtime. Respect the current autonomy strategy unless the latest world state, self-state, or recent multi-turn persona evidence clearly requires a different emphasis. Return JSON only: one object with fields refresh_inner_life, inner_life_intent, refresh_private_docs, private_docs_intent, private_docs_action, refresh_private_garden, private_garden_intent, private_garden_action, refresh_self_model, self_model_intent, self_model_sources, refresh_self_continuity, self_continuity_intent, self_continuity_sources, refresh_self_authored_core, self_authored_core_intent, self_authored_core_sources, refresh_boundary_persona, boundary_persona_intent, refresh_outer_voice, outer_voice_intent, outer_voice_sources, boundary_flush, boundary_flush_reason, request_factual_refresh, factual_reconcile_action, factual_reconcile_intent. Use true only when that layer should change now. Runtime governance actions are hold, rewrite, compress, or cleanup. factual_reconcile_action is hold, reinforce, correct, conflict, or stale. self_model, self_continuity, self_authored_core, boundary_persona, and outer_voice are upward distillation layers: refresh them only when private evolution or newer world/boundary state has produced a better stable core that should influence future main replies. self_authored_core is the board-level core above chat relationships; do not promote one-turn spikes or one-chat quirks into it. Relationship portfolio is the board-level governance layer above relationship overlays. Relationship constitution is the formal board-to-relation contract: respect it when deciding how much a relation may drift, which local layers need realignment, and whether any relation may push upward into board-level distillation. Source lists should name the layers that actually deserve upward distillation, such as inner_life, private_docs, private_garden, self_model, self_continuity, self_authored_core, boundary_persona, outer_voice, world_sense, autonomy_strategy, recent_persona_evidence, or recent_transcript. Treat recent persona evidence as multi-turn support, never as one-turn automatic promotion authority. Favor autonomy, but do not churn memory without gain.";
pub const SELF_RUNTIME_CHANNEL: &str = "_self_runtime";
const SELF_RUNTIME_POST_REPLY_DELAY_MS: u64 = 1_500;
const SELF_RUNTIME_IDLE_TICK_DELAY_MS: u64 = 5_000;

pub(super) fn self_runtime_private_garden_doc_limit(profile: MemoryProfile) -> usize {
    let policy = memory_policy(profile);
    policy
        .private_garden
        .recent_doc_count
        .max(policy.private_garden_governance.existing_doc_count)
        .max(1)
}

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
    pub refresh_self_model: bool,
    #[serde(default)]
    pub self_model_intent: String,
    #[serde(default)]
    pub self_model_sources: Vec<String>,
    #[serde(default)]
    pub refresh_self_authored_core: bool,
    #[serde(default)]
    pub self_authored_core_intent: String,
    #[serde(default)]
    pub self_authored_core_sources: Vec<String>,
    #[serde(default)]
    pub refresh_self_continuity: bool,
    #[serde(default)]
    pub self_continuity_intent: String,
    #[serde(default)]
    pub self_continuity_sources: Vec<String>,
    #[serde(default)]
    pub refresh_private_garden: bool,
    #[serde(default)]
    pub private_garden_intent: String,
    #[serde(default)]
    pub private_garden_action: SelfRuntimeGovernanceAction,
    #[serde(default)]
    pub refresh_boundary_persona: bool,
    #[serde(default)]
    pub boundary_persona_intent: String,
    #[serde(default)]
    pub refresh_outer_voice: bool,
    #[serde(default)]
    pub outer_voice_intent: String,
    #[serde(default)]
    pub outer_voice_sources: Vec<String>,
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
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    pub relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    pub relationship_topology_store: &'a dyn RelationshipTopologyStore,
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
    pub inner_life_result: Result<InnerLifeRefreshOutcome>,
    pub private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    pub self_model_result: Result<SelfModelRefreshOutcome>,
    pub self_authored_core_result: Result<SelfAuthoredCoreRefreshOutcome>,
    pub self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    pub private_garden_result: Result<PrivateGardenGovernanceOutcome>,
    pub boundary_persona_result: Result<BoundaryPersonaRefreshOutcome>,
    pub outer_voice_result: Result<OuterVoiceRefreshOutcome>,
}

struct LoadedSelfRuntimeState {
    summary_text: Option<String>,
    execution_state: Option<crate::memory::ExecutionState>,
    self_model: Option<crate::memory::SelfModel>,
    self_authored_core: Option<crate::memory::SelfAuthoredCore>,
    core_revision_ledger: Option<crate::memory::CoreRevisionLedger>,
    core_revision_governance: CoreRevisionGovernanceDigest,
    private_docs: Option<crate::memory::PrivateDocWorkspace>,
    private_garden_docs: Vec<crate::memory::PrivateGardenDocRecord>,
    inner_life: Option<crate::memory::InnerLife>,
    self_continuity: Option<crate::memory::SelfContinuity>,
    relationship_portfolio: Option<crate::memory::RelationshipPortfolio>,
    relationship_topology: Option<crate::memory::RelationshipTopology>,
    relationship_constitution: Option<crate::memory::RelationshipConstitution>,
    world_sense: Option<crate::memory::WorldSense>,
    autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    outer_voice: Option<crate::memory::OuterVoice>,
    mental_privacy_state: Option<crate::memory::MentalPrivacyState>,
    recent_persona_evidence: Option<crate::memory::RecentPersonaEvidence>,
    active_relationship_scope_id: String,
    active_relationship_channel: String,
    prior_user_channel: String,
    world_snapshot: crate::memory::WorldSnapshot,
    recent: Vec<crate::memory::SessionMessage>,
}

struct SelfRuntimeRefreshPrelude {
    world_sense_result: Result<WorldSenseRefreshOutcome>,
    autonomy_strategy_result: Result<AutonomyStrategyRefreshOutcome>,
    refreshed_world_sense: Option<crate::memory::WorldSense>,
    refreshed_autonomy_strategy: Option<crate::memory::AutonomyStrategy>,
    runtime_self_state: SelfState,
}

struct SelfRuntimeActionResults {
    decision: Option<SelfRuntimeDecision>,
    inner_life_result: Result<InnerLifeRefreshOutcome>,
    private_doc_result: Result<PrivateDocWorkspaceRefreshOutcome>,
    self_model_result: Result<SelfModelRefreshOutcome>,
    self_authored_core_result: Result<SelfAuthoredCoreRefreshOutcome>,
    self_continuity_result: Result<SelfContinuityRefreshOutcome>,
    private_garden_result: Result<PrivateGardenGovernanceOutcome>,
    boundary_persona_result: Result<BoundaryPersonaRefreshOutcome>,
    outer_voice_result: Result<OuterVoiceRefreshOutcome>,
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

fn self_runtime_ingress(trigger: SelfRuntimeTrigger) -> IngressKind {
    match trigger {
        SelfRuntimeTrigger::PostReply => IngressKind::User,
        SelfRuntimeTrigger::IdleTick => IngressKind::System,
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
) -> Box<SelfRuntimeRefreshPrelude> {
    let subject_id = board_subject_scope_id();
    let relationship_id = state.active_relationship_scope_id.as_str();
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
                    channel: &state.active_relationship_channel,
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
    crate::platform::task_wdt::feed_current_task();
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
            channel: &state.active_relationship_channel,
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
    crate::platform::task_wdt::feed_current_task();
    let refreshed_world_sense = ctx
        .world_sense_store
        .get(relationship_id)
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
                    channel: &state.active_relationship_channel,
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
    crate::platform::task_wdt::feed_current_task();
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
            channel: &state.active_relationship_channel,
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
    crate::platform::task_wdt::feed_current_task();
    let refreshed_autonomy_strategy = ctx
        .autonomy_strategy_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(state.autonomy_strategy.clone());
    crate::platform::task_wdt::feed_current_task();
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
    Box::new(SelfRuntimeRefreshPrelude {
        world_sense_result,
        autonomy_strategy_result,
        refreshed_world_sense,
        refreshed_autonomy_strategy,
        runtime_self_state,
    })
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
) -> Box<SelfRuntimeActionResults> {
    let subject_id = board_subject_scope_id();
    let relationship_id = state.active_relationship_scope_id.as_str();
    let boundary_signal = detect_boundary_flush_signal(payload, state, prelude);
    let personality_governance_inspection =
        inspect_personality_governance(PersonalityGovernanceInspectionInput {
            channel: &state.active_relationship_channel,
            chat_id,
            now_secs: payload.now_secs,
            self_authored_core: state.self_authored_core.as_ref(),
            core_revision_ledger: state.core_revision_ledger.as_ref(),
            relationship_constitution: state.relationship_constitution.as_ref(),
            relationship_topology: state.relationship_topology.as_ref(),
            recent_persona_evidence: state.recent_persona_evidence.as_ref(),
        });
    let personality_governance_gate = derive_personality_runtime_governance_gate_from_inspection(
        &personality_governance_inspection,
    );
    crate::platform::task_wdt::feed_current_task();
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
    crate::platform::task_wdt::feed_current_task();
    let factual_snapshot = governance.factual_plane_snapshot;
    let mut decision = match decide_self_runtime(
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
        state.self_authored_core.as_ref(),
        state.core_revision_ledger.as_ref(),
        &state.core_revision_governance,
        state.private_docs.as_ref(),
        &state.private_garden_docs,
        state.inner_life.as_ref(),
        state.self_continuity.as_ref(),
        state.outer_voice.as_ref(),
        state.mental_privacy_state.as_ref(),
        state.relationship_portfolio.as_ref(),
        state.relationship_topology.as_ref(),
        state.relationship_constitution.as_ref(),
        state.active_relationship_scope_id.as_str(),
        state.recent_persona_evidence.as_ref(),
        prelude.refreshed_world_sense.as_ref(),
        &state.world_snapshot,
        prelude.refreshed_autonomy_strategy.as_ref(),
        profile,
        state.recent.as_slice(),
        &factual_snapshot,
        &boundary_signal,
    ) {
        Ok(decision) => {
            let mut decision = normalize_initial_self_runtime_decision(
                decision,
                payload.trigger,
                prelude.refreshed_autonomy_strategy.as_ref(),
                &prelude.runtime_self_state,
                state.self_model.is_some(),
                state.self_authored_core.is_some(),
                state.private_docs.is_some(),
                !state.private_garden_docs.is_empty(),
                state.outer_voice.is_some(),
                state.mental_privacy_state.is_some(),
                &factual_snapshot,
                &boundary_signal,
            );
            apply_personality_runtime_governance_gate(&mut decision, &personality_governance_gate);
            normalize_runtime_distillation_decisions(
                &mut decision,
                state.private_docs.is_some(),
                !state.private_garden_docs.is_empty(),
                state.inner_life.is_some(),
                state.self_model.is_some(),
                state.self_authored_core.is_some(),
                state.self_continuity.is_some(),
                state.outer_voice.is_some(),
                state.mental_privacy_state.is_some(),
                prelude.refreshed_world_sense.is_some() || state.world_sense.is_some(),
                prelude.refreshed_autonomy_strategy.is_some() || state.autonomy_strategy.is_some(),
                state.recent_persona_evidence.is_some(),
            );
            Some(decision)
        }
        Err(error) => {
            return Box::new(SelfRuntimeActionResults {
                decision: None,
                inner_life_result: Err(error),
                private_doc_result: Ok(PrivateDocWorkspaceRefreshOutcome::Skipped),
                self_model_result: Ok(SelfModelRefreshOutcome::Skipped),
                self_authored_core_result: Ok(SelfAuthoredCoreRefreshOutcome::Skipped),
                self_continuity_result: Ok(SelfContinuityRefreshOutcome::Skipped),
                private_garden_result: Ok(PrivateGardenGovernanceOutcome::Skipped),
                boundary_persona_result: Ok(BoundaryPersonaRefreshOutcome::Skipped),
                outer_voice_result: Ok(OuterVoiceRefreshOutcome::Skipped),
            });
        }
    };
    crate::platform::task_wdt::feed_current_task();
    let mut refreshed_inner_life = state.inner_life.clone();
    let mut refreshed_private_docs = state.private_docs.clone();
    let mut refreshed_private_garden_docs = state.private_garden_docs.clone();
    let mut refreshed_self_model = state.self_model.clone();
    let mut refreshed_self_authored_core = state.self_authored_core.clone();
    let mut refreshed_self_continuity = state.self_continuity.clone();
    let mut refreshed_mental_privacy = state.mental_privacy_state.clone();
    let mut refreshed_outer_voice = state.outer_voice.clone();
    let mut refreshed_relationship_constitution = state.relationship_constitution.clone();
    let decision_ref = decision.as_ref();
    let inner_life_result = if decision_ref.is_some_and(|d| d.refresh_inner_life) {
        crate::platform::task_wdt::feed_current_task();
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
    refreshed_inner_life = ctx
        .inner_life_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_inner_life);
    crate::platform::task_wdt::feed_current_task();
    let private_doc_result = if decision_ref.is_some_and(|d| d.refresh_private_docs) {
        crate::platform::task_wdt::feed_current_task();
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
            refreshed_private_docs.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            &refreshed_private_garden_docs,
            decision_ref.and_then(|d| {
                (!d.private_docs_intent.trim().is_empty()).then_some(d.private_docs_intent.as_str())
            }),
            &[],
            prelude.refreshed_autonomy_strategy.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_inner_life.as_ref(),
            prelude.refreshed_world_sense.as_ref(),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(PrivateDocWorkspaceRefreshOutcome::Skipped)
    };
    refreshed_private_docs = ctx
        .private_doc_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_private_docs);
    crate::platform::task_wdt::feed_current_task();
    let private_garden_result = if decision_ref.is_some_and(|d| d.refresh_private_garden) {
        crate::platform::task_wdt::feed_current_task();
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
            refreshed_self_model.as_ref(),
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
    refreshed_private_garden_docs = ctx
        .private_garden_store
        .list(chat_id, self_runtime_private_garden_doc_limit(profile))
        .unwrap_or(refreshed_private_garden_docs);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_model_result = if decision_ref.is_some_and(|d| d.refresh_self_model) {
        crate::platform::task_wdt::feed_current_task();
        run_self_model_refresh_with_state(
            http,
            llm,
            SelfModelRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
                long_term_memory_store: ctx.long_term_memory_store,
                self_model_store: ctx.self_model_store,
            },
            SelfModelRefreshInput {
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
            refreshed_self_model.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_private_docs.as_ref(),
            &refreshed_private_garden_docs,
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.self_model_intent.trim().is_empty()).then_some(d.self_model_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_model_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(SelfModelRefreshOutcome::Skipped)
    };
    refreshed_self_model = ctx
        .self_model_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_self_model);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_continuity_result = if decision_ref.is_some_and(|d| d.refresh_self_continuity) {
        crate::platform::task_wdt::feed_current_task();
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
            refreshed_self_model.as_ref(),
            refreshed_private_docs.as_ref(),
            refreshed_inner_life.as_ref(),
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.self_continuity_intent.trim().is_empty())
                    .then_some(d.self_continuity_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_continuity_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(SelfContinuityRefreshOutcome::Skipped)
    };
    refreshed_self_continuity = ctx
        .self_continuity_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_self_continuity);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let boundary_persona_result = if decision_ref.is_some_and(|d| d.refresh_boundary_persona) {
        let trigger = match payload.trigger {
            SelfRuntimeTrigger::PostReply => "post_reply",
            SelfRuntimeTrigger::IdleTick => "idle_tick",
        };
        crate::platform::task_wdt::feed_current_task();
        run_boundary_persona_refresh_with_state(
            http,
            llm,
            BoundaryPersonaRefreshContext {
                mental_privacy_store: ctx.mental_privacy_store,
                relationship_constitution_store: ctx.relationship_constitution_store,
                outer_voice_store: ctx.outer_voice_store,
            },
            BoundaryPersonaRefreshInput {
                channel: &state.active_relationship_channel,
                chat_id,
                trigger,
                intent: decision_ref
                    .and_then(|d| {
                        (!d.boundary_persona_intent.trim().is_empty())
                            .then_some(d.boundary_persona_intent.as_str())
                    })
                    .unwrap_or(""),
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                now_secs: payload.now_secs,
            },
            refreshed_mental_privacy.clone(),
            refreshed_self_model.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_relationship_constitution.as_ref(),
            state.recent_persona_evidence.as_ref(),
            state.recent.as_slice(),
            Some(true),
        )
    } else {
        Ok(BoundaryPersonaRefreshOutcome::Skipped)
    };
    refreshed_mental_privacy = ctx
        .mental_privacy_store
        .get(relationship_id)
        .ok()
        .flatten()
        .or(refreshed_mental_privacy);
    refreshed_relationship_constitution = refresh_runtime_relationship_constitution(
        ctx,
        state,
        chat_id,
        payload.now_secs,
        refreshed_self_authored_core.as_ref(),
        refreshed_mental_privacy.as_ref(),
        refreshed_outer_voice.as_ref(),
    )
    .or(refreshed_relationship_constitution);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
        &personality_governance_gate,
        state,
        prelude,
        refreshed_private_docs.as_ref(),
        &refreshed_private_garden_docs,
        refreshed_inner_life.as_ref(),
        refreshed_self_model.as_ref(),
        refreshed_self_authored_core.as_ref(),
        refreshed_self_continuity.as_ref(),
        refreshed_outer_voice.as_ref(),
        refreshed_mental_privacy.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let outer_voice_result = if decision_ref.is_some_and(|d| d.refresh_outer_voice) {
        crate::platform::task_wdt::feed_current_task();
        run_outer_voice_refresh_with_state(
            http,
            llm,
            OuterVoiceRefreshContext {
                outer_voice_store: ctx.outer_voice_store,
            },
            OuterVoiceRefreshInput {
                chat_id,
                ingress: IngressKind::System,
                channel: &state.active_relationship_channel,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            profile,
            refreshed_outer_voice.clone(),
            state.summary_text.as_deref(),
            state.execution_state.as_ref(),
            refreshed_self_model.as_ref(),
            &state.world_snapshot,
            prelude.refreshed_world_sense.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            refreshed_inner_life.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_private_docs.as_ref(),
            &refreshed_private_garden_docs,
            refreshed_mental_privacy.as_ref(),
            refreshed_relationship_constitution.as_ref(),
            state.recent_persona_evidence.as_ref(),
            decision_ref.and_then(|d| {
                (!d.outer_voice_intent.trim().is_empty()).then_some(d.outer_voice_intent.as_str())
            }),
            decision_ref
                .map(|d| d.outer_voice_sources.as_slice())
                .unwrap_or(&[]),
            Some(true),
            Some(state.recent.as_slice()),
        )
    } else {
        Ok(OuterVoiceRefreshOutcome::Skipped)
    };
    refreshed_outer_voice = ctx
        .outer_voice_store
        .get(relationship_id)
        .ok()
        .flatten()
        .or(refreshed_outer_voice);
    let _ = refresh_runtime_relationship_constitution(
        ctx,
        state,
        chat_id,
        payload.now_secs,
        refreshed_self_authored_core.as_ref(),
        refreshed_mental_privacy.as_ref(),
        refreshed_outer_voice.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    let decision_ref = decision.as_ref();
    let self_authored_core_result = if decision_ref.is_some_and(|d| d.refresh_self_authored_core) {
        let self_state_text = render_self_state_block(
            &build_self_state(
                refreshed_self_model.as_ref(),
                refreshed_private_docs.as_ref(),
                prelude.refreshed_autonomy_strategy.as_ref(),
                refreshed_inner_life.as_ref(),
                refreshed_self_continuity.as_ref(),
                &refreshed_private_garden_docs,
                payload.now_secs,
                profile,
            ),
            memory_policy(profile).self_state.render_max_len,
        );
        crate::platform::task_wdt::feed_current_task();
        run_self_authored_core_refresh_with_state(
            http,
            llm,
            SelfAuthoredCoreRefreshContext {
                self_authored_core_store: ctx.self_authored_core_store,
                core_revision_ledger_store: ctx.core_revision_ledger_store,
            },
            SelfAuthoredCoreRefreshInput {
                chat_id: subject_id,
                ingress: IngressKind::System,
                channel: SELF_RUNTIME_CHANNEL,
                user_content: &payload.user_content,
                reply_content: &payload.reply_content,
                pressure: PressureLevel::Normal,
                tool_calls: payload.tool_calls,
                now_secs: payload.now_secs,
            },
            refreshed_self_authored_core.clone(),
            refreshed_self_model.as_ref(),
            refreshed_self_continuity.as_ref(),
            refreshed_mental_privacy.as_ref(),
            state.relationship_portfolio.as_ref(),
            state.active_relationship_scope_id.as_str(),
            state.recent_persona_evidence.as_ref(),
            state.relationship_topology.as_ref(),
            prelude.refreshed_world_sense.as_ref(),
            prelude.refreshed_autonomy_strategy.as_ref(),
            self_state_text.as_deref(),
            decision_ref.and_then(|d| {
                (!d.self_authored_core_intent.trim().is_empty())
                    .then_some(d.self_authored_core_intent.as_str())
            }),
            decision_ref
                .map(|d| d.self_authored_core_sources.as_slice())
                .unwrap_or(&[]),
        )
    } else {
        Ok(SelfAuthoredCoreRefreshOutcome::Skipped)
    };
    refreshed_self_authored_core = ctx
        .self_authored_core_store
        .get(subject_id)
        .ok()
        .flatten()
        .or(refreshed_self_authored_core);
    let _ = refresh_runtime_relationship_constitution(
        ctx,
        state,
        chat_id,
        payload.now_secs,
        refreshed_self_authored_core.as_ref(),
        refreshed_mental_privacy.as_ref(),
        refreshed_outer_voice.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    Box::new(SelfRuntimeActionResults {
        decision,
        inner_life_result,
        private_doc_result,
        self_model_result,
        self_authored_core_result,
        self_continuity_result,
        private_garden_result,
        boundary_persona_result,
        outer_voice_result,
    })
}

pub fn run_self_runtime(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
) -> Box<SelfRuntimeOutcome> {
    sync_self_runtime_relationship_topology(
        &ctx,
        payload.source_channel.as_str(),
        chat_id,
        payload.now_secs,
    );
    let _ = sync_self_runtime_relationship_portfolio(&ctx, payload.now_secs);
    let state = load_self_runtime_state(&ctx, chat_id, payload, profile);
    crate::platform::task_wdt::feed_current_task();
    let prelude =
        refresh_world_and_autonomy(http, llm, &ctx, chat_id, payload, profile, state.as_ref());
    crate::platform::task_wdt::feed_current_task();
    let action_results = execute_self_runtime_actions(
        http,
        llm,
        &ctx,
        chat_id,
        payload,
        profile,
        state.as_ref(),
        prelude.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();

    let _ = touch_self_continuity_runtime(
        ctx.self_continuity_store,
        board_subject_scope_id(),
        payload.now_secs,
        payload.trigger == SelfRuntimeTrigger::PostReply,
        true,
        Some(chat_id),
        Some(payload.source_channel.as_str()),
    );
    crate::platform::task_wdt::feed_current_task();
    sync_self_runtime_relationship_topology(
        &ctx,
        state.active_relationship_channel.as_str(),
        chat_id,
        payload.now_secs,
    );
    let portfolio_after = sync_self_runtime_relationship_portfolio(&ctx, payload.now_secs);
    let latest_self_authored_core = ctx
        .self_authored_core_store
        .get(board_subject_scope_id())
        .ok()
        .flatten()
        .or(state.self_authored_core.clone());
    let latest_relationship_topology = ctx
        .relationship_topology_store
        .get(board_subject_scope_id())
        .ok()
        .flatten()
        .or(state.relationship_topology.clone());
    let latest_outer_voice = ctx
        .outer_voice_store
        .get(state.active_relationship_scope_id.as_str())
        .ok()
        .flatten()
        .or(state.outer_voice.clone());
    let latest_mental_privacy = ctx
        .mental_privacy_store
        .get(state.active_relationship_scope_id.as_str())
        .ok()
        .flatten()
        .or(state.mental_privacy_state.clone());
    let _ = sync_self_runtime_relationship_constitution(
        &ctx,
        state.active_relationship_scope_id.as_str(),
        state.active_relationship_channel.as_str(),
        chat_id,
        payload.now_secs,
        latest_self_authored_core.as_ref(),
        portfolio_after
            .as_ref()
            .or(state.relationship_portfolio.as_ref()),
        latest_relationship_topology.as_ref(),
        latest_mental_privacy.as_ref(),
        latest_outer_voice.as_ref(),
        state.recent_persona_evidence.as_ref(),
    );
    crate::platform::task_wdt::feed_current_task();
    if matches!(payload.trigger, SelfRuntimeTrigger::IdleTick) {
        if idle_memory_hygiene_budget_allows_run() {
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
            crate::platform::task_wdt::feed_current_task();
        } else {
            log::debug!(
                "[self_runtime] skip idle memory hygiene this tick because write budget is reserved"
            );
        }
    }

    Box::new(SelfRuntimeOutcome {
        decision: action_results.decision,
        world_sense_result: prelude.world_sense_result,
        autonomy_strategy_result: prelude.autonomy_strategy_result,
        inner_life_result: action_results.inner_life_result,
        private_doc_result: action_results.private_doc_result,
        self_model_result: action_results.self_model_result,
        self_authored_core_result: action_results.self_authored_core_result,
        self_continuity_result: action_results.self_continuity_result,
        private_garden_result: action_results.private_garden_result,
        boundary_persona_result: action_results.boundary_persona_result,
        outer_voice_result: action_results.outer_voice_result,
    })
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod tests {
    use super::llm::parse_self_runtime_decision;
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

    fn sample_distillation_snapshot() -> PersonaDistillationSnapshot {
        PersonaDistillationSnapshot {
            private_material_at: 20,
            boundary_state_at: 18,
            world_context_at: 17,
            world_sense_at: 16,
            autonomy_strategy_at: 17,
            recent_persona_evidence_at: 19,
            self_model_at: 10,
            self_authored_core_at: 9,
            self_continuity_at: 10,
            outer_voice_at: 9,
            has_inner_life: true,
            has_world_sense: true,
            has_autonomy_strategy: true,
            has_recent_persona_evidence: true,
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
            "refresh_self_model": true,
            "self_model_intent": { "goal": "distill self core" },
            "self_model_sources": ["private_docs", "inner-life"],
            "refresh_self_authored_core": true,
            "self_authored_core_intent": { "goal": "refresh board core" },
            "self_authored_core_sources": ["self_model", "boundary persona"],
            "refresh_self_continuity": false,
            "self_continuity_intent": 0,
            "self_continuity_sources": ["self_model", "recent transcript"],
            "refresh_private_garden": { "enabled": true },
            "private_garden_intent": { "path": "journal/today.md" },
            "private_garden_action": "cleanup",
            "refresh_boundary_persona": "true",
            "boundary_persona_intent": ["stabilize boundary stance"],
            "refresh_outer_voice": 1,
            "outer_voice_intent": { "why": "express new stance" },
            "outer_voice_sources": ["boundary persona", "world_sense"],
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
        assert!(parsed.refresh_self_model);
        assert!(parsed.refresh_self_authored_core);
        assert!(parsed.refresh_private_garden);
        assert!(parsed.refresh_boundary_persona);
        assert!(parsed.refresh_outer_voice);
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
        assert!(parsed.self_model_intent.contains("goal: distill self core"));
        assert_eq!(
            parsed.self_model_sources,
            vec!["private_docs".to_string(), "inner_life".to_string()]
        );
        assert!(parsed
            .self_authored_core_intent
            .contains("goal: refresh board core"));
        assert_eq!(
            parsed.self_authored_core_sources,
            vec!["self_model".to_string(), "boundary_persona".to_string()]
        );
        assert_eq!(parsed.self_continuity_intent, "0");
        assert_eq!(
            parsed.self_continuity_sources,
            vec!["self_model".to_string(), "recent_transcript".to_string()]
        );
        assert!(parsed
            .private_garden_intent
            .contains("path: journal/today.md"));
        assert!(parsed
            .boundary_persona_intent
            .contains("stabilize boundary stance"));
        assert!(parsed
            .outer_voice_intent
            .contains("why: express new stance"));
        assert_eq!(
            parsed.outer_voice_sources,
            vec!["boundary_persona".to_string(), "world_sense".to_string()]
        );
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
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            false,
            true,
            false,
            true,
            false,
            false,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_docs);
        assert!(decision
            .private_docs_intent
            .contains("Compress governed docs"));
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
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            false,
            false,
            true,
            true,
            false,
            false,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_private_garden);
        assert!(decision
            .private_garden_intent
            .contains("Rewrite and reorganize"));
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
            &PersonaDistillationSnapshot::default(),
            &CoreRevisionGovernanceDigest::default(),
            true,
            true,
            true,
            true,
            true,
            true,
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
        assert!(decision.refresh_self_model);
        assert!(decision.refresh_self_authored_core);
        assert!(decision.refresh_boundary_persona);
        assert!(decision.refresh_outer_voice);
    }

    #[test]
    fn distillation_lag_refreshes_upper_persona_layers() {
        let snapshot = sample_distillation_snapshot();
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &snapshot,
            &CoreRevisionGovernanceDigest::default(),
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_self_model);
        assert!(decision.refresh_self_authored_core);
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_outer_voice);
        assert!(decision
            .self_model_intent
            .contains("redistill a steadier kernel"));
        assert!(decision
            .self_continuity_intent
            .contains("continuity bridge"));
        assert!(decision.outer_voice_intent.contains("outward expression"));
        assert!(decision
            .self_authored_core_intent
            .contains("board-level self core"));
        assert!(decision
            .self_authored_core_sources
            .contains(&"boundary_persona".to_string()));
        assert!(decision
            .self_model_sources
            .contains(&"recent_persona_evidence".to_string()));
        assert!(decision
            .self_continuity_sources
            .contains(&"world_sense".to_string()));
        assert!(decision
            .self_continuity_sources
            .contains(&"recent_persona_evidence".to_string()));
        assert!(decision
            .outer_voice_sources
            .contains(&"autonomy_strategy".to_string()));
        assert!(decision
            .outer_voice_sources
            .contains(&"recent_persona_evidence".to_string()));
    }

    #[test]
    fn constitutional_review_due_forces_self_authored_core_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &PersonaDistillationSnapshot {
                self_authored_core_at: 200,
                self_model_at: 200,
                self_continuity_at: 200,
                boundary_state_at: 200,
                ..PersonaDistillationSnapshot::default()
            },
            &CoreRevisionGovernanceDigest {
                review_due: true,
                review_reasons: vec!["constitutional_review_cadence_due".to_string()],
                ..CoreRevisionGovernanceDigest::default()
            },
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(decision.refresh_self_authored_core);
        assert!(decision
            .self_authored_core_intent
            .contains("constitutional review"));
        assert!(decision
            .self_authored_core_sources
            .contains(&"self_model".to_string()));
    }

    #[test]
    fn conservative_constitution_blocks_volatile_only_core_refresh() {
        let decision = normalize_self_runtime_decision(
            SelfRuntimeDecision::default(),
            SelfRuntimeTrigger::IdleTick,
            None,
            &sample_self_state(),
            &PersonaDistillationSnapshot {
                self_authored_core_at: 100,
                self_model_at: 100,
                self_continuity_at: 100,
                boundary_state_at: 100,
                outer_voice_at: 140,
                recent_persona_evidence_at: 150,
                has_recent_persona_evidence: true,
                ..PersonaDistillationSnapshot::default()
            },
            &CoreRevisionGovernanceDigest {
                conservative_mode: true,
                latest_stability_score: 48,
                ..CoreRevisionGovernanceDigest::default()
            },
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            true,
            &SharedFactualPlaneSnapshot::default(),
            &SelfRuntimeBoundarySignal::default(),
        );

        assert!(!decision.refresh_self_authored_core);
    }

    #[test]
    fn runtime_governance_gate_blocks_unsettled_upward_distillation() {
        let mut decision = SelfRuntimeDecision {
            refresh_self_model: true,
            self_model_intent: "distill self kernel".to_string(),
            self_model_sources: vec!["private_docs".to_string(), "inner_life".to_string()],
            refresh_self_authored_core: true,
            self_authored_core_intent: "refresh board core".to_string(),
            self_authored_core_sources: vec!["self_model".to_string()],
            refresh_self_continuity: true,
            self_continuity_intent: "keep continuity bridge".to_string(),
            self_continuity_sources: vec!["self_model".to_string()],
            refresh_boundary_persona: true,
            boundary_persona_intent: "stabilize relation boundary".to_string(),
            refresh_outer_voice: true,
            outer_voice_intent: "rewrite outward expression".to_string(),
            outer_voice_sources: vec!["boundary_persona".to_string()],
            ..Default::default()
        };

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "board core still unstable".to_string(),
                outstanding: vec!["review cadence overdue".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    observe_only: true,
                    summary: "review cadence overdue".to_string(),
                    reasons: vec!["review cadence overdue".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(!decision.refresh_self_model);
        assert!(decision.self_model_intent.is_empty());
        assert!(decision.self_model_sources.is_empty());
        assert!(!decision.refresh_self_authored_core);
        assert!(decision.self_authored_core_intent.is_empty());
        assert!(decision.self_authored_core_sources.is_empty());
        assert!(!decision.refresh_outer_voice);
        assert!(decision.outer_voice_intent.is_empty());
        assert!(decision.outer_voice_sources.is_empty());
        assert!(decision.refresh_self_continuity);
        assert!(decision.refresh_boundary_persona);
    }

    #[test]
    fn runtime_governance_gate_allows_targeted_board_core_repair() {
        let mut decision = SelfRuntimeDecision::default();

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "board_core_review_due".to_string(),
                outstanding: vec!["governance_review_due".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    repair_needed: true,
                    primary_action:
                        crate::memory::PersonalityGovernanceRepairAction::RepairSelfAuthoredCore,
                    repair_self_authored_core: true,
                    summary: "board_core_review_due".to_string(),
                    reasons: vec!["board_core_review_due".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(decision.refresh_self_authored_core);
        assert!(decision
            .self_authored_core_intent
            .contains("Repair the board-level self core"));
        assert!(!decision.refresh_self_model);
        assert!(!decision.refresh_outer_voice);
    }

    #[test]
    fn runtime_governance_gate_allows_targeted_expression_repair() {
        let mut decision = SelfRuntimeDecision::default();

        apply_personality_runtime_governance_gate(
            &mut decision,
            &crate::memory::PersonalityRuntimeGovernanceGate {
                conservative_reply: true,
                allow_dynamic_persona_priority: false,
                allow_upward_distillation: false,
                reason_summary: "expression_drift_without_constitution_break".to_string(),
                outstanding: vec!["relationship_response_mode_drift".to_string()],
                repair_plan: crate::memory::PersonalityGovernanceRepairPlan {
                    repair_needed: true,
                    primary_action:
                        crate::memory::PersonalityGovernanceRepairAction::RepairOuterVoice,
                    repair_outer_voice: true,
                    summary: "expression_drift_without_constitution_break".to_string(),
                    reasons: vec!["expression_drift_without_constitution_break".to_string()],
                    ..crate::memory::PersonalityGovernanceRepairPlan::default()
                },
            },
        );

        assert!(decision.refresh_outer_voice);
        assert!(decision
            .outer_voice_intent
            .contains("Repair outward expression drift"));
        assert!(!decision.refresh_self_model);
        assert!(!decision.refresh_self_authored_core);
    }

    #[test]
    fn first_idle_tick_waits_for_strategy_cadence() {
        assert!(!idle_self_runtime_due(1_000, 60, 980, 0, 480));
        assert!(!idle_self_runtime_due(1_000, 300, 400, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 0, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 50, 100, 900));
    }

    #[test]
    fn post_reply_enqueue_runs_for_missing_core_or_runtime_signal() {
        let continuity = crate::memory::SelfContinuity {
            last_user_channel: "qq_channel".to_string(),
            last_autonomy_run_at: 900,
            ..crate::memory::SelfContinuity::default()
        };
        let strategy = crate::memory::AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            ..crate::memory::AutonomyStrategy::default()
        };

        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            false,
            "qq_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "qq_channel",
            1,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
        assert!(should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "feishu_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
    }

    #[test]
    fn post_reply_enqueue_skips_when_runtime_is_fresh_and_untriggered() {
        let continuity = crate::memory::SelfContinuity {
            last_user_channel: "qq_channel".to_string(),
            last_autonomy_run_at: 950,
            ..crate::memory::SelfContinuity::default()
        };
        let strategy = crate::memory::AutonomyStrategy {
            idle_enabled: true,
            idle_interval_secs: 300,
            ..crate::memory::AutonomyStrategy::default()
        };

        assert!(!should_enqueue_self_runtime_post_reply_with_state(
            Some(&continuity),
            Some(&strategy),
            true,
            "qq_channel",
            0,
            false,
            1_000,
            MemoryProfile::Standard,
        ));
    }
}
