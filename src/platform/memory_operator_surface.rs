//! Shared memory/operator surface for HTTP and CLI.

use crate::memory::{
    board_subject_scope_id, derive_personality_runtime_governance_gate_from_inspection,
    inspect_personality_governance, select_active_continuity_snapshot_chat_ids,
    select_personality_governance_targets, ContinuitySnapshotManifest, CrossPlaneRerankResult,
    IntelligenceReplayInspection, PersonalityGovernanceInspection,
    PersonalityGovernanceInspectionInput, PersonalityRuntimeGovernanceGate, PromptRecallIntent,
    RecallSelectionReport,
};
use crate::skills::is_runtime_skill_name;
use crate::tools::ToolRegistry;
use crate::Platform;
use serde::Serialize;

const OPERATOR_SURFACE_ACTIVE_WINDOW_SECS: u64 = 7 * 86_400;
const OPERATOR_SURFACE_TARGET_LIMIT: usize = 4;

#[derive(Clone, Debug)]
pub struct MemoryOperatorTraceInput {
    pub inspection_target: MemoryOperatorInspectionTarget,
    pub snapshot_manifest: ContinuitySnapshotManifest,
    pub intelligence_replay: IntelligenceReplayInspection,
    pub recall: MemoryOperatorRecallTrace,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryOperatorInspectionTarget {
    pub chat_id: String,
    pub channel: String,
    pub query: String,
    pub memory_system_kind: String,
    pub run_id: Option<String>,
    pub message_count: usize,
    pub recent_message_count: usize,
    pub summary_present: bool,
    pub summary_message_count: usize,
    pub execution_state_present: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryOperatorRecallTrace {
    pub prompt_recall_intent: PromptRecallIntent,
    pub shared_factual_report: RecallSelectionReport,
    pub continuity_capsule_report: RecallSelectionReport,
    pub runtime_skill_report: RecallSelectionReport,
    pub archive_recall_report: RecallSelectionReport,
    pub task_recall_report: Option<RecallSelectionReport>,
    pub cross_plane_rerank: CrossPlaneRerankResult,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorSurfaceSummary {
    pub program_memory_view: MemoryOperatorProgramMemoryView,
    pub soul_governance_view: MemoryOperatorSoulGovernanceView,
    pub inspect: MemoryOperatorInspectView,
    pub trace: MemoryOperatorTraceView,
    pub diff: MemoryOperatorDiffView,
    pub repair: MemoryOperatorRepairView,
    pub forge: MemoryOperatorForgeView,
    pub policy_view: MemoryOperatorPolicyView,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorProgramMemoryView {
    pub memory_system_kind: String,
    pub runtime_skill_count: usize,
    pub long_term_count: usize,
    pub continuity_capsule_count: usize,
    pub continuity_snapshot_supported: bool,
    pub saved_snapshot_count: usize,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorSoulGovernanceView {
    pub board_revision: u64,
    pub board_review_due: bool,
    pub board_conservative_mode: bool,
    pub recent_persona_evidence_updated_at: u64,
    pub recent_persona_execution_signal_count: usize,
    pub recent_persona_promotable_signal_count: usize,
    pub recent_persona_operational_signal_count: usize,
    pub latest_turn_reply_feedback_applied: bool,
    pub latest_turn_initiative_feedback_applied: bool,
    pub latest_turn_strategy_feedback_applied: bool,
    pub latest_turn_strategy_post_reply_enqueued: bool,
    pub latest_turn_reply_summary: String,
    pub latest_turn_initiative_summary: String,
    pub latest_turn_strategy_summary: String,
    pub self_model_updated_at: u64,
    pub self_authored_core_updated_at: u64,
    pub self_continuity_updated_at: u64,
    pub relationship_constitution_updated_at: u64,
    pub relationship_needs_runtime_attention: bool,
    pub runtime_governance_repair_needed: bool,
    pub runtime_governance_primary_action: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorInspectView {
    pub subject_id: String,
    pub memory_system_kind: String,
    pub runtime_skill_count: usize,
    pub long_term_count: usize,
    pub continuity_capsule_count: usize,
    pub continuity_snapshot_supported: bool,
    pub saved_snapshot_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_relationship_target: Option<MemoryOperatorRelationshipTarget>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorTraceView {
    pub targeted_trace: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_chat_targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection_target: Option<MemoryOperatorInspectionTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_manifest: Option<ContinuitySnapshotManifest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intelligence_replay: Option<IntelligenceReplayInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recall: Option<MemoryOperatorRecallTrace>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorDiffView {
    pub board_revision: u64,
    pub board_review_due: bool,
    pub board_conservative_mode: bool,
    pub board_observation_active: bool,
    pub self_model_updated_at: u64,
    pub self_authored_core_updated_at: u64,
    pub self_continuity_updated_at: u64,
    pub relationship_constitution_updated_at: u64,
    pub relationship_runtime_refresh_at: u64,
    pub relationship_outer_voice_updated_at: u64,
    pub relationship_boundary_persona_updated_at: u64,
    pub relationship_world_sense_updated_at: u64,
    pub relationship_persona_turn_at: u64,
    pub relationship_needs_runtime_attention: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift_flags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorRepairView {
    pub repair_needed: bool,
    pub primary_action: String,
    pub continuity_snapshot_supported: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub continuity_snapshot_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub personality_governance_targets: Vec<MemoryOperatorRelationshipTarget>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorPolicyView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<MemoryOperatorRelationshipTarget>,
    pub personality_governance: PersonalityGovernanceInspection,
    pub runtime_governance_gate: PersonalityRuntimeGovernanceGate,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorForgeView {
    pub last_run_at: u64,
    pub total_candidates: usize,
    pub attack_findings: usize,
    pub distillation_candidates: usize,
    pub adjudication_state: crate::IdleMemoryForgeAdjudicationState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_source_channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_finding: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryOperatorRelationshipTarget {
    pub scope_id: String,
    pub channel: String,
    pub chat_id: String,
    pub score: i32,
    pub reason: String,
}

type MemoryOperatorPolicyViewBundle = (
    Option<MemoryOperatorRelationshipTarget>,
    PersonalityGovernanceInspection,
    PersonalityRuntimeGovernanceGate,
    Option<crate::memory::RelationshipConstitution>,
    Option<crate::memory::RecentPersonaEvidence>,
    Option<crate::memory::TurnSoulFeedbackLedger>,
);

pub fn build_memory_operator_surface(
    platform: &dyn Platform,
    tool_registry: Option<&ToolRegistry>,
    trace_input: Option<&MemoryOperatorTraceInput>,
) -> crate::error::Result<MemoryOperatorSurfaceSummary> {
    build_memory_operator_surface_with_capabilities(
        platform,
        tool_registry.is_some_and(|registry| registry.get("continuity_snapshot").is_some()),
        trace_input,
    )
}

pub fn build_memory_operator_surface_with_capabilities(
    platform: &dyn Platform,
    continuity_snapshot_supported: bool,
    trace_input: Option<&MemoryOperatorTraceInput>,
) -> crate::error::Result<MemoryOperatorSurfaceSummary> {
    let subject_id = board_subject_scope_id();
    let now_secs = crate::util::current_unix_secs();
    let memory_system_kind = platform.memory_system_kind();
    let skill_storage = platform.skill_storage();
    let runtime_skill_count = skill_storage
        .list_names()
        .map(|names| {
            names
                .into_iter()
                .filter(|name| is_runtime_skill_name(name))
                .count()
        })
        .unwrap_or_default();
    let long_term_count = platform
        .long_term_memory_store()
        .count()
        .unwrap_or_default();
    let continuity_capsule_count = platform
        .continuity_capsule_store()
        .count()
        .unwrap_or_default();
    let saved_snapshot_count = platform
        .state_fs()
        .list_dir("memory/continuity_snapshots/manual")
        .unwrap_or_default()
        .into_iter()
        .filter(|name| name.ends_with(".json"))
        .count();
    let self_model = platform.self_model_store().get(subject_id)?;
    let self_authored_core = platform.self_authored_core_store().get(subject_id)?;
    let self_continuity = platform.self_continuity_store().get(subject_id)?;
    let relationship_portfolio = platform.relationship_portfolio_store().get(subject_id)?;
    let relationship_topology = platform.relationship_topology_store().get(subject_id)?;
    let personality_targets = select_personality_governance_targets(
        self_continuity.as_ref(),
        relationship_portfolio.as_ref(),
        relationship_topology.as_ref(),
        now_secs,
        OPERATOR_SURFACE_TARGET_LIMIT,
    );
    let active_relationship_target = personality_targets.first().map(convert_relationship_target);
    let active_chat_targets = select_active_continuity_snapshot_chat_ids(
        platform.session_store().as_ref(),
        platform.self_continuity_store().as_ref(),
        platform.relationship_portfolio_store().as_ref(),
        platform.relationship_topology_store().as_ref(),
        trace_input.map(|trace| trace.inspection_target.chat_id.as_str()),
        now_secs,
        OPERATOR_SURFACE_ACTIVE_WINDOW_SECS,
        OPERATOR_SURFACE_TARGET_LIMIT,
    );
    let (
        policy_target,
        personality_governance,
        runtime_governance_gate,
        relationship_constitution,
        recent_persona_evidence,
        latest_turn_soul_feedback,
    ) = build_policy_view(platform, now_secs, personality_targets.first())?;
    let forge_summary =
        crate::load_idle_memory_forge_operator_summary(platform.state_fs().as_ref())
            .ok()
            .flatten()
            .unwrap_or_default();
    let topology_entry = relationship_topology.as_ref().and_then(|topology| {
        policy_target.as_ref().and_then(|target| {
            topology
                .entries
                .iter()
                .find(|entry| entry.scope_id == target.scope_id)
        })
    });
    let outer_voice_updated_at = policy_target
        .as_ref()
        .and_then(|target| {
            platform
                .outer_voice_store()
                .get(target.scope_id.as_str())
                .ok()
        })
        .flatten()
        .map(|outer_voice| outer_voice.updated_at)
        .unwrap_or_else(|| {
            topology_entry
                .map(|entry| entry.last_outer_voice_at)
                .unwrap_or_default()
        });
    let boundary_persona_updated_at = policy_target
        .as_ref()
        .and_then(|target| {
            platform
                .mental_privacy_store()
                .get(target.scope_id.as_str())
                .ok()
        })
        .flatten()
        .map(|state| state.updated_at.max(state.boundary_persona.updated_at))
        .unwrap_or_else(|| {
            topology_entry
                .map(|entry| entry.last_mental_privacy_at)
                .unwrap_or_default()
        });

    Ok(MemoryOperatorSurfaceSummary {
        program_memory_view: MemoryOperatorProgramMemoryView {
            memory_system_kind: memory_system_kind.as_str().to_string(),
            runtime_skill_count,
            long_term_count,
            continuity_capsule_count,
            continuity_snapshot_supported,
            saved_snapshot_count,
        },
        soul_governance_view: MemoryOperatorSoulGovernanceView {
            board_revision: self_authored_core
                .as_ref()
                .map(|core| core.revision)
                .unwrap_or(0),
            board_review_due: personality_governance.core_revision_governance.review_due,
            board_conservative_mode: personality_governance
                .core_revision_governance
                .conservative_mode,
            recent_persona_evidence_updated_at: recent_persona_evidence
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            recent_persona_execution_signal_count: recent_persona_evidence
                .as_ref()
                .map(|value| value.execution_continuity_signal_count())
                .unwrap_or(0),
            recent_persona_promotable_signal_count: recent_persona_evidence
                .as_ref()
                .map(|value| value.promotable_growth_signal_count())
                .unwrap_or(0),
            recent_persona_operational_signal_count: recent_persona_evidence
                .as_ref()
                .map(|value| value.operational_trace_signal_count())
                .unwrap_or(0),
            latest_turn_reply_feedback_applied: latest_turn_soul_feedback
                .as_ref()
                .is_some_and(|value| value.reply.applied),
            latest_turn_initiative_feedback_applied: latest_turn_soul_feedback
                .as_ref()
                .is_some_and(|value| value.initiative.applied),
            latest_turn_strategy_feedback_applied: latest_turn_soul_feedback
                .as_ref()
                .is_some_and(|value| value.strategy.applied),
            latest_turn_strategy_post_reply_enqueued: latest_turn_soul_feedback
                .as_ref()
                .is_some_and(|value| value.strategy.post_reply_self_runtime_enqueued),
            latest_turn_reply_summary: latest_turn_soul_feedback
                .as_ref()
                .map(|value| value.reply.summary.clone())
                .unwrap_or_default(),
            latest_turn_initiative_summary: latest_turn_soul_feedback
                .as_ref()
                .map(|value| value.initiative.summary.clone())
                .unwrap_or_default(),
            latest_turn_strategy_summary: latest_turn_soul_feedback
                .as_ref()
                .map(|value| value.strategy.summary.clone())
                .unwrap_or_default(),
            self_model_updated_at: self_model
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            self_authored_core_updated_at: self_authored_core
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            self_continuity_updated_at: self_continuity
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            relationship_constitution_updated_at: relationship_constitution
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            relationship_needs_runtime_attention: topology_entry
                .is_some_and(|entry| entry.needs_runtime_attention()),
            runtime_governance_repair_needed: personality_governance.repair_plan.repair_needed,
            runtime_governance_primary_action: personality_governance
                .repair_plan
                .primary_action
                .label()
                .to_string(),
        },
        inspect: MemoryOperatorInspectView {
            subject_id: subject_id.to_string(),
            memory_system_kind: memory_system_kind.as_str().to_string(),
            runtime_skill_count,
            long_term_count,
            continuity_capsule_count,
            continuity_snapshot_supported,
            saved_snapshot_count,
            active_relationship_target: active_relationship_target.clone(),
        },
        trace: MemoryOperatorTraceView {
            targeted_trace: trace_input.is_some(),
            active_chat_targets,
            inspection_target: trace_input.map(|trace| trace.inspection_target.clone()),
            snapshot_manifest: trace_input.map(|trace| trace.snapshot_manifest.clone()),
            intelligence_replay: trace_input.map(|trace| trace.intelligence_replay.clone()),
            recall: trace_input.map(|trace| trace.recall.clone()),
        },
        diff: MemoryOperatorDiffView {
            board_revision: self_authored_core
                .as_ref()
                .map(|core| core.revision)
                .unwrap_or(0),
            board_review_due: personality_governance.core_revision_governance.review_due,
            board_conservative_mode: personality_governance
                .core_revision_governance
                .conservative_mode,
            board_observation_active: personality_governance
                .core_revision_governance
                .observation_active,
            self_model_updated_at: self_model
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            self_authored_core_updated_at: self_authored_core
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            self_continuity_updated_at: self_continuity
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            relationship_constitution_updated_at: relationship_constitution
                .as_ref()
                .map(|value| value.updated_at)
                .unwrap_or(0),
            relationship_runtime_refresh_at: topology_entry
                .map(|entry| entry.last_runtime_refresh_at)
                .unwrap_or(0),
            relationship_outer_voice_updated_at: outer_voice_updated_at,
            relationship_boundary_persona_updated_at: boundary_persona_updated_at,
            relationship_world_sense_updated_at: topology_entry
                .map(|entry| entry.last_world_sense_at)
                .unwrap_or(0),
            relationship_persona_turn_at: topology_entry
                .map(|entry| entry.last_persona_turn_at)
                .unwrap_or(0),
            relationship_needs_runtime_attention: topology_entry
                .is_some_and(|entry| entry.needs_runtime_attention()),
            drift_flags: personality_governance
                .relationship_audit
                .as_ref()
                .map(|audit| audit.drift_flags.clone())
                .unwrap_or_default(),
            outstanding: personality_governance.closure.outstanding.clone(),
        },
        repair: MemoryOperatorRepairView {
            repair_needed: personality_governance.repair_plan.repair_needed,
            primary_action: personality_governance
                .repair_plan
                .primary_action
                .label()
                .to_string(),
            continuity_snapshot_supported,
            reasons: personality_governance.repair_plan.reasons.clone(),
            continuity_snapshot_targets: select_active_continuity_snapshot_chat_ids(
                platform.session_store().as_ref(),
                platform.self_continuity_store().as_ref(),
                platform.relationship_portfolio_store().as_ref(),
                platform.relationship_topology_store().as_ref(),
                policy_target.as_ref().map(|target| target.chat_id.as_str()),
                now_secs,
                OPERATOR_SURFACE_ACTIVE_WINDOW_SECS,
                OPERATOR_SURFACE_TARGET_LIMIT,
            ),
            personality_governance_targets: personality_targets
                .iter()
                .map(convert_relationship_target)
                .collect(),
        },
        forge: MemoryOperatorForgeView {
            last_run_at: forge_summary.last_run_at,
            total_candidates: forge_summary.total_candidates,
            attack_findings: forge_summary.attack_findings,
            distillation_candidates: forge_summary.distillation_candidates,
            adjudication_state: forge_summary.adjudication_state,
            last_chat_id: forge_summary.last_chat_id,
            last_source_channel: forge_summary.last_source_channel,
            primary_finding: forge_summary.primary_finding,
        },
        policy_view: MemoryOperatorPolicyView {
            target: policy_target,
            personality_governance,
            runtime_governance_gate,
        },
    })
}

pub fn render_memory_operator_surface_text(surface: &MemoryOperatorSurfaceSummary) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "  memory_operator_program_memory: kind={} runtime_skills={} long_term={} continuity_capsules={} snapshots_supported={} saved_snapshots={}\n",
        surface.program_memory_view.memory_system_kind,
        surface.program_memory_view.runtime_skill_count,
        surface.program_memory_view.long_term_count,
        surface.program_memory_view.continuity_capsule_count,
        surface.program_memory_view.continuity_snapshot_supported,
        surface.program_memory_view.saved_snapshot_count,
    ));
    out.push_str(&format!(
        "  memory_operator_soul_governance: board_revision={} review_due={} conservative_mode={} recent_persona_at={} execution_signals={} promotable_signals={} operational_signals={} latest_reply_feedback={} latest_initiative_feedback={} latest_strategy_feedback={} latest_post_reply_runtime={} relationship_attention={} repair_needed={} primary_action={}\n",
        surface.soul_governance_view.board_revision,
        surface.soul_governance_view.board_review_due,
        surface.soul_governance_view.board_conservative_mode,
        surface.soul_governance_view.recent_persona_evidence_updated_at,
        surface.soul_governance_view.recent_persona_execution_signal_count,
        surface.soul_governance_view.recent_persona_promotable_signal_count,
        surface.soul_governance_view.recent_persona_operational_signal_count,
        surface.soul_governance_view.latest_turn_reply_feedback_applied,
        surface.soul_governance_view.latest_turn_initiative_feedback_applied,
        surface.soul_governance_view.latest_turn_strategy_feedback_applied,
        surface.soul_governance_view.latest_turn_strategy_post_reply_enqueued,
        surface.soul_governance_view.relationship_needs_runtime_attention,
        surface.soul_governance_view.runtime_governance_repair_needed,
        surface.soul_governance_view.runtime_governance_primary_action,
    ));
    if let Some(target) = surface.inspect.active_relationship_target.as_ref() {
        out.push_str(&format!(
            "  memory_operator_active_relation: {}:{} ({})\n",
            target.channel, target.chat_id, target.reason
        ));
    }
    out.push_str(&format!(
        "  memory_operator_repair_needed: {}\n  memory_operator_primary_action: {}\n",
        surface.repair.repair_needed, surface.repair.primary_action
    ));
    if !surface.diff.outstanding.is_empty() {
        out.push_str(&format!(
            "  memory_operator_outstanding: {}\n",
            surface.diff.outstanding.join(", ")
        ));
    }
    if !surface.trace.active_chat_targets.is_empty() {
        out.push_str(&format!(
            "  memory_operator_trace_targets: {}\n",
            surface.trace.active_chat_targets.join(", ")
        ));
    }
    out.push_str(&format!(
        "  memory_operator_forge_last_run_at: {}\n  memory_operator_forge_candidates: {}\n  memory_operator_forge_attack_findings: {}\n  memory_operator_forge_distillation_candidates: {}\n  memory_operator_forge_adjudication: {:?}\n",
        surface.forge.last_run_at,
        surface.forge.total_candidates,
        surface.forge.attack_findings,
        surface.forge.distillation_candidates,
        surface.forge.adjudication_state,
    ));
    if let Some(finding) = surface.forge.primary_finding.as_deref() {
        out.push_str(&format!("  memory_operator_forge_finding: {}\n", finding));
    }
    out
}

fn build_policy_view(
    platform: &dyn Platform,
    now_secs: u64,
    target: Option<&crate::memory::RelationshipSelectionTarget>,
) -> crate::error::Result<MemoryOperatorPolicyViewBundle> {
    let Some(target) = target else {
        let inspection = PersonalityGovernanceInspection::default();
        let gate = derive_personality_runtime_governance_gate_from_inspection(&inspection);
        return Ok((None, inspection, gate, None, None, None));
    };
    let self_authored_core = platform
        .self_authored_core_store()
        .get(board_subject_scope_id())?;
    let core_revision_ledger = platform
        .core_revision_ledger_store()
        .get(board_subject_scope_id())?;
    let relationship_constitution = platform
        .relationship_constitution_store()
        .get(target.scope_id.as_str())?;
    let relationship_topology = platform
        .relationship_topology_store()
        .get(board_subject_scope_id())?;
    let recent_persona_evidence = platform
        .turn_ledger_store()
        .recent_persona_evidence(target.chat_id.as_str())?;
    let latest_turn_soul_feedback = platform
        .turn_ledger_store()
        .get(target.scope_id.as_str())?
        .and_then(|ledger| ledger.soul_feedback);
    let inspection = inspect_personality_governance(PersonalityGovernanceInspectionInput {
        channel: target.channel.as_str(),
        chat_id: target.chat_id.as_str(),
        now_secs,
        self_authored_core: self_authored_core.as_ref(),
        core_revision_ledger: core_revision_ledger.as_ref(),
        relationship_constitution: relationship_constitution.as_ref(),
        relationship_topology: relationship_topology.as_ref(),
        recent_persona_evidence: recent_persona_evidence.as_ref(),
    });
    let gate = derive_personality_runtime_governance_gate_from_inspection(&inspection);
    Ok((
        Some(convert_relationship_target(target)),
        inspection,
        gate,
        relationship_constitution,
        recent_persona_evidence,
        latest_turn_soul_feedback,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_memory_operator_surface_text_exposes_split_program_and_soul_views() {
        let text = render_memory_operator_surface_text(&MemoryOperatorSurfaceSummary {
            program_memory_view: MemoryOperatorProgramMemoryView {
                memory_system_kind: "linux_full".to_string(),
                runtime_skill_count: 3,
                long_term_count: 8,
                continuity_capsule_count: 5,
                continuity_snapshot_supported: true,
                saved_snapshot_count: 2,
            },
            soul_governance_view: MemoryOperatorSoulGovernanceView {
                board_revision: 7,
                board_review_due: true,
                board_conservative_mode: true,
                recent_persona_evidence_updated_at: 88,
                recent_persona_execution_signal_count: 5,
                recent_persona_promotable_signal_count: 2,
                recent_persona_operational_signal_count: 3,
                latest_turn_reply_feedback_applied: true,
                latest_turn_initiative_feedback_applied: true,
                latest_turn_strategy_feedback_applied: true,
                latest_turn_strategy_post_reply_enqueued: true,
                relationship_needs_runtime_attention: true,
                runtime_governance_repair_needed: true,
                runtime_governance_primary_action: "repair_self_authored_core".to_string(),
                ..MemoryOperatorSoulGovernanceView::default()
            },
            ..MemoryOperatorSurfaceSummary::default()
        });

        assert!(text.contains("memory_operator_program_memory: kind=linux_full"));
        assert!(text.contains("runtime_skills=3"));
        assert!(text.contains("memory_operator_soul_governance: board_revision=7"));
        assert!(text.contains("execution_signals=5"));
        assert!(text.contains("promotable_signals=2"));
        assert!(text.contains("latest_reply_feedback=true"));
        assert!(text.contains("latest_initiative_feedback=true"));
        assert!(text.contains("latest_strategy_feedback=true"));
        assert!(text.contains("latest_post_reply_runtime=true"));
        assert!(text.contains("primary_action=repair_self_authored_core"));
    }
}

fn convert_relationship_target(
    target: &crate::memory::RelationshipSelectionTarget,
) -> MemoryOperatorRelationshipTarget {
    MemoryOperatorRelationshipTarget {
        scope_id: target.scope_id.clone(),
        channel: target.channel.clone(),
        chat_id: target.chat_id.clone(),
        score: target.score,
        reason: target.reason.clone(),
    }
}
