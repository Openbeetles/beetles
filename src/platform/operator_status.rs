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
use crate::reasoning::summarize_programmable_reasoning_operator;
use crate::runtime;
use crate::skills::build_runtime_skill_operator_summary;
use crate::task_execution::build_task_learning_operator_snapshot;
use crate::tools::{ToolExecutionGovernanceState, ToolRegistry};
use crate::util::current_unix_secs;
use crate::Platform;
use serde::Serialize;
use std::collections::BTreeMap;

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
    let tool_governance_state = input.tool_registry.inspect_execution_governance()?;
    let tool_governance = if compact_view {
        None
    } else {
        tool_governance_state.clone()
    };
    let programmable_reasoning_usage =
        build_programmable_reasoning_usage_analytics(tool_governance_state.as_ref());
    let programmable_reasoning_timeline =
        build_programmable_reasoning_timeline(tool_governance_state.as_ref());
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
    let runtime_skill_summary =
        build_runtime_skill_operator_summary(input.platform.skill_storage().as_ref());
    let task_learning_snapshot =
        build_task_learning_operator_snapshot(input.platform.task_learning_store().as_ref())?;
    let mut programmable_reasoning = crate::programmable_reasoning_operator_snapshot(
        &runtime_skill_summary,
        Some(&task_learning_snapshot),
    );
    programmable_reasoning.usage_analytics = programmable_reasoning_usage;
    programmable_reasoning.timeline = programmable_reasoning_timeline;
    programmable_reasoning.maintenance_digest = build_programmable_reasoning_maintenance_digest(
        &programmable_reasoning.usage_analytics,
        &programmable_reasoning.timeline,
    );
    programmable_reasoning.operator_summary =
        summarize_programmable_reasoning_operator(&programmable_reasoning);
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
        programmable_reasoning,
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
        "  programmable_reasoning_stage: {}\n  programmable_reasoning_execution_enabled: {}\n  programmable_reasoning_backend: {}\n  programmable_reasoning_operator_summary: {}\n  programmable_reasoning_recent_events: {}\n  programmable_reasoning_digest_status: {}\n",
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
            crate::ProgrammableReasoningStage::EngineeringSynthesis => "engineering_synthesis",
        },
        snapshot.programmable_reasoning.runtime_contract.execution_enabled,
        match snapshot.programmable_reasoning.runtime_contract.execution_backend {
            crate::ProgrammableReasoningExecutionBackend::None => "none",
            crate::ProgrammableReasoningExecutionBackend::LuaSandbox => "lua_sandbox",
        },
        snapshot.programmable_reasoning.operator_summary,
        snapshot.programmable_reasoning.timeline.recent_events.len(),
        snapshot.programmable_reasoning.maintenance_digest.status,
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

fn build_programmable_reasoning_usage_analytics(
    governance: Option<&ToolExecutionGovernanceState>,
) -> crate::ProgrammableReasoningUsageAnalytics {
    #[derive(Default)]
    struct ToolUsageAccumulator {
        total_attempts: usize,
        succeeded: usize,
        failed: usize,
        denied: usize,
        resource_denied: usize,
        last_seen_at: Option<u64>,
    }

    let Some(governance) = governance else {
        return crate::ProgrammableReasoningUsageAnalytics::default();
    };

    let mut usage = crate::ProgrammableReasoningUsageAnalytics::default();
    let mut tool_counts: BTreeMap<String, ToolUsageAccumulator> = BTreeMap::new();
    for record in &governance.recent_records {
        if !is_programmable_reasoning_tool(record.tool_name.as_str()) {
            continue;
        }
        let Some(status_bucket) = classify_programmable_reasoning_record(record.status) else {
            continue;
        };
        usage.recent_total_attempts += 1;
        usage.last_seen_at = Some(usage.last_seen_at.map_or(record.recorded_at, |current| {
            current.max(record.recorded_at)
        }));
        if usage.last_seen_at == Some(record.recorded_at) {
            usage.last_tool_name = Some(record.tool_name.clone());
        }
        let entry = tool_counts.entry(record.tool_name.clone()).or_default();
        entry.total_attempts += 1;
        entry.last_seen_at = Some(entry.last_seen_at.map_or(record.recorded_at, |current| {
            current.max(record.recorded_at)
        }));
        match status_bucket {
            ProgrammableReasoningRecordBucket::Succeeded => {
                usage.recent_succeeded += 1;
                entry.succeeded += 1;
            }
            ProgrammableReasoningRecordBucket::Failed => {
                usage.recent_failed += 1;
                entry.failed += 1;
            }
            ProgrammableReasoningRecordBucket::Denied => {
                usage.recent_denied += 1;
                entry.denied += 1;
            }
            ProgrammableReasoningRecordBucket::ResourceDenied => {
                usage.recent_resource_denied += 1;
                entry.resource_denied += 1;
            }
        }
    }

    let mut counts = tool_counts
        .into_iter()
        .map(
            |(tool_name, entry)| crate::ProgrammableReasoningToolUsageSummary {
                tool_name,
                total_attempts: entry.total_attempts,
                succeeded: entry.succeeded,
                failed: entry.failed,
                denied: entry.denied,
                resource_denied: entry.resource_denied,
                last_seen_at: entry.last_seen_at,
            },
        )
        .collect::<Vec<_>>();
    counts.sort_by(|left, right| {
        right
            .total_attempts
            .cmp(&left.total_attempts)
            .then_with(|| right.last_seen_at.cmp(&left.last_seen_at))
            .then_with(|| left.tool_name.cmp(&right.tool_name))
    });
    usage.tool_counts = counts;
    usage
}

fn build_programmable_reasoning_timeline(
    governance: Option<&ToolExecutionGovernanceState>,
) -> crate::ProgrammableReasoningTimeline {
    const PROGRAMMABLE_REASONING_TIMELINE_LIMIT: usize = 8;

    let Some(governance) = governance else {
        return crate::ProgrammableReasoningTimeline::default();
    };

    let recent_events = governance
        .recent_records
        .iter()
        .rev()
        .filter(|record| is_programmable_reasoning_tool(record.tool_name.as_str()))
        .filter_map(programmable_reasoning_timeline_event_from_record)
        .take(PROGRAMMABLE_REASONING_TIMELINE_LIMIT)
        .collect::<Vec<_>>();

    crate::ProgrammableReasoningTimeline { recent_events }
}

fn build_programmable_reasoning_maintenance_digest(
    usage: &crate::ProgrammableReasoningUsageAnalytics,
    timeline: &crate::ProgrammableReasoningTimeline,
) -> crate::ProgrammableReasoningMaintenanceDigest {
    let attention_event_count = usage
        .recent_failed
        .saturating_add(usage.recent_denied)
        .saturating_add(usage.recent_resource_denied);
    let last_event = timeline.recent_events.first();
    let attention_tools = timeline
        .recent_events
        .iter()
        .filter(|event| event.status != "succeeded")
        .map(|event| event.tool_name.clone())
        .fold(Vec::<String>::new(), |mut acc, tool_name| {
            if !acc.iter().any(|item| item == &tool_name) {
                acc.push(tool_name);
            }
            acc
        });
    let status = if usage.recent_total_attempts == 0 {
        "idle"
    } else if attention_event_count > 0 {
        "attention"
    } else {
        "healthy"
    };
    let headline = match status {
        "idle" => "no recent programmable reasoning activity".to_string(),
        "attention" => format!(
            "{} recent attempts, {} need attention",
            usage.recent_total_attempts, attention_event_count
        ),
        _ => format!(
            "{} recent attempts, all completed successfully",
            usage.recent_total_attempts
        ),
    };
    crate::ProgrammableReasoningMaintenanceDigest {
        status: status.to_string(),
        headline,
        attention_event_count,
        last_event_tool_name: last_event.map(|event| event.tool_name.clone()),
        last_event_status: last_event.map(|event| event.status.clone()),
        attention_tools,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgrammableReasoningRecordBucket {
    Succeeded,
    Failed,
    Denied,
    ResourceDenied,
}

fn classify_programmable_reasoning_record(
    status: crate::tools::ToolExecutionRecordStatus,
) -> Option<ProgrammableReasoningRecordBucket> {
    match status {
        crate::tools::ToolExecutionRecordStatus::Succeeded => {
            Some(ProgrammableReasoningRecordBucket::Succeeded)
        }
        crate::tools::ToolExecutionRecordStatus::Failed => {
            Some(ProgrammableReasoningRecordBucket::Failed)
        }
        crate::tools::ToolExecutionRecordStatus::Denied => {
            Some(ProgrammableReasoningRecordBucket::Denied)
        }
        crate::tools::ToolExecutionRecordStatus::ResourceDenied => {
            Some(ProgrammableReasoningRecordBucket::ResourceDenied)
        }
        crate::tools::ToolExecutionRecordStatus::Allowed => None,
    }
}

fn programmable_reasoning_timeline_event_from_record(
    record: &crate::tools::ToolExecutionRecord,
) -> Option<crate::ProgrammableReasoningTimelineEvent> {
    let detail = if record.summary.trim().is_empty() {
        record.reason.trim()
    } else {
        record.summary.trim()
    };
    let status = match record.status {
        crate::tools::ToolExecutionRecordStatus::Succeeded => "succeeded",
        crate::tools::ToolExecutionRecordStatus::Failed => "failed",
        crate::tools::ToolExecutionRecordStatus::Denied => "denied",
        crate::tools::ToolExecutionRecordStatus::ResourceDenied => "resource_denied",
        crate::tools::ToolExecutionRecordStatus::Allowed => return None,
    };
    Some(crate::ProgrammableReasoningTimelineEvent {
        recorded_at: record.recorded_at,
        tool_name: record.tool_name.clone(),
        status: status.to_string(),
        detail: detail.to_string(),
    })
}

fn is_programmable_reasoning_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "lua_query"
            | "lua_memory_query"
            | "lua_tool_bridge"
            | "lua_datasheet_distill"
            | "lua_register_table_helper"
            | "lua_protocol_frame_helper"
            | "lua_state_machine_checker"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::IngressKind;
    use crate::config::AppConfig;
    use crate::tools::{
        ToolApprovalMode, ToolEffectClass, ToolExecutionGovernance, ToolExecutionOutcome,
        ToolExecutionPermit, ToolExecutionRequest, ToolExecutionShape, ToolMetadata, ToolRiskLevel,
        ToolRollbackKind,
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl crate::platform::StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> crate::Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> crate::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> crate::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> crate::Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn build_operator_status_includes_device_capability_planes() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
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

    #[test]
    fn build_operator_status_summarizes_programmable_reasoning_usage() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));
        let tool_registry =
            crate::tools::ToolRegistry::new().with_execution_governance(Arc::clone(&governance));

        governance
            .record_success(
                &reasoning_permit("lua_register_table_helper"),
                &ToolExecutionOutcome::text("register table parsed"),
            )
            .expect("record success");
        governance
            .record_failure(
                &reasoning_permit("lua_state_machine_checker"),
                &crate::Error::config("lua_state_machine_checker_test", "transition missing"),
            )
            .expect("record failure");
        governance
            .record_resource_denial(&reasoning_permit("lua_query"), "runtime capability blocked")
            .expect("record resource denial");
        governance
            .assess(ToolExecutionRequest {
                tool_name: "lua_protocol_frame_helper".to_string(),
                ingress: IngressKind::User,
                channel: "telegram".to_string(),
                metadata: ToolMetadata::task(),
                shape: reasoning_shape("lua_protocol_frame_helper")
                    .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                    .with_approval_granted(false),
                requires_network: false,
            })
            .expect("record denial");
        governance
            .record_success(
                &non_reasoning_permit("message"),
                &ToolExecutionOutcome::text("sent message"),
            )
            .expect("record unrelated success");

        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
        })
        .expect("operator status");

        let usage = snapshot.programmable_reasoning.usage_analytics;
        assert_eq!(usage.recent_total_attempts, 4);
        assert_eq!(usage.recent_succeeded, 1);
        assert_eq!(usage.recent_failed, 1);
        assert_eq!(usage.recent_denied, 1);
        assert_eq!(usage.recent_resource_denied, 1);
        assert_eq!(
            usage.last_tool_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert!(usage
            .tool_counts
            .iter()
            .any(|entry| entry.tool_name == "lua_register_table_helper" && entry.succeeded == 1));
        assert!(usage
            .tool_counts
            .iter()
            .any(|entry| entry.tool_name == "lua_state_machine_checker" && entry.failed == 1));
        assert!(usage
            .tool_counts
            .iter()
            .any(|entry| entry.tool_name == "lua_query" && entry.resource_denied == 1));
        assert!(!usage
            .tool_counts
            .iter()
            .any(|entry| entry.tool_name == "message"));
    }

    #[test]
    fn build_operator_status_exposes_programmable_reasoning_timeline() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));
        let tool_registry =
            crate::tools::ToolRegistry::new().with_execution_governance(Arc::clone(&governance));

        governance
            .record_success(
                &reasoning_permit("lua_register_table_helper"),
                &ToolExecutionOutcome::text("register table parsed"),
            )
            .expect("record success");
        governance
            .record_failure(
                &reasoning_permit("lua_state_machine_checker"),
                &crate::Error::config("lua_state_machine_checker_test", "transition missing"),
            )
            .expect("record failure");
        governance
            .record_resource_denial(&reasoning_permit("lua_query"), "runtime capability blocked")
            .expect("record resource denial");
        governance
            .assess(ToolExecutionRequest {
                tool_name: "lua_protocol_frame_helper".to_string(),
                ingress: IngressKind::User,
                channel: "telegram".to_string(),
                metadata: ToolMetadata::task(),
                shape: reasoning_shape("lua_protocol_frame_helper")
                    .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                    .with_approval_granted(false),
                requires_network: false,
            })
            .expect("record denial");
        governance
            .record_success(
                &non_reasoning_permit("message"),
                &ToolExecutionOutcome::text("sent message"),
            )
            .expect("record unrelated success");

        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
        })
        .expect("operator status");

        let timeline = snapshot.programmable_reasoning.timeline;
        assert_eq!(timeline.recent_events.len(), 4);
        assert_eq!(
            timeline.recent_events[0].tool_name,
            "lua_protocol_frame_helper"
        );
        assert_eq!(timeline.recent_events[0].status, "denied");
        assert_eq!(timeline.recent_events[0].detail, "explicit_intent_required");
        assert_eq!(timeline.recent_events[1].tool_name, "lua_query");
        assert_eq!(timeline.recent_events[1].status, "resource_denied");
        assert_eq!(
            timeline.recent_events[1].detail,
            "runtime capability blocked"
        );
        assert_eq!(
            timeline.recent_events[2].tool_name,
            "lua_state_machine_checker"
        );
        assert_eq!(timeline.recent_events[2].status, "failed");
        assert_eq!(
            timeline.recent_events[2].detail,
            "config: transition missing (stage: lua_state_machine_checker_test)"
        );
        assert_eq!(
            timeline.recent_events[3].tool_name,
            "lua_register_table_helper"
        );
        assert_eq!(timeline.recent_events[3].status, "succeeded");
        assert_eq!(timeline.recent_events[3].detail, "register table parsed");
        assert!(!timeline
            .recent_events
            .iter()
            .any(|event| event.tool_name == "message"));
    }

    #[test]
    fn build_operator_status_exposes_programmable_reasoning_maintenance_digest() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));
        let tool_registry =
            crate::tools::ToolRegistry::new().with_execution_governance(Arc::clone(&governance));

        governance
            .record_success(
                &reasoning_permit("lua_register_table_helper"),
                &ToolExecutionOutcome::text("register table parsed"),
            )
            .expect("record success");
        governance
            .record_failure(
                &reasoning_permit("lua_state_machine_checker"),
                &crate::Error::config("lua_state_machine_checker_test", "transition missing"),
            )
            .expect("record failure");
        governance
            .record_resource_denial(&reasoning_permit("lua_query"), "runtime capability blocked")
            .expect("record resource denial");
        governance
            .assess(ToolExecutionRequest {
                tool_name: "lua_protocol_frame_helper".to_string(),
                ingress: IngressKind::User,
                channel: "telegram".to_string(),
                metadata: ToolMetadata::task(),
                shape: reasoning_shape("lua_protocol_frame_helper")
                    .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                    .with_approval_granted(false),
                requires_network: false,
            })
            .expect("record denial");

        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
        })
        .expect("operator status");

        let digest = snapshot.programmable_reasoning.maintenance_digest;
        assert_eq!(digest.status, "attention");
        assert_eq!(
            digest.last_event_tool_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert_eq!(digest.last_event_status.as_deref(), Some("denied"));
        assert_eq!(digest.attention_event_count, 3);
        assert_eq!(
            digest.attention_tools,
            vec![
                "lua_protocol_frame_helper".to_string(),
                "lua_query".to_string(),
                "lua_state_machine_checker".to_string()
            ]
        );
        assert!(digest
            .headline
            .contains("4 recent attempts, 3 need attention"));
    }

    #[test]
    fn build_operator_status_exposes_real_experience_crystal_counts_and_summary() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        use std::time::{SystemTime, UNIX_EPOCH};

        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let tool_registry = crate::tools::ToolRegistry::new();
        let unique = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let topic = format!("operator_status_review_{unique}");
        let skill_name = format!("runtime_skill__{topic}");
        let now_secs: u64 = 4_102_444_800;
        let baseline_runtime_skills =
            build_runtime_skill_operator_summary(platform.skill_storage().as_ref());
        let baseline_learning =
            build_task_learning_operator_snapshot(platform.task_learning_store().as_ref())
                .expect("baseline task learning");

        crate::skills::upsert_runtime_skill(
            platform.skill_storage().as_ref(),
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Operator status review".to_string(),
                summary: "Validated operator status review procedure.".to_string(),
                content: "1. inspect operator\n2. compare state\n3. verify".to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: now_secs.saturating_sub(60),
            },
        )
        .expect("write runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            platform.skill_storage().as_ref(),
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            now_secs,
            "final_answer",
        )
        .expect("record runtime skill outcome");

        platform
            .task_learning_store()
            .upsert(&crate::task_execution::TaskLearningRecord {
                learning_id: format!("learning-promoted-{unique}"),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-1".to_string(),
                run_id: format!("run-promoted-{unique}"),
                step_id: "s01".to_string(),
                kind: crate::task_execution::TaskLearningKind::ReusableProcedure,
                route: crate::task_execution::TaskLearningRoute::RuntimeSkill,
                run_status: crate::task_execution::TaskRunStatus::Completed,
                topic: topic.clone(),
                summary: "Promoted operator procedure.".to_string(),
                content: "Inspect, compare, verify.".to_string(),
                memory_kind: None,
                review_summary: "Reusable".to_string(),
                source_artifact_ids: Vec::new(),
                provenance: "operator status test".to_string(),
                archive_note_name: String::new(),
                route_detail: "promoted".to_string(),
                candidate_state: Some(crate::task_execution::TaskLearningCandidateState::Promoted),
                candidate_state_updated_at: now_secs,
                last_failure_reason: String::new(),
                observed_at: now_secs,
            })
            .expect("write promoted learning record");
        platform
            .task_learning_store()
            .upsert(&crate::task_execution::TaskLearningRecord {
                learning_id: format!("learning-observed-{unique}"),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-1".to_string(),
                run_id: format!("run-observed-{unique}"),
                step_id: "s02".to_string(),
                kind: crate::task_execution::TaskLearningKind::ReusableProcedure,
                route: crate::task_execution::TaskLearningRoute::Pending,
                run_status: crate::task_execution::TaskRunStatus::Completed,
                topic: format!("{topic}_observed"),
                summary: "Observed operator procedure.".to_string(),
                content: "Observe candidate.".to_string(),
                memory_kind: None,
                review_summary: "Observe".to_string(),
                source_artifact_ids: Vec::new(),
                provenance: "operator status test".to_string(),
                archive_note_name: String::new(),
                route_detail: "observed".to_string(),
                candidate_state: Some(crate::task_execution::TaskLearningCandidateState::Observed),
                candidate_state_updated_at: now_secs,
                last_failure_reason: String::new(),
                observed_at: now_secs.saturating_sub(1),
            })
            .expect("write observed learning record");
        platform
            .task_learning_store()
            .upsert(&crate::task_execution::TaskLearningRecord {
                learning_id: format!("learning-rejected-{unique}"),
                source_channel: "telegram".to_string(),
                source_chat_id: "chat-1".to_string(),
                run_id: format!("run-rejected-{unique}"),
                step_id: "s03".to_string(),
                kind: crate::task_execution::TaskLearningKind::ReusableProcedure,
                route: crate::task_execution::TaskLearningRoute::Pending,
                run_status: crate::task_execution::TaskRunStatus::Completed,
                topic: format!("{topic}_rejected"),
                summary: "Rejected operator procedure.".to_string(),
                content: "Reject candidate.".to_string(),
                memory_kind: None,
                review_summary: "Reject".to_string(),
                source_artifact_ids: Vec::new(),
                provenance: "operator status test".to_string(),
                archive_note_name: String::new(),
                route_detail: "rejected".to_string(),
                candidate_state: Some(crate::task_execution::TaskLearningCandidateState::Rejected),
                candidate_state_updated_at: now_secs,
                last_failure_reason: "weak procedure".to_string(),
                observed_at: now_secs.saturating_sub(2),
            })
            .expect("write rejected learning record");
        let expected_runtime_skills =
            build_runtime_skill_operator_summary(platform.skill_storage().as_ref());
        let expected_learning =
            build_task_learning_operator_snapshot(platform.task_learning_store().as_ref())
                .expect("expected task learning snapshot");

        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
        })
        .expect("operator status");

        let crystals = snapshot.programmable_reasoning.experience_crystals;
        assert!(expected_runtime_skills.total >= baseline_runtime_skills.total);
        assert!(expected_runtime_skills.validated >= baseline_runtime_skills.validated);
        assert!(expected_learning.candidate_promoted >= baseline_learning.candidate_promoted);
        assert!(expected_learning.candidate_observed >= baseline_learning.candidate_observed);
        assert!(expected_learning.candidate_rejected >= baseline_learning.candidate_rejected);
        assert_eq!(crystals.runtime_skill_total, expected_runtime_skills.total);
        assert_eq!(
            crystals.validated_runtime_skills,
            expected_runtime_skills.validated
        );
        assert_eq!(
            crystals.promoted_candidates,
            expected_learning.candidate_promoted
        );
        assert_eq!(
            crystals.pending_candidates,
            expected_learning.candidate_observed
        );
        assert_eq!(
            crystals.rejected_candidates,
            expected_learning.candidate_rejected
        );
        assert!(snapshot
            .programmable_reasoning
            .operator_summary
            .contains(&format!(
                "runtime_skills={} validated={}",
                crystals.runtime_skill_total, crystals.validated_runtime_skills
            )));
        assert!(snapshot
            .programmable_reasoning
            .operator_summary
            .contains(&format!(
                "pending_crystals={} promoted_crystals={} rejected_crystals={}",
                crystals.pending_candidates,
                crystals.promoted_candidates,
                crystals.rejected_candidates
            )));
    }

    fn reasoning_shape(tool_name: &str) -> ToolExecutionShape {
        ToolMetadata::task()
            .default_execution_shape(tool_name)
            .with_effect_class(ToolEffectClass::ReadOnly)
            .with_risk_level(ToolRiskLevel::Low)
            .with_approval_mode(ToolApprovalMode::Automatic)
            .with_rollback_kind(ToolRollbackKind::None)
    }

    fn reasoning_permit(tool_name: &str) -> ToolExecutionPermit {
        ToolExecutionPermit {
            tool_name: tool_name.to_string(),
            ingress: IngressKind::User,
            channel: "telegram".to_string(),
            metadata: ToolMetadata::task(),
            shape: reasoning_shape(tool_name),
            requires_network: false,
        }
    }

    fn non_reasoning_permit(tool_name: &str) -> ToolExecutionPermit {
        ToolExecutionPermit {
            tool_name: tool_name.to_string(),
            ingress: IngressKind::User,
            channel: "telegram".to_string(),
            metadata: ToolMetadata::task(),
            shape: ToolMetadata::task().default_execution_shape(tool_name),
            requires_network: false,
        }
    }
}
