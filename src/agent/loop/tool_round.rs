#![allow(clippy::too_many_arguments)]

use super::*;

fn tool_failure_kind_from_outcome(
    failure_kind: crate::tools::ToolExecutionFailureKind,
) -> crate::agent::tool_outcome::ToolFailureKind {
    match failure_kind {
        crate::tools::ToolExecutionFailureKind::Retryable => {
            crate::agent::tool_outcome::ToolFailureKind::Retryable
        }
        crate::tools::ToolExecutionFailureKind::Permanent => {
            crate::agent::tool_outcome::ToolFailureKind::Permanent
        }
        crate::tools::ToolExecutionFailureKind::Capability => {
            crate::agent::tool_outcome::ToolFailureKind::Capability
        }
    }
}

#[cold]
#[inline(never)]
fn unavailable_tool_execution_result(tool_name: &str) -> ToolCallExecutionResult {
    metrics::record_tool_call(false);
    let assessment = unavailable_tool_assessment();
    let mut message = String::with_capacity(tool_name.len().saturating_add(56));
    message.push_str("tool '");
    message.push_str(tool_name);
    message.push_str("' is not available in the current runtime context");
    ToolCallExecutionResult {
        result_owned: crate::util::scrub_credentials(&build_json_error_object(&message)),
        failure_kind: Some(assessment.kind),
        blocker: Some(crate::agent::WorkflowBlocker::unsupported(format!(
            "tool `{tool_name}`; reason `not_available_in_current_runtime`"
        ))),
        call_succeeded: false,
        had_mutating_effects: false,
        had_visible_outbound_side_effects: false,
    }
}

fn runtime_capability_status_label(
    status: crate::orchestrator::RuntimeCapabilityStatus,
) -> &'static str {
    match status {
        crate::orchestrator::RuntimeCapabilityStatus::Online => "online",
        crate::orchestrator::RuntimeCapabilityStatus::Degraded => "degraded",
        crate::orchestrator::RuntimeCapabilityStatus::Offline => "offline",
    }
}

fn runtime_capability_reason_label(
    reason: crate::orchestrator::RuntimeCapabilityReason,
) -> &'static str {
    match reason {
        crate::orchestrator::RuntimeCapabilityReason::Nominal => "nominal",
        crate::orchestrator::RuntimeCapabilityReason::NotConfigured => "not_configured",
        crate::orchestrator::RuntimeCapabilityReason::RuntimeNotInitialized => {
            "runtime_not_initialized"
        }
        crate::orchestrator::RuntimeCapabilityReason::DeviceMissing => "device_missing",
        crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected => "device_disconnected",
        crate::orchestrator::RuntimeCapabilityReason::WorkerDead => "worker_dead",
        crate::orchestrator::RuntimeCapabilityReason::DriverError => "driver_error",
        crate::orchestrator::RuntimeCapabilityReason::PermissionDenied => "permission_denied",
        crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable => "upstream_unavailable",
        crate::orchestrator::RuntimeCapabilityReason::RecoveryStabilizing => "recovery_stabilizing",
        crate::orchestrator::RuntimeCapabilityReason::OperatorDisabled => "operator_disabled",
    }
}

fn runtime_capability_blocker_kind(
    blocker: &crate::orchestrator::RuntimeCapabilityBlocker,
) -> crate::agent::WorkflowBlockerKind {
    match blocker.capability_reason {
        crate::orchestrator::RuntimeCapabilityReason::NotConfigured
        | crate::orchestrator::RuntimeCapabilityReason::PermissionDenied
        | crate::orchestrator::RuntimeCapabilityReason::OperatorDisabled => {
            crate::agent::WorkflowBlockerKind::Unsupported
        }
        crate::orchestrator::RuntimeCapabilityReason::Nominal
        | crate::orchestrator::RuntimeCapabilityReason::RuntimeNotInitialized
        | crate::orchestrator::RuntimeCapabilityReason::DeviceMissing
        | crate::orchestrator::RuntimeCapabilityReason::DeviceDisconnected
        | crate::orchestrator::RuntimeCapabilityReason::WorkerDead
        | crate::orchestrator::RuntimeCapabilityReason::DriverError
        | crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable
        | crate::orchestrator::RuntimeCapabilityReason::RecoveryStabilizing => {
            crate::agent::WorkflowBlockerKind::RuntimeBlocked
        }
    }
}

fn runtime_capability_blocker_summary(
    tool_name: &str,
    blocker: &crate::orchestrator::RuntimeCapabilityBlocker,
) -> String {
    let mut summary = format!(
        "tool `{tool_name}`; sub_capability `{}`; status `{}`; reason `{}`",
        blocker.sub_capability,
        runtime_capability_status_label(blocker.capability_status),
        runtime_capability_reason_label(blocker.capability_reason)
    );
    if let Some(recovery_hint) = blocker.recovery_hint {
        let _ = write!(&mut summary, "; recovery_hint `{recovery_hint}`");
    }
    summary
}

#[cold]
#[inline(never)]
fn capability_blocked_tool_execution_result(
    tool_name: &str,
    blocker: &crate::orchestrator::RuntimeCapabilityBlocker,
) -> ToolCallExecutionResult {
    metrics::record_tool_call(false);
    let blocker_kind = runtime_capability_blocker_kind(blocker);
    let assessment_kind = match blocker_kind {
        crate::agent::WorkflowBlockerKind::RuntimeBlocked => {
            crate::agent::tool_outcome::ToolFailureKind::Retryable
        }
        crate::agent::WorkflowBlockerKind::Unsupported => {
            crate::agent::tool_outcome::ToolFailureKind::Capability
        }
        crate::agent::WorkflowBlockerKind::NeedsUserFacts
        | crate::agent::WorkflowBlockerKind::NeedsUserChoice
        | crate::agent::WorkflowBlockerKind::NeedsConfirmation
        | crate::agent::WorkflowBlockerKind::ProbeFailed
        | crate::agent::WorkflowBlockerKind::RetryLater
        | crate::agent::WorkflowBlockerKind::TaskBlocked => {
            crate::agent::tool_outcome::ToolFailureKind::Capability
        }
    };
    let payload = serde_json::json!({
        "error": format!(
            "tool '{}' is no longer callable because sub-capability '{}' is {:?}",
            tool_name,
            blocker.sub_capability,
            blocker.capability_status
        )
        .to_ascii_lowercase(),
        "failure_kind": "capability",
        "tool": tool_name,
        "sub_capability": blocker.sub_capability,
        "capability_status": blocker.capability_status,
        "capability_reason": blocker.capability_reason,
        "epoch": blocker.epoch,
        "epoch_changed": blocker.epoch_changed,
        "retry_guidance": "stop_retrying_until_capability_recovers",
        "recovery_hint": blocker.recovery_hint,
    });
    ToolCallExecutionResult {
        result_owned: crate::util::scrub_credentials(&payload.to_string()),
        failure_kind: Some(assessment_kind),
        blocker: Some(match blocker_kind {
            crate::agent::WorkflowBlockerKind::RuntimeBlocked => {
                crate::agent::WorkflowBlocker::runtime_blocked(runtime_capability_blocker_summary(
                    tool_name, blocker,
                ))
            }
            crate::agent::WorkflowBlockerKind::Unsupported => {
                crate::agent::WorkflowBlocker::unsupported(runtime_capability_blocker_summary(
                    tool_name, blocker,
                ))
            }
            crate::agent::WorkflowBlockerKind::NeedsUserFacts
            | crate::agent::WorkflowBlockerKind::NeedsUserChoice
            | crate::agent::WorkflowBlockerKind::NeedsConfirmation
            | crate::agent::WorkflowBlockerKind::ProbeFailed
            | crate::agent::WorkflowBlockerKind::RetryLater
            | crate::agent::WorkflowBlockerKind::TaskBlocked => {
                crate::agent::WorkflowBlocker::runtime_blocked(runtime_capability_blocker_summary(
                    tool_name, blocker,
                ))
            }
        }),
        call_succeeded: false,
        had_mutating_effects: false,
        had_visible_outbound_side_effects: false,
    }
}

#[cold]
#[inline(never)]
fn denied_tool_execution_result(tool_name: &str, reason: &str) -> ToolCallExecutionResult {
    let assessment = denied_tool_assessment(reason);
    let blocker = match assessment.kind {
        crate::agent::tool_outcome::ToolFailureKind::Retryable => {
            crate::agent::WorkflowBlocker::retry_later(format!(
                "tool `{tool_name}`; reason `{reason}`"
            ))
        }
        crate::agent::tool_outcome::ToolFailureKind::Capability
        | crate::agent::tool_outcome::ToolFailureKind::Permanent => {
            crate::agent::WorkflowBlocker::unsupported(format!(
                "tool `{tool_name}`; reason `{reason}`"
            ))
        }
    };
    ToolCallExecutionResult {
        result_owned: crate::util::scrub_credentials(&build_json_error_object(reason)),
        failure_kind: Some(assessment.kind),
        blocker: Some(blocker),
        call_succeeded: false,
        had_mutating_effects: false,
        had_visible_outbound_side_effects: false,
    }
}

#[cold]
#[inline(never)]
fn outbound_error_tool_execution_result(
    tool_name: &str,
    had_mutating_effects: bool,
    error: &crate::error::Error,
) -> ToolCallExecutionResult {
    metrics::record_tool_call(false);
    metrics::record_error_by_stage(error.metrics_stage());
    log::error!(
        "[agent_tool] {} outbound intent delivery failed: {}",
        tool_name,
        error
    );
    state::set_last_error(error);
    let assessment = classify_tool_error(error);
    let mut tool_error_buf = String::with_capacity(128);
    let _ = write!(
        &mut tool_error_buf,
        "[tool error] {}.{}",
        error, assessment.hint
    );
    ToolCallExecutionResult {
        result_owned: crate::util::scrub_credentials(tool_error_buf.as_str()),
        failure_kind: Some(assessment.kind),
        blocker: None,
        call_succeeded: false,
        had_mutating_effects,
        had_visible_outbound_side_effects: false,
    }
}

#[cold]
#[inline(never)]
fn execute_error_tool_execution_result(
    tool_name: &str,
    input: &str,
    had_mutating_effects: bool,
    error: &crate::error::Error,
) -> ToolCallExecutionResult {
    metrics::record_tool_call(false);
    metrics::record_error_by_stage(error.metrics_stage());
    log::error!(
        "[agent_tool] {} execute failed: {} input={:?}",
        tool_name,
        error,
        crate::util::truncate_content_to_max(input, 200).as_ref()
    );
    state::set_last_error(error);
    let assessment = classify_tool_error(error);
    let mut tool_error_buf = String::with_capacity(128);
    let _ = write!(
        &mut tool_error_buf,
        "[tool error] {}.{}",
        error, assessment.hint
    );
    ToolCallExecutionResult {
        result_owned: crate::util::scrub_credentials(tool_error_buf.as_str()),
        failure_kind: Some(assessment.kind),
        blocker: None,
        call_succeeded: false,
        had_mutating_effects,
        had_visible_outbound_side_effects: false,
    }
}

#[inline(never)]
fn execute_tool_call(
    tc: &crate::llm::ToolCall,
    registry: &crate::tools::ToolRegistry,
    request_plan: &AgentRequestPlan,
    delivery: &mut DeliverySession,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> ToolCallExecutionResult {
    if !registry.is_llm_tool_visible(&tc.name, request_plan.policy()) {
        if let Some(blocker) = registry.runtime_capability_blocker(&tc.name) {
            return capability_blocked_tool_execution_result(&tc.name, &blocker);
        }
        return unavailable_tool_execution_result(&tc.name);
    }

    let permit = match registry.assess_llm_execution(&tc.name, &tc.input, request_plan.policy()) {
        Ok(crate::tools::ToolExecutionGateDecision::Allow(permit)) => permit,
        Ok(crate::tools::ToolExecutionGateDecision::Deny { reason }) => {
            log::info!("[agent_tool] {} denied by governance: {}", tc.name, reason);
            return denied_tool_execution_result(&tc.name, &reason);
        }
        Err(error) => {
            return execute_error_tool_execution_result(&tc.name, &tc.input, false, &error);
        }
    };
    let needs_net = permit.requires_network();
    match crate::orchestrator::can_execute_tool_pub(&tc.name, needs_net) {
        ToolDecision::Deny { reason } => {
            log::info!("[agent_tool] {} denied: {}", tc.name, reason);
            if let Err(error) = registry.record_resource_denial(&permit, reason) {
                log::warn!(
                    "[agent_tool] {} failed to persist resource denial audit: {}",
                    tc.name,
                    error
                );
            }
            denied_tool_execution_result(&tc.name, reason)
        }
        ToolDecision::Allow => {
            if let Some(blocker) = registry.runtime_capability_blocker(&tc.name) {
                return capability_blocked_tool_execution_result(&tc.name, &blocker);
            }
            let tool_exec_start = Instant::now();
            let had_mutating_effects = permit.shape().effect_class.is_mutating();
            match registry.execute_permitted(&permit, &tc.input, tool_ctx) {
                Ok(outcome) => {
                    latency.tool_exec_ms = latency
                        .tool_exec_ms
                        .saturating_add(tool_exec_start.elapsed().as_millis());
                    let mut had_visible_outbound_side_effects = false;
                    for intent in &outcome.outbound_intents {
                        match delivery.deliver_tool_outbound_intent(intent) {
                            Ok(ToolIntentDelivery::VisibleUpdate) => {
                                had_visible_outbound_side_effects = true;
                                log_tool_intent_result(
                                    &tc.name,
                                    intent,
                                    ToolIntentDelivery::VisibleUpdate,
                                );
                            }
                            Ok(ToolIntentDelivery::Suppressed) => {
                                log_tool_intent_result(
                                    &tc.name,
                                    intent,
                                    ToolIntentDelivery::Suppressed,
                                );
                            }
                            Err(error) => {
                                latency.tool_exec_ms = latency.tool_exec_ms.saturating_add(0);
                                return outbound_error_tool_execution_result(
                                    &tc.name,
                                    had_mutating_effects,
                                    &error,
                                );
                            }
                        }
                    }
                    if let Some(blocker) = outcome.blocker {
                        metrics::record_tool_call(false);
                        ToolCallExecutionResult {
                            result_owned: crate::util::scrub_credentials(&outcome.content),
                            failure_kind: outcome.failure_kind.map(tool_failure_kind_from_outcome),
                            blocker: Some(crate::agent::workflow_blocker_from_tool_blocker(
                                &blocker,
                            )),
                            call_succeeded: false,
                            had_mutating_effects,
                            had_visible_outbound_side_effects,
                        }
                    } else if let Some(failure_kind) = outcome.failure_kind {
                        metrics::record_tool_call(false);
                        ToolCallExecutionResult {
                            result_owned: crate::util::scrub_credentials(&outcome.content),
                            failure_kind: Some(tool_failure_kind_from_outcome(failure_kind)),
                            blocker: None,
                            call_succeeded: false,
                            had_mutating_effects,
                            had_visible_outbound_side_effects,
                        }
                    } else {
                        metrics::record_tool_call(true);
                        ToolCallExecutionResult {
                            result_owned: crate::util::scrub_credentials(&outcome.content),
                            failure_kind: None,
                            blocker: None,
                            call_succeeded: true,
                            had_mutating_effects,
                            had_visible_outbound_side_effects,
                        }
                    }
                }
                Err(error) => {
                    if let Some(blocker) = registry.runtime_capability_blocker(&tc.name) {
                        return capability_blocked_tool_execution_result(&tc.name, &blocker);
                    }
                    latency.tool_exec_ms = latency
                        .tool_exec_ms
                        .saturating_add(tool_exec_start.elapsed().as_millis());
                    if let Err(audit_error) = registry.record_execution_failure(&permit, &error) {
                        log::warn!(
                            "[agent_tool] {} failed to persist failure audit: {}",
                            tc.name,
                            audit_error
                        );
                    }
                    execute_error_tool_execution_result(
                        &tc.name,
                        &tc.input,
                        had_mutating_effects,
                        &error,
                    )
                }
            }
        }
    }
}

#[inline(never)]
pub(super) fn execute_tool_use_round(
    tool_calls: &[crate::llm::ToolCall],
    delivery: &mut DeliverySession,
    request_plan: &AgentRequestPlan,
    registry: &crate::tools::ToolRegistry,
    tool_ctx: &mut HttpClientToolContext<'_>,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    latency: &mut WorkerLatency,
    tool_result_user_content: &mut String,
    round_evidence_lines: &mut Vec<String>,
) -> ToolUseRoundExecutionOutput {
    let mut truncated = false;
    let mut round_tool_success = false;
    let mut omitted_evidence_count = 0usize;
    let mut used_external_content = false;
    let mut had_mutating_effects = false;
    let mut had_visible_outbound_side_effects = false;
    let mut successful_tool_names = Vec::with_capacity(tool_calls.len());
    let mut blocker = None;

    latency.tool_calls = latency.tool_calls.saturating_add(tool_calls.len() as u32);

    for (i, tc) in tool_calls.iter().enumerate() {
        delivery.emit_tool_progress(&tc.name, i, tool_calls.len());

        let execution = execute_tool_call(tc, registry, request_plan, delivery, tool_ctx, latency);
        had_mutating_effects |= execution.had_mutating_effects;
        had_visible_outbound_side_effects |= execution.had_visible_outbound_side_effects;
        if blocker.is_none() {
            blocker = execution.blocker.clone();
        }
        let result_view = execution.result_owned.as_str();
        if execution.call_succeeded && config.strategy == AgentRunStrategy::LinuxEnhanced {
            used_external_content |= tool_result_uses_external_content(&tc.name, result_view);
            if request_plan.reply_surface().accepts_tool_evidence(&tc.name)
                && round_evidence_lines.len() < MAX_TOOL_EVIDENCE_ITEMS
            {
                if let Some(line) = build_tool_evidence_line(&tc.id, &tc.name, result_view) {
                    round_evidence_lines.push(line);
                }
            } else {
                omitted_evidence_count = omitted_evidence_count.saturating_add(1);
            }
        }
        if execution.call_succeeded {
            round_tool_success = true;
            successful_tool_names.push(tc.name.clone());
        }

        let call_key = hash_tool_call(&tc.name, &tc.input);
        let n = tool_call_repeat.entry(call_key).or_insert(0);
        *n = (*n).saturating_add(1);
        let repeat_count = *n as usize;
        crate::platform::task_wdt::feed_current_task();
        if i > 0
            && push_bounded_utf8(
                tool_result_user_content,
                "\n",
                MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
            )
        {
            truncated = true;
            break;
        }
        if append_tool_result_block(
            tool_result_user_content,
            ToolResultBlock {
                call_id: &tc.id,
                tool_name: &tc.name,
                status: tool_result_status_attr(!execution.call_succeeded),
                failure: failure_kind_attr(execution.failure_kind),
                repeat_count,
                content: result_view,
            },
            MAX_TOOL_RESULTS_USER_MESSAGE_LEN,
        ) {
            truncated = true;
            break;
        }
        if execution.blocker.is_some() {
            break;
        }
    }

    ToolUseRoundExecutionOutput {
        truncated,
        round_tool_success,
        used_external_content,
        had_mutating_effects,
        had_visible_outbound_side_effects,
        omitted_evidence_count,
        successful_tool_names,
        blocker,
    }
}

fn tool_result_uses_external_content(tool_name: &str, result: &str) -> bool {
    match tool_name {
        "web_fetch" | "pdf_read" | "web_search" => true,
        "document_read" | "document_extract" => json_field_is_external_url(result, &["source"]),
        "memory_get" => json_field_is_external_url(result, &["record", "citation"]),
        _ => false,
    }
}

fn json_field_is_external_url(result: &str, path: &[&str]) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(result) else {
        return false;
    };
    let mut current = &value;
    for segment in path {
        let Some(next) = current.get(*segment) else {
            return false;
        };
        current = next;
    }
    current
        .as_str()
        .is_some_and(|text| text.starts_with("http://") || text.starts_with("https://"))
}

fn tool_intent_target_attr(intent: &ToolOutboundIntent) -> &'static str {
    match intent.target {
        ToolOutboundTarget::CurrentChat => "current",
        ToolOutboundTarget::Explicit { .. } => "explicit",
    }
}

fn tool_intent_delivery_attr(intent: &ToolOutboundIntent) -> &'static str {
    match intent.delivery_kind {
        ToolOutboundDeliveryKind::Supplemental => "supplemental",
        ToolOutboundDeliveryKind::Primary => "primary",
    }
}

pub(super) fn log_tool_intent_result(
    tool_name: &str,
    intent: &ToolOutboundIntent,
    result: ToolIntentDelivery,
) {
    let target = tool_intent_target_attr(intent);
    let delivery_kind = tool_intent_delivery_attr(intent);
    match result {
        ToolIntentDelivery::VisibleUpdate => {
            log::info!(
                "[agent_tool] {} outbound update delivered target={} delivery={}",
                tool_name,
                target,
                delivery_kind
            );
        }
        ToolIntentDelivery::Suppressed => {
            log::debug!(
                "[agent_tool] {} outbound intent suppressed target={} delivery={}",
                tool_name,
                target,
                delivery_kind
            );
        }
    }
}
