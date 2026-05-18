use super::turn_prepare::PreReplyGovernanceMode;
use super::*;
use crate::memory::EmotionSignalStore;

pub(super) struct PrepareRuntimeStage {
    pub prepare_trace_enabled: bool,
    pub budget: crate::orchestrator::ResourceBudget,
    pub runtime_mode: crate::runtime::RuntimeModeSnapshot,
    pub runtime: RuntimeContext,
    pub interactive_fast_path: bool,
    pub skill_descriptions_len: usize,
    pub has_tools: bool,
    pub prompt_memory_system_budget: usize,
    pub participation_plan: crate::memory::PromptParticipationPlan,
    pub recent_messages_limit: usize,
    pub capability_package_text: Option<String>,
    pub relationship_id: String,
    pub active_governance_mode: Option<PreReplyGovernanceMode>,
    pub emotion_signal_suffix: Option<&'static str>,
}

fn resolve_recent_messages_limit(
    memory_system_kind: crate::memory::MemorySystemKind,
    ingress: IngressKind,
    pressure: crate::orchestrator::PressureLevel,
    runtime_mode: crate::runtime::RuntimeMode,
) -> usize {
    let grounding_floor = crate::memory::memory_policy(memory_system_kind)
        .long_term_recall
        .recent_grounding_message_count;
    let crate::memory::MemorySystemKind::EspCompact = memory_system_kind else {
        return crate::memory::MAX_SESSION_ENTRIES;
    };
    let base = match pressure {
        crate::orchestrator::PressureLevel::Normal => 32,
        crate::orchestrator::PressureLevel::Cautious => 16,
        crate::orchestrator::PressureLevel::Critical => 6,
    };
    let mode_cap = match runtime_mode {
        crate::runtime::RuntimeMode::Normal => base,
        crate::runtime::RuntimeMode::VoiceExclusive => base.min(6),
        crate::runtime::RuntimeMode::Maintenance
        | crate::runtime::RuntimeMode::ConfigActive
        | crate::runtime::RuntimeMode::RecoverySafeMode => base.min(8),
        crate::runtime::RuntimeMode::Booting | crate::runtime::RuntimeMode::Pairing => base.min(4),
    };
    let ingress_cap = match ingress {
        IngressKind::User => mode_cap,
        IngressKind::System => mode_cap.min(8),
    };
    ingress_cap
        .max(grounding_floor)
        .clamp(1, crate::memory::MAX_SESSION_ENTRIES)
}

pub(super) struct PrepareGovernancePrimer {
    pub mental_privacy_adjudication: Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    pub mental_privacy_adjudication_failed: bool,
}

pub(super) struct PreparePromptStage {
    pub prompt_memory: PromptMemoryContext,
    pub recent_persona_evidence: Option<crate::memory::RecentPersonaEvidence>,
    pub prompt_mental_privacy_state: Option<crate::memory::MentalPrivacyState>,
    pub prompt_relationship_portfolio: Option<crate::memory::RelationshipPortfolio>,
    pub prompt_relationship_topology: Option<crate::memory::RelationshipTopology>,
    pub allow_tool_round_recall_refill: bool,
}

pub(super) struct PrepareGovernanceStage {
    pub subject_state: Option<SubjectState>,
    pub deliberation_gate: TurnDeliberationGate,
    pub soul_feedback_projection: Option<crate::agent::soul_feedback::SoulFeedbackProjection>,
    pub mental_privacy_adjudication: Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    pub persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
}

pub(super) struct WorkerPrepareSession {
    context_start: Instant,
    runtime: Option<PrepareRuntimeStage>,
    primer: Option<PrepareGovernancePrimer>,
    prompt: Option<PreparePromptStage>,
    governance: Option<PrepareGovernanceStage>,
}

impl WorkerPrepareSession {
    pub(super) fn new(context_start: Instant) -> Self {
        Self {
            context_start,
            runtime: None,
            primer: None,
            prompt: None,
            governance: None,
        }
    }

    #[cfg(test)]
    pub(super) fn runtime_stage(&self) -> Option<&PrepareRuntimeStage> {
        self.runtime.as_ref()
    }
}

fn log_prepare_stage(prepare_trace_enabled: bool, msg: &crate::bus::PcMsg, stage: &str) {
    if prepare_trace_enabled {
        log::debug!(
            "[agent_prepare] stage={} channel={} chat_id={}",
            stage,
            msg.channel,
            msg.chat_id
        );
    }
}

fn record_prompt_memory_health_issue(
    issues: &mut Vec<String>,
    layer: &'static str,
    error: &crate::error::Error,
) {
    issues.push(format!("{layer} ({})", error.stage().trim()));
}

fn should_load_pre_reply_recent_persona_evidence(
    memory_system_kind: crate::memory::MemorySystemKind,
    participation_plan: crate::memory::PromptParticipationPlan,
) -> bool {
    !matches!(
        memory_system_kind,
        crate::memory::MemorySystemKind::EspCompact
    ) || participation_plan.load_l2_background_governance
        || participation_plan.load_l3_private_depth
}

#[inline(never)]
fn runtime_platform_label_for_target_arch(target_arch: &str) -> &'static str {
    match target_arch {
        "xtensa" | "riscv32" => "ESP32",
        _ => "Linux",
    }
}

#[inline(never)]
fn runtime_platform_label() -> &'static str {
    runtime_platform_label_for_target_arch(std::env::consts::ARCH)
}

#[inline(never)]
pub(super) fn compute_prepare_runtime(
    session: &mut WorkerPrepareSession,
    msg: &crate::bus::PcMsg,
    config: &AgentLoopConfig,
    has_tools: bool,
) {
    let prepare_trace_enabled = cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && msg.ingress == IngressKind::User;
    log_prepare_stage(prepare_trace_enabled, msg, "start");
    log_prepare_stage(prepare_trace_enabled, msg, "emotion_signal_start");
    let emotion_signal_suffix = config
        .runtime
        .emotion_signal_store
        .get_then_clear(&msg.chat_id)
        .ok()
        .flatten()
        .and_then(|s| {
            if s == "comfort" {
                Some("用户可能需安慰，回复时可适当照顾情绪。")
            } else {
                None
            }
        });
    log_prepare_stage(prepare_trace_enabled, msg, "emotion_signal_ready");
    log_prepare_stage(prepare_trace_enabled, msg, "runtime_snapshot_start");
    let budget = crate::orchestrator::current_budget();
    let snapshot = crate::orchestrator::snapshot();
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    let interactive_fast_path = msg.ingress == IngressKind::User && msg.channel.as_ref() != "voice";
    let runtime = RuntimeContext {
        now_secs: crate::util::current_unix_secs(),
        platform: runtime_platform_label(),
        pressure: snapshot.pressure,
        active_agent_tasks: snapshot.active_agent_tasks,
        inbound_depth: snapshot.inbound_depth,
        outbound_depth: snapshot.outbound_depth,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        cpu_usage_percent: snapshot.cpu_usage_percent,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        load_average: snapshot.load_average,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        process_memory_kb: snapshot.process_memory_kb,
    };
    log_prepare_stage(prepare_trace_enabled, msg, "runtime_snapshot_ready");
    log_prepare_stage(prepare_trace_enabled, msg, "skill_descriptions_start");
    let skill_descriptions_len = {
        let skill_descriptions = (config.get_skill_descriptions)();
        skill_descriptions.len()
    };
    log_prepare_stage(prepare_trace_enabled, msg, "skill_descriptions_ready");
    log_prepare_stage(prepare_trace_enabled, msg, "post_memory_budget_start");
    let post_memory_tail_len = estimate_post_memory_system_tail_len(PostMemoryTailParams {
        channel: msg.channel.as_ref(),
        has_tools,
        skill_descriptions_len,
        is_group: msg.is_group,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix,
        runtime: Some(runtime),
        llm_hint: budget.llm_hint,
    });
    let prompt_memory_system_budget = budget
        .system_prompt_max
        .saturating_sub(post_memory_tail_len);
    let assembly_plan = crate::memory::decide_prompt_assembly(
        config.runtime.memory_system_kind,
        msg.ingress,
        has_tools,
        runtime_mode,
        runtime.pressure,
        prompt_memory_system_budget,
    );
    let participation_plan = assembly_plan.participation_plan;
    let recent_messages_limit = resolve_recent_messages_limit(
        config.runtime.memory_system_kind,
        msg.ingress,
        runtime.pressure,
        runtime_mode.current_mode,
    );
    log_prepare_stage(prepare_trace_enabled, msg, "post_memory_budget_ready");
    log_prepare_stage(prepare_trace_enabled, msg, "capability_package_start");
    let capability_package_text = assembly_plan
        .include_capability_package_text
        .then(|| {
            (config.get_capability_package_text)(
                &msg.channel,
                prompt_memory_system_budget.min(1800),
            )
        })
        .flatten();
    if prepare_trace_enabled {
        log::debug!(
            "[agent_prepare] stage=capability_package_ready channel={} chat_id={} has_text={}",
            msg.channel,
            msg.chat_id,
            capability_package_text
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        );
    }

    session.runtime = Some(PrepareRuntimeStage {
        prepare_trace_enabled,
        budget,
        runtime_mode,
        runtime,
        interactive_fast_path,
        skill_descriptions_len,
        has_tools,
        prompt_memory_system_budget,
        participation_plan,
        recent_messages_limit,
        capability_package_text,
        relationship_id: crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id),
        active_governance_mode: PreReplyGovernanceMode::for_turn(
            config.runtime.memory_system_kind,
            msg.ingress,
        ),
        emotion_signal_suffix,
    });
}

#[inline(never)]
pub(super) fn run_prepare_mental_privacy(
    session: &mut WorkerPrepareSession,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
) {
    let runtime_stage = session
        .runtime
        .as_ref()
        .expect("prepare runtime stage must be computed first");
    log_prepare_stage(
        runtime_stage.prepare_trace_enabled,
        msg,
        "mental_privacy_start",
    );
    let (mental_privacy_adjudication, mental_privacy_adjudication_failed): (
        Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
        bool,
    ) = if runtime_stage
        .active_governance_mode
        .is_some_and(|mode| mode.allow_sync_disclosure_adjudication())
    {
        match run_mental_privacy_disclosure_adjudication(
            tool_ctx,
            worker_llm,
            MentalPrivacyDisclosureAdjudicationContext {
                mental_privacy_store: config.runtime.mental_privacy_store.as_ref(),
                relationship_constitution_store: config
                    .runtime
                    .relationship_constitution_store
                    .as_ref(),
                self_model_store: config.runtime.self_model_store.as_ref(),
                self_continuity_store: config.runtime.self_continuity_store.as_ref(),
                inner_life_store: config.runtime.inner_life_store.as_ref(),
                private_doc_store: config.runtime.private_doc_store.as_ref(),
                private_garden_store: config.runtime.private_garden_store.as_ref(),
            },
            MentalPrivacyDisclosureAdjudicationInput {
                channel: &msg.channel,
                chat_id: &msg.chat_id,
                user_content: &msg.content,
                public_disclosure_surface: msg.is_group,
                now_secs: runtime_stage.runtime.now_secs,
            },
        ) {
            Ok(result) => (result, false),
            Err(error) => {
                log::warn!("[agent_mental_privacy_adjudication] failed: {}", error);
                (
                    Some(crate::memory::mental_privacy_adjudication_failure_fallback()),
                    true,
                )
            }
        }
    } else {
        (None, false)
    };
    if runtime_stage.prepare_trace_enabled {
        log::debug!(
            "[agent_prepare] stage=mental_privacy_ready channel={} chat_id={} adjudication={} failed={}",
            msg.channel,
            msg.chat_id,
            mental_privacy_adjudication.is_some(),
            mental_privacy_adjudication_failed
        );
    }

    session.primer = Some(PrepareGovernancePrimer {
        mental_privacy_adjudication,
        mental_privacy_adjudication_failed,
    });
}

#[inline(never)]
pub(super) fn load_prepare_prompt_memory(
    session: &mut WorkerPrepareSession,
    msg: &crate::bus::PcMsg,
    config: &AgentLoopConfig,
) {
    let runtime_stage = session
        .runtime
        .as_ref()
        .expect("prepare runtime stage must be computed first");
    log_prepare_stage(
        runtime_stage.prepare_trace_enabled,
        msg,
        "prompt_memory_load_start",
    );
    let mut prompt_memory = load_prompt_memory_context(PromptMemoryContextParams {
        chat_id: &msg.chat_id,
        current_channel: &msg.channel,
        user_query: &msg.content,
        memory_system_kind: config.runtime.memory_system_kind,
        system_max_len: runtime_stage.prompt_memory_system_budget,
        now_secs: runtime_stage.runtime.now_secs,
        participation_plan: runtime_stage.participation_plan,
        recent_messages_limit: runtime_stage.recent_messages_limit,
        load_long_term_memory: true,
        include_private_garden_projection: msg.ingress != IngressKind::User,
        session_store: config.runtime.session_store.as_ref(),
        memory_store: config.runtime.memory_store.as_ref(),
        session_summary_store: config.runtime.session_summary_store.as_ref(),
        long_term_memory_store: config.runtime.long_term_memory_store.as_ref(),
        execution_state_store: config.runtime.execution_state_store.as_ref(),
        active_work_store: config.runtime.active_work_store.as_ref(),
        task_run_store: config.runtime.task_run_store.as_ref(),
        task_artifact_store: config.runtime.task_artifact_store.as_ref(),
        task_learning_store: config.runtime.task_learning_store.as_ref(),
        self_model_store: config.runtime.self_model_store.as_ref(),
        self_authored_core_store: config.runtime.self_authored_core_store.as_ref(),
        relationship_constitution_store: config.runtime.relationship_constitution_store.as_ref(),
        relationship_portfolio_store: config.runtime.relationship_portfolio_store.as_ref(),
        relationship_topology_store: config.runtime.relationship_topology_store.as_ref(),
        world_sense_store: config.runtime.world_sense_store.as_ref(),
        autonomy_strategy_store: config.runtime.autonomy_strategy_store.as_ref(),
        outer_voice_store: config.runtime.outer_voice_store.as_ref(),
        inner_life_store: config.runtime.inner_life_store.as_ref(),
        self_continuity_store: config.runtime.self_continuity_store.as_ref(),
        felt_significance_store: config.runtime.felt_significance_store.as_ref(),
        temperament_continuity_store: config.runtime.temperament_continuity_store.as_ref(),
        inner_conflict_store: config.runtime.inner_conflict_store.as_ref(),
        private_doc_store: config.runtime.private_doc_store.as_ref(),
        private_garden_store: config.runtime.private_garden_store.as_ref(),
        mental_privacy_store: config.runtime.mental_privacy_store.as_ref(),
        remind_store: config.runtime.remind_at_store.as_ref(),
        task_store: config.runtime.task_store.as_ref(),
        turn_continuity_evidence_store: config.runtime.turn_continuity_evidence_store.as_ref(),
        turn_ledger_store: config.runtime.turn_ledger_store.as_ref(),
        skill_storage: config.runtime.skill_storage.as_ref(),
        continuity_capsule_store: config.runtime.continuity_capsule_store.as_ref(),
    });
    let mut prompt_memory_health_issues = prompt_memory.memory_health_issues.clone();
    if runtime_stage.prepare_trace_enabled {
        let (recent_messages, has_summary, has_message_summary, has_self_model_text) =
            prompt_memory.trace_summary();
        log::debug!(
            "[agent_prepare] stage=prompt_memory_ready channel={} chat_id={} recent_messages={} has_summary={} has_message_summary={} has_self_model_text={}",
            msg.channel,
            msg.chat_id,
            recent_messages,
            has_summary,
            has_message_summary,
            has_self_model_text
        );
    }
    let recent_persona_evidence = if should_load_pre_reply_recent_persona_evidence(
        config.runtime.memory_system_kind,
        runtime_stage.participation_plan,
    ) {
        match load_recent_persona_evidence(
            config.runtime.turn_continuity_evidence_store.as_ref(),
            &runtime_stage.relationship_id,
        ) {
            Ok(value) => value,
            Err(error) => {
                record_prompt_memory_health_issue(
                    &mut prompt_memory_health_issues,
                    "recent_persona_evidence",
                    &error,
                );
                None
            }
        }
    } else {
        None
    };
    let prompt_mental_privacy_state = runtime_stage
        .active_governance_mode
        .filter(|mode| mode.allow_sync_relationship_constitution())
        .and_then(|_| {
            match config
                .runtime
                .mental_privacy_store
                .get(&runtime_stage.relationship_id)
            {
                Ok(value) => value,
                Err(error) => {
                    record_prompt_memory_health_issue(
                        &mut prompt_memory_health_issues,
                        "mental_privacy_state",
                        &error,
                    );
                    None
                }
            }
        });
    let prompt_relationship_portfolio = runtime_stage
        .active_governance_mode
        .filter(|mode| mode.allow_sync_relationship_constitution())
        .and_then(|_| {
            match config
                .runtime
                .relationship_portfolio_store
                .get(board_subject_scope_id())
            {
                Ok(value) => value,
                Err(error) => {
                    record_prompt_memory_health_issue(
                        &mut prompt_memory_health_issues,
                        "relationship_portfolio",
                        &error,
                    );
                    None
                }
            }
        });
    let prompt_relationship_topology = runtime_stage
        .active_governance_mode
        .filter(|mode| mode.allow_sync_relationship_constitution())
        .and_then(|_| {
            match config
                .runtime
                .relationship_topology_store
                .get(board_subject_scope_id())
            {
                Ok(value) => value,
                Err(error) => {
                    record_prompt_memory_health_issue(
                        &mut prompt_memory_health_issues,
                        "relationship_topology",
                        &error,
                    );
                    None
                }
            }
        });
    prompt_memory.memory_health_issues = prompt_memory_health_issues;
    let allow_tool_round_recall_refill =
        crate::memory::prompt_participation_policy(config.runtime.memory_system_kind)
            .tool_round_recall_enabled
            && prompt_memory.long_term_memory_text.is_none()
            && runtime_stage.prompt_memory_system_budget
                >= memory_policy(config.runtime.memory_system_kind)
                    .long_term_recall
                    .block_min_len
            && runtime_stage
                .runtime_mode
                .action_budget
                .allow_non_voice_outbound
            && !matches!(
                runtime_stage.runtime.pressure,
                crate::orchestrator::PressureLevel::Critical
            );

    session.prompt = Some(PreparePromptStage {
        prompt_memory,
        recent_persona_evidence,
        prompt_mental_privacy_state,
        prompt_relationship_portfolio,
        prompt_relationship_topology,
        allow_tool_round_recall_refill,
    });
}

#[inline(never)]
pub(super) fn enrich_prepare_governance(
    session: &mut WorkerPrepareSession,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
) {
    let runtime_stage = session
        .runtime
        .as_ref()
        .expect("prepare runtime stage must be computed first");
    let primer = session
        .primer
        .take()
        .expect("prepare primer must be computed before governance enrichment");
    let prompt_stage = session
        .prompt
        .as_mut()
        .expect("prepare prompt stage must be loaded before governance enrichment");
    if runtime_stage
        .active_governance_mode
        .is_some_and(|mode| mode.allow_sync_relationship_constitution())
    {
        if let Ok(Some(constitution)) = crate::memory::sync_relationship_constitution(
            config.runtime.relationship_constitution_store.as_ref(),
            crate::memory::RelationshipConstitutionSyncInput {
                scope_id: &runtime_stage.relationship_id,
                channel: &msg.channel,
                chat_id: &msg.chat_id,
                now_secs: runtime_stage.runtime.now_secs,
                self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
                relationship_portfolio: prompt_stage.prompt_relationship_portfolio.as_ref(),
                relationship_topology: prompt_stage.prompt_relationship_topology.as_ref(),
                mental_privacy_state: prompt_stage.prompt_mental_privacy_state.as_ref(),
                outer_voice: prompt_stage.prompt_memory.outer_voice.as_ref(),
                recent_persona_evidence: prompt_stage.recent_persona_evidence.as_ref(),
            },
        ) {
            prompt_stage.prompt_memory.relationship_constitution = Some(constitution.clone());
            prompt_stage.prompt_memory.relationship_constitution_text =
                crate::memory::render_relationship_constitution_block(&constitution, 420);
        }
    }
    if runtime_stage.prepare_trace_enabled {
        log::debug!(
            "[agent_prepare] stage=relationship_constitution_ready channel={} chat_id={} has_constitution={}",
            msg.channel,
            msg.chat_id,
            prompt_stage.prompt_memory.relationship_constitution.is_some()
        );
    }
    let core_revision_ledger = config
        .runtime
        .core_revision_ledger_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    let personality_governance_gate = crate::memory::derive_personality_runtime_governance_gate(
        crate::memory::PersonalityGovernanceInspectionInput {
            channel: &msg.channel,
            chat_id: &msg.chat_id,
            now_secs: runtime_stage.runtime.now_secs,
            self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
            core_revision_ledger: core_revision_ledger.as_ref(),
            relationship_constitution: prompt_stage
                .prompt_memory
                .relationship_constitution
                .as_ref(),
            relationship_topology: prompt_stage.prompt_relationship_topology.as_ref(),
            recent_persona_evidence: prompt_stage.recent_persona_evidence.as_ref(),
        },
    );
    prompt_stage.prompt_memory.personality_governance_gate_text =
        crate::memory::render_personality_runtime_governance_gate_block(
            &personality_governance_gate,
            420,
        );
    prompt_stage.prompt_memory.mental_privacy_adjudication_text = primer
        .mental_privacy_adjudication
        .as_ref()
        .and_then(|adjudication| {
            crate::memory::render_mental_privacy_disclosure_adjudication_block(adjudication, 420)
        })
        .or_else(|| {
            (primer.mental_privacy_adjudication_failed
                && personality_governance_gate.conservative_reply)
                .then(|| {
                    crate::memory::render_mental_privacy_governance_fallback_block(
                        &personality_governance_gate.reason_summary,
                        420,
                    )
                })
                .flatten()
        });
    if runtime_stage.prepare_trace_enabled {
        log::debug!(
            "[agent_prepare] stage=governance_ready channel={} chat_id={} conservative_reply={}",
            msg.channel,
            msg.chat_id,
            personality_governance_gate.conservative_reply
        );
    }
    let core_revision_governance = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        prompt_stage
            .prompt_memory
            .self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        prompt_stage
            .prompt_memory
            .self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        runtime_stage.runtime.now_secs,
    );
    let core_revision_ledger_text = core_revision_ledger.as_ref().and_then(|ledger| {
        render_core_revision_governance_block(
            ledger,
            &core_revision_governance,
            runtime_stage.runtime.now_secs,
            360,
        )
    });
    let persona_priority_runtime = PersonaPriorityRuntimeState {
        pressure: runtime_stage.runtime.pressure,
        system_budget: runtime_stage.prompt_memory_system_budget,
        self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
        core_revision_governance: Some(&core_revision_governance),
        disclosure_adjudication: primer.mental_privacy_adjudication.as_ref(),
        recent_persona_evidence: prompt_stage.recent_persona_evidence.as_ref(),
    };
    let recent_persona_evidence_text = prompt_stage
        .recent_persona_evidence
        .as_ref()
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 420));
    let persistent_persona_priority =
        crate::memory::build_persistent_persona_priority_adjudication(persona_priority_runtime);
    let persistent_persona_priority_text =
        crate::memory::render_persona_priority_block(&persistent_persona_priority, 420);
    let persona_priority_adjudication = if runtime_stage
        .active_governance_mode
        .is_some_and(|mode| mode.allow_dynamic_persona_adjudication())
    {
        if !personality_governance_gate.allow_dynamic_persona_priority {
            persistent_persona_priority_text
                .as_ref()
                .map(|_| persistent_persona_priority.clone())
        } else if crate::memory::should_run_persona_priority_adjudication(persona_priority_runtime)
        {
            match crate::memory::run_persona_priority_adjudication(
                tool_ctx,
                worker_llm,
                PersonaPriorityAdjudicationInput {
                    chat_id: &msg.chat_id,
                    current_channel: &msg.channel,
                    user_content: &msg.content,
                    pressure: runtime_stage.runtime.pressure,
                    now_secs: runtime_stage.runtime.now_secs,
                },
                PersonaPriorityGrounding {
                    self_authored_core_text: prompt_stage
                        .prompt_memory
                        .self_authored_core_text
                        .as_deref(),
                    core_revision_ledger_text: core_revision_ledger_text.as_deref(),
                    relationship_portfolio_text: prompt_stage
                        .prompt_memory
                        .relationship_portfolio_text
                        .as_deref(),
                    relationship_constitution_text: prompt_stage
                        .prompt_memory
                        .relationship_constitution_text
                        .as_deref(),
                    recent_persona_evidence_text: recent_persona_evidence_text.as_deref(),
                    world_snapshot_text: prompt_stage.prompt_memory.world_snapshot_text.as_deref(),
                    world_sense_text: prompt_stage.prompt_memory.world_sense_text.as_deref(),
                    self_state_text: prompt_stage.prompt_memory.self_state_text.as_deref(),
                    self_model_text: prompt_stage.prompt_memory.self_model_text.as_deref(),
                    self_continuity_text: prompt_stage
                        .prompt_memory
                        .self_continuity_text
                        .as_deref(),
                    outer_voice_text: prompt_stage.prompt_memory.outer_voice_text.as_deref(),
                    autonomy_strategy_text: prompt_stage
                        .prompt_memory
                        .autonomy_strategy_text
                        .as_deref(),
                    execution_state_text: prompt_stage
                        .prompt_memory
                        .execution_state_text
                        .as_deref(),
                    mental_privacy_text: prompt_stage.prompt_memory.mental_privacy_text.as_deref(),
                    disclosure_adjudication: primer.mental_privacy_adjudication.as_ref(),
                },
            ) {
                Ok(result) => result.or_else(|| {
                    persistent_persona_priority_text
                        .as_ref()
                        .map(|_| persistent_persona_priority.clone())
                }),
                Err(error) => {
                    log::warn!("[agent_persona_priority] failed: {}", error);
                    persistent_persona_priority_text
                        .as_ref()
                        .map(|_| persistent_persona_priority.clone())
                }
            }
        } else {
            persistent_persona_priority_text
                .as_ref()
                .map(|_| persistent_persona_priority.clone())
        }
    } else {
        None
    };
    prompt_stage.prompt_memory.persona_priority_text = persona_priority_adjudication
        .as_ref()
        .and_then(|adjudication| crate::memory::render_persona_priority_block(adjudication, 420))
        .or(persistent_persona_priority_text);
    let subject_shell =
        crate::memory::compile_subject_shell(crate::memory::SubjectShellCompileInput {
            now_secs: runtime_stage.runtime.now_secs,
            platform: runtime_stage.runtime.platform,
            device_identity: "",
            relationship_scope: &runtime_stage.relationship_id,
            channel: &msg.channel,
            chat_id: &msg.chat_id,
            pressure: runtime_stage.runtime.pressure,
            self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
            self_continuity: prompt_stage.prompt_memory.self_continuity.as_ref(),
            self_model: None,
            outer_voice: prompt_stage.prompt_memory.outer_voice.as_ref(),
            relationship_constitution: prompt_stage
                .prompt_memory
                .relationship_constitution
                .as_ref(),
            summary_text: prompt_stage.prompt_memory.summary_text.as_deref(),
            recent_turn_observation_text: prompt_stage
                .prompt_memory
                .recent_turn_observation_text
                .as_deref(),
            active_task_context_text: None,
            governed_memory_evidence_text: None,
            long_term_memory_text: prompt_stage.prompt_memory.long_term_memory_text.as_deref(),
            continuity_capsule_text: prompt_stage
                .prompt_memory
                .continuity_capsule_text
                .as_deref(),
            world_snapshot_text: prompt_stage.prompt_memory.world_snapshot_text.as_deref(),
            world_sense_text: prompt_stage.prompt_memory.world_sense_text.as_deref(),
            memory_health_issues: &prompt_stage.prompt_memory.memory_health_issues,
        });
    let subject_state = compile_subject_state(SubjectStateCompileInput {
        subject_shell: subject_shell.as_ref(),
        self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
        relationship_constitution: prompt_stage
            .prompt_memory
            .relationship_constitution
            .as_ref(),
        persona_priority: persona_priority_adjudication
            .as_ref()
            .or(Some(&persistent_persona_priority)),
        disclosure_adjudication: primer.mental_privacy_adjudication.as_ref(),
        personality_governance_gate: Some(&personality_governance_gate),
        felt_significance: prompt_stage.prompt_memory.felt_significance.as_ref(),
        temperament_continuity: prompt_stage.prompt_memory.temperament_continuity.as_ref(),
        inner_conflict: prompt_stage.prompt_memory.inner_conflict.as_ref(),
        now_secs: runtime_stage.runtime.now_secs,
        pressure: runtime_stage.runtime.pressure,
    });
    let recent_observation = config
        .runtime
        .turn_ledger_store
        .get(&runtime_stage.relationship_id)
        .ok()
        .flatten()
        .and_then(|ledger| ledger.observation);
    let deliberation_gate = compile_turn_deliberation_gate(TurnDeliberationInput {
        strategy: config.strategy,
        ingress: msg.ingress,
        is_group: msg.is_group,
        user_content: &msg.content,
        has_tools: runtime_stage.has_tools,
        pressure: runtime_stage.runtime.pressure,
        runtime_mode: runtime_stage.runtime_mode,
        recent_observation: recent_observation.as_ref(),
        execution_state_text: prompt_stage.prompt_memory.execution_state_text.as_deref(),
        shared_factual_report: &prompt_stage.prompt_memory.shared_factual_recall_report,
        continuity_capsule_report: &prompt_stage.prompt_memory.continuity_capsule_report,
        archive_report: &prompt_stage.prompt_memory.archive_recall_report,
        runtime_skill_report: &prompt_stage.prompt_memory.runtime_skill_recall_report,
        task_recall_report: prompt_stage.prompt_memory.task_recall_report.as_ref(),
        personality_governance_gate: Some(&personality_governance_gate),
    });
    let soul_feedback_projection = crate::agent::soul_feedback::compile_soul_feedback_projection(
        crate::agent::soul_feedback::SoulFeedbackProjectionInput {
            self_authored_core: prompt_stage.prompt_memory.self_authored_core.as_ref(),
            relationship_constitution: prompt_stage
                .prompt_memory
                .relationship_constitution
                .as_ref(),
            persona_priority: persona_priority_adjudication
                .as_ref()
                .or(Some(&persistent_persona_priority)),
            outer_voice: prompt_stage.prompt_memory.outer_voice.as_ref(),
            autonomy_strategy: prompt_stage.prompt_memory.autonomy_strategy.as_ref(),
            subject_state: subject_state.as_ref(),
            deliberation_gate: &deliberation_gate,
            personality_governance_gate: Some(&personality_governance_gate),
            post_reply_self_runtime_enqueued: false,
        },
    );
    session.governance = Some(PrepareGovernanceStage {
        subject_state,
        deliberation_gate,
        soul_feedback_projection,
        mental_privacy_adjudication: primer.mental_privacy_adjudication,
        persona_priority_adjudication,
    });
}

#[inline(never)]
pub(super) fn finalize_prepare_context(
    msg: &crate::bus::PcMsg,
    config: &AgentLoopConfig,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    mut session: Box<WorkerPrepareSession>,
    latency: &mut WorkerLatency,
) -> Result<PreparedWorkerConversation> {
    let runtime_stage = session
        .runtime
        .take()
        .expect("prepare runtime stage must exist during finalize");
    let prompt_stage = session
        .prompt
        .take()
        .expect("prepare prompt stage must exist during finalize");
    let governance_stage = session
        .governance
        .take()
        .expect("prepare governance stage must exist during finalize");
    let PreparePromptStage {
        mut prompt_memory,
        allow_tool_round_recall_refill,
        ..
    } = prompt_stage;
    let projection_groups = prompt_memory.normalize_projection_groups_for_prompt(
        config.runtime.memory_system_kind,
        runtime_stage.prompt_memory_system_budget,
    );
    let constitutional_stack_text = projection_groups.constitutional_stack_text;
    let active_task_context_text = projection_groups.active_task_context_text;
    let governed_memory_evidence_text = projection_groups.governed_memory_evidence_text;
    let background_governance_text = projection_groups.background_governance_text;
    let subject_state_text = governance_stage
        .subject_state
        .as_ref()
        .and_then(|state| render_subject_state_block(state, 360));
    let deliberation_gate_text =
        render_turn_deliberation_gate_block(&governance_stage.deliberation_gate, 360);
    let active_task_context_present = active_task_context_text
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty());
    let governed_memory_evidence_present = governed_memory_evidence_text
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty());
    let memory_health_text = prompt_memory.render_memory_health_block(360);
    let soul_feedback_projection_text = governance_stage
        .soul_feedback_projection
        .as_ref()
        .and_then(|projection| {
            crate::agent::soul_feedback::render_soul_feedback_projection_block(projection, 420)
        });
    let (mut system, messages) = build_context(&crate::agent::ContextParams {
        msg,
        memory_system_kind: config.runtime.memory_system_kind,
        memory: config.runtime.memory_store.as_ref(),
        session: config.runtime.session_store.as_ref(),
        important_message_store: config.runtime.important_message_store.as_ref(),
        has_tools: runtime_stage.has_tools,
        skill_descriptions: "",
        system_max_len: runtime_stage.budget.system_prompt_max,
        messages_max_len: runtime_stage.budget.messages_max,
        recent_messages_limit: runtime_stage.recent_messages_limit,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix: runtime_stage.emotion_signal_suffix,
        memory_health_text: memory_health_text.as_deref(),
        constitutional_stack_text: constitutional_stack_text.as_deref(),
        subject_state_text: subject_state_text.as_deref(),
        deliberation_gate_text: deliberation_gate_text.as_deref(),
        programmable_reasoning_intent_text: None,
        soul_feedback_projection_text: soul_feedback_projection_text.as_deref(),
        active_task_context_text: active_task_context_text.as_deref(),
        governed_memory_evidence_text: governed_memory_evidence_text.as_deref(),
        background_governance_text: background_governance_text.as_deref(),
        capability_package_text: runtime_stage.capability_package_text.as_deref(),
        summary_text: prompt_memory.message_summary_text.as_deref(),
        recent_messages: (!prompt_memory.recent_messages.is_empty())
            .then_some(prompt_memory.recent_messages.as_slice()),
        runtime: Some(runtime_stage.runtime),
        include_daily_notes: false,
        llm_hint: runtime_stage.budget.llm_hint,
    })
    .map_err(|e| e.with_stage("agent_context"))?;
    if runtime_stage.skill_descriptions_len > 0 {
        let skill_descriptions = (config.get_skill_descriptions)();
        if !skill_descriptions.is_empty() {
            let _ = crate::agent::context::append_capped_section(
                &mut system,
                "\n\n## Skills\n",
                &skill_descriptions,
                runtime_stage.budget.system_prompt_max,
            );
        }
    }
    latency.context_ms = session.context_start.elapsed().as_millis();
    let system_scratch = String::with_capacity(
        system
            .len()
            .saturating_add(TASK_EXECUTION_FINISHER_SYSTEM_SUFFIX.len()),
    );
    let runtime_carry = Box::new(prompt_memory.into_runtime_carry());

    Ok(PreparedWorkerConversation {
        runtime_carry,
        subject_state: governance_stage.subject_state.map(Box::new),
        soul_feedback_projection: governance_stage.soul_feedback_projection.map(Box::new),
        system,
        messages,
        system_scratch,
        deliberation_gate: governance_stage.deliberation_gate,
        interactive_fast_path: runtime_stage.interactive_fast_path,
        allow_tool_round_recall_refill,
        prompt_memory_system_budget: runtime_stage.prompt_memory_system_budget,
        pressure: runtime_stage.runtime.pressure,
        request_semantics,
        active_task_context_present,
        governed_memory_evidence_present,
        mental_privacy_adjudication: governance_stage.mental_privacy_adjudication.map(Box::new),
        persona_priority_adjudication: governance_stage.persona_priority_adjudication.map(Box::new),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_recent_messages_limit, runtime_platform_label_for_target_arch,
        should_load_pre_reply_recent_persona_evidence,
    };
    use crate::bus::IngressKind;
    use crate::memory::{MemorySystemKind, PromptParticipationPlan};

    #[test]
    fn esp_compact_default_pre_reply_skips_recent_persona_history() {
        assert!(!should_load_pre_reply_recent_persona_evidence(
            MemorySystemKind::EspCompact,
            PromptParticipationPlan::embedded_first_turn_default(),
        ));
    }

    #[test]
    fn esp_runtime_platform_label_is_soc_family_not_board_model() {
        assert_eq!(runtime_platform_label_for_target_arch("xtensa"), "ESP32");
        assert_eq!(runtime_platform_label_for_target_arch("riscv32"), "ESP32");
        assert_eq!(runtime_platform_label_for_target_arch("aarch64"), "Linux");
    }

    #[test]
    fn esp_compact_l2_background_keeps_recent_persona_evidence() {
        let plan = PromptParticipationPlan {
            load_l2_background_governance: true,
            ..PromptParticipationPlan::embedded_first_turn_default()
        };

        assert!(should_load_pre_reply_recent_persona_evidence(
            MemorySystemKind::EspCompact,
            plan,
        ));
    }

    #[test]
    fn full_runtime_keeps_recent_persona_evidence() {
        assert!(should_load_pre_reply_recent_persona_evidence(
            MemorySystemKind::LinuxFull,
            PromptParticipationPlan::embedded_first_turn_default(),
        ));
    }

    #[test]
    fn linux_full_recent_limit_ignores_config_maintenance_and_voice_mode_caps() {
        for mode in [
            crate::runtime::RuntimeMode::ConfigActive,
            crate::runtime::RuntimeMode::Maintenance,
            crate::runtime::RuntimeMode::VoiceExclusive,
        ] {
            assert_eq!(
                resolve_recent_messages_limit(
                    MemorySystemKind::LinuxFull,
                    IngressKind::User,
                    crate::orchestrator::PressureLevel::Critical,
                    mode,
                ),
                crate::memory::MAX_SESSION_ENTRIES,
                "LinuxFull should preserve the full recent ring in {mode:?}"
            );
        }
    }

    #[test]
    fn esp_compact_recent_limit_keeps_mode_caps() {
        assert_eq!(
            resolve_recent_messages_limit(
                MemorySystemKind::EspCompact,
                IngressKind::User,
                crate::orchestrator::PressureLevel::Normal,
                crate::runtime::RuntimeMode::VoiceExclusive,
            ),
            6
        );
        assert_eq!(
            resolve_recent_messages_limit(
                MemorySystemKind::EspCompact,
                IngressKind::User,
                crate::orchestrator::PressureLevel::Normal,
                crate::runtime::RuntimeMode::ConfigActive,
            ),
            8
        );
    }
}
