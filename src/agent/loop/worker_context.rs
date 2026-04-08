use super::*;

#[inline(never)]
pub(super) fn prepare_worker_conversation<'a>(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &'a crate::bus::PcMsg,
    request_plan: &AgentRequestPlan<'a>,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> Result<PreparedWorkerConversation> {
    let prepare_trace_enabled = cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && msg.ingress == IngressKind::User;
    let log_prepare_stage = |stage: &str| {
        if prepare_trace_enabled {
            log::info!(
                "[agent_prepare] stage={} channel={} chat_id={}",
                stage,
                msg.channel,
                msg.chat_id
            );
        }
    };
    log_prepare_stage("start");
    log_prepare_stage("emotion_signal_start");
    let emotion_signal_suffix = config
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
    log_prepare_stage("emotion_signal_ready");
    log_prepare_stage("runtime_snapshot_start");
    let budget = crate::orchestrator::current_budget();
    let snapshot = crate::orchestrator::snapshot();
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    let interactive_fast_path = msg.ingress == IngressKind::User && msg.channel.as_ref() != "voice";
    let runtime = RuntimeContext {
        now_secs: crate::util::current_unix_secs(),
        platform: if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            "ESP32-S3"
        } else {
            "Linux"
        },
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
    log_prepare_stage("runtime_snapshot_ready");
    let context_start = Instant::now();
    log_prepare_stage("skill_descriptions_start");
    let skill_descriptions = (config.get_skill_descriptions)();
    log_prepare_stage("skill_descriptions_ready");
    let has_tools = request_plan.has_tools();
    log_prepare_stage("post_memory_budget_start");
    let post_memory_tail_len = estimate_post_memory_system_tail_len(PostMemoryTailParams {
        has_tools,
        skill_descriptions: &skill_descriptions,
        is_group: msg.is_group,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix,
        runtime: Some(runtime),
        llm_hint: budget.llm_hint,
    });
    let prompt_memory_system_budget = budget
        .system_prompt_max
        .saturating_sub(post_memory_tail_len);
    log_prepare_stage("post_memory_budget_ready");
    log_prepare_stage("capability_package_start");
    let capability_package_text =
        (config.get_capability_package_text)(&msg.channel, prompt_memory_system_budget.min(1800));
    if prepare_trace_enabled {
        log::info!(
            "[agent_prepare] stage=capability_package_ready channel={} chat_id={} has_text={}",
            msg.channel,
            msg.chat_id,
            capability_package_text
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        );
    }
    let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
    log_prepare_stage("mental_privacy_start");
    let (mental_privacy_adjudication, mental_privacy_adjudication_failed) = if msg.ingress
        == IngressKind::User
    {
        match run_mental_privacy_disclosure_adjudication(
            tool_ctx,
            worker_llm,
            MentalPrivacyDisclosureAdjudicationContext {
                mental_privacy_store: config.mental_privacy_store.as_ref(),
                relationship_constitution_store: config.relationship_constitution_store.as_ref(),
                self_model_store: config.self_model_store.as_ref(),
                self_continuity_store: config.self_continuity_store.as_ref(),
                inner_life_store: config.inner_life_store.as_ref(),
                private_doc_store: config.private_doc_store.as_ref(),
                private_garden_store: config.private_garden_store.as_ref(),
            },
            MentalPrivacyDisclosureAdjudicationInput {
                channel: &msg.channel,
                chat_id: &msg.chat_id,
                user_content: &msg.content,
                now_secs: runtime.now_secs,
            },
        ) {
            Ok(result) => (result, false),
            Err(error) => {
                log::warn!("[agent_mental_privacy_adjudication] failed: {}", error);
                (None, true)
            }
        }
    } else {
        (None, false)
    };
    if prepare_trace_enabled {
        log::info!(
            "[agent_prepare] stage=mental_privacy_ready channel={} chat_id={} adjudication={} failed={}",
            msg.channel,
            msg.chat_id,
            mental_privacy_adjudication.is_some(),
            mental_privacy_adjudication_failed
        );
    }
    log_prepare_stage("prompt_memory_load_start");
    let mut prompt_memory = load_prompt_memory_context(PromptMemoryContextParams {
        chat_id: &msg.chat_id,
        current_channel: &msg.channel,
        user_query: &msg.content,
        system_max_len: prompt_memory_system_budget,
        now_secs: runtime.now_secs,
        profile: config.memory_profile,
        recent_messages_limit: config.session_max_messages,
        load_long_term_memory: true,
        include_private_garden_projection: msg.ingress != IngressKind::User,
        session_store: config.session_store.as_ref(),
        memory_store: config.memory_store.as_ref(),
        session_summary_store: config.session_summary_store.as_ref(),
        long_term_memory_store: config.long_term_memory_store.as_ref(),
        execution_state_store: config.execution_state_store.as_ref(),
        task_run_store: config.task_run_store.as_ref(),
        task_artifact_store: config.task_artifact_store.as_ref(),
        task_learning_store: config.task_learning_store.as_ref(),
        self_model_store: config.self_model_store.as_ref(),
        self_authored_core_store: config.self_authored_core_store.as_ref(),
        relationship_constitution_store: config.relationship_constitution_store.as_ref(),
        relationship_portfolio_store: config.relationship_portfolio_store.as_ref(),
        relationship_topology_store: config.relationship_topology_store.as_ref(),
        world_sense_store: config.world_sense_store.as_ref(),
        autonomy_strategy_store: config.autonomy_strategy_store.as_ref(),
        outer_voice_store: config.outer_voice_store.as_ref(),
        inner_life_store: config.inner_life_store.as_ref(),
        self_continuity_store: config.self_continuity_store.as_ref(),
        private_doc_store: config.private_doc_store.as_ref(),
        private_garden_store: config.private_garden_store.as_ref(),
        mental_privacy_store: config.mental_privacy_store.as_ref(),
        remind_store: config.remind_store.as_ref(),
        task_store: config.task_store.as_ref(),
        turn_ledger_store: config.turn_ledger_store.as_ref(),
        skill_storage: config.skill_storage.as_ref(),
        continuity_capsule_store: config.continuity_capsule_store.as_ref(),
    });
    if prepare_trace_enabled {
        let (recent_messages, has_summary, has_message_summary, has_self_model_text) =
            prompt_memory.trace_summary();
        log::info!(
            "[agent_prepare] stage=prompt_memory_ready channel={} chat_id={} recent_messages={} has_summary={} has_message_summary={} has_self_model_text={}",
            msg.channel,
            msg.chat_id,
            recent_messages,
            has_summary,
            has_message_summary,
            has_self_model_text
        );
    }
    let recent_persona_evidence =
        load_recent_persona_evidence(config.turn_ledger_store.as_ref(), &relationship_id)
            .ok()
            .flatten();
    let prompt_mental_privacy_state = config
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let prompt_relationship_portfolio = config
        .relationship_portfolio_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    let prompt_relationship_topology = config
        .relationship_topology_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    if let Ok(Some(constitution)) = crate::memory::sync_relationship_constitution(
        config.relationship_constitution_store.as_ref(),
        crate::memory::RelationshipConstitutionSyncInput {
            scope_id: &relationship_id,
            channel: &msg.channel,
            chat_id: &msg.chat_id,
            now_secs: runtime.now_secs,
            self_authored_core: prompt_memory.self_authored_core.as_ref(),
            relationship_portfolio: prompt_relationship_portfolio.as_ref(),
            relationship_topology: prompt_relationship_topology.as_ref(),
            mental_privacy_state: prompt_mental_privacy_state.as_ref(),
            outer_voice: prompt_memory.outer_voice.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    ) {
        prompt_memory.relationship_constitution = Some(constitution.clone());
        prompt_memory.relationship_constitution_text =
            crate::memory::render_relationship_constitution_block(&constitution, 420);
    }
    if prepare_trace_enabled {
        log::info!(
            "[agent_prepare] stage=relationship_constitution_ready channel={} chat_id={} has_constitution={}",
            msg.channel,
            msg.chat_id,
            prompt_memory.relationship_constitution.is_some()
        );
    }
    let core_revision_ledger = config
        .core_revision_ledger_store
        .get(board_subject_scope_id())
        .ok()
        .flatten();
    let personality_governance_gate = crate::memory::derive_personality_runtime_governance_gate(
        crate::memory::PersonalityGovernanceInspectionInput {
            channel: &msg.channel,
            chat_id: &msg.chat_id,
            now_secs: runtime.now_secs,
            self_authored_core: prompt_memory.self_authored_core.as_ref(),
            core_revision_ledger: core_revision_ledger.as_ref(),
            relationship_constitution: prompt_memory.relationship_constitution.as_ref(),
            relationship_topology: prompt_relationship_topology.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    );
    prompt_memory.personality_governance_gate_text =
        crate::memory::render_personality_runtime_governance_gate_block(
            &personality_governance_gate,
            420,
        );
    prompt_memory.mental_privacy_adjudication_text = mental_privacy_adjudication
        .as_ref()
        .and_then(|adjudication| {
            crate::memory::render_mental_privacy_disclosure_adjudication_block(adjudication, 420)
        })
        .or_else(|| {
            (mental_privacy_adjudication_failed && personality_governance_gate.conservative_reply)
                .then(|| {
                    crate::memory::render_mental_privacy_governance_fallback_block(
                        &personality_governance_gate.reason_summary,
                        420,
                    )
                })
                .flatten()
        });
    if prepare_trace_enabled {
        log::info!(
            "[agent_prepare] stage=governance_ready channel={} chat_id={} conservative_reply={}",
            msg.channel,
            msg.chat_id,
            personality_governance_gate.conservative_reply
        );
    }
    let core_revision_governance = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        prompt_memory
            .self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        prompt_memory
            .self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        runtime.now_secs,
    );
    let core_revision_ledger_text = core_revision_ledger.as_ref().and_then(|ledger| {
        render_core_revision_governance_block(
            ledger,
            &core_revision_governance,
            runtime.now_secs,
            360,
        )
    });
    let persona_priority_runtime = PersonaPriorityRuntimeState {
        pressure: runtime.pressure,
        system_budget: prompt_memory_system_budget,
        self_authored_core: prompt_memory.self_authored_core.as_ref(),
        core_revision_governance: Some(&core_revision_governance),
        disclosure_adjudication: mental_privacy_adjudication.as_ref(),
        recent_persona_evidence: recent_persona_evidence.as_ref(),
    };
    let recent_persona_evidence_text = recent_persona_evidence
        .as_ref()
        .and_then(|evidence| render_recent_persona_evidence_block(evidence, 420));
    let persistent_persona_priority =
        crate::memory::build_persistent_persona_priority_adjudication(persona_priority_runtime);
    let persistent_persona_priority_text =
        crate::memory::render_persona_priority_block(&persistent_persona_priority, 420);
    let persona_priority_adjudication = if msg.ingress == IngressKind::User {
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
                    pressure: runtime.pressure,
                    now_secs: runtime.now_secs,
                },
                PersonaPriorityGrounding {
                    self_authored_core_text: prompt_memory.self_authored_core_text.as_deref(),
                    core_revision_ledger_text: core_revision_ledger_text.as_deref(),
                    relationship_portfolio_text: prompt_memory
                        .relationship_portfolio_text
                        .as_deref(),
                    relationship_constitution_text: prompt_memory
                        .relationship_constitution_text
                        .as_deref(),
                    recent_persona_evidence_text: recent_persona_evidence_text.as_deref(),
                    world_snapshot_text: prompt_memory.world_snapshot_text.as_deref(),
                    world_sense_text: prompt_memory.world_sense_text.as_deref(),
                    self_state_text: prompt_memory.self_state_text.as_deref(),
                    self_model_text: prompt_memory.self_model_text.as_deref(),
                    self_continuity_text: prompt_memory.self_continuity_text.as_deref(),
                    outer_voice_text: prompt_memory.outer_voice_text.as_deref(),
                    autonomy_strategy_text: prompt_memory.autonomy_strategy_text.as_deref(),
                    execution_state_text: prompt_memory.execution_state_text.as_deref(),
                    mental_privacy_text: prompt_memory.mental_privacy_text.as_deref(),
                    disclosure_adjudication: mental_privacy_adjudication.as_ref(),
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
    prompt_memory.persona_priority_text = persona_priority_adjudication
        .as_ref()
        .and_then(|adjudication| crate::memory::render_persona_priority_block(adjudication, 420))
        .or(persistent_persona_priority_text);
    let subject_state = compile_subject_state(SubjectStateCompileInput {
        self_authored_core: prompt_memory.self_authored_core.as_ref(),
        relationship_constitution: prompt_memory.relationship_constitution.as_ref(),
        persona_priority: persona_priority_adjudication
            .as_ref()
            .or(Some(&persistent_persona_priority)),
        disclosure_adjudication: mental_privacy_adjudication.as_ref(),
        personality_governance_gate: Some(&personality_governance_gate),
        pressure: runtime.pressure,
    });
    let subject_state_text = subject_state
        .as_ref()
        .and_then(|state| render_subject_state_block(state, 360));
    let recent_observation = config
        .turn_ledger_store
        .get(&relationship_id)
        .ok()
        .flatten()
        .and_then(|ledger| ledger.observation);
    let deliberation_gate = compile_turn_deliberation_gate(TurnDeliberationInput {
        strategy: config.strategy,
        ingress: msg.ingress,
        is_group: msg.is_group,
        user_content: &msg.content,
        has_tools,
        pressure: runtime.pressure,
        runtime_mode,
        recent_observation: recent_observation.as_ref(),
        execution_state_text: prompt_memory.execution_state_text.as_deref(),
        shared_factual_report: &prompt_memory.shared_factual_recall_report,
        continuity_capsule_report: &prompt_memory.continuity_capsule_report,
        archive_report: &prompt_memory.archive_recall_report,
        runtime_skill_report: &prompt_memory.runtime_skill_recall_report,
        task_recall_report: prompt_memory.task_recall_report.as_ref(),
    });
    let deliberation_gate_text = render_turn_deliberation_gate_block(&deliberation_gate, 360);
    prompt_memory.refresh_reply_projection_groups();
    let (mut system, messages) = build_context(&crate::agent::ContextParams {
        msg,
        memory: config.memory_store.as_ref(),
        session: config.session_store.as_ref(),
        important_message_store: config.important_message_store.as_ref(),
        has_tools,
        skill_descriptions: &skill_descriptions,
        system_max_len: budget.system_prompt_max,
        messages_max_len: budget.messages_max,
        session_max_messages: config.session_max_messages,
        group_activation: config.tg_group_activation.as_ref(),
        emotion_signal_suffix,
        constitutional_stack_text: prompt_memory.constitutional_stack_text.as_deref(),
        subject_state_text: subject_state_text.as_deref(),
        deliberation_gate_text: deliberation_gate_text.as_deref(),
        active_task_context_text: prompt_memory.active_task_context_text.as_deref(),
        governed_memory_evidence_text: prompt_memory.governed_memory_evidence_text.as_deref(),
        background_governance_text: prompt_memory.background_governance_text.as_deref(),
        execution_state_text: prompt_memory.execution_state_text.as_deref(),
        task_workspace_text: prompt_memory.task_workspace_text.as_deref(),
        task_recall_text: prompt_memory.task_recall_text.as_deref(),
        world_snapshot_text: prompt_memory.world_snapshot_text.as_deref(),
        world_sense_text: prompt_memory.world_sense_text.as_deref(),
        self_state_text: prompt_memory.self_state_text.as_deref(),
        self_authored_core_text: prompt_memory.self_authored_core_text.as_deref(),
        relationship_portfolio_text: prompt_memory.relationship_portfolio_text.as_deref(),
        relationship_constitution_text: prompt_memory.relationship_constitution_text.as_deref(),
        persona_priority_text: prompt_memory.persona_priority_text.as_deref(),
        self_model_text: prompt_memory.self_model_text.as_deref(),
        autonomy_strategy_text: prompt_memory.autonomy_strategy_text.as_deref(),
        outer_voice_text: prompt_memory.outer_voice_text.as_deref(),
        inner_life_text: prompt_memory.inner_life_text.as_deref(),
        self_continuity_text: prompt_memory.self_continuity_text.as_deref(),
        private_workspace_text: prompt_memory.private_workspace_text.as_deref(),
        private_garden_text: prompt_memory.private_garden_text.as_deref(),
        mental_privacy_adjudication_text: prompt_memory.mental_privacy_adjudication_text.as_deref(),
        mental_privacy_text: prompt_memory.mental_privacy_text.as_deref(),
        long_term_memory_text: prompt_memory.long_term_memory_text.as_deref(),
        archive_evidence_text: prompt_memory.archive_evidence_text.as_deref(),
        runtime_skill_text: prompt_memory.runtime_skill_text.as_deref(),
        capability_package_text: capability_package_text.as_deref(),
        summary_text: prompt_memory.message_summary_text.as_deref(),
        recent_messages: (!prompt_memory.recent_messages.is_empty())
            .then_some(prompt_memory.recent_messages.as_slice()),
        runtime: Some(runtime),
        include_daily_notes: false,
        llm_hint: budget.llm_hint,
    })
    .map_err(|e| e.with_stage("agent_context"))?;
    latency.context_ms = context_start.elapsed().as_millis();
    request_plan.apply_system_prompt(&mut system, budget.system_prompt_max);
    let system_scratch = String::with_capacity(
        system
            .len()
            .saturating_add(TASK_EXECUTION_FINISHER_SYSTEM_SUFFIX.len()),
    );

    Ok(PreparedWorkerConversation {
        prompt_memory,
        subject_state,
        system,
        messages,
        system_scratch,
        deliberation_gate,
        interactive_fast_path,
        prompt_memory_system_budget,
        pressure: runtime.pressure,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    })
}
