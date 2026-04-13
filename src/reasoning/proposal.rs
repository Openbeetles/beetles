//! Programmable reasoning proposal schema.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningProposalKind {
    MemoryPatch,
    ToolRequest,
    DoctrineRevision,
    SkillCrystal,
    CapabilityAtom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgrammableReasoningProposalScope {
    Turn,
    Relation,
    Board,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgrammableReasoningProposal {
    pub proposal_id: String,
    pub kind: ProgrammableReasoningProposalKind,
    pub scope: ProgrammableReasoningProposalScope,
    pub summary: String,
    pub trace_ref: String,
    pub requires_adjudication: bool,
}

pub fn programmable_reasoning_proposal_kinds() -> Vec<ProgrammableReasoningProposalKind> {
    vec![
        ProgrammableReasoningProposalKind::MemoryPatch,
        ProgrammableReasoningProposalKind::ToolRequest,
        ProgrammableReasoningProposalKind::DoctrineRevision,
        ProgrammableReasoningProposalKind::SkillCrystal,
        ProgrammableReasoningProposalKind::CapabilityAtom,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proposal_kinds_cover_p0_contract() {
        let kinds = programmable_reasoning_proposal_kinds();
        assert_eq!(kinds.len(), 5);
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::MemoryPatch));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::ToolRequest));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::DoctrineRevision));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::SkillCrystal));
        assert!(kinds.contains(&ProgrammableReasoningProposalKind::CapabilityAtom));
    }
}
