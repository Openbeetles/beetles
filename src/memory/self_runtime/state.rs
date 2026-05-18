use super::*;

fn load_optional_store<T>(
    layer: &'static str,
    load_health: &mut SelfRuntimeLoadHealth,
    load: impl FnOnce() -> crate::error::Result<Option<T>>,
) -> Option<T> {
    match load() {
        Ok(value) => value,
        Err(error) => {
            load_health.record(layer, &error);
            None
        }
    }
}

fn load_list_store<T>(
    layer: &'static str,
    load_health: &mut SelfRuntimeLoadHealth,
    load: impl FnOnce() -> crate::error::Result<Vec<T>>,
) -> Vec<T> {
    match load() {
        Ok(values) => values,
        Err(error) => {
            load_health.record(layer, &error);
            Vec::new()
        }
    }
}

const SELF_RUNTIME_SANDBOX_PROBE_TURN_LIMIT: usize = 4;

fn load_self_runtime_sandbox_probe_text(
    ctx: &SelfRuntimeContext<'_>,
    relationship_scope_id: &str,
    profile: MemoryProfile,
) -> Option<String> {
    if matches!(profile, MemoryProfile::Embedded) {
        return None;
    }
    let budget = memory_policy(profile)
        .self_runtime
        .grounding_max_len
        .min(1024);
    match ctx
        .turn_ledger_store
        .list_recent(relationship_scope_id, SELF_RUNTIME_SANDBOX_PROBE_TURN_LIMIT)
    {
        Ok(ledgers) => build_self_runtime_sandbox_probe_text(&ledgers, budget),
        Err(error) => {
            log::warn!(
                "[self_runtime] sandbox candidate evidence skipped because turn-ledger read failed relationship_scope={}: {}",
                relationship_scope_id,
                error
            );
            None
        }
    }
}

pub(super) fn load_self_runtime_state(
    ctx: &SelfRuntimeContext<'_>,
    chat_id: &str,
    payload: &SelfRuntimeJobPayload,
    profile: MemoryProfile,
    authority_plan: SelfRuntimeAuthorityPlan,
) -> Box<LoadedSelfRuntimeState> {
    let subject_id = board_subject_scope_id();
    let relationship_governance_enabled = authority_plan.allows_relationship_governance();
    let mut load_health = SelfRuntimeLoadHealth::default();
    let summary_text = load_optional_store("session_summary", &mut load_health, || {
        ctx.session_summary_store
            .get_with_count(chat_id)
            .map(|value| value.map(|(summary, _)| summary))
    });
    let execution_state = load_optional_store("execution_state", &mut load_health, || {
        ctx.execution_state_store.get(chat_id)
    });
    let self_model = load_optional_store("self_model", &mut load_health, || {
        ctx.self_model_store.get(subject_id)
    });
    let self_authored_core = authority_plan
        .allow_direct_self_authored_core
        .then(|| {
            load_optional_store("self_authored_core", &mut load_health, || {
                ctx.self_authored_core_store.get(subject_id)
            })
        })
        .flatten();
    let core_revision_ledger = authority_plan
        .allow_direct_self_authored_core
        .then(|| {
            load_optional_store("core_revision_ledger", &mut load_health, || {
                ctx.core_revision_ledger_store.get(subject_id)
            })
        })
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
    let private_docs = authority_plan
        .allow_direct_private_docs
        .then(|| {
            load_optional_store("private_docs", &mut load_health, || {
                ctx.private_doc_store.get(subject_id)
            })
        })
        .flatten();
    let private_garden_docs = if authority_plan.allow_direct_private_garden {
        load_list_store("private_garden_docs", &mut load_health, || {
            ctx.private_garden_store.list(
                private_garden_scope_id(),
                self_runtime_private_garden_doc_limit(profile),
            )
        })
    } else {
        Vec::new()
    };
    let inner_life = load_optional_store("inner_life", &mut load_health, || {
        ctx.inner_life_store.get(subject_id)
    });
    let self_continuity = load_optional_store("self_continuity", &mut load_health, || {
        ctx.self_continuity_store.get(subject_id)
    });
    let felt_significance = load_optional_store("felt_significance", &mut load_health, || {
        ctx.felt_significance_store.get(subject_id)
    });
    let temperament_continuity =
        load_optional_store("temperament_continuity", &mut load_health, || {
            ctx.temperament_continuity_store.get(subject_id)
        });
    let inner_conflict = load_optional_store("inner_conflict", &mut load_health, || {
        ctx.inner_conflict_store.get(subject_id)
    });
    let relationship_topology =
        load_optional_store("relationship_topology", &mut load_health, || {
            ctx.relationship_topology_store.get(subject_id)
        });
    let relationship_portfolio =
        load_optional_store("relationship_portfolio", &mut load_health, || {
            ctx.relationship_portfolio_store.get(subject_id)
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
    let world_sense = load_optional_store("world_sense", &mut load_health, || {
        ctx.world_sense_store.get(&active_relationship_scope_id)
    });
    let autonomy_strategy = load_optional_store("autonomy_strategy", &mut load_health, || {
        ctx.autonomy_strategy_store.get(subject_id)
    });
    let outer_voice = authority_plan
        .allow_direct_outer_voice
        .then(|| {
            load_optional_store("outer_voice", &mut load_health, || {
                ctx.outer_voice_store.get(&active_relationship_scope_id)
            })
        })
        .flatten();
    let mental_privacy_state = authority_plan
        .allow_direct_boundary_persona
        .then(|| {
            load_optional_store("mental_privacy", &mut load_health, || {
                ctx.mental_privacy_store.get(&active_relationship_scope_id)
            })
        })
        .flatten();
    let recent_persona_evidence =
        load_optional_store("recent_persona_evidence", &mut load_health, || {
            load_recent_persona_evidence(
                ctx.turn_continuity_evidence_store,
                &active_relationship_scope_id,
            )
        });
    let sandbox_probe_text =
        load_self_runtime_sandbox_probe_text(ctx, &active_relationship_scope_id, profile);
    let relationship_constitution = if relationship_governance_enabled {
        load_optional_store("relationship_constitution", &mut load_health, || {
            ctx.relationship_constitution_store
                .get(&active_relationship_scope_id)
        })
    } else {
        None
    };
    let self_continuity = if payload.trigger == SelfRuntimeTrigger::PostReply {
        match self_continuity {
            Some(mut continuity) => {
                continuity.last_user_turn_at = payload.now_secs;
                continuity.last_user_chat_id = chat_id.trim().to_string();
                continuity.last_user_channel = payload.source_channel.trim().to_string();
                Some(continuity)
            }
            None if !load_health.has_issue_for("self_continuity") => {
                Some(crate::memory::SelfContinuity {
                    last_user_turn_at: payload.now_secs,
                    last_user_chat_id: chat_id.trim().to_string(),
                    last_user_channel: payload.source_channel.trim().to_string(),
                    ..crate::memory::SelfContinuity::default()
                })
            }
            None => None,
        }
    } else {
        self_continuity
    };
    let world_snapshot_ctx = WorldSnapshotContext {
        chat_id,
        source_channel: &active_relationship_channel,
        now_secs: payload.now_secs,
        self_continuity: self_continuity.as_ref(),
        remind_store: ctx.remind_store,
        task_store: ctx.task_store,
    };
    let world_snapshot_reminders = match load_world_snapshot_reminders(world_snapshot_ctx) {
        Ok(reminders) => Some(reminders),
        Err(error) => {
            load_health.record("world_snapshot_reminders", &error);
            None
        }
    };
    let world_snapshot_tasks = match load_world_snapshot_tasks(world_snapshot_ctx) {
        Ok(tasks) => Some(tasks),
        Err(error) => {
            load_health.record("world_snapshot_tasks", &error);
            None
        }
    };
    let world_snapshot = match (&world_snapshot_reminders, &world_snapshot_tasks) {
        (Some(reminders), Some(tasks)) => {
            build_world_snapshot_from_commitments(world_snapshot_ctx, reminders, tasks)
        }
        _ => build_world_snapshot_from_commitments(world_snapshot_ctx, &[], &[]),
    };
    let recent = load_list_store("recent_transcript", &mut load_health, || {
        ctx.session_store.load_recent(
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
    });
    let world_snapshot_text = render_world_snapshot_block(
        &world_snapshot,
        memory_policy(profile).self_runtime.grounding_max_len,
    );
    let world_sense_text = world_sense.as_ref().and_then(|world_sense| {
        render_world_sense_block(
            world_sense,
            memory_policy(profile).world_sense.grounding_max_len,
        )
    });
    let load_issue_text = load_health
        .issues
        .iter()
        .map(|issue| format!("{}@{}", issue.layer, issue.stage))
        .collect::<Vec<_>>();
    let subject_shell = compile_subject_shell(SubjectShellCompileInput {
        now_secs: payload.now_secs,
        platform: "self_runtime",
        device_identity: "",
        relationship_scope: &active_relationship_scope_id,
        channel: &active_relationship_channel,
        chat_id,
        pressure: PressureLevel::Normal,
        self_authored_core: self_authored_core.as_ref(),
        self_continuity: self_continuity.as_ref(),
        self_model: self_model.as_ref(),
        outer_voice: outer_voice.as_ref(),
        relationship_constitution: relationship_constitution.as_ref(),
        summary_text: summary_text.as_deref(),
        recent_turn_observation_text: None,
        active_task_context_text: None,
        governed_memory_evidence_text: None,
        long_term_memory_text: None,
        continuity_capsule_text: None,
        world_snapshot_text: world_snapshot_text.as_deref(),
        world_sense_text: world_sense_text.as_deref(),
        memory_health_issues: load_issue_text.as_slice(),
    });
    Box::new(LoadedSelfRuntimeState {
        load_health,
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
        subject_shell,
        felt_significance,
        temperament_continuity,
        inner_conflict,
        relationship_portfolio,
        relationship_topology,
        relationship_constitution,
        world_sense,
        autonomy_strategy,
        outer_voice,
        mental_privacy_state,
        recent_persona_evidence,
        sandbox_probe_text,
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
    let turn_ledger = match ctx.turn_ledger_store.get(&relationship_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship topology sync skipped because turn ledger read failed channel={} chat_id={}: {}",
                relationship_channel,
                chat_id,
                error
            );
            return;
        }
    };
    let mental_privacy_state = match ctx.mental_privacy_store.get(&relationship_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship topology sync skipped because mental privacy read failed channel={} chat_id={}: {}",
                relationship_channel,
                chat_id,
                error
            );
            return;
        }
    };
    let outer_voice = match ctx.outer_voice_store.get(&relationship_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship topology sync skipped because outer voice read failed channel={} chat_id={}: {}",
                relationship_channel,
                chat_id,
                error
            );
            return;
        }
    };
    let world_sense = match ctx.world_sense_store.get(&relationship_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship topology sync skipped because world sense read failed channel={} chat_id={}: {}",
                relationship_channel,
                chat_id,
                error
            );
            return;
        }
    };
    let recent_persona_evidence = match load_recent_persona_evidence(
        ctx.turn_continuity_evidence_store,
        &relationship_id,
    ) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship topology sync skipped because persona evidence read failed channel={} chat_id={}: {}",
                relationship_channel,
                chat_id,
                error
            );
            return;
        }
    };
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
    let relationship_topology = match ctx.relationship_topology_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship portfolio sync skipped because topology read failed: {}",
                error
            );
            match ctx.relationship_portfolio_store.get(subject_id) {
                Ok(existing) => return existing,
                Err(fallback_error) => {
                    log::warn!(
                        "[self_runtime] relationship portfolio fallback read failed: {}",
                        fallback_error
                    );
                    return None;
                }
            }
        }
    };
    let self_authored_core = match ctx.self_authored_core_store.get(subject_id) {
        Ok(value) => value,
        Err(error) => {
            log::warn!(
                "[self_runtime] relationship portfolio sync skipped because self-authored core read failed: {}",
                error
            );
            match ctx.relationship_portfolio_store.get(subject_id) {
                Ok(existing) => return existing,
                Err(fallback_error) => {
                    log::warn!(
                        "[self_runtime] relationship portfolio fallback read failed: {}",
                        fallback_error
                    );
                    return None;
                }
            }
        }
    };
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
            match ctx.relationship_portfolio_store.get(subject_id) {
                Ok(existing) => existing,
                Err(fallback_error) => {
                    log::warn!(
                        "[self_runtime] relationship portfolio fallback read failed: {}",
                        fallback_error
                    );
                    None
                }
            }
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
            match ctx.relationship_constitution_store.get(scope_id) {
                Ok(existing) => existing,
                Err(fallback_error) => {
                    log::warn!(
                        "[self_runtime] relationship constitution fallback read failed scope_id={}: {}",
                        scope_id,
                        fallback_error
                    );
                    None
                }
            }
        }
    }
}
