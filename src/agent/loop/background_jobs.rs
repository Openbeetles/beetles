#![allow(clippy::too_many_arguments)]

use super::*;

pub(super) fn enqueue_post_reply_maintenance_job(
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
    reply_content: &str,
    tool_calls: u32,
    external_content_used: bool,
) -> bool {
    let payload = PostReplyMaintenanceJobPayload::from_turn(
        msg,
        reply_content,
        tool_calls,
        external_content_used,
    );
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            log::warn!(
                "[agent_memory] maintenance job serialize failed chat_id={}: {}",
                msg.chat_id,
                error
            );
            return false;
        }
    };
    let system_inbound_tx = system_inbound_tx.clone();
    let chat_id = msg.chat_id.to_string();
    let scheduled = crate::runtime::schedule_delayed_task(
        Instant::now() + Duration::from_millis(POST_REPLY_MAINTENANCE_DELAY_MS),
        Box::new(move || {
            if let Some(reason) = super::background_enqueue_block_reason() {
                log::debug!(
                    "[agent_memory] skip delayed maintenance enqueue because {} chat_id={}",
                    reason,
                    chat_id
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
                    return;
                }
            };
            match system_inbound_tx.try_send(job) {
                Ok(()) => {}
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    log::debug!(
                        "[agent_memory] skip maintenance enqueue because system queue is full chat_id={}",
                        chat_id
                    );
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    log::warn!(
                        "[agent_memory] maintenance enqueue failed: system queue disconnected"
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

pub(super) fn run_post_reply_maintenance_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) {
    super::run_post_reply_maintenance_job(http, worker_llm, config, system_inbound_tx, msg);
}

pub(super) fn run_self_runtime_job(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    config: &AgentLoopConfig,
    system_inbound_tx: &SystemInboundTx,
    msg: &PcMsg,
) {
    super::run_self_runtime_job(http, worker_llm, config, system_inbound_tx, msg);
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
        super::run_long_term_memory_refresh_job(http, worker_llm, config, msg);
        return true;
    }
    if super::is_post_reply_maintenance_job(msg) {
        run_post_reply_maintenance_job(http, worker_llm, config, system_inbound_tx, msg);
        return true;
    }
    if super::is_self_runtime_job(msg) {
        run_self_runtime_job(http, worker_llm, config, system_inbound_tx, msg);
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
    if crate::state::voice_exclusive_active() {
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
    let _maintenance_scope = BackgroundMaintenanceScope::enter();
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
            let defer_out = PcMsg {
                channel: msg.channel.clone(),
                chat_id: msg.chat_id.clone(),
                content: tr(UiMessage::LowMemoryUserDefer, ctx.loc),
                req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
                ingress: IngressKind::User,
                enqueue_ts_ms: super::now_unix_ms(),
                is_group: false,
            };
            let _ = super::try_send_outbound(ctx.outbound_tx, defer_out, "defer-limit");
        }
        return;
    }

    if msg.ingress == IngressKind::User {
        let defer_out = PcMsg {
            channel: msg.channel.clone(),
            chat_id: msg.chat_id.clone(),
            content: tr(UiMessage::LowMemoryUserDefer, ctx.loc),
            req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
            ingress: IngressKind::User,
            enqueue_ts_ms: super::now_unix_ms(),
            is_group: false,
        };
        let _ = super::try_send_outbound(ctx.outbound_tx, defer_out, "defer");
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
