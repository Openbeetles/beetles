//! Unified startup readiness for runtime worker admission.
//! 运行态启动 readiness：集中判断启动阶段哪些 worker 可以进入执行面。

use crate::orchestrator::{PressureLevel, ResourceLightSnapshot, TlsFragmentationRisk};
use crate::runtime::RuntimeModeSnapshot;
use crate::state::NetworkRuntimeSnapshot;
use std::sync::atomic::{AtomicBool, Ordering};

/// Config worker normal largest-block floor. This mirrors the route worker
/// stack contract without depending on the HTTP platform module from runtime.
pub const CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES: u32 =
    crate::util::STACK_HTTP_CONFIG_WORKER as u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStartupPhase {
    BootKernel,
    ConfigRecoveryReady,
    LocalRuntimeAssembled,
    OutboundNetworkReady,
    SteadyRuntime,
}

impl RuntimeStartupPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BootKernel => "boot_kernel",
            Self::ConfigRecoveryReady => "config_recovery_ready",
            Self::LocalRuntimeAssembled => "local_runtime_assembled",
            Self::OutboundNetworkReady => "outbound_network_ready",
            Self::SteadyRuntime => "steady_runtime",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStartupNetworkReason {
    None,
    WifiNotConfigured,
    WifiNotReady,
    WallClockUntrusted,
}

impl RuntimeStartupNetworkReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::WifiNotConfigured => "wifi_not_configured",
            Self::WifiNotReady => "wifi_not_ready",
            Self::WallClockUntrusted => "wall_clock_untrusted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuntimeStartupReadiness {
    pub phase: RuntimeStartupPhase,
    pub reason: &'static str,
    pub network_reason: RuntimeStartupNetworkReason,
    pub allow_config_recovery_routes: bool,
    pub allow_default_status_routes: bool,
    pub allow_external_wss_worker: bool,
    pub allow_agent_heavy_execution: bool,
    pub allow_channel_outbound_worker: bool,
    pub allow_voice_realtime_connect: bool,
    pub allow_write_back_worker: bool,
    pub allow_display_status_surface: bool,
    pub allow_display_heavy_refresh: bool,
    pub config_worker_floor_available: bool,
}

impl RuntimeStartupReadiness {
    pub fn startup_block_reason(self) -> Option<&'static str> {
        if self.reason == "ready" {
            None
        } else {
            Some(self.reason)
        }
    }

    pub fn worker_block_reason(self) -> &'static str {
        if self.network_reason != RuntimeStartupNetworkReason::None {
            self.network_reason.as_str()
        } else if self.reason != "ready" {
            self.reason
        } else {
            "runtime_mode_budget"
        }
    }
}

pub fn runtime_startup_readiness_snapshot() -> RuntimeStartupReadiness {
    let network = crate::state::network_runtime_snapshot(
        crate::platform::time::wall_clock_is_trustworthy(),
        crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
    );
    runtime_startup_readiness_from_parts(
        &network,
        &crate::runtime::thread_registry::runtime_mode_snapshot(),
        &crate::orchestrator::resource_light_snapshot(),
    )
}

/// Advance boot-phase state once the unified startup readiness reaches steady.
pub fn service_runtime_startup_readiness(tag: &str) {
    static STEADY_RECORDED: AtomicBool = AtomicBool::new(false);

    let readiness = runtime_startup_readiness_snapshot();
    if readiness.phase != RuntimeStartupPhase::SteadyRuntime {
        return;
    }
    if !crate::state::boot_phase_active() {
        return;
    }
    crate::state::set_boot_phase_active(false);
    if !STEADY_RECORDED.swap(true, Ordering::AcqRel) {
        log::info!(
            "[{}] startup readiness reached steady_runtime; boot phase cleared",
            tag
        );
    }
    crate::bg_timer::notify_deadline_changed();
}

pub(crate) fn runtime_startup_readiness_from_parts(
    network: &NetworkRuntimeSnapshot,
    mode: &RuntimeModeSnapshot,
    resource: &ResourceLightSnapshot,
) -> RuntimeStartupReadiness {
    let network_reason = startup_network_reason(network);
    let config_worker_floor_available = config_worker_floor_available(resource);
    let local_runtime_assembled = mode.config_plane_alive;
    let tls_floor_available = !matches!(
        resource.tls_fragmentation_risk,
        TlsFragmentationRisk::Critical
    );
    let tls_steady = matches!(
        resource.tls_fragmentation_risk,
        TlsFragmentationRisk::Healthy | TlsFragmentationRisk::NotApplicable
    );
    let outbound_ready = network_reason == RuntimeStartupNetworkReason::None
        && config_worker_floor_available
        && resource.pressure != PressureLevel::Critical
        && tls_floor_available;
    let steady_ready = local_runtime_assembled
        && outbound_ready
        && resource.pressure == PressureLevel::Normal
        && tls_steady;

    let phase = if steady_ready {
        RuntimeStartupPhase::SteadyRuntime
    } else if outbound_ready {
        RuntimeStartupPhase::OutboundNetworkReady
    } else if local_runtime_assembled {
        RuntimeStartupPhase::LocalRuntimeAssembled
    } else if mode.config_plane_alive || mode.config_active {
        RuntimeStartupPhase::ConfigRecoveryReady
    } else {
        RuntimeStartupPhase::BootKernel
    };

    let allow_config_recovery_routes = matches!(
        phase,
        RuntimeStartupPhase::ConfigRecoveryReady
            | RuntimeStartupPhase::LocalRuntimeAssembled
            | RuntimeStartupPhase::OutboundNetworkReady
            | RuntimeStartupPhase::SteadyRuntime
    );
    let allow_default_status_routes = !matches!(phase, RuntimeStartupPhase::BootKernel);
    let mode_allows_non_voice = mode.action_budget.allow_non_voice_outbound;
    let mode_allows_external_wss = mode.action_budget.allow_external_wss_connect;
    let mode_allows_voice = mode.action_budget.allow_realtime_voice_connect;

    let local_write_back_ready = local_runtime_assembled
        && config_worker_floor_available
        && resource.pressure == PressureLevel::Normal
        && tls_floor_available;

    RuntimeStartupReadiness {
        phase,
        reason: startup_reason(
            phase,
            network_reason,
            config_worker_floor_available,
            resource,
        ),
        network_reason,
        allow_config_recovery_routes,
        allow_default_status_routes,
        allow_external_wss_worker: outbound_ready && mode_allows_external_wss,
        allow_agent_heavy_execution: outbound_ready && mode_allows_non_voice,
        allow_channel_outbound_worker: outbound_ready && mode_allows_non_voice,
        allow_voice_realtime_connect: outbound_ready && mode_allows_voice,
        allow_write_back_worker: local_write_back_ready,
        allow_display_status_surface: !matches!(phase, RuntimeStartupPhase::BootKernel),
        allow_display_heavy_refresh: !matches!(phase, RuntimeStartupPhase::BootKernel)
            && resource.pressure != PressureLevel::Critical,
        config_worker_floor_available,
    }
}

fn startup_network_reason(network: &NetworkRuntimeSnapshot) -> RuntimeStartupNetworkReason {
    if !network.sta_expected || !network.sta_configured {
        return RuntimeStartupNetworkReason::WifiNotConfigured;
    }
    if !network.sta_ip_present || !network.outbound_settled {
        return RuntimeStartupNetworkReason::WifiNotReady;
    }
    if !network.wall_clock_trustworthy {
        return RuntimeStartupNetworkReason::WallClockUntrusted;
    }
    RuntimeStartupNetworkReason::None
}

fn config_worker_floor_available(resource: &ResourceLightSnapshot) -> bool {
    resource.heap_largest_block_internal == 0
        || resource.heap_largest_block_internal >= CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES
}

fn startup_reason(
    phase: RuntimeStartupPhase,
    network_reason: RuntimeStartupNetworkReason,
    config_worker_floor_available: bool,
    resource: &ResourceLightSnapshot,
) -> &'static str {
    if network_reason != RuntimeStartupNetworkReason::None {
        return network_reason.as_str();
    }
    if !config_worker_floor_available {
        return "config_worker_floor_low";
    }
    if resource.pressure != PressureLevel::Normal {
        return "resource_pressure";
    }
    if matches!(
        resource.tls_fragmentation_risk,
        TlsFragmentationRisk::Critical
    ) {
        return "tls_fragmentation_critical";
    }
    if phase == RuntimeStartupPhase::SteadyRuntime {
        "ready"
    } else {
        phase.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{ResourceBudget, StorageContentionRisk};
    use crate::runtime::{
        ConfigActivityPhase, RuntimeForegroundOverlay, RuntimeMode, RuntimeModeActionBudget,
    };
    use crate::state::NetworkWifiStage;

    fn network(
        sta_expected: bool,
        sta_configured: bool,
        sta_ip_present: bool,
        outbound_settled: bool,
        wall_clock_trustworthy: bool,
    ) -> NetworkRuntimeSnapshot {
        NetworkRuntimeSnapshot {
            sta_expected,
            sta_configured,
            sta_connecting: sta_expected && !sta_ip_present,
            sta_l2_connected: sta_ip_present,
            sta_ip_present,
            outbound_settled,
            wall_clock_trustworthy,
            last_wifi_stage: if sta_ip_present {
                NetworkWifiStage::StaIpReady
            } else {
                NetworkWifiStage::StaConnecting
            },
            last_wifi_reason_code: None,
        }
    }

    fn mode(config_plane_alive: bool, current_mode: RuntimeMode) -> RuntimeModeSnapshot {
        let action_budget = match current_mode {
            RuntimeMode::VoiceExclusive => RuntimeModeActionBudget {
                allow_periodic_maintenance: false,
                allow_due_user_timers: false,
                allow_heartbeat_injection: false,
                allow_best_effort_delayed_tasks: false,
                allow_idle_self_runtime: false,
                allow_non_voice_outbound: false,
                allow_realtime_voice_connect: true,
                allow_external_wss_connect: false,
                require_external_wss_suspended: true,
            },
            _ => RuntimeModeActionBudget {
                allow_periodic_maintenance: true,
                allow_due_user_timers: true,
                allow_heartbeat_injection: true,
                allow_best_effort_delayed_tasks: true,
                allow_idle_self_runtime: true,
                allow_non_voice_outbound: true,
                allow_realtime_voice_connect: true,
                allow_external_wss_connect: true,
                require_external_wss_suspended: false,
            },
        };
        RuntimeModeSnapshot {
            current_mode,
            wifi_sta_connected: false,
            boot_phase_active: current_mode == RuntimeMode::Booting,
            pairing_required: false,
            pairing_state_known: true,
            voice_exclusive_active: current_mode == RuntimeMode::VoiceExclusive,
            background_maintenance_active: current_mode == RuntimeMode::Maintenance,
            config_plane_alive,
            config_active: current_mode == RuntimeMode::ConfigActive,
            config_activity_phase: ConfigActivityPhase::Idle,
            channel_plane_alive: false,
            voice_plane_alive: false,
            agent_plane_alive: false,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            recovery_safe_mode_active: current_mode == RuntimeMode::RecoverySafeMode,
            runtime_foreground: RuntimeForegroundOverlay::default(),
            action_budget,
        }
    }

    fn resource(
        pressure: PressureLevel,
        tls_fragmentation_risk: TlsFragmentationRisk,
        heap_largest_block_internal: u32,
    ) -> ResourceLightSnapshot {
        ResourceLightSnapshot {
            pressure,
            tls_fragmentation_risk,
            storage_contention_risk: StorageContentionRisk::Healthy,
            heap_free_internal: 128 * 1024,
            heap_min_free_internal: 96 * 1024,
            heap_free_spiram: 6 * 1024 * 1024,
            heap_total_spiram: 8 * 1024 * 1024,
            heap_min_free_spiram: 5 * 1024 * 1024,
            heap_largest_block_spiram: 6 * 1024 * 1024,
            heap_used_spiram_est: 2 * 1024 * 1024,
            heap_largest_block_internal,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            budget: ResourceBudget {
                level: pressure,
                system_prompt_max: 64 * 1024,
                messages_max: 64 * 1024,
                response_body_max: 512 * 1024,
                reconnect_backoff_secs: 5,
                llm_hint: "[test]",
            },
            session_count: 0,
            storage_used_kb: 0,
            storage_total_kb: 1024,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 0.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.0, 0.0, 0.0),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 0,
        }
    }

    #[test]
    fn startup_readiness_keeps_config_surface_available_without_sta() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, false, false, true),
            &mode(true, RuntimeMode::Booting),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES,
            ),
        );

        assert_eq!(readiness.phase, RuntimeStartupPhase::LocalRuntimeAssembled);
        assert_eq!(
            readiness.network_reason,
            RuntimeStartupNetworkReason::WifiNotReady
        );
        assert!(readiness.allow_config_recovery_routes);
        assert!(readiness.allow_default_status_routes);
        assert!(!readiness.allow_external_wss_worker);
        assert!(!readiness.allow_agent_heavy_execution);
        assert!(!readiness.allow_channel_outbound_worker);
        assert!(!readiness.allow_voice_realtime_connect);
        assert!(
            readiness.allow_write_back_worker,
            "local durable write-back must not wait for outbound WiFi readiness"
        );
    }

    #[test]
    fn startup_readiness_blocks_network_workers_until_wall_clock_is_trusted() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, true, true, false),
            &mode(true, RuntimeMode::Normal),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES,
            ),
        );

        assert_eq!(
            readiness.network_reason,
            RuntimeStartupNetworkReason::WallClockUntrusted
        );
        assert!(!readiness.allow_external_wss_worker);
        assert!(!readiness.allow_agent_heavy_execution);
        assert!(!readiness.allow_channel_outbound_worker);
        assert!(!readiness.allow_voice_realtime_connect);
    }

    #[test]
    fn startup_readiness_blocks_steady_when_config_worker_floor_is_missing() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, true, true, true),
            &mode(true, RuntimeMode::Normal),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES - 1,
            ),
        );

        assert_eq!(readiness.phase, RuntimeStartupPhase::LocalRuntimeAssembled);
        assert_eq!(readiness.reason, "config_worker_floor_low");
        assert!(!readiness.config_worker_floor_available);
        assert!(!readiness.allow_external_wss_worker);
        assert!(!readiness.allow_agent_heavy_execution);
        assert!(!readiness.allow_channel_outbound_worker);
        assert!(!readiness.allow_voice_realtime_connect);
        assert!(!readiness.allow_write_back_worker);
    }

    #[test]
    fn startup_readiness_blocks_network_workers_under_critical_resource() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, true, true, true),
            &mode(true, RuntimeMode::Normal),
            &resource(
                PressureLevel::Critical,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES,
            ),
        );

        assert_eq!(readiness.phase, RuntimeStartupPhase::LocalRuntimeAssembled);
        assert_eq!(readiness.reason, "resource_pressure");
        assert!(!readiness.allow_external_wss_worker);
        assert!(!readiness.allow_agent_heavy_execution);
        assert!(!readiness.allow_channel_outbound_worker);
        assert!(!readiness.allow_voice_realtime_connect);
    }

    #[test]
    fn startup_readiness_keeps_write_back_local_but_network_planes_blocked() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, false, false, true),
            &mode(true, RuntimeMode::Booting),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES,
            ),
        );

        assert_eq!(readiness.phase, RuntimeStartupPhase::LocalRuntimeAssembled);
        assert_eq!(
            readiness.network_reason,
            RuntimeStartupNetworkReason::WifiNotReady
        );
        assert!(readiness.allow_write_back_worker);
        assert!(!readiness.allow_external_wss_worker);
        assert!(!readiness.allow_agent_heavy_execution);
        assert!(!readiness.allow_channel_outbound_worker);
        assert!(!readiness.allow_voice_realtime_connect);
    }

    #[test]
    fn startup_readiness_blocks_write_back_when_local_resource_floor_is_missing() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, false, false, true),
            &mode(true, RuntimeMode::Booting),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES - 1,
            ),
        );

        assert!(!readiness.config_worker_floor_available);
        assert!(!readiness.allow_write_back_worker);
        assert!(readiness.allow_default_status_routes);
    }

    #[test]
    fn startup_readiness_reaches_steady_after_network_and_resource_ready() {
        let readiness = runtime_startup_readiness_from_parts(
            &network(true, true, true, true, true),
            &mode(true, RuntimeMode::Normal),
            &resource(
                PressureLevel::Normal,
                TlsFragmentationRisk::Healthy,
                CONFIG_WORKER_LARGEST_BLOCK_FLOOR_BYTES,
            ),
        );

        assert_eq!(readiness.phase, RuntimeStartupPhase::SteadyRuntime);
        assert_eq!(readiness.reason, "ready");
        assert!(readiness.allow_external_wss_worker);
        assert!(readiness.allow_agent_heavy_execution);
        assert!(readiness.allow_channel_outbound_worker);
        assert!(readiness.allow_voice_realtime_connect);
        assert!(readiness.allow_write_back_worker);
    }
}
