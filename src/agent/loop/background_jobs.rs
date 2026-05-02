#![allow(clippy::too_many_arguments)]

use super::*;

fn append_post_reply_workflow_audit(
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    channel: &str,
    chat_id: &str,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::PostReplyMaintenance,
            crate::runtime::WorkflowTrigger::PostReply,
            disposition,
            effect,
            crate::runtime::WorkflowRecoveryPolicy::DropOnModeExit,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, Some(channel), Some(chat_id)),
    );
}

pub(super) fn enqueue_post_reply_maintenance_job(
    active_work_store: &dyn crate::agent::ActiveWorkStore,
    detached_work_store: &dyn crate::agent::DetachedWorkStore,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
    reply_content: &str,
    tool_calls: u32,
    external_content_used: bool,
    prompt_recall_intent: crate::memory::PromptRecallIntent,
    runtime_skill_selected_ids: &[String],
    task_learning_selected_ids: &[String],
    reuse_outcome: crate::skills::RuntimeSkillReuseOutcome,
    reuse_outcome_note: &str,
    memory_profile: crate::memory::MemoryProfile,
) -> bool {
    let payload = PostReplyMaintenanceJobPayload::from_turn(
        msg,
        reply_content,
        tool_calls,
        external_content_used,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        reuse_outcome,
        reuse_outcome_note,
    );
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job serialize failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "post_reply_maintenance_serialize_failed",
                crate::runtime::WorkflowEffect::Noop,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            return false;
        }
    };
    let job = match PcMsg::new_system(CHANNEL_POST_REPLY_MAINTENANCE, msg.chat_id.as_ref(), body) {
        Ok(job) => job,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job build failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "post_reply_maintenance_build_failed",
                crate::runtime::WorkflowEffect::Noop,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            return false;
        }
    };
    if matches!(memory_profile, crate::memory::MemoryProfile::Embedded) {
        let due_at = std::time::Instant::now()
            + std::time::Duration::from_millis(POST_REPLY_MAINTENANCE_DELAY_MS);
        let coalesce_key = crate::agent::DetachedWorkKey::new(
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
            crate::agent::DetachedJobKind::PostReplyMaintenance,
        )
        .storage_key();
        let scheduled = crate::runtime::schedule_bounded_keyed_system_inbound_msg(
            due_at,
            system_inbound_tx.clone(),
            job,
            std::time::Duration::from_millis(super::BACKGROUND_DEFER_DELAY_MS),
            "post_reply_maintenance",
            coalesce_key,
            std::time::Duration::from_millis(crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS),
        );
        if scheduled {
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::DeferUntil,
                "post_reply_maintenance_scheduled",
                crate::runtime::WorkflowEffect::EnqueueSystemJob,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            return true;
        }
        append_post_reply_workflow_audit(
            crate::runtime::WorkflowDisposition::ExecuteFailed,
            "post_reply_maintenance_queue_full",
            crate::runtime::WorkflowEffect::Noop,
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
        );
        return false;
    }
    match crate::agent::has_meaningful_foreground_work_for_chat(
        active_work_store,
        msg.chat_id.as_ref(),
    ) {
        Ok(true) => {
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::NoTrigger,
                "foreground_work_active",
                crate::runtime::WorkflowEffect::Noop,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            return false;
        }
        Ok(false) => {}
        Err(error) => {
            log::warn!(
                "[agent_memory] active work gate failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "foreground_work_gate_failed",
                crate::runtime::WorkflowEffect::Noop,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            return false;
        }
    }
    let key = crate::agent::DetachedWorkKey::new(
        msg.channel.as_ref(),
        msg.chat_id.as_ref(),
        crate::agent::DetachedJobKind::PostReplyMaintenance,
    );
    match crate::agent::upsert_detached_work_job(
        detached_work_store,
        key,
        &job,
        POST_REPLY_MAINTENANCE_DELAY_MS,
        "post_reply_maintenance_scheduled",
    ) {
        Ok(outcome) => {
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::DeferUntil,
                if outcome.changed {
                    "post_reply_maintenance_scheduled"
                } else {
                    "post_reply_maintenance_merged"
                },
                crate::runtime::WorkflowEffect::EnqueueSystemJob,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            true
        }
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance detached upsert failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteFailed,
                "post_reply_maintenance_schedule_failed",
                crate::runtime::WorkflowEffect::Noop,
                msg.channel.as_ref(),
                msg.chat_id.as_ref(),
            );
            false
        }
    }
}

fn build_system_llm_ctx<'a>(
    http: &'a mut dyn PlatformHttpClient,
    config: &AgentLoopConfig,
    chat_id: &str,
    locale: UiLocale,
) -> HttpClientToolContext<'a> {
    HttpClientToolContext {
        http,
        chat_id: Some(Arc::from(chat_id)),
        ingress: crate::bus::IngressKind::System,
        channel: Some(Arc::from("system")),
        tool_registry: None,
        channel_capability_registry: Arc::clone(&config.channel_capability_registry),
        supports_current_chat_outbound_message: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 0,
        outbound_message_count: 0,
        locale,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetachedJobRunDisposition {
    Completed,
    RetryLater { reason: &'static str, delay_ms: u64 },
    PermanentDrop { reason: &'static str },
}

fn run_long_term_memory_refresh_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
) -> DetachedJobRunDisposition {
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let outcome = run_long_term_memory_refresh(
        &mut llm_ctx,
        worker_llm,
        LongTermMemoryRefreshContext {
            memory_store: config.runtime.memory_store.as_ref(),
            session_store: config.runtime.session_store.as_ref(),
            session_summary_store: config.runtime.session_summary_store.as_ref(),
            long_term_memory_store: config.runtime.long_term_memory_store.as_ref(),
            extraction_state_store: config
                .runtime
                .long_term_memory_extraction_state_store
                .as_ref(),
            turn_ledger_store: config.runtime.turn_ledger_store.as_ref(),
            skill_storage: config.runtime.skill_storage.as_ref(),
        },
        &msg.chat_id,
        crate::orchestrator::snapshot().pressure,
        config.runtime.memory_system_kind.memory_profile(),
    );
    outcome.persist(
        config
            .runtime
            .long_term_memory_extraction_state_store
            .as_ref(),
        &msg.chat_id,
    );
    match outcome {
        LongTermMemoryRefreshOutcome::Processed { changed_count, .. } => {
            if changed_count > 0 {
                log::info!(
                    "[agent_memory] long-term memory refreshed for {} (count={})",
                    msg.chat_id,
                    changed_count
                );
            }
            DetachedJobRunDisposition::Completed
        }
        LongTermMemoryRefreshOutcome::Failed { error, .. } => {
            log::warn!("[agent_memory] refresh failed: {}", error);
            DetachedJobRunDisposition::RetryLater {
                reason: "long_term_memory_refresh_failed",
                delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
            }
        }
        LongTermMemoryRefreshOutcome::Deferred { .. } => DetachedJobRunDisposition::RetryLater {
            reason: "long_term_memory_refresh_deferred",
            delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
        },
    }
}

fn run_idle_memory_forge_job(config: &AgentLoopConfig, msg: &PcMsg) -> DetachedJobRunDisposition {
    match crate::reasoning::run_idle_memory_forge_background_job(
        config.runtime.long_term_memory_store.as_ref(),
        config.runtime.continuity_capsule_store.as_ref(),
        config.runtime.platform.state_fs().as_ref(),
        msg,
    ) {
        Ok(summary) => {
            if summary.total_candidates > 0 {
                log::info!(
                    "[idle_memory_forge] updated for {} (candidates={}, finding={})",
                    msg.chat_id,
                    summary.total_candidates,
                    summary.primary_finding.unwrap_or_default()
                );
            } else {
                log::info!(
                    "[idle_memory_forge] completed for {} (no candidates)",
                    msg.chat_id
                );
            }
            DetachedJobRunDisposition::Completed
        }
        Err(error) => {
            log::warn!("[idle_memory_forge] failed for {}: {}", msg.chat_id, error);
            DetachedJobRunDisposition::RetryLater {
                reason: "idle_memory_forge_failed",
                delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
            }
        }
    }
}

fn run_post_reply_maintenance_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    _system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
    current_background_agent_task_slots: u32,
) -> DetachedJobRunDisposition {
    let payload: PostReplyMaintenanceJobPayload = match serde_json::from_str(&msg.content) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job decode failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return DetachedJobRunDisposition::PermanentDrop {
                reason: "post_reply_payload_invalid",
            };
        }
    };
    match embedded_post_reply_gate_policy(
        config.runtime.memory_system_kind.memory_profile(),
        crate::agent::DetachedJobKind::PostReplyMaintenance,
        &crate::orchestrator::snapshot(),
        current_background_agent_task_slots,
        Some(payload.first_deferred_at_ms),
        super::now_unix_ms(),
    ) {
        BackgroundGatePolicy::Run => {}
        BackgroundGatePolicy::Lightweight => {
            log::info!(
                "[agent_memory] lightweight post-reply maintenance advanced chat_id={} after bounded deferral",
                msg.chat_id
            );
            append_post_reply_workflow_audit(
                crate::runtime::WorkflowDisposition::ExecuteNow,
                "post_reply_lightweight_bounded",
                crate::runtime::WorkflowEffect::Noop,
                payload.source_channel.as_str(),
                msg.chat_id.as_ref(),
            );
            return DetachedJobRunDisposition::Completed;
        }
        BackgroundGatePolicy::Defer { reason, delay_ms } => {
            return DetachedJobRunDisposition::RetryLater { reason, delay_ms };
        }
    }
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let maintenance_outcome = run_post_reply_memory_maintenance(
        &mut llm_ctx,
        worker_llm,
        PostReplyMemoryMaintenanceContext {
            session_store: config.runtime.session_store.as_ref(),
            memory_store: config.runtime.memory_store.as_ref(),
            session_summary_store: config.runtime.session_summary_store.as_ref(),
            execution_state_store: config.runtime.execution_state_store.as_ref(),
            active_work_store: config.runtime.active_work_store.as_ref(),
            long_term_memory_store: config.runtime.long_term_memory_store.as_ref(),
            continuity_capsule_store: config.runtime.continuity_capsule_store.as_ref(),
            extraction_state_store: config
                .runtime
                .long_term_memory_extraction_state_store
                .as_ref(),
            turn_ledger_store: config.runtime.turn_ledger_store.as_ref(),
            skill_storage: config.runtime.skill_storage.as_ref(),
            task_run_store: config.runtime.task_run_store.as_ref(),
            task_artifact_store: config.runtime.task_artifact_store.as_ref(),
            task_learning_store: config.runtime.task_learning_store.as_ref(),
        },
        PostReplyMemoryMaintenanceInput {
            chat_id: &msg.chat_id,
            ingress: payload.ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: crate::orchestrator::snapshot().pressure,
            memory_profile: config.runtime.memory_system_kind.memory_profile(),
            tool_calls: payload.tool_calls,
            external_content_used: payload.external_content_used,
            prompt_recall_intent: payload.prompt_recall_intent,
            runtime_skill_selected_ids: payload.runtime_skill_selected_ids,
            task_learning_selected_ids: payload.task_learning_selected_ids,
            reuse_outcome: payload.reuse_outcome,
            reuse_outcome_note: &payload.reuse_outcome_note,
            now_secs: payload.now_secs,
        },
        || match PcMsg::new_system(CHANNEL_LONG_TERM_MEMORY_REFRESH, msg.chat_id.as_ref(), "") {
            Ok(job) => crate::agent::upsert_detached_work_job(
                config.runtime.detached_work_store.as_ref(),
                crate::agent::DetachedWorkKey::new(
                    "memory_refresh",
                    msg.chat_id.as_ref(),
                    crate::agent::DetachedJobKind::LongTermMemoryRefresh,
                ),
                &job,
                0,
                "long_term_memory_refresh_enqueued",
            )
            .map(|_| true)
            .unwrap_or_else(|error| {
                log::warn!("[agent_memory] refresh detached enqueue failed: {}", error);
                false
            }),
            Err(error) => {
                log::warn!("[agent_memory] refresh job build failed: {}", error);
                false
            }
        },
    );
    let maintenance_failed = maintenance_outcome.summary_result.is_err()
        || maintenance_outcome.execution_state_result.is_err()
        || maintenance_outcome.task_learning_outcome.is_err()
        || maintenance_outcome.continuity_capsule_outcome.is_err()
        || maintenance_outcome.extraction_request_outcome
            == LongTermMemoryRefreshRequestOutcome::RequestFailed;
    match maintenance_outcome.summary_result {
        Ok(SessionSummaryRefreshOutcome::Updated { used_fallback }) => {
            if used_fallback {
                log::info!("[agent_summary] updated for {} (fallback)", msg.chat_id);
            } else {
                log::info!("[agent_summary] updated for {}", msg.chat_id);
            }
        }
        Ok(SessionSummaryRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_summary] failed: {}", error),
    }
    match maintenance_outcome.execution_state_result {
        Ok(crate::memory::ExecutionStateRefreshOutcome::Updated) => {
            log::info!("[agent_execution_state] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::ExecutionStateRefreshOutcome::Cleared) => {
            log::info!("[agent_execution_state] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::ExecutionStateRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_execution_state] failed: {}", error),
    }
    if let Some(summary) = maintenance_outcome.factual_coordination_summary.as_deref() {
        log::info!(
            "[agent_shared_factual_plane] {} suggested_refresh={} summary={}",
            msg.chat_id,
            maintenance_outcome.factual_refresh_suggested,
            summary
        );
    }
    if maintenance_outcome.extraction_request_outcome
        == LongTermMemoryRefreshRequestOutcome::RequestFailed
    {
        log::debug!(
            "[agent_memory] refresh request was eligible but not enqueued chat_id={}",
            msg.chat_id
        );
    }
    match maintenance_outcome.continuity_capsule_outcome {
        Ok(ref outcome) if outcome.upserted > 0 => {
            log::info!(
                "[agent_continuity_capsule] updated chat_id={} drafted={} upserted={} superseded={}",
                msg.chat_id,
                outcome.drafted,
                outcome.upserted,
                outcome.superseded
            );
        }
        Ok(_) => {}
        Err(ref error) => log::warn!("[agent_continuity_capsule] failed: {}", error),
    }
    if maintenance_failed {
        return DetachedJobRunDisposition::RetryLater {
            reason: "post_reply_maintenance_incomplete",
            delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
        };
    }
    DetachedJobRunDisposition::Completed
}

fn run_self_runtime_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    _system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) -> DetachedJobRunDisposition {
    let payload: crate::memory::SelfRuntimeJobPayload = match serde_json::from_str(&msg.content) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!(
                "[self_runtime] decode failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return DetachedJobRunDisposition::PermanentDrop {
                reason: "self_runtime_payload_invalid",
            };
        }
    };
    if payload.trigger != crate::memory::SelfRuntimeTrigger::OperatorRequested
        && !crate::platform::time::wall_clock_is_trustworthy()
    {
        log::debug!(
            "[self_runtime] defer chat_id={} trigger={:?} because wall clock is not trustworthy yet",
            msg.chat_id,
            payload.trigger
        );
        return DetachedJobRunDisposition::RetryLater {
            reason: "clock_unsynchronized",
            delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
        };
    }
    if payload.trigger != crate::memory::SelfRuntimeTrigger::OperatorRequested
        && matches!(
            config.runtime.memory_system_kind.memory_profile(),
            crate::memory::MemoryProfile::Embedded
        )
    {
        let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
        if !runtime_mode.action_budget.allow_idle_self_runtime {
            let reason = runtime_mode
                .mode_block_reason()
                .unwrap_or("runtime_mode_blocked");
            log::debug!(
                "[self_runtime] defer chat_id={} trigger={:?} because runtime mode blocks self-runtime: {}",
                msg.chat_id,
                payload.trigger,
                reason
            );
            return DetachedJobRunDisposition::RetryLater {
                reason,
                delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
            };
        }
        match crate::orchestrator::current_pressure() {
            crate::orchestrator::PressureLevel::Normal => {}
            crate::orchestrator::PressureLevel::Cautious => {
                if payload.trigger == crate::memory::SelfRuntimeTrigger::PostReply
                    && super::now_unix_ms().saturating_sub(payload.now_secs.saturating_mul(1000))
                        >= crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS
                {
                    log::info!(
                        "[self_runtime] lightweight post-reply runtime advanced chat_id={} after bounded deferral",
                        msg.chat_id
                    );
                    return DetachedJobRunDisposition::Completed;
                }
                log::debug!(
                    "[self_runtime] defer chat_id={} trigger={:?} because pressure is cautious",
                    msg.chat_id,
                    payload.trigger
                );
                return DetachedJobRunDisposition::RetryLater {
                    reason: "self_runtime_pressure_cautious",
                    delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
                };
            }
            crate::orchestrator::PressureLevel::Critical => {
                if payload.trigger == crate::memory::SelfRuntimeTrigger::PostReply
                    && super::now_unix_ms().saturating_sub(payload.now_secs.saturating_mul(1000))
                        >= crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS
                {
                    log::info!(
                        "[self_runtime] lightweight post-reply runtime advanced chat_id={} after bounded deferral",
                        msg.chat_id
                    );
                    return DetachedJobRunDisposition::Completed;
                }
                log::debug!(
                    "[self_runtime] defer chat_id={} trigger={:?} because pressure is critical",
                    msg.chat_id,
                    payload.trigger
                );
                return DetachedJobRunDisposition::RetryLater {
                    reason: "self_runtime_pressure_critical",
                    delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
                };
            }
        }
    }
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let outcome = run_self_runtime(
        &mut llm_ctx,
        worker_llm,
        SelfRuntimeContext {
            memory_system_kind: config.runtime.memory_system_kind,
            session_store: config.runtime.session_store.as_ref(),
            memory_store: config.runtime.memory_store.as_ref(),
            session_summary_store: config.runtime.session_summary_store.as_ref(),
            execution_state_store: config.runtime.execution_state_store.as_ref(),
            long_term_memory_store: config.runtime.long_term_memory_store.as_ref(),
            continuity_capsule_store: config.runtime.continuity_capsule_store.as_ref(),
            self_model_store: config.runtime.self_model_store.as_ref(),
            self_authored_core_store: config.runtime.self_authored_core_store.as_ref(),
            core_revision_ledger_store: config.runtime.core_revision_ledger_store.as_ref(),
            relationship_constitution_store: config
                .runtime
                .relationship_constitution_store
                .as_ref(),
            relationship_portfolio_store: config.runtime.relationship_portfolio_store.as_ref(),
            relationship_topology_store: config.runtime.relationship_topology_store.as_ref(),
            world_sense_store: config.runtime.world_sense_store.as_ref(),
            autonomy_strategy_store: config.runtime.autonomy_strategy_store.as_ref(),
            outer_voice_store: config.runtime.outer_voice_store.as_ref(),
            private_doc_store: config.runtime.private_doc_store.as_ref(),
            private_garden_store: config.runtime.private_garden_store.as_ref(),
            inner_life_store: config.runtime.inner_life_store.as_ref(),
            self_continuity_store: config.runtime.self_continuity_store.as_ref(),
            felt_significance_store: config.runtime.felt_significance_store.as_ref(),
            temperament_continuity_store: config.runtime.temperament_continuity_store.as_ref(),
            inner_conflict_store: config.runtime.inner_conflict_store.as_ref(),
            mental_privacy_store: config.runtime.mental_privacy_store.as_ref(),
            remind_store: config.runtime.remind_at_store.as_ref(),
            task_store: config.runtime.task_store.as_ref(),
            task_run_store: config.runtime.task_run_store.as_ref(),
            task_artifact_store: config.runtime.task_artifact_store.as_ref(),
            task_learning_store: config.runtime.task_learning_store.as_ref(),
            turn_ledger_store: config.runtime.turn_ledger_store.as_ref(),
            skill_storage: config.runtime.skill_storage.as_ref(),
        },
        &msg.chat_id,
        &payload,
    );
    let crate::memory::SelfRuntimeOutcome {
        decision,
        world_sense_result,
        autonomy_strategy_result,
        inner_life_result,
        felt_significance_result,
        temperament_continuity_result,
        inner_conflict_result,
        private_doc_result,
        self_model_result,
        self_authored_core_result,
        self_continuity_result,
        task_learning_result,
        private_garden_result,
        boundary_persona_result,
        outer_voice_result,
    } = *outcome;
    let self_runtime_failed = world_sense_result.is_err()
        || autonomy_strategy_result.is_err()
        || inner_life_result.is_err()
        || felt_significance_result.is_err()
        || temperament_continuity_result.is_err()
        || inner_conflict_result.is_err()
        || private_doc_result.is_err()
        || self_model_result.is_err()
        || self_authored_core_result.is_err()
        || self_continuity_result.is_err()
        || task_learning_result.is_err()
        || private_garden_result.is_err()
        || boundary_persona_result.is_err()
        || outer_voice_result.is_err();
    if let Some(decision) = decision.as_ref() {
        log::info!(
            "[self_runtime] {} trigger={:?} inner_life={} private_docs={} private_docs_action={} self_model={} self_authored_core={} self_continuity={} private_garden={} private_garden_action={} boundary_persona={} outer_voice={} boundary_flush={} boundary_reason={:?} factual_refresh={} factual_action={} inner_life_intent={:?} private_docs_intent={:?} self_model_intent={:?} self_authored_core_intent={:?} self_continuity_intent={:?} private_garden_intent={:?} boundary_persona_intent={:?} outer_voice_intent={:?} factual_reconcile_intent={:?}",
            msg.chat_id,
            payload.trigger,
            decision.refresh_inner_life,
            decision.refresh_private_docs,
            decision.private_docs_action.label(),
            decision.refresh_self_model,
            decision.refresh_self_authored_core,
            decision.refresh_self_continuity,
            decision.refresh_private_garden,
            decision.private_garden_action.label(),
            decision.refresh_boundary_persona,
            decision.refresh_outer_voice,
            decision.boundary_flush,
            (!decision.boundary_flush_reason.trim().is_empty())
                .then_some(decision.boundary_flush_reason.as_str()),
            decision.request_factual_refresh,
            decision.factual_reconcile_action.label(),
            (!decision.inner_life_intent.trim().is_empty())
                .then_some(decision.inner_life_intent.as_str()),
            (!decision.private_docs_intent.trim().is_empty())
                .then_some(decision.private_docs_intent.as_str()),
            (!decision.self_model_intent.trim().is_empty())
                .then_some(decision.self_model_intent.as_str()),
            (!decision.self_authored_core_intent.trim().is_empty())
                .then_some(decision.self_authored_core_intent.as_str()),
            (!decision.self_continuity_intent.trim().is_empty())
                .then_some(decision.self_continuity_intent.as_str()),
            (!decision.private_garden_intent.trim().is_empty())
                .then_some(decision.private_garden_intent.as_str()),
            (!decision.boundary_persona_intent.trim().is_empty())
                .then_some(decision.boundary_persona_intent.as_str()),
            (!decision.outer_voice_intent.trim().is_empty())
                .then_some(decision.outer_voice_intent.as_str()),
            (!decision.factual_reconcile_intent.trim().is_empty())
                .then_some(decision.factual_reconcile_intent.as_str()),
        );
        if decision.request_factual_refresh {
            match PcMsg::new_system(CHANNEL_LONG_TERM_MEMORY_REFRESH, msg.chat_id.as_ref(), "") {
                Ok(job) => {
                    let _ = crate::agent::upsert_detached_work_job(
                        config.runtime.detached_work_store.as_ref(),
                        crate::agent::DetachedWorkKey::new(
                            "memory_refresh",
                            msg.chat_id.as_ref(),
                            crate::agent::DetachedJobKind::LongTermMemoryRefresh,
                        ),
                        &job,
                        0,
                        "self_runtime_factual_refresh_enqueued",
                    )
                    .map_err(|error| {
                        log::warn!(
                            "[self_runtime] factual refresh detached enqueue failed: {}",
                            error
                        );
                    });
                }
                Err(error) => {
                    log::warn!("[self_runtime] factual refresh job build failed: {}", error);
                }
            }
        }
    }
    match world_sense_result {
        Ok(crate::memory::WorldSenseRefreshOutcome::Updated) => {
            log::info!("[agent_world_sense] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::WorldSenseRefreshOutcome::Cleared) => {
            log::info!("[agent_world_sense] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::WorldSenseRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_world_sense] failed: {}", error),
    }
    match autonomy_strategy_result {
        Ok(crate::memory::AutonomyStrategyRefreshOutcome::Updated) => {
            log::info!("[agent_autonomy_strategy] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::AutonomyStrategyRefreshOutcome::Cleared) => {
            log::info!("[agent_autonomy_strategy] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::AutonomyStrategyRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_autonomy_strategy] failed: {}", error),
    }
    match outer_voice_result {
        Ok(crate::memory::OuterVoiceRefreshOutcome::Updated) => {
            log::info!("[agent_outer_voice] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::OuterVoiceRefreshOutcome::Cleared) => {
            log::info!("[agent_outer_voice] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::OuterVoiceRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_outer_voice] failed: {}", error),
    }
    match inner_life_result {
        Ok(crate::memory::InnerLifeRefreshOutcome::Updated) => {
            log::info!("[agent_inner_life] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::InnerLifeRefreshOutcome::Cleared) => {
            log::info!("[agent_inner_life] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::InnerLifeRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_inner_life] failed: {}", error),
    }
    match felt_significance_result {
        Ok(crate::memory::FeltSignificanceRefreshOutcome::Updated) => {
            log::info!("[agent_felt_significance] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::FeltSignificanceRefreshOutcome::Cleared) => {
            log::info!("[agent_felt_significance] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::FeltSignificanceRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_felt_significance] failed: {}", error),
    }
    match temperament_continuity_result {
        Ok(crate::memory::TemperamentContinuityRefreshOutcome::Updated) => {
            log::info!("[agent_temperament_continuity] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::TemperamentContinuityRefreshOutcome::Cleared) => {
            log::info!("[agent_temperament_continuity] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::TemperamentContinuityRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_temperament_continuity] failed: {}", error),
    }
    match inner_conflict_result {
        Ok(crate::memory::InnerConflictRefreshOutcome::Updated) => {
            log::info!("[agent_inner_conflict] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::InnerConflictRefreshOutcome::Cleared) => {
            log::info!("[agent_inner_conflict] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::InnerConflictRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_inner_conflict] failed: {}", error),
    }
    match private_doc_result {
        Ok(crate::memory::PrivateDocWorkspaceRefreshOutcome::Updated) => {
            log::info!("[self_runtime_private_docs] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::PrivateDocWorkspaceRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[self_runtime_private_docs] failed: {}", error),
    }
    match self_model_result {
        Ok(crate::memory::SelfModelRefreshOutcome::Updated) => {
            log::info!("[agent_self_model] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::SelfModelRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_self_model] failed: {}", error),
    }
    match self_authored_core_result {
        Ok(crate::memory::SelfAuthoredCoreRefreshOutcome::Updated) => {
            log::info!("[agent_self_authored_core] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::SelfAuthoredCoreRefreshOutcome::ReviewedRejected) => {
            log::info!(
                "[agent_self_authored_core] reviewed and rejected for {}",
                msg.chat_id
            );
        }
        Ok(crate::memory::SelfAuthoredCoreRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_self_authored_core] failed: {}", error),
    }
    match self_continuity_result {
        Ok(crate::memory::SelfContinuityRefreshOutcome::Updated) => {
            log::info!("[agent_self_continuity] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::SelfContinuityRefreshOutcome::Cleared) => {
            log::info!("[agent_self_continuity] cleared for {}", msg.chat_id);
        }
        Ok(crate::memory::SelfContinuityRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_self_continuity] failed: {}", error),
    }
    match task_learning_result {
        Ok(ref outcome) if outcome.considered > 0 => log::info!(
            "[self_runtime] method_distillation chat_id={} considered={} canonical={} runtime_skill={} archived={} pruned={} rejected={}",
            msg.chat_id,
            outcome.considered,
            outcome.canonical_writes,
            outcome.runtime_skill_promotions,
            outcome.archived_records,
            outcome.pruned_artifacts,
            outcome.rejected
        ),
        Ok(_) => {}
        Err(error) => log::warn!("[self_runtime] method_distillation failed: {}", error),
    }
    match private_garden_result {
        Ok(crate::memory::PrivateGardenGovernanceOutcome::Updated {
            writes,
            moves,
            deletes,
        }) => {
            log::info!(
                "[self_runtime_private_garden] updated for {} (writes={}, moves={}, deletes={})",
                msg.chat_id,
                writes,
                moves,
                deletes
            );
        }
        Ok(crate::memory::PrivateGardenGovernanceOutcome::Skipped) => {}
        Err(error) => log::warn!("[self_runtime_private_garden] failed: {}", error),
    }
    match boundary_persona_result {
        Ok(crate::memory::BoundaryPersonaRefreshOutcome::Updated) => {
            log::info!("[agent_boundary_persona] updated for {}", msg.chat_id);
        }
        Ok(crate::memory::BoundaryPersonaRefreshOutcome::Skipped) => {}
        Err(error) => log::warn!("[agent_boundary_persona] failed: {}", error),
    }
    if self_runtime_failed {
        return DetachedJobRunDisposition::RetryLater {
            reason: "self_runtime_incomplete",
            delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
        };
    }
    DetachedJobRunDisposition::Completed
}

fn append_operator_maintenance_workflow_audit(
    request: &crate::runtime::OperatorMaintenanceRequest,
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::OperatorMaintenance,
            crate::runtime::WorkflowTrigger::OperatorRequested,
            disposition,
            effect,
            crate::runtime::WorkflowRecoveryPolicy::RetryAfterModeResume,
            rationale,
            crate::util::current_unix_secs(),
        )
        .with_target(None, request.channel.as_deref(), request.chat_id.as_deref()),
    );
}

fn resolve_operator_maintenance_target(
    config: &AgentLoopConfig,
    request: &crate::runtime::OperatorMaintenanceRequest,
    now_secs: u64,
) -> Option<(String, String)> {
    if let Some(chat_id) = request
        .chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let channel = request
            .channel
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("operator_maintenance");
        return Some((chat_id.to_string(), channel.to_string()));
    }
    let subject_id = crate::memory::board_subject_scope_id();
    let self_continuity = config
        .runtime
        .self_continuity_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_portfolio = config
        .runtime
        .relationship_portfolio_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_topology = config
        .runtime
        .relationship_topology_store
        .get(subject_id)
        .ok()
        .flatten();
    crate::memory::select_personality_governance_targets(
        self_continuity.as_ref(),
        relationship_portfolio.as_ref(),
        relationship_topology.as_ref(),
        now_secs,
        1,
    )
    .into_iter()
    .next()
    .map(|target| (target.chat_id, target.channel))
}

fn rebuild_operator_continuity_snapshots(
    config: &AgentLoopConfig,
    request: &crate::runtime::OperatorMaintenanceRequest,
    now_secs: u64,
) -> crate::error::Result<usize> {
    let mut target_chat_ids = if let Some(chat_id) = request
        .chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![chat_id.to_string()]
    } else {
        crate::memory::select_active_continuity_snapshot_chat_ids(
            config.runtime.session_store.as_ref(),
            config.runtime.self_continuity_store.as_ref(),
            config.runtime.relationship_portfolio_store.as_ref(),
            config.runtime.relationship_topology_store.as_ref(),
            None,
            now_secs,
            7 * 86_400,
            4,
        )
    };
    target_chat_ids.sort();
    target_chat_ids.dedup();
    let mut exported = 0usize;
    for chat_id in target_chat_ids {
        let snapshot = crate::memory::export_continuity_snapshot(
            crate::memory::ContinuitySnapshotExportContext {
                long_term_memory_store: config.runtime.long_term_memory_store.as_ref(),
                session_summary_store: config.runtime.session_summary_store.as_ref(),
                execution_state_store: config.runtime.execution_state_store.as_ref(),
                self_model_store: config.runtime.self_model_store.as_ref(),
                self_authored_core_store: config.runtime.self_authored_core_store.as_ref(),
                core_revision_ledger_store: config.runtime.core_revision_ledger_store.as_ref(),
                self_continuity_store: config.runtime.self_continuity_store.as_ref(),
                relationship_constitution_store: config
                    .runtime
                    .relationship_constitution_store
                    .as_ref(),
                relationship_portfolio_store: config.runtime.relationship_portfolio_store.as_ref(),
                relationship_topology_store: config.runtime.relationship_topology_store.as_ref(),
            },
            chat_id.as_str(),
            crate::memory::ContinuitySnapshotMode::FullRestore,
            now_secs,
        )?;
        let sanitized = chat_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let rel_path = format!(
            "memory/continuity_snapshots/manual/operator_{}_{}.json",
            sanitized.trim_matches('_'),
            now_secs
        );
        let payload = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
            crate::error::Error::config("operator_maintenance_snapshot", error.to_string())
        })?;
        config
            .runtime
            .platform
            .state_fs()
            .write(rel_path.as_str(), &payload)?;
        exported = exported.saturating_add(1);
    }
    Ok(exported)
}

fn run_operator_maintenance_job(
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) -> DetachedJobRunDisposition {
    let request: crate::runtime::OperatorMaintenanceRequest =
        match serde_json::from_str(&msg.content) {
            Ok(request) => request,
            Err(error) => {
                log::warn!(
                    "[operator_maintenance] decode failed chat_id={}: {}",
                    msg.chat_id,
                    error
                );
                return DetachedJobRunDisposition::PermanentDrop {
                    reason: "operator_maintenance_payload_invalid",
                };
            }
        };
    let now_secs = crate::util::current_unix_secs();
    match request.action {
        crate::runtime::OperatorMaintenanceAction::RunRepairPlan
        | crate::runtime::OperatorMaintenanceAction::ReconcileRelationshipGovernance => {
            let Some((chat_id, source_channel)) =
                resolve_operator_maintenance_target(config, &request, now_secs)
            else {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::NoTrigger,
                    "operator_target_unavailable",
                    crate::runtime::WorkflowEffect::Noop,
                );
                return DetachedJobRunDisposition::Completed;
            };
            if crate::memory::enqueue_self_runtime_operator_request(
                system_inbound_tx,
                config.runtime.detached_work_store.as_ref(),
                chat_id.as_str(),
                source_channel.as_str(),
            ) {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteNow,
                    "operator_repair_dispatched",
                    crate::runtime::WorkflowEffect::RunRepairPass,
                );
                DetachedJobRunDisposition::Completed
            } else {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteFailed,
                    "operator_repair_enqueue_failed",
                    crate::runtime::WorkflowEffect::Noop,
                );
                DetachedJobRunDisposition::RetryLater {
                    reason: "operator_repair_enqueue_failed",
                    delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
                }
            }
        }
        crate::runtime::OperatorMaintenanceAction::RebuildContinuitySnapshot => {
            match rebuild_operator_continuity_snapshots(config, &request, now_secs) {
                Ok(0) => {
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::NoTrigger,
                        "operator_snapshot_target_unavailable",
                        crate::runtime::WorkflowEffect::Noop,
                    );
                    DetachedJobRunDisposition::Completed
                }
                Ok(count) => {
                    log::info!(
                        "[operator_maintenance] continuity snapshots rebuilt count={}",
                        count
                    );
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteNow,
                        "operator_snapshot_rebuilt",
                        crate::runtime::WorkflowEffect::PersistRecoveryIntent,
                    );
                    DetachedJobRunDisposition::Completed
                }
                Err(error) => {
                    log::warn!("[operator_maintenance] snapshot rebuild failed: {}", error);
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "operator_snapshot_rebuild_failed",
                        crate::runtime::WorkflowEffect::Noop,
                    );
                    DetachedJobRunDisposition::RetryLater {
                        reason: "operator_snapshot_rebuild_failed",
                        delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
                    }
                }
            }
        }
        crate::runtime::OperatorMaintenanceAction::ReplayRecovery => {
            let report = crate::runtime::ensure_platform_soul_kernel_recovery(
                config.runtime.platform.as_ref(),
                now_secs,
            );
            if report.restore_attempted {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteNow,
                    "operator_recovery_replayed",
                    crate::runtime::WorkflowEffect::ReplayRecovery,
                );
                DetachedJobRunDisposition::Completed
            } else {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::NoTrigger,
                    "operator_recovery_already_steady",
                    crate::runtime::WorkflowEffect::Noop,
                );
                DetachedJobRunDisposition::Completed
            }
        }
        crate::runtime::OperatorMaintenanceAction::RefreshOperatorDigest => {
            match crate::platform::memory_operator_surface::build_memory_operator_surface(
                config.runtime.platform.as_ref(),
                None,
                None,
            ) {
                Ok(surface) => {
                    log::info!(
                        "[operator_maintenance] operator digest refreshed repair_needed={} primary_action={}",
                        surface.repair.repair_needed,
                        surface.repair.primary_action
                    );
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteNow,
                        "operator_digest_refreshed",
                        crate::runtime::WorkflowEffect::Noop,
                    );
                    DetachedJobRunDisposition::Completed
                }
                Err(error) => {
                    log::warn!("[operator_maintenance] digest refresh failed: {}", error);
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "operator_digest_refresh_failed",
                        crate::runtime::WorkflowEffect::Noop,
                    );
                    DetachedJobRunDisposition::RetryLater {
                        reason: "operator_digest_refresh_failed",
                        delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
                    }
                }
            }
        }
    }
}

#[cold]
#[inline(never)]
pub(super) fn try_run_lane_background_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
    current_background_agent_task_slots: u32,
) -> DetachedJobRunDisposition {
    if super::is_long_term_memory_refresh_job(msg) {
        return run_long_term_memory_refresh_job(http, worker_llm, config, msg);
    }
    if super::is_post_reply_maintenance_job(msg) {
        return run_post_reply_maintenance_job(
            http,
            worker_llm,
            config,
            system_inbound_tx,
            msg,
            current_background_agent_task_slots,
        );
    }
    if super::is_idle_memory_forge_job(msg) {
        return run_idle_memory_forge_job(config, msg);
    }
    if super::is_self_runtime_job(msg) {
        return run_self_runtime_job(http, worker_llm, config, system_inbound_tx, msg);
    }
    if super::is_operator_maintenance_job(msg) {
        return run_operator_maintenance_job(config, system_inbound_tx, msg);
    }
    DetachedJobRunDisposition::PermanentDrop {
        reason: "unknown_detached_job",
    }
}

fn detached_workflow_identity(
    kind: crate::agent::DetachedJobKind,
) -> (
    crate::runtime::WorkflowKind,
    crate::runtime::WorkflowTrigger,
) {
    match kind {
        crate::agent::DetachedJobKind::LongTermMemoryRefresh => (
            crate::runtime::WorkflowKind::LongTermMemoryRefresh,
            crate::runtime::WorkflowTrigger::PostReply,
        ),
        crate::agent::DetachedJobKind::PostReplyMaintenance => (
            crate::runtime::WorkflowKind::PostReplyMaintenance,
            crate::runtime::WorkflowTrigger::PostReply,
        ),
        crate::agent::DetachedJobKind::IdleMemoryForge => (
            crate::runtime::WorkflowKind::IdleMemoryForge,
            crate::runtime::WorkflowTrigger::CronTick,
        ),
        crate::agent::DetachedJobKind::SelfRuntimePostReply => (
            crate::runtime::WorkflowKind::SelfRuntimePostReply,
            crate::runtime::WorkflowTrigger::PostReply,
        ),
        crate::agent::DetachedJobKind::SelfRuntimeIdleTick => (
            crate::runtime::WorkflowKind::SelfRuntimeIdleTick,
            crate::runtime::WorkflowTrigger::CronTick,
        ),
        crate::agent::DetachedJobKind::OperatorMaintenance => (
            crate::runtime::WorkflowKind::OperatorMaintenance,
            crate::runtime::WorkflowTrigger::OperatorRequested,
        ),
    }
}

fn detached_work_defer_delay_ms(
    kind: crate::agent::DetachedJobKind,
    reason: &str,
    explicit_delay_ms: Option<u64>,
) -> u64 {
    explicit_delay_ms.unwrap_or_else(|| match reason {
        "external_wss_active" if kind == crate::agent::DetachedJobKind::SelfRuntimeIdleTick => {
            super::IDLE_SELF_RUNTIME_RETRY_DELAY_MS
        }
        _ => super::BACKGROUND_DEFER_DELAY_MS,
    })
}

fn append_detached_work_defer_audit(key: &crate::agent::DetachedWorkKey, reason: &str) {
    let (workflow, trigger) = detached_workflow_identity(key.kind);
    super::append_background_defer_workflow_audit(
        key.owner_channel.as_str(),
        key.owner_chat_id.as_str(),
        trigger,
        workflow,
        reason,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackgroundGatePolicy {
    Run,
    Lightweight,
    Defer { reason: &'static str, delay_ms: u64 },
}

fn embedded_post_reply_gate_applies(
    memory_profile: crate::memory::MemoryProfile,
    kind: crate::agent::DetachedJobKind,
) -> bool {
    matches!(memory_profile, crate::memory::MemoryProfile::Embedded)
        && matches!(
            kind,
            crate::agent::DetachedJobKind::PostReplyMaintenance
                | crate::agent::DetachedJobKind::SelfRuntimePostReply
        )
}

fn post_reply_pressure_defer_reason_for_level(
    pressure: crate::orchestrator::PressureLevel,
) -> Option<(&'static str, u64)> {
    match pressure {
        crate::orchestrator::PressureLevel::Normal => None,
        crate::orchestrator::PressureLevel::Cautious => Some((
            "post_reply_pressure_cautious",
            super::BACKGROUND_DEFER_DELAY_MS,
        )),
        crate::orchestrator::PressureLevel::Critical => Some((
            "post_reply_pressure_critical",
            super::BACKGROUND_DEFER_DELAY_MS,
        )),
    }
}

fn embedded_post_reply_pressure_policy(
    memory_profile: crate::memory::MemoryProfile,
    kind: crate::agent::DetachedJobKind,
    pressure: crate::orchestrator::PressureLevel,
) -> BackgroundGatePolicy {
    if !embedded_post_reply_gate_applies(memory_profile, kind) {
        return BackgroundGatePolicy::Run;
    }
    match post_reply_pressure_defer_reason_for_level(pressure) {
        Some((reason, delay_ms)) => BackgroundGatePolicy::Defer { reason, delay_ms },
        None => BackgroundGatePolicy::Run,
    }
}

fn embedded_post_reply_gate_policy(
    memory_profile: crate::memory::MemoryProfile,
    kind: crate::agent::DetachedJobKind,
    resource: &crate::orchestrator::ResourceSnapshot,
    current_background_agent_task_slots: u32,
    first_deferred_at_ms: Option<u64>,
    now_ms: u64,
) -> BackgroundGatePolicy {
    if !embedded_post_reply_gate_applies(memory_profile, kind) {
        return BackgroundGatePolicy::Run;
    }
    if first_deferred_at_ms
        .filter(|first| {
            now_ms.saturating_sub(*first) >= crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS
        })
        .is_some()
    {
        return BackgroundGatePolicy::Lightweight;
    }
    let largest_low = resource.heap_largest_block_internal > 0
        && resource.heap_largest_block_internal
            < crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
    let other_active_agent_tasks = resource
        .active_agent_tasks
        .saturating_sub(current_background_agent_task_slots);
    // Established WSS sessions are steady-state channel capacity, not short foreground work.
    let foreground_busy = resource.active_http_count > 0
        || other_active_agent_tasks > 0
        || resource.inbound_depth > 0
        || resource.outbound_depth > 0;
    if post_reply_pressure_defer_reason_for_level(resource.pressure).is_some()
        || largest_low
        || foreground_busy
    {
        return BackgroundGatePolicy::Defer {
            reason: "post_reply_resource_window_busy",
            delay_ms: super::BACKGROUND_DEFER_DELAY_MS,
        };
    }
    BackgroundGatePolicy::Run
}

fn detached_work_gate_policy(
    config: &AgentLoopConfig,
    key: &crate::agent::DetachedWorkKey,
) -> Result<BackgroundGatePolicy> {
    if matches!(
        key.kind,
        crate::agent::DetachedJobKind::PostReplyMaintenance
            | crate::agent::DetachedJobKind::SelfRuntimePostReply
    ) {
        if let Some(delay_ms) = super::post_reply_quiet_delay_ms() {
            return Ok(BackgroundGatePolicy::Defer {
                reason: "post_reply_quiet_window",
                delay_ms,
            });
        }
    }
    match embedded_post_reply_pressure_policy(
        config.runtime.memory_system_kind.memory_profile(),
        key.kind,
        crate::orchestrator::current_pressure(),
    ) {
        BackgroundGatePolicy::Defer { reason, delay_ms } => {
            return Ok(BackgroundGatePolicy::Defer { reason, delay_ms });
        }
        BackgroundGatePolicy::Run | BackgroundGatePolicy::Lightweight => {}
    }
    let live = crate::agent::live_foreground_state_for_chat(
        config.runtime.active_work_store.as_ref(),
        key.owner_chat_id.as_str(),
    )?;
    Ok(
        match crate::agent::classify_background_job_disposition(key.kind, live) {
            crate::agent::BackgroundDisposition::RunNow => BackgroundGatePolicy::Run,
            crate::agent::BackgroundDisposition::Defer(reason) => BackgroundGatePolicy::Defer {
                reason,
                delay_ms: detached_work_defer_delay_ms(key.kind, reason, None),
            },
        },
    )
}

fn reschedule_detached_work(
    store: &dyn crate::agent::DetachedWorkStore,
    key: &crate::agent::DetachedWorkKey,
    revision: u64,
    reason: &str,
    delay_ms: u64,
) {
    let wake_at_ms = crate::agent::current_unix_ms().saturating_add(delay_ms);
    match store.reschedule(key, revision, wake_at_ms, reason) {
        Ok(Some(_)) => append_detached_work_defer_audit(key, reason),
        Ok(None) => {}
        Err(error) => {
            log::warn!(
                "[agent] detached work reschedule failed channel={} chat_id={} kind={:?}: {}",
                key.owner_channel,
                key.owner_chat_id,
                key.kind,
                error
            );
        }
    }
}

fn apply_detached_job_run_disposition(
    store: &dyn crate::agent::DetachedWorkStore,
    key: &crate::agent::DetachedWorkKey,
    revision: u64,
    disposition: DetachedJobRunDisposition,
) {
    match disposition {
        DetachedJobRunDisposition::Completed => {
            if let Err(error) = store.finish(key, revision) {
                log::warn!(
                    "[agent] detached work finish failed channel={} chat_id={} kind={:?}: {}",
                    key.owner_channel,
                    key.owner_chat_id,
                    key.kind,
                    error
                );
            }
        }
        DetachedJobRunDisposition::RetryLater { reason, delay_ms } => {
            reschedule_detached_work(store, key, revision, reason, delay_ms);
        }
        DetachedJobRunDisposition::PermanentDrop { reason } => {
            log::warn!(
                "[agent] detached work dropped channel={} chat_id={} kind={:?} reason={}",
                key.owner_channel,
                key.owner_chat_id,
                key.kind,
                reason
            );
            if let Err(error) = store.finish(key, revision) {
                log::warn!(
                    "[agent] detached work drop-finish failed channel={} chat_id={} kind={:?}: {}",
                    key.owner_channel,
                    key.owner_chat_id,
                    key.kind,
                    error
                );
            }
        }
    }
}

fn run_detached_background_work_wake(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    wake_msg: &PcMsg,
) {
    let wake: crate::agent::DetachedWorkWake = match serde_json::from_str(&wake_msg.content) {
        Ok(wake) => wake,
        Err(error) => {
            log::warn!(
                "[agent] detached wake decode failed chat_id={}: {}",
                wake_msg.chat_id,
                error
            );
            return;
        }
    };
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_periodic_maintenance {
        let reason = runtime_mode
            .mode_block_reason()
            .unwrap_or("runtime_mode_blocked");
        reschedule_detached_work(
            config.runtime.detached_work_store.as_ref(),
            &wake.key,
            wake.revision,
            reason,
            super::BACKGROUND_DEFER_DELAY_MS,
        );
        return;
    }
    let record = match config.runtime.detached_work_store.get(&wake.key) {
        Ok(Some(record)) => record,
        Ok(None) => return,
        Err(error) => {
            log::warn!(
                "[agent] detached wake load failed channel={} chat_id={} kind={:?}: {}",
                wake.key.owner_channel,
                wake.key.owner_chat_id,
                wake.key.kind,
                error
            );
            return;
        }
    };
    if record.revision != wake.revision {
        return;
    }
    match detached_work_gate_policy(config, &wake.key) {
        Ok(BackgroundGatePolicy::Defer { reason, delay_ms }) => {
            reschedule_detached_work(
                config.runtime.detached_work_store.as_ref(),
                &wake.key,
                wake.revision,
                reason,
                delay_ms,
            );
            return;
        }
        Ok(BackgroundGatePolicy::Run | BackgroundGatePolicy::Lightweight) => {}
        Err(error) => {
            log::warn!(
                "[agent] detached foreground gate failed channel={} chat_id={} kind={:?}: {}",
                wake.key.owner_channel,
                wake.key.owner_chat_id,
                wake.key.kind,
                error
            );
            reschedule_detached_work(
                config.runtime.detached_work_store.as_ref(),
                &wake.key,
                wake.revision,
                "foreground_state_unavailable",
                super::BACKGROUND_DEFER_DELAY_MS,
            );
            return;
        }
    }
    if wake.key.kind.needs_llm() {
        match crate::orchestrator::can_call_llm_for_channel_pub(&wake.key.owner_channel) {
            crate::orchestrator::admission::LlmDecision::Proceed => {}
            crate::orchestrator::admission::LlmDecision::RetryLater { delay_ms } => {
                reschedule_detached_work(
                    config.runtime.detached_work_store.as_ref(),
                    &wake.key,
                    wake.revision,
                    "llm_retry_later",
                    delay_ms,
                );
                return;
            }
            crate::orchestrator::admission::LlmDecision::Degrade { reason } => {
                reschedule_detached_work(
                    config.runtime.detached_work_store.as_ref(),
                    &wake.key,
                    wake.revision,
                    reason,
                    super::BACKGROUND_DEFER_DELAY_MS,
                );
                return;
            }
        }
    }
    let record = match config
        .runtime
        .detached_work_store
        .claim_running(&wake.key, wake.revision)
    {
        Ok(Some(record)) => record,
        Ok(None) => return,
        Err(error) => {
            log::warn!(
                "[agent] detached work claim failed channel={} chat_id={} kind={:?}: {}",
                wake.key.owner_channel,
                wake.key.owner_chat_id,
                wake.key.kind,
                error
            );
            return;
        }
    };
    let _agent_task_guard = crate::orchestrator::begin_agent_task();
    let _maintenance_scope = crate::runtime::BackgroundMaintenanceGuard::enter();
    apply_detached_job_run_disposition(
        config.runtime.detached_work_store.as_ref(),
        &wake.key,
        wake.revision,
        try_run_lane_background_job(http, worker_llm, config, system_inbound_tx, &record.job, 1),
    );
    metrics::record_system_message_done(false);
}

fn schedule_volatile_background_retry(
    system_inbound_tx: &SystemInboundTx,
    msg: PcMsg,
    delay_ms: u64,
    label: &'static str,
) {
    let due_at = std::time::Instant::now() + std::time::Duration::from_millis(delay_ms);
    let scheduled = if let Some(key) = super::detached_work_key_for_msg(&msg) {
        crate::runtime::schedule_bounded_keyed_system_inbound_msg(
            due_at,
            system_inbound_tx.clone(),
            msg,
            std::time::Duration::from_millis(super::BACKGROUND_DEFER_DELAY_MS),
            label,
            key.storage_key(),
            std::time::Duration::from_millis(crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS),
        )
    } else {
        crate::runtime::schedule_system_inbound_msg(
            due_at,
            system_inbound_tx.clone(),
            msg,
            std::time::Duration::from_millis(super::BACKGROUND_DEFER_DELAY_MS),
            label,
        )
    };
    if !scheduled {
        log::warn!(
            "[agent] volatile background retry dropped because delayed queue is full label={}",
            label
        );
    }
}

fn run_volatile_background_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: PcMsg,
) {
    let Some(key) = super::detached_work_key_for_msg(&msg) else {
        return;
    };
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_periodic_maintenance {
        let reason = runtime_mode
            .mode_block_reason()
            .unwrap_or("runtime_mode_blocked");
        append_detached_work_defer_audit(&key, reason);
        schedule_volatile_background_retry(
            system_inbound_tx,
            msg,
            super::BACKGROUND_DEFER_DELAY_MS,
            "volatile_background_mode_defer",
        );
        return;
    }
    match detached_work_gate_policy(config, &key) {
        Ok(BackgroundGatePolicy::Defer { reason, delay_ms }) => {
            append_detached_work_defer_audit(&key, reason);
            schedule_volatile_background_retry(
                system_inbound_tx,
                msg,
                delay_ms,
                "volatile_background_defer",
            );
            return;
        }
        Ok(BackgroundGatePolicy::Run | BackgroundGatePolicy::Lightweight) => {}
        Err(error) => {
            log::warn!(
                "[agent] volatile background foreground gate failed channel={} chat_id={} kind={:?}: {}",
                key.owner_channel,
                key.owner_chat_id,
                key.kind,
                error
            );
            schedule_volatile_background_retry(
                system_inbound_tx,
                msg,
                super::BACKGROUND_DEFER_DELAY_MS,
                "volatile_background_gate_retry",
            );
            return;
        }
    }
    if key.kind.needs_llm() {
        match crate::orchestrator::can_call_llm_for_channel_pub(&key.owner_channel) {
            crate::orchestrator::admission::LlmDecision::Proceed => {}
            crate::orchestrator::admission::LlmDecision::RetryLater { delay_ms } => {
                schedule_volatile_background_retry(
                    system_inbound_tx,
                    msg,
                    delay_ms,
                    "volatile_background_llm_retry",
                );
                return;
            }
            crate::orchestrator::admission::LlmDecision::Degrade { reason } => {
                log::debug!(
                    "[agent] volatile background deferred channel={} chat_id={} kind={:?} reason={}",
                    key.owner_channel,
                    key.owner_chat_id,
                    key.kind,
                    reason
                );
                schedule_volatile_background_retry(
                    system_inbound_tx,
                    msg,
                    super::BACKGROUND_DEFER_DELAY_MS,
                    "volatile_background_llm_defer",
                );
                return;
            }
        }
    }
    let _agent_task_guard = crate::orchestrator::begin_agent_task();
    let _maintenance_scope = crate::runtime::BackgroundMaintenanceGuard::enter();
    match try_run_lane_background_job(http, worker_llm, config, system_inbound_tx, &msg, 1) {
        DetachedJobRunDisposition::Completed => {}
        DetachedJobRunDisposition::RetryLater { reason, delay_ms } => {
            append_detached_work_defer_audit(&key, reason);
            schedule_volatile_background_retry(
                system_inbound_tx,
                msg,
                delay_ms,
                "volatile_background_retry",
            );
        }
        DetachedJobRunDisposition::PermanentDrop { reason } => {
            log::warn!(
                "[agent] volatile background dropped channel={} chat_id={} kind={:?} reason={}",
                key.owner_channel,
                key.owner_chat_id,
                key.kind,
                reason
            );
        }
    }
    metrics::record_system_message_done(false);
}

#[cold]
#[inline(never)]
pub(super) fn run_background_job_with_accounting(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    loc: UiLocale,
    msg: PcMsg,
) {
    if super::is_detached_work_wake(&msg) {
        run_detached_background_work_wake(http, worker_llm, config, system_inbound_tx, &msg);
        return;
    }
    if !super::should_persist_background_job_as_detached(
        &msg,
        config.runtime.memory_system_kind.memory_profile(),
    ) {
        run_volatile_background_job(http, worker_llm, config, system_inbound_tx, msg);
        return;
    }
    if let Ok(true) = super::adopt_background_job_as_detached(
        config.runtime.detached_work_store.as_ref(),
        &msg,
        "background_job_re_adopted",
    ) {
        return;
    }
    let _ = (user_inbound_tx, outbound_tx, loc);
}

#[cold]
#[inline(never)]
pub(super) fn handle_admission_defer(
    delay_ms: u64,
    mut msg: PcMsg,
    msg_key: u64,
    ctx: AdmissionDeferContext<'_>,
) {
    let entry = ctx
        .defer_tracker
        .entry(msg_key)
        .or_insert((0, Instant::now()));
    entry.0 = entry.0.saturating_add(1);
    entry.1 = Instant::now();
    let defer_count = entry.0;

    if defer_count >= MAX_DEFER_RETRIES && msg.ingress != IngressKind::User {
        log::warn!(
            "[agent] defer limit reached ({}) for chat_id={}, dropping message",
            MAX_DEFER_RETRIES,
            msg.chat_id
        );
        ctx.defer_tracker.remove(&msg_key);
        if msg.ingress == IngressKind::User {
            match PcMsg::new_outbound_reply_to(&msg, tr(UiMessage::LowMemoryUserDefer, ctx.loc)) {
                Ok(defer_out) => {
                    let _ = super::try_send_outbound(ctx.outbound_tx, defer_out, "defer-limit");
                }
                Err(error) => {
                    metrics::record_error_by_stage(error.metrics_stage());
                    log::error!(
                        "[agent] failed to build defer-limit reply channel={} chat_id={}: {}",
                        msg.channel,
                        msg.chat_id,
                        error
                    );
                }
            }
        }
        return;
    }

    if msg.ingress == IngressKind::User && defer_count < MAX_DEFER_RETRIES {
        match PcMsg::new_outbound_reply_to(&msg, tr(UiMessage::LowMemoryUserDefer, ctx.loc)) {
            Ok(defer_out) => {
                let _ = super::try_send_outbound(ctx.outbound_tx, defer_out, "defer");
            }
            Err(error) => {
                metrics::record_error_by_stage(error.metrics_stage());
                log::error!(
                    "[agent] failed to build defer reply channel={} chat_id={}: {}",
                    msg.channel,
                    msg.chat_id,
                    error
                );
            }
        }
    } else if msg.ingress == IngressKind::User {
        log::warn!(
            "[agent] defer limit reached ({}) for chat_id={}, keeping primary turn replayable",
            MAX_DEFER_RETRIES,
            msg.chat_id
        );
    }
    let chat_id = msg.chat_id.clone();
    msg.enqueue_ts_ms = super::now_unix_ms();
    let inbound_tx =
        super::choose_inbound_tx(msg.ingress, ctx.user_inbound_tx, ctx.system_inbound_tx);
    match inbound_tx.try_send(msg) {
        Ok(()) => {
            let now = Instant::now();
            let should_log = ctx
                .low_mem_defer_log
                .as_ref()
                .map(|(id, t)| {
                    id.as_ref() != chat_id.as_ref() || t.elapsed() >= LOW_MEM_DEFER_LOG_INTERVAL
                })
                .unwrap_or(true);
            if should_log {
                log::warn!("[agent] admission defer chat_id={}", chat_id);
                *ctx.low_mem_defer_log = Some((chat_id.clone(), now));
            }
        }
        Err(std::sync::mpsc::TrySendError::Full(m)) => {
            let _ = ctx
                .config
                .runtime
                .pending_retry_store
                .save_pending_retry(&m);
            log::warn!(
                "[agent] admission defer, pending_retry saved chat_id={}",
                m.chat_id
            );
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
            log::error!("[agent] inbound_tx disconnected");
        }
    }
    std::thread::sleep(Duration::from_millis(delay_ms));
    crate::platform::task_wdt::feed_current_task();
}

#[cold]
#[inline(never)]
pub(super) fn handle_admission_reject(
    reason: &str,
    low_mem_defer_log: &mut Option<(Arc<str>, Instant)>,
) {
    let now = Instant::now();
    let should_log = low_mem_defer_log
        .as_ref()
        .map(|(id, t)| id.as_ref() != reason || t.elapsed() >= LOW_MEM_DEFER_LOG_INTERVAL)
        .unwrap_or(true);
    if should_log {
        log::warn!("[agent] inbound rejected: {}", reason);
        *low_mem_defer_log = Some((Arc::from(reason), now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{
        ActiveWorkKind, ActiveWorkRecord, ActiveWorkStore, DetachedJobKind, DetachedWorkKey,
        DetachedWorkRecord, DetachedWorkState, DetachedWorkStore, DetachedWorkUpsertOutcome,
    };
    use crate::bus::PcMsg;
    use crate::error::Result;
    use crate::memory::PromptRecallIntent;
    use crate::runtime::system_work::{CHANNEL_POST_REPLY_MAINTENANCE, CHANNEL_SELF_RUNTIME};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubDetachedWorkStore {
        entries: Mutex<HashMap<String, DetachedWorkRecord>>,
    }

    impl DetachedWorkStore for StubDetachedWorkStore {
        fn get(&self, key: &DetachedWorkKey) -> Result<Option<DetachedWorkRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key.storage_key())
                .cloned())
        }

        fn list(&self) -> Result<Vec<DetachedWorkRecord>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn upsert(
            &self,
            key: &DetachedWorkKey,
            job: &PcMsg,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<DetachedWorkUpsertOutcome> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let next = DetachedWorkRecord {
                key: key.clone(),
                job: job.clone(),
                state: DetachedWorkState::Pending,
                wake_at_ms,
                revision: 1,
                last_reason: reason.to_string(),
                updated_at_ms: 1,
            };
            entries.insert(key.storage_key(), next.clone());
            Ok(DetachedWorkUpsertOutcome {
                changed: true,
                record: next,
            })
        }

        fn mark_queued(&self, key: &DetachedWorkKey, revision: u64) -> Result<bool> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(false);
            };
            if record.revision != revision || record.state != DetachedWorkState::Pending {
                return Ok(false);
            }
            record.state = DetachedWorkState::Queued;
            Ok(true)
        }

        fn claim_running(
            &self,
            key: &DetachedWorkKey,
            revision: u64,
        ) -> Result<Option<DetachedWorkRecord>> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision
                || !matches!(
                    record.state,
                    DetachedWorkState::Pending | DetachedWorkState::Queued
                )
            {
                return Ok(None);
            }
            record.state = DetachedWorkState::Running;
            Ok(Some(record.clone()))
        }

        fn reschedule(
            &self,
            key: &DetachedWorkKey,
            revision: u64,
            wake_at_ms: u64,
            reason: &str,
        ) -> Result<Option<DetachedWorkRecord>> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let Some(record) = entries.get_mut(&key.storage_key()) else {
                return Ok(None);
            };
            if record.revision != revision {
                return Ok(None);
            }
            record.revision = record.revision.saturating_add(1);
            record.state = DetachedWorkState::Pending;
            record.wake_at_ms = wake_at_ms;
            record.last_reason = reason.to_string();
            Ok(Some(record.clone()))
        }

        fn finish(&self, key: &DetachedWorkKey, revision: u64) -> Result<()> {
            let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            if entries
                .get(&key.storage_key())
                .is_some_and(|record| record.revision == revision)
            {
                entries.remove(&key.storage_key());
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubActiveWorkStore {
        value: Mutex<Option<ActiveWorkRecord>>,
    }

    impl ActiveWorkStore for StubActiveWorkStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ActiveWorkRecord>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, record: &ActiveWorkRecord) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(record.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[test]
    fn retry_later_keeps_detached_work_record() {
        let store = StubDetachedWorkStore::default();
        let key =
            DetachedWorkKey::new("qq_channel", "chat-1", DetachedJobKind::SelfRuntimeIdleTick);
        let job = PcMsg::new_system(CHANNEL_SELF_RUNTIME, "chat-1", "{}").expect("job");
        let stored = store
            .upsert(&key, &job, 10, "seeded")
            .expect("upsert")
            .record;

        apply_detached_job_run_disposition(
            &store,
            &key,
            stored.revision,
            DetachedJobRunDisposition::RetryLater {
                reason: "temporary_failure",
                delay_ms: 50,
            },
        );

        let record = store.get(&key).expect("load").expect("record");
        assert_eq!(record.state, DetachedWorkState::Pending);
        assert_eq!(record.last_reason, "temporary_failure");
        assert!(record.revision > stored.revision);
    }

    #[test]
    fn permanent_drop_removes_detached_work_record() {
        let store = StubDetachedWorkStore::default();
        let key = DetachedWorkKey::new(
            "qq_channel",
            "chat-1",
            DetachedJobKind::PostReplyMaintenance,
        );
        let job = PcMsg::new_system(CHANNEL_POST_REPLY_MAINTENANCE, "chat-1", "{}").expect("job");
        let stored = store
            .upsert(&key, &job, 10, "seeded")
            .expect("upsert")
            .record;

        apply_detached_job_run_disposition(
            &store,
            &key,
            stored.revision,
            DetachedJobRunDisposition::PermanentDrop {
                reason: "invalid_payload",
            },
        );

        assert!(store.get(&key).expect("load").is_none());
    }

    #[test]
    fn post_reply_maintenance_is_not_scheduled_while_foreground_work_is_active() {
        let store = StubDetachedWorkStore::default();
        let active_work_store = StubActiveWorkStore {
            value: Mutex::new(Some(ActiveWorkRecord {
                kind: ActiveWorkKind::InteractiveAction,
                title: "配置 QQ 邮箱账户".to_string(),
                status: crate::agent::ForegroundWorkStatus::AwaitingUser,
                continuity_open: true,
                blocks_background_llm: true,
                progress_summary: String::new(),
                blocker: "缺少 SMTP 授权码".to_string(),
                next_action: "等待用户补充 SMTP 授权码".to_string(),
                recent_outcome: String::new(),
                active_artifact_refs: Vec::new(),
                updated_at: 9,
            })),
        };
        let (system_inbound_tx, _system_inbound_rx, _depth) = crate::bus::new_inbound_channel(4);
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");

        let scheduled = enqueue_post_reply_maintenance_job(
            &active_work_store,
            &store,
            &system_inbound_tx,
            &msg,
            "请先提供 SMTP 授权码。",
            0,
            false,
            PromptRecallIntent::default(),
            &[],
            &[],
            crate::skills::RuntimeSkillReuseOutcome::Neutral,
            "final_answer",
            crate::memory::MemoryProfile::Standard,
        );

        assert!(!scheduled);
        let key = DetachedWorkKey::new(
            "qq_channel",
            "chat-1",
            DetachedJobKind::PostReplyMaintenance,
        );
        assert!(store.get(&key).expect("load").is_none());
    }

    #[test]
    fn post_reply_maintenance_is_not_blocked_by_execution_state_projection_alone() {
        let store = StubDetachedWorkStore::default();
        let active_work_store = StubActiveWorkStore::default();
        let (system_inbound_tx, _system_inbound_rx, _depth) = crate::bus::new_inbound_channel(4);
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");

        let scheduled = enqueue_post_reply_maintenance_job(
            &active_work_store,
            &store,
            &system_inbound_tx,
            &msg,
            "请先提供 SMTP 授权码。",
            0,
            false,
            PromptRecallIntent::default(),
            &[],
            &[],
            crate::skills::RuntimeSkillReuseOutcome::Neutral,
            "final_answer",
            crate::memory::MemoryProfile::Standard,
        );

        assert!(scheduled);
        let key = DetachedWorkKey::new(
            "qq_channel",
            "chat-1",
            DetachedJobKind::PostReplyMaintenance,
        );
        assert!(store.get(&key).expect("load").is_some());
    }

    #[test]
    fn primary_user_turn_is_not_dropped_after_defer_limit() {
        let (user_inbound_tx, user_inbound_rx, _user_depth) = crate::bus::new_inbound_channel(8);
        let (system_inbound_tx, _system_inbound_rx, _system_depth) =
            crate::bus::new_inbound_channel(8);
        let (outbound_tx, _outbound_rx, _outbound_depth) = crate::bus::new_inbound_channel(8);
        let platform: Arc<dyn crate::Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let config = AgentLoopConfig {
            runtime: crate::RuntimeServices::from_platform(platform),
            get_skill_descriptions: Arc::new(String::new),
            get_capability_package_text: Arc::new(|_, _| None),
            tg_group_activation: Arc::from(""),
            channel_capability_registry: Arc::new(crate::build_channel_capability_registry(
                &crate::AppConfig::load_from_env(),
                false,
            )),
            strategy: AgentRunStrategy::Embedded,
            stream_editor: None,
            stream_editor_channel: None,
            resolve_locale: Arc::new(|| UiLocale::Zh),
        };
        let mut defer_tracker = HashMap::new();
        let mut low_mem_defer_log = None;

        for _ in 0..MAX_DEFER_RETRIES {
            let msg =
                PcMsg::new_inbound("qq_channel", "chat-1", "primary", false).expect("message");
            let mut hasher = DefaultHasher::new();
            msg.channel.hash(&mut hasher);
            msg.chat_id.hash(&mut hasher);
            msg.content.hash(&mut hasher);
            let msg_key = hasher.finish();
            handle_admission_defer(
                0,
                msg,
                msg_key,
                AdmissionDeferContext {
                    loc: UiLocale::Zh,
                    user_inbound_tx: &user_inbound_tx,
                    system_inbound_tx: &system_inbound_tx,
                    outbound_tx: &outbound_tx,
                    config: &config,
                    defer_tracker: &mut defer_tracker,
                    low_mem_defer_log: &mut low_mem_defer_log,
                },
            );
        }

        assert!(
            !defer_tracker.is_empty(),
            "primary user turn must remain replayable after the defer limit"
        );
        assert!(
            user_inbound_rx.try_recv().is_ok(),
            "primary user turn should be replayed or parked, not dropped"
        );
    }

    #[test]
    fn embedded_post_reply_maintenance_uses_delayed_system_queue() {
        let (_state_guard, _delayed_guard) =
            crate::runtime::delayed_task::delayed_task_test_scope();
        let store = StubDetachedWorkStore::default();
        let active_work_store = StubActiveWorkStore::default();
        let (system_inbound_tx, _system_inbound_rx, _depth) = crate::bus::new_inbound_channel(4);
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");

        let scheduled = enqueue_post_reply_maintenance_job(
            &active_work_store,
            &store,
            &system_inbound_tx,
            &msg,
            "请先提供 SMTP 授权码。",
            0,
            false,
            PromptRecallIntent::default(),
            &[],
            &[],
            crate::skills::RuntimeSkillReuseOutcome::Neutral,
            "final_answer",
            crate::memory::MemoryProfile::Embedded,
        );

        assert!(scheduled);
        let key = DetachedWorkKey::new(
            "qq_channel",
            "chat-1",
            DetachedJobKind::PostReplyMaintenance,
        );
        assert!(
            store.get(&key).expect("load").is_none(),
            "embedded post-reply scheduling should not write detached-work SPIFFS state on agent_loop"
        );
    }

    #[test]
    fn embedded_background_jobs_do_not_adopt_into_detached_store() {
        let payload = serde_json::json!({
            "ingress": IngressKind::User,
            "source_channel": "qq_channel",
            "user_content": "查看系统状态",
            "reply_content": "正在检查",
            "tool_calls": 0,
            "external_content_used": false,
            "now_secs": 42
        })
        .to_string();
        let msg = PcMsg::new_system(CHANNEL_POST_REPLY_MAINTENANCE, "chat-1", payload)
            .expect("build maintenance message");

        assert!(!super::should_persist_background_job_as_detached(
            &msg,
            crate::memory::MemoryProfile::Embedded,
        ));
        assert!(super::should_persist_background_job_as_detached(
            &msg,
            crate::memory::MemoryProfile::Standard,
        ));
    }

    #[test]
    fn embedded_post_reply_jobs_wait_for_normal_pressure() {
        assert_eq!(
            embedded_post_reply_pressure_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                crate::orchestrator::PressureLevel::Cautious,
            ),
            BackgroundGatePolicy::Defer {
                reason: "post_reply_pressure_cautious",
                delay_ms: BACKGROUND_DEFER_DELAY_MS,
            }
        );
        assert_eq!(
            embedded_post_reply_pressure_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::SelfRuntimePostReply,
                crate::orchestrator::PressureLevel::Critical,
            ),
            BackgroundGatePolicy::Defer {
                reason: "post_reply_pressure_critical",
                delay_ms: BACKGROUND_DEFER_DELAY_MS,
            }
        );
        assert_eq!(
            embedded_post_reply_pressure_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::SelfRuntimePostReply,
                crate::orchestrator::PressureLevel::Normal,
            ),
            BackgroundGatePolicy::Run
        );
        assert_eq!(
            embedded_post_reply_pressure_policy(
                crate::memory::MemoryProfile::Standard,
                DetachedJobKind::SelfRuntimePostReply,
                crate::orchestrator::PressureLevel::Cautious,
            ),
            BackgroundGatePolicy::Run
        );
    }

    #[test]
    fn embedded_post_reply_bounded_deferral_eventually_allows_lightweight_run() {
        let state = crate::orchestrator::state::OrchestratorState::new();
        state.update_heap(
            64 * 1024,
            8 * 1024 * 1024,
            (crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32).saturating_sub(1024),
        );
        let resource = crate::orchestrator::ResourceSnapshot::from_state(&state);

        assert!(matches!(
            embedded_post_reply_gate_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                &resource,
                0,
                Some(1_000),
                5_000,
            ),
            BackgroundGatePolicy::Defer {
                reason: "post_reply_resource_window_busy",
                ..
            }
        ));

        assert_eq!(
            embedded_post_reply_gate_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                &resource,
                0,
                Some(1_000),
                1_000u64.saturating_add(crate::constants::POST_REPLY_BACKGROUND_MAX_DEFER_MS),
            ),
            BackgroundGatePolicy::Lightweight
        );
    }

    #[test]
    fn embedded_post_reply_admission_ignores_current_background_agent_task_slot() {
        let state = crate::orchestrator::state::OrchestratorState::new();
        state
            .active_agent_tasks
            .store(1, std::sync::atomic::Ordering::Relaxed);
        let mut resource = crate::orchestrator::ResourceSnapshot::from_state(&state);
        resource.active_http_count = 0;
        resource.active_wss_count = 0;
        resource.inbound_depth = 0;
        resource.outbound_depth = 0;

        assert_eq!(
            embedded_post_reply_gate_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                &resource,
                1,
                None,
                5_000,
            ),
            BackgroundGatePolicy::Run
        );
    }

    #[test]
    fn embedded_post_reply_admission_treats_established_wss_as_steady_state() {
        let state = crate::orchestrator::state::OrchestratorState::new();
        let mut resource = crate::orchestrator::ResourceSnapshot::from_state(&state);
        resource.active_http_count = 0;
        resource.active_wss_count = 1;
        resource.active_agent_tasks = 0;
        resource.inbound_depth = 0;
        resource.outbound_depth = 0;

        assert_eq!(
            embedded_post_reply_gate_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                &resource,
                0,
                None,
                5_000,
            ),
            BackgroundGatePolicy::Run
        );
    }

    #[test]
    fn embedded_post_reply_admission_defers_when_another_agent_task_is_active() {
        let state = crate::orchestrator::state::OrchestratorState::new();
        state
            .active_agent_tasks
            .store(2, std::sync::atomic::Ordering::Relaxed);
        let mut resource = crate::orchestrator::ResourceSnapshot::from_state(&state);
        resource.active_http_count = 0;
        resource.active_wss_count = 0;
        resource.inbound_depth = 0;
        resource.outbound_depth = 0;

        assert!(matches!(
            embedded_post_reply_gate_policy(
                crate::memory::MemoryProfile::Embedded,
                DetachedJobKind::PostReplyMaintenance,
                &resource,
                1,
                None,
                5_000,
            ),
            BackgroundGatePolicy::Defer {
                reason: "post_reply_resource_window_busy",
                ..
            }
        ));
    }
}
