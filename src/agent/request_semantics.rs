//! Typed request semantics carried through agent telemetry/governance.
//! 预回合请求语义编译结果：为后续 routing / governance / telemetry 提供正式输入。

use super::active_work::{ActiveWorkKind, ActiveWorkRecord};
use crate::bus::{IngressKind, PcMsg};

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
pub(crate) enum ActionFamily {
    Conversation,
    ActiveAction,
    TaskExecution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForegroundControlDecision {
    IndependentTurn,
    ContinueActiveWork,
    ReviseActiveWork,
    CancelOrAbortActiveWork,
    SupersedeActiveWork,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReasoningContract {
    pub(crate) request_kind: RequestKind,
    pub(crate) disclosure_surface: DisclosureSurface,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) confidence: u8,
}

impl Default for ReasoningContract {
    fn default() -> Self {
        Self {
            request_kind: RequestKind::General,
            disclosure_surface: DisclosureSurface::Governed,
            evidence_need: EvidenceNeed::None,
            execution_preference: ExecutionPreference::AnswerDirect,
            confidence: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReasoningContractCompileInput<'a> {
    pub(crate) msg: &'a PcMsg,
    pub(crate) has_tools: bool,
    pub(crate) deliberation_class: crate::memory::TurnDeliberationClass,
    pub(crate) reply_surface: crate::agent::reply_surface::ReplySurface,
    pub(crate) request_semantics: RequestSemantics,
    pub(crate) active_task_context_present: bool,
    pub(crate) governed_memory_evidence_present: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestSemanticsCompileInput<'a> {
    pub(crate) msg: &'a PcMsg,
    pub(crate) active_work: Option<&'a ActiveWorkRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestSemantics {
    pub(crate) request_kind: RequestKind,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) disclosure_surface: DisclosureSurface,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) action_family: ActionFamily,
    pub(crate) foreground_control: ForegroundControlDecision,
    pub(crate) confidence: u8,
}

impl RequestSemantics {
    pub(crate) fn compile_for_turn(input: RequestSemanticsCompileInput<'_>) -> Self {
        let mut semantics = Self::conservative_default();
        semantics.confidence = if input.msg.ingress == IngressKind::User {
            40
        } else {
            100
        };
        if input.active_work.is_some() {
            semantics.request_kind = RequestKind::General;
        }
        semantics
    }

    pub(crate) fn apply_foreground_control(
        mut self,
        active_work: Option<&ActiveWorkRecord>,
        decision: ForegroundControlDecision,
    ) -> Self {
        self.foreground_control = decision;
        self.action_family = match (active_work, decision) {
            (
                Some(active_work),
                ForegroundControlDecision::ContinueActiveWork
                | ForegroundControlDecision::ReviseActiveWork,
            ) => active_work.kind.into(),
            _ => ActionFamily::Conversation,
        };
        self.confidence = match (active_work, decision) {
            (_, ForegroundControlDecision::IndependentTurn) => self.confidence,
            (Some(active_work), ForegroundControlDecision::ContinueActiveWork) => {
                match active_work.kind {
                    ActiveWorkKind::InteractiveAction => 75,
                    ActiveWorkKind::TaskExecution => 100,
                }
            }
            (None, ForegroundControlDecision::ContinueActiveWork) => self.confidence,
            (_, ForegroundControlDecision::ReviseActiveWork) => 88,
            (_, ForegroundControlDecision::CancelOrAbortActiveWork) => 90,
            (_, ForegroundControlDecision::SupersedeActiveWork) => 82,
        };
        self
    }

    pub(crate) fn apply_reasoning_contract(mut self, contract: ReasoningContract) -> Self {
        self.request_kind = contract.request_kind;
        self.disclosure_surface = contract.disclosure_surface;
        self.evidence_need = contract.evidence_need;
        self.execution_preference = contract.execution_preference;
        self.confidence = self.confidence.max(contract.confidence);
        self
    }

    pub(crate) fn conservative_default() -> Self {
        Self {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::None,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference: ExecutionPreference::AnswerDirect,
            action_family: ActionFamily::Conversation,
            foreground_control: ForegroundControlDecision::IndependentTurn,
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
            action_family: ActionFamily::Conversation,
            foreground_control: ForegroundControlDecision::IndependentTurn,
            confidence: 100,
        }
    }
}

impl From<ActiveWorkKind> for ActionFamily {
    fn from(value: ActiveWorkKind) -> Self {
        match value {
            ActiveWorkKind::InteractiveAction => ActionFamily::ActiveAction,
            ActiveWorkKind::TaskExecution => ActionFamily::TaskExecution,
        }
    }
}

pub(crate) fn compile_reasoning_contract(
    input: ReasoningContractCompileInput<'_>,
) -> ReasoningContract {
    if input.msg.ingress != IngressKind::User {
        return ReasoningContract::default();
    }
    if input.reply_surface == crate::agent::reply_surface::ReplySurface::PrivateBoundary {
        return ReasoningContract {
            request_kind: RequestKind::PrivateMaterialRequest,
            disclosure_surface: DisclosureSurface::Private,
            confidence: input.request_semantics.confidence.max(85),
            ..ReasoningContract::default()
        };
    }
    if input.governed_memory_evidence_present
        && matches!(
            input.request_semantics.foreground_control,
            ForegroundControlDecision::IndependentTurn
                | ForegroundControlDecision::SupersedeActiveWork
        )
    {
        return ReasoningContract {
            request_kind: RequestKind::MemoryRecall,
            evidence_need: EvidenceNeed::ArchiveMemory,
            execution_preference: ExecutionPreference::MemoryFirst,
            confidence: input.request_semantics.confidence.max(86),
            ..ReasoningContract::default()
        };
    }
    if input.reply_surface == crate::agent::reply_surface::ReplySurface::TaskExecution
        || input.active_task_context_present
        || matches!(
            input.request_semantics.action_family,
            ActionFamily::ActiveAction | ActionFamily::TaskExecution
        )
    {
        return ReasoningContract {
            evidence_need: if input.has_tools {
                EvidenceNeed::HostTool
            } else {
                EvidenceNeed::None
            },
            execution_preference: if input.has_tools {
                ExecutionPreference::ToolFirst
            } else {
                ExecutionPreference::AnswerDirect
            },
            confidence: input.request_semantics.confidence.max(90),
            ..ReasoningContract::default()
        };
    }
    if input.has_tools
        && input.deliberation_class == crate::memory::TurnDeliberationClass::HardReasoning
        && input.reply_surface != crate::agent::reply_surface::ReplySurface::InternalOnly
    {
        return ReasoningContract {
            request_kind: RequestKind::HostDiagnostics,
            evidence_need: EvidenceNeed::HostTool,
            execution_preference: ExecutionPreference::ToolFirst,
            confidence: input.request_semantics.confidence.max(78),
            ..ReasoningContract::default()
        };
    }
    ReasoningContract::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::reply_surface::ReplySurface;

    fn compile_with_active_work(msg: &PcMsg, active_work: &ActiveWorkRecord) -> RequestSemantics {
        RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg,
            active_work: Some(active_work),
        })
        .apply_foreground_control(
            Some(active_work),
            active_work.foreground_control_for_user_turn(&msg.content),
        )
    }

    #[test]
    fn compiler_defaults_plain_user_turn_to_conversation() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "你好", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: None,
        });

        assert_eq!(semantics.request_kind, RequestKind::General);
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(semantics.disclosure_surface, DisclosureSurface::Governed);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::IndependentTurn
        );
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_marks_active_task_execution_work_as_continue_control() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let active_work = ActiveWorkRecord {
            kind: ActiveWorkKind::TaskExecution,
            title: "QQ 邮箱配置".to_string(),
            status: super::super::active_work::ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: String::new(),
            next_action: "补认证信息并继续配置".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };
        let semantics = compile_with_active_work(&msg, &active_work);

        assert_eq!(semantics.action_family, ActionFamily::TaskExecution);
        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::ContinueActiveWork
        );
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn compiler_marks_explicit_cancel_for_active_action_as_cancel_control() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "先别配了，这个动作取消", false)
            .expect("message");
        let active_work = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            status: super::super::active_work::ForegroundWorkStatus::AwaitingUser,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: "等待用户补认证信息".to_string(),
            next_action: "补认证信息并继续配置".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };
        let semantics = compile_with_active_work(&msg, &active_work);

        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::CancelOrAbortActiveWork
        );
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 90);
    }

    #[test]
    fn compiler_does_not_cancel_mixed_turn_that_still_supplies_followup_input() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "算了，还是用 Work 账号继续", false)
            .expect("message");
        let active_work = ActiveWorkRecord {
            kind: ActiveWorkKind::InteractiveAction,
            title: "QQ 邮箱配置".to_string(),
            status: super::super::active_work::ForegroundWorkStatus::AwaitingUser,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: "你要用 Work（mail-work）还是 Personal（mail-personal）这个邮箱账户？"
                .to_string(),
            next_action: "请明确要继续的邮箱账户".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };
        let semantics = compile_with_active_work(&msg, &active_work);

        assert_ne!(
            semantics.foreground_control,
            ForegroundControlDecision::CancelOrAbortActiveWork
        );
    }

    #[test]
    fn compiler_does_not_resume_active_task_run_for_explicit_stop_turn() {
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "算了，先停下", false).expect("message");
        let active_work = ActiveWorkRecord {
            kind: ActiveWorkKind::TaskExecution,
            title: "QQ 邮箱配置".to_string(),
            status: super::super::active_work::ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: String::new(),
            next_action: "补认证信息并继续配置".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };
        let semantics = compile_with_active_work(&msg, &active_work);

        assert_ne!(
            semantics.foreground_control,
            ForegroundControlDecision::ContinueActiveWork
        );
    }

    #[test]
    fn compiler_keeps_system_ingress_out_of_resume_path() {
        let msg = PcMsg::new_system("self_runtime", "chat-1", "idle tick").expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::IndependentTurn
        );
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn compiler_does_not_resume_from_execution_state_projection_without_active_work() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::IndependentTurn
        );
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_marks_mismatched_active_work_as_supersede_instead_of_resume() {
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "查看当前系统状态", false).expect("message");
        let active_work = ActiveWorkRecord {
            kind: ActiveWorkKind::TaskExecution,
            title: "QQ 邮箱配置".to_string(),
            status: super::super::active_work::ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "账户草案已创建".to_string(),
            blocker: String::new(),
            next_action: "补认证信息并继续配置".to_string(),
            recent_outcome: String::new(),
            active_artifact_refs: Vec::new(),
            updated_at: 7,
        };
        let semantics = compile_with_active_work(&msg, &active_work);

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(
            semantics.foreground_control,
            ForegroundControlDecision::SupersedeActiveWork
        );
    }

    #[test]
    fn reasoning_contract_requires_host_tool_for_fresh_hard_runtime_turn() {
        let msg =
            PcMsg::new_inbound("qq_channel", "chat-1", "查看系统状态", false).expect("message");
        let contract = compile_reasoning_contract(ReasoningContractCompileInput {
            msg: &msg,
            has_tools: true,
            deliberation_class: crate::memory::TurnDeliberationClass::HardReasoning,
            reply_surface: ReplySurface::GovernedConversation,
            request_semantics: RequestSemantics::conservative_default(),
            active_task_context_present: false,
            governed_memory_evidence_present: false,
        });

        assert_eq!(contract.request_kind, RequestKind::HostDiagnostics);
        assert_eq!(contract.evidence_need, EvidenceNeed::HostTool);
        assert_eq!(
            contract.execution_preference,
            ExecutionPreference::ToolFirst
        );
    }

    #[test]
    fn reasoning_contract_requires_memory_first_when_governed_memory_evidence_exists() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "把我上次提到的偏好回忆一下", false)
            .expect("message");
        let contract = compile_reasoning_contract(ReasoningContractCompileInput {
            msg: &msg,
            has_tools: true,
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            request_semantics: RequestSemantics::conservative_default(),
            active_task_context_present: false,
            governed_memory_evidence_present: true,
        });

        assert_eq!(contract.request_kind, RequestKind::MemoryRecall);
        assert_eq!(contract.evidence_need, EvidenceNeed::ArchiveMemory);
        assert_eq!(
            contract.execution_preference,
            ExecutionPreference::MemoryFirst
        );
    }
}
