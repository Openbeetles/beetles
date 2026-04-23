use crate::diagnosis::{
    DiagnosisAction, DiagnosisConfidence, DiagnosisDegradation, DiagnosisEvidence,
    DiagnosisFinding, DiagnosisKind, DiagnosisResult, DiagnosisRootCause,
};
use crate::metrics::MetricsSnapshot;
use crate::orchestrator::{
    PressureLevel, ResourceSnapshot, RuntimeCapabilityState, RuntimeCapabilityStatus,
};

pub struct SystemDiagnosisInput<'a> {
    pub enabled_channel: Option<&'a str>,
    pub resource: ResourceSnapshot,
    pub metrics: MetricsSnapshot,
    pub runtime_capabilities: Vec<RuntimeCapabilityState>,
    pub presence_state: &'a str,
    pub runtime_mode: &'a str,
    pub os_closure_ready: bool,
    pub wifi_connected: bool,
}

pub(crate) fn visit_runtime_capability_degradations(
    runtime_capabilities: &[RuntimeCapabilityState],
    mut on_degradation: impl FnMut(&RuntimeCapabilityState, RuntimeCapabilityStatus),
) {
    for capability in runtime_capabilities {
        match capability.status {
            RuntimeCapabilityStatus::Offline
            | RuntimeCapabilityStatus::Degraded
            | RuntimeCapabilityStatus::Online => on_degradation(capability, capability.status),
        }
    }
}

pub fn build_system_diagnosis(input: SystemDiagnosisInput<'_>) -> DiagnosisResult {
    let mut findings = Vec::new();
    let mut suspected_root_causes = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut degraded_by = Vec::new();
    let mut evidence = vec![
        DiagnosisEvidence::new(
            "enabled_channel",
            input.enabled_channel.unwrap_or("unknown"),
        ),
        DiagnosisEvidence::new(
            "pressure",
            format!("{:?}", input.resource.pressure).to_ascii_lowercase(),
        ),
        DiagnosisEvidence::new("presence_state", input.presence_state),
        DiagnosisEvidence::new("runtime_mode", input.runtime_mode),
        DiagnosisEvidence::new("wifi_connected", input.wifi_connected.to_string()),
        DiagnosisEvidence::new("os_closure_ready", input.os_closure_ready.to_string()),
        DiagnosisEvidence::new(
            "active_http_count",
            input.resource.active_http_count.to_string(),
        ),
        DiagnosisEvidence::new(
            "active_wss_count",
            input.resource.active_wss_count.to_string(),
        ),
        DiagnosisEvidence::new(
            "active_agent_tasks",
            input.resource.active_agent_tasks.to_string(),
        ),
        DiagnosisEvidence::new("inbound_depth", input.resource.inbound_depth.to_string()),
        DiagnosisEvidence::new("outbound_depth", input.resource.outbound_depth.to_string()),
        DiagnosisEvidence::new("llm_last_ms", input.metrics.llm_last_ms.to_string()),
        DiagnosisEvidence::new(
            "tool_exec_last_ms",
            input.metrics.tool_exec_last_ms.to_string(),
        ),
    ];
    let mut confidence = DiagnosisConfidence::Medium;
    let mut summary = "The current runtime snapshot looks generally stable.".to_string();

    if input.resource.pressure == PressureLevel::Critical {
        summary = "The system is under critical resource pressure.".to_string();
        confidence = DiagnosisConfidence::High;
        findings.push(DiagnosisFinding::observed(
            "resource pressure is currently critical",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "critical_resource_pressure",
            "resource admission is in a protective state and can delay or suppress runtime work",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_resource_pressure",
            "inspect resource pressure, queues, and active transports before retrying user work",
        ));
    } else if input.resource.pressure == PressureLevel::Cautious {
        summary = "The system is running in a cautious resource state.".to_string();
        findings.push(DiagnosisFinding::observed(
            "resource pressure is elevated above normal",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "elevated_resource_pressure",
            "resource pressure is elevated and may increase latency or defer non-critical work",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_resource_pressure",
            "inspect current queues and active transport counts if latency persists",
        ));
    }

    if !input.os_closure_ready {
        findings.push(DiagnosisFinding::correlated(
            "Beetle OS closure is not fully ready in the current runtime snapshot",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "os_closure_not_ready",
            "one or more required runtime planes are not ready yet",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_operator_status",
            "inspect operator status presence, initiative, and os_closure planes",
        ));
    }

    if !input.wifi_connected {
        findings.push(DiagnosisFinding::observed(
            "wifi station is currently disconnected",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "wifi_disconnected",
            "network-dependent capabilities may be degraded while WiFi is disconnected",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_network_state",
            "inspect WiFi and network path state before blaming higher-level agent behavior",
        ));
    }

    visit_runtime_capability_degradations(&input.runtime_capabilities, |capability, status| {
        match status {
            RuntimeCapabilityStatus::Offline => {
                degraded_by.push(DiagnosisDegradation::new(
                    capability.id,
                    format!("runtime capability {} is offline", capability.id),
                ));
                evidence.push(DiagnosisEvidence::new(
                    format!("runtime_capability.{}.status", capability.id),
                    "offline",
                ));
            }
            RuntimeCapabilityStatus::Degraded => {
                degraded_by.push(DiagnosisDegradation::new(
                    capability.id,
                    format!("runtime capability {} is degraded", capability.id),
                ));
                evidence.push(DiagnosisEvidence::new(
                    format!("runtime_capability.{}.status", capability.id),
                    "degraded",
                ));
            }
            RuntimeCapabilityStatus::Online => {}
        }
    });

    if !degraded_by.is_empty() {
        findings.push(DiagnosisFinding::correlated(
            "runtime capabilities are degraded or offline in the current snapshot",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "runtime_capability_degraded",
            "capability degradation is contributing to the current system state",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_runtime_capabilities",
            "inspect degraded/offline runtime capabilities and recent recovery hints",
        ));
    }

    DiagnosisResult {
        kind: DiagnosisKind::System,
        summary,
        findings,
        suspected_root_causes,
        recommended_next_steps,
        evidence,
        confidence,
        degraded_by,
        safe_actions_available: vec![
            "inspect_resource_pressure".to_string(),
            "inspect_operator_status".to_string(),
            "inspect_runtime_capabilities".to_string(),
        ],
    }
}

pub fn build_system_diagnosis_from_runtime(
    platform: &dyn crate::Platform,
    enabled_channel: Option<&str>,
) -> DiagnosisResult {
    let now = crate::util::current_unix_secs();
    let presence = crate::runtime::inspect_platform_presence(platform, now);
    let initiative = crate::runtime::inspect_platform_initiative(platform, now);
    let os_closure = crate::runtime::inspect_beetle_os_closure(&presence, &initiative);
    build_system_diagnosis(SystemDiagnosisInput {
        enabled_channel,
        resource: crate::orchestrator::snapshot(),
        metrics: crate::metrics::snapshot(),
        runtime_capabilities: crate::orchestrator::runtime_capability_snapshot(),
        presence_state: presence.state.as_str(),
        runtime_mode: presence.runtime_mode.current_mode.as_str(),
        os_closure_ready: os_closure.ready,
        wifi_connected: presence.wifi_connected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_resource() -> ResourceSnapshot {
        crate::orchestrator::snapshot()
    }

    #[test]
    fn system_diagnosis_marks_critical_pressure_as_root_cause() {
        let mut resource = base_resource();
        resource.pressure = PressureLevel::Critical;

        let diagnosis = build_system_diagnosis(SystemDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            resource,
            metrics: crate::metrics::snapshot(),
            runtime_capabilities: Vec::new(),
            presence_state: "busy",
            runtime_mode: "normal",
            os_closure_ready: true,
            wifi_connected: true,
        });

        assert_eq!(diagnosis.kind, DiagnosisKind::System);
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "critical_resource_pressure"));
    }

    #[test]
    fn system_diagnosis_reports_runtime_capability_degradation() {
        let diagnosis = build_system_diagnosis(SystemDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            resource: base_resource(),
            metrics: crate::metrics::snapshot(),
            runtime_capabilities: vec![crate::orchestrator::RuntimeCapabilityState {
                id: crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
                status: crate::orchestrator::RuntimeCapabilityStatus::Offline,
                reason: crate::orchestrator::RuntimeCapabilityReason::UpstreamUnavailable,
                epoch: 1,
                changed_at_secs: 1,
                observed_at_secs: 1,
                recovery_hint: Some("wait_for_network_recovery"),
            }],
            presence_state: "busy",
            runtime_mode: "normal",
            os_closure_ready: true,
            wifi_connected: true,
        });

        assert!(!diagnosis.degraded_by.is_empty());
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "runtime_capability_degraded"));
    }
}
