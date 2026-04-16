//! Operator-visible programmable reasoning snapshot.

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
use crate::skills::RuntimeSkillOperatorSummary;
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
    pub usage_analytics: ProgrammableReasoningUsageAnalytics,
    pub timeline: ProgrammableReasoningTimeline,
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
    }
}

pub fn summarize_programmable_reasoning_operator(
    snapshot: &ProgrammableReasoningOperatorSnapshot,
) -> String {
    format!(
        "{} | backend={:?} | execution_enabled={} | runtime_skills={} validated={} pending_crystals={} promoted_crystals={} rejected_crystals={} | recent_attempts={} attention={}",
        programmable_reasoning_stage_label(snapshot.stage),
        snapshot.runtime_contract.execution_backend,
        snapshot.runtime_contract.execution_enabled,
        snapshot.experience_crystals.runtime_skill_total,
        snapshot.experience_crystals.validated_runtime_skills,
        snapshot.experience_crystals.pending_candidates,
        snapshot.experience_crystals.promoted_candidates,
        snapshot.experience_crystals.rejected_candidates,
        snapshot.usage_analytics.recent_total_attempts,
        snapshot.maintenance_digest.attention_event_count,
    )
}

pub fn programmable_reasoning_operator_snapshot(
    runtime_skills: &RuntimeSkillOperatorSummary,
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
        usage_analytics: ProgrammableReasoningUsageAnalytics::default(),
        timeline: ProgrammableReasoningTimeline::default(),
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

    #[test]
    fn operator_snapshot_reports_p7_contract() {
        let snapshot =
            programmable_reasoning_operator_snapshot(&RuntimeSkillOperatorSummary::default(), None);
        assert_eq!(
            snapshot.stage,
            ProgrammableReasoningStage::EngineeringSynthesis
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
        assert!(snapshot.maintenance_digest.status.is_empty());
        assert!(snapshot
            .operator_summary
            .contains("engineering_synthesis |"));
        assert!(snapshot.operator_summary.contains("recent_attempts=0"));
    }

    #[test]
    fn system_info_summary_stays_compact() {
        let summary = programmable_reasoning_system_info_summary();
        assert_eq!(
            summary.stage,
            ProgrammableReasoningStage::EngineeringSynthesis
        );
        assert_eq!(summary.execution_enabled, cfg!(target_os = "linux"));
        assert!(summary.linux_only);
        assert!(summary.proposal_only_persistence);
    }
}
