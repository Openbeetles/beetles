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
pub(crate) enum ResumeRelation {
    IndependentTurn,
    DenyOrCancelActiveAction,
    ResumeActiveAction,
    ResumeActiveTaskRun,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestSemanticsCompileInput<'a> {
    pub(crate) msg: &'a PcMsg,
    pub(crate) has_tools: bool,
    pub(crate) active_work: Option<&'a ActiveWorkRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RequestSemantics {
    pub(crate) request_kind: RequestKind,
    pub(crate) evidence_need: EvidenceNeed,
    pub(crate) disclosure_surface: DisclosureSurface,
    pub(crate) execution_preference: ExecutionPreference,
    pub(crate) action_family: ActionFamily,
    pub(crate) resume_relation: ResumeRelation,
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
        if let Some(active_work) = input.active_work.filter(|record| {
            record.kind == ActiveWorkKind::InteractiveAction
                && looks_like_cancel_active_action_request(&input.msg.content)
        }) {
            semantics.request_kind = RequestKind::General;
            semantics.action_family = active_work.kind.into();
            semantics.resume_relation = ResumeRelation::DenyOrCancelActiveAction;
            semantics.confidence = 90;
            return semantics;
        }
        if let Some(active_work) = input
            .active_work
            .filter(|record| record.should_resume(&input.msg.content))
        {
            semantics.request_kind = RequestKind::General;
            semantics.evidence_need = if input.has_tools {
                EvidenceNeed::HostTool
            } else {
                EvidenceNeed::None
            };
            semantics.execution_preference = if input.has_tools {
                ExecutionPreference::ToolFirst
            } else {
                ExecutionPreference::AnswerDirect
            };
            match active_work.kind {
                ActiveWorkKind::InteractiveAction => {
                    semantics.action_family = ActionFamily::ActiveAction;
                    semantics.resume_relation = ResumeRelation::ResumeActiveAction;
                    semantics.confidence = 75;
                }
                ActiveWorkKind::TaskExecution => {
                    semantics.action_family = ActionFamily::TaskExecution;
                    semantics.resume_relation = ResumeRelation::ResumeActiveTaskRun;
                    semantics.confidence = 100;
                }
            }
        }
        semantics
    }

    pub(crate) fn conservative_default() -> Self {
        Self {
            request_kind: RequestKind::General,
            evidence_need: EvidenceNeed::None,
            disclosure_surface: DisclosureSurface::Governed,
            execution_preference: ExecutionPreference::AnswerDirect,
            action_family: ActionFamily::Conversation,
            resume_relation: ResumeRelation::IndependentTurn,
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
            resume_relation: ResumeRelation::IndependentTurn,
            confidence: 100,
        }
    }
}

fn looks_like_cancel_active_action_request(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    trimmed.contains("取消")
        || trimmed.contains("别配了")
        || trimmed.contains("先别")
        || trimmed.contains("算了")
        || lower.contains("cancel")
        || lower.contains("stop this")
        || lower.contains("never mind")
}

impl From<ActiveWorkKind> for ActionFamily {
    fn from(value: ActiveWorkKind) -> Self {
        match value {
            ActiveWorkKind::InteractiveAction => ActionFamily::ActiveAction,
            ActiveWorkKind::TaskExecution => ActionFamily::TaskExecution,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiler_defaults_plain_user_turn_to_conversation() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "你好", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
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
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_marks_active_task_execution_work_as_resume_relation() {
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
            has_tools: true,
            active_work: Some(&active_work),
        });

        assert_eq!(semantics.action_family, ActionFamily::TaskExecution);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::ResumeActiveTaskRun
        );
        assert_eq!(semantics.evidence_need, EvidenceNeed::HostTool);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::ToolFirst
        );
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn compiler_marks_explicit_cancel_for_active_action_as_deny_or_cancel() {
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
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            active_work: Some(&active_work),
        });

        assert_eq!(semantics.action_family, ActionFamily::ActiveAction);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::DenyOrCancelActiveAction
        );
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 90);
    }

    #[test]
    fn compiler_keeps_system_ingress_out_of_resume_path() {
        let msg = PcMsg::new_system("self_runtime", "chat-1", "idle tick").expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn compiler_does_not_resume_from_execution_state_projection_without_active_work() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            active_work: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
        assert_eq!(semantics.evidence_need, EvidenceNeed::None);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::AnswerDirect
        );
        assert_eq!(semantics.confidence, 40);
    }

    #[test]
    fn compiler_does_not_fall_back_to_execution_state_when_active_work_is_present_but_mismatched() {
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
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            active_work: Some(&active_work),
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
    }
}
