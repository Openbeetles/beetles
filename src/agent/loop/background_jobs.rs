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
    let system_inbound_tx = system_inbound_tx.clone();
    let chat_id = msg.chat_id.to_string();
    let source_channel = msg.channel.to_string();
    let scheduled = crate::runtime::schedule_delayed_task(
        Instant::now() + Duration::from_millis(POST_REPLY_MAINTENANCE_DELAY_MS),
        Box::new(move || {
            if let Some(reason) = super::background_enqueue_block_reason() {
                log::debug!(
                    "[agent_memory] skip delayed maintenance enqueue because {} chat_id={}",
                    reason,
                    chat_id
                );
                append_post_reply_workflow_audit(
                    crate::runtime::WorkflowDisposition::Suppress,
                    reason,
                    crate::runtime::WorkflowEffect::Noop,
                    source_channel.as_str(),
                    chat_id.as_str(),
                );
                return;
            }
            let job = match PcMsg::new_system(CHANNEL_POST_REPLY_MAINTENANCE, &chat_id, body) {
                Ok(job) => job,
                Err(error) => {
                    log::warn!(
                        "[agent_memory] maintenance job build failed chat_id={}: {}",
                        chat_id,
                        error
                    );
                    append_post_reply_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "post_reply_maintenance_build_failed",
                        crate::runtime::WorkflowEffect::Noop,
                        source_channel.as_str(),
                        chat_id.as_str(),
                    );
                    return;
                }
            };
            match system_inbound_tx.try_send(job) {
                Ok(()) => {
                    append_post_reply_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteNow,
                        "post_reply_maintenance_enqueued",
                        crate::runtime::WorkflowEffect::EnqueueSystemJob,
                        source_channel.as_str(),
                        chat_id.as_str(),
                    );
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::debug!(
                        "[agent_memory] skip maintenance enqueue because system queue is full chat_id={}",
                        chat_id
                    );
                    append_post_reply_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "post_reply_maintenance_queue_full",
                        crate::runtime::WorkflowEffect::Noop,
                        source_channel.as_str(),
                        chat_id.as_str(),
                    );
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!(
                        "[agent_memory] maintenance enqueue failed: system queue disconnected"
                    );
                    append_post_reply_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "post_reply_maintenance_queue_disconnected",
                        crate::runtime::WorkflowEffect::Noop,
                        source_channel.as_str(),
                        chat_id.as_str(),
                    );
                }
            }
        }),
    );
    if !scheduled {
        log::debug!(
            "[agent_memory] delayed queue full, skip maintenance schedule chat_id={}",
            msg.chat_id
        );
        append_post_reply_workflow_audit(
            crate::runtime::WorkflowDisposition::ExecuteFailed,
            "post_reply_maintenance_schedule_failed",
            crate::runtime::WorkflowEffect::Noop,
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
        );
    } else {
        append_post_reply_workflow_audit(
            crate::runtime::WorkflowDisposition::DeferUntil,
            "post_reply_maintenance_scheduled",
            crate::runtime::WorkflowEffect::EnqueueSystemJob,
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
        );
    }
    scheduled
}

pub(super) fn maybe_yield_background_job_to_pending_user(
    background_msg: PcMsg,
    user_inbound_rx: &UserInboundRx,
    system_inbound_tx: &SystemInboundTx,
) -> PcMsg {
    match user_inbound_rx.try_recv() {
        Ok(user_msg) => {
            let mut background_msg = background_msg;
            background_msg.enqueue_ts_ms = super::now_unix_ms();
            match system_inbound_tx.try_send(background_msg) {
                Ok(()) => {
                    log::debug!(
                        "[agent] yielded background job to pending user chat_id={}",
                        user_msg.chat_id
                    );
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::warn!("[agent] background yield requeue dropped: system queue full");
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!(
                        "[agent] background yield requeue failed: system queue disconnected"
                    );
                }
            }
            user_msg
        }
        Err(std::sync::mpsc::TryRecvError::Empty)
        | Err(std::sync::mpsc::TryRecvError::Disconnected) => background_msg,
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

fn run_long_term_memory_refresh_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
) {
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let outcome = run_long_term_memory_refresh(
        &mut llm_ctx,
        worker_llm,
        LongTermMemoryRefreshContext {
            memory_store: config.memory_store.as_ref(),
            session_store: config.session_store.as_ref(),
            session_summary_store: config.session_summary_store.as_ref(),
            long_term_memory_store: config.long_term_memory_store.as_ref(),
            extraction_state_store: config.long_term_memory_extraction_state_store.as_ref(),
            turn_ledger_store: config.turn_ledger_store.as_ref(),
            skill_storage: config.skill_storage.as_ref(),
        },
        &msg.chat_id,
        crate::orchestrator::snapshot().pressure,
        config.memory_system_kind.memory_profile(),
    );
    outcome.persist(
        config.long_term_memory_extraction_state_store.as_ref(),
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
        }
        LongTermMemoryRefreshOutcome::Failed { error, .. } => {
            log::warn!("[agent_memory] refresh failed: {}", error);
        }
        LongTermMemoryRefreshOutcome::Deferred { .. } => {}
    }
}

fn run_idle_memory_forge_job(config: &AgentLoopConfig, msg: &PcMsg) {
    match crate::reasoning::run_idle_memory_forge_background_job(
        config.long_term_memory_store.as_ref(),
        config.continuity_capsule_store.as_ref(),
        config.platform.state_fs().as_ref(),
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
        }
        Err(error) => log::warn!("[idle_memory_forge] failed for {}: {}", msg.chat_id, error),
    }
}

fn run_post_reply_maintenance_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) {
    let payload: PostReplyMaintenanceJobPayload = match serde_json::from_str(&msg.content) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job decode failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return;
        }
    };
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let maintenance_outcome = run_post_reply_memory_maintenance(
        &mut llm_ctx,
        worker_llm,
        PostReplyMemoryMaintenanceContext {
            session_store: config.session_store.as_ref(),
            memory_store: config.memory_store.as_ref(),
            session_summary_store: config.session_summary_store.as_ref(),
            execution_state_store: config.execution_state_store.as_ref(),
            long_term_memory_store: config.long_term_memory_store.as_ref(),
            continuity_capsule_store: config.continuity_capsule_store.as_ref(),
            extraction_state_store: config.long_term_memory_extraction_state_store.as_ref(),
            turn_ledger_store: config.turn_ledger_store.as_ref(),
            skill_storage: config.skill_storage.as_ref(),
            task_run_store: config.task_run_store.as_ref(),
            task_artifact_store: config.task_artifact_store.as_ref(),
            task_learning_store: config.task_learning_store.as_ref(),
        },
        PostReplyMemoryMaintenanceInput {
            chat_id: &msg.chat_id,
            ingress: payload.ingress,
            channel: &payload.source_channel,
            user_content: &payload.user_content,
            reply_content: &payload.reply_content,
            pressure: crate::orchestrator::snapshot().pressure,
            memory_profile: config.memory_system_kind.memory_profile(),
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
            Ok(job) => match system_inbound_tx.try_send(job) {
                Ok(()) => true,
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::debug!(
                        "[agent_memory] skip refresh enqueue because system queue is full chat_id={}",
                        msg.chat_id
                    );
                    false
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!("[agent_memory] refresh enqueue failed: system queue disconnected");
                    false
                }
            },
            Err(error) => {
                log::warn!("[agent_memory] refresh job build failed: {}", error);
                false
            }
        },
    );
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
}

fn run_self_runtime_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) {
    let payload: crate::memory::SelfRuntimeJobPayload = match serde_json::from_str(&msg.content) {
        Ok(payload) => payload,
        Err(error) => {
            log::warn!(
                "[self_runtime] decode failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return;
        }
    };
    let locale = (config.resolve_locale)();
    let mut llm_ctx = build_system_llm_ctx(http, config, &msg.chat_id, locale);
    let outcome = run_self_runtime(
        &mut llm_ctx,
        worker_llm,
        SelfRuntimeContext {
            memory_system_kind: config.memory_system_kind,
            session_store: config.session_store.as_ref(),
            memory_store: config.memory_store.as_ref(),
            session_summary_store: config.session_summary_store.as_ref(),
            execution_state_store: config.execution_state_store.as_ref(),
            long_term_memory_store: config.long_term_memory_store.as_ref(),
            continuity_capsule_store: config.continuity_capsule_store.as_ref(),
            self_model_store: config.self_model_store.as_ref(),
            self_authored_core_store: config.self_authored_core_store.as_ref(),
            core_revision_ledger_store: config.core_revision_ledger_store.as_ref(),
            relationship_constitution_store: config.relationship_constitution_store.as_ref(),
            relationship_portfolio_store: config.relationship_portfolio_store.as_ref(),
            relationship_topology_store: config.relationship_topology_store.as_ref(),
            world_sense_store: config.world_sense_store.as_ref(),
            autonomy_strategy_store: config.autonomy_strategy_store.as_ref(),
            outer_voice_store: config.outer_voice_store.as_ref(),
            private_doc_store: config.private_doc_store.as_ref(),
            private_garden_store: config.private_garden_store.as_ref(),
            inner_life_store: config.inner_life_store.as_ref(),
            self_continuity_store: config.self_continuity_store.as_ref(),
            mental_privacy_store: config.mental_privacy_store.as_ref(),
            remind_store: config.remind_store.as_ref(),
            task_store: config.task_store.as_ref(),
            task_run_store: config.task_run_store.as_ref(),
            task_artifact_store: config.task_artifact_store.as_ref(),
            task_learning_store: config.task_learning_store.as_ref(),
            turn_ledger_store: config.turn_ledger_store.as_ref(),
            skill_storage: config.skill_storage.as_ref(),
        },
        &msg.chat_id,
        &payload,
    );
    let crate::memory::SelfRuntimeOutcome {
        decision,
        world_sense_result,
        autonomy_strategy_result,
        inner_life_result,
        private_doc_result,
        self_model_result,
        self_authored_core_result,
        self_continuity_result,
        task_learning_result,
        private_garden_result,
        boundary_persona_result,
        outer_voice_result,
    } = *outcome;
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
                Ok(job) => match system_inbound_tx.try_send(job) {
                    Ok(()) => {}
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        log::debug!(
                            "[self_runtime] skip factual refresh enqueue because system queue is full chat_id={}",
                            msg.chat_id
                        );
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        log::warn!(
                            "[self_runtime] factual refresh enqueue failed: system queue disconnected"
                        );
                    }
                },
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
    let self_continuity = config.self_continuity_store.get(subject_id).ok().flatten();
    let relationship_portfolio = config
        .relationship_portfolio_store
        .get(subject_id)
        .ok()
        .flatten();
    let relationship_topology = config
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
            config.session_store.as_ref(),
            config.self_continuity_store.as_ref(),
            config.relationship_portfolio_store.as_ref(),
            config.relationship_topology_store.as_ref(),
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
                long_term_memory_store: config.long_term_memory_store.as_ref(),
                session_summary_store: config.session_summary_store.as_ref(),
                execution_state_store: config.execution_state_store.as_ref(),
                self_model_store: config.self_model_store.as_ref(),
                self_authored_core_store: config.self_authored_core_store.as_ref(),
                core_revision_ledger_store: config.core_revision_ledger_store.as_ref(),
                self_continuity_store: config.self_continuity_store.as_ref(),
                relationship_constitution_store: config.relationship_constitution_store.as_ref(),
                relationship_portfolio_store: config.relationship_portfolio_store.as_ref(),
                relationship_topology_store: config.relationship_topology_store.as_ref(),
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
) {
    let request: crate::runtime::OperatorMaintenanceRequest =
        match serde_json::from_str(&msg.content) {
            Ok(request) => request,
            Err(error) => {
                log::warn!(
                    "[operator_maintenance] decode failed chat_id={}: {}",
                    msg.chat_id,
                    error
                );
                return;
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
                return;
            };
            if crate::memory::enqueue_self_runtime_operator_request(
                system_inbound_tx,
                chat_id.as_str(),
                source_channel.as_str(),
            ) {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteNow,
                    "operator_repair_dispatched",
                    crate::runtime::WorkflowEffect::RunRepairPass,
                );
            } else {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteFailed,
                    "operator_repair_enqueue_failed",
                    crate::runtime::WorkflowEffect::Noop,
                );
            }
        }
        crate::runtime::OperatorMaintenanceAction::RebuildContinuitySnapshot => {
            match rebuild_operator_continuity_snapshots(config, &request, now_secs) {
                Ok(0) => append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::NoTrigger,
                    "operator_snapshot_target_unavailable",
                    crate::runtime::WorkflowEffect::Noop,
                ),
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
                }
                Err(error) => {
                    log::warn!("[operator_maintenance] snapshot rebuild failed: {}", error);
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "operator_snapshot_rebuild_failed",
                        crate::runtime::WorkflowEffect::Noop,
                    );
                }
            }
        }
        crate::runtime::OperatorMaintenanceAction::ReplayRecovery => {
            let report = crate::runtime::ensure_platform_soul_kernel_recovery(
                config.platform.as_ref(),
                now_secs,
            );
            if report.restore_attempted {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::ExecuteNow,
                    "operator_recovery_replayed",
                    crate::runtime::WorkflowEffect::ReplayRecovery,
                );
            } else {
                append_operator_maintenance_workflow_audit(
                    &request,
                    crate::runtime::WorkflowDisposition::NoTrigger,
                    "operator_recovery_already_steady",
                    crate::runtime::WorkflowEffect::Noop,
                );
            }
        }
        crate::runtime::OperatorMaintenanceAction::RefreshOperatorDigest => {
            match crate::platform::memory_operator_surface::build_memory_operator_surface(
                config.platform.as_ref(),
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
                }
                Err(error) => {
                    log::warn!("[operator_maintenance] digest refresh failed: {}", error);
                    append_operator_maintenance_workflow_audit(
                        &request,
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "operator_digest_refresh_failed",
                        crate::runtime::WorkflowEffect::Noop,
                    );
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
) -> bool {
    if super::is_long_term_memory_refresh_job(msg) {
        run_long_term_memory_refresh_job(http, worker_llm, config, msg);
        return true;
    }
    if super::is_post_reply_maintenance_job(msg) {
        run_post_reply_maintenance_job(http, worker_llm, config, system_inbound_tx, msg);
        return true;
    }
    if super::is_idle_memory_forge_job(msg) {
        run_idle_memory_forge_job(config, msg);
        return true;
    }
    if super::is_self_runtime_job(msg) {
        run_self_runtime_job(http, worker_llm, config, system_inbound_tx, msg);
        return true;
    }
    if super::is_operator_maintenance_job(msg) {
        run_operator_maintenance_job(config, system_inbound_tx, msg);
        return true;
    }
    false
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
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_periodic_maintenance {
        super::requeue_background_job_with_delay(msg, system_inbound_tx, 500);
        return;
    }
    if let Some((reason, delay_ms)) = super::should_defer_background_job(&msg) {
        log::debug!(
            "[agent] defer background job channel={} chat_id={} because {}",
            msg.channel,
            msg.chat_id,
            reason
        );
        super::requeue_background_job_with_delay(msg, system_inbound_tx, delay_ms);
        return;
    }

    let msg = match super::handle_llm_gate(
        msg,
        loc,
        user_inbound_tx,
        system_inbound_tx,
        outbound_tx,
        config,
    ) {
        GateResult::Proceed(msg) => msg,
        GateResult::Skipped => return,
    };

    let _agent_task_guard = crate::orchestrator::begin_agent_task();
    let _maintenance_scope = crate::runtime::BackgroundMaintenanceGuard::enter();
    let _ = try_run_lane_background_job(http, worker_llm, config, system_inbound_tx, &msg);
    metrics::record_system_message_done(false);
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

    if defer_count >= MAX_DEFER_RETRIES {
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

    if msg.ingress == IngressKind::User {
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
            let _ = ctx.config.pending_retry.save_pending_retry(&m);
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
    metrics::record_wdt_feed();
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
