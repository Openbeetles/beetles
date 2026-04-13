use super::*;

pub(super) struct ExecutedTurn {
    pub(super) outcome: WorkerOutcome,
    pub(super) telemetry: WorkerRunTelemetry,
}

/// 完整 context + worker LLM + ReAct 循环，返回执行结果与 telemetry。
/// telemetry.streamed=true 表示已通过流式编辑发送到通道，调用方应跳过 outbound_tx。
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_turn(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    outbound_tx: &OutboundTx,
    req_id: &str,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    loc: UiLocale,
) -> Result<ExecutedTurn> {
    let mut latency = WorkerLatency::default();
    let worker_start = Instant::now();
    let mut tool_ctx = HttpClientToolContext {
        http,
        chat_id: Some(msg.chat_id.clone()),
        ingress: msg.ingress,
        channel: Some(msg.channel.clone()),
        tool_registry: Some(registry),
        channel_capability_registry: Arc::clone(&config.channel_capability_registry),
        supports_current_chat_outbound_message: false,
        supports_explicit_outbound_message: false,
        outbound_message_budget: 2,
        outbound_message_count: 0,
        locale: loc,
    };
    let compiler_tool_policy =
        crate::tools::ToolPolicyContext::new(msg.ingress, msg.channel.as_ref());
    let compiler_tool_specs = registry.tool_specs_for_llm(&compiler_tool_policy);
    let request_semantics_started = Instant::now();
    let request_semantics = super::super::request_semantics::compile_request_semantics(
        &mut tool_ctx,
        worker_llm,
        super::super::request_semantics::RequestSemanticCompilerInput {
            strategy: config.strategy,
            ingress: msg.ingress,
            channel: msg.channel.as_ref(),
            is_group: msg.is_group,
            content: &msg.content,
            pressure: crate::orchestrator::current_pressure(),
            runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
            tool_specs: &compiler_tool_specs,
        },
    );
    latency.request_semantics_ms = request_semantics_started.elapsed().as_millis();
    let request_plan = AgentRequestPlan::build(
        msg,
        registry,
        worker_llm,
        config.strategy,
        request_semantics,
    );
    let channel_capability = config.channel_capability_registry.get(msg.channel.as_ref());
    let editor = if config.llm_stream
        && config.stream_editor_channel.as_deref() == Some(msg.channel.as_ref())
        && channel_capability
            .map(|entry| entry.enabled && entry.contract.supports_stream_edit)
            .unwrap_or(false)
    {
        config.stream_editor.as_deref()
    } else {
        None
    };
    let current_channel_enabled = channel_capability
        .map(|entry| entry.enabled)
        .unwrap_or(false);
    let current_user_visible = msg.ingress == IngressKind::User
        && current_channel_enabled
        && msg.channel.as_ref() != crate::CHANNEL_VOICE;
    tool_ctx.supports_current_chat_outbound_message = current_user_visible
        && channel_capability
            .map(|entry| entry.contract.supports_supplemental_reply)
            .unwrap_or(false);
    tool_ctx.supports_explicit_outbound_message =
        msg.ingress == IngressKind::User && msg.channel.as_ref() != crate::CHANNEL_VOICE;
    let mut delivery = DeliverySession::new(
        msg,
        req_id,
        outbound_tx,
        editor,
        channel_capability,
        config.memory_system_kind,
        loc,
    );
    let PreparedWorkerConversation {
        mut runtime_carry,
        subject_state,
        system,
        mut messages,
        mut system_scratch,
        deliberation_gate,
        interactive_fast_path,
        allow_tool_round_recall_refill,
        prompt_memory_system_budget,
        pressure,
        request_semantics,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    } = super::turn_prepare::prepare_turn(
        worker_llm,
        msg,
        &request_plan,
        request_semantics,
        config,
        &mut tool_ctx,
        &mut latency,
    )?;
    let reply_surface = ReplySurface::for_turn(msg.ingress, request_semantics);
    if let Some(task_execution_outcome) = try_run_task_execution(
        worker_llm,
        msg,
        outbound_tx,
        registry,
        config,
        &request_plan,
        &mut tool_ctx,
        loc,
        &mut latency,
        &system,
        &messages,
        &mut system_scratch,
        pressure,
        deliberation_gate.class,
        request_semantics,
        subject_state.as_deref().cloned(),
        mental_privacy_adjudication.as_deref().cloned(),
        persona_priority_adjudication.as_deref().cloned(),
    )? {
        let (outcome, telemetry) = task_execution_outcome;
        return Ok(ExecutedTurn { outcome, telemetry });
    }

    let initial_msg_count = messages.len();
    tool_call_repeat.clear();
    let mut final_content = String::with_capacity(4096);
    let mut memory_grounding: Option<String> = None;
    let mut tool_result_user_content = String::with_capacity(1024);
    let mut round_evidence_lines = Vec::with_capacity(MAX_TOOL_EVIDENCE_ITEMS);
    let mut any_tool_used = false;
    let mut external_content_used = false;
    let mut recent_tool_round = RecentToolRoundState::default();
    let mut delivered_current_chat_reply: Option<String> = None;
    let mut used_surface_finalization = false;
    let mut used_final_answer_recovery = false;

    for round in 0..MAX_REACT_ROUNDS {
        latency.react_rounds = round as u32 + 1;
        if round > 0 {
            match crate::orchestrator::refresh_heap_if_stale() {
                crate::orchestrator::PressureLevel::Normal => {}
                crate::orchestrator::PressureLevel::Cautious
                | crate::orchestrator::PressureLevel::Critical => {
                    match crate::orchestrator::can_call_llm_pub() {
                        LlmDecision::Proceed => {}
                        LlmDecision::RetryLater { .. } | LlmDecision::Degrade { .. } => {
                            if final_content.is_empty() {
                                final_content = tr(UiMessage::LowMemoryUserDefer, loc);
                            } else {
                                final_content.push_str("\n\n");
                                final_content.push_str(&tr(UiMessage::StreamLowMemoryOmitted, loc));
                            }
                            break;
                        }
                    }
                }
            }
        }
        if round >= 2 {
            compact_early_tool_rounds(&mut messages, initial_msg_count);
        }
        let t0 = metrics::record_llm_call_start();
        let llm_round_start = Instant::now();
        let mut first_token_marked = latency.ttft_ms.is_some();
        let round_tools = request_plan.request_tools();
        let response = if config.llm_stream {
            let progress_base = worker_start;
            let mut progress_cb = |_delta: &str, accumulated: &str| {
                crate::platform::task_wdt::feed_current_task();
                if !first_token_marked && !accumulated.is_empty() {
                    latency.ttft_ms = Some(progress_base.elapsed().as_millis());
                    first_token_marked = true;
                }
                if matches!(
                    crate::orchestrator::current_pressure(),
                    crate::orchestrator::PressureLevel::Critical
                ) {
                    return;
                }
                delivery.on_stream_delta(accumulated);
            };
            worker_llm.chat_with_progress(
                &mut tool_ctx,
                &system,
                &messages,
                round_tools,
                request_plan.tool_choice(round, any_tool_used),
                &mut progress_cb,
            )
        } else {
            worker_llm.chat(
                &mut tool_ctx,
                &system,
                &messages,
                round_tools,
                request_plan.tool_choice(round, any_tool_used),
            )
        };
        let response = match response {
            Ok(r) => {
                metrics::record_llm_call_end(t0);
                latency.llm_round_total_ms = latency
                    .llm_round_total_ms
                    .saturating_add(llm_round_start.elapsed().as_millis());
                r
            }
            Err(e) => {
                metrics::record_llm_call_end(t0);
                metrics::record_llm_error();
                metrics::record_error_by_stage("agent_chat");
                return Err(e.with_stage("agent_chat"));
            }
        };
        let response = request_plan.recover_response(response);
        crate::platform::task_wdt::feed_current_task();
        metrics::record_wdt_feed();

        let tc_count = response.tool_calls.as_ref().map_or(0, |v| v.len());
        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "[agent] llm round={} stop_reason={:?} tool_calls={} content_len={}",
                round,
                response.stop_reason,
                tc_count,
                response.content.len()
            );
        }

        if response.stop_reason == StopReason::MaxTokens {
            let mut content = response.content;
            if !content.is_empty() {
                content.push_str("\n\n");
                content.push_str(&tr(UiMessage::ReplyTruncated, loc));
            }
            mark_ttft_if_visible(&mut latency, worker_start, &content);
            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::EndTurn {
            let content = response.content;
            let delivery_report = delivery.report();
            let primary_reply_already_delivered = delivery_report.current_primary_delivered;
            let tool_visible_reply_sent = delivery_report.tool_visible_updates_sent > 0;
            if any_tool_used
                && delivered_current_chat_reply.is_none()
                && reply_surface.requires_structured_finalization_after_tool_success()
            {
                if !content.trim().is_empty() {
                    metrics::record_tool_succeeded_final_drift();
                    log::info!(
                        "[reply_surface] tool succeeded but final drift detected surface={} channel={} chat_id={}",
                        reply_surface.as_str(),
                        msg.channel,
                        msg.chat_id
                    );
                }
                used_surface_finalization = true;
                final_content = run_surface_finalization_round(
                    worker_llm,
                    &mut tool_ctx,
                    &system,
                    &messages,
                    reply_surface,
                    &content,
                    recovery_suffix_for_gate(&deliberation_gate),
                    config.llm_stream,
                    &mut latency,
                    &mut system_scratch,
                )?;
                break;
            }
            if let Some(followup) = empty_final_answer_followup(
                config.strategy,
                any_tool_used && !primary_reply_already_delivered && !tool_visible_reply_sent,
                &content,
            ) {
                if any_tool_used {
                    used_final_answer_recovery = true;
                    let mut recovery_suffix =
                        recovery_suffix_for_gate(&deliberation_gate).to_string();
                    recovery_suffix.push_str("\n\n## EndTurn correction\n");
                    recovery_suffix.push_str(followup);
                    final_content = run_final_answer_recovery_round(
                        worker_llm,
                        &mut tool_ctx,
                        &system,
                        &messages,
                        &content,
                        recovery_suffix.as_str(),
                        config.llm_stream,
                        &mut latency,
                        &mut system_scratch,
                    )?;
                    break;
                }
            }
            if let Some(recovery_suffix) = resolve_end_turn_followup(EndTurnFollowupContext {
                strategy: config.strategy,
                any_tool_used,
                recent_tool_round: &recent_tool_round,
                messages: &messages,
                content: &content,
            }) {
                used_final_answer_recovery = true;
                let mut combined_suffix = recovery_suffix_for_gate(&deliberation_gate).to_string();
                combined_suffix.push_str(recovery_suffix.as_str());
                final_content = run_final_answer_recovery_round(
                    worker_llm,
                    &mut tool_ctx,
                    &system,
                    &messages,
                    &content,
                    combined_suffix.as_str(),
                    config.llm_stream,
                    &mut latency,
                    &mut system_scratch,
                )?;
                break;
            }

            mark_ttft_if_visible(&mut latency, worker_start, &content);
            final_content = content;
            break;
        }

        if response.stop_reason == StopReason::ToolUse {
            let tool_calls = response.tool_calls.as_deref().unwrap_or(&[]);
            if tool_calls.is_empty() {
                mark_ttft_if_visible(&mut latency, worker_start, &response.content);
                final_content = response.content;
                break;
            }
            if !response.content.trim().is_empty() && response.content.trim() != "[tool_use]" {
                mark_ttft_if_visible(&mut latency, worker_start, &response.content);
                delivery.emit_partial(&response.content);
            }
            messages.push(Message {
                role: Cow::Borrowed("assistant"),
                content: if response.content.is_empty() {
                    "[tool_use]".to_string()
                } else {
                    response.content
                },
            });
            let mut cap =
                MAX_TOOL_RESULTS_USER_MESSAGE_LEN.min(tool_calls.len().saturating_mul(192));
            cap = cap.max(TOOL_RESULTS_PREFIX.len());
            tool_result_user_content.clear();
            if tool_result_user_content.capacity() < cap {
                tool_result_user_content.reserve(cap - tool_result_user_content.capacity());
            }
            tool_result_user_content.push_str(TOOL_RESULTS_PREFIX);
            round_evidence_lines.clear();
            let tool_round_output = execute_tool_use_round(
                tool_calls,
                &mut delivery,
                &request_plan,
                registry,
                &mut tool_ctx,
                config,
                tool_call_repeat,
                &mut latency,
                &mut tool_result_user_content,
                &mut round_evidence_lines,
            );
            if let Some(reply) = tool_round_output.delivered_current_chat_reply {
                delivered_current_chat_reply = Some(reply);
            }
            if tool_round_output.round_tool_success {
                any_tool_used = true;
            }
            let round_failure_summary = tool_round_output.round_failure_summary;
            external_content_used |= tool_round_output.used_external_content;
            recent_tool_round.record_round(
                tool_calls.len(),
                tool_round_output.round_tool_success,
                round_failure_summary,
            );
            let evidence_block = (!round_evidence_lines.is_empty()).then(|| {
                render_surface_evidence_block(
                    reply_surface,
                    &round_evidence_lines,
                    tool_round_output.omitted_evidence_count,
                )
            });
            if memory_grounding.is_none() {
                if runtime_carry.long_term_memory_text.is_none()
                    && interactive_fast_path
                    && allow_tool_round_recall_refill
                    && prompt_memory_system_budget
                        >= memory_policy(config.memory_system_kind)
                            .long_term_recall
                            .block_min_len
                {
                    let recall_recent_count = memory_policy(config.memory_system_kind)
                        .long_term_recall
                        .recent_grounding_message_count;
                    let recent_start = runtime_carry
                        .recent_messages
                        .len()
                        .saturating_sub(recall_recent_count);
                    runtime_carry.long_term_memory_text = recall_long_term_memory_block(
                        config.long_term_memory_store.as_ref(),
                        &msg.chat_id,
                        &msg.content,
                        runtime_carry.summary_text.as_deref(),
                        &runtime_carry.recent_messages[recent_start..],
                        prompt_memory_system_budget,
                        config.memory_system_kind.memory_profile(),
                    );
                }
                memory_grounding = build_memory_grounding_text(
                    runtime_carry.summary_text.as_deref(),
                    runtime_carry.long_term_memory_text.as_deref(),
                );
            }
            let memory_block = memory_grounding
                .as_deref()
                .filter(|_| reply_surface.allows_memory_grounding_block())
                .map(render_memory_grounding_block);
            let raw_tool_results = std::mem::take(&mut tool_result_user_content);
            let (assembled_tool_results, _assembled_truncated) = assemble_tool_round_user_message(
                raw_tool_results.as_str(),
                tool_round_output.truncated,
                evidence_block.as_deref(),
                memory_block.as_deref(),
                MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
            );
            messages.push(Message {
                role: Cow::Borrowed("user"),
                content: assembled_tool_results,
            });
            continue;
        }

        let content = response.content;
        final_content = content;
        break;
    }
    if final_content.trim().is_empty() && any_tool_used && delivered_current_chat_reply.is_none() {
        if reply_surface.requires_structured_finalization_after_tool_success() {
            used_surface_finalization = true;
            final_content = run_surface_finalization_round(
                worker_llm,
                &mut tool_ctx,
                &system,
                &messages,
                reply_surface,
                final_content.as_str(),
                recovery_suffix_for_gate(&deliberation_gate),
                config.llm_stream,
                &mut latency,
                &mut system_scratch,
            )?;
        } else {
            used_final_answer_recovery = true;
            final_content = run_final_answer_recovery_round(
                worker_llm,
                &mut tool_ctx,
                &system,
                &messages,
                final_content.as_str(),
                recovery_suffix_for_gate(&deliberation_gate),
                config.llm_stream,
                &mut latency,
                &mut system_scratch,
            )?;
        }
    }
    if delivered_current_chat_reply.is_none()
        && final_content.trim().is_empty()
        && msg.ingress == IngressKind::User
        && msg.channel.as_ref() != CHANNEL_CRON
    {
        metrics::record_empty_final_blocked();
        return Err(crate::error::Error::config(
            "final_reply_empty",
            format!(
                "reply_surface={} any_tool_used={} used_surface_finalization={} used_final_answer_recovery={}",
                reply_surface.as_str(),
                any_tool_used,
                used_surface_finalization,
                used_final_answer_recovery
            ),
        ));
    }
    let streamed = delivery.finalize(&final_content);
    let outcome = if let Some(reply) = delivered_current_chat_reply {
        WorkerOutcome::Delivered(reply)
    } else {
        WorkerOutcome::Content(final_content)
    };
    Ok(ExecutedTurn {
        outcome,
        telemetry: WorkerRunTelemetry {
            streamed,
            latency,
            delivery: delivery.report(),
            any_tool_used,
            external_content_used,
            used_surface_finalization,
            used_final_answer_recovery,
            task_execution_used: false,
            pressure,
            runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
            deliberation_class: deliberation_gate.class,
            request_semantics,
            reply_surface,
            prompt_recall_intent: runtime_carry.prompt_recall_intent,
            runtime_skill_selected_ids: runtime_carry.runtime_skill_selected_ids,
            task_learning_selected_ids: runtime_carry.task_recall_selected_ids,
            subject_state: subject_state.map(|value| *value),
            mental_privacy_adjudication: mental_privacy_adjudication.map(|value| *value),
            persona_priority_adjudication: persona_priority_adjudication.map(|value| *value),
        },
    })
}
