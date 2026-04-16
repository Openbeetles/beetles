use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct PersonaDistillationSnapshot {
    pub(super) private_material_at: u64,
    pub(super) boundary_state_at: u64,
    pub(super) world_context_at: u64,
    pub(super) world_sense_at: u64,
    pub(super) autonomy_strategy_at: u64,
    pub(super) recent_persona_evidence_at: u64,
    pub(super) self_model_at: u64,
    pub(super) self_authored_core_at: u64,
    pub(super) self_continuity_at: u64,
    pub(super) outer_voice_at: u64,
    pub(super) has_inner_life: bool,
    pub(super) has_world_sense: bool,
    pub(super) has_autonomy_strategy: bool,
    pub(super) has_recent_persona_evidence: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GovernedRuntimeLayer {
    PrivateDocs,
    PrivateGarden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SelfRuntimeBoundaryReason {
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
pub(super) struct SelfRuntimeBoundarySignal {
    pub(super) reasons: Vec<SelfRuntimeBoundaryReason>,
}

impl SelfRuntimeBoundarySignal {
    pub(super) fn is_active(&self) -> bool {
        !self.reasons.is_empty()
    }

    pub(super) fn summary(&self) -> String {
        self.reasons
            .iter()
            .map(|reason| reason.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(super) fn human_summary(&self) -> String {
        self.reasons
            .iter()
            .map(|reason| reason.human_label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn unix_day_bucket(now_secs: u64) -> u64 {
    now_secs / 86_400
}

pub(super) fn detect_boundary_flush_signal(
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
        .map(|evidence| evidence.promotable_growth_updated_at())
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
        has_recent_persona_evidence: recent_persona_evidence
            .is_some_and(|evidence| evidence.has_promotable_growth_signals()),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn re_finalize_staged_self_runtime_decision(
    decision: &mut Option<SelfRuntimeDecision>,
    personality_governance_gate: &crate::memory::PersonalityRuntimeGovernanceGate,
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
    let mut finalized = finalize_self_runtime_decision(
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
    );
    apply_personality_runtime_governance_gate(&mut finalized, personality_governance_gate);
    normalize_runtime_distillation_decisions(
        &mut finalized,
        refreshed_private_docs.is_some(),
        !refreshed_private_garden_docs.is_empty(),
        refreshed_inner_life.is_some(),
        refreshed_self_model.is_some(),
        refreshed_self_authored_core.is_some(),
        refreshed_self_continuity.is_some(),
        refreshed_outer_voice.is_some(),
        refreshed_mental_privacy.is_some(),
        prelude.refreshed_world_sense.is_some() || state.world_sense.is_some(),
        prelude.refreshed_autonomy_strategy.is_some() || state.autonomy_strategy.is_some(),
        recent_persona_evidence.is_some(),
    );
    *decision = Some(finalized);
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn normalize_self_runtime_decision(
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

pub(super) fn normalize_initial_self_runtime_decision(
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

pub(super) fn apply_personality_runtime_governance_gate(
    decision: &mut SelfRuntimeDecision,
    gate: &crate::memory::PersonalityRuntimeGovernanceGate,
) {
    if gate.allow_upward_distillation {
        return;
    }

    // Conservative runtime governance freezes generic upward promotion, but still allows the
    // already-inspected repair path to execute so governance debt can converge instead of only
    // accumulating.
    decision.refresh_self_model = false;
    decision.self_model_intent.clear();
    decision.self_model_sources.clear();

    if gate.repair_plan.repair_self_authored_core {
        decision.refresh_self_authored_core = true;
        if decision.self_authored_core_intent.trim().is_empty() {
            decision.self_authored_core_intent =
                "Repair the board-level self core before unresolved governance debt hardens"
                    .to_string();
        }
    } else {
        decision.refresh_self_authored_core = false;
        decision.self_authored_core_intent.clear();
        decision.self_authored_core_sources.clear();
    }

    if gate.repair_plan.repair_relationship_constitution {
        decision.refresh_boundary_persona = true;
        if decision.boundary_persona_intent.trim().is_empty() {
            decision.boundary_persona_intent =
                "Repair relation-local boundary drift so the relationship constitution can realign"
                    .to_string();
        }
    }

    if gate.repair_plan.repair_outer_voice {
        decision.refresh_outer_voice = true;
        if decision.outer_voice_intent.trim().is_empty() {
            decision.outer_voice_intent =
                "Repair outward expression drift without promoting new board-level persona"
                    .to_string();
        }
    } else {
        decision.refresh_outer_voice = false;
        decision.outer_voice_intent.clear();
        decision.outer_voice_sources.clear();
    }
}

pub(super) fn refresh_runtime_relationship_constitution(
    ctx: &SelfRuntimeContext<'_>,
    state: &LoadedSelfRuntimeState,
    chat_id: &str,
    now_secs: u64,
    self_authored_core: Option<&crate::memory::SelfAuthoredCore>,
    mental_privacy_state: Option<&crate::memory::MentalPrivacyState>,
    outer_voice: Option<&crate::memory::OuterVoice>,
) -> Option<RelationshipConstitution> {
    sync_self_runtime_relationship_constitution(
        ctx,
        state.active_relationship_scope_id.as_str(),
        &state.active_relationship_channel,
        chat_id,
        now_secs,
        self_authored_core,
        state.relationship_portfolio.as_ref(),
        state.relationship_topology.as_ref(),
        mental_privacy_state,
        outer_voice,
        state.recent_persona_evidence.as_ref(),
    )
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

pub(super) fn normalize_runtime_distillation_decisions(
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

pub(super) fn normalize_runtime_source_id(raw: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distillation_snapshot_ignores_operational_only_recent_persona_evidence() {
        let operational_only = crate::memory::RecentPersonaEvidence {
            repeated_response_mode: "protective_brief".to_string(),
            repeated_task_scope: "narrow".to_string(),
            repeated_initiative_posture: "answer directly".to_string(),
            pressure_pattern: "cautious=4".to_string(),
            tool_usage_pattern: "tool_calls=4".to_string(),
            updated_at: 88,
            ..crate::memory::RecentPersonaEvidence::default()
        };
        let snapshot = build_persona_distillation_snapshot_from_layers(
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&operational_only),
        );
        assert_eq!(snapshot.recent_persona_evidence_at, 0);
        assert!(!snapshot.has_recent_persona_evidence);

        let promotable = crate::memory::RecentPersonaEvidence {
            repeated_priority_order: vec!["self_authored_core".to_string()],
            repeated_relationship_posture: "warm but bounded".to_string(),
            updated_at: 144,
            ..crate::memory::RecentPersonaEvidence::default()
        };
        let snapshot = build_persona_distillation_snapshot_from_layers(
            None,
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&promotable),
        );
        assert_eq!(snapshot.recent_persona_evidence_at, 144);
        assert!(snapshot.has_recent_persona_evidence);
    }
}
