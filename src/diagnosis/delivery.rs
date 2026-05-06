use crate::diagnosis::{
    DiagnosisAction, DiagnosisConfidence, DiagnosisDegradation, DiagnosisEvidence,
    DiagnosisFinding, DiagnosisKind, DiagnosisResult, DiagnosisRootCause,
};
use crate::metrics::MetricsSnapshot;
use crate::orchestrator::{RuntimeCapabilityState, RuntimeCapabilityStatus};

pub struct DeliveryDiagnosisInput<'a> {
    pub enabled_channel: Option<&'a str>,
    pub metrics: MetricsSnapshot,
    pub runtime_capabilities: Vec<RuntimeCapabilityState>,
}

pub fn build_delivery_diagnosis(input: DeliveryDiagnosisInput<'_>) -> DiagnosisResult {
    let mut findings = Vec::new();
    let mut suspected_root_causes = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut evidence = vec![
        DiagnosisEvidence::new(
            "enabled_channel",
            input.enabled_channel.unwrap_or("unknown"),
        ),
        DiagnosisEvidence::new(
            "dispatch_send_fail_total",
            input.metrics.dispatch_send_fail.to_string(),
        ),
        DiagnosisEvidence::new(
            "outbound_enqueue_fail_total",
            input.metrics.outbound_enqueue_fail.to_string(),
        ),
        DiagnosisEvidence::new(
            "tool_succeeded_final_drift_total",
            input.metrics.tool_succeeded_final_drift_total.to_string(),
        ),
        DiagnosisEvidence::new(
            "empty_final_blocked_total",
            input.metrics.empty_final_blocked_total.to_string(),
        ),
    ];
    let mut degraded_by = Vec::new();
    let mut confidence = DiagnosisConfidence::Medium;
    let mut summary =
        "No recent delivery failures were recorded in the current runtime snapshot.".to_string();

    if input.metrics.dispatch_send_fail > 0 || input.metrics.outbound_enqueue_fail > 0 {
        summary = format!(
            "Recent delivery failures were recorded for {}.",
            input.enabled_channel.unwrap_or("the active channel")
        );
        confidence = DiagnosisConfidence::High;
        findings.push(DiagnosisFinding::observed(
            "dispatch/outbound delivery failures were recorded recently",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "delivery_failure",
            "delivery handoff is failing after the agent produced a reply",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_channel_connectivity",
            "inspect /api/channel_connectivity?channel=<enabled_channel> and sender/dispatch failure counters",
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_operator_status",
            "review operator status reply pipeline and runtime mode snapshots",
        ));
    }

    if input.metrics.tool_succeeded_final_drift_total > 0
        || input.metrics.empty_final_blocked_total > 0
    {
        findings.push(DiagnosisFinding::correlated(
            "reply pipeline instability was observed alongside delivery activity",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "reply_pipeline_instability",
            "finalization or delivery handoff instability may be suppressing visible replies",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_reply_pipeline",
            "check reply pipeline timing and empty-final counters in operator status",
        ));
    }

    for capability in input.runtime_capabilities {
        if capability.status == RuntimeCapabilityStatus::Offline {
            degraded_by.push(DiagnosisDegradation::new(
                capability.id,
                format!("runtime capability {} is offline", capability.id),
            ));
            evidence.push(DiagnosisEvidence::new(
                format!("runtime_capability.{}.status", capability.id),
                "offline",
            ));
        } else if capability.status == RuntimeCapabilityStatus::Degraded {
            degraded_by.push(DiagnosisDegradation::new(
                capability.id,
                format!("runtime capability {} is degraded", capability.id),
            ));
            evidence.push(DiagnosisEvidence::new(
                format!("runtime_capability.{}.status", capability.id),
                "degraded",
            ));
        }
    }

    if !degraded_by.is_empty() {
        if suspected_root_causes.is_empty() {
            summary =
                "Runtime capability degradation may be affecting delivery stability.".to_string();
        }
        suspected_root_causes.push(DiagnosisRootCause::new(
            "runtime_capability_degraded",
            "one or more runtime capabilities are degraded or offline during diagnosis",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_runtime_capabilities",
            "review degraded/offline runtime capabilities before retrying delivery",
        ));
    }

    DiagnosisResult {
        kind: DiagnosisKind::Delivery,
        summary,
        findings,
        suspected_root_causes,
        recommended_next_steps,
        evidence,
        confidence,
        degraded_by,
        safe_actions_available: vec![
            "inspect_channel_connectivity".to_string(),
            "inspect_operator_status".to_string(),
        ],
    }
}

pub fn build_delivery_diagnosis_from_runtime(enabled_channel: Option<&str>) -> DiagnosisResult {
    build_delivery_diagnosis(DeliveryDiagnosisInput {
        enabled_channel,
        metrics: crate::metrics::snapshot(),
        runtime_capabilities: crate::orchestrator::runtime_capability_snapshot(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{
        RuntimeCapabilityReason, RuntimeCapabilityState, RuntimeCapabilityStatus,
    };

    #[test]
    fn delivery_diagnosis_marks_delivery_failure_as_root_cause() {
        let before = crate::metrics::snapshot();
        crate::metrics::record_dispatch_send(false);
        crate::metrics::record_outbound_enqueue_fail();

        let diagnosis = build_delivery_diagnosis(DeliveryDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            metrics: crate::metrics::snapshot(),
            runtime_capabilities: crate::orchestrator::runtime_capability_snapshot(),
        });

        assert_eq!(diagnosis.kind, DiagnosisKind::Delivery);
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "delivery_failure"));
        assert!(diagnosis
            .recommended_next_steps
            .iter()
            .any(|step| step.code == "inspect_channel_connectivity"));
        assert!(crate::metrics::snapshot().dispatch_send_fail >= before.dispatch_send_fail);
        assert!(crate::metrics::snapshot().outbound_enqueue_fail >= before.outbound_enqueue_fail);
    }

    #[test]
    fn delivery_diagnosis_reports_degraded_runtime_capability() {
        let diagnosis = build_delivery_diagnosis(DeliveryDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            metrics: crate::metrics::snapshot(),
            runtime_capabilities: vec![RuntimeCapabilityState {
                id: crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
                status: RuntimeCapabilityStatus::Offline,
                reason: RuntimeCapabilityReason::UpstreamUnavailable,
                epoch: 1,
                changed_at_secs: 1,
                observed_at_secs: 1,
                active_calls: 0,
                draining: false,
                last_transition_uptime_ms: 0,
                drain_denied_total: 0,
                recovery_hint: Some("wait_for_network_recovery"),
            }],
        });

        assert!(!diagnosis.degraded_by.is_empty());
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "runtime_capability_degraded"));
    }
}
