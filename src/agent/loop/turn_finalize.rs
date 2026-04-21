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
        .runtime
        .turn_ledger_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let mental_privacy_state = config
        .runtime
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let outer_voice = config
        .runtime
        .outer_voice_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let world_sense = config
        .runtime
        .world_sense_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let recent_persona_evidence =
        load_recent_persona_evidence(config.runtime.turn_ledger_store.as_ref(), &relationship_id)
            .ok()
            .flatten();
    if let Err(error) = upsert_relationship_topology_entry(
        config.runtime.relationship_topology_store.as_ref(),
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
