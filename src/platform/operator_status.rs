//! Unified operator-facing status contract for HTTP and CLI.

use crate::Platform;
use crate::capability_package::{
    CapabilityPackageOperatorSnapshot, CapabilityPackageRuntimeCapabilities,
    build_capability_package_operator_snapshot, render_capability_package_operator_text,
};
use crate::channel_capability::{
    ChannelCapabilityRegistry, ChannelCapabilitySnapshot,
    build_channel_capability_snapshots_for_registry,
};
use crate::memory::MemoryProfile;
use crate::orchestrator;
use crate::runtime;
use crate::task_execution::{
    TaskExecutionOperatorSnapshot, build_task_execution_operator_snapshot,
    render_task_execution_operator_text,
};
use crate::tools::{ToolCatalogEntry, ToolExecutionGovernanceState, ToolRegistry};
use serde::Serialize;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";

#[derive(Debug)]
pub struct OperatorStatusInput<'a> {
    pub platform: &'a dyn Platform,
    pub tool_registry: &'a ToolRegistry,
    pub channel_capability_registry: &'a ChannelCapabilityRegistry,
    pub capability_package_runtime_capabilities: &'a CapabilityPackageRuntimeCapabilities,
    pub current_channel: &'a str,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub version: &'a str,
    pub board_id: &'a str,
    pub llm_stream_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct OperatorPlatformContract {
    pub board_id: String,
    pub firmware_version: String,
    pub memory_profile: String,
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

#[derive(Debug, Serialize)]
pub struct OperatorContinuityTooling {
    pub export_supported: bool,
    pub import_supported: bool,
    pub inspect_governance_supported: bool,
    pub inspect_recall_supported: bool,
    pub inspect_hygiene_supported: bool,
    pub inspect_tool_governance_supported: bool,
    pub saved_snapshot_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_snapshots: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OperatorStatusSnapshot {
    pub platform_contract: OperatorPlatformContract,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub last_error: String,
    pub metrics: crate::metrics::MetricsSnapshot,
    pub resource: orchestrator::ResourceSnapshot,
    pub threads: runtime::ThreadRegistrySnapshot,
    pub runtime_mode: runtime::thread_registry::RuntimeModeSnapshot,
    pub continuity_tooling: OperatorContinuityTooling,
    pub task_execution: TaskExecutionOperatorSnapshot,
    pub capability_packages: CapabilityPackageOperatorSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelCapabilitySnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_governance: Option<ToolExecutionGovernanceState>,
}

pub fn build_operator_status(
    input: OperatorStatusInput<'_>,
) -> crate::error::Result<OperatorStatusSnapshot> {
    let continuity_tool_available = input.tool_registry.get("continuity_snapshot").is_some();
    let tool_governance = input.tool_registry.inspect_execution_governance()?;
    let storage_media = input.platform.storage_media();
    let (storage_media_count, storage_media_error) = match storage_media {
        Ok(items) => (items.len(), None),
        Err(error) => (0, Some(error.to_string())),
    };
    let saved_snapshots = input
        .platform
        .state_fs()
        .list_dir(REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            name.strip_suffix(".json")
                .map(str::to_string)
                .filter(|value| !value.trim().is_empty())
        })
        .collect::<Vec<_>>();
    let audio_caps = input.platform.audio_duplex_capabilities();
    let task_execution = build_task_execution_operator_snapshot(
        input.platform.task_run_store().as_ref(),
        input.platform.task_artifact_store().as_ref(),
        input.platform.task_learning_store().as_ref(),
    )?;
    let capability_packages = build_capability_package_operator_snapshot(
        input.platform.state_fs().as_ref(),
        input.capability_package_runtime_capabilities,
        input.current_channel,
    )?;
    let channels = build_channel_capability_snapshots_for_registry(
        input.channel_capability_registry,
        input.llm_stream_enabled,
    );
    Ok(OperatorStatusSnapshot {
        platform_contract: OperatorPlatformContract {
            board_id: input.board_id.to_string(),
            firmware_version: input.version.to_string(),
            memory_profile: memory_profile_label(input.platform.memory_profile()).to_string(),
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
        inbound_depth: input.inbound_depth,
        outbound_depth: input.outbound_depth,
        last_error: crate::state::get_current_error().unwrap_or_else(|| "none".to_string()),
        metrics: crate::metrics::snapshot(),
        resource: orchestrator::snapshot(),
        threads: runtime::thread_registry::snapshot(),
        runtime_mode: runtime::thread_registry::runtime_mode_snapshot(),
        continuity_tooling: OperatorContinuityTooling {
            export_supported: continuity_tool_available,
            import_supported: continuity_tool_available,
            inspect_governance_supported: continuity_tool_available,
            inspect_recall_supported: continuity_tool_available,
            inspect_hygiene_supported: continuity_tool_available,
            inspect_tool_governance_supported: continuity_tool_available
                && tool_governance.is_some(),
            saved_snapshot_count: saved_snapshots.len(),
            saved_snapshots,
        },
        task_execution,
        capability_packages,
        channels,
        tools: input.tool_registry.tool_catalog()?,
        tool_governance,
    })
}

pub fn render_operator_status_text(snapshot: &OperatorStatusSnapshot) -> String {
    let mut out = String::from("operator_status:\n");
    out.push_str(&format!(
        "  board_id: {}\n  firmware_version: {}\n  memory_profile: {}\n  wifi_connected: {}\n  config_plane_active: {}\n  display_available: {}\n  wifi_scan_available: {}\n  hardware_discovery_available: {}\n  ota_supported: {}\n  audio_duplex_profile: {}\n  storage_media_count: {}\n",
        snapshot.platform_contract.board_id,
        snapshot.platform_contract.firmware_version,
        snapshot.platform_contract.memory_profile,
        snapshot.platform_contract.wifi_connected,
        snapshot.platform_contract.config_plane_active,
        snapshot.platform_contract.display_available,
        snapshot.platform_contract.wifi_scan_available,
        snapshot.platform_contract.hardware_discovery_available,
        snapshot.platform_contract.ota_supported,
        snapshot.platform_contract.audio_duplex_profile,
        snapshot.platform_contract.storage_media_count,
    ));
    if let Some(error) = snapshot.platform_contract.storage_media_error.as_deref() {
        out.push_str(&format!("  storage_media_error: {}\n", error));
    }
    out.push_str(&format!(
        "  inbound_depth: {}\n  outbound_depth: {}\n  last_error: {}\n  pressure: {:?}\n  continuity_saved_snapshots: {}\n",
        snapshot.inbound_depth,
        snapshot.outbound_depth,
        snapshot.last_error,
        snapshot.resource.pressure,
        snapshot.continuity_tooling.saved_snapshot_count,
    ));
    if let Some(governance) = snapshot.tool_governance.as_ref() {
        out.push_str(&format!(
            "  tool_emergency_stop: {}\n  tool_breakers: {}\n  tool_records: {}\n",
            governance.emergency_stop.active,
            governance.breakers.len(),
            governance.recent_records.len(),
        ));
    }
    out.push_str(&render_task_execution_operator_text(
        &snapshot.task_execution,
    ));
    out.push_str(&render_capability_package_operator_text(
        &snapshot.capability_packages,
    ));
    out.push_str("  channels:\n");
    for channel in &snapshot.channels {
        out.push_str(&format!(
            "    - {} | configured={} enabled={} primary={} supplemental={} edit={} stream_edit={} explicit_target={} typing={} stream_edit_active={} degraded={}\n",
            channel.id,
            channel.configured,
            channel.enabled,
            channel.supports_primary_reply,
            channel.supports_supplemental_reply,
            channel.supports_edit,
            channel.supports_stream_edit,
            channel.supports_explicit_target,
            channel.supports_typing_or_chat_action,
            channel.stream_edit_active,
            if channel.degraded_reasons.is_empty() {
                "none".to_string()
            } else {
                channel.degraded_reasons.join(",")
            }
        ));
    }
    out.push_str("  tools:\n");
    for tool in snapshot.tools.iter().take(12) {
        out.push_str(&format!(
            "    - {} | exposure={} effect={} risk={} approval={} net={} user_llm={} breaker={}\n",
            tool.name,
            tool.exposure,
            tool.effect_class,
            tool.risk_level,
            tool.approval_mode,
            tool.requires_network,
            tool.llm_visible_user,
            tool.governance_breaker_tripped,
        ));
    }
    if snapshot.tools.len() > 12 {
        out.push_str(&format!(
            "    - ... {} more tools\n",
            snapshot.tools.len() - 12
        ));
    }
    out
}

fn memory_profile_label(profile: MemoryProfile) -> &'static str {
    match profile {
        MemoryProfile::Embedded => "embedded",
        MemoryProfile::Standard => "standard",
    }
}
