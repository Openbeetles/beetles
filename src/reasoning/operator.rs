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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningOperatorSnapshot {
    pub stage: ProgrammableReasoningStage,
    pub runtime_contract: ProgrammableReasoningRuntimeContract,
    pub capabilities: Vec<ProgrammableReasoningCapabilityContract>,
    pub proposal_kinds: Vec<ProgrammableReasoningProposalKind>,
    pub experience_crystals: ExperienceCrystalOperatorSummary,
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
        ),
        operator_summary: "experience_crystal: programmable reasoning can now validate adjudication-required skill crystal candidates and promote them into the governed runtime-skill path without adding a second execution plane".to_string(),
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
    fn operator_snapshot_reports_p6_contract() {
        let snapshot = programmable_reasoning_operator_snapshot();
        assert_eq!(
            snapshot.stage,
            ProgrammableReasoningStage::ExperienceCrystal
        );
        assert_eq!(snapshot.capabilities.len(), 6);
        assert_eq!(snapshot.proposal_kinds.len(), 5);
        assert_eq!(
            snapshot.runtime_contract.execution_enabled,
            cfg!(target_os = "linux")
        );
    }

    #[test]
    fn system_info_summary_stays_compact() {
        let summary = programmable_reasoning_system_info_summary();
        assert_eq!(summary.stage, ProgrammableReasoningStage::ExperienceCrystal);
        assert_eq!(summary.execution_enabled, cfg!(target_os = "linux"));
        assert!(summary.linux_only);
        assert!(summary.proposal_only_persistence);
    }
}
