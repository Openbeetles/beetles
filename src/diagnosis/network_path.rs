use crate::channels::ChannelConnectivitySnapshot;
use crate::diagnosis::system::visit_runtime_capability_degradations;
use crate::diagnosis::{
    DiagnosisAction, DiagnosisConfidence, DiagnosisDegradation, DiagnosisEvidence,
    DiagnosisFinding, DiagnosisKind, DiagnosisResult, DiagnosisRootCause,
};
use crate::i18n::Locale;
use crate::orchestrator::{
    PressureLevel, RuntimeCapabilityState, RuntimeCapabilityStatus, TlsFragmentationRisk,
};

pub struct NetworkPathDiagnosisInput<'a> {
    pub enabled_channel: Option<&'a str>,
    pub wifi_connected: bool,
    pub pressure: PressureLevel,
    pub tls_fragmentation_risk: TlsFragmentationRisk,
    pub runtime_capabilities: Vec<RuntimeCapabilityState>,
    pub proxy_configured: bool,
    pub connectivity_snapshot: Option<ChannelConnectivitySnapshot>,
    pub dns_nameserver_count: Option<usize>,
    pub default_route_available: Option<bool>,
    pub probe_error: Option<String>,
}

pub fn build_network_path_diagnosis(input: NetworkPathDiagnosisInput<'_>) -> DiagnosisResult {
    let mut findings = Vec::new();
    let mut suspected_root_causes = Vec::new();
    let mut recommended_next_steps = Vec::new();
    let mut degraded_by = Vec::new();
    let mut evidence = vec![
        DiagnosisEvidence::new(
            "enabled_channel",
            input.enabled_channel.unwrap_or("unknown"),
        ),
        DiagnosisEvidence::new("wifi_connected", input.wifi_connected.to_string()),
        DiagnosisEvidence::new(
            "pressure",
            format!("{:?}", input.pressure).to_ascii_lowercase(),
        ),
        DiagnosisEvidence::new(
            "tls_fragmentation_risk",
            format!("{:?}", input.tls_fragmentation_risk).to_ascii_lowercase(),
        ),
        DiagnosisEvidence::new("proxy_configured", input.proxy_configured.to_string()),
    ];
    let mut confidence = DiagnosisConfidence::Medium;
    let mut summary = "The current network path snapshot looks generally reachable.".to_string();

    if let Some(count) = input.dns_nameserver_count {
        evidence.push(DiagnosisEvidence::new(
            "dns_nameserver_count",
            count.to_string(),
        ));
        if count == 0 {
            findings.push(DiagnosisFinding::observed(
                "the host resolver currently has no nameservers configured",
            ));
            suspected_root_causes.push(DiagnosisRootCause::new(
                "dns_unavailable",
                "dns resolution is likely to fail because no nameserver is configured",
                DiagnosisConfidence::High,
            ));
            recommended_next_steps.push(DiagnosisAction::new(
                "inspect_dns_config",
                "inspect resolver configuration before blaming upstream services",
            ));
            summary = "The current network path is missing DNS configuration.".to_string();
            confidence = DiagnosisConfidence::High;
        }
    }

    if let Some(has_route) = input.default_route_available {
        evidence.push(DiagnosisEvidence::new(
            "default_route_available",
            has_route.to_string(),
        ));
        if !has_route {
            findings.push(DiagnosisFinding::observed(
                "no default route is currently visible on the host network snapshot",
            ));
            suspected_root_causes.push(DiagnosisRootCause::new(
                "default_route_missing",
                "outbound requests may fail because the network stack has no default route",
                DiagnosisConfidence::High,
            ));
            recommended_next_steps.push(DiagnosisAction::new(
                "inspect_default_route",
                "inspect the default route and gateway before retrying outbound traffic",
            ));
            if confidence != DiagnosisConfidence::High {
                summary = "The current network path has no visible default route.".to_string();
                confidence = DiagnosisConfidence::High;
            }
        }
    }

    if !input.wifi_connected {
        findings.push(DiagnosisFinding::observed(
            "wifi station is currently disconnected",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "wifi_disconnected",
            "outbound network traffic is unavailable because the board is not connected to WiFi",
            DiagnosisConfidence::High,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_wifi_state",
            "inspect WiFi station state before retrying channel or upstream requests",
        ));
        summary = "The current network path is blocked by WiFi disconnection.".to_string();
        confidence = DiagnosisConfidence::High;
    }

    if matches!(
        input.tls_fragmentation_risk,
        TlsFragmentationRisk::Cautious | TlsFragmentationRisk::Critical
    ) {
        findings.push(DiagnosisFinding::correlated(
            "tls fragmentation risk is elevated and may suppress live probes or outbound connects",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "tls_fragmentation_risk",
            "resource fragmentation is increasing the risk of outbound tls admission failures",
            DiagnosisConfidence::Medium,
        ));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_resource_pressure",
            "inspect heap fragmentation and resource pressure before retrying network-heavy work",
        ));
        if confidence != DiagnosisConfidence::High {
            summary = "The current network path is guarded by elevated TLS fragmentation risk."
                .to_string();
        }
    }

    if let Some(snapshot) = input.connectivity_snapshot.as_ref() {
        evidence.push(DiagnosisEvidence::new(
            "connectivity_snapshot_stale",
            snapshot.stale.to_string(),
        ));
        if let Some(active_item) = snapshot
            .channels
            .iter()
            .find(|item| item.id == input.enabled_channel.unwrap_or_default())
        {
            evidence.push(DiagnosisEvidence::new(
                format!("channel.{}.configured", active_item.id),
                active_item.configured.to_string(),
            ));
            evidence.push(DiagnosisEvidence::new(
                format!("channel.{}.ok", active_item.id),
                active_item.ok.to_string(),
            ));
            if let Some(message_key) = active_item.message_key {
                evidence.push(DiagnosisEvidence::new(
                    format!("channel.{}.message_key", active_item.id),
                    message_key,
                ));
            }
            if !active_item.configured {
                findings.push(DiagnosisFinding::observed(
                    "the active channel is not fully configured for outbound connectivity",
                ));
                suspected_root_causes.push(DiagnosisRootCause::new(
                    "active_channel_not_configured",
                    "the active channel cannot pass connectivity checks because credentials or endpoint configuration are incomplete",
                    DiagnosisConfidence::High,
                ));
                recommended_next_steps.push(DiagnosisAction::new(
                    "inspect_channel_config",
                    "inspect active channel configuration before retrying outbound delivery",
                ));
                if confidence != DiagnosisConfidence::High {
                    summary = format!(
                        "The active channel {} is not fully configured for outbound connectivity.",
                        active_item.id
                    );
                    confidence = DiagnosisConfidence::High;
                }
            } else if !active_item.ok {
                findings.push(DiagnosisFinding::correlated(
                    "the active channel connectivity probe is currently failing",
                ));
                suspected_root_causes.push(DiagnosisRootCause::new(
                    "active_channel_connectivity_failed",
                    "the active channel probe failed, so outbound delivery may be blocked above the raw network layer",
                    DiagnosisConfidence::High,
                ));
                recommended_next_steps.push(DiagnosisAction::new(
                    "inspect_channel_connectivity",
                    "inspect the active channel connectivity result and last failure message",
                ));
                if confidence != DiagnosisConfidence::High {
                    summary = format!(
                        "The active channel {} is currently failing its connectivity probe.",
                        active_item.id
                    );
                    confidence = DiagnosisConfidence::High;
                }
            } else {
                findings.push(DiagnosisFinding::observed(
                    "the active channel connectivity probe is currently passing",
                ));
            }
        }
        if snapshot.stale {
            findings.push(DiagnosisFinding::correlated(
                "live connectivity probing is currently suppressed, so the snapshot is stale",
            ));
            suspected_root_causes.push(DiagnosisRootCause::new(
                "live_probe_suppressed",
                "the runtime is intentionally avoiding a live probe because outbound readiness is not settled yet",
                DiagnosisConfidence::Medium,
            ));
            recommended_next_steps.push(DiagnosisAction::new(
                "inspect_outbound_readiness",
                "inspect outbound readiness, WiFi settle state, and TLS fragmentation guardrails",
            ));
            if summary == "The current network path snapshot looks generally reachable." {
                summary =
                    "The current network snapshot is stale because live probing is suppressed."
                        .to_string();
            }
        }
    }

    if let Some(error) = input.probe_error.as_deref() {
        findings.push(DiagnosisFinding::correlated(
            "capturing a live network-path snapshot hit a runtime probe error",
        ));
        suspected_root_causes.push(DiagnosisRootCause::new(
            "network_probe_error",
            "the runtime could not complete its network-path probe, so diagnosis depth is partially reduced",
            DiagnosisConfidence::Medium,
        ));
        evidence.push(DiagnosisEvidence::new("probe_error", error));
        recommended_next_steps.push(DiagnosisAction::new(
            "inspect_network_probe_runtime",
            "inspect http client creation or probe-stage failures before assuming the path is fully healthy",
        ));
        if summary == "The current network path snapshot looks generally reachable." {
            summary = "The network-path probe could not complete cleanly in the current runtime."
                .to_string();
        }
    }

    visit_runtime_capability_degradations(&input.runtime_capabilities, |capability, status| {
        if capability.id != crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP {
            return;
        }
        match status {
            RuntimeCapabilityStatus::Offline => {
                degraded_by.push(DiagnosisDegradation::new(
                    capability.id,
                    "runtime capability network.outbound_http is offline",
                ));
                suspected_root_causes.push(DiagnosisRootCause::new(
                        "network_outbound_capability_offline",
                        "the runtime has marked outbound http as offline, so upstream network work is currently blocked",
                        DiagnosisConfidence::High,
                    ));
                recommended_next_steps.push(DiagnosisAction::new(
                    "inspect_runtime_capabilities",
                    "inspect runtime capability degradation and recent network recovery hints",
                ));
                evidence.push(DiagnosisEvidence::new(
                    "runtime_capability.network.outbound_http.status",
                    "offline",
                ));
                summary = "The current network path is degraded because outbound HTTP is offline."
                    .to_string();
                confidence = DiagnosisConfidence::High;
            }
            RuntimeCapabilityStatus::Degraded => {
                degraded_by.push(DiagnosisDegradation::new(
                    capability.id,
                    "runtime capability network.outbound_http is degraded",
                ));
                suspected_root_causes.push(DiagnosisRootCause::new(
                        "network_outbound_capability_degraded",
                        "the runtime has marked outbound http as degraded, so upstream network work may be unstable",
                        DiagnosisConfidence::Medium,
                    ));
                recommended_next_steps.push(DiagnosisAction::new(
                    "inspect_runtime_capabilities",
                    "inspect runtime capability degradation and recent network recovery hints",
                ));
                evidence.push(DiagnosisEvidence::new(
                    "runtime_capability.network.outbound_http.status",
                    "degraded",
                ));
                if confidence != DiagnosisConfidence::High {
                    summary =
                        "The current network path is degraded because outbound HTTP is unstable."
                            .to_string();
                }
            }
            RuntimeCapabilityStatus::Online => {
                evidence.push(DiagnosisEvidence::new(
                    "runtime_capability.network.outbound_http.status",
                    "online",
                ));
            }
        }
    });

    DiagnosisResult {
        kind: DiagnosisKind::NetworkPath,
        summary,
        findings,
        suspected_root_causes,
        recommended_next_steps,
        evidence,
        confidence,
        degraded_by,
        safe_actions_available: vec![
            "inspect_wifi_state".to_string(),
            "inspect_channel_connectivity".to_string(),
            "inspect_dns_config".to_string(),
            "inspect_default_route".to_string(),
            "inspect_runtime_capabilities".to_string(),
            "inspect_resource_pressure".to_string(),
        ],
    }
}

pub fn build_network_path_diagnosis_from_runtime(
    platform: &dyn crate::Platform,
    config: &crate::config::AppConfig,
    loc: Locale,
) -> DiagnosisResult {
    let wifi_connected = crate::state::wifi_sta_connected();
    let pressure = crate::orchestrator::snapshot().pressure;
    let tls_fragmentation_risk = crate::orchestrator::current_tls_fragmentation_risk();
    let runtime_capabilities = crate::orchestrator::runtime_capability_snapshot();
    let proxy_configured = !config.proxy_url.trim().is_empty();
    let (connectivity_snapshot, probe_error) = capture_connectivity_snapshot(platform, config, loc);
    #[cfg(target_os = "linux")]
    let dns_nameserver_count = Some(
        crate::host_observability::read_linux_dns_config()
            .nameservers
            .len(),
    );
    #[cfg(not(target_os = "linux"))]
    let dns_nameserver_count = None;
    #[cfg(target_os = "linux")]
    let default_route_available =
        Some(crate::host_observability::read_linux_default_route().is_some());
    #[cfg(not(target_os = "linux"))]
    let default_route_available = None;

    build_network_path_diagnosis(NetworkPathDiagnosisInput {
        enabled_channel: Some(config.enabled_channel.as_str()),
        wifi_connected,
        pressure,
        tls_fragmentation_risk,
        runtime_capabilities,
        proxy_configured,
        connectivity_snapshot,
        dns_nameserver_count,
        default_route_available,
        probe_error,
    })
}

fn capture_connectivity_snapshot(
    platform: &dyn crate::Platform,
    config: &crate::config::AppConfig,
    loc: Locale,
) -> (Option<ChannelConnectivitySnapshot>, Option<String>) {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let wifi_settled = crate::state::wifi_sta_settled_for_outbound(3);
        let fragmentation_risk = crate::orchestrator::current_tls_fragmentation_risk();
        if !wifi_settled || fragmentation_risk.blocks_live_probe() {
            return (
                Some(crate::channels::build_unavailable_snapshot(config, loc)),
                None,
            );
        }
    }

    match crate::network::create_http_client_with_config(
        platform,
        config,
        crate::network::HttpClientClass::Background,
    ) {
        Ok(mut http) => (
            Some(crate::channels::build_snapshot(config, http.as_mut(), loc)),
            None,
        ),
        Err(error) => (None, Some(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::{ChannelConnectivityItem, ChannelConnectivitySnapshot};
    use crate::orchestrator::RuntimeCapabilityStatus;
    use crate::orchestrator::{RuntimeCapabilityReason, RuntimeCapabilityState};

    #[test]
    fn network_path_diagnosis_marks_wifi_disconnect_as_root_cause() {
        let diagnosis = build_network_path_diagnosis(NetworkPathDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            wifi_connected: false,
            pressure: PressureLevel::Normal,
            tls_fragmentation_risk: TlsFragmentationRisk::Healthy,
            runtime_capabilities: Vec::new(),
            proxy_configured: false,
            connectivity_snapshot: None,
            dns_nameserver_count: Some(2),
            default_route_available: Some(true),
            probe_error: None,
        });

        assert_eq!(diagnosis.kind, DiagnosisKind::NetworkPath);
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "wifi_disconnected"));
    }

    #[test]
    fn network_path_diagnosis_reports_channel_probe_failure_and_runtime_degradation() {
        let diagnosis = build_network_path_diagnosis(NetworkPathDiagnosisInput {
            enabled_channel: Some("qq_channel"),
            wifi_connected: true,
            pressure: PressureLevel::Normal,
            tls_fragmentation_risk: TlsFragmentationRisk::Healthy,
            runtime_capabilities: vec![RuntimeCapabilityState {
                id: crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
                status: RuntimeCapabilityStatus::Degraded,
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
            proxy_configured: true,
            connectivity_snapshot: Some(ChannelConnectivitySnapshot {
                channels: vec![ChannelConnectivityItem {
                    id: "qq_channel".to_string(),
                    configured: true,
                    ok: false,
                    message_key: Some("network.connectivity_check_failed"),
                    runtime_status: crate::channels::ChannelRuntimeStatus::Failed,
                    runtime_reason: Some("network.connectivity_check_failed"),
                }],
                checked_at_unix_secs: Some(1),
                stale: false,
            }),
            dns_nameserver_count: Some(1),
            default_route_available: Some(true),
            probe_error: None,
        });

        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "active_channel_connectivity_failed"));
        assert!(diagnosis
            .suspected_root_causes
            .iter()
            .any(|cause| cause.code == "network_outbound_capability_degraded"));
    }
}
