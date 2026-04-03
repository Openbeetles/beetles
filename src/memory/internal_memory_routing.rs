//! 内部记忆写入路由：由 LLM 决定这一轮是否刷新 self_model / private_docs / private_garden。
//! Internal routing for self-owned memory layers.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, Message, ToolChoicePolicy};
use crate::orchestrator::PressureLevel;
use crate::util::{scrub_credentials, truncate_content_to_max};
use std::borrow::Cow;
use std::fmt::Write as _;

use super::{
    llm_json::{
        get_object_bool, get_object_string_list, get_optional_object_text, parse_llm_json_payload,
        LlmJsonPayload,
    },
    memory_policy, normalize_private_garden_doc_path, render_execution_state_block,
    render_internal_memory_topology_block, render_private_memory_boundary_block,
    render_shared_factual_plane_block, ExecutionState, InternalMemoryLayerFocus,
    InternalMemoryRoutingPolicy, LongTermMemoryStore, MemoryProfile, PrivateDocWorkspace,
    PrivateGardenDocRecord, SelfModel, SessionMessage,
};

pub const INTERNAL_MEMORY_ROUTING_SYSTEM_PROMPT: &str = "You decide whether a persistent embodied AI assistant should refresh each private internal memory layer after the latest turn. Return JSON only: either null, or one object with boolean fields refresh_self_model, refresh_private_docs, refresh_private_garden, plus optional self_model_intent, private_docs_intent, private_garden_intent, self_model_sources, private_docs_sources, and private_garden_cleanup_paths. Choose true only when that layer should be rewritten now. This router governs private layers only; durable objective facts remain in the shared factual plane. If a layer is true, provide a short intent describing what that layer should capture so downstream writers avoid overlap. self_model_sources and private_docs_sources are optional short descriptors of material being distilled or promoted, such as private_docs.inner_journal or private_garden:journal/current.md. private_garden_cleanup_paths are optional garden-relative document paths that can be deleted after successful upstream promotion. self_model is for durable private continuity and stance. private_docs is for compact governed subjective docs. private_garden is for free-form self-owned drafts, organization, and exploratory internal work. Use self-state pressure and current workspace shape to avoid unnecessary writes. If nothing should change, return null.";
const ROUTING_INTENT_MAX_CHARS: usize = 160;
const ROUTING_SOURCE_MAX_CHARS: usize = 96;
const ROUTING_MAX_SOURCES_PER_LAYER: usize = 4;
const ROUTING_MAX_GARDEN_CLEANUP_PATHS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InternalMemoryRoutingInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub tool_calls: u32,
    pub now_secs: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InternalMemoryRoutingDecision {
    pub refresh_self_model: bool,
    pub self_model_intent: Option<String>,
    pub self_model_sources: Vec<String>,
    pub refresh_private_docs: bool,
    pub private_docs_intent: Option<String>,
    pub private_docs_sources: Vec<String>,
    pub refresh_private_garden: bool,
    pub private_garden_intent: Option<String>,
    pub private_garden_cleanup_paths: Vec<String>,
}

pub(crate) fn should_route_internal_memory_turn(
    input: InternalMemoryRoutingInput<'_>,
    profile: MemoryProfile,
) -> bool {
    if input.ingress != IngressKind::User || input.channel == "cron" {
        return false;
    }
    if input.pressure != PressureLevel::Normal {
        return false;
    }
    let user = input.user_content.trim();
    let reply = input.reply_content.trim();
    if user.is_empty() || reply.is_empty() {
        return false;
    }
    if input.tool_calls > 0 {
        return true;
    }
    let policy = memory_policy(profile).internal_memory_routing;
    let user_chars = user.chars().count();
    let reply_chars = reply.chars().count();
    let combined_chars = user_chars.saturating_add(reply_chars);
    user_chars >= policy.substantive_user_chars
        || reply_chars >= policy.substantive_reply_chars
        || combined_chars >= policy.substantive_combined_chars
        || user.contains('\n')
        || reply.contains('\n')
}

pub(crate) fn run_internal_memory_routing_with_state(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    long_term_memory_store: &dyn LongTermMemoryStore,
    input: InternalMemoryRoutingInput<'_>,
    profile: MemoryProfile,
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    recent_messages: &[SessionMessage],
) -> Result<Option<InternalMemoryRoutingDecision>> {
    if !should_route_internal_memory_turn(input, profile) {
        return Ok(None);
    }
    let policy = memory_policy(profile).internal_memory_routing;
    let recent = internal_memory_recent_window(recent_messages, policy.recent_message_count);
    let routing_input = build_internal_memory_routing_input(
        summary_text,
        execution_state,
        render_shared_factual_plane_block(
            long_term_memory_store,
            input.chat_id,
            summary_text,
            recent,
            policy.grounding_max_len,
            profile,
        )
        .as_deref(),
        self_model,
        private_workspace,
        private_garden_docs,
        recent,
        input.now_secs,
        profile,
        policy,
    );
    let messages = [Message {
        role: Cow::Borrowed("user"),
        content: routing_input,
    }];
    let response = llm.chat(
        http,
        INTERNAL_MEMORY_ROUTING_SYSTEM_PROMPT,
        &messages,
        None,
        ToolChoicePolicy::Auto,
    )?;
    Ok(parse_internal_memory_routing_response(
        response.content.trim(),
    ))
}

fn internal_memory_recent_window(recent: &[SessionMessage], limit: usize) -> &[SessionMessage] {
    let start = recent.len().saturating_sub(limit);
    &recent[start..]
}

fn build_internal_memory_routing_input(
    summary_text: Option<&str>,
    execution_state: Option<&ExecutionState>,
    shared_factual_block: Option<&str>,
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    recent: &[SessionMessage],
    now_secs: u64,
    profile: MemoryProfile,
    policy: InternalMemoryRoutingPolicy,
) -> String {
    let mut input = String::with_capacity(3072);
    if let Some(topology_text) = render_internal_memory_topology_block(
        self_model,
        private_workspace,
        private_garden_docs,
        now_secs,
        profile,
        InternalMemoryLayerFocus::Router,
        policy.grounding_max_len.saturating_mul(2),
    ) {
        input.push_str(topology_text.trim());
        input.push_str("\n\n");
    }
    input.push_str("## Shared Grounding\n");
    if let Some(summary_text) = summary_text.map(str::trim).filter(|text| !text.is_empty()) {
        let summary = truncate_content_to_max(summary_text, policy.grounding_max_len);
        let _ = writeln!(input, "Summary: {}", scrub_credentials(summary.as_ref()));
    } else {
        input.push_str("Summary: \n");
    }
    if let Some(block) = execution_state
        .and_then(|state| render_execution_state_block(state, policy.grounding_max_len))
    {
        input.push_str(block.trim());
        input.push('\n');
    }
    if let Some(shared_factual_block) = shared_factual_block {
        input.push('\n');
        input.push_str(shared_factual_block.trim());
        input.push('\n');
    }
    if let Some(block) = render_private_memory_boundary_block(
        "internal_memory_router",
        "deciding whether private layers need refresh while leaving objective facts in the shared plane",
        policy.grounding_max_len,
    ) {
        input.push('\n');
        input.push_str(block.trim());
        input.push('\n');
    }
    input.push_str("\n## Recent Transcript\n");
    input.push_str(&build_internal_memory_routing_transcript(recent, policy));
    input.push_str("\n## Routing Rules\n");
    input.push_str("- Prefer false when a layer would remain effectively unchanged.\n");
    input.push_str("- Choose self_model for durable private continuity or stance shifts.\n");
    input.push_str(
        "- Choose private_docs for compact governed inward docs that should stay load-bearing.\n",
    );
    input.push_str("- Choose private_garden for exploratory notes, reorganization, temporary drafts, or self-owned workspace cleanup.\n");
    input.push_str("- When a layer is true, give it a short intent that clarifies what belongs there and therefore should stay out of the other layers.\n");
    input.push_str("- If self_model or private_docs is distilling material from another internal layer, include short source descriptors so downstream writers know what is being promoted.\n");
    input.push_str("- If a garden document becomes redundant after successful upstream promotion, include its garden-relative path in private_garden_cleanup_paths.\n");
    input.push_str("- Multiple true values are allowed only when multiple layers genuinely need different updates.\n");
    input
}

fn build_internal_memory_routing_transcript(
    recent: &[SessionMessage],
    policy: InternalMemoryRoutingPolicy,
) -> String {
    let mut transcript = String::with_capacity(1024);
    for message in recent {
        let preview = truncate_content_to_max(&message.content, policy.transcript_preview_chars);
        let _ = writeln!(
            transcript,
            "{}: {}",
            message.role.to_uppercase(),
            scrub_credentials(preview.as_ref())
        );
    }
    transcript
}

fn parse_internal_memory_routing_response(raw: &str) -> Option<InternalMemoryRoutingDecision> {
    let LlmJsonPayload::Value(value) = parse_llm_json_payload(raw) else {
        return None;
    };
    let parsed = value.as_object()?;
    let refresh_self_model = get_object_bool(parsed, "refresh_self_model").unwrap_or(false);
    let refresh_private_docs = get_object_bool(parsed, "refresh_private_docs").unwrap_or(false);
    let refresh_private_garden = get_object_bool(parsed, "refresh_private_garden").unwrap_or(false);
    let decision = InternalMemoryRoutingDecision {
        refresh_self_model,
        self_model_intent: normalize_routing_intent(
            get_optional_object_text(parsed, "self_model_intent"),
            refresh_self_model,
        ),
        self_model_sources: normalize_routing_sources(
            get_object_string_list(parsed, "self_model_sources"),
            refresh_self_model,
        ),
        refresh_private_docs,
        private_docs_intent: normalize_routing_intent(
            get_optional_object_text(parsed, "private_docs_intent"),
            refresh_private_docs,
        ),
        private_docs_sources: normalize_routing_sources(
            get_object_string_list(parsed, "private_docs_sources"),
            refresh_private_docs,
        ),
        refresh_private_garden,
        private_garden_intent: normalize_routing_intent(
            get_optional_object_text(parsed, "private_garden_intent"),
            refresh_private_garden,
        ),
        private_garden_cleanup_paths: normalize_private_garden_cleanup_paths(
            get_object_string_list(parsed, "private_garden_cleanup_paths"),
        ),
    };
    (decision.refresh_self_model
        || decision.refresh_private_docs
        || decision.refresh_private_garden)
        .then_some(decision)
}

fn normalize_routing_intent(raw: Option<String>, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    let raw = raw?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_content_to_max(trimmed, ROUTING_INTENT_MAX_CHARS).into_owned())
}

fn normalize_routing_sources(raw: Vec<String>, enabled: bool) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    let mut normalized = Vec::new();
    for source in raw {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            continue;
        }
        let source = truncate_content_to_max(trimmed, ROUTING_SOURCE_MAX_CHARS).into_owned();
        if normalized.contains(&source) {
            continue;
        }
        normalized.push(source);
        if normalized.len() >= ROUTING_MAX_SOURCES_PER_LAYER {
            break;
        }
    }
    normalized
}

fn normalize_private_garden_cleanup_paths(raw: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for path in raw {
        let Ok(path) = normalize_private_garden_doc_path(&path) else {
            continue;
        };
        if normalized.contains(&path) {
            continue;
        }
        normalized.push(path);
        if normalized.len() >= ROUTING_MAX_GARDEN_CLEANUP_PATHS {
            break;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PrivateGardenDocRecord, SelfModel};
    use serde_json::json;

    #[test]
    fn routing_parser_returns_none_for_empty_work() {
        assert!(parse_internal_memory_routing_response("null").is_none());
        assert!(parse_internal_memory_routing_response("{}").is_none());
    }

    #[test]
    fn routing_parser_keeps_true_targets() {
        let parsed = parse_internal_memory_routing_response(
            r#"{"refresh_self_model":true,"self_model_intent":"沉淀最近形成的持续自我定位","self_model_sources":["private_docs.inner_journal","private_garden:journal/current.md"],"refresh_private_docs":false,"private_docs_intent":"should drop","private_docs_sources":["private_garden:notes/skip.md"],"refresh_private_garden":true,"private_garden_intent":"把当前草稿整理成更稳定的目录结构","private_garden_cleanup_paths":["journal/current.md","../escape","journal/current.md"]}"#,
        )
        .unwrap();

        assert!(parsed.refresh_self_model);
        assert!(!parsed.refresh_private_docs);
        assert!(parsed.refresh_private_garden);
        assert_eq!(
            parsed.self_model_intent.as_deref(),
            Some("沉淀最近形成的持续自我定位")
        );
        assert_eq!(
            parsed.self_model_sources,
            vec![
                "private_docs.inner_journal".to_string(),
                "private_garden:journal/current.md".to_string()
            ]
        );
        assert!(parsed.private_docs_intent.is_none());
        assert!(parsed.private_docs_sources.is_empty());
        assert_eq!(
            parsed.private_garden_intent.as_deref(),
            Some("把当前草稿整理成更稳定的目录结构")
        );
        assert_eq!(
            parsed.private_garden_cleanup_paths,
            vec!["journal/current.md".to_string()]
        );
    }

    #[test]
    fn routing_parser_coerces_nested_fields() {
        let raw = json!({
            "refresh_self_model": "true",
            "self_model_intent": { "intent": "stabilize self stance" },
            "self_model_sources": [{ "target": "private_docs.inner_journal" }],
            "refresh_private_docs": 1,
            "private_docs_intent": ["rewrite workspace"],
            "private_docs_sources": "private_garden:journal/today.md",
            "refresh_private_garden": { "enabled": false },
            "private_garden_intent": "ignored",
            "private_garden_cleanup_paths": [{ "path": "journal/today.md" }]
        })
        .to_string();
        let parsed = parse_internal_memory_routing_response(&raw).unwrap();
        assert!(parsed.refresh_self_model);
        assert!(parsed.refresh_private_docs);
        assert!(!parsed.refresh_private_garden);
        assert!(parsed
            .self_model_intent
            .unwrap()
            .contains("intent: stabilize self stance"));
        assert_eq!(
            parsed.private_docs_intent.as_deref(),
            Some("rewrite workspace")
        );
        assert_eq!(
            parsed.self_model_sources,
            vec!["private_docs.inner_journal".to_string()]
        );
        assert_eq!(
            parsed.private_docs_sources,
            vec!["private_garden:journal/today.md".to_string()]
        );
        assert_eq!(
            parsed.private_garden_cleanup_paths,
            vec!["journal/today.md".to_string()]
        );
    }

    #[test]
    fn routing_input_includes_shape_and_self_state() {
        let rendered = build_internal_memory_routing_input(
            Some("summary"),
            None,
            None,
            Some(&SelfModel {
                continuity_anchor: "anchor".to_string(),
                self_narrative: String::new(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 3,
            }),
            None,
            &[PrivateGardenDocRecord {
                path: "journal/now.md".to_string(),
                updated_at: 4,
                revision: 1,
                bytes: 24,
                preview: "preview".to_string(),
            }],
            &[],
            10,
            MemoryProfile::Embedded,
            memory_policy(MemoryProfile::Embedded).internal_memory_routing,
        );

        assert!(rendered.contains("## Internal Memory Topology"));
        assert!(rendered.contains("Pressure:"));
        assert!(rendered.contains("self_model: anchor=anchor"));
        assert!(rendered.contains("private_garden:"));
        assert!(rendered.contains("1/16 docs"));
    }
}
