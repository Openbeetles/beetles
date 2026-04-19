use super::*;
use crate::agent::final_reply::{
    build_canonical_reply, finalize_user_visible_reply, reply_has_concrete_anchor,
    reply_looks_like_future_action_narration, CanonicalReply,
};
use crate::memory::EmotionSignalStore;

pub(super) struct FinalizedTurn {
    pub(super) delivery: DeliveryReport,
    pub(super) reply: CanonicalReply,
    pub(super) is_interrupt: bool,
    pub(super) reply_already_delivered: bool,
    pub(super) skip_delivery: bool,
    pub(super) mark_important: bool,
    pub(super) streamed: bool,
    pub(super) msg_start: Instant,
    pub(super) turn_observation: Option<TurnObservationLedger>,
    pub(super) mental_privacy_review: MentalPrivacyReviewOutcome,
    pub(super) review_input_before: String,
    pub(super) worker_latency: WorkerLatency,
    pub(super) any_tool_used: bool,
    pub(super) external_content_used: bool,
    pub(super) used_surface_finalization: bool,
    pub(super) pressure: crate::orchestrator::PressureLevel,
    pub(super) reply_surface: ReplySurface,
    pub(super) foreground_work_packet: Option<crate::agent::ForegroundWorkPacket>,
    pub(super) prompt_recall_intent: crate::memory::PromptRecallIntent,
    pub(super) runtime_skill_selected_ids: Vec<String>,
    pub(super) task_learning_selected_ids: Vec<String>,
    pub(super) programmable_reasoning_intent:
        Option<crate::agent::reasoning_intent::ProgrammableReasoningIntent>,
    pub(super) counterfactual_analysis:
        Option<crate::agent::counterfactual::CounterfactualAnalysis>,
    pub(super) adversarial_arena_adjudication:
        Option<crate::reasoning::AdversarialArenaAdjudication>,
    pub(super) subject_state: Option<SubjectState>,
    pub(super) soul_feedback_projection: Option<SoulFeedbackProjection>,
    pub(super) mental_privacy_adjudication:
        Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    pub(super) persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
}

fn should_run_full_mental_privacy_review(
    reply_surface: ReplySurface,
    disclosure: Option<&crate::memory::MentalPrivacyDisclosureAdjudication>,
) -> bool {
    match reply_surface.governance_policy() {
        crate::agent::reply_surface::SurfaceGovernancePolicy::SkipMentalPrivacyReview
        | crate::agent::reply_surface::SurfaceGovernancePolicy::SuppressUserDelivery => false,
        crate::agent::reply_surface::SurfaceGovernancePolicy::PrivateBoundaryReview
        | crate::agent::reply_surface::SurfaceGovernancePolicy::TaskExecutionReview => true,
        crate::agent::reply_surface::SurfaceGovernancePolicy::ApplyMentalPrivacyReview => {
            disclosure.is_some()
        }
    }
}

fn truthful_no_new_execution_result_copy(loc: UiLocale) -> &'static str {
    match loc {
        UiLocale::Zh => "这轮还没有实际执行新的工具或任务步骤，也还没有产生新结果。",
        UiLocale::En => "This turn has not executed a new tool or task step yet, so there is no new result to report.",
    }
}

pub(super) fn looks_like_truthful_blocker_or_input_request(content: &str) -> bool {
    let trimmed = content.trim();
    let lower = trimmed.to_ascii_lowercase();
    trimmed.contains('?')
        || trimmed.contains('？')
        || trimmed.contains("缺")
        || trimmed.contains("请先提供")
        || trimmed.contains("无法继续")
        || trimmed.contains("不能继续")
        || trimmed.contains("才能继续")
        || trimmed.contains("请提供")
        || trimmed.contains("请把")
        || trimmed.contains("请发")
        || lower.contains("missing ")
        || lower.contains("cannot continue")
        || lower.contains("can't continue")
        || lower.contains("please provide")
        || lower.contains("please send")
}

fn should_apply_truth_guard(
    strategy: AgentRunStrategy,
    delivery: &DeliveryReport,
    any_tool_used: bool,
    external_content_used: bool,
    reply_content: &str,
) -> bool {
    if strategy != AgentRunStrategy::LinuxEnhanced
        || any_tool_used
        || external_content_used
        || delivery.planner_progress_updates_sent > 0
        || delivery.tool_progress_updates_sent > 0
        || delivery.terminal_progress_updates_sent > 0
    {
        return false;
    }
    let trimmed = reply_content.trim();
    !trimmed.is_empty()
        && !reply_has_concrete_anchor(trimmed)
        && !looks_like_truthful_blocker_or_input_request(trimmed)
        && reply_looks_like_future_action_narration(trimmed)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finalize_turn(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    msg: &PcMsg,
    loc: UiLocale,
    msg_start: Instant,
    outcome: WorkerOutcome,
    telemetry: WorkerRunTelemetry,
) -> Result<FinalizedTurn> {
    let final_outcome = if telemetry.delivery.current_primary_delivered {
        "current_primary"
    } else if telemetry.used_surface_finalization {
        "surface_finalization"
    } else {
        "final_answer"
    };
    let turn_observation = build_turn_observation_ledger(final_outcome, false, &telemetry);
    let WorkerRunTelemetry {
        streamed,
        latency: mut worker_latency,
        delivery,
        any_tool_used,
        external_content_used,
        used_surface_finalization,
        task_execution_used: _task_execution_used,
        foreground_work_context_present: _foreground_work_context_present,
        pressure,
        runtime_mode: _runtime_mode,
        deliberation_class: _deliberation_class,
        reply_surface,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        programmable_reasoning_intent,
        counterfactual_analysis,
        adversarial_arena_adjudication,
        subject_state,
        soul_feedback_projection,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    } = telemetry;

    // Canonical final reply is produced exactly once here.
    // Delivery only transports the already-finalized reply afterwards.
    let (mut reply_content, is_interrupt, reply_already_delivered, apply_finalizer) = match outcome
    {
        WorkerOutcome::Content(s) => {
            let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
            let s = if let Cow::Borrowed(_) = &cow {
                s
            } else {
                cow.into_owned()
            };
            (s, false, false, true)
        }
    };
    let foreground_work_packet = None;
    if !is_interrupt {
        let (stripped_reply_content, _) =
            crate::agent::extract_foreground_work_packet(&reply_content, false)?;
        reply_content = stripped_reply_content;
    }

    if !is_interrupt && apply_finalizer {
        reply_content = finalize_user_visible_reply(config.strategy, &reply_content);
    }
    if !is_interrupt
        && should_apply_truth_guard(
            config.strategy,
            &delivery,
            any_tool_used,
            external_content_used,
            &reply_content,
        )
    {
        log::warn!(
            "[reply_surface] truth_guard replaced unsupported future-action narration surface={} channel={} chat_id={}",
            reply_surface.as_str(),
            msg.channel,
            msg.chat_id
        );
        reply_content = truthful_no_new_execution_result_copy(loc).to_string();
    }
    let review_input_before = reply_content.clone();
    let mut mental_privacy_review = MentalPrivacyReviewOutcome {
        reply_content: reply_content.clone(),
        action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
        applied: false,
        touched_targets: Vec::new(),
    };
    if !is_interrupt
        && should_run_full_mental_privacy_review(
            reply_surface,
            mental_privacy_adjudication.as_ref(),
        )
    {
        mental_privacy_review = super::worker_governance::maybe_apply_mental_privacy_review(
            http,
            worker_llm,
            config,
            msg,
            loc,
            reply_surface,
            reply_content,
            &mut worker_latency,
        );
        reply_content = mental_privacy_review.reply_content.clone();
    }
    let mark_important = !is_interrupt && reply_content.contains(AGENT_MARKER_MARK_IMPORTANT);
    let signal_comfort = !is_interrupt && reply_content.contains(AGENT_MARKER_SIGNAL_COMFORT);
    if mark_important || signal_comfort {
        reply_content = remove_substrings_all_trim(
            &reply_content,
            &[AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT],
        );
        if signal_comfort {
            let _ = config
                .runtime
                .emotion_signal_store
                .set(&msg.chat_id, "comfort");
        }
        reply_content = truncate_content_to_max(&reply_content, MAX_CONTENT_LEN).into_owned();
    }
    if !is_interrupt && !reply_content.is_empty() {
        metrics::record_final_answer_call();
    }
    let reply = if is_interrupt {
        CanonicalReply::new(reply_content.clone())
    } else {
        match build_canonical_reply(config.strategy, &reply_content) {
            Ok(reply) => reply,
            Err(kind) => {
                metrics::record_empty_final_blocked();
                log::warn!(
                    "[reply_surface] canonical reply contract breached stage={} surface={} finalization_policy={:?} governance_policy={:?} channel={} chat_id={}",
                    kind.stage(),
                    reply_surface.as_str(),
                    reply_surface.finalization_policy(),
                    reply_surface.governance_policy(),
                    msg.channel,
                    msg.chat_id
                );
                return Err(crate::error::Error::config(
                    kind.stage(),
                    format!(
                        "reply_surface={} finalization_policy={:?} governance_policy={:?} channel={} chat_id={}",
                        reply_surface.as_str(),
                        reply_surface.finalization_policy(),
                        reply_surface.governance_policy(),
                        msg.channel,
                        msg.chat_id
                    ),
                ));
            }
        }
    };

    Ok(FinalizedTurn {
        delivery,
        skip_delivery: reply.as_str().trim() == "SILENT"
            || (msg.channel.as_ref() == CHANNEL_CRON && reply.as_str().is_empty()),
        reply,
        is_interrupt,
        reply_already_delivered,
        mark_important,
        streamed,
        msg_start,
        turn_observation,
        mental_privacy_review,
        review_input_before,
        worker_latency: std::mem::take(&mut worker_latency),
        any_tool_used,
        external_content_used,
        used_surface_finalization,
        pressure,
        reply_surface,
        foreground_work_packet,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        programmable_reasoning_intent,
        counterfactual_analysis,
        adversarial_arena_adjudication,
        subject_state,
        soul_feedback_projection,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    })
}

pub(super) fn complete_turn(
    ctx: LaneTurnFinalizeContext<'_>,
    llm_failure_count: &mut HashMap<u64, (u8, Instant)>,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    finalized: FinalizedTurn,
    handoff: super::delivery_handoff::DeliveryHandoff,
) {
    let LaneTurnFinalizeContext {
        worker_lane_tag,
        config,
        system_inbound_tx,
        outbound_tx: _outbound_tx,
        msg,
        loc: _loc,
        msg_start,
        queue_wait_ms,
        admission_ms,
        worker_prepare_ms,
        msg_key,
        mut turn_ledger,
        latency_warn_ms,
    } = ctx;
    let FinalizedTurn {
        reply,
        is_interrupt,
        reply_already_delivered,
        skip_delivery,
        mark_important,
        streamed,
        mut delivery,
        turn_observation,
        mental_privacy_review,
        review_input_before,
        mut worker_latency,
        any_tool_used,
        external_content_used,
        used_surface_finalization,
        pressure,
        reply_surface,
        foreground_work_packet: _foreground_work_packet,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        programmable_reasoning_intent,
        counterfactual_analysis,
        adversarial_arena_adjudication,
        subject_state,
        mut soul_feedback_projection,
        mental_privacy_adjudication,
        persona_priority_adjudication,
        msg_start: _,
    } = finalized;
    let reply_content = reply.visible_text;

    if skip_delivery {
        llm_failure_count.remove(&msg_key);
        defer_tracker.remove(&msg_key);
        let total_ms = msg_start.elapsed().as_millis();
        metrics::record_e2e_ms(total_ms);
        if msg.ingress == IngressKind::System {
            let is_cron = msg.channel.as_ref() == CHANNEL_CRON;
            metrics::record_system_message_done(is_cron);
            if is_cron {
                let cron_e2e = super::now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
                metrics::record_cron_e2e_ms(cron_e2e);
            }
        } else {
            metrics::record_user_message_done();
        }
        return;
    }

    let delivered = handoff.delivered;
    let outbound_enqueue_ms = handoff.outbound_enqueue_ms;
    let reply_handoff_ms = handoff.reply_handoff_ms;
    delivery.current_primary_delivered |= reply_already_delivered;
    delivery.finalize_streamed |= streamed && delivered;

    let session_start = Instant::now();
    let session_write_result = if delivered {
        let entries = [
            SessionMessage {
                role: "user".to_string(),
                content: msg.content.clone(),
            },
            SessionMessage {
                role: "assistant".to_string(),
                content: reply_content.clone(),
            },
        ];
        config
            .runtime
            .session_store
            .append_batch(&msg.chat_id, &entries)
    } else {
        config
            .runtime
            .session_store
            .append(&msg.chat_id, "user", &msg.content)
    };
    if let Err(e) = session_write_result {
        log::warn!("[agent_session] append failed: {}", e);
        metrics::record_error_by_stage("session_append");
    }
    worker_latency.session_write_ms = worker_latency
        .session_write_ms
        .saturating_add(session_start.elapsed().as_millis());
    llm_failure_count.remove(&msg_key);
    defer_tracker.remove(&msg_key);

    if delivered && mark_important {
        let _ = config
            .runtime
            .important_message_store
            .set_important_offset_from_end(&msg.chat_id, 1);
    }
    let now_secs = super::now_unix_ms() / 1000;
    let reply_requests_input = looks_like_truthful_blocker_or_input_request(&reply_content);
    let clear_execution_state = delivered
        && msg.ingress == IngressKind::User
        && reply_surface != ReplySurface::TaskExecution;
    if clear_execution_state {
        if let Err(error) = config.runtime.execution_state_store.clear(&msg.chat_id) {
            log::warn!(
                "[agent_execution_state] clear failed chat_id={}: {}",
                msg.chat_id,
                error
            );
        }
    }
    let should_seed_execution_state = delivered
        && msg.ingress == IngressKind::User
        && reply_surface != ReplySurface::PrivateBoundary
        && (reply_requests_input
            || (any_tool_used && !used_surface_finalization)
            || matches!(reply_surface, ReplySurface::TaskExecution)
            || turn_observation
                .as_ref()
                .and_then(|observation| observation.blocker.as_ref())
                .is_some());
    if should_seed_execution_state {
        if let Err(error) = crate::memory::seed_execution_state_from_turn(
            config.runtime.execution_state_store.as_ref(),
            crate::memory::ProvisionalExecutionStateInput {
                chat_id: &msg.chat_id,
                ingress: msg.ingress,
                channel: msg.channel.as_ref(),
                user_content: &msg.content,
                reply_content: &reply_content,
                reply_requests_input,
                tool_calls: worker_latency.tool_calls,
                now_secs,
                turn_observation: turn_observation.as_ref(),
            },
        ) {
            log::warn!(
                "[agent_execution_state] provisional seed failed chat_id={}: {}",
                msg.chat_id,
                error
            );
        }
    }
    if delivered && msg.ingress == IngressKind::User {
        let active_task_run = active_task_run_for_chat(
            config.runtime.task_run_store.as_ref(),
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
        )
        .ok()
        .flatten();
        let execution_state = if active_task_run.is_none() {
            match config.runtime.execution_state_store.get(&msg.chat_id) {
                Ok(state) => state,
                Err(error) => {
                    log::warn!(
                        "[agent_execution_state] read failed chat_id={}: {}",
                        msg.chat_id,
                        error
                    );
                    None
                }
            }
        } else {
            None
        };
        if let Err(error) = crate::agent::sync_active_work_after_turn(
            config.runtime.active_work_store.as_ref(),
            crate::agent::ActiveWorkSyncInput {
                chat_id: &msg.chat_id,
                active_task_run: active_task_run.as_ref(),
                execution_state: execution_state.as_ref(),
                user_request: &msg.content,
                now_secs,
            },
        ) {
            log::warn!(
                "[agent_active_work] sync failed chat_id={}: {}",
                msg.chat_id,
                error
            );
        }
    }
    let llm_ms = worker_latency
        .context_ms
        .saturating_add(worker_latency.llm_round_total_ms)
        .saturating_add(worker_latency.tool_exec_ms)
        .saturating_add(worker_latency.session_write_ms);

    let reuse_outcome = if is_interrupt
        || (runtime_skill_selected_ids.is_empty() && task_learning_selected_ids.is_empty())
    {
        crate::skills::RuntimeSkillReuseOutcome::Neutral
    } else {
        crate::skills::RuntimeSkillReuseOutcome::Succeeded
    };
    let reuse_outcome_note = if used_surface_finalization {
        "surface_finalization"
    } else if reply_already_delivered || delivery.current_primary_delivered {
        "current_primary"
    } else {
        "final_answer"
    };

    if delivered
        && !super::background_jobs::enqueue_post_reply_maintenance_job(
            config.runtime.active_work_store.as_ref(),
            config.runtime.detached_work_store.as_ref(),
            system_inbound_tx,
            &msg,
            &reply_content,
            worker_latency.tool_calls,
            external_content_used,
            prompt_recall_intent,
            &runtime_skill_selected_ids,
            &task_learning_selected_ids,
            reuse_outcome,
            reuse_outcome_note,
        )
    {
        log::debug!(
            "[agent_memory] post-reply maintenance job skipped chat_id={}",
            msg.chat_id
        );
    }
    let self_runtime_post_reply_enqueued = delivered
        && crate::memory::enqueue_self_runtime_post_reply(
            system_inbound_tx,
            config.runtime.detached_work_store.as_ref(),
            config.runtime.active_work_store.as_ref(),
            config.runtime.self_continuity_store.as_ref(),
            config.runtime.autonomy_strategy_store.as_ref(),
            config.runtime.self_authored_core_store.as_ref(),
            config.runtime.memory_system_kind.memory_profile(),
            msg.chat_id.as_ref(),
            msg.channel.as_ref(),
            &msg.content,
            &reply_content,
            worker_latency.tool_calls,
            external_content_used,
        );
    if delivered && !self_runtime_post_reply_enqueued {
        log::debug!(
            "[self_runtime] post-reply job skipped chat_id={}",
            msg.chat_id
        );
    }
    if let Some(projection) = soul_feedback_projection.as_mut() {
        projection.strategy.post_reply_self_runtime_enqueued = self_runtime_post_reply_enqueued;
        if self_runtime_post_reply_enqueued {
            projection.strategy.applied = true;
            if !projection
                .strategy
                .signal_layers
                .iter()
                .any(|layer| layer == "self_runtime_scheduler")
            {
                projection
                    .strategy
                    .signal_layers
                    .push("self_runtime_scheduler".to_string());
            }
        }
    }

    let total_ms = msg_start.elapsed().as_millis();
    let post_reply_ms = total_ms.saturating_sub(reply_handoff_ms);
    let canonical_reply_source = if is_interrupt {
        "interrupt"
    } else if reply_already_delivered || delivery.current_primary_delivered {
        "current_primary"
    } else if used_surface_finalization {
        "surface_finalization"
    } else {
        "final_answer"
    };
    let outbound_source = if reply_already_delivered || delivery.current_primary_delivered {
        "current_primary"
    } else if streamed && delivered {
        "stream_edit"
    } else if delivered {
        "reply"
    } else {
        ""
    };
    turn_ledger.status = if is_interrupt {
        TurnLedgerStatus::Interrupted
    } else {
        TurnLedgerStatus::Answered
    };
    turn_ledger.reason = normalize_turn_reason(canonical_reply_source);
    turn_ledger.outbound_source = normalize_turn_reason(outbound_source);
    turn_ledger.canonical_reply_source = normalize_turn_reason(canonical_reply_source);
    turn_ledger.reply_preview = normalize_turn_preview(&reply_content);
    turn_ledger.updated_at_ms = super::now_unix_ms();
    turn_ledger.finished_at_ms = turn_ledger.updated_at_ms;
    turn_ledger.react_rounds = worker_latency.react_rounds;
    turn_ledger.tool_calls = worker_latency.tool_calls;
    turn_ledger.any_tool_used = any_tool_used;
    turn_ledger.final_reply_delivered = delivered;
    turn_ledger.reply_handoff_ms = reply_handoff_ms.min(u64::MAX as u128) as u64;
    turn_ledger.post_reply_ms = post_reply_ms.min(u64::MAX as u128) as u64;
    turn_ledger.total_ms = total_ms.min(u64::MAX as u128) as u64;
    turn_ledger.ttft_ms = worker_latency.ttft_ms.unwrap_or(0).min(u64::MAX as u128) as u64;
    turn_ledger.delivery = super::turn_finalize::build_turn_delivery_ledger(delivery);
    turn_ledger.observation = turn_observation;
    turn_ledger.subject_state = if msg.ingress == IngressKind::User {
        subject_state
            .as_ref()
            .and_then(build_turn_subject_state_ledger)
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.subject_state)
    };
    turn_ledger.persona = if msg.ingress == IngressKind::User {
        super::worker_governance::build_turn_persona_ledger(
            pressure,
            worker_latency.tool_calls,
            delivered,
            is_interrupt,
            mental_privacy_adjudication.as_ref(),
            persona_priority_adjudication.as_ref(),
            &mental_privacy_review,
            mental_privacy_review.applied
                && mental_privacy_review.reply_content.trim() != review_input_before.trim(),
        )
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.persona)
    };
    turn_ledger.soul_feedback = if msg.ingress == IngressKind::User {
        soul_feedback_projection
            .as_ref()
            .and_then(build_turn_soul_feedback_ledger)
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.soul_feedback)
    };
    turn_ledger.reasoning_intent = if msg.ingress == IngressKind::User {
        programmable_reasoning_intent
            .as_ref()
            .map(crate::agent::reasoning_intent::build_turn_reasoning_intent_ledger)
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.reasoning_intent)
    };
    turn_ledger.counterfactual = if msg.ingress == IngressKind::User {
        counterfactual_analysis
            .as_ref()
            .map(crate::agent::counterfactual::build_turn_counterfactual_ledger)
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.counterfactual)
    };
    turn_ledger.adversarial_arena = if msg.ingress == IngressKind::User {
        adversarial_arena_adjudication
            .as_ref()
            .map(crate::agent::adversarial_arena::build_turn_adversarial_arena_ledger)
    } else {
        let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
        config
            .runtime
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.adversarial_arena)
    };
    super::turn_finalize::persist_turn_ledger(
        config.runtime.turn_ledger_store.as_ref(),
        &crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id),
        &turn_ledger,
        "finish",
    );
    if let Some(adjudication) = adversarial_arena_adjudication.as_ref() {
        crate::reasoning::append_adversarial_arena_event(
            crate::agent::adversarial_arena::build_turn_adversarial_arena_timeline_event(
                adjudication,
                turn_ledger.finished_at_ms / 1000,
            ),
        );
    }
    if msg.ingress == IngressKind::User {
        super::turn_finalize::sync_user_turn_relationship_topology(
            config,
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
            turn_ledger.finished_at_ms / 1000,
        );
    }
    metrics::record_react_rounds(worker_latency.react_rounds);
    metrics::record_tool_calls_last(worker_latency.tool_calls);
    metrics::record_request_semantics_ms(worker_latency.request_semantics_ms);
    metrics::record_tool_exec_ms(worker_latency.tool_exec_ms);
    metrics::record_surface_finalize_ms(worker_latency.surface_finalize_ms);
    metrics::record_mental_privacy_review_ms(worker_latency.mental_privacy_review_ms);
    metrics::record_ttft_ms(worker_latency.ttft_ms.unwrap_or(0));
    metrics::record_e2e_ms(reply_handoff_ms);
    metrics::record_post_reply_ms(post_reply_ms);
    if msg.ingress == IngressKind::System {
        let is_cron = msg.channel.as_ref() == CHANNEL_CRON;
        metrics::record_system_message_done(is_cron);
        if is_cron {
            let cron_e2e = super::now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
            metrics::record_cron_e2e_ms(cron_e2e);
        }
    } else {
        metrics::record_user_message_done();
    }
    super::log_agent_latency_summary(
        worker_lane_tag,
        msg.req_id.as_deref().unwrap_or_default(),
        msg.channel.as_ref(),
        msg.chat_id.as_ref(),
        queue_wait_ms,
        admission_ms,
        worker_prepare_ms,
        &worker_latency,
        llm_ms,
        outbound_enqueue_ms,
        reply_handoff_ms,
        post_reply_ms,
        total_ms,
        streamed,
        delivered,
        latency_warn_ms,
    );
}
