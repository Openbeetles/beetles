//! Reply surface contract selection for user-visible answers.
//! 统一回复面合同：把“这轮该怎么交付”升级成正式类型，而不是散落标签。

use super::request_semantics::RequestSemantics;
use crate::bus::IngressKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceEvidencePolicy {
    PublicRuntimeAuthority,
    GovernedConversationContext,
    PrivateBoundaryContext,
    TaskWorkspaceAuthority,
    InternalOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceGovernancePolicy {
    SkipMentalPrivacyReview,
    ApplyMentalPrivacyReview,
    PrivateBoundaryReview,
    TaskExecutionReview,
    SuppressUserDelivery,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceFinalizationPolicy {
    StructuredJson,
    DirectOrRecovery,
    TaskFinisher,
    InternalOnly,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplySurface {
    PublicRuntime,
    GovernedConversation,
    PrivateBoundary,
    TaskExecution,
    InternalOnly,
}

impl ReplySurface {
    pub(crate) fn for_turn(ingress: IngressKind, _semantics: RequestSemantics) -> Self {
        if ingress != IngressKind::User {
            return Self::InternalOnly;
        }
        Self::GovernedConversation
    }

    pub(crate) fn evidence_policy(self) -> SurfaceEvidencePolicy {
        match self {
            Self::PublicRuntime => SurfaceEvidencePolicy::PublicRuntimeAuthority,
            Self::GovernedConversation => SurfaceEvidencePolicy::GovernedConversationContext,
            Self::PrivateBoundary => SurfaceEvidencePolicy::PrivateBoundaryContext,
            Self::TaskExecution => SurfaceEvidencePolicy::TaskWorkspaceAuthority,
            Self::InternalOnly => SurfaceEvidencePolicy::InternalOnly,
        }
    }

    pub(crate) fn governance_policy(self) -> SurfaceGovernancePolicy {
        match self {
            Self::PublicRuntime => SurfaceGovernancePolicy::SkipMentalPrivacyReview,
            Self::GovernedConversation => SurfaceGovernancePolicy::ApplyMentalPrivacyReview,
            Self::PrivateBoundary => SurfaceGovernancePolicy::PrivateBoundaryReview,
            Self::TaskExecution => SurfaceGovernancePolicy::TaskExecutionReview,
            Self::InternalOnly => SurfaceGovernancePolicy::SuppressUserDelivery,
        }
    }

    pub(crate) fn finalization_policy(self) -> SurfaceFinalizationPolicy {
        match self {
            Self::PublicRuntime | Self::PrivateBoundary => {
                SurfaceFinalizationPolicy::StructuredJson
            }
            Self::GovernedConversation => SurfaceFinalizationPolicy::DirectOrRecovery,
            Self::TaskExecution => SurfaceFinalizationPolicy::TaskFinisher,
            Self::InternalOnly => SurfaceFinalizationPolicy::InternalOnly,
        }
    }

    pub(crate) fn requires_structured_finalization_after_tool_success(self) -> bool {
        matches!(
            self.finalization_policy(),
            SurfaceFinalizationPolicy::StructuredJson
        )
    }

    pub(crate) fn allows_mental_privacy_review(self) -> bool {
        !matches!(
            self.governance_policy(),
            SurfaceGovernancePolicy::SkipMentalPrivacyReview
                | SurfaceGovernancePolicy::SuppressUserDelivery
        )
    }

    pub(crate) fn evidence_authority(self) -> Option<&'static str> {
        match self.evidence_policy() {
            SurfaceEvidencePolicy::PublicRuntimeAuthority => Some("public_runtime_host"),
            SurfaceEvidencePolicy::GovernedConversationContext => Some("governed_context"),
            SurfaceEvidencePolicy::PrivateBoundaryContext => Some("private_boundary_context"),
            SurfaceEvidencePolicy::TaskWorkspaceAuthority => Some("task_workspace"),
            SurfaceEvidencePolicy::InternalOnly => None,
        }
    }

    pub(crate) fn allows_memory_grounding_block(self) -> bool {
        matches!(
            self.evidence_policy(),
            SurfaceEvidencePolicy::GovernedConversationContext
                | SurfaceEvidencePolicy::TaskWorkspaceAuthority
        )
    }

    pub(crate) fn accepts_tool_evidence(self, tool_name: &str) -> bool {
        match self.evidence_policy() {
            SurfaceEvidencePolicy::PublicRuntimeAuthority => matches!(
                tool_name,
                "board_info" | "process" | "network" | "network_scan" | "system_control"
            ),
            SurfaceEvidencePolicy::GovernedConversationContext
            | SurfaceEvidencePolicy::PrivateBoundaryContext
            | SurfaceEvidencePolicy::TaskWorkspaceAuthority => true,
            SurfaceEvidencePolicy::InternalOnly => false,
        }
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
        assert_eq!(surface, ReplySurface::GovernedConversation);
    }

    #[test]
    fn private_boundary_surface_maps_from_private_disclosure() {
        let surface = ReplySurface::for_turn(
            IngressKind::User,
            semantics(DisclosureSurface::Private, EvidenceNeed::CanonicalMemory),
        );
        assert_eq!(surface, ReplySurface::GovernedConversation);
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

    #[test]
    fn public_runtime_contract_skips_privacy_review_and_uses_structured_finalization() {
        let surface = ReplySurface::PublicRuntime;
        assert_eq!(
            surface.evidence_policy(),
            SurfaceEvidencePolicy::PublicRuntimeAuthority
        );
        assert_eq!(
            surface.governance_policy(),
            SurfaceGovernancePolicy::SkipMentalPrivacyReview
        );
        assert_eq!(
            surface.finalization_policy(),
            SurfaceFinalizationPolicy::StructuredJson
        );
        assert!(!surface.allows_mental_privacy_review());
    }

    #[test]
    fn private_boundary_contract_keeps_governance_and_structured_finalization() {
        let surface = ReplySurface::PrivateBoundary;
        assert_eq!(
            surface.evidence_policy(),
            SurfaceEvidencePolicy::PrivateBoundaryContext
        );
        assert_eq!(
            surface.governance_policy(),
            SurfaceGovernancePolicy::PrivateBoundaryReview
        );
        assert_eq!(
            surface.finalization_policy(),
            SurfaceFinalizationPolicy::StructuredJson
        );
        assert!(surface.allows_mental_privacy_review());
    }

    #[test]
    fn governed_and_task_execution_contracts_keep_review_paths_enabled() {
        let governed = ReplySurface::GovernedConversation;
        assert_eq!(
            governed.governance_policy(),
            SurfaceGovernancePolicy::ApplyMentalPrivacyReview
        );
        assert_eq!(
            governed.finalization_policy(),
            SurfaceFinalizationPolicy::DirectOrRecovery
        );
        assert!(governed.allows_mental_privacy_review());

        let task = ReplySurface::TaskExecution;
        assert_eq!(
            task.governance_policy(),
            SurfaceGovernancePolicy::TaskExecutionReview
        );
        assert_eq!(
            task.finalization_policy(),
            SurfaceFinalizationPolicy::TaskFinisher
        );
        assert!(task.allows_mental_privacy_review());
        assert!(task.allows_memory_grounding_block());
    }

    #[test]
    fn public_runtime_contract_keeps_memory_grounding_out_of_evidence_block() {
        let surface = ReplySurface::PublicRuntime;
        assert!(!surface.allows_memory_grounding_block());
        assert!(surface.accepts_tool_evidence("board_info"));
        assert!(surface.accepts_tool_evidence("network"));
        assert!(!surface.accepts_tool_evidence("memory_get"));
    }
}
