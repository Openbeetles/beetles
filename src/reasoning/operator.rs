//! Operator-visible programmable reasoning snapshot.

use super::adversarial_arena::{adversarial_arena_snapshot, AdversarialArenaAuditSnapshot};
use crate::reasoning::constitution::{
    programmable_reasoning_capability_taxonomy, programmable_reasoning_runtime_contract,
    ProgrammableReasoningCapabilityContract, ProgrammableReasoningRuntimeContract,
    ProgrammableReasoningStage,
};
use crate::reasoning::experience_crystal::{
    build_experience_crystal_operator_summary, ExperienceCrystalOperatorSummary,
};
use crate::reasoning::proposal::{
    programmable_reasoning_proposal_kinds, ProgrammableReasoningProposalKind,
};
use crate::skills::{
    CapabilityAtomOperatorSummary, RuntimeSkillDoctrineSnapshot, RuntimeSkillGenomeSnapshot,
    RuntimeSkillOperatorSummary,
};
use crate::task_execution::TaskLearningOperatorSnapshot;
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningToolUsageSummary {
    pub tool_name: String,
    pub total_attempts: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub denied: usize,
    pub resource_denied: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningUsageAnalytics {
    pub recent_total_attempts: usize,
    pub recent_succeeded: usize,
    pub recent_failed: usize,
    pub recent_denied: usize,
    pub recent_resource_denied: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
    #[serde(default)]
    pub tool_counts: Vec<ProgrammableReasoningToolUsageSummary>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningTimelineEvent {
    pub recorded_at: u64,
    pub tool_name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningTimeline {
    #[serde(default)]
    pub recent_events: Vec<ProgrammableReasoningTimelineEvent>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningMaintenanceDigest {
    pub status: String,
    pub headline: String,
    pub attention_event_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_status: Option<String>,
    #[serde(default)]
    pub attention_tools: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningOperatorSnapshot {
    pub stage: ProgrammableReasoningStage,
    pub runtime_contract: ProgrammableReasoningRuntimeContract,
    pub capabilities: Vec<ProgrammableReasoningCapabilityContract>,
    pub proposal_kinds: Vec<ProgrammableReasoningProposalKind>,
    pub experience_crystals: ExperienceCrystalOperatorSummary,
    pub doctrine: RuntimeSkillDoctrineSnapshot,
    pub genome: RuntimeSkillGenomeSnapshot,
    pub capability_atoms: CapabilityAtomOperatorSummary,
    pub usage_analytics: ProgrammableReasoningUsageAnalytics,
    pub timeline: ProgrammableReasoningTimeline,
    pub adversarial_arena: AdversarialArenaAuditSnapshot,
    pub maintenance_digest: ProgrammableReasoningMaintenanceDigest,
    pub operator_summary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningSystemInfoSummary {
    pub stage: ProgrammableReasoningStage,
    pub execution_enabled: bool,
    pub backend: crate::reasoning::constitution::ProgrammableReasoningExecutionBackend,
    pub linux_only: bool,
    pub proposal_only_persistence: bool,
}

fn programmable_reasoning_stage_label(stage: ProgrammableReasoningStage) -> &'static str {
    match stage {
        ProgrammableReasoningStage::ConstitutionOnly => "constitution_only",
        ProgrammableReasoningStage::TaskScriptingBaseline => "task_scripting_baseline",
        ProgrammableReasoningStage::MemoryQueryPlane => "memory_query_plane",
        ProgrammableReasoningStage::IdleMemoryForge => "idle_memory_forge",
        ProgrammableReasoningStage::MemoryAttackDistillation => "memory_attack_distillation",
        ProgrammableReasoningStage::CapabilityBridgeExpansion => "capability_bridge_expansion",
        ProgrammableReasoningStage::ExperienceCrystal => "experience_crystal",
        ProgrammableReasoningStage::EngineeringSynthesis => "engineering_synthesis",
        ProgrammableReasoningStage::IntentCompiler => "intent_compiler",
        ProgrammableReasoningStage::CounterfactualSandbox => "counterfactual_sandbox",
        ProgrammableReasoningStage::AdversarialArena => "adversarial_arena",
        ProgrammableReasoningStage::DoctrineGenomeEvolution => "doctrine_genome_evolution",
        ProgrammableReasoningStage::CapabilityAtomsExchange => "capability_atoms_exchange",
    }
}

pub fn summarize_programmable_reasoning_operator(
    snapshot: &ProgrammableReasoningOperatorSnapshot,
) -> String {
    format!(
        "{} | backend={:?} | execution_enabled={} | runtime_skills={} validated={} pending_crystals={} promoted_crystals={} rejected_crystals={} | doctrine_stable={} doctrine_pending={} genome_lineages={} genome_retired={} genome_diffs={} atoms_total={} atoms_local={} atoms_pending={} atoms_adopted={} | recent_attempts={} arena_revised={} arena_hold={} attention={}",
        programmable_reasoning_stage_label(snapshot.stage),
        snapshot.runtime_contract.execution_backend,
        snapshot.runtime_contract.execution_enabled,
        snapshot.experience_crystals.runtime_skill_total,
        snapshot.experience_crystals.validated_runtime_skills,
        snapshot.experience_crystals.pending_candidates,
        snapshot.experience_crystals.promoted_candidates,
        snapshot.experience_crystals.rejected_candidates,
        snapshot.doctrine.stable_clauses,
        snapshot.doctrine.revision_pending_clauses,
        snapshot.genome.total_lineages,
        snapshot.genome.retired_lineages,
        snapshot.genome.total_diff_events,
        snapshot.capability_atoms.total,
        snapshot.capability_atoms.local_verified,
        snapshot.capability_atoms.imported_pending_adjudication,
        snapshot.capability_atoms.imported_adopted,
        snapshot.usage_analytics.recent_total_attempts,
        snapshot.adversarial_arena.summary.revised,
        snapshot.adversarial_arena.summary.held_for_clarification,
        snapshot.maintenance_digest.attention_event_count,
    )
}

pub fn programmable_reasoning_operator_snapshot(
    runtime_skills: &RuntimeSkillOperatorSummary,
    doctrine: &RuntimeSkillDoctrineSnapshot,
    genome: &RuntimeSkillGenomeSnapshot,
    capability_atoms: &CapabilityAtomOperatorSummary,
    task_learning: Option<&TaskLearningOperatorSnapshot>,
) -> ProgrammableReasoningOperatorSnapshot {
    let runtime_contract = programmable_reasoning_runtime_contract();
    let mut snapshot = ProgrammableReasoningOperatorSnapshot {
        stage: runtime_contract.stage,
        runtime_contract: runtime_contract.clone(),
        capabilities: programmable_reasoning_capability_taxonomy(),
        proposal_kinds: programmable_reasoning_proposal_kinds(),
        experience_crystals: build_experience_crystal_operator_summary(
            runtime_skills,
            task_learning,
        ),
        doctrine: doctrine.clone(),
        genome: genome.clone(),
        capability_atoms: capability_atoms.clone(),
        usage_analytics: ProgrammableReasoningUsageAnalytics::default(),
        timeline: ProgrammableReasoningTimeline::default(),
        adversarial_arena: adversarial_arena_snapshot(12),
        maintenance_digest: ProgrammableReasoningMaintenanceDigest::default(),
        operator_summary: String::new(),
    };
    snapshot.operator_summary = summarize_programmable_reasoning_operator(&snapshot);
    snapshot
}

pub fn programmable_reasoning_system_info_summary() -> ProgrammableReasoningSystemInfoSummary {
    let contract = programmable_reasoning_runtime_contract();
    ProgrammableReasoningSystemInfoSummary {
        stage: contract.stage,
        execution_enabled: contract.execution_enabled,
        backend: contract.execution_backend,
        linux_only: contract.linux_only,
        proposal_only_persistence: contract.proposal_only_persistence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning::adversarial_arena::{
        adversarial_arena_test_guard, reset_adversarial_arena_for_tests,
    };

    #[test]
    fn operator_snapshot_reports_p13_contract() {
        let _guard = adversarial_arena_test_guard();
        reset_adversarial_arena_for_tests();
        let snapshot = programmable_reasoning_operator_snapshot(
            &RuntimeSkillOperatorSummary::default(),
            &RuntimeSkillDoctrineSnapshot::default(),
            &RuntimeSkillGenomeSnapshot::default(),
            &CapabilityAtomOperatorSummary::default(),
            None,
        );
        assert_eq!(
            snapshot.stage,
            ProgrammableReasoningStage::CapabilityAtomsExchange
        );
        assert_eq!(snapshot.capabilities.len(), 6);
        assert_eq!(snapshot.proposal_kinds.len(), 5);
        assert_eq!(
            snapshot.runtime_contract.execution_enabled,
            cfg!(target_os = "linux")
        );
        assert_eq!(snapshot.usage_analytics.recent_total_attempts, 0);
        assert!(snapshot.usage_analytics.tool_counts.is_empty());
        assert!(snapshot.timeline.recent_events.is_empty());
        assert_eq!(snapshot.adversarial_arena.summary.total_retained, 0);
        assert!(snapshot.adversarial_arena.recent_events.is_empty());
        assert_eq!(snapshot.doctrine.total_clauses, 0);
        assert!(snapshot.doctrine.recent_clauses.is_empty());
        assert_eq!(snapshot.genome.total_lineages, 0);
        assert!(snapshot.genome.recent_lineages.is_empty());
        assert_eq!(snapshot.capability_atoms.total, 0);
        assert!(snapshot.capability_atoms.recent_records.is_empty());
        assert!(snapshot.maintenance_digest.status.is_empty());
        assert!(snapshot
            .operator_summary
            .contains("capability_atoms_exchange |"));
        assert!(snapshot.operator_summary.contains("recent_attempts=0"));
    }

    #[test]
    fn system_info_summary_stays_compact() {
        let summary = programmable_reasoning_system_info_summary();
        assert_eq!(
            summary.stage,
            ProgrammableReasoningStage::CapabilityAtomsExchange
        );
        assert_eq!(summary.execution_enabled, cfg!(target_os = "linux"));
        assert!(summary.linux_only);
        assert!(summary.proposal_only_persistence);
    }
}
