//! Shared inbound channel backpressure accounting for channel event sources.

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

pub(crate) fn record_deferred_without_queue_full() {
    crate::metrics::record_inbound_defer();
}

pub(crate) fn record_disconnected_drop() {
    crate::metrics::record_inbound_drop();
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

        let after = crate::metrics::snapshot();
        assert!(after.inbound_queue_full_total >= before.inbound_queue_full_total + 3);
        assert!(after.inbound_defer_total >= before.inbound_defer_total + 3);
        assert!(after.inbound_drop_total >= before.inbound_drop_total + 2);
    }
}
