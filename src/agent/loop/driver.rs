use super::*;
use crate::memory::{get_object_text, parse_llm_json_payload, LlmJsonPayload};
use serde_json::Value;
use std::borrow::Cow;

const PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Public Runtime Finalization\nThis turn is on the public_runtime reply surface. Using only the completed tool results and public runtime evidence already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be public_runtime. reply must be a non-empty user-facing answer grounded in the current runtime evidence. Do not greet, do not ask generic follow-up questions, do not mention private/internal mechanisms, do not output progress logs, and do not call tools.";
const GOVERNED_CONVERSATION_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Governed Conversation Finalization\nThis turn is on the governed_conversation reply surface. Using only the completed tool results, governed memory grounding, and current conclusions already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be governed_conversation. reply must be a non-empty user-facing answer. If the current action is blocked pending explicit user input or account choice, reply must be one brief clarification question that lets the user continue. Do not greet, do not output internal logs, do not mention hidden mechanisms, do not describe future execution steps, and do not call tools.";
const PRIVATE_BOUNDARY_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Private Boundary Finalization\nThis turn is on the private_boundary reply surface. Using only the already-governed conclusions, tool evidence, and safe boundary decisions already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be private_boundary. reply must be a non-empty user-facing answer. Do not reveal private source material, raw inner notes, internal-only memory, or hidden mechanisms. If the governed conclusion is that the request cannot be fulfilled, state that boundary clearly and briefly. Do not greet, do not output internal logs, and do not call tools.";

pub(super) fn prepare_system_with_suffix<'a>(
    base: &str,
    suffix: &str,
    scratch: &'a mut String,
) -> &'a str {
    prepare_system_with_two_suffixes(base, suffix, "", scratch)
}

pub(super) fn prepare_system_with_two_suffixes<'a>(
    base: &str,
    first_suffix: &str,
    second_suffix: &str,
    scratch: &'a mut String,
) -> &'a str {
    scratch.clear();
    let required = base
        .len()
        .saturating_add(first_suffix.len())
        .saturating_add(second_suffix.len());
    if scratch.capacity() < required {
        scratch.reserve(required - scratch.capacity());
    }
    scratch.push_str(base);
    scratch.push_str(first_suffix);
    scratch.push_str(second_suffix);
    scratch.as_str()
}

fn prepare_finalization_messages<'a>(
    messages: &'a [Message],
    draft_content: &str,
) -> Cow<'a, [Message]> {
    if draft_content.trim().is_empty() {
        return Cow::Borrowed(messages);
    }
    if messages.last().is_some_and(|message| {
        message.role.as_ref() == "assistant" && message.content.trim() == draft_content.trim()
    }) {
        return Cow::Borrowed(messages);
    }
    let mut finalization_messages = Vec::with_capacity(messages.len().saturating_add(1));
    finalization_messages.extend_from_slice(messages);
    finalization_messages.push(Message {
        role: Cow::Borrowed("assistant"),
        content: draft_content.to_string(),
    });
    Cow::Owned(finalization_messages)
}

pub(super) fn current_turn_scope_start(messages: &[Message], initial_msg_count: usize) -> usize {
    let search_end = initial_msg_count.min(messages.len());
    messages[..search_end]
        .iter()
        .rposition(|message| message.role.as_ref() == "user")
        .unwrap_or(0)
}

fn prepare_request_scoped_finalization_messages<'a>(
    messages: &'a [Message],
    scope_start: usize,
    draft_content: &str,
) -> Cow<'a, [Message]> {
    let scoped_messages = messages.get(scope_start..).unwrap_or(messages);
    if scoped_messages.is_empty() {
        return prepare_finalization_messages(messages, draft_content);
    }
    prepare_finalization_messages(scoped_messages, draft_content)
}

fn structured_finalization_system_suffix(reply_surface: ReplySurface) -> Option<&'static str> {
    match reply_surface {
        ReplySurface::PublicRuntime => Some(PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX),
        ReplySurface::GovernedConversation => {
            Some(GOVERNED_CONVERSATION_FINALIZATION_SYSTEM_SUFFIX)
        }
        ReplySurface::PrivateBoundary => Some(PRIVATE_BOUNDARY_FINALIZATION_SYSTEM_SUFFIX),
        ReplySurface::TaskExecution | ReplySurface::InternalOnly => None,
    }
}

fn parse_surface_finalization_reply(reply_surface: ReplySurface, raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match parse_llm_json_payload(trimmed) {
        LlmJsonPayload::Value(Value::Object(object)) => {
            let reply = get_object_text(&object, "reply");
            if reply.trim().is_empty() {
                return None;
            }
            let surface = get_object_text(&object, "surface");
            if !surface.trim().is_empty() && surface.trim() != reply_surface.as_str() {
                log::warn!(
                    "[reply_surface] finalization surface mismatch expected={} actual={}",
                    reply_surface.as_str(),
                    surface.trim()
                );
            }
            Some(reply.trim().to_string())
        }
        _ => None,
    }
}

pub(super) fn run_surface_finalization_round(
    worker_llm: &(dyn LlmClient + Send + Sync),
    tool_ctx: &mut HttpClientToolContext<'_>,
    system: &str,
    messages: &[Message],
    scope_start: usize,
    reply_surface: ReplySurface,
    draft_content: &str,
    contract_suffix: &str,
    llm_stream: bool,
    latency: &mut WorkerLatency,
    system_scratch: &mut String,
) -> Result<String> {
    let surface_suffix = structured_finalization_system_suffix(reply_surface).ok_or_else(|| {
        crate::error::Error::config(
            "surface_finalization",
            format!("unsupported reply_surface={}", reply_surface.as_str()),
        )
    })?;
    let finalization_system =
        prepare_system_with_two_suffixes(system, surface_suffix, contract_suffix, system_scratch);
    let finalization_messages =
        prepare_request_scoped_finalization_messages(messages, scope_start, draft_content);
    let t0 = metrics::record_llm_call_start();
    let llm_round_start = Instant::now();
    let response = if llm_stream {
        let mut ignore_progress = |_delta: &str, _accumulated: &str| {
            crate::platform::task_wdt::feed_current_task();
        };
        worker_llm.chat_with_progress(
            tool_ctx,
            finalization_system,
            finalization_messages.as_ref(),
            None,
            ToolChoicePolicy::Auto,
            &mut ignore_progress,
        )
    } else {
        worker_llm.chat(
            tool_ctx,
            finalization_system,
            finalization_messages.as_ref(),
            None,
            ToolChoicePolicy::Auto,
        )
    };
    match response {
        Ok(response) => {
            metrics::record_llm_call_end(t0);
            latency.surface_finalize_ms = latency
                .surface_finalize_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            latency.react_rounds = latency.react_rounds.saturating_add(1);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            parse_surface_finalization_reply(reply_surface, &response.content).ok_or_else(|| {
                crate::error::Error::config(
                    "surface_finalization_contract_breach",
                    format!(
                        "reply_surface={} invalid structured finalization payload",
                        reply_surface.as_str()
                    ),
                )
            })
        }
        Err(e) => {
            metrics::record_llm_call_end(t0);
            metrics::record_llm_error();
            metrics::record_error_by_stage("agent_chat");
            latency.surface_finalize_ms = latency
                .surface_finalize_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            Err(e.with_stage("agent_chat"))
        }
    }
}

pub(super) fn recv_next_agent_msg(
    user_inbound_rx: &UserInboundRx,
    system_inbound_rx: &InboundRx,
    recv_timeout: Duration,
    prefer_system_once: bool,
    before_poll: &mut dyn FnMut(),
) -> AgentRecvStatus {
    let poll_slice = recv_timeout.min(Duration::from_millis(INBOUND_POLL_SLICE_MS));
    let deadline = Instant::now() + recv_timeout;
    let mut user_disconnected = false;
    let mut system_disconnected = false;

    loop {
        before_poll();
        if prefer_system_once && !system_disconnected {
            match system_inbound_rx.try_recv() {
                Ok(msg) => return AgentRecvStatus::Message(msg),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => system_disconnected = true,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        if !user_disconnected {
            match user_inbound_rx.try_recv() {
                Ok(msg) => return AgentRecvStatus::Message(msg),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => user_disconnected = true,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        if !system_disconnected {
            match system_inbound_rx.try_recv() {
                Ok(msg) => return AgentRecvStatus::Message(msg),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => system_disconnected = true,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        if user_disconnected && system_disconnected {
            return AgentRecvStatus::Disconnected;
        }
        if Instant::now() >= deadline {
            return AgentRecvStatus::Timeout;
        }

        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(poll_slice);
        before_poll();
        if !user_disconnected {
            match user_inbound_rx.recv_timeout(wait) {
                Ok(msg) => return AgentRecvStatus::Message(msg),
                Err(RecvTimeoutError::Disconnected) => user_disconnected = true,
                Err(RecvTimeoutError::Timeout) => {}
            }
        } else {
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_boundary_has_structured_finalization_prompt() {
        assert!(structured_finalization_system_suffix(ReplySurface::PrivateBoundary).is_some());
        assert!(
            structured_finalization_system_suffix(ReplySurface::GovernedConversation).is_some()
        );
    }

    #[test]
    fn parse_surface_finalization_reply_accepts_private_boundary_json() {
        let raw =
            r#"{"surface":"private_boundary","reply":"这部分属于私域材料，我不能直接公开。"}"#;
        let parsed = parse_surface_finalization_reply(ReplySurface::PrivateBoundary, raw)
            .expect("parsed reply");
        assert_eq!(parsed, "这部分属于私域材料，我不能直接公开。");
    }

    #[test]
    fn parse_surface_finalization_reply_rejects_plain_text_payload() {
        assert!(parse_surface_finalization_reply(
            ReplySurface::GovernedConversation,
            "直接把这段文本当最终答复。"
        )
        .is_none());
    }
}
