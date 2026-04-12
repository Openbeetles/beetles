//! Operator-visible programmable reasoning snapshot.

use crate::reasoning::constitution::{
    programmable_reasoning_capability_taxonomy, programmable_reasoning_runtime_contract,
    ProgrammableReasoningCapabilityContract, ProgrammableReasoningRuntimeContract,
    ProgrammableReasoningStage,
};
use crate::reasoning::proposal::{
    programmable_reasoning_proposal_kinds, ProgrammableReasoningProposalKind,
};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningOperatorSnapshot {
    pub stage: ProgrammableReasoningStage,
    pub runtime_contract: ProgrammableReasoningRuntimeContract,
    pub capabilities: Vec<ProgrammableReasoningCapabilityContract>,
    pub proposal_kinds: Vec<ProgrammableReasoningProposalKind>,
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
        operator_summary:
            "task_scripting_baseline: single-turn lua sandbox, no host bridge, no persistence write"
                .to_string(),
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
    fn operator_snapshot_reports_p1_contract() {
        let snapshot = programmable_reasoning_operator_snapshot();
        assert_eq!(snapshot.stage, ProgrammableReasoningStage::TaskScriptingBaseline);
        assert_eq!(snapshot.capabilities.len(), 3);
        assert_eq!(snapshot.proposal_kinds.len(), 4);
        assert_eq!(
            snapshot.runtime_contract.execution_enabled,
            cfg!(target_os = "linux")
        );
    }

    #[test]
    fn system_info_summary_stays_compact() {
        let summary = programmable_reasoning_system_info_summary();
        assert_eq!(summary.stage, ProgrammableReasoningStage::TaskScriptingBaseline);
        assert_eq!(summary.execution_enabled, cfg!(target_os = "linux"));
        assert!(summary.linux_only);
        assert!(summary.proposal_only_persistence);
    }
}
