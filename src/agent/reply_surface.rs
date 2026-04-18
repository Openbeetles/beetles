//! Reply surface contract selection for user-visible answers.
//! 统一回复面合同：把“这轮该怎么交付”升级成正式类型，而不是散落标签。

use super::request_semantics::{ActionFamily, DisclosureSurface, RequestSemantics};
use crate::agent::final_reply::{
    reply_has_concrete_anchor, reply_looks_like_future_action_narration,
};
use crate::bus::IngressKind;
use std::collections::BTreeSet;

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
    TaskFinisher,
    InternalOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplySurface {
    PublicRuntime,
    GovernedConversation,
    PrivateBoundary,
    TaskExecution,
    InternalOnly,
}

impl ReplySurface {
    pub(crate) fn for_prepared_turn(
        ingress: IngressKind,
        semantics: RequestSemantics,
        privacy_boundary_hit: bool,
    ) -> Self {
        if ingress != IngressKind::User {
            return Self::InternalOnly;
        }
        if semantics.action_family == ActionFamily::TaskExecution {
            return Self::TaskExecution;
        }
        if privacy_boundary_hit || semantics.disclosure_surface == DisclosureSurface::Private {
            return Self::PrivateBoundary;
        }
        Self::GovernedConversation
    }

    pub(crate) fn promote_for_runtime_tools(
        self,
        successful_tool_names: &BTreeSet<String>,
        external_content_used: bool,
        draft_content: &str,
    ) -> Self {
        if self != Self::GovernedConversation
            || external_content_used
            || successful_tool_names.is_empty()
            || !runtime_tool_draft_supports_public_surface(draft_content)
        {
            return self;
        }
        if successful_tool_names
            .iter()
            .all(|tool_name| is_public_runtime_tool(tool_name))
        {
            Self::PublicRuntime
        } else {
            self
        }
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
            Self::PublicRuntime | Self::GovernedConversation | Self::PrivateBoundary => {
                SurfaceFinalizationPolicy::StructuredJson
            }
            Self::TaskExecution => SurfaceFinalizationPolicy::TaskFinisher,
            Self::InternalOnly => SurfaceFinalizationPolicy::InternalOnly,
        }
    }

    pub(crate) fn should_run_structured_finalization_after_tool_round(
        self,
        draft_content: &str,
    ) -> bool {
        if !matches!(
            self.finalization_policy(),
            SurfaceFinalizationPolicy::StructuredJson
        ) {
            return false;
        }
        let trimmed = draft_content.trim();
        if trimmed.is_empty() {
            return true;
        }
        match self {
            Self::PublicRuntime => {
                reply_looks_like_future_action_narration(trimmed)
                    || !reply_has_concrete_anchor(trimmed)
            }
            Self::GovernedConversation | Self::PrivateBoundary => {
                reply_looks_like_future_action_narration(trimmed)
            }
            Self::TaskExecution | Self::InternalOnly => false,
        }
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
            SurfaceEvidencePolicy::PublicRuntimeAuthority => is_public_runtime_tool(tool_name),
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

fn runtime_tool_draft_supports_public_surface(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty()
        || looks_like_boundary_or_input_request(trimmed)
        || reply_looks_like_future_action_narration(trimmed)
    {
        return false;
    }
    reply_has_concrete_anchor(trimmed)
}

fn is_public_runtime_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "board_info" | "process" | "network" | "network_scan" | "system_control"
    )
}

fn looks_like_boundary_or_input_request(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.contains('?')
        || content.contains('？')
        || content.contains("请先提供")
        || content.contains("请提供")
        || content.contains("请把")
        || content.contains("请发")
        || content.contains("无法继续")
        || content.contains("不能继续")
        || content.contains("内部")
        || content.contains("不对外公开")
        || content.contains("不公开")
        || lower.contains("please provide")
        || lower.contains("please send")
        || lower.contains("cannot continue")
        || lower.contains("can't continue")
        || lower.contains("internal")
        || lower.contains("not disclose")
        || lower.contains("not public")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::request_semantics::{
        ActionFamily, DisclosureSurface, EvidenceNeed, ExecutionPreference,
        ForegroundControlDecision, RequestKind,
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
            action_family: ActionFamily::Conversation,
            foreground_control: ForegroundControlDecision::IndependentTurn,
            confidence: 100,
        }
    }

    #[test]
    fn public_runtime_is_not_selected_from_request_semantics_alone() {
        let surface = ReplySurface::for_prepared_turn(
            IngressKind::User,
            semantics(DisclosureSurface::Public, EvidenceNeed::PublicRuntime),
            false,
        );
        assert_eq!(surface, ReplySurface::GovernedConversation);
    }

    #[test]
    fn internal_only_surface_maps_from_system_ingress() {
        let surface = ReplySurface::for_prepared_turn(
            IngressKind::System,
            semantics(DisclosureSurface::Public, EvidenceNeed::PublicRuntime),
            false,
        );
        assert_eq!(surface, ReplySurface::InternalOnly);
        assert_eq!(surface.as_str(), "internal_only");
    }

    #[test]
    fn private_boundary_surface_maps_from_governance_hit() {
        let surface = ReplySurface::for_prepared_turn(
            IngressKind::User,
            semantics(DisclosureSurface::Governed, EvidenceNeed::CanonicalMemory),
            true,
        );
        assert_eq!(surface, ReplySurface::PrivateBoundary);
    }

    #[test]
    fn public_runtime_surface_is_promoted_only_after_runtime_tools_succeed() {
        let mut successful_tool_names = BTreeSet::new();
        successful_tool_names.insert("board_info".to_string());
        let surface = ReplySurface::GovernedConversation.promote_for_runtime_tools(
            &successful_tool_names,
            false,
            "当前版本是 1.2.3，配置目录在 /var/lib/beetle/config。",
        );
        assert_eq!(surface, ReplySurface::PublicRuntime);

        successful_tool_names.insert("mail".to_string());
        let surface = ReplySurface::GovernedConversation.promote_for_runtime_tools(
            &successful_tool_names,
            false,
            "当前版本是 1.2.3，配置目录在 /var/lib/beetle/config。",
        );
        assert_eq!(surface, ReplySurface::GovernedConversation);
    }

    #[test]
    fn public_runtime_surface_does_not_promote_vague_or_boundary_tool_drafts() {
        let mut successful_tool_names = BTreeSet::new();
        successful_tool_names.insert("board_info".to_string());

        let vague = ReplySurface::GovernedConversation.promote_for_runtime_tools(
            &successful_tool_names,
            false,
            "我先整理一下当前状态。",
        );
        assert_eq!(vague, ReplySurface::GovernedConversation);

        let boundary = ReplySurface::GovernedConversation.promote_for_runtime_tools(
            &successful_tool_names,
            false,
            "系统信息属于内部运行机制，这部分内容不对外公开。",
        );
        assert_eq!(boundary, ReplySurface::GovernedConversation);
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
        assert!(surface.should_run_structured_finalization_after_tool_round("系统状态正常。"));
        assert!(
            !surface.should_run_structured_finalization_after_tool_round(
                "当前版本是 1.2.3，配置目录在 /var/lib/beetle/config。"
            )
        );
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
            SurfaceFinalizationPolicy::StructuredJson
        );
        assert!(governed.allows_mental_privacy_review());
        assert!(
            governed.should_run_structured_finalization_after_tool_round("我先整理一下当前状态。")
        );
        assert!(
            !governed.should_run_structured_finalization_after_tool_round(
                "当前主机 beetle 在线，可继续配置 QQ 邮箱。"
            )
        );

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
        assert!(!task.should_run_structured_finalization_after_tool_round("当前任务正在继续。"));
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
