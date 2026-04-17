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
pub struct ProgrammableReasoningStageUsageSummary {
    pub stage_name: String,
    pub total_events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningUsageAnalytics {
    pub recent_total_events: usize,
    pub recent_total_attempts: usize,
    pub recent_attention_events: usize,
    pub recent_governance_holds: usize,
    pub recent_succeeded: usize,
    pub recent_failed: usize,
    pub recent_denied: usize,
    pub recent_resource_denied: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tool_name: Option<String>,
    #[serde(default)]
    pub tool_counts: Vec<ProgrammableReasoningToolUsageSummary>,
    #[serde(default)]
    pub stage_counts: Vec<ProgrammableReasoningStageUsageSummary>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningTimelineEvent {
    pub recorded_at: u64,
    pub activity_kind: String,
    pub activity_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub status: String,
    pub detail: String,
    pub attention_required: bool,
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
    pub last_event_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_event_status: Option<String>,
    #[serde(default)]
    pub attention_activities: Vec<String>,
    #[serde(default)]
    pub attention_tools: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningDemoScenario {
    pub scenario_id: String,
    pub title: String,
    pub trigger: String,
    pub expected_outcome: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningProductSurface {
    pub headline: String,
    pub differentiator: String,
    pub operator_pitch: String,
    #[serde(default)]
    pub demo_scenarios: Vec<ProgrammableReasoningDemoScenario>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningDoctrineInspection {
    pub headline: String,
    pub stable_clauses: usize,
    pub revision_pending_clauses: usize,
    #[serde(default)]
    pub highlighted_topics: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningGenomeInspection {
    pub headline: String,
    pub active_lineages: usize,
    pub retired_lineages: usize,
    pub total_diff_events: usize,
    #[serde(default)]
    pub highlighted_skills: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningTensionInspection {
    pub headline: String,
    pub doctrine_revision_pending: usize,
    pub genome_diff_events: usize,
    pub attention_event_count: usize,
    pub arena_clarification_holds: usize,
    #[serde(default)]
    pub dominant_tensions: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningInspectionViews {
    pub doctrine: ProgrammableReasoningDoctrineInspection,
    pub genome: ProgrammableReasoningGenomeInspection,
    pub tension: ProgrammableReasoningTensionInspection,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningBranchReplayRecord {
    pub recorded_at_ms: u64,
    pub channel: String,
    pub chat_id: String,
    pub user_preview: String,
    pub reasoning_strategy: String,
    pub selected_branch: String,
    pub selected_branch_score: u8,
    #[serde(default)]
    pub rejected_branches: Vec<String>,
    pub outcome: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningArenaReplayRecord {
    pub recorded_at_ms: u64,
    pub channel: String,
    pub chat_id: String,
    pub user_preview: String,
    pub subject_kind: String,
    pub disposition: String,
    pub winner: String,
    pub attacker: String,
    pub defender: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningDoctrineReplayRecord {
    pub recorded_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_chat_id: Option<String>,
    pub source_skill_name: String,
    pub topic: String,
    pub status: String,
    pub validated_success_count: u32,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningGenomeReplayRecord {
    pub recorded_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_chat_id: Option<String>,
    pub skill_name: String,
    pub topic: String,
    pub status: String,
    pub lineage_depth: usize,
    pub diff_events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_node_id: Option<String>,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningCapabilityAtomReplayRecord {
    pub recorded_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_chat_id: Option<String>,
    pub atom_name: String,
    pub topic: String,
    pub trust: String,
    pub source_kind: String,
    pub status: String,
    pub summary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningReplayInspection {
    pub branch_replays_retained: usize,
    #[serde(default)]
    pub recent_branch_replays: Vec<ProgrammableReasoningBranchReplayRecord>,
    pub arena_replays_retained: usize,
    #[serde(default)]
    pub recent_arena_replays: Vec<ProgrammableReasoningArenaReplayRecord>,
    pub doctrine_replays_retained: usize,
    #[serde(default)]
    pub recent_doctrine_replays: Vec<ProgrammableReasoningDoctrineReplayRecord>,
    pub genome_replays_retained: usize,
    #[serde(default)]
    pub recent_genome_replays: Vec<ProgrammableReasoningGenomeReplayRecord>,
    pub capability_atom_replays_retained: usize,
    #[serde(default)]
    pub recent_capability_atom_replays: Vec<ProgrammableReasoningCapabilityAtomReplayRecord>,
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
    pub product_surface: ProgrammableReasoningProductSurface,
    pub inspection: ProgrammableReasoningInspectionViews,
    pub replay: ProgrammableReasoningReplayInspection,
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
    pub product_headline: String,
    pub demo_scenario_count: usize,
    pub inspection_ready: bool,
    pub replay_ready: bool,
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

fn programmable_reasoning_product_headline() -> &'static str {
    "beetle programmable reasoning compiles intent into replayable, adjudicated capability growth"
}

fn build_programmable_reasoning_product_surface() -> ProgrammableReasoningProductSurface {
    ProgrammableReasoningProductSurface {
        headline: programmable_reasoning_product_headline().to_string(),
        differentiator: "Not a generic IDE sandbox: Beetle turns governed reasoning into durable device capability.".to_string(),
        operator_pitch: "Intent compiler, branch replay, adversarial arena, doctrine/genome evolution, and capability atoms now share one runtime spine.".to_string(),
        demo_scenarios: vec![
            ProgrammableReasoningDemoScenario {
                scenario_id: "register_debugging".to_string(),
                title: "Register-table debugging".to_string(),
                trigger: "User drops an unfamiliar datasheet or register block.".to_string(),
                expected_outcome:
                    "Beetle distills the table, validates a reusable runtime skill, and promotes a capability atom when the procedure stabilizes.".to_string(),
            },
            ProgrammableReasoningDemoScenario {
                scenario_id: "protocol_triage".to_string(),
                title: "Protocol-frame triage".to_string(),
                trigger: "A byte stream has multiple plausible interpretations.".to_string(),
                expected_outcome:
                    "Beetle compares branches, replays the rejected interpretations, and surfaces why one framing strategy won.".to_string(),
            },
            ProgrammableReasoningDemoScenario {
                scenario_id: "guarded_tooling".to_string(),
                title: "Guarded tooling in a live chat".to_string(),
                trigger: "A user request needs tools but also carries relationship or boundary risk.".to_string(),
                expected_outcome:
                    "Beetle compiles intent, adjudicates the challenger/defender claims, and keeps a replayable rationale instead of rushing the tool round.".to_string(),
            },
        ],
    }
}

fn collect_unique_highlights<I>(items: I, limit: usize) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut highlights = Vec::new();
    for item in items {
        if item.is_empty() || highlights.iter().any(|existing| existing == &item) {
            continue;
        }
        highlights.push(item);
        if highlights.len() >= limit {
            break;
        }
    }
    highlights
}

fn build_programmable_reasoning_doctrine_inspection(
    doctrine: &RuntimeSkillDoctrineSnapshot,
) -> ProgrammableReasoningDoctrineInspection {
    let highlighted_topics = collect_unique_highlights(
        doctrine.recent_clauses.iter().filter_map(|record| {
            let topic = record.topic.trim();
            (!topic.is_empty()).then(|| topic.to_string())
        }),
        6,
    );
    let headline = if doctrine.total_clauses == 0 {
        "No runtime-skill doctrine has stabilized yet.".to_string()
    } else if doctrine.revision_pending_clauses == 0 {
        format!(
            "{} doctrine clauses are stable across {} highlighted topics.",
            doctrine.stable_clauses,
            highlighted_topics.len()
        )
    } else {
        format!(
            "{} stable doctrine clauses with {} pending revision.",
            doctrine.stable_clauses, doctrine.revision_pending_clauses
        )
    };
    ProgrammableReasoningDoctrineInspection {
        headline,
        stable_clauses: doctrine.stable_clauses,
        revision_pending_clauses: doctrine.revision_pending_clauses,
        highlighted_topics,
    }
}

fn build_programmable_reasoning_genome_inspection(
    genome: &RuntimeSkillGenomeSnapshot,
) -> ProgrammableReasoningGenomeInspection {
    let highlighted_skills = collect_unique_highlights(
        genome.recent_lineages.iter().filter_map(|record| {
            let name = record.skill_name.trim();
            (!name.is_empty()).then(|| name.to_string())
        }),
        6,
    );
    let headline = if genome.total_lineages == 0 {
        "No runtime-skill genome lineage has been retained yet.".to_string()
    } else {
        format!(
            "{} active lineages, {} retired, {} recorded strategy diffs.",
            genome.active_lineages, genome.retired_lineages, genome.total_diff_events
        )
    };
    ProgrammableReasoningGenomeInspection {
        headline,
        active_lineages: genome.active_lineages,
        retired_lineages: genome.retired_lineages,
        total_diff_events: genome.total_diff_events,
        highlighted_skills,
    }
}

fn build_programmable_reasoning_tension_inspection(
    doctrine: &RuntimeSkillDoctrineSnapshot,
    genome: &RuntimeSkillGenomeSnapshot,
    adversarial_arena: &AdversarialArenaAuditSnapshot,
    maintenance_digest: &ProgrammableReasoningMaintenanceDigest,
) -> ProgrammableReasoningTensionInspection {
    let mut dominant_tensions = Vec::new();
    if doctrine.revision_pending_clauses > 0 {
        dominant_tensions.push("doctrine_revision".to_string());
    }
    if genome.total_diff_events > 0 {
        dominant_tensions.push("strategy_evolution".to_string());
    }
    if maintenance_digest.attention_event_count > 0 {
        dominant_tensions.push("operator_attention".to_string());
    }
    if adversarial_arena.summary.held_for_clarification > 0 {
        dominant_tensions.push("arena_clarification".to_string());
    }
    let headline = if dominant_tensions.is_empty() {
        "No dominant programmable reasoning tension is currently active.".to_string()
    } else {
        format!(
            "{} programmable reasoning tensions need tracking: {}.",
            dominant_tensions.len(),
            dominant_tensions.join(", ")
        )
    };
    ProgrammableReasoningTensionInspection {
        headline,
        doctrine_revision_pending: doctrine.revision_pending_clauses,
        genome_diff_events: genome.total_diff_events,
        attention_event_count: maintenance_digest.attention_event_count,
        arena_clarification_holds: adversarial_arena.summary.held_for_clarification,
        dominant_tensions,
    }
}

pub fn programmable_reasoning_inspection_views(
    doctrine: &RuntimeSkillDoctrineSnapshot,
    genome: &RuntimeSkillGenomeSnapshot,
    adversarial_arena: &AdversarialArenaAuditSnapshot,
    maintenance_digest: &ProgrammableReasoningMaintenanceDigest,
) -> ProgrammableReasoningInspectionViews {
    ProgrammableReasoningInspectionViews {
        doctrine: build_programmable_reasoning_doctrine_inspection(doctrine),
        genome: build_programmable_reasoning_genome_inspection(genome),
        tension: build_programmable_reasoning_tension_inspection(
            doctrine,
            genome,
            adversarial_arena,
            maintenance_digest,
        ),
    }
}

pub fn summarize_programmable_reasoning_operator(
    snapshot: &ProgrammableReasoningOperatorSnapshot,
) -> String {
    format!(
        "{} | backend={:?} | execution_enabled={} | runtime_skills={} validated={} pending_crystals={} promoted_crystals={} rejected_crystals={} | doctrine_stable={} doctrine_pending={} genome_lineages={} genome_retired={} genome_diffs={} atoms_total={} atoms_local={} atoms_pending={} atoms_adopted={} | demos={} recent_events={} tool_attempts={} governance_holds={} arena_revised={} arena_hold={} attention={} branch_replays={} arena_replays={} doctrine_replays={} genome_replays={} capability_atom_replays={}",
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
        snapshot.product_surface.demo_scenarios.len(),
        snapshot.usage_analytics.recent_total_events,
        snapshot.usage_analytics.recent_total_attempts,
        snapshot.usage_analytics.recent_governance_holds,
        snapshot.adversarial_arena.summary.revised,
        snapshot.adversarial_arena.summary.held_for_clarification,
        snapshot.maintenance_digest.attention_event_count,
        snapshot.replay.branch_replays_retained,
        snapshot.replay.arena_replays_retained,
        snapshot.replay.doctrine_replays_retained,
        snapshot.replay.genome_replays_retained,
        snapshot.replay.capability_atom_replays_retained,
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
    let product_surface = build_programmable_reasoning_product_surface();
    let adversarial_arena = adversarial_arena_snapshot(12);
    let maintenance_digest = ProgrammableReasoningMaintenanceDigest::default();
    let inspection = programmable_reasoning_inspection_views(
        doctrine,
        genome,
        &adversarial_arena,
        &maintenance_digest,
    );
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
        product_surface,
        inspection,
        replay: ProgrammableReasoningReplayInspection::default(),
        usage_analytics: ProgrammableReasoningUsageAnalytics::default(),
        timeline: ProgrammableReasoningTimeline::default(),
        adversarial_arena,
        maintenance_digest,
        operator_summary: String::new(),
    };
    snapshot.operator_summary = summarize_programmable_reasoning_operator(&snapshot);
    snapshot
}

pub fn programmable_reasoning_system_info_summary(
    doctrine: &RuntimeSkillDoctrineSnapshot,
    genome: &RuntimeSkillGenomeSnapshot,
    capability_atoms: &CapabilityAtomOperatorSummary,
    replay: &ProgrammableReasoningReplayInspection,
) -> ProgrammableReasoningSystemInfoSummary {
    let contract = programmable_reasoning_runtime_contract();
    ProgrammableReasoningSystemInfoSummary {
        stage: contract.stage,
        execution_enabled: contract.execution_enabled,
        backend: contract.execution_backend,
        linux_only: contract.linux_only,
        proposal_only_persistence: contract.proposal_only_persistence,
        product_headline: programmable_reasoning_product_headline().to_string(),
        demo_scenario_count: build_programmable_reasoning_product_surface()
            .demo_scenarios
            .len(),
        inspection_ready: doctrine.total_clauses > 0
            || genome.total_lineages > 0
            || capability_atoms.total > 0,
        replay_ready: replay.branch_replays_retained > 0
            || replay.arena_replays_retained > 0
            || replay.doctrine_replays_retained > 0
            || replay.genome_replays_retained > 0
            || replay.capability_atom_replays_retained > 0,
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
        assert_eq!(snapshot.capabilities.len(), 12);
        assert_eq!(snapshot.proposal_kinds.len(), 6);
        assert_eq!(
            snapshot.runtime_contract.execution_enabled,
            cfg!(target_os = "linux")
        );
        assert_eq!(snapshot.usage_analytics.recent_total_attempts, 0);
        assert_eq!(snapshot.usage_analytics.last_event_name, None);
        assert!(snapshot.usage_analytics.tool_counts.is_empty());
        assert!(snapshot.usage_analytics.stage_counts.is_empty());
        assert!(snapshot.timeline.recent_events.is_empty());
        assert_eq!(snapshot.adversarial_arena.summary.total_retained, 0);
        assert!(snapshot.adversarial_arena.recent_events.is_empty());
        assert_eq!(snapshot.doctrine.total_clauses, 0);
        assert!(snapshot.doctrine.recent_clauses.is_empty());
        assert_eq!(snapshot.genome.total_lineages, 0);
        assert!(snapshot.genome.recent_lineages.is_empty());
        assert_eq!(snapshot.capability_atoms.total, 0);
        assert!(snapshot.capability_atoms.recent_records.is_empty());
        assert!(!snapshot.product_surface.headline.is_empty());
        assert_eq!(snapshot.product_surface.demo_scenarios.len(), 3);
        assert!(snapshot
            .product_surface
            .headline
            .contains("programmable reasoning"));
        assert_eq!(snapshot.inspection.doctrine.stable_clauses, 0);
        assert!(snapshot.inspection.doctrine.highlighted_topics.is_empty());
        assert_eq!(snapshot.inspection.genome.active_lineages, 0);
        assert!(snapshot.inspection.genome.highlighted_skills.is_empty());
        assert_eq!(snapshot.inspection.tension.attention_event_count, 0);
        assert!(snapshot.inspection.tension.dominant_tensions.is_empty());
        assert_eq!(snapshot.replay.branch_replays_retained, 0);
        assert!(snapshot.replay.recent_branch_replays.is_empty());
        assert_eq!(snapshot.replay.arena_replays_retained, 0);
        assert!(snapshot.replay.recent_arena_replays.is_empty());
        assert_eq!(snapshot.replay.doctrine_replays_retained, 0);
        assert!(snapshot.replay.recent_doctrine_replays.is_empty());
        assert_eq!(snapshot.replay.genome_replays_retained, 0);
        assert!(snapshot.replay.recent_genome_replays.is_empty());
        assert_eq!(snapshot.replay.capability_atom_replays_retained, 0);
        assert!(snapshot.replay.recent_capability_atom_replays.is_empty());
        assert!(snapshot.maintenance_digest.status.is_empty());
        assert!(snapshot
            .operator_summary
            .contains("capability_atoms_exchange |"));
        assert!(snapshot.operator_summary.contains("recent_events=0"));
        assert!(snapshot.operator_summary.contains("tool_attempts=0"));
        assert!(snapshot.operator_summary.contains("demos=3"));
        assert!(snapshot.operator_summary.contains("branch_replays=0"));
        assert!(snapshot.operator_summary.contains("arena_replays=0"));
        assert!(snapshot.operator_summary.contains("doctrine_replays=0"));
        assert!(snapshot.operator_summary.contains("genome_replays=0"));
        assert!(snapshot
            .operator_summary
            .contains("capability_atom_replays=0"));
    }

    #[test]
    fn system_info_summary_reflects_real_readiness_instead_of_stage_constants() {
        let summary = programmable_reasoning_system_info_summary(
            &RuntimeSkillDoctrineSnapshot::default(),
            &RuntimeSkillGenomeSnapshot::default(),
            &CapabilityAtomOperatorSummary::default(),
            &ProgrammableReasoningReplayInspection::default(),
        );
        assert_eq!(
            summary.stage,
            ProgrammableReasoningStage::CapabilityAtomsExchange
        );
        assert_eq!(summary.execution_enabled, cfg!(target_os = "linux"));
        assert!(summary.linux_only);
        assert!(summary.proposal_only_persistence);
        assert!(!summary.product_headline.is_empty());
        assert_eq!(summary.demo_scenario_count, 3);
        assert!(!summary.inspection_ready);
        assert!(!summary.replay_ready);
    }
}
