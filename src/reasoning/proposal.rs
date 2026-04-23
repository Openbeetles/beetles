//! Programmable reasoning proposal schema.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningProposalKind {
    MemoryPatch,
    ToolRequest,
    DoctrineRevision,
    SkillCrystal,
    EngineeringAsset,
    CapabilityAtom,
}

pub fn programmable_reasoning_proposal_kinds() -> Vec<ProgrammableReasoningProposalKind> {
    vec![
        ProgrammableReasoningProposalKind::MemoryPatch,
        ProgrammableReasoningProposalKind::ToolRequest,
        ProgrammableReasoningProposalKind::DoctrineRevision,
        ProgrammableReasoningProposalKind::SkillCrystal,
        ProgrammableReasoningProposalKind::EngineeringAsset,
        ProgrammableReasoningProposalKind::CapabilityAtom,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_kinds_cover_runtime_contract_up_through_engineering_assets() {
        let kinds = programmable_reasoning_proposal_kinds();
        assert_eq!(kinds.len(), 6);
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::MemoryPatch));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::ToolRequest));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::DoctrineRevision));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::SkillCrystal));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::EngineeringAsset));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::CapabilityAtom));
    }
}
