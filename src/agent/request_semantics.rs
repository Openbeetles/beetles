//! Typed request semantics carried through agent telemetry/governance.
//! 预回合请求语义编译结果：为后续 routing / governance / telemetry 提供正式输入。

use super::strategy::AgentRunStrategy;
use crate::bus::{IngressKind, PcMsg};
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::memory::{
    get_object_text, get_object_u64, parse_llm_json_payload, render_execution_state_block,
    ExecutionState, LlmJsonPayload,
};
use serde_json::Value;
use std::borrow::Cow;

const REQUEST_SEMANTICS_PROBE_SYSTEM_PROMPT: &str = concat!(
    "## Request Semantics Probe\n",
    "Classify the current user turn for the agent control plane. Return JSON only with fields ",
    "request_kind, evidence_need, disclosure_surface, execution_preference, action_family, resume_relation, confidence.\n",
    "Allowed request_kind: general, ops_observability, host_diagnostics, memory_recall, private_material_request.\n",
    "Allowed evidence_need: none, public_runtime, host_tool, archive_memory, canonical_memory.\n",
    "Allowed disclosure_surface: public, governed, private.\n",
    "Allowed execution_preference: answer_direct, tool_first, memory_first.\n",
    "Allowed action_family: conversation, action_request, active_action.\n",
    "Allowed resume_relation: independent_turn, resume_active_action, confirm_active_action, supply_active_action_input, deny_or_cancel_active_action, switch_to_new_request.\n",
    "Use action_request only when the user is asking the agent to carry out or continue a concrete external/system/workspace action, not merely answer a question.\n",
    "Use active_action only when there is an active execution state and the current turn is still part of that same action.\n",
    "Use confirm_active_action when the user is confirming or approving the active action without adding material new facts.\n",
    "Use supply_active_action_input when the user is providing concrete missing input, credentials, parameters, or values needed to continue the active action.\n",
    "Use deny_or_cancel_active_action when the user is explicitly stopping, rejecting, or cancelling the currently active action instead of continuing it.\n",
    "Use switch_to_new_request when there is an active action but the user is redirecting the agent to a different concrete request that should supersede the current one.\n",
    "Use public_runtime when the answer must be grounded in current observable runtime or host state.\n",
    "Use host_tool when live tools are needed.\n",
    "If uncertain, prefer conversation + answer_direct with lower confidence.\n",
    "Do not use cue words. Judge the overall intent and execution shape of the request.\n",
);
const REQUEST_SEMANTICS_PROBE_CONFIDENCE_FLOOR: u8 = 75;

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
    ActionRequest,
    ActiveAction,
    TaskExecution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResumeRelation {
    IndependentTurn,
    ConfirmActiveAction,
    SupplyActiveActionInput,
    DenyOrCancelActiveAction,
    SwitchToNewRequest,
    ResumeActiveAction,
    ResumeActiveTaskRun,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestSemanticsCompileInput<'a> {
    pub(crate) msg: &'a PcMsg,
    pub(crate) has_tools: bool,
    pub(crate) has_active_task_run: bool,
    pub(crate) active_execution_state: Option<&'a ExecutionState>,
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
    pub(crate) fn compile_or_probe_for_turn(
        http: &mut dyn LlmHttpClient,
        llm: &(dyn LlmClient + Send + Sync),
        strategy: AgentRunStrategy,
        input: RequestSemanticsCompileInput<'_>,
    ) -> Self {
        let deterministic = Self::compile_for_turn(input);
        if !should_run_request_semantics_probe(strategy, input, deterministic) {
            return deterministic;
        }
        probe_request_semantics(http, llm, input).unwrap_or(deterministic)
    }

    pub(crate) fn compile_for_turn(input: RequestSemanticsCompileInput<'_>) -> Self {
        let mut semantics = Self::conservative_default();
        semantics.confidence = if input.msg.ingress == IngressKind::User {
            40
        } else {
            100
        };
        if input.has_active_task_run {
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
            semantics.action_family = ActionFamily::TaskExecution;
            semantics.resume_relation = ResumeRelation::ResumeActiveTaskRun;
            semantics.confidence = 100;
        } else if input.active_execution_state.is_some_and(|state| {
            crate::memory::should_resume_active_execution_state(state, &input.msg.content)
        }) {
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
            semantics.action_family = ActionFamily::ActiveAction;
            semantics.resume_relation = ResumeRelation::ResumeActiveAction;
            semantics.confidence = 75;
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

    pub(crate) fn is_public_surface(self) -> bool {
        matches!(self.disclosure_surface, DisclosureSurface::Public)
    }
}

fn should_run_request_semantics_probe(
    strategy: AgentRunStrategy,
    input: RequestSemanticsCompileInput<'_>,
    deterministic: RequestSemantics,
) -> bool {
    strategy == AgentRunStrategy::LinuxEnhanced
        && input.has_tools
        && input.msg.ingress == IngressKind::User
        && !input.msg.is_group
        && !input.has_active_task_run
        && matches!(
            (deterministic.action_family, deterministic.resume_relation),
            (ActionFamily::Conversation, ResumeRelation::IndependentTurn)
                | (
                    ActionFamily::ActiveAction,
                    ResumeRelation::ResumeActiveAction
                )
        )
}

fn probe_request_semantics(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    input: RequestSemanticsCompileInput<'_>,
) -> Option<RequestSemantics> {
    let mut content = String::with_capacity(512);
    let _ = std::fmt::Write::write_fmt(
        &mut content,
        format_args!(
            "Channel: {}\nChat: {}\n",
            input.msg.channel, input.msg.chat_id
        ),
    );
    if let Some(state_block) = input
        .active_execution_state
        .and_then(|state| render_execution_state_block(state, 320))
    {
        content.push_str(&state_block);
        content.push_str("\n\n");
    }
    content.push_str("User request:\n");
    content.push_str(input.msg.content.trim());
    let probe_messages = [Message {
        role: Cow::Borrowed("user"),
        content,
    }];
    let t0 = crate::metrics::record_llm_call_start();
    let response = llm.chat(
        http,
        REQUEST_SEMANTICS_PROBE_SYSTEM_PROMPT,
        &probe_messages,
        None,
        ToolChoicePolicy::Auto,
    );
    crate::metrics::record_llm_call_end(t0);
    let Ok(response) = response else {
        crate::metrics::record_llm_error();
        crate::metrics::record_error_by_stage("request_semantics_probe");
        return None;
    };
    parse_request_semantics_probe_response(&response.content)
        .filter(|semantics| semantics.confidence >= REQUEST_SEMANTICS_PROBE_CONFIDENCE_FLOOR)
}

fn parse_request_semantics_probe_response(raw: &str) -> Option<RequestSemantics> {
    let payload = parse_llm_json_payload(raw);
    let LlmJsonPayload::Value(Value::Object(object)) = payload else {
        return None;
    };
    let request_kind = parse_request_kind(&get_object_text(&object, "request_kind"))?;
    let evidence_need = parse_evidence_need(&get_object_text(&object, "evidence_need"))?;
    let disclosure_surface =
        parse_disclosure_surface(&get_object_text(&object, "disclosure_surface"))?;
    let execution_preference =
        parse_execution_preference(&get_object_text(&object, "execution_preference"))?;
    let action_family = parse_action_family(&get_object_text(&object, "action_family"))?;
    let resume_relation = parse_resume_relation(&get_object_text(&object, "resume_relation"))
        .unwrap_or(ResumeRelation::IndependentTurn);
    let confidence = get_object_u64(&object, "confidence")
        .unwrap_or(u64::from(REQUEST_SEMANTICS_PROBE_CONFIDENCE_FLOOR))
        .min(100) as u8;
    Some(RequestSemantics {
        request_kind,
        evidence_need,
        disclosure_surface,
        execution_preference,
        action_family,
        resume_relation,
        confidence,
    })
}

fn parse_request_kind(value: &str) -> Option<RequestKind> {
    match value.trim() {
        "general" => Some(RequestKind::General),
        "ops_observability" => Some(RequestKind::OpsObservability),
        "host_diagnostics" => Some(RequestKind::HostDiagnostics),
        "memory_recall" => Some(RequestKind::MemoryRecall),
        "private_material_request" => Some(RequestKind::PrivateMaterialRequest),
        _ => None,
    }
}

fn parse_evidence_need(value: &str) -> Option<EvidenceNeed> {
    match value.trim() {
        "none" => Some(EvidenceNeed::None),
        "public_runtime" => Some(EvidenceNeed::PublicRuntime),
        "host_tool" => Some(EvidenceNeed::HostTool),
        "archive_memory" => Some(EvidenceNeed::ArchiveMemory),
        "canonical_memory" => Some(EvidenceNeed::CanonicalMemory),
        _ => None,
    }
}

fn parse_disclosure_surface(value: &str) -> Option<DisclosureSurface> {
    match value.trim() {
        "public" => Some(DisclosureSurface::Public),
        "governed" => Some(DisclosureSurface::Governed),
        "private" => Some(DisclosureSurface::Private),
        _ => None,
    }
}

fn parse_execution_preference(value: &str) -> Option<ExecutionPreference> {
    match value.trim() {
        "answer_direct" => Some(ExecutionPreference::AnswerDirect),
        "tool_first" => Some(ExecutionPreference::ToolFirst),
        "memory_first" => Some(ExecutionPreference::MemoryFirst),
        _ => None,
    }
}

fn parse_action_family(value: &str) -> Option<ActionFamily> {
    match value.trim() {
        "conversation" => Some(ActionFamily::Conversation),
        "action_request" => Some(ActionFamily::ActionRequest),
        "active_action" => Some(ActionFamily::ActiveAction),
        _ => None,
    }
}

fn parse_resume_relation(value: &str) -> Option<ResumeRelation> {
    match value.trim() {
        "independent_turn" => Some(ResumeRelation::IndependentTurn),
        "confirm_active_action" => Some(ResumeRelation::ConfirmActiveAction),
        "supply_active_action_input" => Some(ResumeRelation::SupplyActiveActionInput),
        "deny_or_cancel_active_action" => Some(ResumeRelation::DenyOrCancelActiveAction),
        "switch_to_new_request" => Some(ResumeRelation::SwitchToNewRequest),
        "resume_active_action" => Some(ResumeRelation::ResumeActiveAction),
        "resume_active_task_run" => Some(ResumeRelation::ResumeActiveTaskRun),
        _ => None,
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
            has_active_task_run: false,
            active_execution_state: None,
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
    fn compiler_marks_active_task_run_as_resume_relation() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续配置", false).expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            has_active_task_run: true,
            active_execution_state: None,
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
    fn compiler_keeps_system_ingress_out_of_resume_path() {
        let msg = PcMsg::new_system("self_runtime", "chat-1", "idle tick").expect("message");
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            has_active_task_run: false,
            active_execution_state: None,
        });

        assert_eq!(semantics.action_family, ActionFamily::Conversation);
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
        assert_eq!(semantics.confidence, 100);
    }

    #[test]
    fn compiler_marks_active_execution_state_as_resume_relation() {
        let msg = PcMsg::new_inbound("qq_channel", "chat-1", "继续", false).expect("message");
        let state = ExecutionState {
            status: crate::memory::ExecutionStatus::Active,
            goal: "配置 QQ 邮箱账户".to_string(),
            next_action: "补认证信息并继续配置".to_string(),
            updated_at: 7,
            ..ExecutionState::default()
        };
        let semantics = RequestSemantics::compile_for_turn(RequestSemanticsCompileInput {
            msg: &msg,
            has_tools: true,
            has_active_task_run: false,
            active_execution_state: Some(&state),
        });

        assert_eq!(semantics.action_family, ActionFamily::ActiveAction);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::ResumeActiveAction
        );
        assert_eq!(semantics.evidence_need, EvidenceNeed::HostTool);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::ToolFirst
        );
        assert_eq!(semantics.confidence, 75);
    }

    #[test]
    fn parses_probe_response_into_action_request_semantics() {
        let semantics = parse_request_semantics_probe_response(
            r#"{"request_kind":"general","evidence_need":"host_tool","disclosure_surface":"governed","execution_preference":"tool_first","action_family":"action_request","confidence":88}"#,
        )
        .expect("semantics");

        assert_eq!(semantics.request_kind, RequestKind::General);
        assert_eq!(semantics.evidence_need, EvidenceNeed::HostTool);
        assert_eq!(semantics.disclosure_surface, DisclosureSurface::Governed);
        assert_eq!(
            semantics.execution_preference,
            ExecutionPreference::ToolFirst
        );
        assert_eq!(semantics.action_family, ActionFamily::ActionRequest);
        assert_eq!(semantics.resume_relation, ResumeRelation::IndependentTurn);
        assert_eq!(semantics.confidence, 88);
    }

    #[test]
    fn parses_probe_response_into_active_action_supply_relation() {
        let semantics = parse_request_semantics_probe_response(
            r#"{"request_kind":"general","evidence_need":"host_tool","disclosure_surface":"governed","execution_preference":"tool_first","action_family":"active_action","resume_relation":"supply_active_action_input","confidence":91}"#,
        )
        .expect("semantics");

        assert_eq!(semantics.action_family, ActionFamily::ActiveAction);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::SupplyActiveActionInput
        );
        assert_eq!(semantics.confidence, 91);
    }

    #[test]
    fn parses_probe_response_into_cancel_active_action_relation() {
        let semantics = parse_request_semantics_probe_response(
            r#"{"request_kind":"general","evidence_need":"none","disclosure_surface":"governed","execution_preference":"answer_direct","action_family":"active_action","resume_relation":"deny_or_cancel_active_action","confidence":90}"#,
        )
        .expect("semantics");

        assert_eq!(semantics.action_family, ActionFamily::ActiveAction);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::DenyOrCancelActiveAction
        );
        assert_eq!(semantics.confidence, 90);
    }

    #[test]
    fn parses_probe_response_into_switch_to_new_request_relation() {
        let semantics = parse_request_semantics_probe_response(
            r#"{"request_kind":"general","evidence_need":"host_tool","disclosure_surface":"governed","execution_preference":"tool_first","action_family":"action_request","resume_relation":"switch_to_new_request","confidence":93}"#,
        )
        .expect("semantics");

        assert_eq!(semantics.action_family, ActionFamily::ActionRequest);
        assert_eq!(
            semantics.resume_relation,
            ResumeRelation::SwitchToNewRequest
        );
        assert_eq!(semantics.confidence, 93);
    }
}
