//! Cross-plane governance kernel for canonical shared memory coordination.

use super::{
    build_shared_factual_plane_snapshot, memory_capability_profile, MemoryProfile, MemoryStore,
    SessionMessage, SessionStore, SharedFactualPlaneSnapshot, SharedFactualReconcileAction,
    TurnLedgerStore,
};

pub struct MemoryGovernanceContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub long_term_memory_store: &'a dyn super::LongTermMemoryStore,
    pub memory_store: &'a dyn MemoryStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
}

pub struct MemoryGovernanceInput<'a> {
    pub chat_id: &'a str,
    pub query_hint: &'a str,
    pub summary_text: Option<&'a str>,
    pub recent: &'a [SessionMessage],
    pub max_len: usize,
    pub profile: MemoryProfile,
    pub external_content_used: bool,
}

pub struct MemoryGovernanceOutcome {
    pub factual_plane_snapshot: SharedFactualPlaneSnapshot,
    pub strongest_action: Option<SharedFactualReconcileAction>,
    pub factual_coordination_summary: Option<String>,
    pub extraction_brief: Option<String>,
    pub factual_refresh_suggested: bool,
}

pub fn run_memory_governance_kernel(
    ctx: MemoryGovernanceContext<'_>,
    input: MemoryGovernanceInput<'_>,
) -> MemoryGovernanceOutcome {
    let capability = memory_capability_profile(input.profile);
    let max_len = input
        .max_len
        .min(capability.archive_prompt_max_chars.saturating_add(384));
    let factual_plane_snapshot = build_shared_factual_plane_snapshot(
        ctx.session_store,
        ctx.long_term_memory_store,
        ctx.memory_store,
        ctx.turn_ledger_store,
        input.chat_id,
        input.query_hint,
        input.summary_text,
        input.recent,
        max_len,
        input.profile,
    );
    let strongest_action = factual_plane_snapshot.strongest_refresh_action();
    let factual_coordination_summary = factual_plane_snapshot.refresh_summary();
    let extraction_brief = factual_plane_snapshot.extraction_brief();
    let factual_refresh_suggested = matches!(
        strongest_action,
        Some(
            SharedFactualReconcileAction::Correct
                | SharedFactualReconcileAction::Conflict
                | SharedFactualReconcileAction::Stale
        )
    ) || (input.external_content_used
        && matches!(
            strongest_action,
            Some(SharedFactualReconcileAction::Reinforce)
        ));
    MemoryGovernanceOutcome {
        factual_plane_snapshot,
        strongest_action,
        factual_coordination_summary,
        extraction_brief,
        factual_refresh_suggested,
    }
}
