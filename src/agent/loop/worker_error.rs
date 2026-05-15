use super::*;

fn worker_path_error_uses_maintenance_copy(error: &crate::error::Error) -> bool {
    !crate::agent::final_reply::is_reply_contract_breach_stage(error.stage())
}

#[cold]
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_worker_path_error(
    error: crate::error::Error,
    worker_lane_tag: &str,
    msg: &mut PcMsg,
    loc: UiLocale,
    msg_start: Instant,
    queue_wait_ms: u128,
    admission_ms: u128,
    worker_prepare_ms: u128,
    msg_key: u64,
    llm_failure_count: &mut HashMap<u64, (u8, Instant)>,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    config: &AgentLoopConfig,
    turn_ledger: &mut TurnLedger,
) {
    let relationship_id = crate::memory::relationship_scope_id(&msg.channel, &msg.chat_id);
    let llm_ms = msg_start
        .elapsed()
        .as_millis()
        .saturating_sub(admission_ms)
        .saturating_sub(worker_prepare_ms);
    let total_ms = msg_start.elapsed().as_millis();
    let user_message = if worker_path_error_uses_maintenance_copy(&error) {
        UiMessage::NodeMaintenance
    } else {
        UiMessage::OperationFailed
    };
    turn_ledger.status = TurnLedgerStatus::Failed;
    turn_ledger.reason = normalize_turn_reason(error.stage());
    turn_ledger.updated_at_ms = now_unix_ms();
    turn_ledger.finished_at_ms = turn_ledger.updated_at_ms;
    turn_ledger.total_ms = total_ms.min(u64::MAX as u128) as u64;
    turn_ledger.reply_preview = normalize_turn_preview(&tr(user_message.clone(), loc));
    persist_turn_ledger(
        config.runtime.turn_ledger_store.as_ref(),
        &relationship_id,
        turn_ledger,
        "error",
    );
    crate::platform::task_wdt::feed_current_task();
    metrics::record_error_by_stage(error.metrics_stage());
    log::warn!("[agent:{}] chat loop failed: {}", worker_lane_tag, error);
    log::warn!(
        "[latency][agent:{}] req_id={} channel={} chat_id={} queue_wait_ms={} admission_ms={} worker_prepare_ms={} llm_ms={} total_ms={} status=llm_error",
        worker_lane_tag,
        msg.req_id.as_deref().unwrap_or_default(),
        msg.channel,
        msg.chat_id,
        queue_wait_ms,
        admission_ms,
        worker_prepare_ms,
        llm_ms,
        total_ms
    );
    state::set_last_error(&error);

    if worker_path_error_uses_maintenance_copy(&error) {
        let (counter, _) = llm_failure_count
            .entry(msg_key)
            .or_insert((0, Instant::now()));
        *counter = counter.saturating_add(1);

        if *counter < 3 && error.is_retryable_upstream() {
            msg.enqueue_ts_ms = now_unix_ms();
            let inbound_tx = choose_inbound_tx(msg.ingress, user_inbound_tx, system_inbound_tx);
            match inbound_tx.try_send(msg.clone()) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(m)) => {
                    match config.runtime.pending_retry_store.save_pending_retry(&m) {
                        Ok(()) => {
                            log::warn!(
                                "[agent] llm retry: inbound full, pending_retry saved chat_id={}",
                                m.chat_id
                            );
                        }
                        Err(error) => {
                            metrics::record_error_by_stage(error.metrics_stage());
                            log::error!(
                                "[agent] llm retry: pending_retry save failed chat_id={}: {}",
                                m.chat_id,
                                error
                            );
                        }
                    }
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::error!("[agent] inbound_tx disconnected during llm retry");
                }
            }
            let delay_ms =
                (AGENT_RETRY_BASE_MS * (1 << (*counter as u64).min(4))).min(AGENT_RETRY_MAX_MS);
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            return;
        }
    }

    if crate::chat_stream::is_configure_ui_stream_turn(msg) {
        if let Some(stream_id) = msg.req_id.as_deref() {
            config
                .chat_streams
                .emit_error(stream_id, "chat.failed", Some(error.stage()));
        }
        return;
    }

    match PcMsg::new_outbound_reply_to(msg, tr(user_message, loc)) {
        Ok(reply) => {
            turn_ledger.reason = normalize_turn_reason("chat_failure_copy");
            turn_ledger.outbound_source = normalize_turn_reason("chat-failure");
            turn_ledger.canonical_reply_source.clear();
            persist_turn_ledger(
                config.runtime.turn_ledger_store.as_ref(),
                &relationship_id,
                turn_ledger,
                "error_reply",
            );
            metrics::record_internal_error_copy_suppressed();
            if try_send_outbound(outbound_tx, reply, "chat-failure") {
                let req_id = msg.req_id.as_deref().unwrap_or("chat-failure");
                crate::agent::delivery::send_terminal_reaction_if_enabled(
                    msg,
                    req_id,
                    outbound_tx,
                    config.channel_capability_registry.get(msg.channel.as_ref()),
                    false,
                );
            }
        }
        Err(build_error) => {
            metrics::record_error_by_stage(build_error.metrics_stage());
            log::error!(
                "[agent] failed to build chat-failure reply channel={} chat_id={}: {}",
                msg.channel,
                msg.chat_id,
                build_error
            );
        }
    }
}
