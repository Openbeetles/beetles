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
use crate::reasoning::{
    programmable_reasoning_inspection_views, summarize_programmable_reasoning_operator,
};
use crate::runtime;
use crate::skills::{
    build_capability_atom_operator_summary, build_runtime_skill_doctrine_snapshot,
    build_runtime_skill_genome_snapshot, build_runtime_skill_operator_summary,
    capability_atom_lifecycle_event_at, list_capability_atom_records, list_runtime_skill_records,
    runtime_skill_doctrine_event_at, runtime_skill_genome_event_at, CapabilityAtomSourceKind,
    CapabilityAtomTrustLevel, RuntimeSkillRecord, RuntimeSkillStrategyDiffKind,
};
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
    pub mental_privacy_review_last_ms: u64,
    pub dispatch_send_fail_total: u64,
    pub outbound_enqueue_fail_total: u64,
    pub dominant_stage: &'static str,
}

impl ReplyPipelineOperatorSummary {
    fn from_metrics(metrics: &crate::metrics::MetricsSnapshot) -> Self {
        let finalization_total_ms = metrics.mental_privacy_review_last_ms;
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
            mental_privacy_review_last_ms: metrics.mental_privacy_review_last_ms,
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
    let programmable_reasoning_activity_records = collect_programmable_reasoning_activity_records(
        tool_governance_state.as_ref(),
        input.platform.session_store().as_ref(),
        input.platform.turn_ledger_store().as_ref(),
        input.platform.skill_storage().as_ref(),
    )?;
    let programmable_reasoning_usage =
        build_programmable_reasoning_usage_analytics(&programmable_reasoning_activity_records);
    let programmable_reasoning_timeline =
        build_programmable_reasoning_timeline(&programmable_reasoning_activity_records);
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
    let runtime_skill_doctrine =
        build_runtime_skill_doctrine_snapshot(input.platform.skill_storage().as_ref());
    let runtime_skill_genome =
        build_runtime_skill_genome_snapshot(input.platform.skill_storage().as_ref());
    let capability_atom_summary =
        build_capability_atom_operator_summary(input.platform.skill_storage().as_ref());
    let task_learning_snapshot =
        build_task_learning_operator_snapshot(input.platform.task_learning_store().as_ref())?;
    let mut programmable_reasoning = crate::programmable_reasoning_operator_snapshot(
        &runtime_skill_summary,
        &runtime_skill_doctrine,
        &runtime_skill_genome,
        &capability_atom_summary,
        Some(&task_learning_snapshot),
    );
    programmable_reasoning.usage_analytics = programmable_reasoning_usage;
    programmable_reasoning.timeline = programmable_reasoning_timeline;
    programmable_reasoning.maintenance_digest = build_programmable_reasoning_maintenance_digest(
        &programmable_reasoning.usage_analytics,
        &programmable_reasoning.timeline,
    );
    programmable_reasoning.replay = build_programmable_reasoning_replay_inspection(
        input.platform.session_store().as_ref(),
        input.platform.turn_ledger_store().as_ref(),
        input.platform.skill_storage().as_ref(),
    )?;
    programmable_reasoning.inspection = programmable_reasoning_inspection_views(
        &programmable_reasoning.doctrine,
        &programmable_reasoning.genome,
        &programmable_reasoning.adversarial_arena,
        &programmable_reasoning.maintenance_digest,
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

pub(crate) fn build_programmable_reasoning_system_info_summary(
    platform: &dyn Platform,
) -> crate::error::Result<crate::ProgrammableReasoningSystemInfoSummary> {
    let doctrine = build_runtime_skill_doctrine_snapshot(platform.skill_storage().as_ref());
    let genome = build_runtime_skill_genome_snapshot(platform.skill_storage().as_ref());
    let capability_atoms =
        build_capability_atom_operator_summary(platform.skill_storage().as_ref());
    let replay = build_programmable_reasoning_replay_inspection(
        platform.session_store().as_ref(),
        platform.turn_ledger_store().as_ref(),
        platform.skill_storage().as_ref(),
    )?;
    Ok(crate::programmable_reasoning_system_info_summary(
        &doctrine,
        &genome,
        &capability_atoms,
        &replay,
    ))
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
        "  reply_pipeline_dominant_stage: {}\n  reply_pipeline_request_semantics_last_ms: {}\n  reply_pipeline_tool_exec_last_ms: {}\n  reply_pipeline_mental_privacy_review_last_ms: {}\n  reply_pipeline_dispatch_send_fail_total: {}\n  reply_pipeline_outbound_enqueue_fail_total: {}\n",
        snapshot.reply_pipeline.dominant_stage,
        snapshot.reply_pipeline.request_semantics_last_ms,
        snapshot.reply_pipeline.tool_exec_last_ms,
        snapshot.reply_pipeline.mental_privacy_review_last_ms,
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
        "  programmable_reasoning_stage: {}\n  programmable_reasoning_execution_enabled: {}\n  programmable_reasoning_backend: {}\n  programmable_reasoning_operator_summary: {}\n  programmable_reasoning_product_headline: {}\n  programmable_reasoning_demo_scenarios: {}\n  programmable_reasoning_doctrine_headline: {}\n  programmable_reasoning_genome_headline: {}\n  programmable_reasoning_tension_headline: {}\n  programmable_reasoning_recent_events: {}\n  programmable_reasoning_governance_holds: {}\n  programmable_reasoning_digest_status: {}\n  programmable_reasoning_branch_replays: {}\n  programmable_reasoning_arena_replays: {}\n  programmable_reasoning_doctrine_replays: {}\n  programmable_reasoning_genome_replays: {}\n  programmable_reasoning_capability_atom_replays: {}\n",
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
            crate::ProgrammableReasoningStage::IntentCompiler => "intent_compiler",
            crate::ProgrammableReasoningStage::CounterfactualSandbox => {
                "counterfactual_sandbox"
            }
            crate::ProgrammableReasoningStage::AdversarialArena => "adversarial_arena",
            crate::ProgrammableReasoningStage::DoctrineGenomeEvolution => {
                "doctrine_genome_evolution"
            }
            crate::ProgrammableReasoningStage::CapabilityAtomsExchange => {
                "capability_atoms_exchange"
            }
        },
        snapshot.programmable_reasoning.runtime_contract.execution_enabled,
        match snapshot.programmable_reasoning.runtime_contract.execution_backend {
            crate::ProgrammableReasoningExecutionBackend::None => "none",
            crate::ProgrammableReasoningExecutionBackend::LuaSandbox => "lua_sandbox",
        },
        snapshot.programmable_reasoning.operator_summary,
        snapshot.programmable_reasoning.product_surface.headline,
        snapshot
            .programmable_reasoning
            .product_surface
            .demo_scenarios
            .len(),
        snapshot.programmable_reasoning.inspection.doctrine.headline,
        snapshot.programmable_reasoning.inspection.genome.headline,
        snapshot.programmable_reasoning.inspection.tension.headline,
        snapshot.programmable_reasoning.timeline.recent_events.len(),
        snapshot
            .programmable_reasoning
            .usage_analytics
            .recent_governance_holds,
        snapshot.programmable_reasoning.maintenance_digest.status,
        snapshot.programmable_reasoning.replay.recent_branch_replays.len(),
        snapshot.programmable_reasoning.replay.recent_arena_replays.len(),
        snapshot
            .programmable_reasoning
            .replay
            .recent_doctrine_replays
            .len(),
        snapshot
            .programmable_reasoning
            .replay
            .recent_genome_replays
            .len(),
        snapshot
            .programmable_reasoning
            .replay
            .recent_capability_atom_replays
            .len(),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgrammableReasoningActivityKind {
    Tool,
    TurnStage,
    AssetLifecycle,
}

impl ProgrammableReasoningActivityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::TurnStage => "turn_stage",
            Self::AssetLifecycle => "asset_lifecycle",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProgrammableReasoningActivityRecord {
    recorded_at: u64,
    activity_kind: ProgrammableReasoningActivityKind,
    activity_name: String,
    tool_name: Option<String>,
    status: String,
    detail: String,
    attention_required: bool,
    bucket: ProgrammableReasoningRecordBucket,
    same_timestamp_order: u32,
}

fn collect_programmable_reasoning_activity_records(
    governance: Option<&ToolExecutionGovernanceState>,
    session_store: &dyn crate::memory::SessionStore,
    turn_ledger_store: &dyn crate::memory::TurnLedgerStore,
    skill_storage: &dyn crate::platform::SkillStorage,
) -> crate::error::Result<Vec<ProgrammableReasoningActivityRecord>> {
    let mut records = Vec::new();
    if let Some(governance) = governance {
        for (index, record) in governance.recent_records.iter().enumerate() {
            if let Some(mut activity) =
                programmable_reasoning_activity_record_from_tool_record(record)
            {
                activity.same_timestamp_order = index as u32 + 1;
                records.push(activity);
            }
        }
    }

    for (ledger_index, (_chat_id, ledger)) in
        collect_recent_programmable_reasoning_turn_ledgers(session_store, turn_ledger_store)?
            .into_iter()
            .enumerate()
    {
        records.extend(programmable_reasoning_activity_records_from_turn_ledger(
            &ledger,
            (ledger_index as u32) * 10,
        ));
    }

    records.extend(collect_programmable_reasoning_asset_activity_records(
        skill_storage,
        10_000,
    ));

    records.sort_by(|left, right| {
        right
            .recorded_at
            .cmp(&left.recorded_at)
            .then_with(|| right.same_timestamp_order.cmp(&left.same_timestamp_order))
            .then_with(|| left.activity_name.cmp(&right.activity_name))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    Ok(records)
}

fn collect_programmable_reasoning_asset_activity_records(
    skill_storage: &dyn crate::platform::SkillStorage,
    base_order: u32,
) -> Vec<ProgrammableReasoningActivityRecord> {
    let mut records = Vec::new();
    let mut next_order = base_order;

    for record in list_runtime_skill_records(skill_storage) {
        if let Some(activity) = programmable_reasoning_doctrine_activity_record(&record, next_order)
        {
            records.push(activity);
            next_order = next_order.saturating_add(1);
        }
        if let Some(activity) = programmable_reasoning_genome_activity_record(&record, next_order) {
            records.push(activity);
            next_order = next_order.saturating_add(1);
        }
    }

    for record in list_capability_atom_records(skill_storage) {
        if let Some(activity) =
            programmable_reasoning_capability_atom_activity_record(&record, next_order)
        {
            records.push(activity);
            next_order = next_order.saturating_add(1);
        }
    }

    records
}

fn build_programmable_reasoning_usage_analytics(
    activity_records: &[ProgrammableReasoningActivityRecord],
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

    #[derive(Default)]
    struct StageUsageAccumulator {
        total_events: usize,
        last_seen_at: Option<u64>,
    }

    if activity_records.is_empty() {
        return crate::ProgrammableReasoningUsageAnalytics::default();
    }

    let mut usage = crate::ProgrammableReasoningUsageAnalytics {
        last_event_name: Some(activity_records[0].activity_name.clone()),
        last_seen_at: Some(activity_records[0].recorded_at),
        ..crate::ProgrammableReasoningUsageAnalytics::default()
    };
    let mut tool_counts: BTreeMap<String, ToolUsageAccumulator> = BTreeMap::new();
    let mut stage_counts: BTreeMap<String, StageUsageAccumulator> = BTreeMap::new();
    let mut last_tool_event: Option<(&str, u64, u32)> = None;
    for record in activity_records {
        let status_bucket = record.bucket;
        usage.recent_total_events += 1;
        if record.attention_required {
            usage.recent_attention_events += 1;
        }
        usage.last_seen_at = Some(usage.last_seen_at.map_or(record.recorded_at, |current| {
            current.max(record.recorded_at)
        }));
        match record.activity_kind {
            ProgrammableReasoningActivityKind::Tool => {
                usage.recent_total_attempts += 1;
                if let Some(tool_name) = record.tool_name.as_deref() {
                    let entry = tool_counts.entry(tool_name.to_string()).or_default();
                    entry.total_attempts += 1;
                    entry.last_seen_at =
                        Some(entry.last_seen_at.map_or(record.recorded_at, |current| {
                            current.max(record.recorded_at)
                        }));
                    let should_replace_last_tool = last_tool_event
                        .map(|(_, current_at, current_order)| {
                            record.recorded_at > current_at
                                || (record.recorded_at == current_at
                                    && record.same_timestamp_order >= current_order)
                        })
                        .unwrap_or(true);
                    if should_replace_last_tool {
                        last_tool_event =
                            Some((tool_name, record.recorded_at, record.same_timestamp_order));
                    }
                }
                match status_bucket {
                    ProgrammableReasoningRecordBucket::Succeeded => {
                        usage.recent_succeeded += 1;
                        if let Some(tool_name) = record.tool_name.as_deref() {
                            if let Some(entry) = tool_counts.get_mut(tool_name) {
                                entry.succeeded += 1;
                            }
                        }
                    }
                    ProgrammableReasoningRecordBucket::GovernanceHold => {
                        usage.recent_governance_holds += 1;
                    }
                    ProgrammableReasoningRecordBucket::Failed => {
                        usage.recent_failed += 1;
                        if let Some(tool_name) = record.tool_name.as_deref() {
                            if let Some(entry) = tool_counts.get_mut(tool_name) {
                                entry.failed += 1;
                            }
                        }
                    }
                    ProgrammableReasoningRecordBucket::Denied => {
                        usage.recent_denied += 1;
                        if let Some(tool_name) = record.tool_name.as_deref() {
                            if let Some(entry) = tool_counts.get_mut(tool_name) {
                                entry.denied += 1;
                            }
                        }
                    }
                    ProgrammableReasoningRecordBucket::ResourceDenied => {
                        usage.recent_resource_denied += 1;
                        if let Some(tool_name) = record.tool_name.as_deref() {
                            if let Some(entry) = tool_counts.get_mut(tool_name) {
                                entry.resource_denied += 1;
                            }
                        }
                    }
                }
            }
            ProgrammableReasoningActivityKind::TurnStage
            | ProgrammableReasoningActivityKind::AssetLifecycle => {
                if matches!(
                    status_bucket,
                    ProgrammableReasoningRecordBucket::GovernanceHold
                ) {
                    usage.recent_governance_holds += 1;
                }
                let entry = stage_counts
                    .entry(record.activity_name.clone())
                    .or_default();
                entry.total_events += 1;
                entry.last_seen_at =
                    Some(entry.last_seen_at.map_or(record.recorded_at, |current| {
                        current.max(record.recorded_at)
                    }));
            }
        }
    }

    if let Some((tool_name, _, _)) = last_tool_event {
        usage.last_tool_name = Some(tool_name.to_string());
    }

    let mut stage_counts = stage_counts
        .into_iter()
        .map(
            |(stage_name, entry)| crate::ProgrammableReasoningStageUsageSummary {
                stage_name,
                total_events: entry.total_events,
                last_seen_at: entry.last_seen_at,
            },
        )
        .collect::<Vec<_>>();
    stage_counts.sort_by(|left, right| {
        right
            .total_events
            .cmp(&left.total_events)
            .then_with(|| right.last_seen_at.cmp(&left.last_seen_at))
            .then_with(|| left.stage_name.cmp(&right.stage_name))
    });
    usage.stage_counts = stage_counts;

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
    activity_records: &[ProgrammableReasoningActivityRecord],
) -> crate::ProgrammableReasoningTimeline {
    const PROGRAMMABLE_REASONING_TIMELINE_LIMIT: usize = 8;

    if activity_records.is_empty() {
        return crate::ProgrammableReasoningTimeline::default();
    }

    let recent_events = activity_records
        .iter()
        .map(programmable_reasoning_timeline_event_from_record)
        .take(PROGRAMMABLE_REASONING_TIMELINE_LIMIT)
        .collect::<Vec<_>>();

    crate::ProgrammableReasoningTimeline { recent_events }
}

fn build_programmable_reasoning_maintenance_digest(
    usage: &crate::ProgrammableReasoningUsageAnalytics,
    timeline: &crate::ProgrammableReasoningTimeline,
) -> crate::ProgrammableReasoningMaintenanceDigest {
    let attention_event_count = usage.recent_attention_events;
    let last_event = timeline.recent_events.first();
    let attention_activities = timeline
        .recent_events
        .iter()
        .filter(|event| event.attention_required)
        .map(|event| event.activity_name.clone())
        .fold(Vec::<String>::new(), |mut acc, activity_name| {
            if !acc.iter().any(|item| item == &activity_name) {
                acc.push(activity_name);
            }
            acc
        });
    let attention_tools = timeline
        .recent_events
        .iter()
        .filter(|event| event.attention_required)
        .filter_map(|event| event.tool_name.clone())
        .fold(Vec::<String>::new(), |mut acc, tool_name| {
            if !acc.iter().any(|item| item == &tool_name) {
                acc.push(tool_name);
            }
            acc
        });
    let status = if usage.recent_total_events == 0 {
        "idle"
    } else if attention_event_count > 0 {
        "attention"
    } else {
        "healthy"
    };
    let headline = match status {
        "idle" => "no recent programmable reasoning activity".to_string(),
        "attention" => format!(
            "{} recent programmable reasoning events, {} need attention",
            usage.recent_total_events, attention_event_count
        ),
        _ => format!(
            "{} recent programmable reasoning events, no operator intervention needed",
            usage.recent_total_events
        ),
    };
    crate::ProgrammableReasoningMaintenanceDigest {
        status: status.to_string(),
        headline,
        attention_event_count,
        last_event_kind: last_event.map(|event| event.activity_kind.clone()),
        last_event_name: last_event.map(|event| event.activity_name.clone()),
        last_event_tool_name: last_event.and_then(|event| event.tool_name.clone()),
        last_event_status: last_event.map(|event| event.status.clone()),
        attention_activities,
        attention_tools,
    }
}

const PROGRAMMABLE_REASONING_LEDGER_SCAN_LIMIT_PER_CHAT: usize = 4;
const PROGRAMMABLE_REASONING_BRANCH_REPLAY_LIMIT: usize = 6;
const PROGRAMMABLE_REASONING_ARENA_REPLAY_LIMIT: usize = 6;
const PROGRAMMABLE_REASONING_DOCTRINE_REPLAY_LIMIT: usize = 6;
const PROGRAMMABLE_REASONING_GENOME_REPLAY_LIMIT: usize = 6;
const PROGRAMMABLE_REASONING_CAPABILITY_ATOM_REPLAY_LIMIT: usize = 6;

fn collect_recent_programmable_reasoning_turn_ledgers(
    session_store: &dyn crate::memory::SessionStore,
    turn_ledger_store: &dyn crate::memory::TurnLedgerStore,
) -> crate::error::Result<Vec<(String, crate::memory::TurnLedger)>> {
    let mut chat_ids = session_store.list_chat_ids()?;
    chat_ids.sort();
    chat_ids.dedup();

    let mut ledgers = Vec::new();
    for chat_id in chat_ids {
        for ledger in turn_ledger_store
            .list_recent(&chat_id, PROGRAMMABLE_REASONING_LEDGER_SCAN_LIMIT_PER_CHAT)?
        {
            ledgers.push((chat_id.clone(), ledger));
        }
    }
    Ok(ledgers)
}

fn build_programmable_reasoning_replay_inspection(
    session_store: &dyn crate::memory::SessionStore,
    turn_ledger_store: &dyn crate::memory::TurnLedgerStore,
    skill_storage: &dyn crate::platform::SkillStorage,
) -> crate::error::Result<crate::ProgrammableReasoningReplayInspection> {
    let mut branch_replays = Vec::new();
    let mut arena_replays = Vec::new();
    let mut doctrine_replays = collect_programmable_reasoning_doctrine_replays(skill_storage);
    let mut genome_replays = collect_programmable_reasoning_genome_replays(skill_storage);
    let mut capability_atom_replays =
        collect_programmable_reasoning_capability_atom_replays(skill_storage);

    for (chat_id, ledger) in
        collect_recent_programmable_reasoning_turn_ledgers(session_store, turn_ledger_store)?
    {
        if let Some(record) = branch_replay_record_from_ledger(&chat_id, &ledger) {
            branch_replays.push(record);
        }
        if let Some(record) = arena_replay_record_from_ledger(&chat_id, &ledger) {
            arena_replays.push(record);
        }
    }

    branch_replays.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| left.chat_id.cmp(&right.chat_id))
            .then_with(|| left.selected_branch.cmp(&right.selected_branch))
    });
    arena_replays.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| left.chat_id.cmp(&right.chat_id))
            .then_with(|| left.winner.cmp(&right.winner))
    });

    let branch_replays_retained = branch_replays.len();
    let arena_replays_retained = arena_replays.len();
    doctrine_replays.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| left.source_skill_name.cmp(&right.source_skill_name))
    });
    genome_replays.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| left.skill_name.cmp(&right.skill_name))
    });
    capability_atom_replays.sort_by(|left, right| {
        right
            .recorded_at_ms
            .cmp(&left.recorded_at_ms)
            .then_with(|| left.atom_name.cmp(&right.atom_name))
    });

    let doctrine_replays_retained = doctrine_replays.len();
    let genome_replays_retained = genome_replays.len();
    let capability_atom_replays_retained = capability_atom_replays.len();
    branch_replays.truncate(PROGRAMMABLE_REASONING_BRANCH_REPLAY_LIMIT);
    arena_replays.truncate(PROGRAMMABLE_REASONING_ARENA_REPLAY_LIMIT);
    doctrine_replays.truncate(PROGRAMMABLE_REASONING_DOCTRINE_REPLAY_LIMIT);
    genome_replays.truncate(PROGRAMMABLE_REASONING_GENOME_REPLAY_LIMIT);
    capability_atom_replays.truncate(PROGRAMMABLE_REASONING_CAPABILITY_ATOM_REPLAY_LIMIT);

    Ok(crate::ProgrammableReasoningReplayInspection {
        branch_replays_retained,
        recent_branch_replays: branch_replays,
        arena_replays_retained,
        recent_arena_replays: arena_replays,
        doctrine_replays_retained,
        recent_doctrine_replays: doctrine_replays,
        genome_replays_retained,
        recent_genome_replays: genome_replays,
        capability_atom_replays_retained,
        recent_capability_atom_replays: capability_atom_replays,
    })
}

fn collect_programmable_reasoning_doctrine_replays(
    skill_storage: &dyn crate::platform::SkillStorage,
) -> Vec<crate::ProgrammableReasoningDoctrineReplayRecord> {
    list_runtime_skill_records(skill_storage)
        .into_iter()
        .filter(|record| record.validated_success_count > 0 || record.revision_pending)
        .filter_map(|record| {
            Some(crate::ProgrammableReasoningDoctrineReplayRecord {
                recorded_at_ms: runtime_skill_doctrine_event_at(&record)?.saturating_mul(1000),
                source_chat_id: record.source_chat_id.clone(),
                source_skill_name: record.name.clone(),
                topic: record.topic.clone(),
                status: if record.revision_pending {
                    "revision_pending".to_string()
                } else {
                    "stable".to_string()
                },
                validated_success_count: record.validated_success_count,
                summary: summarize_runtime_skill_doctrine_replay(&record),
            })
        })
        .collect()
}

fn collect_programmable_reasoning_genome_replays(
    skill_storage: &dyn crate::platform::SkillStorage,
) -> Vec<crate::ProgrammableReasoningGenomeReplayRecord> {
    list_runtime_skill_records(skill_storage)
        .into_iter()
        .filter(|record| !record.strategy_diffs.is_empty() || record.retired_at.is_some())
        .filter_map(|record| {
            Some(crate::ProgrammableReasoningGenomeReplayRecord {
                recorded_at_ms: runtime_skill_genome_event_at(&record)?.saturating_mul(1000),
                source_chat_id: record.source_chat_id.clone(),
                skill_name: record.name.clone(),
                topic: record.topic.clone(),
                status: runtime_skill_genome_replay_status(&record),
                lineage_depth: record.genome_lineage.len().max(1),
                diff_events: record.strategy_diffs.len(),
                active_node_id: record
                    .genome_lineage
                    .last()
                    .map(|node| node.node_id.clone()),
                summary: summarize_runtime_skill_genome_replay(&record),
            })
        })
        .collect()
}

fn collect_programmable_reasoning_capability_atom_replays(
    skill_storage: &dyn crate::platform::SkillStorage,
) -> Vec<crate::ProgrammableReasoningCapabilityAtomReplayRecord> {
    list_capability_atom_records(skill_storage)
        .into_iter()
        .filter_map(|record| {
            Some(crate::ProgrammableReasoningCapabilityAtomReplayRecord {
                recorded_at_ms: capability_atom_lifecycle_event_at(&record)?.saturating_mul(1000),
                source_chat_id: record.provenance.source_chat_id.clone(),
                atom_name: record.name.clone(),
                topic: record.topic.clone(),
                trust: capability_atom_trust_label(record.trust).to_string(),
                source_kind: capability_atom_source_kind_label(record.provenance.source_kind)
                    .to_string(),
                status: capability_atom_replay_status(&record),
                summary: summarize_capability_atom_replay(&record),
            })
        })
        .collect()
}

fn programmable_reasoning_doctrine_activity_record(
    record: &RuntimeSkillRecord,
    same_timestamp_order: u32,
) -> Option<ProgrammableReasoningActivityRecord> {
    if record.validated_success_count == 0 && !record.revision_pending {
        return None;
    }
    Some(ProgrammableReasoningActivityRecord {
        recorded_at: runtime_skill_doctrine_event_at(record)?.saturating_mul(1000),
        activity_kind: ProgrammableReasoningActivityKind::AssetLifecycle,
        activity_name: "doctrine_genome_evolution".to_string(),
        tool_name: None,
        status: if record.revision_pending {
            "revision_pending".to_string()
        } else {
            "stable".to_string()
        },
        detail: summarize_runtime_skill_doctrine_replay(record),
        attention_required: record.revision_pending,
        bucket: if record.revision_pending {
            ProgrammableReasoningRecordBucket::GovernanceHold
        } else {
            ProgrammableReasoningRecordBucket::Succeeded
        },
        same_timestamp_order,
    })
}

fn programmable_reasoning_genome_activity_record(
    record: &RuntimeSkillRecord,
    same_timestamp_order: u32,
) -> Option<ProgrammableReasoningActivityRecord> {
    if record.strategy_diffs.is_empty() && record.retired_at.is_none() {
        return None;
    }
    Some(ProgrammableReasoningActivityRecord {
        recorded_at: runtime_skill_genome_event_at(record)?.saturating_mul(1000),
        activity_kind: ProgrammableReasoningActivityKind::AssetLifecycle,
        activity_name: "doctrine_genome_evolution".to_string(),
        tool_name: None,
        status: runtime_skill_genome_replay_status(record),
        detail: summarize_runtime_skill_genome_replay(record),
        attention_required: false,
        bucket: ProgrammableReasoningRecordBucket::Succeeded,
        same_timestamp_order,
    })
}

fn programmable_reasoning_capability_atom_activity_record(
    record: &crate::skills::CapabilityAtomRecord,
    same_timestamp_order: u32,
) -> Option<ProgrammableReasoningActivityRecord> {
    let attention_required = record.provenance.requires_local_adjudication
        || matches!(
            record.trust,
            CapabilityAtomTrustLevel::ImportedPendingAdjudication
        );
    Some(ProgrammableReasoningActivityRecord {
        recorded_at: capability_atom_lifecycle_event_at(record)?.saturating_mul(1000),
        activity_kind: ProgrammableReasoningActivityKind::AssetLifecycle,
        activity_name: "capability_atoms_exchange".to_string(),
        tool_name: None,
        status: capability_atom_replay_status(record),
        detail: summarize_capability_atom_replay(record),
        attention_required,
        bucket: if attention_required {
            ProgrammableReasoningRecordBucket::GovernanceHold
        } else {
            ProgrammableReasoningRecordBucket::Succeeded
        },
        same_timestamp_order,
    })
}

fn branch_replay_record_from_ledger(
    chat_id: &str,
    ledger: &crate::memory::TurnLedger,
) -> Option<crate::ProgrammableReasoningBranchReplayRecord> {
    let counterfactual = ledger.counterfactual.as_ref()?;
    if !counterfactual.is_meaningful() {
        return None;
    }
    let rejected_branches = counterfactual
        .alternatives
        .iter()
        .filter(|branch| branch.is_meaningful())
        .filter_map(|branch| {
            let label = branch.branch.trim();
            (!label.is_empty()).then(|| label.to_string())
        })
        .collect::<Vec<_>>();
    Some(crate::ProgrammableReasoningBranchReplayRecord {
        recorded_at_ms: crate::memory::turn_ledger_observed_at_ms(ledger),
        channel: ledger.channel.clone(),
        chat_id: chat_id.to_string(),
        user_preview: ledger.user_preview.clone(),
        reasoning_strategy: counterfactual
            .snapshot
            .reasoning_strategy
            .trim()
            .to_string(),
        selected_branch: counterfactual.selected_branch.branch.trim().to_string(),
        selected_branch_score: counterfactual.selected_branch.score,
        rejected_branches,
        outcome: ledger.status.label().to_string(),
        summary: counterfactual.summary.trim().to_string(),
    })
}

fn arena_replay_record_from_ledger(
    chat_id: &str,
    ledger: &crate::memory::TurnLedger,
) -> Option<crate::ProgrammableReasoningArenaReplayRecord> {
    let arena = ledger.adversarial_arena.as_ref()?;
    if !arena.is_meaningful() {
        return None;
    }
    Some(crate::ProgrammableReasoningArenaReplayRecord {
        recorded_at_ms: crate::memory::turn_ledger_observed_at_ms(ledger),
        channel: ledger.channel.clone(),
        chat_id: chat_id.to_string(),
        user_preview: ledger.user_preview.clone(),
        subject_kind: arena.subject_kind.trim().to_string(),
        disposition: arena.disposition.trim().to_string(),
        winner: arena.winner.label.trim().to_string(),
        attacker: arena.attacker.label.trim().to_string(),
        defender: arena.defender.label.trim().to_string(),
        summary: arena.summary.trim().to_string(),
    })
}

fn summarize_runtime_skill_doctrine_replay(record: &RuntimeSkillRecord) -> String {
    if record.revision_pending {
        format!(
            "{} now needs doctrine revision review after the latest governed update.",
            record.title
        )
    } else {
        format!(
            "{} stabilized with {} validated successes.",
            record.title, record.validated_success_count
        )
    }
}

fn runtime_skill_strategy_diff_kind_label(kind: RuntimeSkillStrategyDiffKind) -> &'static str {
    match kind {
        RuntimeSkillStrategyDiffKind::SummaryRevision => "summary_revision",
        RuntimeSkillStrategyDiffKind::ProcedureRefinement => "procedure_refinement",
        RuntimeSkillStrategyDiffKind::DoctrineRevision => "doctrine_revision",
    }
}

fn runtime_skill_genome_replay_status(record: &RuntimeSkillRecord) -> String {
    if record.retired_at.is_some() {
        "retired".to_string()
    } else if let Some(diff) = record.strategy_diffs.last() {
        runtime_skill_strategy_diff_kind_label(diff.change_kind).to_string()
    } else {
        "lineage_recorded".to_string()
    }
}

fn summarize_runtime_skill_genome_replay(record: &RuntimeSkillRecord) -> String {
    if let Some(diff) = record.strategy_diffs.last() {
        diff.summary.trim().to_string()
    } else if !record.retirement_reason.trim().is_empty() {
        record.retirement_reason.trim().to_string()
    } else {
        format!(
            "{} lineage depth={} diff_events={}",
            record.title,
            record.genome_lineage.len().max(1),
            record.strategy_diffs.len()
        )
    }
}

fn capability_atom_trust_label(trust: CapabilityAtomTrustLevel) -> &'static str {
    match trust {
        CapabilityAtomTrustLevel::LocalVerified => "local_verified",
        CapabilityAtomTrustLevel::ImportedPendingAdjudication => "imported_pending_adjudication",
        CapabilityAtomTrustLevel::ImportedAdopted => "imported_adopted",
    }
}

fn capability_atom_source_kind_label(kind: CapabilityAtomSourceKind) -> &'static str {
    match kind {
        CapabilityAtomSourceKind::RuntimeSkill => "runtime_skill",
        CapabilityAtomSourceKind::ImportedAtom => "imported_atom",
    }
}

fn capability_atom_replay_status(record: &crate::skills::CapabilityAtomRecord) -> String {
    match record.trust {
        CapabilityAtomTrustLevel::LocalVerified => "local_verified".to_string(),
        CapabilityAtomTrustLevel::ImportedPendingAdjudication => {
            "pending_local_adjudication".to_string()
        }
        CapabilityAtomTrustLevel::ImportedAdopted => "imported_adopted".to_string(),
    }
}

fn summarize_capability_atom_replay(record: &crate::skills::CapabilityAtomRecord) -> String {
    match record.trust {
        CapabilityAtomTrustLevel::LocalVerified => format!(
            "{} promoted from {} into the local verified atom set.",
            record.title, record.provenance.source_name
        ),
        CapabilityAtomTrustLevel::ImportedPendingAdjudication => format!(
            "{} imported from {} and now waits for local adjudication.",
            record.title, record.provenance.source_name
        ),
        CapabilityAtomTrustLevel::ImportedAdopted => format!(
            "{} adopted the imported lineage from {} after local validation.",
            record.title, record.provenance.source_name
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProgrammableReasoningRecordBucket {
    Succeeded,
    GovernanceHold,
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
    record: &ProgrammableReasoningActivityRecord,
) -> crate::ProgrammableReasoningTimelineEvent {
    crate::ProgrammableReasoningTimelineEvent {
        recorded_at: record.recorded_at,
        activity_kind: record.activity_kind.as_str().to_string(),
        activity_name: record.activity_name.clone(),
        tool_name: record.tool_name.clone(),
        status: record.status.clone(),
        detail: record.detail.clone(),
        attention_required: record.attention_required,
    }
}

fn programmable_reasoning_activity_record_from_tool_record(
    record: &crate::tools::ToolExecutionRecord,
) -> Option<ProgrammableReasoningActivityRecord> {
    if !crate::tools::is_programmable_reasoning_tool_name(record.tool_name.as_str()) {
        return None;
    }
    let status = match record.status {
        crate::tools::ToolExecutionRecordStatus::Succeeded => "succeeded",
        crate::tools::ToolExecutionRecordStatus::Failed => "failed",
        crate::tools::ToolExecutionRecordStatus::Denied => "denied",
        crate::tools::ToolExecutionRecordStatus::ResourceDenied => "resource_denied",
        crate::tools::ToolExecutionRecordStatus::Allowed => return None,
    };
    Some(ProgrammableReasoningActivityRecord {
        recorded_at: record.recorded_at,
        activity_kind: ProgrammableReasoningActivityKind::Tool,
        activity_name: record.tool_name.clone(),
        tool_name: Some(record.tool_name.clone()),
        status: status.to_string(),
        detail: if record.summary.trim().is_empty() {
            record.reason.trim()
        } else {
            record.summary.trim()
        }
        .to_string(),
        attention_required: !matches!(
            record.status,
            crate::tools::ToolExecutionRecordStatus::Succeeded
        ),
        bucket: classify_programmable_reasoning_record(record.status)?,
        same_timestamp_order: 0,
    })
}

fn programmable_reasoning_activity_records_from_turn_ledger(
    ledger: &crate::memory::TurnLedger,
    base_order: u32,
) -> Vec<ProgrammableReasoningActivityRecord> {
    let recorded_at = crate::memory::turn_ledger_observed_at_ms(ledger);
    let mut records = Vec::new();

    if let Some(intent) = ledger
        .reasoning_intent
        .as_ref()
        .filter(|intent| intent.is_meaningful())
    {
        records.push(ProgrammableReasoningActivityRecord {
            recorded_at,
            activity_kind: ProgrammableReasoningActivityKind::TurnStage,
            activity_name: "intent_compiler".to_string(),
            tool_name: None,
            status: "succeeded".to_string(),
            detail: summarize_reasoning_intent_activity(intent),
            attention_required: false,
            bucket: ProgrammableReasoningRecordBucket::Succeeded,
            same_timestamp_order: base_order + 1,
        });
    }

    if let Some(counterfactual) = ledger
        .counterfactual
        .as_ref()
        .filter(|counterfactual| counterfactual.is_meaningful())
    {
        records.push(ProgrammableReasoningActivityRecord {
            recorded_at,
            activity_kind: ProgrammableReasoningActivityKind::TurnStage,
            activity_name: "counterfactual_sandbox".to_string(),
            tool_name: None,
            status: "succeeded".to_string(),
            detail: summarize_counterfactual_activity(counterfactual),
            attention_required: false,
            bucket: ProgrammableReasoningRecordBucket::Succeeded,
            same_timestamp_order: base_order + 2,
        });
    }

    if let Some(arena) = ledger
        .adversarial_arena
        .as_ref()
        .filter(|arena| arena.is_meaningful())
    {
        records.push(ProgrammableReasoningActivityRecord {
            recorded_at,
            activity_kind: ProgrammableReasoningActivityKind::TurnStage,
            activity_name: "adversarial_arena".to_string(),
            tool_name: None,
            status: "succeeded".to_string(),
            detail: summarize_adversarial_arena_activity(arena),
            attention_required: false,
            bucket: ProgrammableReasoningRecordBucket::Succeeded,
            same_timestamp_order: base_order + 3,
        });
    }

    records
}

fn summarize_reasoning_intent_activity(
    intent: &crate::memory::TurnReasoningIntentLedger,
) -> String {
    let kind = intent.kind.trim();
    let strategy = intent.strategy.trim();
    if !kind.is_empty() && !strategy.is_empty() {
        format!("{kind} via {strategy}")
    } else if !intent.summary.trim().is_empty() {
        intent.summary.trim().to_string()
    } else if !kind.is_empty() {
        format!("compiled {kind}")
    } else {
        "compiled governed reasoning intent".to_string()
    }
}

fn summarize_counterfactual_activity(
    counterfactual: &crate::memory::TurnCounterfactualLedger,
) -> String {
    let selected = counterfactual.selected_branch.branch.trim();
    let rejected = counterfactual
        .alternatives
        .iter()
        .find_map(|branch| {
            let label = branch.branch.trim();
            (!label.is_empty()).then_some(label)
        })
        .unwrap_or("");
    if !selected.is_empty() && !rejected.is_empty() {
        format!("selected {selected} over {rejected}")
    } else if !selected.is_empty() {
        format!("selected {selected}")
    } else if !counterfactual.summary.trim().is_empty() {
        counterfactual.summary.trim().to_string()
    } else {
        "compared counterfactual branches".to_string()
    }
}

fn summarize_adversarial_arena_activity(
    arena: &crate::memory::TurnAdversarialArenaLedger,
) -> String {
    let winner = arena.winner.label.trim();
    let disposition = arena.disposition.trim();
    if !winner.is_empty() && !disposition.is_empty() {
        format!("{winner} won with {disposition} disposition")
    } else if !arena.summary.trim().is_empty() {
        arena.summary.trim().to_string()
    } else {
        "adjudicated programmable reasoning arena".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{IngressKind, PcMsg};
    use crate::config::AppConfig;
    use crate::error::Error;
    use crate::memory::{SessionMessage, SessionStore, TurnLedger, TurnLedgerStore};
    use crate::platform::SkillStorage;
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

    #[derive(Default)]
    struct TestSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for TestSkillStorage {
        fn list_names(&self) -> crate::Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> crate::Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> crate::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> crate::Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestSessionStore {
        sessions: Mutex<HashMap<String, Vec<SessionMessage>>>,
    }

    impl SessionStore for TestSessionStore {
        fn append(&self, chat_id: &str, role: &str, content: &str) -> crate::Result<()> {
            self.sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .entry(chat_id.to_string())
                .or_default()
                .push(SessionMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                });
            Ok(())
        }

        fn load_recent(&self, chat_id: &str, n: usize) -> crate::Result<Vec<SessionMessage>> {
            let sessions = self
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let Some(messages) = sessions.get(chat_id) else {
                return Ok(Vec::new());
            };
            let keep_from = messages.len().saturating_sub(n);
            Ok(messages[keep_from..].to_vec())
        }

        fn clear(&self, chat_id: &str) -> crate::Result<()> {
            self.sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(chat_id);
            Ok(())
        }

        fn list_chat_ids(&self) -> crate::Result<Vec<String>> {
            Ok(self
                .sessions
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct TestTurnLedgerStore {
        ledgers: Mutex<HashMap<String, TurnLedger>>,
    }

    impl TurnLedgerStore for TestTurnLedgerStore {
        fn get(&self, chat_id: &str) -> crate::Result<Option<TurnLedger>> {
            Ok(self
                .ledgers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(chat_id)
                .cloned())
        }

        fn set(&self, chat_id: &str, ledger: &TurnLedger) -> crate::Result<()> {
            self.ledgers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(chat_id.to_string(), ledger.clone());
            Ok(())
        }

        fn clear(&self, chat_id: &str) -> crate::Result<()> {
            self.ledgers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(chat_id);
            Ok(())
        }
    }

    #[test]
    fn build_operator_status_includes_device_capability_planes() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        let config = AppConfig::load_from_env();
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let runtime_services = crate::RuntimeServices::from_platform(Arc::clone(&platform));
        let (tool_registry, _) = crate::tools::build_default_registry(&config, &runtime_services);
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
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));

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

        let state = governance.inspect().expect("inspect governance");
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let activity_records = collect_programmable_reasoning_activity_records(
            Some(&state),
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("collect activity records");
        let usage = build_programmable_reasoning_usage_analytics(&activity_records);
        assert_eq!(usage.recent_total_events, 4);
        assert_eq!(usage.recent_total_attempts, 4);
        assert_eq!(usage.recent_attention_events, 3);
        assert_eq!(usage.recent_succeeded, 1);
        assert_eq!(usage.recent_failed, 1);
        assert_eq!(usage.recent_denied, 1);
        assert_eq!(usage.recent_resource_denied, 1);
        assert_eq!(
            usage.last_event_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert_eq!(
            usage.last_tool_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert!(usage.stage_counts.is_empty());
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
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));

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

        let state = governance.inspect().expect("inspect governance");
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let activity_records = collect_programmable_reasoning_activity_records(
            Some(&state),
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("collect activity records");
        let timeline = build_programmable_reasoning_timeline(&activity_records);
        assert_eq!(timeline.recent_events.len(), 4);
        assert_eq!(timeline.recent_events[0].activity_kind, "tool");
        assert_eq!(
            timeline.recent_events[0].activity_name,
            "lua_protocol_frame_helper"
        );
        assert_eq!(
            timeline.recent_events[0].tool_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert_eq!(timeline.recent_events[0].status, "denied");
        assert_eq!(timeline.recent_events[0].detail, "explicit_intent_required");
        assert!(timeline.recent_events[0].attention_required);
        assert_eq!(timeline.recent_events[1].activity_kind, "tool");
        assert_eq!(timeline.recent_events[1].activity_name, "lua_query");
        assert_eq!(
            timeline.recent_events[1].tool_name.as_deref(),
            Some("lua_query")
        );
        assert_eq!(timeline.recent_events[1].status, "resource_denied");
        assert_eq!(
            timeline.recent_events[1].detail,
            "runtime capability blocked"
        );
        assert!(timeline.recent_events[1].attention_required);
        assert_eq!(timeline.recent_events[2].activity_kind, "tool");
        assert_eq!(
            timeline.recent_events[2].activity_name,
            "lua_state_machine_checker"
        );
        assert_eq!(
            timeline.recent_events[2].tool_name.as_deref(),
            Some("lua_state_machine_checker")
        );
        assert_eq!(timeline.recent_events[2].status, "failed");
        assert_eq!(
            timeline.recent_events[2].detail,
            "config: transition missing (stage: lua_state_machine_checker_test)"
        );
        assert!(timeline.recent_events[2].attention_required);
        assert_eq!(timeline.recent_events[3].activity_kind, "tool");
        assert_eq!(
            timeline.recent_events[3].activity_name,
            "lua_register_table_helper"
        );
        assert_eq!(
            timeline.recent_events[3].tool_name.as_deref(),
            Some("lua_register_table_helper")
        );
        assert_eq!(timeline.recent_events[3].status, "succeeded");
        assert_eq!(timeline.recent_events[3].detail, "register table parsed");
        assert!(!timeline.recent_events[3].attention_required);
        assert!(!timeline
            .recent_events
            .iter()
            .any(|event| event.tool_name.as_deref() == Some("message")));
    }

    #[test]
    fn build_operator_status_exposes_programmable_reasoning_maintenance_digest() {
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));

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

        let state = governance.inspect().expect("inspect governance");
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let activity_records = collect_programmable_reasoning_activity_records(
            Some(&state),
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("collect activity records");
        let usage = build_programmable_reasoning_usage_analytics(&activity_records);
        let timeline = build_programmable_reasoning_timeline(&activity_records);
        let digest = build_programmable_reasoning_maintenance_digest(&usage, &timeline);
        assert_eq!(digest.status, "attention");
        assert_eq!(digest.last_event_kind.as_deref(), Some("tool"));
        assert_eq!(
            digest.last_event_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert_eq!(
            digest.last_event_tool_name.as_deref(),
            Some("lua_protocol_frame_helper")
        );
        assert_eq!(digest.last_event_status.as_deref(), Some("denied"));
        assert_eq!(digest.attention_event_count, 3);
        assert_eq!(
            digest.attention_activities,
            vec![
                "lua_protocol_frame_helper".to_string(),
                "lua_query".to_string(),
                "lua_state_machine_checker".to_string()
            ]
        );
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
            .contains("4 recent programmable reasoning events, 3 need attention"));
    }

    #[test]
    fn build_operator_status_counts_turn_stage_activity_without_tool_records() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        use crate::memory::{
            build_turn_ledger_start, TurnCounterfactualBranchLedger, TurnCounterfactualLedger,
            TurnCounterfactualSnapshotLedger, TurnLedgerStatus, TurnReasoningIntentLedger,
        };
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let chat_id = format!("reasoning-turn-activity-{unique}");

        session_store
            .append(
                &chat_id,
                "user",
                "decide whether to inspect memory or ask first",
            )
            .expect("write session");

        let mut ledger_msg = PcMsg::new_inbound(
            "qq_channel",
            &chat_id,
            "Decide whether to inspect memory or ask first",
            false,
        )
        .expect("turn activity message");
        ledger_msg.req_id = Some("req-turn-activity".to_string());
        let mut ledger = build_turn_ledger_start(&ledger_msg, 4_400_000_000_000);
        ledger.status = TurnLedgerStatus::Answered;
        ledger.updated_at_ms = 4_400_000_000_150;
        ledger.finished_at_ms = 4_400_000_000_150;
        ledger.reason = "compiled programmable reasoning turn stages".to_string();
        ledger.reasoning_intent = Some(TurnReasoningIntentLedger {
            kind: "memory_query".to_string(),
            strategy: "intent_compiler".to_string(),
            confidence: 84,
            summary: "Memory evidence is available and should be queried before replying."
                .to_string(),
            rationale: vec!["governed memory evidence present".to_string()],
            preferred_tools: vec!["memory_recall".to_string()],
            runtime_grounding_required: true,
        });
        ledger.counterfactual = Some(TurnCounterfactualLedger {
            summary: "Compared direct reply against memory query.".to_string(),
            snapshot: TurnCounterfactualSnapshotLedger {
                reasoning_kind: "intent_compiler".to_string(),
                reasoning_strategy: "branch_compare".to_string(),
                confidence: 84,
                runtime_grounding_required: true,
                governed_memory_evidence_present: true,
                ..TurnCounterfactualSnapshotLedger::default()
            },
            selected_branch: TurnCounterfactualBranchLedger {
                branch: "memory_query".to_string(),
                score: 89,
                summary: "Memory query preserves factual grounding.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            },
            alternatives: vec![TurnCounterfactualBranchLedger {
                branch: "direct_reply".to_string(),
                score: 41,
                summary: "Direct reply risks skipping governed memory evidence.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            }],
        });
        turn_ledger_store
            .set(&chat_id, &ledger)
            .expect("write turn ledger");

        let activity_records = collect_programmable_reasoning_activity_records(
            None,
            &session_store,
            &turn_ledger_store,
            &TestSkillStorage::default(),
        )
        .expect("collect activity records");
        let usage = build_programmable_reasoning_usage_analytics(&activity_records);
        assert_eq!(usage.recent_total_events, 2);
        assert_eq!(usage.recent_total_attempts, 0);
        assert_eq!(usage.recent_attention_events, 0);
        assert_eq!(usage.recent_succeeded, 0);
        assert_eq!(usage.recent_failed, 0);
        assert_eq!(usage.recent_denied, 0);
        assert_eq!(usage.recent_resource_denied, 0);
        assert_eq!(
            usage.last_event_name.as_deref(),
            Some("counterfactual_sandbox")
        );
        assert_eq!(usage.last_tool_name, None);
        assert!(usage.tool_counts.is_empty());
        assert!(usage
            .stage_counts
            .iter()
            .any(|entry| entry.stage_name == "intent_compiler" && entry.total_events == 1));
        assert!(usage.stage_counts.iter().any(|entry| {
            entry.stage_name == "counterfactual_sandbox" && entry.total_events == 1
        }));

        let timeline = build_programmable_reasoning_timeline(&activity_records);
        assert_eq!(timeline.recent_events.len(), 2);
        assert_eq!(timeline.recent_events[0].activity_kind, "turn_stage");
        assert_eq!(
            timeline.recent_events[0].activity_name,
            "counterfactual_sandbox"
        );
        assert_eq!(timeline.recent_events[0].tool_name, None);
        assert_eq!(timeline.recent_events[0].status, "succeeded");
        assert_eq!(
            timeline.recent_events[0].detail,
            "selected memory_query over direct_reply"
        );
        assert!(!timeline.recent_events[0].attention_required);
        assert_eq!(timeline.recent_events[1].activity_name, "intent_compiler");
        assert_eq!(timeline.recent_events[1].status, "succeeded");
        assert_eq!(
            timeline.recent_events[1].detail,
            "memory_query via intent_compiler"
        );

        let digest = build_programmable_reasoning_maintenance_digest(&usage, &timeline);
        assert_eq!(digest.status, "healthy");
        assert_eq!(digest.last_event_kind.as_deref(), Some("turn_stage"));
        assert_eq!(
            digest.last_event_name.as_deref(),
            Some("counterfactual_sandbox")
        );
        assert_eq!(digest.last_event_tool_name, None);
        assert_eq!(digest.last_event_status.as_deref(), Some("succeeded"));
        assert_eq!(digest.attention_event_count, 0);
        assert!(digest
            .headline
            .contains("2 recent programmable reasoning events, no operator intervention needed"));
    }

    #[test]
    fn build_operator_status_counts_asset_lifecycle_activity_in_main_observability_chain() {
        let skill_storage = TestSkillStorage::default();
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let topic = "serial_framing".to_string();
        let skill_name = crate::skills::runtime_skill_name_for_topic(&topic);

        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Serial framing".to_string(),
                summary: "Recover frame boundaries before decoding.".to_string(),
                content: "1. detect sync word\n2. validate length\n3. emit frame".to_string(),
                citations: vec!["transcript:chat-42#message=1".to_string()],
                source_chat_id: Some("chat-42".to_string()),
                observed_at: 4_500_000_000,
            },
        )
        .expect("write base runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            4_500_000_100,
            "validated in field test",
        )
        .expect("record success");
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Serial framing".to_string(),
                summary: "Recover frame boundaries and checksum windows before decoding."
                    .to_string(),
                content: "1. detect sync word\n2. validate checksum window\n3. emit frame"
                    .to_string(),
                citations: vec!["transcript:chat-42#message=2".to_string()],
                source_chat_id: Some("chat-42".to_string()),
                observed_at: 4_500_000_200,
            },
        )
        .expect("write revised runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Mismatch,
            4_500_000_300,
            "needs doctrine revision",
        )
        .expect("record mismatch");
        crate::skills::sync_capability_atoms_from_runtime_skills(&skill_storage, 4_500_000_900)
            .expect("sync capability atoms");

        let activity_records = collect_programmable_reasoning_activity_records(
            None,
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("collect activity records");
        let usage = build_programmable_reasoning_usage_analytics(&activity_records);
        let timeline = build_programmable_reasoning_timeline(&activity_records);
        let digest = build_programmable_reasoning_maintenance_digest(&usage, &timeline);

        assert_eq!(usage.recent_total_events, 3);
        assert_eq!(usage.recent_total_attempts, 0);
        assert_eq!(usage.recent_attention_events, 1);
        assert_eq!(usage.recent_governance_holds, 1);
        assert_eq!(usage.recent_failed, 0);
        assert_eq!(usage.recent_denied, 0);
        assert!(usage.tool_counts.is_empty());
        assert!(usage.stage_counts.iter().any(|entry| entry.stage_name
            == "doctrine_genome_evolution"
            && entry.total_events == 2));
        assert!(usage.stage_counts.iter().any(|entry| entry.stage_name
            == "capability_atoms_exchange"
            && entry.total_events == 1));

        assert_eq!(timeline.recent_events.len(), 3);
        assert_eq!(timeline.recent_events[0].activity_kind, "asset_lifecycle");
        assert_eq!(
            timeline.recent_events[0].activity_name,
            "capability_atoms_exchange"
        );
        assert_eq!(timeline.recent_events[0].tool_name, None);
        assert_eq!(timeline.recent_events[1].activity_kind, "asset_lifecycle");
        assert_eq!(
            timeline.recent_events[1].activity_name,
            "doctrine_genome_evolution"
        );
        assert_eq!(timeline.recent_events[1].status, "revision_pending");
        assert!(timeline.recent_events[1].attention_required);
        assert!(timeline
            .recent_events
            .iter()
            .any(|event| event.activity_name == "capability_atoms_exchange"
                && event.status == "local_verified"));

        assert_eq!(digest.status, "attention");
        assert_eq!(
            digest.attention_activities,
            vec!["doctrine_genome_evolution".to_string()]
        );
        assert!(digest
            .headline
            .contains("3 recent programmable reasoning events, 1 need attention"));
    }

    #[test]
    fn build_operator_status_counts_capability_atoms_inspect_in_programmable_reasoning_usage() {
        let governance = Arc::new(ToolExecutionGovernance::new(Arc::new(
            MemoryStateFs::default(),
        )));

        governance
            .record_success(
                &reasoning_permit("capability_atoms_inspect"),
                &ToolExecutionOutcome::text("inspected 4 capability atoms"),
            )
            .expect("record inspect success");

        let state = governance.inspect().expect("inspect governance");
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let activity_records = collect_programmable_reasoning_activity_records(
            Some(&state),
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("collect activity records");
        let usage = build_programmable_reasoning_usage_analytics(&activity_records);
        let timeline = build_programmable_reasoning_timeline(&activity_records);
        let digest = build_programmable_reasoning_maintenance_digest(&usage, &timeline);

        assert_eq!(usage.recent_total_events, 1);
        assert_eq!(usage.recent_total_attempts, 1);
        assert_eq!(usage.recent_attention_events, 0);
        assert_eq!(
            usage.last_event_name.as_deref(),
            Some("capability_atoms_inspect")
        );
        assert_eq!(
            usage.last_tool_name.as_deref(),
            Some("capability_atoms_inspect")
        );
        assert!(usage
            .tool_counts
            .iter()
            .any(|entry| entry.tool_name == "capability_atoms_inspect" && entry.succeeded == 1));

        assert_eq!(timeline.recent_events.len(), 1);
        assert_eq!(timeline.recent_events[0].activity_kind, "tool");
        assert_eq!(
            timeline.recent_events[0].activity_name,
            "capability_atoms_inspect"
        );
        assert_eq!(
            timeline.recent_events[0].tool_name.as_deref(),
            Some("capability_atoms_inspect")
        );
        assert_eq!(timeline.recent_events[0].status, "succeeded");
        assert_eq!(
            timeline.recent_events[0].detail,
            "inspected 4 capability atoms"
        );
        assert!(!timeline.recent_events[0].attention_required);

        assert_eq!(digest.status, "healthy");
        assert_eq!(digest.last_event_kind.as_deref(), Some("tool"));
        assert_eq!(
            digest.last_event_name.as_deref(),
            Some("capability_atoms_inspect")
        );
        assert_eq!(
            digest.last_event_tool_name.as_deref(),
            Some("capability_atoms_inspect")
        );
        assert_eq!(digest.last_event_status.as_deref(), Some("succeeded"));
        assert!(digest
            .headline
            .contains("1 recent programmable reasoning events, no operator intervention needed"));
    }

    #[test]
    fn render_operator_status_text_reports_full_programmable_reasoning_replay_surface() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        use crate::memory::{
            build_turn_ledger_start, TurnAdversarialArenaClaimLedger, TurnAdversarialArenaLedger,
            TurnCounterfactualBranchLedger, TurnCounterfactualLedger,
            TurnCounterfactualSnapshotLedger, TurnLedgerStatus,
        };
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        let topic = format!("operator_status_text_replay_{unique}");
        let chat_id = format!("operator-status-text-{unique}");
        let skill_name = crate::skills::runtime_skill_name_for_topic(&topic);

        crate::skills::upsert_runtime_skill(
            ctx.platform.skill_storage().as_ref(),
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Operator text replay".to_string(),
                summary: "Keep doctrine, genome, and capability replay visible in text mode."
                    .to_string(),
                content: "1. inspect doctrine\n2. record lineage\n3. sync atom".to_string(),
                citations: vec!["transcript:chat#message=1".to_string()],
                source_chat_id: Some(chat_id.clone()),
                observed_at: 6_300_000_000,
            },
        )
        .expect("write runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            ctx.platform.skill_storage().as_ref(),
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            6_300_000_100,
            "validated doctrine",
        )
        .expect("record runtime skill outcome");
        crate::skills::sync_capability_atoms_from_runtime_skills(
            ctx.platform.skill_storage().as_ref(),
            6_300_000_200,
        )
        .expect("sync capability atoms");

        ctx.session_store
            .append(&chat_id, "user", "replay the programmable reasoning turn")
            .expect("write session");

        let mut branch_msg = PcMsg::new_inbound(
            "qq_channel",
            &chat_id,
            "Replay the programmable reasoning turn",
            false,
        )
        .expect("operator branch message");
        branch_msg.req_id = Some("req-operator-text-branch".to_string());
        let mut branch_ledger = build_turn_ledger_start(&branch_msg, 6_300_000_300_000);
        branch_ledger.status = TurnLedgerStatus::Answered;
        branch_ledger.updated_at_ms = 6_300_000_300_120;
        branch_ledger.finished_at_ms = 6_300_000_300_120;
        branch_ledger.counterfactual = Some(TurnCounterfactualLedger {
            summary: "Compared guarded tool execution against a direct reply.".to_string(),
            snapshot: TurnCounterfactualSnapshotLedger {
                reasoning_kind: "counterfactual_sandbox".to_string(),
                reasoning_strategy: "branch_compare".to_string(),
                confidence: 91,
                ..TurnCounterfactualSnapshotLedger::default()
            },
            selected_branch: TurnCounterfactualBranchLedger {
                branch: "guarded_tool".to_string(),
                score: 89,
                summary: "Tool path preserved replayability.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            },
            alternatives: vec![TurnCounterfactualBranchLedger {
                branch: "direct_reply".to_string(),
                score: 35,
                summary: "Direct reply would hide the reasoning trail.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            }],
        });
        ctx.platform
            .turn_ledger_store()
            .set(&chat_id, &branch_ledger)
            .expect("write branch ledger");

        let mut arena_msg = PcMsg::new_inbound(
            "qq_channel",
            &chat_id,
            "Adjudicate the programmable reasoning replay",
            false,
        )
        .expect("operator arena message");
        arena_msg.req_id = Some("req-operator-text-arena".to_string());
        let mut arena_ledger = build_turn_ledger_start(&arena_msg, 6_300_000_300_200);
        arena_ledger.status = TurnLedgerStatus::Answered;
        arena_ledger.updated_at_ms = 6_300_000_300_360;
        arena_ledger.finished_at_ms = 6_300_000_300_360;
        arena_ledger.adversarial_arena = Some(TurnAdversarialArenaLedger {
            subject_kind: "capability_atom".to_string(),
            disposition: "approve".to_string(),
            summary: "Arena approved the exchange-ready atom.".to_string(),
            winner: TurnAdversarialArenaClaimLedger {
                role: "defender".to_string(),
                label: "defender".to_string(),
                evidence_score: 86,
                summary: "The atom preserved audited lineage.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
            defender: TurnAdversarialArenaClaimLedger {
                role: "defender".to_string(),
                label: "defender".to_string(),
                evidence_score: 86,
                summary: "Adopt the verified capability atom.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
            attacker: TurnAdversarialArenaClaimLedger {
                role: "attacker".to_string(),
                label: "attacker".to_string(),
                evidence_score: 51,
                summary: "Requested more local evidence.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
        });
        ctx.platform
            .turn_ledger_store()
            .set(&format!("{chat_id}-arena"), &arena_ledger)
            .expect("write arena ledger");

        let config = ctx.config();
        let snapshot = build_operator_status(OperatorStatusInput {
            config: &config,
            platform: ctx.platform.as_ref(),
            tool_registry: ctx.tool_registry.as_ref(),
        })
        .expect("operator status");
        let rendered = render_operator_status_text(&snapshot);

        assert!(rendered.contains("programmable_reasoning_branch_replays: "));
        assert!(rendered.contains("programmable_reasoning_arena_replays: "));
        assert!(rendered.contains("programmable_reasoning_doctrine_replays: "));
        assert!(rendered.contains("programmable_reasoning_genome_replays: "));
        assert!(rendered.contains("programmable_reasoning_capability_atom_replays: "));
    }

    #[test]
    fn doctrine_replay_uses_doctrine_event_time_instead_of_generic_updated_at() {
        let skill_storage = TestSkillStorage::default();
        let topic = "operator_status_truth".to_string();
        let skill_name = crate::skills::runtime_skill_name_for_topic(&topic);

        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "Operator status truth".to_string(),
                summary: "Capture doctrine evidence at the time it changes.".to_string(),
                content: "1. inspect operator surface\n2. compare evidence\n3. record result"
                    .to_string(),
                citations: vec!["transcript:chat-77#message=1".to_string()],
                source_chat_id: Some("chat-77".to_string()),
                observed_at: 4_600_000_100,
            },
        )
        .expect("write base runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            4_600_000_200,
            "validated doctrine clause",
        )
        .expect("record doctrine validation");
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic,
                title: "Operator status truth".to_string(),
                summary: "Capture doctrine evidence at the time it changes.".to_string(),
                content: "1. inspect operator surface\n2. compare evidence\n3. record result"
                    .to_string(),
                citations: vec!["transcript:chat-77#message=2".to_string()],
                source_chat_id: Some("chat-77".to_string()),
                observed_at: 4_600_000_900,
            },
        )
        .expect("touch runtime skill without doctrine change");

        let doctrine_replays = collect_programmable_reasoning_doctrine_replays(&skill_storage);
        assert_eq!(doctrine_replays.len(), 1);
        assert_eq!(doctrine_replays[0].recorded_at_ms, 4_600_000_200_000);
    }

    #[test]
    fn capability_atom_replay_uses_lifecycle_event_time_instead_of_sync_wall_clock() {
        let skill_storage = TestSkillStorage::default();
        let topic = "serial_protocol".to_string();
        let skill_name = crate::skills::runtime_skill_name_for_topic(&topic);

        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic,
                title: "Serial protocol".to_string(),
                summary: "Recover frames with a reusable macro.".to_string(),
                content: "1. detect preamble\n2. confirm crc\n3. emit payload".to_string(),
                citations: vec!["transcript:chat-88#message=1".to_string()],
                source_chat_id: Some("chat-88".to_string()),
                observed_at: 4_700_000_100,
            },
        )
        .expect("write base runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            4_700_000_200,
            "validated reusable macro",
        )
        .expect("record validation");
        crate::skills::sync_capability_atoms_from_runtime_skills(&skill_storage, 4_700_000_900)
            .expect("sync capability atoms");

        let atom_replays = collect_programmable_reasoning_capability_atom_replays(&skill_storage);
        assert_eq!(atom_replays.len(), 1);
        assert_eq!(atom_replays[0].recorded_at_ms, 4_700_000_200_000);
        assert_eq!(atom_replays[0].status, "local_verified");
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
        assert_eq!(
            snapshot
                .programmable_reasoning
                .product_surface
                .demo_scenarios
                .len(),
            3
        );
        assert_eq!(
            snapshot
                .programmable_reasoning
                .inspection
                .doctrine
                .stable_clauses,
            snapshot.programmable_reasoning.doctrine.stable_clauses
        );
        assert_eq!(
            snapshot
                .programmable_reasoning
                .inspection
                .genome
                .active_lineages,
            snapshot.programmable_reasoning.genome.active_lineages
        );
        assert!(!snapshot
            .programmable_reasoning
            .inspection
            .doctrine
            .headline
            .is_empty());
        assert!(!snapshot
            .programmable_reasoning
            .inspection
            .genome
            .headline
            .is_empty());
        assert!(!snapshot
            .programmable_reasoning
            .inspection
            .doctrine
            .highlighted_topics
            .is_empty());
        assert!(!snapshot
            .programmable_reasoning
            .inspection
            .genome
            .highlighted_skills
            .is_empty());
    }

    #[test]
    fn build_operator_status_exposes_programmable_reasoning_replay_views() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        use crate::memory::{
            build_turn_ledger_start, TurnAdversarialArenaClaimLedger, TurnAdversarialArenaLedger,
            TurnCounterfactualBranchLedger, TurnCounterfactualLedger,
            TurnCounterfactualSnapshotLedger, TurnLedgerStatus,
        };
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let chat_branch = format!("replay-branch-{unique}");
        let chat_arena = format!("replay-arena-{unique}");

        session_store
            .append(&chat_branch, "user", "compile the reasoning replay")
            .expect("write branch session");
        session_store
            .append(&chat_arena, "user", "adjudicate the replay claim")
            .expect("write arena session");

        let mut branch_msg = PcMsg::new_inbound(
            "qq_channel",
            &chat_branch,
            "Compile the reasoning replay",
            false,
        )
        .expect("branch message");
        branch_msg.req_id = Some("req-branch".to_string());
        let mut branch_ledger = build_turn_ledger_start(&branch_msg, 4_300_000_000_000);
        branch_ledger.status = TurnLedgerStatus::Answered;
        branch_ledger.updated_at_ms = 4_300_000_000_120;
        branch_ledger.finished_at_ms = 4_300_000_000_120;
        branch_ledger.reason = "branch replay".to_string();
        branch_ledger.counterfactual = Some(TurnCounterfactualLedger {
            summary: "Compared ask-clarify and execute-native-tool branches.".to_string(),
            snapshot: TurnCounterfactualSnapshotLedger {
                reasoning_kind: "intent_compiler".to_string(),
                reasoning_strategy: "branch_compare".to_string(),
                confidence: 92,
                ..TurnCounterfactualSnapshotLedger::default()
            },
            selected_branch: TurnCounterfactualBranchLedger {
                branch: "execute_native_tool".to_string(),
                score: 91,
                summary: "Tool execution has enough grounding.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            },
            alternatives: vec![TurnCounterfactualBranchLedger {
                branch: "ask_for_clarification".to_string(),
                score: 44,
                summary: "Clarification would slow a grounded action.".to_string(),
                ..TurnCounterfactualBranchLedger::default()
            }],
        });
        turn_ledger_store
            .set(&chat_branch, &branch_ledger)
            .expect("write branch ledger");

        let mut arena_msg = PcMsg::new_inbound(
            "telegram",
            &chat_arena,
            "Adjudicate the replay claim",
            false,
        )
        .expect("arena message");
        arena_msg.req_id = Some("req-arena".to_string());
        let mut arena_ledger = build_turn_ledger_start(&arena_msg, 4_300_000_000_220);
        arena_ledger.status = TurnLedgerStatus::Answered;
        arena_ledger.updated_at_ms = 4_300_000_000_360;
        arena_ledger.finished_at_ms = 4_300_000_000_360;
        arena_ledger.reason = "arena replay".to_string();
        arena_ledger.adversarial_arena = Some(TurnAdversarialArenaLedger {
            subject_kind: "tool_request".to_string(),
            disposition: "revise".to_string(),
            summary: "The defender kept the tool round but revised the safety boundary."
                .to_string(),
            winner: TurnAdversarialArenaClaimLedger {
                role: "defender".to_string(),
                label: "defender".to_string(),
                evidence_score: 88,
                summary: "Grounding was strong enough for a guarded tool round.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
            defender: TurnAdversarialArenaClaimLedger {
                role: "defender".to_string(),
                label: "defender".to_string(),
                evidence_score: 88,
                summary: "Kept the tool round with a narrower safety boundary.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
            attacker: TurnAdversarialArenaClaimLedger {
                role: "attacker".to_string(),
                label: "attacker".to_string(),
                evidence_score: 63,
                summary: "Argued the tool round was too eager.".to_string(),
                ..TurnAdversarialArenaClaimLedger::default()
            },
        });
        turn_ledger_store
            .set(&chat_arena, &arena_ledger)
            .expect("write arena ledger");

        let replay = build_programmable_reasoning_replay_inspection(
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("build replay inspection");
        assert!(replay.branch_replays_retained >= 1);
        assert!(!replay.recent_branch_replays.is_empty());
        assert_eq!(replay.recent_branch_replays[0].chat_id, chat_branch);
        assert_eq!(
            replay.recent_branch_replays[0].selected_branch,
            "execute_native_tool"
        );
        assert_eq!(
            replay.recent_branch_replays[0].rejected_branches,
            vec!["ask_for_clarification".to_string()]
        );
        assert_eq!(
            replay.recent_branch_replays[0].reasoning_strategy,
            "branch_compare"
        );
        assert!(replay.arena_replays_retained >= 1);
        assert!(!replay.recent_arena_replays.is_empty());
        assert_eq!(replay.recent_arena_replays[0].chat_id, chat_arena);
        assert_eq!(replay.recent_arena_replays[0].disposition, "revise");
        assert_eq!(replay.recent_arena_replays[0].winner, "defender");
        assert_eq!(replay.recent_arena_replays[0].attacker, "attacker");
        assert_eq!(replay.doctrine_replays_retained, 0);
        assert!(replay.recent_doctrine_replays.is_empty());
        assert_eq!(replay.genome_replays_retained, 0);
        assert!(replay.recent_genome_replays.is_empty());
        assert_eq!(replay.capability_atom_replays_retained, 0);
        assert!(replay.recent_capability_atom_replays.is_empty());
    }

    #[test]
    fn build_operator_status_exposes_doctrine_genome_and_capability_atom_replays() {
        let _guard = crate::platform::http_server::handlers::default_test_handler_context_guard();
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = format!(
            "{:x}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let session_store = TestSessionStore::default();
        let turn_ledger_store = TestTurnLedgerStore::default();
        let skill_storage = TestSkillStorage::default();
        let topic = format!("p13_replay_contract_{unique}");
        let chat_id = format!("replay-chat-{unique}");
        let skill_name = format!("runtime_skill__{topic}");
        let observed_at = 6_100_000_000_u64;

        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "P13 replay contract".to_string(),
                summary: "Compile the initial doctrine before promoting reusable capability."
                    .to_string(),
                content: "1. inspect doctrine\n2. record genome\n3. sync capability atom"
                    .to_string(),
                citations: vec!["turn_log:chat-1#req=req-1".to_string()],
                source_chat_id: Some(chat_id.clone()),
                observed_at,
            },
        )
        .expect("write initial runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Succeeded,
            observed_at + 20,
            "validated initial doctrine",
        )
        .expect("record validated outcome");
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: skill_name.clone(),
                topic: topic.clone(),
                title: "P13 replay contract".to_string(),
                summary:
                    "Compile the revised doctrine, then preserve the genome diff before exchange."
                        .to_string(),
                content:
                    "1. inspect doctrine delta\n2. review genome diff\n3. sync capability atom"
                        .to_string(),
                citations: vec!["turn_log:chat-1#req=req-2".to_string()],
                source_chat_id: Some(chat_id.clone()),
                observed_at: observed_at + 40,
            },
        )
        .expect("write revised runtime skill");
        crate::skills::record_runtime_skill_outcomes(
            &skill_storage,
            std::slice::from_ref(&skill_name),
            crate::skills::RuntimeSkillReuseOutcome::Mismatch,
            observed_at + 60,
            "revision pending after doctrine change",
        )
        .expect("record mismatch outcome");
        crate::skills::sync_capability_atoms_from_runtime_skills(&skill_storage, observed_at + 80)
            .expect("sync capability atoms");

        let replay = build_programmable_reasoning_replay_inspection(
            &session_store,
            &turn_ledger_store,
            &skill_storage,
        )
        .expect("build replay inspection");
        assert!(replay.doctrine_replays_retained >= 1);
        assert!(replay.recent_doctrine_replays.iter().any(|record| {
            record.source_skill_name == skill_name
                && record.source_chat_id.as_deref() == Some(chat_id.as_str())
        }));
        assert!(replay.genome_replays_retained >= 1);
        assert!(replay.recent_genome_replays.iter().any(|record| {
            record.skill_name == skill_name
                && record.source_chat_id.as_deref() == Some(chat_id.as_str())
                && record.diff_events >= 1
        }));
        assert!(replay.capability_atom_replays_retained >= 1);
        assert!(replay.recent_capability_atom_replays.iter().any(|record| {
            record.atom_name == format!("capability_atom__{topic}")
                && record.source_chat_id.as_deref() == Some(chat_id.as_str())
        }));
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
