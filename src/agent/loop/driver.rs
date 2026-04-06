use super::*;

pub(super) fn enqueue_end_turn_followup(
    messages: &mut Vec<Message>,
    progress_history: &mut [Option<RoundProgress>; 3],
    content: &str,
    followup: &str,
) {
    if !content.trim().is_empty() {
        messages.push(Message {
            role: Cow::Borrowed("assistant"),
            content: content.to_string(),
        });
    }
    messages.push(Message {
        role: Cow::Borrowed("user"),
        content: followup.to_string(),
    });
    progress_history[0] = progress_history[1];
    progress_history[1] = progress_history[2];
    progress_history[2] = Some(RoundProgress { new_info: false });
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
    scratch.clear();
    let required = base.len().saturating_add(suffix.len());
    if scratch.capacity() < required {
        scratch.reserve(required - scratch.capacity());
    }
    scratch.push_str(base);
    scratch.push_str(suffix);
    scratch.as_str()
}

pub(super) fn resolve_end_turn_followup(ctx: EndTurnFollowupContext<'_>) -> Option<(String, bool)> {
    if let Some(followup) =
        ctx.request_plan
            .missing_tool_followup(ctx.round, ctx.any_tool_used, ctx.content)
    {
        return Some((followup.to_string(), false));
    }
    if ctx.end_turn_followup_used {
        return None;
    }
    if let Some(followup) = final_answer_followup(
        ctx.strategy,
        ctx.recent_tool_round.successful_round,
        ctx.content,
    ) {
        return Some((followup, true));
    }
    let mut recent_assistant_messages = Vec::with_capacity(3);
    collect_recent_assistant_messages(ctx.messages, 3, &mut recent_assistant_messages);
    if let Some(followup) =
        repeated_answer_followup(ctx.strategy, &recent_assistant_messages, ctx.content)
    {
        return Some((followup.to_string(), true));
    }
    if let Some(followup) =
        blocker_end_turn_followup(ctx.strategy, ctx.recent_tool_round.blocker, ctx.content)
    {
        return Some((followup.to_string(), true));
    }
    stalled_end_turn_followup(
        ctx.strategy,
        ctx.recent_tool_round.consecutive_stalled_rounds,
        ctx.content,
    )
    .map(|followup| (followup.to_string(), true))
}

pub(super) fn run_final_answer_recovery_round(
    worker_llm: &(dyn LlmClient + Send + Sync),
    tool_ctx: &mut HttpClientToolContext<'_>,
    system: &str,
    messages: &[Message],
    llm_stream: bool,
    latency: &mut WorkerLatency,
    system_scratch: &mut String,
) -> Result<String> {
    let recovery_system =
        prepare_system_with_suffix(system, FINAL_RECOVERY_SYSTEM_SUFFIX, system_scratch);
    let t0 = metrics::record_llm_call_start();
    let llm_round_start = Instant::now();
    let response = if llm_stream {
        let mut ignore_progress = |_delta: &str, _accumulated: &str| {
            crate::platform::task_wdt::feed_current_task();
        };
        worker_llm.chat_with_progress(
            tool_ctx,
            recovery_system,
            messages,
            None,
            ToolChoicePolicy::Auto,
            &mut ignore_progress,
        )
    } else {
        worker_llm.chat(
            tool_ctx,
            recovery_system,
            messages,
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

pub(super) fn recv_next_agent_msg(
    user_inbound_rx: &UserInboundRx,
    system_inbound_rx: &InboundRx,
    recv_timeout: Duration,
    prefer_system_once: bool,
) -> AgentRecvStatus {
    let poll_slice = recv_timeout.min(Duration::from_millis(INBOUND_POLL_SLICE_MS));
    let deadline = Instant::now() + recv_timeout;
    let mut user_disconnected = false;
    let mut system_disconnected = false;

    loop {
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
