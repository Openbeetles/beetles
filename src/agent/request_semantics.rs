//! Typed request semantics carried through agent telemetry/governance.
//! 预回合请求语义编译结果：为后续 routing / governance / telemetry 提供正式输入。

use super::active_work::{ActiveWorkKind, ActiveWorkRecord};
use crate::bus::{IngressKind, PcMsg};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestKind {
    General,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RequestShapeMetrics {
    pub(crate) char_count: usize,
    pub(crate) line_count: usize,
    pub(crate) separator_count: usize,
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
    pub(crate) foreground_work_context_present: bool,
    pub(crate) governed_memory_evidence_present: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestSemanticsCompileInput<'a> {
    pub(crate) msg: &'a PcMsg,
    #[allow(dead_code)]
    pub(crate) active_work: Option<&'a ActiveWorkRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestSemantics {
    pub(crate) request_kind: RequestKind,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) disclosure_surface: DisclosureSurface,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) action_family: ActionFamily,
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
        semantics
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
            confidence: 0,
        }
    }
}

pub(crate) fn request_shape_metrics(content: &str) -> RequestShapeMetrics {
    request_shape_metrics_with_colons(content, true)
}

pub(crate) fn request_shape_metrics_without_colons(content: &str) -> RequestShapeMetrics {
    request_shape_metrics_with_colons(content, false)
}

fn request_shape_metrics_with_colons(content: &str, include_colons: bool) -> RequestShapeMetrics {
    RequestShapeMetrics {
        char_count: content.chars().count(),
        line_count: content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
        separator_count: content
            .chars()
            .filter(|ch| {
                matches!(ch, '\n' | ',' | '，' | '.' | '。' | ';' | '；')
                    || (include_colons && matches!(ch, ':' | '：'))
            })
            .count(),
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
        && !input.active_task_context_present
        && !input.foreground_work_context_present
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
        || input.foreground_work_context_present
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
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_keeps_active_work_followup_on_independent_turn_until_agent_settles_it() {
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
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: Some(&active_work),
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_does_not_resume_from_execution_state_projection_without_active_work() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_keeps_system_ingress_out_of_resume_path() {
        let msg = PcMsg::new_system("self_runtime", "chat-1", "idle tick").expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn reasoning_contract_keeps_tool_first_when_foreground_work_context_exists() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
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
        let request_semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            active_work: Some(&active_work),
        });
        let contract = compile_reasoning_contract(ReasoningContractCompileInput {
            msg: &msg,
            has_tools: true,
            deliberation_class: crate::memory::TurnDeliberationClass::Standard,
            reply_surface: ReplySurface::GovernedConversation,
            request_semantics,
            active_task_context_present: false,
            foreground_work_context_present: true,
            governed_memory_evidence_present: false,
        });

        assert_eq!(contract.evidence_need, EvidenceNeed::HostTool);
        assert_eq!(
            contract.execution_preference,
            ExecutionPreference::ToolFirst
        );
        assert!(contract.confidence >= 85);
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
            foreground_work_context_present: false,
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
            foreground_work_context_present: false,
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
