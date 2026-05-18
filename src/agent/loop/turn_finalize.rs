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

pub(super) fn persist_turn_continuity_evidence(
    store: &dyn crate::memory::TurnContinuityEvidenceStore,
    chat_id: &str,
    ledger: &TurnLedger,
    stage: &str,
) {
    let Some(evidence) = crate::memory::TurnContinuityEvidence::from_turn_ledger(ledger) else {
        return;
    };
    if let Err(error) = store.append(chat_id, &evidence) {
        log::warn!(
            "[agent_turn] failed to persist continuity evidence stage={} chat_id={}: {}",
            stage,
            chat_id,
            error
        );
    }
}

pub(super) fn build_turn_delivery_ledger(report: DeliveryReport) -> TurnDeliveryLedger {
    TurnDeliveryLedger {
        append_only_ack_sent: report.append_only_ack_sent,
        append_only_heartbeat_sent: report.append_only_heartbeat_sent,
        append_only_first_tool_milestone_sent: report.append_only_first_tool_milestone_sent,
        edit_phase_header_updates_sent: report.edit_phase_header_updates_sent,
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
