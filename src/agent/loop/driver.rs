use super::*;
use crate::memory::{get_object_text, parse_llm_json_payload, LlmJsonPayload};
use serde_json::Value;
use std::borrow::Cow;

const PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Public Runtime Finalization\nThis turn is on the public_runtime reply surface. Using only the completed tool results and public runtime evidence already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be public_runtime. reply must be a non-empty user-facing answer grounded in the current runtime evidence. Do not greet, do not ask generic follow-up questions, do not mention private/internal mechanisms, do not output progress logs, and do not call tools.";
const PRIVATE_BOUNDARY_FINALIZATION_SYSTEM_SUFFIX: &str = "\n\n## Private Boundary Finalization\nThis turn is on the private_boundary reply surface. Using only the already-governed conclusions, tool evidence, and safe boundary decisions already present in this conversation, produce the final user-facing answer now. Return JSON only with fields: surface and reply. surface must be private_boundary. reply must be a non-empty user-facing answer. Do not reveal private source material, raw inner notes, internal-only memory, or hidden mechanisms. If the governed conclusion is that the request cannot be fulfilled, state that boundary clearly and briefly. Do not greet, do not output internal logs, and do not call tools.";

#[derive(Clone, Debug, PartialEq, Eq)]
struct OfficeResolveHintCandidate {
    account_key: String,
    account_label: String,
    provider_kind: String,
    identity_class: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OfficeResolveHintSummary {
    capability: String,
    provider: Option<String>,
    candidate_accounts: Vec<OfficeResolveHintCandidate>,
}

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

fn extract_tool_result_attr<'a>(line: &'a str, attr: &str) -> Option<&'a str> {
    let needle = {
        let mut value = String::with_capacity(attr.len().saturating_add(2));
        value.push_str(attr);
        value.push_str("=\"");
        value
    };
    let start = line.find(needle.as_str())?;
    let value_start = start + needle.len();
    let remain = &line[value_start..];
    let end = remain.find('"')?;
    Some(&remain[..end])
}

fn parse_office_resolve_hint_from_tool_result_block(
    block: &str,
) -> Option<OfficeResolveHintSummary> {
    let payload = serde_json::from_str::<Value>(block.trim()).ok()?;
    let office_assessment = payload.get("office_assessment")?;
    let resolve_hint = office_assessment.get("resolve_hint")?;
    if resolve_hint.get("status")?.as_str()? != "ambiguous" {
        return None;
    }
    let capability = office_assessment
        .get("capability")?
        .as_str()?
        .trim()
        .to_string();
    if capability.is_empty() {
        return None;
    }
    let provider = payload
        .get("provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let candidate_accounts = resolve_hint
        .get("candidate_accounts")?
        .as_array()?
        .iter()
        .filter_map(|candidate| {
            let account_key = candidate.get("account_key")?.as_str()?.trim().to_string();
            let account_label = candidate
                .get("account_label")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let provider_kind = candidate
                .get("provider_kind")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let identity_class = candidate
                .get("identity_class")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            if account_key.is_empty() {
                return None;
            }
            Some(OfficeResolveHintCandidate {
                account_key,
                account_label,
                provider_kind,
                identity_class,
            })
        })
        .collect::<Vec<_>>();
    if candidate_accounts.len() < 2 {
        return None;
    }
    Some(OfficeResolveHintSummary {
        capability,
        provider,
        candidate_accounts,
    })
}

fn extract_recent_office_resolve_hint(messages: &[Message]) -> Option<OfficeResolveHintSummary> {
    for message in messages.iter().rev() {
        if message.role.as_ref() != "user" {
            continue;
        }
        let mut lines = message.content.lines();
        while let Some(line) = lines.next() {
            if !line.starts_with("<tool_result ") {
                continue;
            }
            let status = extract_tool_result_attr(line, "status");
            let failure = extract_tool_result_attr(line, "failure");
            if status != Some("error") || failure != Some("capability") {
                for next in lines.by_ref() {
                    if next == "</tool_result>" {
                        break;
                    }
                }
                continue;
            }
            let mut block = String::new();
            for next in lines.by_ref() {
                if next == "</tool_result>" {
                    break;
                }
                if !block.is_empty() {
                    block.push('\n');
                }
                block.push_str(next);
            }
            if let Some(summary) = parse_office_resolve_hint_from_tool_result_block(&block) {
                return Some(summary);
            }
        }
    }
    None
}

fn content_already_requests_office_account_choice(
    content: &str,
    summary: &OfficeResolveHintSummary,
) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let asks_for_choice = trimmed.contains('?')
        || trimmed.contains('？')
        || trimmed.contains("哪个")
        || trimmed.contains("选择")
        || trimmed.contains("选")
        || lower.contains("which")
        || lower.contains("choose")
        || lower.contains("select")
        || lower.contains("pick");
    if !asks_for_choice {
        return false;
    }
    summary.candidate_accounts.iter().any(|candidate| {
        trimmed.contains(&candidate.account_key)
            || (!candidate.account_label.is_empty() && trimmed.contains(&candidate.account_label))
    })
}

fn office_account_clarification_followup(messages: &[Message], content: &str) -> Option<String> {
    let summary = extract_recent_office_resolve_hint(messages)?;
    if content_already_requests_office_account_choice(content, &summary) {
        return None;
    }
    let mut out = String::with_capacity(512);
    out.push_str("[SYSTEM] A completed office tool call could not continue because account selection is ambiguous. Ask one brief user-facing clarification question now so the current action can continue. Do not claim work is still running. Do not mention JSON, tool logs, internal mechanisms, or hidden state.");
    out.push_str(" The unresolved capability is ");
    out.push_str(summary.capability.as_str());
    if let Some(provider) = summary.provider.as_deref() {
        out.push_str(" on provider ");
        out.push_str(provider);
    }
    out.push_str(". Candidate accounts:\n");
    for candidate in &summary.candidate_accounts {
        out.push_str("- ");
        if !candidate.account_label.is_empty() {
            out.push_str(candidate.account_label.as_str());
            out.push_str(" (");
            out.push_str(candidate.account_key.as_str());
            out.push(')');
        } else {
            out.push_str(candidate.account_key.as_str());
        }
        if !candidate.identity_class.is_empty() {
            out.push_str(", identity_class=");
            out.push_str(candidate.identity_class.as_str());
        }
        if !candidate.provider_kind.is_empty() {
            out.push_str(", provider=");
            out.push_str(candidate.provider_kind.as_str());
        }
        out.push('\n');
    }
    out.push_str("Return only the clarification question. Prefer account labels over raw account keys when the labels are clear.");
    Some(out)
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

pub(super) fn current_turn_scope_start(messages: &[Message], initial_msg_count: usize) -> usize {
    let search_end = initial_msg_count.min(messages.len());
    messages[..search_end]
        .iter()
        .rposition(|message| message.role.as_ref() == "user")
        .unwrap_or(0)
}

fn prepare_request_scoped_final_recovery_messages<'a>(
    messages: &'a [Message],
    scope_start: usize,
    draft_content: &str,
) -> Cow<'a, [Message]> {
    let scoped_messages = messages.get(scope_start..).unwrap_or(messages);
    if scoped_messages.is_empty() {
        return prepare_final_recovery_messages(messages, draft_content);
    }
    prepare_final_recovery_messages(scoped_messages, draft_content)
}

pub(super) fn resolve_end_turn_followup(ctx: EndTurnFollowupContext<'_>) -> Option<String> {
    if let Some(followup) = office_account_clarification_followup(ctx.messages, ctx.content) {
        return Some(end_turn_recovery_suffix(&followup));
    }
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
    scope_start: usize,
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
    let recovery_messages =
        prepare_request_scoped_final_recovery_messages(messages, scope_start, draft_content);
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
            latency.final_recovery_ms = latency
                .final_recovery_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
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
            latency.final_recovery_ms = latency
                .final_recovery_ms
                .saturating_add(llm_round_start.elapsed().as_millis());
            Err(e.with_stage("agent_chat"))
        }
    }
}

fn structured_finalization_system_suffix(reply_surface: ReplySurface) -> Option<&'static str> {
    match reply_surface {
        ReplySurface::PublicRuntime => Some(PUBLIC_RUNTIME_FINALIZATION_SYSTEM_SUFFIX),
        ReplySurface::PrivateBoundary => Some(PRIVATE_BOUNDARY_FINALIZATION_SYSTEM_SUFFIX),
        ReplySurface::GovernedConversation
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
        prepare_request_scoped_final_recovery_messages(messages, scope_start, draft_content);
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
    fn parse_recent_office_resolve_hint_reads_ambiguous_tool_result_block() {
        let messages = vec![Message {
            role: Cow::Borrowed("user"),
            content: concat!(
                "Tool results:\n",
                "<tool_result id=\"call_1\" tool=\"mail\" status=\"error\" failure=\"capability\">\n",
                "{\"ok\":false,\"office_assessment\":{\"capability\":\"mail\",\"resolve_hint\":{\"status\":\"ambiguous\",\"candidate_accounts\":[{\"account_key\":\"mail-work\",\"account_label\":\"Work\",\"provider_kind\":\"imap_smtp\",\"identity_class\":\"work\"},{\"account_key\":\"mail-personal\",\"account_label\":\"Personal\",\"provider_kind\":\"imap_smtp\",\"identity_class\":\"personal\"}]}}}\n",
                "</tool_result>\n"
            )
            .to_string(),
        }];

        let summary = extract_recent_office_resolve_hint(&messages).expect("resolve hint");
        assert_eq!(summary.capability, "mail");
        assert_eq!(summary.candidate_accounts.len(), 2);
        assert_eq!(summary.candidate_accounts[0].account_key, "mail-work");
        assert_eq!(summary.candidate_accounts[1].account_label, "Personal");
    }

    #[test]
    fn office_account_clarification_followup_skips_when_draft_already_asks_choice() {
        let messages = vec![Message {
            role: Cow::Borrowed("user"),
            content: concat!(
                "Tool results:\n",
                "<tool_result id=\"call_1\" tool=\"calendar\" status=\"error\" failure=\"capability\">\n",
                "{\"ok\":false,\"office_assessment\":{\"capability\":\"calendar\",\"resolve_hint\":{\"status\":\"ambiguous\",\"candidate_accounts\":[{\"account_key\":\"calendar-work\",\"account_label\":\"Work\",\"provider_kind\":\"mock_remote\",\"identity_class\":\"work\"},{\"account_key\":\"calendar-personal\",\"account_label\":\"Personal\",\"provider_kind\":\"mock_remote\",\"identity_class\":\"personal\"}]}}}\n",
                "</tool_result>\n"
            )
            .to_string(),
        }];

        assert!(office_account_clarification_followup(
            &messages,
            "你要用 Work（calendar-work）还是 Personal（calendar-personal）这个日历账户？"
        )
        .is_none());
    }

    #[test]
    fn private_boundary_has_structured_finalization_prompt() {
        assert!(structured_finalization_system_suffix(ReplySurface::PrivateBoundary).is_some());
        assert!(
            structured_finalization_system_suffix(ReplySurface::GovernedConversation).is_none()
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
}
