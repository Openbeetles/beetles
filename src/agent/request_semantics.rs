//! Typed request semantics carried through agent telemetry/governance.
//! P3 removes the pre-turn compiler so this stays as an internal contract only.

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestKind {
    General,
    OpsObservability,
    HostDiagnostics,
    MemoryRecall,
    PrivateMaterialRequest,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EvidenceNeed {
    None,
    PublicRuntime,
    HostTool,
    ArchiveMemory,
    CanonicalMemory,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisclosureSurface {
    Public,
    Governed,
    Private,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecutionPreference {
    AnswerDirect,
    ToolFirst,
    MemoryFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestSemantics {
    pub(crate) request_kind: RequestKind,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) disclosure_surface: DisclosureSurface,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) confidence: u8,
}

impl RequestSemantics {
    pub(crate) fn conservative_default() -> Self {
        Self {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::None,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference: ExecutionPreference::AnswerDirect,
            confidence: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn public_tool_first() -> Self {
        Self {
            request_kind: RequestKind::OpsObservability,
            evidence_need: EvidenceNeed::PublicRuntime,
            disclosure_surface: DisclosureSurface::Public,
            execution_preference: ExecutionPreference::ToolFirst,
            confidence: 100,
        }
    }

    pub(crate) fn is_public_surface(self) -> bool {
        matches!(self.disclosure_surface, DisclosureSurface::Public)
    }
}
