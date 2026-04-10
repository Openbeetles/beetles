use super::*;
use crate::orchestrator::AdmissionDecision;

pub(super) struct AdmittedTurn {
    pub(super) msg: PcMsg,
    pub(super) msg_key: u64,
    pub(super) queue_wait_ms: u128,
    pub(super) admission_ms: u128,
    pub(super) work_class: crate::runtime::system_work::SystemWorkClass,
    pub(super) _agent_task_guard: crate::orchestrator::AgentTaskGuard,
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

    let work_class = classify_system_work(msg.channel.as_ref(), msg.ingress);
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
        user_inbound_tx,
        system_inbound_tx,
        outbound_tx,
        config,
    ) {
        GateResult::Proceed(msg) => msg,
        GateResult::Skipped => return None,
    };

    Some(AdmittedTurn {
        msg,
        msg_key,
        queue_wait_ms,
        admission_ms,
        work_class,
        _agent_task_guard: crate::orchestrator::begin_agent_task(),
    })
}
