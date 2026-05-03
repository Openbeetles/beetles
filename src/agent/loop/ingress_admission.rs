use super::*;
use crate::orchestrator::AdmissionDecision;

pub(super) struct AdmittedTurn {
    pub(super) msg: PcMsg,
    pub(super) msg_key: u64,
    pub(super) queue_wait_ms: u128,
    pub(super) admission_ms: u128,
    pub(super) _agent_task_guard: AdmittedTurnGuard,
}

#[allow(dead_code)]
pub(super) enum AdmittedTurnGuard {
    Foreground(crate::orchestrator::ForegroundTurnGuard),
    Background(crate::orchestrator::AgentTaskGuard),
}

#[allow(clippy::too_many_arguments)]
pub(super) fn admit_turn(
    msg: PcMsg,
    msg_start: Instant,
    loc: UiLocale,
    user_inbound_tx: &UserInboundTx,
    system_inbound_tx: &SystemInboundTx,
    outbound_tx: &OutboundTx,
    config: &AgentLoopConfig,
    defer_tracker: &mut HashMap<u64, (u8, Instant)>,
    low_mem_defer_log: &mut Option<(Arc<str>, Instant)>,
) -> Option<AdmittedTurn> {
    let queue_wait_ms = now_unix_ms().saturating_sub(msg.enqueue_ts_ms) as u128;
    if msg.ingress == IngressKind::System {
        metrics::record_system_queue_wait_ms(queue_wait_ms);
    } else {
        metrics::record_user_queue_wait_ms(queue_wait_ms);
    }

    let msg_key = {
        let mut hasher = DefaultHasher::new();
        msg.channel.hash(&mut hasher);
        msg.chat_id.hash(&mut hasher);
        msg.content.hash(&mut hasher);
        hasher.finish()
    };

    crate::orchestrator::refresh_heap_if_stale();
    match crate::orchestrator::should_accept_inbound_pub(&msg.channel, msg.ingress) {
        AdmissionDecision::Accept => {}
        AdmissionDecision::Defer { delay_ms } => {
            super::background_jobs::handle_admission_defer(
                delay_ms,
                msg,
                msg_key,
                AdmissionDeferContext {
                    source: "inbound-admission",
                    loc,
                    user_inbound_tx,
                    system_inbound_tx,
                    outbound_tx,
                    config,
                    defer_tracker,
                    low_mem_defer_log,
                },
            );
            return None;
        }
        AdmissionDecision::Reject { reason } => {
            super::background_jobs::handle_admission_reject(reason, low_mem_defer_log);
            return None;
        }
    }

    let admission_ms = msg_start.elapsed().as_millis();
    let msg = match super::handle_llm_gate(
        msg,
        loc,
        msg_key,
        user_inbound_tx,
        system_inbound_tx,
        outbound_tx,
        config,
        defer_tracker,
        low_mem_defer_log,
    ) {
        GateResult::Proceed(msg) => *msg,
        GateResult::Skipped => return None,
    };

    let turn_guard = if msg.ingress == IngressKind::User {
        match crate::orchestrator::begin_foreground_turn() {
            Ok(guard) => AdmittedTurnGuard::Foreground(guard),
            Err(error) => {
                metrics::record_error_by_stage(error.metrics_stage());
                log::warn!(
                    "[agent] foreground turn lease denied channel={} chat_id={}: {}",
                    msg.channel,
                    msg.chat_id,
                    error
                );
                super::background_jobs::handle_admission_defer(
                    crate::constants::LOW_MEM_DEFER_SLEEP_MS,
                    msg,
                    msg_key,
                    AdmissionDeferContext {
                        source: "foreground-turn-lease",
                        loc,
                        user_inbound_tx,
                        system_inbound_tx,
                        outbound_tx,
                        config,
                        defer_tracker,
                        low_mem_defer_log,
                    },
                );
                return None;
            }
        }
    } else {
        AdmittedTurnGuard::Background(crate::orchestrator::begin_agent_task())
    };

    Some(AdmittedTurn {
        msg,
        msg_key,
        queue_wait_ms,
        admission_ms,
        _agent_task_guard: turn_guard,
    })
}
