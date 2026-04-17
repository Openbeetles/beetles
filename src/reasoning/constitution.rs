//! Programmable reasoning constitution and runtime contract.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningStage {
    ConstitutionOnly,
    TaskScriptingBaseline,
    MemoryQueryPlane,
    IdleMemoryForge,
    MemoryAttackDistillation,
    CapabilityBridgeExpansion,
    ExperienceCrystal,
    EngineeringSynthesis,
    IntentCompiler,
    CounterfactualSandbox,
    AdversarialArena,
    DoctrineGenomeEvolution,
    CapabilityAtomsExchange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningCapabilityKind {
    TurnLocalReadonly,
    MemoryQuery,
    IdleMaintenance,
    MemoryAttackDistillation,
    CapabilityBridgeExpansion,
    ExperienceCrystal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningExecutionBackend {
    None,
    LuaSandbox,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningCapabilityContract {
    pub kind: ProgrammableReasoningCapabilityKind,
    pub linux_only: bool,
    pub execution_enabled: bool,
    pub proposal_only_persistence: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningRuntimeContract {
    pub stage: ProgrammableReasoningStage,
    pub linux_only: bool,
    pub execution_backend: ProgrammableReasoningExecutionBackend,
    pub execution_enabled: bool,
    pub proposal_only_persistence: bool,
    pub operator_visible_contract: bool,
    pub user_authored_scripts: bool,
    pub direct_host_tool_execution: bool,
    pub second_execution_plane_forbidden: bool,
}

pub fn programmable_reasoning_runtime_contract() -> ProgrammableReasoningRuntimeContract {
    let execution_enabled = cfg!(target_os = "linux");
    ProgrammableReasoningRuntimeContract {
        stage: ProgrammableReasoningStage::CapabilityAtomsExchange,
        linux_only: true,
        execution_backend: if execution_enabled {
            ProgrammableReasoningExecutionBackend::LuaSandbox
        } else {
            ProgrammableReasoningExecutionBackend::None
        },
        execution_enabled,
        proposal_only_persistence: true,
        operator_visible_contract: true,
        user_authored_scripts: false,
        direct_host_tool_execution: false,
        second_execution_plane_forbidden: true,
    }
}

pub fn programmable_reasoning_capability_taxonomy() -> Vec<ProgrammableReasoningCapabilityContract>
{
    [
        ProgrammableReasoningCapabilityKind::TurnLocalReadonly,
        ProgrammableReasoningCapabilityKind::MemoryQuery,
        ProgrammableReasoningCapabilityKind::IdleMaintenance,
        ProgrammableReasoningCapabilityKind::MemoryAttackDistillation,
        ProgrammableReasoningCapabilityKind::CapabilityBridgeExpansion,
        ProgrammableReasoningCapabilityKind::ExperienceCrystal,
    ]
    .into_iter()
    .map(|kind| ProgrammableReasoningCapabilityContract {
        kind,
        linux_only: true,
        execution_enabled: cfg!(target_os = "linux"),
        proposal_only_persistence: true,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_contract_moves_to_p13_capability_atoms_exchange() {
        let contract = programmable_reasoning_runtime_contract();
        assert_eq!(
            contract.stage,
            ProgrammableReasoningStage::CapabilityAtomsExchange
        );
        assert!(contract.linux_only);
        assert_eq!(contract.execution_enabled, cfg!(target_os = "linux"));
        assert!(contract.proposal_only_persistence);
        assert!(contract.second_execution_plane_forbidden);
        assert!(!contract.user_authored_scripts);
        assert!(!contract.direct_host_tool_execution);
    }

    #[test]
    fn capability_taxonomy_is_linux_only_and_non_executable() {
        let taxonomy = programmable_reasoning_capability_taxonomy();
        assert_eq!(taxonomy.len(), 6);
        assert!(taxonomy.iter().all(|entry| entry.linux_only));
        assert!(taxonomy
            .iter()
            .all(|entry| entry.execution_enabled == cfg!(target_os = "linux")));
        assert!(taxonomy.iter().all(|entry| entry.proposal_only_persistence));
    }
}
