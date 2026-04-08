use super::*;

pub(super) fn persist_turn_ledger(
    store: &dyn TurnLedgerStore,
    chat_id: &str,
    ledger: &TurnLedger,
    stage: &str,
) {
    if let Err(error) = store.set(chat_id, ledger) {
        log::warn!(
            "[agent_turn] failed to persist ledger stage={} chat_id={}: {}",
            stage,
            chat_id,
            error
        );
    }
}

pub(super) fn sync_user_turn_relationship_topology(
    config: &AgentLoopConfig,
    channel: &str,
    chat_id: &str,
    now_secs: u64,
) {
    let relationship_id = crate::memory::relationship_scope_id(channel, chat_id);
    let turn_ledger = config
        .turn_ledger_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let mental_privacy_state = config
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let outer_voice = config
        .outer_voice_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let world_sense = config
        .world_sense_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let recent_persona_evidence =
        load_recent_persona_evidence(config.turn_ledger_store.as_ref(), &relationship_id)
            .ok()
            .flatten();
    if let Err(error) = upsert_relationship_topology_entry(
        config.relationship_topology_store.as_ref(),
        crate::memory::RelationshipTopologyUpsertInput {
            channel,
            chat_id,
            now_secs,
            touch_user_turn: true,
            touch_runtime_refresh: false,
            turn_ledger: turn_ledger.as_ref(),
            mental_privacy_state: mental_privacy_state.as_ref(),
            outer_voice: outer_voice.as_ref(),
            world_sense: world_sense.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        },
    ) {
        log::warn!(
            "[agent_relationship_topology] user-turn sync failed channel={} chat_id={}: {}",
            channel,
            chat_id,
            error
        );
    }
}

pub(super) fn build_turn_delivery_ledger(report: DeliveryReport) -> TurnDeliveryLedger {
    TurnDeliveryLedger {
        waiting_notice_sent: report.waiting_notice_sent,
        progress_updates_sent: report.progress_updates_sent,
        partial_updates_sent: report.partial_updates_sent,
        tool_outbound_intents_seen: report.tool_outbound_intents_seen,
        tool_visible_updates_sent: report.tool_visible_updates_sent,
        explicit_outbound_sent: report.explicit_outbound_sent,
        tool_outbound_suppressed: report.tool_outbound_suppressed,
        current_primary_delivered: report.current_primary_delivered,
        finalize_streamed: report.finalize_streamed,
        visible_text_updates_sent: report.visible_text_updates_sent,
    }
}

#[inline(never)]
pub(super) fn finalize_lane_turn(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    ctx: LaneTurnFinalizeContext<'_>,
    llm_failure_count: &mut HashMap<u64, (u8, Instant)>,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    outcome: WorkerOutcome,
    telemetry: WorkerRunTelemetry,
) {
    let final_outcome = if matches!(outcome, WorkerOutcome::Interrupt(_)) {
        "interrupt"
    } else if telemetry.delivery.current_primary_delivered {
        "current_primary"
    } else if telemetry.used_final_answer_recovery {
        "final_recovery"
    } else {
        "final_answer"
    };
    let turn_observation = build_turn_observation_ledger(
        final_outcome,
        matches!(outcome, WorkerOutcome::Interrupt(_)),
        &telemetry,
    );
    let LaneTurnFinalizeContext {
        worker_lane_tag,
        config,
        system_inbound_tx,
        outbound_tx,
        msg,
        loc,
        msg_start,
        queue_wait_ms,
        admission_ms,
        worker_prepare_ms,
        msg_key,
        mut turn_ledger,
        latency_warn_ms,
    } = ctx;
    let WorkerRunTelemetry {
        streamed,
        latency: mut worker_latency,
        delivery,
        any_tool_used,
        external_content_used,
        used_final_answer_recovery,
        task_execution_used: _task_execution_used,
        pressure,
        runtime_mode: _runtime_mode,
        deliberation_class: _deliberation_class,
        tool_blocker: _tool_blocker,
        subject_state,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    } = telemetry;

    let (mut reply_content, is_interrupt, reply_already_delivered, apply_finalizer) = match outcome
    {
        WorkerOutcome::Interrupt(confirm) => {
            let cow = truncate_content_to_max(&confirm, MAX_CONTENT_LEN);
            let s = if let Cow::Borrowed(_) = &cow {
                confirm
            } else {
                cow.into_owned()
            };
            (s, true, false, false)
        }
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
        mental_privacy_review = super::maybe_apply_mental_privacy_review(
            http,
            worker_llm,
            config,
            &msg,
            loc,
            reply_content,
        );
        reply_content = mental_privacy_review.reply_content.clone();
    }
    if !is_interrupt
        && reply_content.trim().is_empty()
        && msg.ingress == IngressKind::User
        && msg.channel.as_ref() != CHANNEL_CRON
    {
        reply_content = tr(UiMessage::AgentNoFinalReply, loc);
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

    if reply_content.trim() == "SILENT"
        || (msg.channel.as_ref() == CHANNEL_CRON && reply_content.is_empty())
    {
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

    let outbound_start = Instant::now();
    let delivered = if reply_already_delivered {
        crate::platform::task_wdt::feed_current_task();
        true
    } else if !streamed {
        let out = PcMsg {
            channel: msg.channel.clone(),
            chat_id: msg.chat_id.clone(),
            content: reply_content.clone(),
            req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
            ingress: IngressKind::User,
            enqueue_ts_ms: super::now_unix_ms(),
            is_group: false,
        };
        crate::platform::task_wdt::feed_current_task();
        super::try_send_outbound(outbound_tx, out, "reply")
    } else {
        metrics::record_message_out();
        crate::platform::task_wdt::feed_current_task();
        true
    };
    let outbound_enqueue_ms = outbound_start.elapsed().as_millis();
    let reply_handoff_ms = if delivered {
        msg_start.elapsed().as_millis()
    } else {
        0
    };

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

    if delivered
        && !super::enqueue_post_reply_maintenance_job(
            system_inbound_tx,
            &msg,
            &reply_content,
            worker_latency.tool_calls,
            external_content_used,
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
    turn_ledger.delivery = build_turn_delivery_ledger(delivery);
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
        super::build_turn_persona_ledger(
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
    persist_turn_ledger(
        config.turn_ledger_store.as_ref(),
        &crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id),
        &turn_ledger,
        "finish",
    );
    if msg.ingress == IngressKind::User {
        sync_user_turn_relationship_topology(
            config,
            msg.channel.as_ref(),
            msg.chat_id.as_ref(),
            turn_ledger.finished_at_ms / 1000,
        );
    }
    metrics::record_react_rounds(worker_latency.react_rounds);
    metrics::record_tool_calls_last(worker_latency.tool_calls);
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
