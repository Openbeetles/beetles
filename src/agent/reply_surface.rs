//! Reply surface contract selection for user-visible answers.
//! 统一回复面合同：把“这轮该怎么交付”升级成正式类型，而不是散落标签。

use super::request_semantics::{DisclosureSurface, EvidenceNeed, RequestSemantics};
use crate::bus::IngressKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplySurface {
    PublicRuntime,
    GovernedConversation,
    PrivateBoundary,
    TaskExecution,
    InternalOnly,
}

impl ReplySurface {
    pub(crate) fn for_turn(ingress: IngressKind, semantics: RequestSemantics) -> Self {
        if ingress != IngressKind::User {
            return Self::InternalOnly;
        }
        match semantics.disclosure_surface {
            DisclosureSurface::Private => Self::PrivateBoundary,
            DisclosureSurface::Public => match semantics.evidence_need {
                EvidenceNeed::PublicRuntime | EvidenceNeed::HostTool => Self::PublicRuntime,
                _ => Self::GovernedConversation,
            },
            DisclosureSurface::Governed => Self::GovernedConversation,
        }
    }

    pub(crate) fn requires_structured_finalization_after_tool_success(self) -> bool {
        matches!(self, Self::PublicRuntime)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PublicRuntime => "public_runtime",
            Self::GovernedConversation => "governed_conversation",
            Self::PrivateBoundary => "private_boundary",
            Self::TaskExecution => "task_execution",
            Self::InternalOnly => "internal_only",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::request_semantics::{
        DisclosureSurface, EvidenceNeed, ExecutionPreference, RequestKind,
    };

    fn semantics(
        disclosure_surface: DisclosureSurface,
        evidence_need: EvidenceNeed,
    ) -> RequestSemantics {
        RequestSemantics {
            request_kind: RequestKind::General,
            evidence_need,
            disclosure_surface,
            execution_preference: ExecutionPreference::AnswerDirect,
            confidence: 100,
        }
    }

    #[test]
    fn public_runtime_surface_maps_from_public_runtime_evidence() {
        let surface = ReplySurface::for_turn(
            IngressKind::User,
            semantics(DisclosureSurface::Public, EvidenceNeed::PublicRuntime),
        );
        assert_eq!(surface, ReplySurface::PublicRuntime);
    }

    #[test]
    fn private_boundary_surface_maps_from_private_disclosure() {
        let surface = ReplySurface::for_turn(
            IngressKind::User,
            semantics(DisclosureSurface::Private, EvidenceNeed::CanonicalMemory),
        );
        assert_eq!(surface, ReplySurface::PrivateBoundary);
    }

    #[test]
    fn internal_only_surface_maps_from_system_ingress() {
        let surface = ReplySurface::for_turn(
            IngressKind::System,
            semantics(DisclosureSurface::Public, EvidenceNeed::PublicRuntime),
        );
        assert_eq!(surface, ReplySurface::InternalOnly);
        assert_eq!(surface.as_str(), "internal_only");
    }
}
