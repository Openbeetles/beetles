use super::*;

pub(super) struct FinalizedTurn {
    pub(super) delivery: DeliveryReport,
    pub(super) reply_content: String,
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
    pub(super) used_final_answer_recovery: bool,
    pub(super) pressure: crate::orchestrator::PressureLevel,
    pub(super) prompt_recall_intent: crate::memory::PromptRecallIntent,
    pub(super) runtime_skill_selected_ids: Vec<String>,
    pub(super) task_learning_selected_ids: Vec<String>,
    pub(super) subject_state: Option<SubjectState>,
    pub(super) mental_privacy_adjudication:
        Option<crate::memory::MentalPrivacyDisclosureAdjudication>,
    pub(super) persona_priority_adjudication: Option<PersonaPriorityAdjudication>,
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
    } else if telemetry.used_final_answer_recovery {
        "final_recovery"
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
        used_final_answer_recovery,
        task_execution_used: _task_execution_used,
        pressure,
        runtime_mode: _runtime_mode,
        deliberation_class: _deliberation_class,
        request_semantics: _request_semantics,
        reply_surface,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        subject_state,
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
        WorkerOutcome::Delivered(s) => {
            let cow = truncate_content_to_max(&s, MAX_CONTENT_LEN);
            let s = if let Cow::Borrowed(_) = &cow {
                s
            } else {
                cow.into_owned()
            };
            (s, false, true, false)
        }
    };

    if !is_interrupt && apply_finalizer {
        reply_content = finalize_user_visible_reply(config.strategy, &reply_content);
    }
    let review_input_before = reply_content.clone();
    let mut mental_privacy_review = MentalPrivacyReviewOutcome {
        reply_content: reply_content.clone(),
        action: crate::memory::MentalPrivacyShareAction::AllowOriginal,
        applied: false,
        touched_targets: Vec::new(),
    };
    if !is_interrupt {
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
    if !is_interrupt
        && reply_content.trim().is_empty()
        && msg.ingress == IngressKind::User
        && msg.channel.as_ref() != CHANNEL_CRON
    {
        metrics::record_empty_final_blocked();
        log::warn!(
            "[reply_surface] empty finalized reply blocked surface={} finalization_policy={:?} governance_policy={:?} channel={} chat_id={}",
            reply_surface.as_str(),
            reply_surface.finalization_policy(),
            reply_surface.governance_policy(),
            msg.channel,
            msg.chat_id
        );
        return Err(crate::error::Error::config(
            "final_reply_empty_after_finalize",
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
    let mark_important = !is_interrupt && reply_content.contains(AGENT_MARKER_MARK_IMPORTANT);
    let signal_comfort = !is_interrupt && reply_content.contains(AGENT_MARKER_SIGNAL_COMFORT);
    if mark_important || signal_comfort {
        reply_content = remove_substrings_all_trim(
            &reply_content,
            &[AGENT_MARKER_MARK_IMPORTANT, AGENT_MARKER_SIGNAL_COMFORT],
        );
        if signal_comfort {
            let _ = config.emotion_signal_store.set(&msg.chat_id, "comfort");
        }
        reply_content = truncate_content_to_max(&reply_content, MAX_CONTENT_LEN).into_owned();
    }
    if !is_interrupt && !reply_content.is_empty() {
        metrics::record_final_answer_call();
    }

    Ok(FinalizedTurn {
        delivery,
        skip_delivery: reply_content.trim() == "SILENT"
            || (msg.channel.as_ref() == CHANNEL_CRON && reply_content.is_empty()),
        reply_content,
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
        used_final_answer_recovery,
        pressure,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        subject_state,
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
        reply_content,
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
        used_final_answer_recovery,
        pressure,
        prompt_recall_intent,
        runtime_skill_selected_ids,
        task_learning_selected_ids,
        subject_state,
        mental_privacy_adjudication,
        persona_priority_adjudication,
        ..
    } = finalized;

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
        config.session_store.append_batch(&msg.chat_id, &entries)
    } else {
        config
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
            .important_message_store
            .set_important_offset_from_end(&msg.chat_id, 1);
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
    } else if used_final_answer_recovery {
        crate::skills::RuntimeSkillReuseOutcome::Mismatch
    } else {
        crate::skills::RuntimeSkillReuseOutcome::Succeeded
    };
    let reuse_outcome_note = if used_surface_finalization {
        "surface_finalization"
    } else if used_final_answer_recovery {
        "final_recovery"
    } else if reply_already_delivered || delivery.current_primary_delivered {
        "current_primary"
    } else {
        "final_answer"
    };

    if delivered
        && !super::background_jobs::enqueue_post_reply_maintenance_job(
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
    if delivered
        && !crate::memory::enqueue_self_runtime_post_reply(
            system_inbound_tx,
            config.self_continuity_store.as_ref(),
            config.autonomy_strategy_store.as_ref(),
            config.self_authored_core_store.as_ref(),
            config.memory_system_kind.memory_profile(),
            msg.chat_id.as_ref(),
            msg.channel.as_ref(),
            &msg.content,
            &reply_content,
            worker_latency.tool_calls,
            external_content_used,
        )
    {
        log::debug!(
            "[self_runtime] post-reply job skipped chat_id={}",
            msg.chat_id
        );
    }

    let total_ms = msg_start.elapsed().as_millis();
    let post_reply_ms = total_ms.saturating_sub(reply_handoff_ms);
    turn_ledger.status = if is_interrupt {
        TurnLedgerStatus::Interrupted
    } else {
        TurnLedgerStatus::Answered
    };
    turn_ledger.reason = normalize_turn_reason(if is_interrupt {
        "interrupt"
    } else if reply_already_delivered || delivery.current_primary_delivered {
        "current_primary"
    } else if used_surface_finalization {
        "surface_finalization"
    } else if used_final_answer_recovery {
        "final_recovery"
    } else {
        "final_answer"
    });
    turn_ledger.reply_preview = normalize_turn_preview(&reply_content);
    turn_ledger.updated_at_ms = super::now_unix_ms();
    turn_ledger.finished_at_ms = turn_ledger.updated_at_ms;
    turn_ledger.react_rounds = worker_latency.react_rounds;
    turn_ledger.tool_calls = worker_latency.tool_calls;
    turn_ledger.any_tool_used = any_tool_used;
    turn_ledger.final_answer_recovered = used_final_answer_recovery;
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
            .turn_ledger_store
            .get(&relationship_id)
            .ok()
            .flatten()
            .and_then(|ledger| ledger.persona)
    };
    super::turn_finalize::persist_turn_ledger(
        config.turn_ledger_store.as_ref(),
        &crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id),
        &turn_ledger,
        "finish",
    );
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
    metrics::record_final_recovery_ms(worker_latency.final_recovery_ms);
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
