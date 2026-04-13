//! Unified operator-facing status contract for HTTP and CLI.

use crate::device_capability::{build_device_capability_snapshots, DeviceCapabilityPlaneSnapshot};
use crate::diagnosis::{
    build_delivery_diagnosis, build_memory_runtime_diagnosis,
    build_network_path_diagnosis_from_runtime, build_system_diagnosis,
    build_voice_path_diagnosis_from_runtime, DeliveryDiagnosisInput, DiagnosisResult,
    SystemDiagnosisInput,
};
use crate::orchestrator;
use crate::platform::memory_operator_surface::{
    build_memory_operator_surface_with_capabilities, render_memory_operator_surface_text,
    MemoryOperatorSurfaceSummary,
};
use crate::runtime;
use crate::tools::{ToolExecutionGovernanceState, ToolRegistry};
use crate::util::current_unix_secs;
use crate::Platform;
use serde::Serialize;

pub struct OperatorStatusInput<'a> {
    pub config: &'a crate::config::AppConfig,
    pub platform: &'a dyn Platform,
    pub tool_registry: &'a ToolRegistry,
}

#[derive(Debug, Serialize)]
pub struct OperatorPlatformContract {
    pub memory_system_kind: String,
    pub config_plane_active: bool,
    pub wifi_scan_available: bool,
    pub hardware_discovery_available: bool,
}

#[derive(Serialize)]
pub struct OperatorStatusSnapshot {
    pub platform_contract: OperatorPlatformContract,
    pub build_package: crate::BuildPackageSnapshot,
    pub operator_surface: crate::platform::operator_surface::OperatorSurfaceBudget,
    pub reply_pipeline: ReplyPipelineOperatorSummary,
    pub delivery_diagnosis: DiagnosisResult,
    pub system_diagnosis: DiagnosisResult,
    pub memory_runtime_diagnosis: DiagnosisResult,
    pub network_path_diagnosis: DiagnosisResult,
    pub voice_path_diagnosis: DiagnosisResult,
    pub memory_operator_surface: MemoryOperatorSurfaceSummary,
    pub workflow: runtime::WorkflowAuditSnapshot,
    pub programmable_reasoning: crate::ProgrammableReasoningOperatorSnapshot,
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

#[derive(Debug, Serialize)]
pub struct ReplyPipelineOperatorSummary {
    pub request_semantics_last_ms: u64,
    pub tool_exec_last_ms: u64,
    pub surface_finalize_last_ms: u64,
    pub mental_privacy_review_last_ms: u64,
    pub final_recovery_last_ms: u64,
    pub dispatch_send_fail_total: u64,
    pub outbound_enqueue_fail_total: u64,
    pub dominant_stage: &'static str,
}

impl ReplyPipelineOperatorSummary {
    fn from_metrics(metrics: &crate::metrics::MetricsSnapshot) -> Self {
        let finalization_total_ms = metrics
            .surface_finalize_last_ms
            .saturating_add(metrics.mental_privacy_review_last_ms)
            .saturating_add(metrics.final_recovery_last_ms);
        let dominant_stage = if metrics.dispatch_send_fail > 0 || metrics.outbound_enqueue_fail > 0
        {
            "delivery_failure"
        } else if metrics.tool_exec_last_ms >= metrics.request_semantics_last_ms
            && metrics.tool_exec_last_ms >= finalization_total_ms
            && metrics.tool_exec_last_ms > 0
        {
            "tool_execution"
        } else if finalization_total_ms >= metrics.request_semantics_last_ms
            && finalization_total_ms > 0
        {
            "finalization"
        } else if metrics.request_semantics_last_ms > 0 {
            "request_semantics"
        } else {
            "idle"
        };
        Self {
            request_semantics_last_ms: metrics.request_semantics_last_ms,
            tool_exec_last_ms: metrics.tool_exec_last_ms,
            surface_finalize_last_ms: metrics.surface_finalize_last_ms,
            mental_privacy_review_last_ms: metrics.mental_privacy_review_last_ms,
            final_recovery_last_ms: metrics.final_recovery_last_ms,
            dispatch_send_fail_total: metrics.dispatch_send_fail,
            outbound_enqueue_fail_total: metrics.outbound_enqueue_fail,
            dominant_stage,
        }
    }
}

pub fn build_operator_status(
    input: OperatorStatusInput<'_>,
) -> crate::error::Result<OperatorStatusSnapshot> {
    crate::platform::refresh_runtime_state();
    let memory_system_kind = input.platform.memory_system_kind();
    let operator_surface =
        crate::platform::operator_surface::current_operator_surface_budget(memory_system_kind);
    let compact_view = operator_surface.compact_view;
    let tool_governance = if compact_view {
        None
    } else {
        input.tool_registry.inspect_execution_governance()?
    };
    let capability_planes = build_device_capability_snapshots(input.config, input.platform);
    let runtime_capabilities = orchestrator::runtime_capability_snapshot();
    let reply_pipeline = ReplyPipelineOperatorSummary::from_metrics(&crate::metrics::snapshot());
    let delivery_diagnosis = build_delivery_diagnosis(DeliveryDiagnosisInput {
        enabled_channel: Some(input.config.enabled_channel.as_str()),
        metrics: crate::metrics::snapshot(),
        runtime_capabilities: runtime_capabilities.clone(),
    });
    let presence = runtime::inspect_platform_presence(input.platform, current_unix_secs());
    let initiative = runtime::inspect_platform_initiative(input.platform, current_unix_secs());
    let os_closure = runtime::inspect_beetle_os_closure(&presence, &initiative);
    let system_diagnosis = build_system_diagnosis(SystemDiagnosisInput {
        enabled_channel: Some(input.config.enabled_channel.as_str()),
        resource: orchestrator::snapshot(),
        metrics: crate::metrics::snapshot(),
        runtime_capabilities: runtime_capabilities.clone(),
        presence_state: presence.state.as_str(),
        runtime_mode: presence.runtime_mode.current_mode.as_str(),
        os_closure_ready: os_closure.ready,
        wifi_connected: presence.wifi_connected,
    });
    let continuity_snapshot_supported = input.tool_registry.get("continuity_snapshot").is_some();
    let memory_operator_surface = build_memory_operator_surface_with_capabilities(
        input.platform,
        continuity_snapshot_supported,
        None,
    )?;
    let memory_runtime_diagnosis = build_memory_runtime_diagnosis(&memory_operator_surface);
    let network_path_diagnosis = build_network_path_diagnosis_from_runtime(
        input.platform,
        input.config,
        crate::i18n::locale_from_store(input.platform.config_store().as_ref()),
    );
    let voice_path_diagnosis =
        build_voice_path_diagnosis_from_runtime(input.platform, input.config);
    let runtime_mode = presence.runtime_mode;
    let soul_kernel = presence.soul_kernel.clone();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let supervisor = presence.supervisor.clone();
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let release = presence.release.clone();
    Ok(OperatorStatusSnapshot {
        platform_contract: OperatorPlatformContract {
            memory_system_kind: memory_system_kind.as_str().to_string(),
            config_plane_active: crate::state::config_plane_active(),
            wifi_scan_available: input.platform.wifi_scan().is_some(),
            hardware_discovery_available: input.platform.hardware_discovery().is_some(),
        },
        build_package: crate::current_build_package(),
        operator_surface,
        reply_pipeline,
        delivery_diagnosis,
        system_diagnosis,
        memory_runtime_diagnosis,
        network_path_diagnosis,
        voice_path_diagnosis,
        memory_operator_surface,
        workflow: runtime::workflow_audit_snapshot(8),
        programmable_reasoning: crate::programmable_reasoning_operator_snapshot(),
        threads: runtime::thread_registry::snapshot(),
        os_closure,
        initiative,
        runtime_mode,
        soul_kernel,
        presence,
        capability_planes,
        runtime_capabilities,
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
        "  memory_system_kind: {}\n  config_plane_active: {}\n  wifi_scan_available: {}\n  hardware_discovery_available: {}\n  operator_surface_compact: {}\n  operator_window_required: {}\n",
        snapshot.platform_contract.memory_system_kind,
        snapshot.platform_contract.config_plane_active,
        snapshot.platform_contract.wifi_scan_available,
        snapshot.platform_contract.hardware_discovery_available,
        snapshot.operator_surface.compact_view,
        snapshot.operator_surface.window_required_for_deep_routes,
    ));
    out.push_str(&format!(
        "  reply_pipeline_dominant_stage: {}\n  reply_pipeline_request_semantics_last_ms: {}\n  reply_pipeline_tool_exec_last_ms: {}\n  reply_pipeline_surface_finalize_last_ms: {}\n  reply_pipeline_mental_privacy_review_last_ms: {}\n  reply_pipeline_final_recovery_last_ms: {}\n  reply_pipeline_dispatch_send_fail_total: {}\n  reply_pipeline_outbound_enqueue_fail_total: {}\n",
        snapshot.reply_pipeline.dominant_stage,
        snapshot.reply_pipeline.request_semantics_last_ms,
        snapshot.reply_pipeline.tool_exec_last_ms,
        snapshot.reply_pipeline.surface_finalize_last_ms,
        snapshot.reply_pipeline.mental_privacy_review_last_ms,
        snapshot.reply_pipeline.final_recovery_last_ms,
        snapshot.reply_pipeline.dispatch_send_fail_total,
        snapshot.reply_pipeline.outbound_enqueue_fail_total,
    ));
    out.push_str(&format!(
        "  delivery_diagnosis_summary: {}\n  delivery_diagnosis_confidence: {:?}\n",
        snapshot.delivery_diagnosis.summary, snapshot.delivery_diagnosis.confidence,
    ));
    out.push_str(&format!(
        "  system_diagnosis_summary: {}\n  system_diagnosis_confidence: {:?}\n",
        snapshot.system_diagnosis.summary, snapshot.system_diagnosis.confidence,
    ));
    out.push_str(&format!(
        "  memory_runtime_diagnosis_summary: {}\n  memory_runtime_diagnosis_confidence: {:?}\n",
        snapshot.memory_runtime_diagnosis.summary, snapshot.memory_runtime_diagnosis.confidence,
    ));
    out.push_str(&format!(
        "  network_path_diagnosis_summary: {}\n  network_path_diagnosis_confidence: {:?}\n",
        snapshot.network_path_diagnosis.summary, snapshot.network_path_diagnosis.confidence,
    ));
    out.push_str(&format!(
        "  voice_path_diagnosis_summary: {}\n  voice_path_diagnosis_confidence: {:?}\n",
        snapshot.voice_path_diagnosis.summary, snapshot.voice_path_diagnosis.confidence,
    ));
    if let Some(window) = snapshot.operator_surface.operator_window.as_ref() {
        out.push_str(&format!(
            "  operator_window_active: {}\n  operator_window_remaining_secs: {}\n",
            window.active, window.remaining_secs,
        ));
    }
    out.push_str(&format!(
        "  presence_state: {}\n  presence_headline: {}\n  presence_rationale: {}\n  initiative_action: {}\n  initiative_ready: {}\n  initiative_rationale: {}\n  runtime_mode: {}\n  soul_kernel_ready: {}\n  soul_kernel_safe_mode_readable: {}\n  soul_kernel_degraded: {}\n  soul_kernel_key_memory: {}\n",
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
    out.push_str(&format!(
        "  programmable_reasoning_stage: {}\n  programmable_reasoning_execution_enabled: {}\n  programmable_reasoning_backend: {}\n  programmable_reasoning_operator_summary: {}\n",
        match snapshot.programmable_reasoning.stage {
            crate::ProgrammableReasoningStage::ConstitutionOnly => "constitution_only",
            crate::ProgrammableReasoningStage::TaskScriptingBaseline => "task_scripting_baseline",
            crate::ProgrammableReasoningStage::MemoryQueryPlane => "memory_query_plane",
            crate::ProgrammableReasoningStage::IdleMemoryForge => "idle_memory_forge",
            crate::ProgrammableReasoningStage::MemoryAttackDistillation => {
                "memory_attack_distillation"
            }
            crate::ProgrammableReasoningStage::CapabilityBridgeExpansion => {
                "capability_bridge_expansion"
            }
            crate::ProgrammableReasoningStage::ExperienceCrystal => "experience_crystal",
        },
        snapshot.programmable_reasoning.runtime_contract.execution_enabled,
        match snapshot.programmable_reasoning.runtime_contract.execution_backend {
            crate::ProgrammableReasoningExecutionBackend::None => "none",
            crate::ProgrammableReasoningExecutionBackend::LuaSandbox => "lua_sandbox",
        },
        snapshot.programmable_reasoning.operator_summary,
    ));
    out.push_str(&format!(
        "  workflow_recent_records: {}\n  workflow_executed: {}\n  workflow_deferred: {}\n  workflow_suppressed: {}\n  workflow_no_trigger: {}\n  workflow_failed: {}\n",
        snapshot.workflow.summary.total_retained,
        snapshot.workflow.summary.executed,
        snapshot.workflow.summary.deferred,
        snapshot.workflow.summary.suppressed,
        snapshot.workflow.summary.no_trigger,
        snapshot.workflow.summary.failed,
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
    out.push_str(&render_memory_operator_surface_text(
        &snapshot.memory_operator_surface,
    ));
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
        assert!(payload.get("reply_pipeline").is_some());
        assert!(payload["reply_pipeline"].get("tool_exec_last_ms").is_some());
        assert!(payload["reply_pipeline"].get("dominant_stage").is_some());
        assert!(payload.get("workflow").is_some());
        assert!(payload["workflow"].get("summary").is_some());
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
