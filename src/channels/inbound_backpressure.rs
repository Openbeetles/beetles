//! Shared inbound channel backpressure accounting for channel event sources.

/// Bounded event sources that enter Beetle through an existing queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventIngressSource {
    WssGateway,
    #[cfg(feature = "telegram")]
    TelegramPoll,
    RuntimeInitiative,
    Heartbeat,
    WriteBack,
}

/// Whether an ingress event must survive mode changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventIngressRetention {
    Deadline,
    BestEffort,
}

/// Stable full-queue behavior for an ingress owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventIngressFullPolicy {
    DeferToPendingRetry,
    RequestRedelivery,
    Drop,
    Coalesce,
}

/// Minimal P3.2 ingress contract.
///
/// This deliberately describes the current queues instead of introducing a new
/// router. Capacity is the fixed queue bound, `owner_key` is the stable
/// cancellation/coalescing owner, and `full_policy` documents what happens when
/// the owner cannot enqueue work immediately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EventIngressContract {
    pub owner_key: &'static str,
    pub capacity: usize,
    pub retention: EventIngressRetention,
    pub full_policy: EventIngressFullPolicy,
}

pub(crate) fn event_ingress_contract(source: EventIngressSource) -> EventIngressContract {
    let bus_capacity = crate::constants::DEFAULT_CAPACITY;
    match source {
        EventIngressSource::WssGateway => EventIngressContract {
            owner_key: "external_wss",
            capacity: bus_capacity,
            retention: EventIngressRetention::Deadline,
            full_policy: EventIngressFullPolicy::DeferToPendingRetry,
        },
        #[cfg(feature = "telegram")]
        EventIngressSource::TelegramPoll => EventIngressContract {
            owner_key: "telegram_poll",
            capacity: bus_capacity,
            retention: EventIngressRetention::Deadline,
            full_policy: EventIngressFullPolicy::DeferToPendingRetry,
        },
        EventIngressSource::RuntimeInitiative => EventIngressContract {
            owner_key: "runtime_initiative",
            capacity: bus_capacity,
            retention: EventIngressRetention::BestEffort,
            full_policy: EventIngressFullPolicy::Drop,
        },
        EventIngressSource::Heartbeat => EventIngressContract {
            owner_key: "heartbeat",
            capacity: bus_capacity,
            retention: EventIngressRetention::BestEffort,
            full_policy: EventIngressFullPolicy::Drop,
        },
        EventIngressSource::WriteBack => EventIngressContract {
            owner_key: "write_back",
            capacity: crate::runtime::write_back::WRITE_BACK_QUEUE_MAX,
            retention: EventIngressRetention::BestEffort,
            full_policy: EventIngressFullPolicy::Coalesce,
        },
    }
}

/// Outcome when an event source cannot enqueue an inbound message immediately.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InboundBackpressureOutcome {
    /// Message is persisted locally and replayed later.
    DeferredToPendingRetry,
    /// Upstream is left unacknowledged so it can redeliver.
    RedeliveryRequested,
    /// Message cannot be replayed and is dropped.
    Dropped,
}

pub(crate) fn record_queue_full(outcome: InboundBackpressureOutcome) {
    crate::metrics::record_inbound_queue_full();
    crate::metrics::record_event_ingress_rejected();
    match outcome {
        InboundBackpressureOutcome::DeferredToPendingRetry
        | InboundBackpressureOutcome::RedeliveryRequested => {
            crate::metrics::record_inbound_defer();
        }
        InboundBackpressureOutcome::Dropped => {
            crate::metrics::record_inbound_drop();
        }
    }
}

pub(crate) fn record_queue_full_for_source(
    source: EventIngressSource,
    outcome: InboundBackpressureOutcome,
) {
    let contract = event_ingress_contract(source);
    let recorded_policy = match outcome {
        InboundBackpressureOutcome::DeferredToPendingRetry => {
            EventIngressFullPolicy::DeferToPendingRetry
        }
        InboundBackpressureOutcome::RedeliveryRequested => {
            EventIngressFullPolicy::RequestRedelivery
        }
        InboundBackpressureOutcome::Dropped => EventIngressFullPolicy::Drop,
    };
    debug_assert!(
        contract.full_policy == recorded_policy || matches!(source, EventIngressSource::WssGateway),
        "event ingress source full policy drifted from recorded outcome"
    );
    record_queue_full(outcome);
}

pub(crate) fn record_deferred_without_queue_full() {
    crate::metrics::record_inbound_defer();
    crate::metrics::record_event_ingress_rejected();
}

pub(crate) fn record_deferred_without_queue_full_for_source(source: EventIngressSource) {
    let contract = event_ingress_contract(source);
    debug_assert!(matches!(
        contract.full_policy,
        EventIngressFullPolicy::DeferToPendingRetry | EventIngressFullPolicy::RequestRedelivery
    ));
    record_deferred_without_queue_full();
}

pub(crate) fn record_drop_without_queue_full() {
    crate::metrics::record_inbound_drop();
    crate::metrics::record_event_ingress_rejected();
}

pub(crate) fn record_drop_without_queue_full_for_source(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    record_drop_without_queue_full();
}

pub(crate) fn record_disconnected_drop() {
    crate::metrics::record_inbound_drop();
    crate::metrics::record_event_ingress_rejected();
}

pub(crate) fn record_disconnected_drop_for_source(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    record_disconnected_drop();
}

pub(crate) fn record_enqueued(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    crate::metrics::record_event_ingress_enqueued();
}

pub(crate) fn record_rejected(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    crate::metrics::record_event_ingress_rejected();
}

pub(crate) fn record_cancelled(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    crate::metrics::record_event_ingress_cancelled();
}

pub(crate) fn record_purged(source: EventIngressSource) {
    let contract = event_ingress_contract(source);
    debug_assert_eq!(contract.retention, EventIngressRetention::BestEffort);
    crate::metrics::record_event_ingress_purged();
}

pub(crate) fn record_stale_drop(source: EventIngressSource) {
    let _ = event_ingress_contract(source);
    crate::metrics::record_event_ingress_stale_drop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_full_outcome_records_defer_or_drop() {
        let before = crate::metrics::snapshot();

        record_queue_full(InboundBackpressureOutcome::DeferredToPendingRetry);
        record_queue_full(InboundBackpressureOutcome::RedeliveryRequested);
        record_queue_full(InboundBackpressureOutcome::Dropped);
        record_disconnected_drop();
        record_deferred_without_queue_full();
        record_drop_without_queue_full();

        let after = crate::metrics::snapshot();
        assert!(after.inbound_queue_full_total >= before.inbound_queue_full_total + 3);
        assert!(after.inbound_defer_total >= before.inbound_defer_total + 3);
        assert!(after.inbound_drop_total >= before.inbound_drop_total + 3);
        assert!(after.event_ingress_rejected_total >= before.event_ingress_rejected_total + 6);
    }

    #[test]
    fn ingress_contracts_define_capacity_owner_retention_and_full_policy() {
        let wss = event_ingress_contract(EventIngressSource::WssGateway);
        assert_eq!(wss.capacity, crate::constants::DEFAULT_CAPACITY);
        assert_eq!(wss.owner_key, "external_wss");
        assert_eq!(wss.retention, EventIngressRetention::Deadline);
        assert_eq!(wss.full_policy, EventIngressFullPolicy::DeferToPendingRetry);

        let initiative = event_ingress_contract(EventIngressSource::RuntimeInitiative);
        assert_eq!(initiative.owner_key, "runtime_initiative");
        assert_eq!(initiative.retention, EventIngressRetention::BestEffort);
        assert_eq!(initiative.full_policy, EventIngressFullPolicy::Drop);

        let write_back = event_ingress_contract(EventIngressSource::WriteBack);
        assert_eq!(
            write_back.capacity,
            crate::runtime::write_back::WRITE_BACK_QUEUE_MAX
        );
        assert_eq!(write_back.full_policy, EventIngressFullPolicy::Coalesce);
    }

    #[test]
    fn mode_transition_purges_only_best_effort_work() {
        let before = crate::metrics::snapshot();

        record_purged(EventIngressSource::RuntimeInitiative);
        record_cancelled(EventIngressSource::WriteBack);
        record_stale_drop(EventIngressSource::Heartbeat);

        let after = crate::metrics::snapshot();
        assert!(after.event_ingress_purged_total > before.event_ingress_purged_total);
        assert!(after.event_ingress_cancelled_total > before.event_ingress_cancelled_total);
        assert!(after.event_ingress_stale_drop_total > before.event_ingress_stale_drop_total);
    }
}
