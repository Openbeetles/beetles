//! Unified device-presence contract for Beetle OS.
//! Beetle OS 统一设备存在态合同。

use crate::display::DisplaySystemState;
use crate::orchestrator::{self, PressureLevel, ResourceSnapshot};
use crate::platform::Platform;
use crate::runtime::{self, RuntimeMode, RuntimeModeSnapshot, SoulKernelStatus};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceState {
    Booting,
    Pairing,
    Recovery,
    Fault,
    NoWifi,
    Idle,
    Busy,
    Listening,
    Speaking,
}

impl PresenceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Booting => "booting",
            Self::Pairing => "pairing",
            Self::Recovery => "recovery",
            Self::Fault => "fault",
            Self::NoWifi => "no_wifi",
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Listening => "listening",
            Self::Speaking => "speaking",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresenceDisplayProjection {
    pub state: DisplaySystemState,
    pub subtitle_override: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PresenceSnapshot {
    pub state: PresenceState,
    pub display_state: DisplaySystemState,
    pub headline: String,
    pub subtitle: String,
    pub rationale: String,
    pub busy: bool,
    pub wifi_connected: bool,
    pub pairing_required: bool,
    pub audio_recording: bool,
    pub audio_playing: bool,
    pub critical_pressure: bool,
    pub display_sleep_candidate: bool,
    pub runtime_mode: RuntimeModeSnapshot,
    pub soul_kernel: SoulKernelStatus,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisor: Option<crate::runtime::linux_supervisor::LinuxSupervisorStatusSnapshot>,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<crate::runtime::LinuxReleaseStatus>,
}

impl PresenceSnapshot {
    pub fn display_projection(&self, network_hint: Option<&str>) -> PresenceDisplayProjection {
        let subtitle_override = match self.state {
            PresenceState::Booting | PresenceState::Recovery | PresenceState::Fault => {
                Some(self.subtitle.clone())
            }
            PresenceState::Pairing => Some(
                network_hint
                    .map(|hint| format!("pair at {}", hint))
                    .unwrap_or_else(|| self.subtitle.clone()),
            ),
            PresenceState::NoWifi => Some(
                network_hint
                    .map(|hint| format!("config at {}", hint))
                    .unwrap_or_else(|| self.subtitle.clone()),
            ),
            PresenceState::Idle
            | PresenceState::Busy
            | PresenceState::Listening
            | PresenceState::Speaking => None,
        };
        PresenceDisplayProjection {
            state: self.display_state,
            subtitle_override,
        }
    }
}

pub fn inspect_platform_presence(platform: &dyn Platform, now_secs: u64) -> PresenceSnapshot {
    let resource = orchestrator::snapshot();
    let soul_kernel = runtime::inspect_platform_soul_kernel(platform, now_secs);
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let supervisor = crate::runtime::linux_supervisor::read_status_snapshot()
        .ok()
        .flatten();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let release = Some(crate::runtime::inspect_platform_linux_release(
        platform, now_secs,
    ));

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let mut runtime_mode_source = crate::runtime::thread_registry::runtime_mode_source();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let runtime_mode_source = crate::runtime::thread_registry::runtime_mode_source();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if let Some(snapshot) = supervisor.as_ref() {
        runtime_mode_source.supervisor_present = true;
        runtime_mode_source.supervisor_alive = snapshot.supervisor_alive;
        runtime_mode_source.supervisor_agent_alive = snapshot.agent_alive;
    }
    let runtime_mode = crate::runtime::mode::snapshot_from_source(runtime_mode_source);
    let busy = resource.active_agent_tasks > 0
        || resource.active_http_count > 0
        || resource.inbound_depth > 0
        || resource.outbound_depth > 0
        || runtime_mode.current_mode == RuntimeMode::Maintenance;
    let state = derive_presence_state(runtime_mode, &soul_kernel, &resource, busy);
    let display_state = map_presence_to_display_state(state);
    let (headline, subtitle, rationale) =
        build_presence_copy(state, runtime_mode.current_mode, &soul_kernel, &resource);

    PresenceSnapshot {
        state,
        display_state,
        headline: headline.to_string(),
        subtitle: subtitle.to_string(),
        rationale: rationale.to_string(),
        busy,
        wifi_connected: runtime_mode.wifi_sta_connected,
        pairing_required: runtime_mode.pairing_required,
        audio_recording: resource.audio_recording,
        audio_playing: resource.audio_playing,
        critical_pressure: resource.pressure == PressureLevel::Critical,
        display_sleep_candidate: matches!(state, PresenceState::Idle | PresenceState::NoWifi),
        runtime_mode,
        soul_kernel,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        supervisor,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        release,
    }
}

fn derive_presence_state(
    runtime_mode: RuntimeModeSnapshot,
    _soul_kernel: &SoulKernelStatus,
    resource: &ResourceSnapshot,
    busy: bool,
) -> PresenceState {
    if runtime_mode.recovery_safe_mode_active {
        PresenceState::Recovery
    } else if resource.pressure == PressureLevel::Critical {
        PresenceState::Fault
    } else if runtime_mode.current_mode == RuntimeMode::Booting {
        PresenceState::Booting
    } else if runtime_mode.current_mode == RuntimeMode::Pairing {
        PresenceState::Pairing
    } else if resource.audio_recording {
        PresenceState::Listening
    } else if resource.audio_playing {
        PresenceState::Speaking
    } else if !runtime_mode.wifi_sta_connected {
        PresenceState::NoWifi
    } else if busy {
        PresenceState::Busy
    } else {
        PresenceState::Idle
    }
}

fn map_presence_to_display_state(state: PresenceState) -> DisplaySystemState {
    match state {
        PresenceState::Booting => DisplaySystemState::Booting,
        PresenceState::Pairing => DisplaySystemState::Pairing,
        PresenceState::Recovery => DisplaySystemState::Recovery,
        PresenceState::Fault => DisplaySystemState::Fault,
        PresenceState::NoWifi => DisplaySystemState::NoWifi,
        PresenceState::Idle => DisplaySystemState::Idle,
        PresenceState::Busy => DisplaySystemState::Busy,
        PresenceState::Listening => DisplaySystemState::Recording,
        PresenceState::Speaking => DisplaySystemState::Playing,
    }
}

fn build_presence_copy(
    state: PresenceState,
    runtime_mode: RuntimeMode,
    _soul_kernel: &SoulKernelStatus,
    resource: &ResourceSnapshot,
) -> (&'static str, &'static str, &'static str) {
    match state {
        PresenceState::Booting => ("BOOTING", "restoring runtime shell", "boot_phase_active"),
        PresenceState::Pairing => (
            "PAIRING",
            "set pairing code to trust this board",
            "pairing_required",
        ),
        PresenceState::Recovery => (
            "RECOVERY",
            "safe mode keeps control plane reachable",
            "recovery_safe_mode",
        ),
        PresenceState::Fault => (
            "PROTECT",
            "resource pressure is critical",
            "critical_pressure",
        ),
        PresenceState::NoWifi => ("NETWORK", "network link is not ready", "wifi_disconnected"),
        PresenceState::Idle => ("READY", "present and waiting", "idle"),
        PresenceState::Busy => {
            if runtime_mode == RuntimeMode::Maintenance {
                (
                    "BUSY",
                    "doing low-priority housekeeping",
                    "maintenance_mode",
                )
            } else if resource.active_agent_tasks > 0 {
                ("BUSY", "working on the current turn", "active_agent_task")
            } else {
                (
                    "BUSY",
                    "keeping queues and channels moving",
                    "runtime_activity",
                )
            }
        }
        PresenceState::Listening => (
            "LISTEN",
            "listening for your next instruction",
            "audio_recording",
        ),
        PresenceState::Speaking => (
            "SPEAK",
            "replying through the device speaker",
            "audio_playing",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_presence_copy, derive_presence_state, PresenceDisplayProjection, PresenceSnapshot,
        PresenceState,
    };
    use crate::display::DisplaySystemState;
    use crate::orchestrator::state::{ChannelHealthSnapshot, ChannelsHealthSnapshot};
    use crate::orchestrator::{PressureLevel, ResourceSnapshot};
    use crate::runtime::{RuntimeMode, RuntimeModeSnapshot};

    fn runtime_mode(mode: RuntimeMode) -> RuntimeModeSnapshot {
        RuntimeModeSnapshot {
            current_mode: mode,
            wifi_sta_connected: true,
            boot_phase_active: mode == RuntimeMode::Booting,
            pairing_required: mode == RuntimeMode::Pairing,
            pairing_state_known: true,
            voice_exclusive_active: mode == RuntimeMode::VoiceExclusive,
            background_maintenance_active: mode == RuntimeMode::Maintenance,
            config_plane_alive: false,
            channel_plane_alive: false,
            voice_plane_alive: false,
            agent_plane_alive: false,
            user_agent_lane_alive: false,
            system_agent_lane_alive: false,
            dual_agent_lanes_alive: false,
            external_wss_managed_present: false,
            external_wss_suspend_requested: false,
            external_wss_suspended: false,
            supervisor_present: false,
            supervisor_alive: false,
            supervisor_agent_alive: false,
            recovery_safe_mode_active: mode == RuntimeMode::RecoverySafeMode,
            action_budget: crate::runtime::mode::snapshot_from_source(
                crate::runtime::mode::RuntimeModeSource {
                    boot_phase_active: mode == RuntimeMode::Booting,
                    pairing_required: mode == RuntimeMode::Pairing,
                    pairing_state_known: true,
                    background_maintenance_active: mode == RuntimeMode::Maintenance,
                    voice_exclusive_active: mode == RuntimeMode::VoiceExclusive,
                    recovery_safe_mode_active: mode == RuntimeMode::RecoverySafeMode,
                    ..crate::runtime::mode::RuntimeModeSource::default()
                },
            )
            .action_budget,
        }
    }

    fn resource() -> ResourceSnapshot {
        ResourceSnapshot {
            pressure: PressureLevel::Normal,
            tls_fragmentation_risk: crate::orchestrator::TlsFragmentationRisk::NotApplicable,
            storage_contention_risk: crate::orchestrator::StorageContentionRisk::Healthy,
            heap_free_internal: 0,
            heap_free_spiram: 0,
            heap_largest_block_internal: 0,
            active_http_count: 0,
            active_wss_count: 0,
            active_agent_tasks: 0,
            inbound_depth: 0,
            outbound_depth: 0,
            budget: crate::orchestrator::current_budget(),
            channels: ChannelsHealthSnapshot {
                telegram: healthy_channel(),
                feishu: healthy_channel(),
                dingtalk: healthy_channel(),
                wecom: healthy_channel(),
                qq_channel: healthy_channel(),
            },
            session_count: 0,
            storage_used_kb: 0,
            storage_total_kb: 0,
            audio_recording: false,
            audio_playing: false,
            audio_interrupt_listening: false,
            audio_interrupt_requested: false,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            cpu_usage_percent: 0.0,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            load_average: (0.0, 0.0, 0.0),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            process_memory_kb: 0,
        }
    }

    fn healthy_channel() -> ChannelHealthSnapshot {
        ChannelHealthSnapshot {
            consecutive_failures: 0,
            total_failures: 0,
            total_successes: 0,
            healthy: true,
        }
    }

    fn snapshot_with_state(
        state: PresenceState,
        display_state: DisplaySystemState,
    ) -> PresenceSnapshot {
        PresenceSnapshot {
            state,
            display_state,
            headline: state.as_str().to_string(),
            subtitle: "subtitle".to_string(),
            rationale: "rationale".to_string(),
            busy: false,
            wifi_connected: true,
            pairing_required: false,
            audio_recording: false,
            audio_playing: false,
            critical_pressure: false,
            display_sleep_candidate: false,
            runtime_mode: runtime_mode(RuntimeMode::Normal),
            soul_kernel: crate::runtime::SoulKernelStatus::default(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            supervisor: None,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            release: None,
        }
    }

    #[test]
    fn recovery_outranks_boot_and_pairing() {
        let state = derive_presence_state(
            runtime_mode(RuntimeMode::RecoverySafeMode),
            &crate::runtime::SoulKernelStatus {
                minimum_viable: true,
                safe_mode_minimum_readable: true,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource(),
            false,
        );
        assert_eq!(state, PresenceState::Recovery);
    }

    #[test]
    fn pairing_outranks_no_wifi_when_not_booting() {
        let mut snapshot = runtime_mode(RuntimeMode::Pairing);
        snapshot.wifi_sta_connected = false;
        let state = derive_presence_state(
            snapshot,
            &crate::runtime::SoulKernelStatus {
                minimum_viable: true,
                safe_mode_minimum_readable: true,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource(),
            false,
        );
        assert_eq!(state, PresenceState::Pairing);
    }

    #[test]
    fn listening_outranks_busy() {
        let mut resource = resource();
        resource.audio_recording = true;
        resource.active_agent_tasks = 1;
        let state = derive_presence_state(
            runtime_mode(RuntimeMode::Normal),
            &crate::runtime::SoulKernelStatus {
                minimum_viable: true,
                safe_mode_minimum_readable: true,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource,
            true,
        );
        assert_eq!(state, PresenceState::Listening);
    }

    #[test]
    fn maintenance_busy_copy_mentions_housekeeping() {
        let (_, subtitle, rationale) = build_presence_copy(
            PresenceState::Busy,
            RuntimeMode::Maintenance,
            &crate::runtime::SoulKernelStatus::default(),
            &resource(),
        );
        assert_eq!(subtitle, "doing low-priority housekeeping");
        assert_eq!(rationale, "maintenance_mode");
    }

    #[test]
    fn expected_bootstrap_empty_does_not_force_recovery_or_fault() {
        let mut snapshot = runtime_mode(RuntimeMode::Normal);
        snapshot.wifi_sta_connected = false;
        let state = derive_presence_state(
            snapshot,
            &crate::runtime::SoulKernelStatus {
                expected_bootstrap_empty: true,
                minimum_viable: false,
                safe_mode_minimum_readable: false,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource(),
            false,
        );
        assert_eq!(state, PresenceState::NoWifi);
    }

    #[test]
    fn non_bootstrap_kernel_gap_does_not_mask_live_idle_presence() {
        let state = derive_presence_state(
            runtime_mode(RuntimeMode::Normal),
            &crate::runtime::SoulKernelStatus {
                expected_bootstrap_empty: false,
                minimum_viable: false,
                safe_mode_minimum_readable: false,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource(),
            false,
        );
        assert_eq!(state, PresenceState::Idle);
    }

    #[test]
    fn non_bootstrap_kernel_gap_does_not_mask_live_busy_presence() {
        let mut resource = resource();
        resource.active_agent_tasks = 1;
        let state = derive_presence_state(
            runtime_mode(RuntimeMode::Normal),
            &crate::runtime::SoulKernelStatus {
                expected_bootstrap_empty: false,
                minimum_viable: false,
                safe_mode_minimum_readable: false,
                ..crate::runtime::SoulKernelStatus::default()
            },
            &resource,
            true,
        );
        assert_eq!(state, PresenceState::Busy);
    }

    #[test]
    fn critical_pressure_fault_keeps_fault_display_projection() {
        let mut snapshot = snapshot_with_state(PresenceState::Fault, DisplaySystemState::Fault);
        snapshot.critical_pressure = true;

        assert_eq!(
            snapshot.display_projection(Some("192.168.4.1")),
            PresenceDisplayProjection {
                state: DisplaySystemState::Fault,
                subtitle_override: Some("subtitle".to_string()),
            }
        );
    }
}
