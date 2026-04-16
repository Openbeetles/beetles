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

pub fn programmable_reasoning_operator_snapshot() -> ProgrammableReasoningOperatorSnapshot {
    let runtime_contract = programmable_reasoning_runtime_contract();
    ProgrammableReasoningOperatorSnapshot {
        stage: runtime_contract.stage,
        runtime_contract: runtime_contract.clone(),
        capabilities: programmable_reasoning_capability_taxonomy(),
        proposal_kinds: programmable_reasoning_proposal_kinds(),
        experience_crystals: build_experience_crystal_operator_summary(
            &RuntimeSkillOperatorSummary::default(),
            None,
        ),
        usage_analytics: ProgrammableReasoningUsageAnalytics::default(),
        timeline: ProgrammableReasoningTimeline::default(),
        maintenance_digest: ProgrammableReasoningMaintenanceDigest::default(),
        operator_summary: "engineering_synthesis: programmable reasoning can now distill engineering references into structured datasheet assets without adding a second execution plane".to_string(),
    }
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
        let snapshot = programmable_reasoning_operator_snapshot();
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
