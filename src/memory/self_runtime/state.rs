use super::*;

pub(super) fn load_self_runtime_state(
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
        .list(chat_id, self_runtime_private_garden_doc_limit(profile))
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

pub(super) fn sync_self_runtime_relationship_topology(
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

pub(super) fn sync_self_runtime_relationship_portfolio(
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
pub(super) fn sync_self_runtime_relationship_constitution(
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
