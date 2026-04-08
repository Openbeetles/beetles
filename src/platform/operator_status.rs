//! Unified operator-facing status contract for HTTP and CLI.

use crate::capability_package::{
    build_capability_package_operator_snapshot, render_capability_package_operator_text,
    CapabilityPackageOperatorSnapshot, CapabilityPackageRuntimeCapabilities,
};
use crate::channel_capability::{
    build_channel_capability_snapshots_for_registry, ChannelCapabilityRegistry,
    ChannelCapabilitySnapshot,
};
use crate::device_capability::{build_device_capability_snapshots, DeviceCapabilityPlaneSnapshot};
use crate::memory::{
    board_subject_scope_id, compute_core_revision_governance_digest,
    inspect_personality_governance, load_recent_persona_evidence,
    select_personality_governance_targets, CoreRevisionLedgerStore, MemoryProfile,
    PersonalityGovernanceInspectionInput, RelationshipConstitutionStore,
    RelationshipPortfolioStore, RelationshipTopologyStore, SelfAuthoredCoreStore,
    SelfContinuityStore, TurnLedgerStore,
};
use crate::orchestrator;
use crate::runtime;
use crate::task_execution::{
    build_task_execution_operator_snapshot, render_task_execution_operator_text,
    TaskExecutionOperatorSnapshot,
};
use crate::tools::{ToolCatalogEntry, ToolExecutionGovernanceState, ToolRegistry};
use crate::util::{current_unix_secs, truncate_content_to_max};
use crate::Platform;
use serde::Serialize;

const REL_DIR_MANUAL_CONTINUITY_SNAPSHOTS: &str = "memory/continuity_snapshots/manual";
const OPERATOR_GOVERNANCE_RELATION_SUMMARY_MAX_CHARS: usize = 120;

#[derive(Debug, Serialize)]
pub struct OperatorPersonalityGovernanceRelation {
    pub scope_id: String,
    pub channel: String,
    pub chat_id: String,
    pub closure_ready: bool,
    pub repair_needed: bool,
    pub primary_action: String,
    pub repair_summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outstanding: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drift_flags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OperatorPersonalityGovernanceSnapshot {
    pub board_core_present: bool,
    pub board_revision: u64,
    pub core_review_due: bool,
    pub core_conservative_mode: bool,
    pub observation_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_channel: Option<String>,
    pub active_relations: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<OperatorPersonalityGovernanceRelation>,
}

pub struct OperatorStatusInput<'a> {
    pub config: &'a crate::config::AppConfig,
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

#[derive(Serialize)]
pub struct OperatorStatusSnapshot {
    pub platform_contract: OperatorPlatformContract,
    pub build_package: crate::BuildPackageSnapshot,
    pub operator_surface: crate::platform::operator_surface::OperatorSurfaceBudget,
    pub inbound_depth: usize,
    pub outbound_depth: usize,
    pub last_error: String,
    pub metrics: crate::metrics::MetricsSnapshot,
    pub resource: orchestrator::ResourceSnapshot,
    pub threads: runtime::ThreadRegistrySnapshot,
    pub os_closure: runtime::BeetleOsClosureReport,
    pub initiative: runtime::InitiativeSnapshot,
    pub presence: runtime::PresenceSnapshot,
    pub runtime_mode: runtime::RuntimeModeSnapshot,
    pub soul_kernel: runtime::SoulKernelStatus,
    pub continuity_tooling: OperatorContinuityTooling,
    pub task_execution: TaskExecutionOperatorSnapshot,
    pub capability_packages: CapabilityPackageOperatorSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_planes: Vec<DeviceCapabilityPlaneSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<ChannelCapabilitySnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_governance: Option<ToolExecutionGovernanceState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personality_governance: Option<OperatorPersonalityGovernanceSnapshot>,
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
    let continuity_tool_available = input.tool_registry.get("continuity_snapshot").is_some();
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
    let saved_snapshots = if compact_view {
        Vec::new()
    } else {
        input
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
            .collect::<Vec<_>>()
    };
    let audio_caps = if crate::compiled_voice_capability() {
        input.platform.audio_duplex_capabilities()
    } else {
        crate::platform::AudioDuplexCapabilities::unavailable()
    };
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
    let capability_planes = build_device_capability_snapshots(input.config, input.platform);
    let channels = if compact_view {
        Vec::new()
    } else {
        build_channel_capability_snapshots_for_registry(
            input.channel_capability_registry,
            input.llm_stream_enabled,
        )
    };
    let personality_governance = if compact_view {
        None
    } else {
        inspect_operator_personality_governance(OperatorPersonalityGovernanceInspectInput {
            self_authored_core_store: input.platform.self_authored_core_store().as_ref(),
            core_revision_ledger_store: input.platform.core_revision_ledger_store().as_ref(),
            relationship_constitution_store: input
                .platform
                .relationship_constitution_store()
                .as_ref(),
            relationship_portfolio_store: input.platform.relationship_portfolio_store().as_ref(),
            relationship_topology_store: input.platform.relationship_topology_store().as_ref(),
            self_continuity_store: input.platform.self_continuity_store().as_ref(),
            turn_ledger_store: input.platform.turn_ledger_store().as_ref(),
            profile: memory_system_kind.memory_profile(),
            now_secs: current_unix_secs(),
        })?
    };
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
        metrics: crate::metrics::snapshot(),
        resource: orchestrator::snapshot(),
        threads: runtime::thread_registry::snapshot(),
        os_closure,
        initiative,
        runtime_mode,
        soul_kernel,
        presence,
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
        capability_planes,
        channels,
        tools: if compact_view {
            Vec::new()
        } else {
            input.tool_registry.tool_catalog()?
        },
        tool_governance,
        personality_governance,
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
        "  inbound_depth: {}\n  outbound_depth: {}\n  last_error: {}\n  presence_state: {}\n  presence_headline: {}\n  presence_rationale: {}\n  initiative_action: {}\n  initiative_ready: {}\n  initiative_rationale: {}\n  runtime_mode: {}\n  soul_kernel_ready: {}\n  soul_kernel_safe_mode_readable: {}\n  soul_kernel_degraded: {}\n  soul_kernel_key_memory: {}\n  pressure: {:?}\n  continuity_saved_snapshots: {}\n",
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
        snapshot.resource.pressure,
        snapshot.continuity_tooling.saved_snapshot_count,
    ));
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
    if let Some(governance) = snapshot.personality_governance.as_ref() {
        out.push_str(&format!(
            "  personality_governance: board_revision={} board_core_present={} review_due={} conservative={} observation_active={} active_relations={}\n",
            governance.board_revision,
            governance.board_core_present,
            governance.core_review_due,
            governance.core_conservative_mode,
            governance.observation_active,
            governance.active_relations,
        ));
        if let Some(chat_id) = governance.anchor_chat_id.as_deref() {
            out.push_str(&format!("  personality_anchor_chat: {}\n", chat_id));
        }
        if let Some(channel) = governance.anchor_channel.as_deref() {
            out.push_str(&format!("  personality_anchor_channel: {}\n", channel));
        }
        for relation in &governance.relations {
            out.push_str(&format!(
                "    - relation {}:{} ready={} repair={} summary={}\n",
                relation.channel,
                relation.chat_id,
                relation.closure_ready,
                relation.primary_action,
                relation.repair_summary,
            ));
        }
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

struct OperatorPersonalityGovernanceInspectInput<'a> {
    self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    core_revision_ledger_store: &'a dyn CoreRevisionLedgerStore,
    relationship_constitution_store: &'a dyn RelationshipConstitutionStore,
    relationship_portfolio_store: &'a dyn RelationshipPortfolioStore,
    relationship_topology_store: &'a dyn RelationshipTopologyStore,
    self_continuity_store: &'a dyn SelfContinuityStore,
    turn_ledger_store: &'a dyn TurnLedgerStore,
    profile: MemoryProfile,
    now_secs: u64,
}

fn inspect_operator_personality_governance(
    input: OperatorPersonalityGovernanceInspectInput<'_>,
) -> crate::error::Result<Option<OperatorPersonalityGovernanceSnapshot>> {
    let subject_id = board_subject_scope_id();
    let self_authored_core = input.self_authored_core_store.get(subject_id)?;
    let core_revision_ledger = input.core_revision_ledger_store.get(subject_id)?;
    let self_continuity = input.self_continuity_store.get(subject_id)?;
    let relationship_portfolio = input.relationship_portfolio_store.get(subject_id)?;
    let relationship_topology = input.relationship_topology_store.get(subject_id)?;
    let has_governance_state = self_authored_core.is_some()
        || core_revision_ledger.is_some()
        || relationship_portfolio.is_some()
        || relationship_topology.is_some();
    if !has_governance_state {
        return Ok(None);
    }

    let governance = compute_core_revision_governance_digest(
        core_revision_ledger.as_ref(),
        self_authored_core
            .as_ref()
            .map(|core| core.last_reviewed_at)
            .unwrap_or(0),
        self_authored_core
            .as_ref()
            .map(|core| core.stability_score)
            .unwrap_or(0),
        input.now_secs,
    );
    let targets = select_operator_personality_governance_targets(
        self_continuity.as_ref(),
        relationship_portfolio.as_ref(),
        relationship_topology.as_ref(),
        input.now_secs,
        input.profile,
    );
    let mut relations = Vec::with_capacity(targets.len());
    for target in targets {
        let relationship_constitution = input
            .relationship_constitution_store
            .get(&target.scope_id)?;
        let recent_persona_evidence =
            load_recent_persona_evidence(input.turn_ledger_store, &target.scope_id)?;
        let inspection = inspect_personality_governance(PersonalityGovernanceInspectionInput {
            channel: &target.channel,
            chat_id: &target.chat_id,
            now_secs: input.now_secs,
            self_authored_core: self_authored_core.as_ref(),
            core_revision_ledger: core_revision_ledger.as_ref(),
            relationship_constitution: relationship_constitution.as_ref(),
            relationship_topology: relationship_topology.as_ref(),
            recent_persona_evidence: recent_persona_evidence.as_ref(),
        });
        relations.push(OperatorPersonalityGovernanceRelation {
            scope_id: target.scope_id,
            channel: target.channel,
            chat_id: target.chat_id,
            closure_ready: inspection.closure.ready,
            repair_needed: inspection.repair_plan.repair_needed,
            primary_action: inspection.repair_plan.primary_action.label().to_string(),
            repair_summary: truncate_content_to_max(
                inspection.repair_plan.summary.trim(),
                OPERATOR_GOVERNANCE_RELATION_SUMMARY_MAX_CHARS,
            )
            .into_owned(),
            outstanding: inspection.closure.outstanding,
            drift_flags: inspection
                .relationship_audit
                .map(|audit| audit.drift_flags)
                .unwrap_or_default(),
        });
    }

    Ok(Some(OperatorPersonalityGovernanceSnapshot {
        board_core_present: self_authored_core.is_some(),
        board_revision: self_authored_core
            .as_ref()
            .map(|core| core.revision)
            .unwrap_or(0),
        core_review_due: governance.review_due,
        core_conservative_mode: governance.conservative_mode,
        observation_active: governance.observation_active,
        anchor_chat_id: self_continuity
            .as_ref()
            .map(|continuity| continuity.last_user_chat_id.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        anchor_channel: self_continuity
            .as_ref()
            .map(|continuity| continuity.last_user_channel.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        active_relations: relations.len(),
        relations,
    }))
}

fn select_operator_personality_governance_targets(
    self_continuity: Option<&crate::memory::SelfContinuity>,
    relationship_portfolio: Option<&crate::memory::RelationshipPortfolio>,
    relationship_topology: Option<&crate::memory::RelationshipTopology>,
    now_secs: u64,
    profile: MemoryProfile,
) -> Vec<crate::memory::RelationshipSelectionTarget> {
    select_personality_governance_targets(
        self_continuity,
        relationship_portfolio,
        relationship_topology,
        now_secs,
        operator_governance_relation_limit(profile),
    )
}

fn operator_governance_relation_limit(profile: MemoryProfile) -> usize {
    match profile {
        MemoryProfile::Embedded => 1,
        MemoryProfile::Standard => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::error::Result;
    use crate::memory::{
        board_subject_scope_id, CoreRevisionLedger, CoreRevisionRecord,
        RelationshipGovernanceState, RelationshipInheritanceMode, RelationshipPortfolio,
        RelationshipPortfolioEntry, RelationshipTopology, RelationshipTopologyEntry,
        SelfAuthoredCore, SelfContinuity, TurnLedger,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore {
        values: Mutex<HashMap<String, SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), core.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubCoreRevisionLedgerStore {
        values: Mutex<HashMap<String, CoreRevisionLedger>>,
    }

    impl CoreRevisionLedgerStore for StubCoreRevisionLedgerStore {
        fn get(&self, scope_id: &str) -> Result<Option<CoreRevisionLedger>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, ledger: &CoreRevisionLedger) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipConstitutionStore;

    impl RelationshipConstitutionStore for StubRelationshipConstitutionStore {
        fn get(&self, _scope_id: &str) -> Result<Option<crate::memory::RelationshipConstitution>> {
            Ok(None)
        }

        fn set(
            &self,
            _scope_id: &str,
            _constitution: &crate::memory::RelationshipConstitution,
        ) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipPortfolioStore {
        values: Mutex<HashMap<String, RelationshipPortfolio>>,
    }

    impl RelationshipPortfolioStore for StubRelationshipPortfolioStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipPortfolio>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, portfolio: &RelationshipPortfolio) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), portfolio.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRelationshipTopologyStore {
        values: Mutex<HashMap<String, RelationshipTopology>>,
    }

    impl RelationshipTopologyStore for StubRelationshipTopologyStore {
        fn get(&self, scope_id: &str) -> Result<Option<RelationshipTopology>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(scope_id)
                .cloned())
        }

        fn set(&self, scope_id: &str, topology: &RelationshipTopology) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(scope_id.to_string(), topology.clone());
            Ok(())
        }

        fn clear(&self, scope_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(scope_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        values: Mutex<HashMap<String, SelfContinuity>>,
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), continuity.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> Result<()> {
            self.values
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn operator_personality_governance_summary_uses_active_relation_and_repair_plan() {
        let subject_id = board_subject_scope_id().to_string();
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let core_revision_ledger_store = StubCoreRevisionLedgerStore::default();
        let relationship_constitution_store = StubRelationshipConstitutionStore;
        let relationship_portfolio_store = StubRelationshipPortfolioStore::default();
        let relationship_topology_store = StubRelationshipTopologyStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let turn_ledger_store = StubTurnLedgerStore;

        self_authored_core_store
            .set(
                &subject_id,
                &SelfAuthoredCore {
                    revision: 3,
                    identity_anchor: "board".to_string(),
                    stability_score: 78,
                    last_reviewed_at: 900,
                    ..SelfAuthoredCore::default()
                },
            )
            .unwrap();
        core_revision_ledger_store
            .set(
                &subject_id,
                &CoreRevisionLedger {
                    entries: vec![CoreRevisionRecord {
                        reviewed_at: 900,
                        ..CoreRevisionRecord::default()
                    }],
                    updated_at: 900,
                },
            )
            .unwrap();
        self_continuity_store
            .set(
                &subject_id,
                &SelfContinuity {
                    last_user_chat_id: "chat-a".to_string(),
                    last_user_channel: "qq".to_string(),
                    updated_at: 950,
                    ..SelfContinuity::default()
                },
            )
            .unwrap();
        relationship_portfolio_store
            .set(
                &subject_id,
                &RelationshipPortfolio {
                    entries: vec![RelationshipPortfolioEntry {
                        scope_id: "rel:qq:chat-a".to_string(),
                        channel: "qq".to_string(),
                        chat_id: "chat-a".to_string(),
                        governance_state: RelationshipGovernanceState::Repair,
                        inheritance_mode: RelationshipInheritanceMode::Guarded,
                        priority_score: 120,
                        reason: "needs_attention".to_string(),
                        needs_runtime_attention: true,
                        next_review_at: 800,
                        last_active_at: 940,
                        ..RelationshipPortfolioEntry::default()
                    }],
                    updated_at: 940,
                },
            )
            .unwrap();
        relationship_topology_store
            .set(
                &subject_id,
                &RelationshipTopology {
                    entries: vec![RelationshipTopologyEntry {
                        scope_id: "rel:qq:chat-a".to_string(),
                        channel: "qq".to_string(),
                        chat_id: "chat-a".to_string(),
                        last_active_at: 940,
                        last_user_turn_at: 940,
                        last_runtime_refresh_at: 900,
                        ..RelationshipTopologyEntry::default()
                    }],
                    updated_at: 940,
                },
            )
            .unwrap();

        let snapshot =
            inspect_operator_personality_governance(OperatorPersonalityGovernanceInspectInput {
                self_authored_core_store: &self_authored_core_store,
                core_revision_ledger_store: &core_revision_ledger_store,
                relationship_constitution_store: &relationship_constitution_store,
                relationship_portfolio_store: &relationship_portfolio_store,
                relationship_topology_store: &relationship_topology_store,
                self_continuity_store: &self_continuity_store,
                turn_ledger_store: &turn_ledger_store,
                profile: MemoryProfile::Embedded,
                now_secs: 1_000,
            })
            .unwrap()
            .expect("governance snapshot");

        assert!(snapshot.board_core_present);
        assert_eq!(snapshot.board_revision, 3);
        assert_eq!(snapshot.active_relations, 1);
        assert_eq!(snapshot.anchor_chat_id.as_deref(), Some("chat-a"));
        assert_eq!(
            snapshot.relations[0].primary_action,
            "repair_relationship_constitution"
        );
        assert!(snapshot.relations[0].repair_needed);
        assert_eq!(snapshot.relations[0].chat_id, "chat-a");
    }

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
        let channel_capability_registry = crate::build_channel_capability_registry(&config, false);
        let capability_package_runtime_capabilities =
            crate::build_capability_package_runtime_capabilities(
                &channel_capability_registry,
                false,
            );

        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
            channel_capability_registry: &channel_capability_registry,
            capability_package_runtime_capabilities: &capability_package_runtime_capabilities,
            current_channel: config.enabled_channel.as_str(),
            inbound_depth: 0,
            outbound_depth: 0,
            version: "0.0.0",
            board_id: "test-board",
            llm_stream_enabled: false,
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
