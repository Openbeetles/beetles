use super::driver::current_turn_scope_start;
use super::*;

pub(super) struct ExecutedTurn {
    pub(super) outcome: WorkerOutcome,
    pub(super) telemetry: WorkerRunTelemetry,
}

fn foreground_action_progress_kind_for_turn(
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    has_resumeable_work: bool,
    has_tools: bool,
) -> Option<crate::agent::delivery::TaskActionProgressKind> {
    use crate::agent::request_semantics::{ActionFamily, ExecutionPreference};

    if !has_tools
        || !has_resumeable_work
        || request_semantics.execution_preference != ExecutionPreference::ToolFirst
    {
        return None;
    }
    if matches!(request_semantics.action_family, ActionFamily::ActiveAction) {
        Some(crate::agent::delivery::TaskActionProgressKind::Resumed)
    } else {
        None
    }
}

fn maybe_emit_regular_foreground_action_progress(
    delivery: &mut DeliverySession<'_>,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    has_resumeable_work: bool,
    has_tools: bool,
) {
    use crate::agent::delivery::TaskActionProgressKind;

    if delivery.report().action_progress_updates_sent > 0 {
        return;
    }
    match foreground_action_progress_kind_for_turn(
        request_semantics,
        has_resumeable_work,
        has_tools,
    ) {
        Some(TaskActionProgressKind::Resumed) => delivery.emit_foreground_work_resumed(),
        None | Some(TaskActionProgressKind::Started) => {}
    }
}

fn should_emit_regular_foreground_blocked_progress(
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    any_tool_used: bool,
    delivery: &DeliverySession<'_>,
    content: &str,
) -> bool {
    use crate::agent::request_semantics::{ActionFamily, ExecutionPreference};

    !any_tool_used
        && request_semantics.execution_preference == ExecutionPreference::ToolFirst
        && matches!(request_semantics.action_family, ActionFamily::ActiveAction)
        && delivery.report().action_progress_updates_sent > 0
        && delivery.report().terminal_progress_updates_sent == 0
        && super::reply_finalize::looks_like_truthful_blocker_or_input_request(content)
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
    let request_semantics_started = Instant::now();
    let active_run = active_task_run_for_chat(
        config.runtime.task_run_store.as_ref(),
        &msg.channel,
        &msg.chat_id,
    )
    .ok()
    .flatten();
    let active_work = crate::agent::load_active_work_for_chat(
        config.runtime.active_work_store.as_ref(),
        active_run.as_ref(),
        &msg.chat_id,
    )
    .ok()
    .flatten();
    let foreground_work_context_present = active_work.is_some() || active_run.is_some();
    let tool_policy = crate::tools::ToolPolicyContext::new(msg.ingress, msg.channel.as_ref());
    let has_tools = !registry.tool_specs_for_llm(&tool_policy).is_empty();
    let request_semantics = super::super::request_semantics::RequestSemantics::compile_for_turn(
        super::super::request_semantics::RequestSemanticsCompileInput {
            msg,
            active_work: active_work.as_ref(),
        },
    );
    latency.request_semantics_ms = request_semantics_started.elapsed().as_millis();
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
    tool_ctx.supports_current_chat_outbound_message = false;
    tool_ctx.supports_explicit_outbound_message =
        msg.ingress == IngressKind::User && msg.channel.as_ref() != crate::CHANNEL_VOICE;
    let mut delivery = DeliverySession::new(
        msg,
        req_id,
        outbound_tx,
        editor,
        channel_capability,
        config.runtime.memory_system_kind,
        loc,
    );
    let PreparedWorkerConversation {
        mut runtime_carry,
        subject_state,
        soul_feedback_projection,
        system,
        mut messages,
        mut system_scratch,
        deliberation_gate,
        interactive_fast_path,
        allow_tool_round_recall_refill,
        prompt_memory_system_budget,
        pressure,
        request_semantics,
        active_task_context_present,
        governed_memory_evidence_present,
        mental_privacy_adjudication,
        persona_priority_adjudication,
    } = super::turn_prepare::prepare_turn(
        worker_llm,
        msg,
        has_tools,
        request_semantics,
        config,
        &mut tool_ctx,
        &mut latency,
    )?;
    let mut system = system;
    let reply_surface = ReplySurface::for_prepared_turn(
        msg.ingress,
        request_semantics,
        mental_privacy_adjudication.is_some(),
    );
    let request_semantics = request_semantics.apply_reasoning_contract(
        crate::agent::request_semantics::compile_reasoning_contract(
            crate::agent::request_semantics::ReasoningContractCompileInput {
                msg,
                has_tools,
                deliberation_class: deliberation_gate.class,
                reply_surface,
                request_semantics,
                active_task_context_present,
                foreground_work_context_present,
                governed_memory_evidence_present,
            },
        ),
    );
    let programmable_reasoning_intent =
        crate::agent::reasoning_intent::compile_programmable_reasoning_intent(
            crate::agent::reasoning_intent::ProgrammableReasoningIntentInput {
                strategy: config.strategy,
                runtime_contract: crate::programmable_reasoning_runtime_contract(),
                request_semantics,
                deliberation_gate: &deliberation_gate,
                has_tools,
                active_task_context_present,
                governed_memory_evidence_present,
            },
        );
    let programmable_reasoning_intent = programmable_reasoning_intent
        .is_meaningful()
        .then_some(programmable_reasoning_intent);
    let counterfactual_analysis = crate::agent::counterfactual::compile_counterfactual_analysis(
        crate::agent::counterfactual::CounterfactualAnalysisInput {
            strategy: config.strategy,
            runtime_contract: crate::programmable_reasoning_runtime_contract(),
            request_semantics,
            deliberation_gate: &deliberation_gate,
            reasoning_intent: programmable_reasoning_intent.as_ref(),
            has_tools,
            active_task_context_present,
            governed_memory_evidence_present,
        },
    );
    let counterfactual_analysis = counterfactual_analysis
        .is_meaningful()
        .then_some(counterfactual_analysis);
    let adversarial_arena_adjudication =
        crate::agent::adversarial_arena::compile_turn_strategy_adjudication(
            crate::agent::adversarial_arena::TurnStrategyArenaInput {
                strategy: config.strategy,
                runtime_contract: crate::programmable_reasoning_runtime_contract(),
                counterfactual_analysis: counterfactual_analysis.as_ref(),
            },
        );
    let adversarial_arena_adjudication = adversarial_arena_adjudication
        .is_meaningful()
        .then_some(adversarial_arena_adjudication);
    let request_plan = AgentRequestPlan::build_for_prepared_turn(
        msg,
        registry,
        worker_llm,
        config.strategy,
        request_semantics,
        reply_surface,
    )
    .with_programmable_reasoning_intent(programmable_reasoning_intent.as_ref())
    .with_counterfactual_analysis(counterfactual_analysis.as_ref())
    .with_adversarial_arena_adjudication(adversarial_arena_adjudication.as_ref());
    request_plan.apply_system_prompt(
        &mut system,
        crate::orchestrator::current_budget().system_prompt_max,
    );
    maybe_emit_regular_foreground_action_progress(
        &mut delivery,
        request_semantics,
        active_work.is_some(),
        request_plan.has_tools(),
    );
    if let Some(task_execution_outcome) = try_run_task_execution(
        worker_llm,
        msg,
        outbound_tx,
        &mut delivery,
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
        active_run,
        subject_state.as_deref().cloned(),
        soul_feedback_projection.as_deref().cloned(),
        mental_privacy_adjudication.as_deref().cloned(),
        persona_priority_adjudication.as_deref().cloned(),
    )? {
        let (outcome, telemetry) = task_execution_outcome;
        return Ok(ExecutedTurn { outcome, telemetry });
    }
    if msg.ingress == IngressKind::User && config.strategy == AgentRunStrategy::LinuxEnhanced {
        crate::agent::append_foreground_work_packet_guidance(
            &mut system,
            crate::orchestrator::current_budget().system_prompt_max,
        );
    }

    let initial_msg_count = messages.len();
    let current_turn_scope_start = current_turn_scope_start(&messages, initial_msg_count);
    tool_call_repeat.clear();
    let mut final_content = String::with_capacity(4096);
    let mut memory_grounding: Option<String> = None;
    let mut tool_result_user_content = String::with_capacity(1024);
    let mut round_evidence_lines = Vec::with_capacity(MAX_TOOL_EVIDENCE_ITEMS);
    let mut successful_tool_names = std::collections::BTreeSet::new();
    let mut any_tool_round_executed = false;
    let mut any_tool_used = false;
    let mut external_content_used = false;
    let mut effective_reply_surface = reply_surface;
    let mut used_surface_finalization = false;

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
            effective_reply_surface = reply_surface.promote_for_runtime_tools(
                &successful_tool_names,
                external_content_used,
                &content,
            );
            if any_tool_round_executed
                && effective_reply_surface
                    .should_run_structured_finalization_after_tool_round(&content)
            {
                used_surface_finalization = true;
                final_content = run_surface_finalization_round(
                    worker_llm,
                    &mut tool_ctx,
                    &system,
                    &messages,
                    current_turn_scope_start,
                    effective_reply_surface,
                    &content,
                    recovery_suffix_for_gate(&deliberation_gate),
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
            any_tool_round_executed = true;
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
            if tool_round_output.round_tool_success {
                any_tool_used = true;
            }
            successful_tool_names.extend(tool_round_output.successful_tool_names);
            external_content_used |= tool_round_output.used_external_content;
            let evidence_block = (!round_evidence_lines.is_empty()).then(|| {
                render_surface_evidence_block(
                    effective_reply_surface,
                    &round_evidence_lines,
                    tool_round_output.omitted_evidence_count,
                )
            });
            if memory_grounding.is_none() {
                if runtime_carry.long_term_memory_text.is_none()
                    && interactive_fast_path
                    && allow_tool_round_recall_refill
                    && prompt_memory_system_budget
                        >= memory_policy(config.runtime.memory_system_kind)
                            .long_term_recall
                            .block_min_len
                {
                    let recall_recent_count = memory_policy(config.runtime.memory_system_kind)
                        .long_term_recall
                        .recent_grounding_message_count;
                    let recent_start = runtime_carry
                        .recent_messages
                        .len()
                        .saturating_sub(recall_recent_count);
                    runtime_carry.long_term_memory_text = recall_long_term_memory_block(
                        config.runtime.long_term_memory_store.as_ref(),
                        &msg.chat_id,
                        &msg.content,
                        runtime_carry.summary_text.as_deref(),
                        &runtime_carry.recent_messages[recent_start..],
                        prompt_memory_system_budget,
                        config.runtime.memory_system_kind.memory_profile(),
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
    if !used_surface_finalization
        && any_tool_round_executed
        && reply_surface
            .promote_for_runtime_tools(
                &successful_tool_names,
                external_content_used,
                final_content.as_str(),
            )
            .should_run_structured_finalization_after_tool_round(final_content.as_str())
    {
        effective_reply_surface = reply_surface.promote_for_runtime_tools(
            &successful_tool_names,
            external_content_used,
            final_content.as_str(),
        );
        used_surface_finalization = true;
        final_content = run_surface_finalization_round(
            worker_llm,
            &mut tool_ctx,
            &system,
            &messages,
            current_turn_scope_start,
            effective_reply_surface,
            final_content.as_str(),
            recovery_suffix_for_gate(&deliberation_gate),
            config.llm_stream,
            &mut latency,
            &mut system_scratch,
        )?;
    }
    if any_tool_used && !used_surface_finalization {
        effective_reply_surface = reply_surface.promote_for_runtime_tools(
            &successful_tool_names,
            external_content_used,
            final_content.as_str(),
        );
    }
    if final_content.trim().is_empty()
        && msg.ingress == IngressKind::User
        && msg.channel.as_ref() != CHANNEL_CRON
    {
        metrics::record_empty_final_blocked();
        return Err(crate::error::Error::config(
            crate::agent::final_reply::ReplyContractBreachKind::ProducerEmpty.stage(),
            format!(
                "reply_surface={} any_tool_used={} used_surface_finalization={}",
                effective_reply_surface.as_str(),
                any_tool_used,
                used_surface_finalization
            ),
        ));
    }
    if should_emit_regular_foreground_blocked_progress(
        request_semantics,
        any_tool_used,
        &delivery,
        &final_content,
    ) {
        delivery.emit_foreground_work_blocked();
    }
    let streamed = delivery.finalize(&final_content);
    let outcome = WorkerOutcome::Content(final_content);
    Ok(ExecutedTurn {
        outcome,
        telemetry: WorkerRunTelemetry {
            streamed,
            latency,
            delivery: delivery.report(),
            any_tool_used,
            external_content_used,
            used_surface_finalization,
            task_execution_used: false,
            foreground_work_context_present,
            pressure,
            runtime_mode: crate::runtime::thread_registry::runtime_mode_snapshot(),
            deliberation_class: deliberation_gate.class,
            reply_surface: effective_reply_surface,
            prompt_recall_intent: runtime_carry.prompt_recall_intent,
            runtime_skill_selected_ids: runtime_carry.runtime_skill_selected_ids,
            task_learning_selected_ids: runtime_carry.task_recall_selected_ids,
            programmable_reasoning_intent,
            counterfactual_analysis,
            adversarial_arena_adjudication,
            subject_state: subject_state.map(|value| *value),
            soul_feedback_projection: soul_feedback_projection.map(|value| *value),
            mental_privacy_adjudication: mental_privacy_adjudication.map(|value| *value),
            persona_priority_adjudication: persona_priority_adjudication.map(|value| *value),
        },
    })
}
