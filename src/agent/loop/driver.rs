use super::*;
use crate::memory::{get_object_text, parse_llm_json_payload, LlmJsonPayload};
use serde_json::Value;
use std::borrow::Cow;

const PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Public Runtime Finalization\nThis turn is on the public_runtime reply surface. Using only the completed tool results and public runtime evidence already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be public_runtime. reply must be a non-empty user-facing answer grounded in the current runtime evidence. Do not greet, do not ask generic follow-up questions, do not mention private/internal mechanisms, do not output progress logs, and do not call tools.";

fn collect_recent_assistant_messages<'a>(
    messages: &'a [Message],
    limit: usize,
    out: &mut Vec<&'a str>,
) {
    out.clear();
    if out.capacity() < limit {
        out.reserve(limit - out.capacity());
    }
    for message in messages.iter().rev() {
        if out.len() >= limit {
            break;
        }
        if message.role.as_ref() != "assistant" {
            continue;
        }
        let content = message.content.trim();
        if content.is_empty() || content == "[tool_use]" {
            continue;
        }
        out.push(content);
    }
}

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

fn end_turn_recovery_suffix(followup: &str) -> String {
    let mut out = String::with_capacity(followup.len().saturating_add(32));
    out.push_str("\n\n## EndTurn correction\n");
    out.push_str(followup.trim());
    out
}

fn prepare_final_recovery_messages<'a>(
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
    let mut recovery_messages = Vec::with_capacity(messages.len().saturating_add(1));
    recovery_messages.extend_from_slice(messages);
    recovery_messages.push(Message {
        role: Cow::Borrowed("assistant"),
        content: draft_content.to_string(),
    });
    Cow::Owned(recovery_messages)
}

pub(super) fn resolve_end_turn_followup(ctx: EndTurnFollowupContext<'_>) -> Option<String> {
    if let Some(followup) = final_answer_followup(
        ctx.strategy,
        ctx.recent_tool_round.successful_round,
        ctx.content,
    ) {
        return Some(end_turn_recovery_suffix(&followup));
    }
    let mut recent_assistant_messages = Vec::with_capacity(3);
    collect_recent_assistant_messages(ctx.messages, 3, &mut recent_assistant_messages);
    if ctx.any_tool_used {
        if let Some(followup) =
            repeated_answer_followup(ctx.strategy, &recent_assistant_messages, ctx.content)
        {
            return Some(end_turn_recovery_suffix(followup));
        }
    }
    None
}

pub(super) fn run_final_answer_recovery_round(
    worker_llm: &(dyn LlmClient + Send + Sync),
    tool_ctx: &mut HttpClientToolContext<'_>,
    system: &str,
    messages: &[Message],
    draft_content: &str,
    recovery_suffix: &str,
    llm_stream: bool,
    latency: &mut WorkerLatency,
    system_scratch: &mut String,
) -> Result<String> {
    let recovery_system = prepare_system_with_two_suffixes(
        system,
        FINAL_RECOVERY_SYSTEM_SUFFIX,
        recovery_suffix,
        system_scratch,
    );
    let recovery_messages = prepare_final_recovery_messages(messages, draft_content);
    let t0 = metrics::record_llm_call_start();
    let llm_round_start = Instant::now();
    let response = if llm_stream {
        let mut ignore_progress = |_delta: &str, _accumulated: &str| {
            crate::platform::task_wdt::feed_current_task();
        };
        worker_llm.chat_with_progress(
            tool_ctx,
            recovery_system,
            recovery_messages.as_ref(),
            None,
            ToolChoicePolicy::Auto,
            &mut ignore_progress,
        )
    } else {
        worker_llm.chat(
            tool_ctx,
            recovery_system,
            recovery_messages.as_ref(),
            None,
            ToolChoicePolicy::Auto,
        )
    };
    match response {
        Ok(response) => {
            metrics::record_llm_call_end(t0);
            latency.react_rounds = latency.react_rounds.saturating_add(1);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            Ok(response.content)
        }
        Err(e) => {
            metrics::record_llm_call_end(t0);
            metrics::record_llm_error();
            metrics::record_error_by_stage("agent_chat");
            Err(e.with_stage("agent_chat"))
        }
    }
}

fn structured_finalization_system_suffix(reply_surface: ReplySurface) -> Option<&'static str> {
    match reply_surface {
        ReplySurface::PublicRuntime => Some(PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX),
        ReplySurface::GovernedConversation
        | ReplySurface::PrivateBoundary
        | ReplySurface::TaskExecution
        | ReplySurface::InternalOnly => None,
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
        _ => Some(trimmed.to_string()),
    }
}

pub(super) fn run_surface_finalization_round(
    worker_llm: &(dyn LlmClient + Send + Sync),
    tool_ctx: &mut HttpClientToolContext<'_>,
    system: &str,
    messages: &[Message],
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
    let finalization_messages = prepare_final_recovery_messages(messages, draft_content);
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
            latency.react_rounds = latency.react_rounds.saturating_add(1);
            latency.llm_round_total_ms = latency
                .llm_round_total_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            parse_surface_finalization_reply(reply_surface, &response.content).ok_or_else(|| {
                crate::error::Error::config(
                    "surface_finalization_empty",
                    format!(
                        "reply_surface={} empty finalization reply",
                        reply_surface.as_str()
                    ),
                )
            })
        }
        Err(e) => {
            metrics::record_llm_call_end(t0);
            metrics::record_llm_error();
            metrics::record_error_by_stage("agent_chat");
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
