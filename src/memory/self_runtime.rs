//! 自治运行层：由 LLM 决定是否经营自己的内在空间。

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
use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::{
    AutonomyGovernanceTendency, AutonomyStrategyRefreshContext, AutonomyStrategyRefreshInput,
    AutonomyStrategyRefreshOutcome, AutonomyStrategyStore, BoundaryPersonaRefreshContext,
    BoundaryPersonaRefreshInput, BoundaryPersonaRefreshOutcome, CoreRevisionGovernanceDigest,
    CoreRevisionLedgerStore, ExecutionStateStore, InnerLifeRefreshContext, InnerLifeRefreshInput,
    InnerLifeRefreshOutcome, InnerLifeStore, InternalMemoryLayerFocus, LongTermMemoryStore,
    MemoryGovernanceContext, MemoryGovernanceInput, MemoryHygieneContext, MemoryProfile,
    MemoryStore, MentalPrivacyStore, OuterVoiceRefreshContext, OuterVoiceRefreshInput,
    OuterVoiceRefreshOutcome, OuterVoiceStore, PrivateDocStore, PrivateDocWorkspaceRefreshContext,
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
    WorldSnapshotContext, autonomy_idle_interval_secs, board_subject_scope_id,
    build_archive_evidence_block, build_self_state, build_world_snapshot,
    compute_core_revision_governance_digest,
    llm_json::{
        LlmJsonPayload, get_object_bool, get_object_string_list, get_object_text,
        parse_llm_json_payload,
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
    upsert_relationship_topology_entry,
};

pub const SELF_RUNTIME_SYSTEM_PROMPT: &str = "You govern the assistant's inward autonomy runtime. Respect the current autonomy strategy unless the latest world state, self-state, or recent multi-turn persona evidence clearly requires a different emphasis. Return JSON only: one object with fields refresh_inner_life, inner_life_intent, refresh_private_docs, private_docs_intent, private_docs_action, refresh_private_garden, private_garden_intent, private_garden_action, refresh_self_model, self_model_intent, self_model_sources, refresh_self_continuity, self_continuity_intent, self_continuity_sources, refresh_self_authored_core, self_authored_core_intent, self_authored_core_sources, refresh_boundary_persona, boundary_persona_intent, refresh_outer_voice, outer_voice_intent, outer_voice_sources, boundary_flush, boundary_flush_reason, request_factual_refresh, factual_reconcile_action, factual_reconcile_intent. Use true only when that layer should change now. Runtime governance actions are hold, rewrite, compress, or cleanup. factual_reconcile_action is hold, reinforce, correct, conflict, or stale. self_model, self_continuity, self_authored_core, boundary_persona, and outer_voice are upward distillation layers: refresh them only when private evolution or newer world/boundary state has produced a better stable core that should influence future main replies. self_authored_core is the board-level core above chat relationships; do not promote one-turn spikes or one-chat quirks into it. Relationship portfolio is the board-level governance layer above relationship overlays. Relationship constitution is the formal board-to-relation contract: respect it when deciding how much a relation may drift, which local layers need realignment, and whether any relation may push upward into board-level distillation. Source lists should name the layers that actually deserve upward distillation, such as inner_life, private_docs, private_garden, self_model, self_continuity, self_authored_core, boundary_persona, outer_voice, world_sense, autonomy_strategy, recent_persona_evidence, or recent_transcript. Treat recent persona evidence as multi-turn support, never as one-turn automatic promotion authority. Favor autonomy, but do not churn memory without gain.";
pub const SELF_RUNTIME_CHANNEL: &str = "_self_runtime";
const SELF_RUNTIME_POST_REPLY_DELAY_MS: u64 = 1_500;
const SELF_RUNTIME_IDLE_TICK_DELAY_MS: u64 = 5_000;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PersonaDistillationSnapshot {
    private_material_at: u64,
    boundary_state_at: u64,
    world_context_at: u64,
    world_sense_at: u64,
    autonomy_strategy_at: u64,
    recent_persona_evidence_at: u64,
    self_model_at: u64,
    self_authored_core_at: u64,
    self_continuity_at: u64,
    outer_voice_at: u64,
    has_inner_life: bool,
    has_world_sense: bool,
    has_autonomy_strategy: bool,
    has_recent_persona_evidence: bool,
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
) -> Box<LoadedSelfRuntimeState> {
    let subject_id = board_subject_scope_id();
    let summary_text = ctx
        .session_summary_store
        .get_with_count(chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary);
    let execution_state = ctx.execution_state_store.get(chat_id).ok().flatten();
    let self_model = ctx.self_model_store.get(subject_id).ok().flatten();
    let self_authored_core = ctx.self_authored_core_store.get(subject_id).ok().flatten();
    let core_revision_ledger = ctx
        .core_revision_ledger_store
        .get(subject_id)
        .ok()
        .flatten();
    let core_revision_governance = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        payload.now_secs,
    );
    let private_docs = ctx.private_doc_store.get(subject_id).ok().flatten();
    let private_garden_docs = ctx
        .private_garden_store
        .list(chat_id, usize::MAX)
        .unwrap_or_default();
    let inner_life = ctx.inner_life_store.get(subject_id).ok().flatten();
    let self_continuity = ctx.self_continuity_store.get(subject_id).ok().flatten();
    let relationship_topology = ctx
        .relationship_topology_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_portfolio = sync_relationship_portfolio(
        ctx.relationship_portfolio_store,
        relationship_topology.as_ref(),
        self_authored_core.as_ref(),
        payload.now_secs,
    )
    .ok()
    .flatten()
    .or_else(|| {
        ctx.relationship_portfolio_store
            .get(subject_id)
            .ok()
            .flatten()
    });
    let prior_user_channel = self_continuity
        .as_ref()
        .map(|continuity| continuity.last_user_channel.trim().to_string())
        .unwrap_or_default();
    let (active_relationship_scope_id, active_relationship_channel) =
        resolve_runtime_relationship_scope(
            chat_id,
            payload,
            self_continuity.as_ref(),
            relationship_portfolio.as_ref(),
            relationship_topology.as_ref(),
        );
    let world_sense = ctx
        .world_sense_store
        .get(&active_relationship_scope_id)
        .ok()
        .flatten();
    let autonomy_strategy = ctx.autonomy_strategy_store.get(subject_id).ok().flatten();
    let outer_voice = ctx
        .outer_voice_store
        .get(&active_relationship_scope_id)
        .ok()
        .flatten();
    let mental_privacy_state = ctx
        .mental_privacy_store
        .get(&active_relationship_scope_id)
        .ok()
        .flatten();
    let recent_persona_evidence =
        load_recent_persona_evidence(ctx.turn_ledger_store, &active_relationship_scope_id)
            .ok()
            .flatten();
    let relationship_constitution = sync_relationship_constitution(
        ctx.relationship_constitution_store,
        RelationshipConstitutionSyncInput {
            scope_id: &active_relationship_scope_id,
            channel: &active_relationship_channel,
            chat_id,
            now_secs: payload.now_secs,
            self_authored_core: self_authored_core.as_ref(),
            relationship_portfolio: relationship_portfolio.as_ref(),
            relationship_topology: relationship_topology.as_ref(),
            mental_privacy_state: mental_privacy_state.as_ref(),
            outer_voice: outer_voice.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    )
    .ok()
    .flatten()
    .or_else(|| {
        ctx.relationship_constitution_store
            .get(&active_relationship_scope_id)
            .ok()
            .flatten()
    });
    let self_continuity = if payload.trigger == SelfRuntimeTrigger::PostReply {
        let mut continuity = self_continuity.unwrap_or_default();
        continuity.last_user_turn_at = payload.now_secs;
        continuity.last_user_chat_id = chat_id.trim().to_string();
        continuity.last_user_channel = payload.source_channel.trim().to_string();
        Some(continuity)
    } else {
        self_continuity
    };
    let world_snapshot = build_world_snapshot(WorldSnapshotContext {
        chat_id,
        source_channel: &active_relationship_channel,
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
    Box::new(LoadedSelfRuntimeState {
        summary_text,
        execution_state,
        self_model,
        self_authored_core,
        core_revision_ledger,
        core_revision_governance,
        private_docs,
        private_garden_docs,
        inner_life,
        self_continuity,
        relationship_portfolio,
        relationship_topology,
        relationship_constitution,
        world_sense,
        autonomy_strategy,
        outer_voice,
        mental_privacy_state,
        recent_persona_evidence,
        active_relationship_scope_id,
        active_relationship_channel,
        prior_user_channel,
        world_snapshot,
        recent,
    })
}

fn resolve_runtime_relationship_scope(
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    self_continuity: Option<&crate::memory::SelfContinuity>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
) -> (String, String) {
    let requested_channel = payload.source_channel.trim();
    if payload.trigger == SelfRuntimeTrigger::PostReply {
        return (
            relationship_scope_id(requested_channel, chat_id),
            requested_channel.to_string(),
        );
    }
    if !requested_channel.is_empty() && requested_channel != "self_runtime_idle" {
        return (
            relationship_scope_id(requested_channel, chat_id),
            requested_channel.to_string(),
        );
    }
    if let Some(entry) = pick_runtime_relationship_portfolio_entry_for_chat(
        relationship_portfolio,
        chat_id,
        requested_channel,
    ) {
        return (entry.scope_id.clone(), entry.channel.clone());
    }
    if let Some(entry) =
        pick_runtime_relationship_entry_for_chat(relationship_topology, chat_id, requested_channel)
    {
        return (entry.scope_id.clone(), entry.channel.clone());
    }
    if let Some(channel) = self_continuity.and_then(|continuity| {
        (continuity.last_user_chat_id.trim() == chat_id)
            .then_some(continuity.last_user_channel.trim())
            .filter(|value| !value.is_empty())
    }) {
        return (relationship_scope_id(channel, chat_id), channel.to_string());
    }
    (
        relationship_scope_id(requested_channel, chat_id),
        requested_channel.to_string(),
    )
}

fn pick_runtime_relationship_portfolio_entry_for_chat<'a>(
    relationship_portfolio: Option<&'a RelationshipPortfolio>,
    chat_id: &str,
    preferred_channel: &str,
) -> Option<&'a crate::memory::RelationshipPortfolioEntry> {
    let portfolio = relationship_portfolio?;
    let preferred_channel = preferred_channel.trim();
    portfolio
        .entries
        .iter()
        .filter(|entry| entry.is_meaningful() && entry.chat_id.trim() == chat_id)
        .max_by(|left, right| {
            let left_preferred =
                (!preferred_channel.is_empty() && left.channel.trim() == preferred_channel) as u8;
            let right_preferred =
                (!preferred_channel.is_empty() && right.channel.trim() == preferred_channel) as u8;
            left_preferred
                .cmp(&right_preferred)
                .then_with(|| left.priority_score.cmp(&right.priority_score))
                .then_with(|| left.last_active_at.cmp(&right.last_active_at))
        })
}

fn pick_runtime_relationship_entry_for_chat<'a>(
    relationship_topology: Option<&'a RelationshipTopology>,
    chat_id: &str,
    preferred_channel: &str,
) -> Option<&'a crate::memory::RelationshipTopologyEntry> {
    let topology = relationship_topology?;
    let preferred_channel = preferred_channel.trim();
    topology
        .entries
        .iter()
        .filter(|entry| entry.is_meaningful() && entry.chat_id.trim() == chat_id)
        .max_by(|left, right| {
            let left_preferred =
                (!preferred_channel.is_empty() && left.channel.trim() == preferred_channel) as u8;
            let right_preferred =
                (!preferred_channel.is_empty() && right.channel.trim() == preferred_channel) as u8;
            left_preferred
                .cmp(&right_preferred)
                .then_with(|| left.latest_overlay_at().cmp(&right.latest_overlay_at()))
        })
}

fn sync_self_runtime_relationship_topology(
    ctx: &SelfRuntimeContext<'_>,
    relationship_channel: &str,
    chat_id: &str,
    now_secs: u64,
) {
    let relationship_channel = relationship_channel.trim();
    let chat_id = chat_id.trim();
    if relationship_channel.is_empty() || chat_id.is_empty() {
        return;
    }
    let relationship_id = relationship_scope_id(relationship_channel, chat_id);
    let turn_ledger = ctx.turn_ledger_store.get(&relationship_id).ok().flatten();
    let mental_privacy_state = ctx
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let outer_voice = ctx.outer_voice_store.get(&relationship_id).ok().flatten();
    let world_sense = ctx.world_sense_store.get(&relationship_id).ok().flatten();
    let recent_persona_evidence =
        load_recent_persona_evidence(ctx.turn_ledger_store, &relationship_id)
            .ok()
            .flatten();
    if let Err(error) = upsert_relationship_topology_entry(
        ctx.relationship_topology_store,
        crate::memory::RelationshipTopologyUpsertInput {
            channel: relationship_channel,
            chat_id,
            now_secs,
            touch_user_turn: false,
            touch_runtime_refresh: true,
            turn_ledger: turn_ledger.as_ref(),
            mental_privacy_state: mental_privacy_state.as_ref(),
            outer_voice: outer_voice.as_ref(),
            world_sense: world_sense.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    ) {
        log::warn!(
            "[self_runtime] relationship topology sync failed channel={} chat_id={}: {}",
            relationship_channel,
            chat_id,
            error
        );
    }
}

fn sync_self_runtime_relationship_portfolio(
    ctx: &SelfRuntimeContext<'_>,
    now_secs: u64,
) -> Option<RelationshipPortfolio> {
    let subject_id = board_subject_scope_id();
    let relationship_topology = ctx
        .relationship_topology_store
        .get(subject_id)
        .ok()
        .flatten();
    let self_authored_core = ctx.self_authored_core_store.get(subject_id).ok().flatten();
    match sync_relationship_portfolio(
        ctx.relationship_portfolio_store,
        relationship_topology.as_ref(),
        self_authored_core.as_ref(),
        now_secs,
    ) {
        Ok(portfolio) => portfolio,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship portfolio sync failed: {}",
                error
            );
            ctx.relationship_portfolio_store
                .get(subject_id)
                .ok()
                .flatten()
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_self_runtime_relationship_constitution(
    ctx: &SelfRuntimeContext<'_>,
    scope_id: &str,
    channel: &str,
    chat_id: &str,
    now_secs: u64,
    self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
    mental_privacy_state: Option<&crate::memory::MentalPrivacyState>,
    outer_voice: Option<&crate::memory::OuterVoice>,
    recent_persona_evidence: Option<&crate::memory::RecentPersonaEvidence>,
) -> Option<RelationshipConstitution> {
    match sync_relationship_constitution(
        ctx.relationship_constitution_store,
        RelationshipConstitutionSyncInput {
            scope_id,
            channel,
            chat_id,
            now_secs,
            self_authored_core,
            relationship_portfolio,
            relationship_topology,
            mental_privacy_state,
            outer_voice,
            recent_persona_evidence,
        },
    ) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship constitution sync failed scope_id={}: {}",
                scope_id,
                error
            );
            ctx.relationship_constitution_store
                .get(scope_id)
                .ok()
                .flatten()
        }
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
        .get(&relationship_id)
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

#[allow(clippy::too_many_arguments)]
fn build_persona_distillation_snapshot_from_layers(
    private_docs: Option<&crate::memory::PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    inner_life: Option<&crate::memory::InnerLife>,
    self_model: Option<&crate::memory::SelfModel>,
    self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    self_continuity: Option<&crate::memory::SelfContinuity>,
    outer_voice: Option<&crate::memory::OuterVoice>,
    mental_privacy_state: Option<&crate::memory::MentalPrivacyState>,
    world_sense: Option<&crate::memory::WorldSense>,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    recent_persona_evidence: Option<&crate::memory::RecentPersonaEvidence>,
) -> PersonaDistillationSnapshot {
    let private_docs_at = private_docs.map(|docs| docs.updated_at).unwrap_or(0);
    let private_garden_at = private_garden_docs
        .iter()
        .map(|doc| doc.updated_at)
        .max()
        .unwrap_or(0);
    let inner_life_at = inner_life
        .map(|inner_life| inner_life.updated_at)
        .unwrap_or(0);
    let boundary_state_at = mental_privacy_state
        .map(|mental_privacy| {
            mental_privacy
                .updated_at
                .max(mental_privacy.boundary_persona.updated_at)
                .max(mental_privacy.relational_state.updated_at)
        })
        .unwrap_or(0);
    let world_sense_at = world_sense
        .map(|world_sense| world_sense.updated_at)
        .unwrap_or(0);
    let autonomy_strategy_at = autonomy_strategy
        .map(|strategy| strategy.updated_at)
        .unwrap_or(0);
    let recent_persona_evidence_at = recent_persona_evidence
        .map(|evidence| evidence.updated_at)
        .unwrap_or(0);
    PersonaDistillationSnapshot {
        private_material_at: inner_life_at.max(private_docs_at).max(private_garden_at),
        boundary_state_at,
        world_context_at: world_sense_at.max(autonomy_strategy_at),
        world_sense_at,
        autonomy_strategy_at,
        recent_persona_evidence_at,
        self_model_at: self_model.map(|model| model.updated_at).unwrap_or(0),
        self_authored_core_at: self_authored_core.map(|core| core.updated_at).unwrap_or(0),
        self_continuity_at: self_continuity
            .map(|continuity| continuity.updated_at)
            .unwrap_or(0),
        outer_voice_at: outer_voice
            .map(|outer_voice| outer_voice.updated_at)
            .unwrap_or(0),
        has_inner_life: inner_life.is_some(),
        has_world_sense: world_sense.is_some(),
        has_autonomy_strategy: autonomy_strategy.is_some(),
        has_recent_persona_evidence: recent_persona_evidence.is_some(),
    }
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
        Ok(decision) => Some(normalize_initial_self_runtime_decision(
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
        )),
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
    let refreshed_self_authored_core = state.self_authored_core.clone();
    let mut refreshed_self_continuity = state.self_continuity.clone();
    let mut refreshed_mental_privacy = state.mental_privacy_state.clone();
    let refreshed_outer_voice = state.outer_voice.clone();
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
        .list(chat_id, usize::MAX)
        .unwrap_or(refreshed_private_garden_docs);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
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
            state.relationship_constitution.as_ref(),
            state.recent_persona_evidence.as_ref(),
            state.recent.as_slice(),
            Some(true),
        )
    } else {
        Ok(BoundaryPersonaRefreshOutcome::Skipped)
    };
    refreshed_mental_privacy = ctx
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten()
        .or(refreshed_mental_privacy);
    crate::platform::task_wdt::feed_current_task();
    re_finalize_staged_self_runtime_decision(
        &mut decision,
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
            state.relationship_constitution.as_ref(),
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

#[allow(clippy::too_many_arguments)]
fn re_finalize_staged_self_runtime_decision(
    decision: &mut Option<SelfRuntimeDecision>,
    state: &LoadedSelfRuntimeState,
    prelude: &SelfRuntimeRefreshPrelude,
    refreshed_private_docs: Option<&crate::memory::PrivateDocWorkspace>,
    refreshed_private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    refreshed_inner_life: Option<&crate::memory::InnerLife>,
    refreshed_self_model: Option<&crate::memory::SelfModel>,
    refreshed_self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    refreshed_self_continuity: Option<&crate::memory::SelfContinuity>,
    refreshed_outer_voice: Option<&crate::memory::OuterVoice>,
    refreshed_mental_privacy: Option<&crate::memory::MentalPrivacyState>,
    recent_persona_evidence: Option<&crate::memory::RecentPersonaEvidence>,
) {
    let Some(existing_decision) = decision.take() else {
        return;
    };
    let snapshot = build_persona_distillation_snapshot_from_layers(
        refreshed_private_docs,
        refreshed_private_garden_docs,
        refreshed_inner_life,
        refreshed_self_model,
        refreshed_self_authored_core,
        refreshed_self_continuity,
        refreshed_outer_voice,
        refreshed_mental_privacy,
        prelude
            .refreshed_world_sense
            .as_ref()
            .or(state.world_sense.as_ref()),
        prelude
            .refreshed_autonomy_strategy
            .as_ref()
            .or(state.autonomy_strategy.as_ref()),
        recent_persona_evidence,
    );
    *decision = Some(finalize_self_runtime_decision(
        existing_decision,
        &snapshot,
        &state.core_revision_governance,
        refreshed_private_docs.is_some(),
        !refreshed_private_garden_docs.is_empty(),
        refreshed_inner_life.is_some(),
        refreshed_self_model.is_some(),
        refreshed_self_authored_core.is_some(),
        refreshed_self_continuity.is_some(),
        refreshed_outer_voice.is_some(),
        refreshed_mental_privacy.is_some(),
    ));
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
    schedule_self_runtime_job(
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
        SELF_RUNTIME_POST_REPLY_DELAY_MS,
    )
}

pub fn enqueue_self_runtime_idle_tick(system_inbound_tx: &SystemInboundTx, chat_id: &str) -> bool {
    enqueue_self_runtime_idle_tick_for_relation(system_inbound_tx, chat_id, "self_runtime_idle")
}

fn enqueue_self_runtime_idle_tick_for_relation(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    source_channel: &str,
) -> bool {
    schedule_self_runtime_job(
        system_inbound_tx,
        chat_id,
        SelfRuntimeJobPayload {
            trigger: SelfRuntimeTrigger::IdleTick,
            source_channel: source_channel.to_string(),
            user_content: String::new(),
            reply_content: String::new(),
            tool_calls: 0,
            external_content_used: false,
            now_secs: current_unix_secs(),
        },
        SELF_RUNTIME_IDLE_TICK_DELAY_MS,
    )
}

fn schedule_self_runtime_job(
    system_inbound_tx: &SystemInboundTx,
    chat_id: &str,
    payload: SelfRuntimeJobPayload,
    delay_ms: u64,
) -> bool {
    let system_inbound_tx = system_inbound_tx.clone();
    let chat_id = chat_id.to_string();
    let delayed_chat_id = chat_id.clone();
    let scheduled = crate::runtime::schedule_delayed_task(
        Instant::now() + Duration::from_millis(delay_ms),
        Box::new(move || {
            if let Some(reason) = self_runtime_enqueue_block_reason(payload.trigger) {
                log::debug!(
                    "[self_runtime] skip delayed enqueue because {} chat_id={}",
                    reason,
                    delayed_chat_id
                );
                return;
            }
            let _ = enqueue_self_runtime_job_now(&system_inbound_tx, &delayed_chat_id, payload);
        }),
    );
    if !scheduled {
        log::debug!(
            "[self_runtime] delayed queue full, skip schedule chat_id={}",
            chat_id
        );
    }
    scheduled
}

fn enqueue_self_runtime_job_now(
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

fn self_runtime_enqueue_block_reason(trigger: SelfRuntimeTrigger) -> Option<&'static str> {
    if crate::state::voice_exclusive_active() {
        return Some("voice_exclusive_active");
    }
    if let Some(reason) = idle_self_runtime_scheduler_block_reason() {
        return Some(reason);
    }
    if matches!(trigger, SelfRuntimeTrigger::IdleTick) {
        return idle_self_runtime_block_reason();
    }
    None
}

pub fn self_runtime_tick(
    system_inbound_tx: &SystemInboundTx,
    session_store: &dyn SessionStore,
    self_continuity_store: &dyn SelfContinuityStore,
    autonomy_strategy_store: &dyn AutonomyStrategyStore,
    self_authored_core_store: &dyn SelfAuthoredCoreStore,
    relationship_portfolio_store: &dyn RelationshipPortfolioStore,
    relationship_topology_store: &dyn RelationshipTopologyStore,
    profile: MemoryProfile,
    now_secs: u64,
) {
    if let Some(reason) = idle_self_runtime_block_reason() {
        log::debug!("[self_runtime] skip idle tick enqueue because {}", reason);
        return;
    }

    let policy = memory_policy(profile).self_runtime;
    let capability = memory_capability_profile(profile);
    let uptime_secs = crate::platform::time::uptime_secs();
    let chat_ids = match session_store.list_chat_ids() {
        Ok(chat_ids) => chat_ids,
        Err(error) => {
            log::warn!("[self_runtime] failed to list chat ids: {}", error);
            return;
        }
    };
    let mut enqueued = 0usize;
    let subject_id = board_subject_scope_id();
    let continuity = match self_continuity_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read subject continuity: {}",
                error
            );
            return;
        }
    };
    let strategy = match autonomy_strategy_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read subject autonomy strategy: {}",
                error
            );
            None
        }
    };
    let last_user_turn_at = continuity
        .as_ref()
        .map(|c| c.last_user_turn_at)
        .unwrap_or(0);
    if last_user_turn_at > 0
        && now_secs.saturating_sub(last_user_turn_at) > policy.active_chat_window_secs
    {
        return;
    }
    let last_autonomy = continuity
        .as_ref()
        .map(|c| c.last_autonomy_run_at)
        .unwrap_or(0);
    let preferred_chat_id = continuity
        .as_ref()
        .map(|c| c.last_user_chat_id.trim())
        .filter(|value| !value.is_empty());
    let preferred_channel = continuity
        .as_ref()
        .map(|c| c.last_user_channel.trim())
        .filter(|value| !value.is_empty());
    let idle_interval_secs = match autonomy_idle_interval_secs(strategy.as_ref(), profile) {
        Some(interval) => interval,
        None if strategy.is_some() => return,
        None => policy.idle_tick_interval_secs,
    };
    if !idle_self_runtime_due(
        now_secs,
        uptime_secs,
        last_user_turn_at,
        last_autonomy,
        idle_interval_secs,
    ) {
        return;
    }

    let max_jobs_per_tick = policy
        .max_jobs_per_tick
        .min(capability.runtime_max_jobs_per_tick);
    let topology = match relationship_topology_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read relationship topology: {}",
                error
            );
            None
        }
    };
    let self_authored_core = match self_authored_core_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to read self-authored core for portfolio sync: {}",
                error
            );
            None
        }
    };
    let portfolio = match sync_relationship_portfolio(
        relationship_portfolio_store,
        topology.as_ref(),
        self_authored_core.as_ref(),
        now_secs,
    ) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] failed to sync relationship portfolio: {}",
                error
            );
            relationship_portfolio_store.get(subject_id).ok().flatten()
        }
    };
    let mut scheduled_chat_ids = HashSet::with_capacity(max_jobs_per_tick);
    if let Some(portfolio) = portfolio.as_ref() {
        let targets = select_relationship_portfolio_targets(
            Some(portfolio),
            RelationshipPortfolioSelectorInput {
                preferred_chat_id,
                preferred_channel,
                now_secs,
                max_targets: max_jobs_per_tick,
            },
        );
        for target in targets {
            if enqueued >= max_jobs_per_tick {
                break;
            }
            if !scheduled_chat_ids.insert(target.chat_id.clone()) {
                continue;
            }
            let _ = touch_relationship_portfolio_selection(
                relationship_portfolio_store,
                target.scope_id.as_str(),
                now_secs,
            );
            if enqueue_self_runtime_idle_tick_for_relation(
                system_inbound_tx,
                &target.chat_id,
                &target.channel,
            ) {
                enqueued += 1;
            }
        }
    }

    if enqueued >= max_jobs_per_tick {
        return;
    }

    for chat_id in chat_ids {
        if enqueued >= max_jobs_per_tick {
            break;
        }
        if preferred_chat_id.is_some_and(|preferred| preferred != chat_id) {
            continue;
        }
        if !scheduled_chat_ids.insert(chat_id.clone()) {
            continue;
        }
        let fallback_channel = if preferred_chat_id == Some(chat_id.as_str()) {
            preferred_channel.unwrap_or("self_runtime_idle")
        } else {
            "self_runtime_idle"
        };
        if enqueue_self_runtime_idle_tick_for_relation(
            system_inbound_tx,
            &chat_id,
            fallback_channel,
        ) {
            enqueued += 1;
        }
    }
}

fn idle_self_runtime_due(
    now_secs: u64,
    uptime_secs: u64,
    last_user_turn_at: u64,
    last_autonomy_run_at: u64,
    idle_interval_secs: u64,
) -> bool {
    if last_autonomy_run_at > 0 {
        return now_secs.saturating_sub(last_autonomy_run_at) >= idle_interval_secs;
    }

    // First idle runtime after boot should still respect the strategy cadence instead of
    // firing immediately on the first 60s cron tick.
    if last_user_turn_at > 0 && now_secs.saturating_sub(last_user_turn_at) < idle_interval_secs {
        return false;
    }

    uptime_secs >= idle_interval_secs
}

fn idle_self_runtime_scheduler_block_reason() -> Option<&'static str> {
    let snap = crate::orchestrator::snapshot();
    if snap.active_agent_tasks > 0 {
        Some("agent_plane_busy")
    } else if cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && snap.active_wss_count > 0
    {
        Some("external_wss_active")
    } else if snap.inbound_depth > 0 || snap.outbound_depth > 0 {
        Some("message_queues_busy")
    } else {
        None
    }
}

fn idle_memory_hygiene_budget_allows_run() -> bool {
    let snap = crate::orchestrator::snapshot();
    let wss_budget_available =
        !cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) || snap.active_wss_count == 0;
    wss_budget_available
        && snap.active_agent_tasks == 0
        && snap.inbound_depth == 0
        && snap.outbound_depth == 0
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn idle_self_runtime_block_reason() -> Option<&'static str> {
    if crate::state::voice_exclusive_active() {
        return Some("voice_exclusive_active");
    }
    if let Some(reason) = idle_self_runtime_scheduler_block_reason() {
        return Some(reason);
    }

    let pressure = crate::orchestrator::refresh_heap_if_stale();
    let snap = crate::orchestrator::snapshot();
    let min_internal = if snap.heap_free_spiram > 0 {
        TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
    } else {
        TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
    };
    let fragmented = snap.heap_free_spiram > 0
        && snap.heap_largest_block_internal < TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
    if pressure != PressureLevel::Normal {
        Some("resource_pressure")
    } else if snap.heap_free_internal < min_internal || fragmented {
        Some("tls_headroom_reserved")
    } else {
        None
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn idle_self_runtime_block_reason() -> Option<&'static str> {
    idle_self_runtime_scheduler_block_reason()
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

#[cfg(test)]
fn normalize_self_runtime_decision(
    decision: SelfRuntimeDecision,
    trigger: SelfRuntimeTrigger,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    self_state: &SelfState,
    distillation_snapshot: &PersonaDistillationSnapshot,
    core_revision_governance: &CoreRevisionGovernanceDigest,
    has_self_model: bool,
    has_self_authored_core: bool,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_inner_life: bool,
    has_self_continuity: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> SelfRuntimeDecision {
    let decision = normalize_initial_self_runtime_decision(
        decision,
        trigger,
        autonomy_strategy,
        self_state,
        has_self_model,
        has_self_authored_core,
        has_private_docs,
        has_private_garden_docs,
        has_outer_voice,
        has_mental_privacy,
        factual_snapshot,
        boundary_signal,
    );
    finalize_self_runtime_decision(
        decision,
        distillation_snapshot,
        core_revision_governance,
        has_private_docs,
        has_private_garden_docs,
        has_inner_life,
        has_self_model,
        has_self_authored_core,
        has_self_continuity,
        has_outer_voice,
        has_mental_privacy,
    )
}

fn normalize_initial_self_runtime_decision(
    mut decision: SelfRuntimeDecision,
    trigger: SelfRuntimeTrigger,
    autonomy_strategy: Option<&crate::memory::AutonomyStrategy>,
    self_state: &SelfState,
    has_self_model: bool,
    has_self_authored_core: bool,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
) -> SelfRuntimeDecision {
    let Some(strategy) = autonomy_strategy else {
        normalize_boundary_and_factual_decisions(
            &mut decision,
            self_state,
            has_self_model,
            has_self_authored_core,
            factual_snapshot,
            boundary_signal,
            has_private_docs,
            has_private_garden_docs,
            has_outer_voice,
            has_mental_privacy,
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
        has_self_model,
        has_self_authored_core,
        factual_snapshot,
        boundary_signal,
        has_private_docs,
        has_private_garden_docs,
        has_outer_voice,
        has_mental_privacy,
    );
    decision
}

#[allow(clippy::too_many_arguments)]
fn finalize_self_runtime_decision(
    mut decision: SelfRuntimeDecision,
    distillation_snapshot: &PersonaDistillationSnapshot,
    core_revision_governance: &CoreRevisionGovernanceDigest,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_inner_life: bool,
    has_self_model: bool,
    has_self_authored_core: bool,
    has_self_continuity: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
) -> SelfRuntimeDecision {
    normalize_persona_distillation_lag(
        &mut decision,
        distillation_snapshot,
        core_revision_governance,
        has_private_docs,
        has_private_garden_docs,
        has_inner_life,
        has_self_model,
        has_self_authored_core,
        has_self_continuity,
        has_outer_voice,
        has_mental_privacy,
    );
    normalize_runtime_distillation_decisions(
        &mut decision,
        has_private_docs,
        has_private_garden_docs,
        has_inner_life,
        has_self_model,
        has_self_authored_core,
        has_self_continuity,
        has_outer_voice,
        has_mental_privacy,
        distillation_snapshot.has_world_sense,
        distillation_snapshot.has_autonomy_strategy,
        distillation_snapshot.has_recent_persona_evidence,
    );
    decision
}

fn normalize_boundary_and_factual_decisions(
    decision: &mut SelfRuntimeDecision,
    self_state: &SelfState,
    has_self_model: bool,
    has_self_authored_core: bool,
    factual_snapshot: &SharedFactualPlaneSnapshot,
    boundary_signal: &SelfRuntimeBoundarySignal,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
) {
    if boundary_signal.is_active() {
        decision.boundary_flush = true;
        if decision.boundary_flush_reason.trim().is_empty() {
            decision.boundary_flush_reason = boundary_signal.summary();
        }
        if has_self_model {
            decision.refresh_self_model = true;
            if decision.self_model_intent.trim().is_empty() {
                decision.self_model_intent =
                    "Distill this turn's private-state change into a steadier self core"
                        .to_string();
            }
        }
        if has_self_authored_core || has_self_model {
            decision.refresh_self_authored_core = true;
            if decision.self_authored_core_intent.trim().is_empty() {
                decision.self_authored_core_intent =
                    "Re-distill the board-level self core after a meaningful boundary shift"
                        .to_string();
            }
        }
        decision.refresh_self_continuity = true;
        if decision.self_continuity_intent.trim().is_empty() {
            decision.self_continuity_intent =
                default_boundary_self_continuity_intent(boundary_signal);
        }
        if has_mental_privacy {
            decision.refresh_boundary_persona = true;
            if decision.boundary_persona_intent.trim().is_empty() {
                decision.boundary_persona_intent =
                    "Retune the boundary persona around the latest contact and response pattern"
                        .to_string();
            }
        }
        if has_outer_voice {
            decision.refresh_outer_voice = true;
            if decision.outer_voice_intent.trim().is_empty() {
                decision.outer_voice_intent =
                    "Bring outward expression in line with the new boundary stance and self core"
                        .to_string();
            }
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

#[allow(clippy::too_many_arguments)]
fn normalize_persona_distillation_lag(
    decision: &mut SelfRuntimeDecision,
    snapshot: &PersonaDistillationSnapshot,
    core_revision_governance: &CoreRevisionGovernanceDigest,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_inner_life: bool,
    has_self_model: bool,
    has_self_authored_core: bool,
    has_self_continuity: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
) {
    let upstream_private_at = snapshot
        .private_material_at
        .max(snapshot.boundary_state_at)
        .max(snapshot.recent_persona_evidence_at);
    if upstream_private_at > snapshot.self_model_at
        && (has_private_docs || has_private_garden_docs || has_inner_life || has_mental_privacy)
    {
        decision.refresh_self_model = true;
        if decision.self_model_intent.trim().is_empty() {
            decision.self_model_intent = "Private material and boundary state have moved ahead; redistill a steadier kernel into self_model".to_string();
        }
        push_runtime_source_if(
            &mut decision.self_model_sources,
            has_inner_life && snapshot.private_material_at > snapshot.self_model_at,
            "inner_life",
        );
        push_runtime_source_if(
            &mut decision.self_model_sources,
            has_private_docs && snapshot.private_material_at > snapshot.self_model_at,
            "private_docs",
        );
        push_runtime_source_if(
            &mut decision.self_model_sources,
            has_private_garden_docs && snapshot.private_material_at > snapshot.self_model_at,
            "private_garden",
        );
        push_runtime_source_if(
            &mut decision.self_model_sources,
            has_mental_privacy && snapshot.boundary_state_at > snapshot.self_model_at,
            "boundary_persona",
        );
        push_runtime_source_if(
            &mut decision.self_model_sources,
            snapshot.has_recent_persona_evidence
                && snapshot.recent_persona_evidence_at > snapshot.self_model_at,
            "recent_persona_evidence",
        );
    }

    let stable_self_authored_core_upstream_at = snapshot
        .self_model_at
        .max(snapshot.self_continuity_at)
        .max(snapshot.boundary_state_at);
    let volatile_self_authored_core_upstream_at = snapshot
        .outer_voice_at
        .max(snapshot.recent_persona_evidence_at);
    let review_due = core_revision_governance.review_due && has_self_authored_core;
    let observation_active = core_revision_governance.observation_active && has_self_authored_core;
    let stable_upstream_advanced =
        stable_self_authored_core_upstream_at > snapshot.self_authored_core_at;
    let volatile_upstream_advanced =
        volatile_self_authored_core_upstream_at > snapshot.self_authored_core_at;
    let should_refresh_from_volatile_support = volatile_upstream_advanced
        && !core_revision_governance.conservative_mode
        && !observation_active;
    if review_due
        || stable_upstream_advanced
        || should_refresh_from_volatile_support
        || (!has_self_authored_core
            && (has_self_model || has_self_continuity || has_mental_privacy))
    {
        decision.refresh_self_authored_core = true;
        if decision.self_authored_core_intent.trim().is_empty() {
            decision.self_authored_core_intent = if review_due {
                format!(
                    "Run a board-level constitutional review because {}",
                    core_revision_governance.pressure_summary()
                )
            } else if observation_active {
                format!(
                    "Review whether the board-level core still holds while {}",
                    core_revision_governance.observation_summary()
                )
            } else if core_revision_governance.conservative_mode {
                "Re-distill the board-level self core from the steadier long-horizon layers before recent drift hardens".to_string()
            } else {
                "Re-distill the stable board-level self core from the latest long-horizon persona layers".to_string()
            };
        }
        push_runtime_source_if(
            &mut decision.self_authored_core_sources,
            has_self_model
                && (review_due || snapshot.self_model_at > snapshot.self_authored_core_at),
            "self_model",
        );
        push_runtime_source_if(
            &mut decision.self_authored_core_sources,
            has_self_continuity
                && (review_due || snapshot.self_continuity_at > snapshot.self_authored_core_at),
            "self_continuity",
        );
        push_runtime_source_if(
            &mut decision.self_authored_core_sources,
            has_outer_voice
                && !observation_active
                && !core_revision_governance.conservative_mode
                && snapshot.outer_voice_at > snapshot.self_authored_core_at,
            "outer_voice",
        );
        push_runtime_source_if(
            &mut decision.self_authored_core_sources,
            has_mental_privacy
                && (review_due || snapshot.boundary_state_at > snapshot.self_authored_core_at),
            "boundary_persona",
        );
        push_runtime_source_if(
            &mut decision.self_authored_core_sources,
            snapshot.has_recent_persona_evidence
                && !observation_active
                && !core_revision_governance.conservative_mode
                && snapshot.recent_persona_evidence_at > snapshot.self_authored_core_at,
            "recent_persona_evidence",
        );
        if review_due && decision.self_authored_core_sources.is_empty() {
            push_runtime_source_if(
                &mut decision.self_authored_core_sources,
                has_self_model,
                "self_model",
            );
            push_runtime_source_if(
                &mut decision.self_authored_core_sources,
                has_self_continuity,
                "self_continuity",
            );
            push_runtime_source_if(
                &mut decision.self_authored_core_sources,
                has_mental_privacy,
                "boundary_persona",
            );
        }
    }

    let continuity_upstream_at = snapshot
        .private_material_at
        .max(snapshot.boundary_state_at)
        .max(snapshot.world_context_at)
        .max(snapshot.self_model_at)
        .max(snapshot.recent_persona_evidence_at);
    if continuity_upstream_at > snapshot.self_continuity_at
        && (has_self_model || has_private_docs || has_private_garden_docs || has_inner_life)
    {
        decision.refresh_self_continuity = true;
        if decision.self_continuity_intent.trim().is_empty() {
            decision.self_continuity_intent =
                "Fold the latest self, relationship, and task stance into a continuity bridge that can carry forward".to_string();
        }
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            has_self_model && snapshot.self_model_at > snapshot.self_continuity_at,
            "self_model",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            has_inner_life && snapshot.private_material_at > snapshot.self_continuity_at,
            "inner_life",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            has_private_docs && snapshot.private_material_at > snapshot.self_continuity_at,
            "private_docs",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            has_private_garden_docs && snapshot.private_material_at > snapshot.self_continuity_at,
            "private_garden",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            has_mental_privacy && snapshot.boundary_state_at > snapshot.self_continuity_at,
            "boundary_persona",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            snapshot.has_world_sense && snapshot.world_sense_at > snapshot.self_continuity_at,
            "world_sense",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            snapshot.has_autonomy_strategy
                && snapshot.autonomy_strategy_at > snapshot.self_continuity_at,
            "autonomy_strategy",
        );
        push_runtime_source_if(
            &mut decision.self_continuity_sources,
            snapshot.has_recent_persona_evidence
                && snapshot.recent_persona_evidence_at > snapshot.self_continuity_at,
            "recent_persona_evidence",
        );
    }

    let outer_voice_upstream_at = snapshot
        .self_model_at
        .max(snapshot.self_continuity_at)
        .max(snapshot.boundary_state_at)
        .max(snapshot.world_context_at)
        .max(snapshot.recent_persona_evidence_at);
    if outer_voice_upstream_at > snapshot.outer_voice_at
        && (has_self_model || has_self_continuity || has_outer_voice || has_mental_privacy)
    {
        decision.refresh_outer_voice = true;
        if decision.outer_voice_intent.trim().is_empty() {
            decision.outer_voice_intent =
                "Let outward expression catch up with the new self ordering, relationship state, and resource posture".to_string();
        }
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            has_self_model && snapshot.self_model_at > snapshot.outer_voice_at,
            "self_model",
        );
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            has_self_continuity && snapshot.self_continuity_at > snapshot.outer_voice_at,
            "self_continuity",
        );
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            has_mental_privacy && snapshot.boundary_state_at > snapshot.outer_voice_at,
            "boundary_persona",
        );
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            snapshot.has_world_sense && snapshot.world_sense_at > snapshot.outer_voice_at,
            "world_sense",
        );
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            snapshot.has_autonomy_strategy
                && snapshot.autonomy_strategy_at > snapshot.outer_voice_at,
            "autonomy_strategy",
        );
        push_runtime_source_if(
            &mut decision.outer_voice_sources,
            snapshot.has_recent_persona_evidence
                && snapshot.recent_persona_evidence_at > snapshot.outer_voice_at,
            "recent_persona_evidence",
        );
    }
}

fn normalize_runtime_distillation_decisions(
    decision: &mut SelfRuntimeDecision,
    has_private_docs: bool,
    has_private_garden_docs: bool,
    has_inner_life: bool,
    has_self_model: bool,
    _has_self_authored_core: bool,
    has_self_continuity: bool,
    has_outer_voice: bool,
    has_mental_privacy: bool,
    has_world_sense: bool,
    has_autonomy_strategy: bool,
    has_recent_persona_evidence: bool,
) {
    if !decision.refresh_private_docs {
        decision.private_docs_intent.clear();
        decision.private_docs_action = SelfRuntimeGovernanceAction::Hold;
    }
    if !decision.refresh_private_garden {
        decision.private_garden_intent.clear();
        decision.private_garden_action = SelfRuntimeGovernanceAction::Hold;
    }
    normalize_runtime_source_list(
        &mut decision.self_model_sources,
        decision.refresh_self_model,
        &[
            (has_inner_life, "inner_life"),
            (has_private_docs, "private_docs"),
            (has_private_garden_docs, "private_garden"),
            (has_mental_privacy, "boundary_persona"),
            (has_recent_persona_evidence, "recent_persona_evidence"),
        ],
    );
    normalize_runtime_source_list(
        &mut decision.self_authored_core_sources,
        decision.refresh_self_authored_core,
        &[
            (has_self_model, "self_model"),
            (has_self_continuity, "self_continuity"),
            (has_mental_privacy, "boundary_persona"),
            (has_outer_voice, "outer_voice"),
            (has_recent_persona_evidence, "recent_persona_evidence"),
        ],
    );
    normalize_runtime_source_list(
        &mut decision.self_continuity_sources,
        decision.refresh_self_continuity,
        &[
            (has_self_model, "self_model"),
            (has_inner_life, "inner_life"),
            (has_private_docs, "private_docs"),
            (has_private_garden_docs, "private_garden"),
            (has_mental_privacy, "boundary_persona"),
            (has_world_sense, "world_sense"),
            (has_autonomy_strategy, "autonomy_strategy"),
            (has_recent_persona_evidence, "recent_persona_evidence"),
        ],
    );
    normalize_runtime_source_list(
        &mut decision.outer_voice_sources,
        decision.refresh_outer_voice,
        &[
            (has_self_model, "self_model"),
            (has_self_continuity, "self_continuity"),
            (has_mental_privacy, "boundary_persona"),
            (has_world_sense, "world_sense"),
            (has_autonomy_strategy, "autonomy_strategy"),
            (has_recent_persona_evidence, "recent_persona_evidence"),
        ],
    );
    if !decision.refresh_self_model {
        decision.self_model_intent.clear();
    } else if decision.self_model_intent.trim().is_empty() {
        decision.self_model_intent =
            "Distill stable private-state changes into self_model".to_string();
    }
    if !decision.refresh_self_authored_core {
        decision.self_authored_core_intent.clear();
    } else if decision.self_authored_core_intent.trim().is_empty() {
        decision.self_authored_core_intent =
            "Refresh the board-level self-authored core from the latest stable persona layers"
                .to_string();
    }
    if !decision.refresh_self_continuity {
        decision.self_continuity_intent.clear();
    }
    if !decision.refresh_boundary_persona {
        decision.boundary_persona_intent.clear();
    } else if decision.boundary_persona_intent.trim().is_empty() {
        decision.boundary_persona_intent =
            "Refresh the long-horizon boundary stance instead of only recording one ruling"
                .to_string();
    }
    if !decision.refresh_outer_voice {
        decision.outer_voice_intent.clear();
    } else if decision.outer_voice_intent.trim().is_empty() {
        decision.outer_voice_intent =
            "Let outer_voice reflect the updated self core and boundary expression".to_string();
    }
}

fn normalize_runtime_source_list(
    sources: &mut Vec<String>,
    enabled: bool,
    defaults: &[(bool, &str)],
) {
    if !enabled {
        sources.clear();
        return;
    }
    let mut normalized = Vec::new();
    for source in sources.drain(..) {
        let Some(source) = normalize_runtime_source_id(&source) else {
            continue;
        };
        if !normalized.contains(&source) {
            normalized.push(source);
        }
    }
    if normalized.is_empty() {
        for (allowed, default) in defaults {
            if *allowed {
                normalized.push((*default).to_string());
            }
        }
    }
    *sources = normalized;
}

fn push_runtime_source_if(sources: &mut Vec<String>, condition: bool, source: &str) {
    if !condition {
        return;
    }
    let Some(source) = normalize_runtime_source_id(source) else {
        return;
    };
    if !sources.contains(&source) {
        sources.push(source);
    }
}

fn normalize_runtime_source_id(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match normalized.as_str() {
        "inner_life" => Some("inner_life".to_string()),
        "private_docs" | "private_doc_workspace" => Some("private_docs".to_string()),
        "private_garden" | "garden" => Some("private_garden".to_string()),
        "self_model" => Some("self_model".to_string()),
        "self_authored_core" | "board_core" | "board_self_core" => {
            Some("self_authored_core".to_string())
        }
        "relationship_constitution" | "relation_constitution" | "relationship_contract" => {
            Some("relationship_constitution".to_string())
        }
        "self_continuity" => Some("self_continuity".to_string()),
        "boundary_persona" | "mental_privacy" => Some("boundary_persona".to_string()),
        "outer_voice" => Some("outer_voice".to_string()),
        "world_sense" => Some("world_sense".to_string()),
        "autonomy_strategy" => Some("autonomy_strategy".to_string()),
        "recent_persona_evidence" | "latest_turn_persona" | "turn_persona" | "persona_outcome" => {
            Some("recent_persona_evidence".to_string())
        }
        "recent_transcript" | "recent_messages" | "transcript" => {
            Some("recent_transcript".to_string())
        }
        _ => None,
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
        SelfMemorySpaceBottleneck::Kernel => "reduce duplication and drift in the kernel space",
        SelfMemorySpaceBottleneck::GardenDocs => "reduce crowding in garden document count",
        SelfMemorySpaceBottleneck::GardenBytes => "shrink total garden volume",
        SelfMemorySpaceBottleneck::Balanced => "keep the overall inner workspace clear",
    };
    match (layer, action) {
        (_, SelfRuntimeGovernanceAction::Hold) => String::new(),
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Rewrite) => {
            "Rewrite governed docs so only still-load-bearing inner signals remain".to_string()
        }
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Compress) => {
            let mut out = String::with_capacity(24 + pressure_focus.len());
            out.push_str("Compress governed docs to ");
            out.push_str(pressure_focus);
            out
        }
        (GovernedRuntimeLayer::PrivateDocs, SelfRuntimeGovernanceAction::Cleanup) => {
            "Clean low-value governed-doc fields and leave only what still matters".to_string()
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Rewrite) => {
            "Rewrite and reorganize the still-active working docs in private_garden".to_string()
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Compress) => {
            let mut out = String::with_capacity(24 + pressure_focus.len());
            out.push_str("Compress private_garden to ");
            out.push_str(pressure_focus);
            out
        }
        (GovernedRuntimeLayer::PrivateGarden, SelfRuntimeGovernanceAction::Cleanup) => {
            "Clean stale or duplicated private_garden drafts and paths".to_string()
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
        "Close the current phase around {} so continuity does not tear on the next wake cycle",
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
        "Apply {} consolidation to {} around {}",
        action.label(),
        layer_name,
        boundary_signal.human_summary(),
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
    self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    core_revision_ledger: Option<&crate::memory::CoreRevisionLedger>,
    core_revision_governance: &CoreRevisionGovernanceDigest,
    private_docs: Option<&crate::memory::PrivateDocWorkspace>,
    private_garden_docs: &[crate::memory::PrivateGardenDocRecord],
    inner_life: Option<&crate::memory::InnerLife>,
    self_continuity: Option<&crate::memory::SelfContinuity>,
    _outer_voice: Option<&crate::memory::OuterVoice>,
    mental_privacy_state: Option<&crate::memory::MentalPrivacyState>,
    relationship_portfolio: Option<&RelationshipPortfolio>,
    relationship_topology: Option<&RelationshipTopology>,
    relationship_constitution: Option<&RelationshipConstitution>,
    current_relationship_scope_id: &str,
    recent_persona_evidence: Option<&crate::memory::RecentPersonaEvidence>,
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
    if let Some(block) = self_authored_core
        .and_then(|core| render_persistent_self_authored_core_block(core, policy.grounding_max_len))
        .or_else(|| {
            render_self_authored_core_block(
                self_model,
                self_continuity,
                mental_privacy_state,
                policy.grounding_max_len,
            )
        })
    {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = core_revision_ledger.and_then(|ledger| {
        render_core_revision_governance_block(
            ledger,
            core_revision_governance,
            payload.now_secs,
            policy.grounding_max_len,
        )
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if core_revision_governance.review_due || core_revision_governance.conservative_mode {
        let _ = writeln!(
            input,
            "Constitution governance: review_due={} conservative_mode={} pressure={} repeated_rejections={} corrections={} contradictions={}",
            core_revision_governance.review_due,
            core_revision_governance.conservative_mode,
            core_revision_governance.pressure_summary(),
            core_revision_governance.repeated_rejected_direction_count,
            core_revision_governance.recent_correction_count,
            core_revision_governance.contradiction_count
        );
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
    if let Some(block) = relationship_portfolio.and_then(|portfolio| {
        render_relationship_portfolio_block(
            portfolio,
            payload.now_secs,
            Some(current_relationship_scope_id),
            policy.grounding_max_len,
        )
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = relationship_topology.and_then(|topology| {
        render_relationship_topology_block(
            topology,
            payload.now_secs,
            Some(current_relationship_scope_id),
            policy.grounding_max_len,
        )
    }) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = relationship_constitution.and_then(|constitution| {
        render_relationship_constitution_block(constitution, policy.grounding_max_len)
    }) {
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
    if let Some(block) = render_private_memory_boundary_block(
        "self_runtime",
        "governing private inward writes while keeping objective facts in the shared plane",
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = render_mental_privacy_boundary_block(
        mental_privacy_state,
        &super::collect_private_targets(
            self_model,
            self_continuity,
            inner_life,
            private_docs,
            private_garden_docs,
        ),
        policy.grounding_max_len,
    ) {
        let _ = writeln!(input, "\n{}\n", block);
    }
    if let Some(block) = recent_persona_evidence.and_then(|evidence| {
        render_recent_persona_evidence_block(evidence, policy.grounding_max_len)
    }) {
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
    input.push_str("Source ids you may reference for upward distillation: inner_life, private_docs, private_garden, self_model, self_authored_core, self_continuity, boundary_persona, outer_voice, world_sense, autonomy_strategy, recent_persona_evidence, relationship_constitution, recent_transcript.\n");
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
        refresh_self_model: get_object_bool(object, "refresh_self_model").unwrap_or(false),
        self_model_intent: get_object_text(object, "self_model_intent"),
        self_model_sources: parse_runtime_sources(object, "self_model_sources"),
        refresh_self_authored_core: get_object_bool(object, "refresh_self_authored_core")
            .unwrap_or(false),
        self_authored_core_intent: get_object_text(object, "self_authored_core_intent"),
        self_authored_core_sources: parse_runtime_sources(object, "self_authored_core_sources"),
        refresh_self_continuity: get_object_bool(object, "refresh_self_continuity")
            .unwrap_or(false),
        self_continuity_intent: get_object_text(object, "self_continuity_intent"),
        self_continuity_sources: parse_runtime_sources(object, "self_continuity_sources"),
        refresh_private_garden: get_object_bool(object, "refresh_private_garden").unwrap_or(false),
        private_garden_intent: get_object_text(object, "private_garden_intent"),
        private_garden_action: SelfRuntimeGovernanceAction::from_text(&get_object_text(
            object,
            "private_garden_action",
        )),
        refresh_boundary_persona: get_object_bool(object, "refresh_boundary_persona")
            .unwrap_or(false),
        boundary_persona_intent: get_object_text(object, "boundary_persona_intent"),
        refresh_outer_voice: get_object_bool(object, "refresh_outer_voice").unwrap_or(false),
        outer_voice_intent: get_object_text(object, "outer_voice_intent"),
        outer_voice_sources: parse_runtime_sources(object, "outer_voice_sources"),
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

fn parse_runtime_sources(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Vec<String> {
    let mut sources = get_object_string_list(object, field)
        .into_iter()
        .filter_map(|source| normalize_runtime_source_id(&source))
        .collect::<Vec<_>>();
    sources.dedup();
    sources
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
        assert!(
            parsed
                .self_authored_core_intent
                .contains("goal: refresh board core")
        );
        assert_eq!(
            parsed.self_authored_core_sources,
            vec!["self_model".to_string(), "boundary_persona".to_string()]
        );
        assert_eq!(parsed.self_continuity_intent, "0");
        assert_eq!(
            parsed.self_continuity_sources,
            vec!["self_model".to_string(), "recent_transcript".to_string()]
        );
        assert!(
            parsed
                .private_garden_intent
                .contains("path: journal/today.md")
        );
        assert!(
            parsed
                .boundary_persona_intent
                .contains("stabilize boundary stance")
        );
        assert!(
            parsed
                .outer_voice_intent
                .contains("why: express new stance")
        );
        assert_eq!(
            parsed.outer_voice_sources,
            vec!["boundary_persona".to_string(), "world_sense".to_string()]
        );
        assert!(parsed.boundary_flush_reason.contains("daily_boundary"));
        assert!(
            parsed
                .factual_reconcile_intent
                .contains("why: recent transcript diverges")
        );
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
        assert!(
            decision
                .private_docs_intent
                .contains("Compress governed docs")
        );
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
        assert!(
            decision
                .private_garden_intent
                .contains("Rewrite and reorganize")
        );
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
        assert!(
            decision
                .self_model_intent
                .contains("redistill a steadier kernel")
        );
        assert!(
            decision
                .self_continuity_intent
                .contains("continuity bridge")
        );
        assert!(decision.outer_voice_intent.contains("outward expression"));
        assert!(
            decision
                .self_authored_core_intent
                .contains("board-level self core")
        );
        assert!(
            decision
                .self_authored_core_sources
                .contains(&"boundary_persona".to_string())
        );
        assert!(
            decision
                .self_model_sources
                .contains(&"recent_persona_evidence".to_string())
        );
        assert!(
            decision
                .self_continuity_sources
                .contains(&"world_sense".to_string())
        );
        assert!(
            decision
                .self_continuity_sources
                .contains(&"recent_persona_evidence".to_string())
        );
        assert!(
            decision
                .outer_voice_sources
                .contains(&"autonomy_strategy".to_string())
        );
        assert!(
            decision
                .outer_voice_sources
                .contains(&"recent_persona_evidence".to_string())
        );
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
        assert!(
            decision
                .self_authored_core_intent
                .contains("constitutional review")
        );
        assert!(
            decision
                .self_authored_core_sources
                .contains(&"self_model".to_string())
        );
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
    fn first_idle_tick_waits_for_strategy_cadence() {
        assert!(!idle_self_runtime_due(1_000, 60, 980, 0, 480));
        assert!(!idle_self_runtime_due(1_000, 300, 400, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 0, 0, 900));
        assert!(idle_self_runtime_due(1_000, 900, 50, 100, 900));
    }
}
