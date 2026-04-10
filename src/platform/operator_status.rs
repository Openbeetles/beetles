//! Unified operator-facing status contract for HTTP and CLI.

use crate::device_capability::{build_device_capability_snapshots, DeviceCapabilityPlaneSnapshot};
use crate::orchestrator;
use crate::runtime;
use crate::tools::{ToolExecutionGovernanceState, ToolRegistry};
use crate::util::current_unix_secs;
use crate::Platform;
use serde::Serialize;

pub struct OperatorStatusInput<'a> {
    pub config: &'a crate::config::AppConfig,
    pub platform: &'a dyn Platform,
    pub tool_registry: &'a ToolRegistry,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub version: &'a str,
    pub board_id: &'a str,
}

#[derive(Debug, Serialize)]
pub struct OperatorPlatformContract {
    pub board_id: String,
    pub firmware_version: String,
    pub memory_system_kind: String,
    pub wifi_connected: bool,
    pub config_plane_active: bool,
    pub display_available: bool,
    pub wifi_scan_available: bool,
    pub hardware_discovery_available: bool,
    pub ota_supported: bool,
    pub audio_duplex_profile: String,
    pub storage_media_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_media_error: Option<String>,
}

#[derive(Serialize)]
pub struct OperatorStatusSnapshot {
    pub platform_contract: OperatorPlatformContract,
    pub build_package: crate::BuildPackageSnapshot,
    pub operator_surface: crate::platform::operator_surface::OperatorSurfaceBudget,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub last_error: String,
    pub threads: runtime::ThreadRegistrySnapshot,
    pub os_closure: runtime::BeetleOsClosureReport,
    pub initiative: runtime::InitiativeSnapshot,
    pub presence: runtime::PresenceSnapshot,
    pub runtime_mode: runtime::RuntimeModeSnapshot,
    pub soul_kernel: runtime::SoulKernelStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_planes: Vec<DeviceCapabilityPlaneSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_capabilities: Vec<crate::orchestrator::RuntimeCapabilityState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_governance: Option<ToolExecutionGovernanceState>,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisor: Option<crate::runtime::linux_supervisor::LinuxSupervisorStatusSnapshot>,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<crate::runtime::LinuxReleaseStatus>,
}

pub fn build_operator_status(
    input: OperatorStatusInput<'_>,
) -> crate::error::Result<OperatorStatusSnapshot> {
    let memory_system_kind = input.platform.memory_system_kind();
    let operator_surface =
        crate::platform::operator_surface::current_operator_surface_budget(memory_system_kind);
    let compact_view = operator_surface.compact_view;
    let tool_governance = if compact_view {
        None
    } else {
        input.tool_registry.inspect_execution_governance()?
    };
    let storage_media = input.platform.storage_media();
    let (storage_media_count, storage_media_error) = match storage_media {
        Ok(items) => (items.len(), None),
        Err(error) => (0, Some(error.to_string())),
    };
    let audio_caps = if crate::compiled_voice_capability() {
        input.platform.audio_duplex_capabilities()
    } else {
        crate::platform::AudioDuplexCapabilities::unavailable()
    };
    let capability_planes = build_device_capability_snapshots(input.config, input.platform);
    let presence = runtime::inspect_platform_presence(input.platform, current_unix_secs());
    let initiative = runtime::inspect_platform_initiative(input.platform, current_unix_secs());
    let os_closure = runtime::inspect_beetle_os_closure(&presence, &initiative);
    let runtime_mode = presence.runtime_mode;
    let soul_kernel = presence.soul_kernel.clone();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let supervisor = presence.supervisor.clone();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let release = presence.release.clone();
    Ok(OperatorStatusSnapshot {
        platform_contract: OperatorPlatformContract {
            board_id: input.board_id.to_string(),
            firmware_version: input.version.to_string(),
            memory_system_kind: memory_system_kind.as_str().to_string(),
            wifi_connected: crate::state::wifi_sta_connected(),
            config_plane_active: crate::state::config_plane_active(),
            display_available: input.platform.display_available(),
            wifi_scan_available: input.platform.wifi_scan().is_some(),
            hardware_discovery_available: input.platform.hardware_discovery().is_some(),
            ota_supported: cfg!(feature = "ota"),
            audio_duplex_profile: audio_caps.profile().as_str().to_string(),
            storage_media_count,
            storage_media_error,
        },
        build_package: crate::current_build_package(),
        operator_surface,
        inbound_depth: input.inbound_depth,
        outbound_depth: input.outbound_depth,
        last_error: crate::state::get_current_error().unwrap_or_else(|| "none".to_string()),
        threads: runtime::thread_registry::snapshot(),
        os_closure,
        initiative,
        runtime_mode,
        soul_kernel,
        presence,
        capability_planes,
        runtime_capabilities: orchestrator::runtime_capability_snapshot(),
        tool_governance,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        supervisor,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        release,
    })
}

pub fn render_operator_status_text(snapshot: &OperatorStatusSnapshot) -> String {
    let mut out = String::from("operator_status:\n");
    out.push_str(&format!(
        "  build_package_profile: {}\n  build_package_target: {}\n  build_package_default_full: {}\n  build_package_caps: voice={} vision={} sensor={}\n",
        snapshot.build_package.profile,
        snapshot.build_package.target_family.as_str(),
        snapshot.build_package.default_full_package,
        snapshot.build_package.capabilities.voice,
        snapshot.build_package.capabilities.vision,
        snapshot.build_package.capabilities.sensor,
    ));
    out.push_str(&format!(
        "  board_id: {}\n  firmware_version: {}\n  memory_system_kind: {}\n  wifi_connected: {}\n  config_plane_active: {}\n  display_available: {}\n  wifi_scan_available: {}\n  hardware_discovery_available: {}\n  ota_supported: {}\n  audio_duplex_profile: {}\n  storage_media_count: {}\n  operator_surface_compact: {}\n  operator_window_required: {}\n",
        snapshot.platform_contract.board_id,
        snapshot.platform_contract.firmware_version,
        snapshot.platform_contract.memory_system_kind,
        snapshot.platform_contract.wifi_connected,
        snapshot.platform_contract.config_plane_active,
        snapshot.platform_contract.display_available,
        snapshot.platform_contract.wifi_scan_available,
        snapshot.platform_contract.hardware_discovery_available,
        snapshot.platform_contract.ota_supported,
        snapshot.platform_contract.audio_duplex_profile,
        snapshot.platform_contract.storage_media_count,
        snapshot.operator_surface.compact_view,
        snapshot.operator_surface.window_required_for_deep_routes,
    ));
    if let Some(error) = snapshot.platform_contract.storage_media_error.as_deref() {
        out.push_str(&format!("  storage_media_error: {}\n", error));
    }
    if let Some(window) = snapshot.operator_surface.operator_window.as_ref() {
        out.push_str(&format!(
            "  operator_window_active: {}\n  operator_window_remaining_secs: {}\n",
            window.active, window.remaining_secs,
        ));
    }
    out.push_str(&format!(
        "  inbound_depth: {}\n  outbound_depth: {}\n  last_error: {}\n  presence_state: {}\n  presence_headline: {}\n  presence_rationale: {}\n  initiative_action: {}\n  initiative_ready: {}\n  initiative_rationale: {}\n  runtime_mode: {}\n  soul_kernel_ready: {}\n  soul_kernel_safe_mode_readable: {}\n  soul_kernel_degraded: {}\n  soul_kernel_key_memory: {}\n",
        snapshot.inbound_depth,
        snapshot.outbound_depth,
        snapshot.last_error,
        snapshot.presence.state.as_str(),
        snapshot.presence.headline,
        snapshot.presence.rationale,
        snapshot.initiative.action.as_str(),
        snapshot.initiative.ready,
        snapshot.initiative.rationale,
        snapshot.runtime_mode.current_mode.as_str(),
        snapshot.soul_kernel.minimum_viable,
        snapshot.soul_kernel.safe_mode_minimum_readable,
        snapshot.soul_kernel.degraded,
        snapshot.soul_kernel.key_memory_count,
    ));
    let runtime_summary = orchestrator::runtime_capability_summary();
    out.push_str(&format!(
        "  runtime_capabilities_offline: {}\n  runtime_capabilities_degraded: {}\n",
        runtime_summary.offline_count, runtime_summary.degraded_count,
    ));
    if !runtime_summary.offline_ids.is_empty() {
        out.push_str(&format!(
            "  runtime_capabilities_offline_ids: {}\n",
            runtime_summary.offline_ids.join(", ")
        ));
    }
    out.push_str(&format!(
        "  os_closure_ready: {}\n  os_closure_summary: {}\n  os_closure_planes: {}/{}\n",
        snapshot.os_closure.ready,
        snapshot.os_closure.summary,
        snapshot.os_closure.ready_planes,
        snapshot.os_closure.plane_count,
    ));
    if !snapshot.os_closure.outstanding.is_empty() {
        out.push_str(&format!(
            "  os_closure_outstanding: {}\n",
            snapshot.os_closure.outstanding.join(", ")
        ));
    }
    if let Some(reason) = snapshot.initiative.suppression_reason {
        out.push_str(&format!(
            "  initiative_suppressed_by: {}\n",
            reason.as_str()
        ));
    }
    if let Some(target) = snapshot.initiative.target.as_ref() {
        out.push_str(&format!(
            "  initiative_target: {}:{} ({})\n",
            target.channel, target.chat_id, target.selection_reason
        ));
    }
    if !snapshot.soul_kernel.degradation_reasons.is_empty() {
        out.push_str(&format!(
            "  soul_kernel_degradation: {}\n",
            snapshot.soul_kernel.degradation_reasons.join(", ")
        ));
    }
    out.push_str("  capability_planes:\n");
    for plane in &snapshot.capability_planes {
        out.push_str(&format!(
            "    - {} configured={} discovered={} mounted={} runtime_active={} candidates={} mode={:?}\n",
            plane.id,
            plane.configured,
            plane.discovered,
            plane.mounted,
            plane.runtime_active,
            plane.candidate_count,
            plane.mount_model,
        ));
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if let Some(supervisor) = snapshot.supervisor.as_ref() {
        out.push_str(&format!(
            "  supervisor_alive: {}\n  supervisor_state: {}\n  agent_alive: {}\n  agent_state: {}\n",
            supervisor.supervisor_alive,
            supervisor.state.current_state,
            supervisor.agent_alive,
            supervisor.state.agent.state,
        ));
        if let Some(reason) = supervisor.state.safe_mode_reason.as_deref() {
            out.push_str(&format!("  safe_mode_reason: {}\n", reason));
        }
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if let Some(release) = snapshot.release.as_ref() {
        out.push_str(&format!(
            "  release_managed: {}\n  release_rollout_state: {}\n  release_rollback_available: {}\n",
            release.managed,
            release.rollout_state_label(),
            release.rollback_available,
        ));
    }
    if let Some(governance) = snapshot.tool_governance.as_ref() {
        out.push_str(&format!(
            "  tool_emergency_stop: {}\n  tool_breakers: {}\n  tool_records: {}\n",
            governance.emergency_stop.active,
            governance.breakers.len(),
            governance.recent_records.len(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use std::sync::Arc;

    #[test]
    fn build_operator_status_includes_device_capability_planes() {
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let (tool_registry, _) = crate::tools::build_default_registry(
            &config,
            crate::tools::DefaultRegistryDeps {
                platform: Arc::clone(&platform),
                remind_at_store: platform.remind_at_store(),
                session_store: platform.session_store(),
                memory_store: platform.memory_store(),
                long_term_memory_store: platform.long_term_memory_store(),
                turn_ledger_store: platform.turn_ledger_store(),
                private_garden_store: platform.private_garden_store(),
                config_store: platform.config_store(),
            },
        );
        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
            inbound_depth: 0,
            outbound_depth: 0,
            version: "0.0.0",
            board_id: "test-board",
        })
        .expect("operator status");

        assert!(snapshot
            .capability_planes
            .iter()
            .any(|plane| plane.id == crate::DEVICE_CAPABILITY_VOICE));
        assert!(snapshot
            .capability_planes
            .iter()
            .any(|plane| plane.id == crate::DEVICE_CAPABILITY_SENSOR));

        let payload = serde_json::to_value(&snapshot).expect("serialize operator status");
        assert!(payload.get("build_package").is_some());
        assert!(payload["build_package"].get("profile").is_some());
        assert!(payload["build_package"]
            .get("default_full_package")
            .is_some());
        assert!(payload["build_package"]["capabilities"]
            .get("voice")
            .is_some());
    }

    #[test]
    fn embedded_operator_surface_defaults_to_compact_budget() {
        let budget = crate::platform::operator_surface::operator_surface_budget(
            crate::memory::MemorySystemKind::EspCompact,
            false,
        );

        assert!(budget.compact_view);
        assert!(budget.window_required_for_deep_routes);
    }
}
