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
    Foreground {
        _turn_guard: crate::orchestrator::ForegroundTurnGuard,
        runtime_foreground_ticket: Option<crate::runtime::RuntimeForegroundTicket>,
    },
    Background(crate::orchestrator::AgentTaskGuard),
}

impl AdmittedTurnGuard {
    pub(super) fn finish_user_visible_delivery_window(&mut self) {
        if let Self::Foreground {
            runtime_foreground_ticket,
            ..
        } = self
        {
            if let Some(ticket) = runtime_foreground_ticket.take() {
                let _ = crate::runtime::finish_runtime_foreground(ticket);
            }
        }
    }
}

impl Drop for AdmittedTurnGuard {
    fn drop(&mut self) {
        self.finish_user_visible_delivery_window();
    }
}

fn renew_runtime_foreground_for_admitted_turn(
    msg: &PcMsg,
) -> Option<crate::runtime::RuntimeForegroundTicket> {
    msg.runtime_foreground_source()
        .map(crate::runtime::renew_runtime_foreground_now)
}

#[cfg(test)]
fn renew_runtime_foreground_for_admitted_turn_at(
    msg: &PcMsg,
    now_ms: u64,
) -> Option<crate::runtime::RuntimeForegroundTicket> {
    msg.runtime_foreground_source()
        .map(|source| crate::runtime::renew_runtime_foreground(source, now_ms))
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
            Ok(guard) => {
                let runtime_foreground_ticket = renew_runtime_foreground_for_admitted_turn(&msg);
                AdmittedTurnGuard::Foreground {
                    _turn_guard: guard,
                    runtime_foreground_ticket,
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admitted_user_turn_renews_foreground_at_admission_boundary() {
        let _guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "primary", false).expect("message");

        let ticket = renew_runtime_foreground_for_admitted_turn_at(&msg, 1_000)
            .expect("user message creates foreground ticket");

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot_at(1_001);
        assert!(snapshot.active);
        assert_eq!(
            snapshot.primary_source,
            Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage)
        );
        assert_eq!(
            snapshot.records[0].ticket, ticket,
            "the admitted turn must keep the concrete ticket it will finish after primary delivery"
        );
    }

    #[test]
    fn delivered_foreground_turn_finishes_runtime_ticket_into_recovery() {
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        let _foreground_guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        let ticket = crate::runtime::renew_runtime_foreground_now(
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
        );
        let mut guard = AdmittedTurnGuard::Foreground {
            _turn_guard: crate::orchestrator::begin_foreground_turn()
                .expect("foreground turn should acquire lease"),
            runtime_foreground_ticket: Some(ticket),
        };

        guard.finish_user_visible_delivery_window();

        let snapshot = crate::runtime::foreground::runtime_foreground_snapshot();
        assert!(!snapshot.active);
        assert!(snapshot.recovery_active);
        assert_eq!(
            snapshot.recovery_source,
            Some(crate::runtime::RuntimeForegroundSource::ExternalUserMessage)
        );
    }

    #[test]
    fn foreground_user_turn_does_not_request_external_wss_evict() {
        let _state_guard = crate::state::test_state_guard();
        let _lease_guard = crate::runtime::lease::lease_test_guard();
        let _foreground_guard = crate::runtime::foreground::runtime_foreground_test_guard();
        crate::runtime::foreground::reset_runtime_foreground_for_tests();
        crate::network::set_external_wss_managed_present(true);
        let ticket = crate::runtime::renew_runtime_foreground_now(
            crate::runtime::RuntimeForegroundSource::ExternalUserMessage,
        );

        let _guard = AdmittedTurnGuard::Foreground {
            _turn_guard: crate::orchestrator::begin_foreground_turn()
                .expect("foreground turn should acquire lease"),
            runtime_foreground_ticket: Some(ticket),
        };

        assert!(!crate::network::external_wss_suspend_requested());
        assert!(!crate::network::external_wss_worker_evict_requested());
        assert_eq!(crate::network::active_external_wss_count(), 0);
        assert!(
            crate::network::external_wss_managed_present(),
            "ordinary foreground user work must keep the active external ingress owner online"
        );
    }
}
