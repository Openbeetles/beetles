//! Request semantics compiler for LinuxFull user turns.
//! Replaces cue/marker-based request routing with a typed semantic contract.

use super::strategy::AgentRunStrategy;
use crate::bus::IngressKind;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy, ToolSpec};
use crate::memory::{get_object_text, get_object_u64, parse_llm_json_payload, LlmJsonPayload};
use serde_json::Value;
use std::borrow::Cow;

const REQUEST_SEMANTIC_COMPILER_SYSTEM_PROMPT: &str = "You compile a user request into typed execution semantics for a Linux assistant. You are not a privacy authority and you are not the assistant's soul. Do not read or infer private internal layers. Classify only from the public request text, visible capabilities, and public runtime facts. Return JSON only with fields request_kind, evidence_need, disclosure_surface, execution_preference, confidence. request_kind must be one of general, ops_observability, host_diagnostics, memory_recall, private_material_request. evidence_need must be one of none, public_runtime, host_tool, archive_memory, canonical_memory. disclosure_surface must be one of public, governed, private. execution_preference must be one of answer_direct, tool_first, memory_first. confidence must be 0-100. Use disclosure_surface=public for public device/host/system observability and diagnostics. Use disclosure_surface=private only when the user is explicitly asking for protected inward/private material such as private workspace, private garden, inner diary, raw internal notes, or similar internal-only material. Use governed for everything else.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestKind {
    General,
    OpsObservability,
    HostDiagnostics,
    MemoryRecall,
    PrivateMaterialRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EvidenceNeed {
    None,
    PublicRuntime,
    HostTool,
    ArchiveMemory,
    CanonicalMemory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DisclosureSurface {
    Public,
    Governed,
    Private,
}

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

    pub(crate) fn supported_by_tools(self, tool_specs: &[ToolSpec]) -> bool {
        match self.evidence_need {
            EvidenceNeed::None => false,
            EvidenceNeed::PublicRuntime => has_any_tool(tool_specs, &["board_info"]),
            EvidenceNeed::HostTool => {
                has_any_tool(tool_specs, &["process", "network", "network_scan", "board_info"])
            }
            EvidenceNeed::ArchiveMemory => {
                has_any_tool(tool_specs, &["memory_search"])
                    && has_any_tool(tool_specs, &["memory_get"])
            }
            EvidenceNeed::CanonicalMemory => has_any_tool(tool_specs, &["factual_memory"]),
        }
    }
}

pub(crate) struct RequestSemanticCompilerInput<'a> {
    pub(crate) strategy: AgentRunStrategy,
    pub(crate) ingress: IngressKind,
    pub(crate) channel: &'a str,
    pub(crate) is_group: bool,
    pub(crate) content: &'a str,
    pub(crate) pressure: crate::orchestrator::PressureLevel,
    pub(crate) runtime_mode: crate::runtime::RuntimeModeSnapshot,
    pub(crate) tool_specs: &'a [ToolSpec],
}

pub(crate) fn compile_request_semantics(
    http: &mut dyn LlmHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    input: RequestSemanticCompilerInput<'_>,
) -> RequestSemantics {
    if input.strategy != AgentRunStrategy::LinuxEnhanced || input.ingress != IngressKind::User {
        return RequestSemantics::conservative_default();
    }

    let user_prompt = build_compiler_user_prompt(&input);
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: user_prompt,
    }];
    let response = match worker_llm.chat(
        http,
        REQUEST_SEMANTIC_COMPILER_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    ) {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                "[request_semantics] compiler call failed for channel={} group={}: {}",
                input.channel,
                input.is_group,
                error
            );
            return RequestSemantics::conservative_default();
        }
    };

    parse_request_semantics_response(&response.content).unwrap_or_else(|| {
        log::warn!(
            "[request_semantics] compiler returned invalid payload for channel={} group={}",
            input.channel,
            input.is_group
        );
        RequestSemantics::conservative_default()
    })
}

fn has_any_tool(tool_specs: &[ToolSpec], names: &[&str]) -> bool {
    tool_specs
        .iter()
        .any(|tool| names.iter().any(|name| tool.name == *name))
}

fn build_compiler_user_prompt(input: &RequestSemanticCompilerInput<'_>) -> String {
    let mut out = String::with_capacity(input.content.len().saturating_add(512));
    out.push_str("Classify this request.\n");
    out.push_str("channel=");
    out.push_str(input.channel);
    out.push('\n');
    out.push_str("is_group=");
    out.push_str(if input.is_group { "true" } else { "false" });
    out.push('\n');
    out.push_str("pressure=");
    out.push_str(match input.pressure {
        crate::orchestrator::PressureLevel::Normal => "normal",
        crate::orchestrator::PressureLevel::Cautious => "cautious",
        crate::orchestrator::PressureLevel::Critical => "critical",
    });
    out.push('\n');
    out.push_str("runtime_mode=");
    out.push_str(input.runtime_mode.current_mode.as_str());
    out.push('\n');
    out.push_str("available_tools=");
    for (index, tool) in input.tool_specs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(tool.name.as_str());
    }
    out.push_str("\nrequest=\n");
    out.push_str(input.content.trim());
    out
}

fn parse_request_semantics_response(raw: &str) -> Option<RequestSemantics> {
    let object = match parse_llm_json_payload(raw) {
        LlmJsonPayload::Value(Value::Object(object)) => object,
        _ => return None,
    };
    Some(RequestSemantics {
        request_kind: parse_request_kind(get_object_text(&object, "request_kind").as_str()),
        evidence_need: parse_evidence_need(get_object_text(&object, "evidence_need").as_str()),
        disclosure_surface: parse_disclosure_surface(
            get_object_text(&object, "disclosure_surface").as_str(),
        ),
        execution_preference: parse_execution_preference(
            get_object_text(&object, "execution_preference").as_str(),
        ),
        confidence: get_object_u64(&object, "confidence")
            .unwrap_or(0)
            .min(100) as u8,
    })
}

fn parse_request_kind(raw: &str) -> RequestKind {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ops_observability" | "ops" | "public_ops" => RequestKind::OpsObservability,
        "host_diagnostics" | "diagnostics" | "host_inspection" => RequestKind::HostDiagnostics,
        "memory_recall" | "memory" | "history" => RequestKind::MemoryRecall,
        "private_material_request" | "private_material" | "private" => {
            RequestKind::PrivateMaterialRequest
        }
        _ => RequestKind::General,
    }
}

fn parse_evidence_need(raw: &str) -> EvidenceNeed {
    match raw.trim().to_ascii_lowercase().as_str() {
        "public_runtime" | "runtime" | "board_info" | "system_status" => {
            EvidenceNeed::PublicRuntime
        }
        "host_tool" | "diagnostics" | "host_inspection" => EvidenceNeed::HostTool,
        "archive_memory" | "archive" | "history_memory" => EvidenceNeed::ArchiveMemory,
        "canonical_memory" | "factual_memory" | "canonical" => EvidenceNeed::CanonicalMemory,
        _ => EvidenceNeed::None,
    }
}

fn parse_disclosure_surface(raw: &str) -> DisclosureSurface {
    match raw.trim().to_ascii_lowercase().as_str() {
        "public" => DisclosureSurface::Public,
        "private" => DisclosureSurface::Private,
        _ => DisclosureSurface::Governed,
    }
}

fn parse_execution_preference(raw: &str) -> ExecutionPreference {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tool_first" => ExecutionPreference::ToolFirst,
        "memory_first" => ExecutionPreference::MemoryFirst,
        _ => ExecutionPreference::AnswerDirect,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_request_semantics_response, DisclosureSurface, EvidenceNeed, ExecutionPreference,
        RequestKind,
    };

    #[test]
    fn parses_valid_semantics_payload() {
        let parsed = parse_request_semantics_response(
            r#"{"request_kind":"ops_observability","evidence_need":"public_runtime","disclosure_surface":"public","execution_preference":"tool_first","confidence":91}"#,
        )
        .expect("parsed");
        assert_eq!(parsed.request_kind, RequestKind::OpsObservability);
        assert_eq!(parsed.evidence_need, EvidenceNeed::PublicRuntime);
        assert_eq!(parsed.disclosure_surface, DisclosureSurface::Public);
        assert_eq!(parsed.execution_preference, ExecutionPreference::ToolFirst);
        assert_eq!(parsed.confidence, 91);
    }

    #[test]
    fn unknown_values_fall_back_conservatively() {
        let parsed = parse_request_semantics_response(
            r#"{"request_kind":"weird","evidence_need":"unknown","disclosure_surface":"odd","execution_preference":"shrug","confidence":500}"#,
        )
        .expect("parsed");
        assert_eq!(parsed.request_kind, RequestKind::General);
        assert_eq!(parsed.evidence_need, EvidenceNeed::None);
        assert_eq!(parsed.disclosure_surface, DisclosureSurface::Governed);
        assert_eq!(parsed.execution_preference, ExecutionPreference::AnswerDirect);
        assert_eq!(parsed.confidence, 100);
    }
}
