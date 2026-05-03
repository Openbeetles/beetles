use super::*;

pub(super) struct ExecutedTurn {
    pub(super) outcome: WorkerOutcome,
    pub(super) telemetry: WorkerRunTelemetry,
}

fn foreground_task_started_for_turn(
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    has_resumeable_work: bool,
    has_tools: bool,
) -> Option<bool> {
    use crate::agent::request_semantics::{ActionFamily, ExecutionPreference};

    if !has_tools
        || !has_resumeable_work
        || request_semantics.execution_preference != ExecutionPreference::ToolFirst
    {
        return None;
    }
    if matches!(request_semantics.action_family, ActionFamily::ActiveAction) {
        Some(true)
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
    if delivery.has_visible_task_started_fact() {
        return;
    }
    if let Some(resumed) =
        foreground_task_started_for_turn(request_semantics, has_resumeable_work, has_tools)
    {
        delivery.emit_fact(crate::agent::TurnVisibilityFact::TaskStarted { resumed });
    }
}

fn should_emit_regular_foreground_blocked_progress(
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    any_tool_used: bool,
    delivery: &DeliverySession<'_>,
    blocker: Option<&crate::agent::WorkflowBlocker>,
) -> bool {
    use crate::agent::request_semantics::{ActionFamily, ExecutionPreference};

    !any_tool_used
        && request_semantics.execution_preference == ExecutionPreference::ToolFirst
        && matches!(request_semantics.action_family, ActionFamily::ActiveAction)
        && delivery.has_visible_task_started_fact()
        && !delivery.has_visible_task_terminal_fact()
        && blocker.is_some()
}

fn render_tool_blocker_field_list(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| format!("`{}`", field.trim()))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn render_tool_blocker_option_list(
    options: &[crate::agent::WorkflowClarificationOption],
) -> Option<String> {
    let values = options
        .iter()
        .filter_map(|option| {
            let value = option.value.trim();
            if value.is_empty() {
                return None;
            }
            let label = option.label.trim();
            if label.is_empty() || label.eq_ignore_ascii_case(value) {
                Some(value.to_string())
            } else {
                Some(format!("{label} (`{value}`)"))
            }
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(" / "))
}

fn render_tool_clarification_field_prompt(
    field: &crate::agent::WorkflowClarificationField,
    blocker_kind: crate::agent::WorkflowBlockerKind,
    loc: UiLocale,
) -> String {
    let field_name = format!("`{}`", field.key.trim());
    if let Some(options) = render_tool_blocker_option_list(&field.options) {
        return match blocker_kind {
            crate::agent::WorkflowBlockerKind::NeedsUserChoice
            | crate::agent::WorkflowBlockerKind::NeedsConfirmation => match loc {
                UiLocale::Zh => format!("{field_name}（可选值：{options}）"),
                UiLocale::En => format!("{field_name} (allowed values: {options})"),
            },
            _ => match loc {
                UiLocale::Zh => format!("{field_name}（可选值：{options}）"),
                UiLocale::En => format!("{field_name} (allowed values: {options})"),
            },
        };
    }
    field_name
}

fn render_tool_clarification_field_list(
    blocker: &crate::agent::WorkflowBlocker,
    loc: UiLocale,
) -> Option<String> {
    let rendered = blocker
        .clarification
        .as_ref()
        .map(|clarification| clarification.fields.as_slice())
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter_map(|(idx, field)| {
            let key = field.key.trim();
            if key.is_empty() {
                return None;
            }
            Some(match loc {
                UiLocale::Zh => format!(
                    "{}. {}",
                    idx + 1,
                    render_tool_clarification_field_prompt(field, blocker.kind, loc)
                ),
                UiLocale::En => format!(
                    "{}. {}",
                    idx + 1,
                    render_tool_clarification_field_prompt(field, blocker.kind, loc)
                ),
            })
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| match loc {
        UiLocale::Zh => rendered.join("；"),
        UiLocale::En => rendered.join("; "),
    })
}

pub(super) fn render_programmatic_clarification_question(
    blocker: &crate::agent::WorkflowBlocker,
    loc: UiLocale,
) -> String {
    match blocker.kind {
        crate::agent::WorkflowBlockerKind::ProbeFailed => {
            return match loc {
                UiLocale::Zh => format!("这一步的探测失败了：{}。", blocker.summary.trim()),
                UiLocale::En => format!(
                    "This step failed during probing: {}.",
                    blocker.summary.trim()
                ),
            };
        }
        crate::agent::WorkflowBlockerKind::RuntimeBlocked => {
            return match loc {
                UiLocale::Zh => {
                    format!("这一步当前被运行时条件阻塞：{}。", blocker.summary.trim())
                }
                UiLocale::En => format!(
                    "This step is currently blocked by runtime conditions: {}.",
                    blocker.summary.trim()
                ),
            };
        }
        crate::agent::WorkflowBlockerKind::Unsupported => {
            return match loc {
                UiLocale::Zh => format!("这一步当前不可用：{}。", blocker.summary.trim()),
                UiLocale::En => {
                    format!(
                        "This step is currently unsupported: {}.",
                        blocker.summary.trim()
                    )
                }
            };
        }
        crate::agent::WorkflowBlockerKind::RetryLater => {
            return match loc {
                UiLocale::Zh => format!("这一步当前还不能继续：{}。", blocker.summary.trim()),
                UiLocale::En => {
                    format!("This step cannot continue yet: {}.", blocker.summary.trim())
                }
            };
        }
        crate::agent::WorkflowBlockerKind::TaskBlocked => {
            return match loc {
                UiLocale::Zh => format!("这项工作流当前被阻塞：{}。", blocker.summary.trim()),
                UiLocale::En => {
                    format!(
                        "This workflow is currently blocked: {}.",
                        blocker.summary.trim()
                    )
                }
            };
        }
        crate::agent::WorkflowBlockerKind::NeedsUserFacts
        | crate::agent::WorkflowBlockerKind::NeedsUserChoice
        | crate::agent::WorkflowBlockerKind::NeedsConfirmation => {}
    }
    let clarification_fields = blocker
        .clarification
        .as_ref()
        .map(|clarification| clarification.fields.as_slice())
        .unwrap_or(&[]);
    if clarification_fields.len() == 1 {
        let field = &clarification_fields[0];
        let field_name = format!("`{}`", field.key.trim());
        if let Some(options) = render_tool_blocker_option_list(&field.options) {
            return match blocker.kind {
                crate::agent::WorkflowBlockerKind::NeedsUserChoice => match loc {
                    UiLocale::Zh => {
                        format!("要继续这一步，还需要你选择 {field_name}。可选值：{options}。")
                    }
                    UiLocale::En => {
                        format!("To continue, I still need you to choose {field_name}. Allowed values: {options}.")
                    }
                },
                crate::agent::WorkflowBlockerKind::NeedsConfirmation => match loc {
                    UiLocale::Zh => {
                        format!("要继续这一步，还需要你明确确认 {field_name}。可选值：{options}。")
                    }
                    UiLocale::En => {
                        format!(
                            "To continue, I still need you to confirm {field_name}. Allowed values: {options}."
                        )
                    }
                },
                _ => match loc {
                    UiLocale::Zh => {
                        format!("要继续这一步，还需要你告诉我 {field_name}。可选值：{options}。")
                    }
                    UiLocale::En => {
                        format!(
                            "To continue, I still need {field_name}. Allowed values: {options}."
                        )
                    }
                },
            };
        }
        return match blocker.kind {
            crate::agent::WorkflowBlockerKind::NeedsUserChoice => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你选择 {field_name}。"),
                UiLocale::En => format!("To continue, I still need you to choose {field_name}."),
            },
            crate::agent::WorkflowBlockerKind::NeedsConfirmation => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你明确确认 {field_name}。"),
                UiLocale::En => format!("To continue, I still need you to confirm {field_name}."),
            },
            _ => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你提供 {field_name}。"),
                UiLocale::En => format!("To continue, I still need {field_name}."),
            },
        };
    }
    if let Some(fields) = render_tool_clarification_field_list(blocker, loc) {
        return match blocker.kind {
            crate::agent::WorkflowBlockerKind::NeedsUserChoice => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你完成这些选择：{fields}。"),
                UiLocale::En => {
                    format!("To continue, I still need you to make these choices: {fields}.")
                }
            },
            crate::agent::WorkflowBlockerKind::NeedsConfirmation => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你完成这些确认：{fields}。"),
                UiLocale::En => {
                    format!("To continue, I still need these confirmations: {fields}.")
                }
            },
            _ => match loc {
                UiLocale::Zh => format!("要继续这一步，还需要你补充这些信息：{fields}。"),
                UiLocale::En => format!("To continue, I still need these facts: {fields}."),
            },
        };
    }
    if !blocker.missing_fields.is_empty() {
        let fields = render_tool_blocker_field_list(&blocker.missing_fields);
        return match loc {
            UiLocale::Zh => format!("要继续这一步，还需要你补充这些信息：{fields}。"),
            UiLocale::En => format!("To continue, I still need these facts: {fields}."),
        };
    }
    blocker.summary.trim().to_string()
}

/// 完整 context + worker LLM + ReAct 循环，返回执行结果与 telemetry。
/// telemetry.streamed=true 表示规范化最终答复已交付到通道，调用方应跳过 outbound_tx。
#[cfg(test)]
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
    execute_turn_boxed(
        http,
        worker_llm,
        msg,
        outbound_tx,
        req_id,
        registry,
        config,
        tool_call_repeat,
        loc,
    )
    .map(|executed| *executed)
}

/// Same turn worker as `execute_turn`, but returns the heavy telemetry bundle on
/// the heap so the ESP agent loop does not keep it resident on its caller stack.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_turn_boxed(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    outbound_tx: &OutboundTx,
    req_id: &str,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    loc: UiLocale,
) -> Result<Box<ExecutedTurn>> {
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
    let tool_policy = crate::tools::ToolPolicyContext::new(msg.ingress, msg.channel.as_ref())
        .with_runtime_mode(crate::runtime::thread_registry::runtime_mode_snapshot().current_mode);
    let has_tools = !registry.tool_specs_for_llm(&tool_policy).is_empty();
    let request_semantics = super::super::request_semantics::RequestSemantics::compile_for_turn(
        super::super::request_semantics::RequestSemanticsCompileInput {
            msg,
            active_work: active_work.as_ref(),
        },
    );
    latency.request_semantics_ms = request_semantics_started.elapsed().as_millis();
    let channel_capability = config.channel_capability_registry.get(msg.channel.as_ref());
    let editor = if config.stream_editor_channel.as_deref() == Some(msg.channel.as_ref())
        && channel_capability
            .map(|entry| entry.enabled && entry.contract.supports_stream_edit)
            .unwrap_or(false)
    {
        config.stream_editor.as_deref()
    } else {
        None
    };
    tool_ctx.supports_current_chat_outbound_message = msg.ingress == IngressKind::User
        && msg.channel.as_ref() != crate::CHANNEL_VOICE
        && editor.is_none();
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
    delivery.emit_fact(crate::agent::TurnVisibilityFact::Acknowledged);
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
        return Ok(Box::new(ExecutedTurn { outcome, telemetry }));
    }
    let initial_msg_count = messages.len();
    tool_call_repeat.clear();
    let mut final_content = String::with_capacity(4096);
    let mut memory_grounding: Option<String> = None;
    let mut tool_result_user_content = String::with_capacity(1024);
    let mut round_evidence_lines = Vec::with_capacity(MAX_TOOL_EVIDENCE_ITEMS);
    let mut successful_tool_names = std::collections::BTreeSet::new();
    let mut any_tool_round_executed = false;
    let mut any_tool_used = false;
    let mut tool_round_completion = ToolRoundCompletionTelemetry::default();
    let mut artifact_bundle = None;
    let mut external_content_used = false;
    let mut effective_reply_surface = reply_surface;
    for round in 0..MAX_REACT_ROUNDS {
        latency.react_rounds = round as u32 + 1;
        if round > 0 {
            match crate::orchestrator::refresh_heap_if_stale() {
                crate::orchestrator::PressureLevel::Normal => {}
                crate::orchestrator::PressureLevel::Cautious
                | crate::orchestrator::PressureLevel::Critical => {
                    match crate::orchestrator::can_call_llm_for_channel_pub(&msg.channel) {
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
        delivery.emit_fact(crate::agent::TurnVisibilityFact::Reasoning {
            round: round as u32 + 1,
        });
        let t0 = metrics::record_llm_call_start();
        let llm_round_start = Instant::now();
        let mut first_token_marked = latency.ttft_ms.is_some();
        let round_tools = request_plan.request_tools();
        let progress_base = worker_start;
        let mut progress_cb = |_delta: &str, accumulated: &str| {
            crate::platform::task_wdt::feed_current_task();
            if !first_token_marked && !accumulated.is_empty() {
                latency.ttft_ms = Some(progress_base.elapsed().as_millis());
                first_token_marked = true;
            }
        };
        super::log_user_turn_memory_checkpoint("agent_llm_before_request", msg);
        let response = worker_llm.chat_with_progress(
            &mut tool_ctx,
            &system,
            &messages,
            round_tools,
            request_plan.tool_choice(round, any_tool_used),
            &mut progress_cb,
        );
        super::log_user_turn_memory_checkpoint("agent_llm_after_response", msg);
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
                false,
                &content,
            );

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
            tool_round_completion.had_mutating_effects |= tool_round_output.had_mutating_effects;
            tool_round_completion.had_visible_outbound_side_effects |=
                tool_round_output.had_visible_outbound_side_effects;
            if tool_round_completion.blocker.is_none() {
                tool_round_completion.blocker = tool_round_output.blocker.clone();
            }
            if let Some(next_artifact_bundle) = tool_round_output.artifact_bundle {
                super::merge_reply_artifact_bundle(&mut artifact_bundle, next_artifact_bundle)?;
            }
            successful_tool_names.extend(tool_round_output.successful_tool_names);
            external_content_used |= tool_round_output.used_external_content;
            if tool_round_output.protocol_repair_exhausted {
                let reply = tr(UiMessage::OperationFailed, loc);
                mark_ttft_if_visible(&mut latency, worker_start, &reply);
                final_content = reply;
                break;
            }
            if let Some(blocker) = tool_round_output.blocker {
                let reply = render_programmatic_clarification_question(&blocker, loc);
                mark_ttft_if_visible(&mut latency, worker_start, &reply);
                final_content = reply;
                break;
            }
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
    if any_tool_used {
        effective_reply_surface = reply_surface.promote_for_runtime_tools(
            &successful_tool_names,
            external_content_used,
            tool_round_completion.blocker.is_some(),
            final_content.as_str(),
        );
    }
    if final_content.trim().is_empty()
        && msg.ingress == IngressKind::User
        && msg.channel.as_ref() != CHANNEL_CRON
        && !any_tool_round_executed
    {
        metrics::record_empty_final_blocked();
        return Err(crate::error::Error::config(
            crate::agent::final_reply::ReplyContractBreachKind::ProducerEmpty.stage(),
            format!(
                "reply_surface={} any_tool_used={}",
                effective_reply_surface.as_str(),
                any_tool_used,
            ),
        ));
    }
    if should_emit_regular_foreground_blocked_progress(
        request_semantics,
        any_tool_used,
        &delivery,
        tool_round_completion.blocker.as_ref(),
    ) {
        delivery.emit_fact(crate::agent::TurnVisibilityFact::TaskTerminal {
            status: crate::agent::TaskTerminalVisibilityStatus::Blocked,
        });
    }
    delivery.emit_fact(crate::agent::TurnVisibilityFact::Finalizing);
    let streamed = delivery.close_before_canonical_reply();
    let outcome = WorkerOutcome::Content(final_content);
    Ok(Box::new(ExecutedTurn {
        outcome,
        telemetry: WorkerRunTelemetry {
            streamed,
            latency,
            delivery: delivery.report(),
            artifact_bundle,
            any_tool_round_executed,
            any_tool_used,
            tool_round_completion,
            external_content_used,
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
    }))
}
