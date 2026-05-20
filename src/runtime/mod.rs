//! Runtime utilities for beetle application.
//! 运行时工具模块。

pub mod acceptance;
pub mod agent_supervision;
pub mod channel_wss_supervision;
pub mod continuity_flush;
pub mod delayed_task;
pub mod execution_budget;
pub mod foreground;
pub mod frame_lease;
pub mod governance;
pub mod initiative;
pub mod lease;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_release;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_service;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_signal;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_stop;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_systemd;
pub mod mode;
pub mod operator_maintenance;
pub mod plane;
pub mod plane_lifecycle;
pub mod presence;
pub mod scheduler;
pub mod soul_kernel;
pub mod system_work;
pub mod thread_registry;
pub mod thread_util;
pub mod workflow;
pub mod write_back;

pub use acceptance::{
    inspect_beetle_os_closure, inspect_platform_beetle_os_closure, BeetleOsClosureReport,
    BeetleOsPlane, BeetleOsPlaneReport,
};
pub use agent_supervision::{register_agent_loop_guard, service_agent_loop_guard};
pub use channel_wss_supervision::{
    channel_wss_worker_start_defer_reason, next_channel_wss_supervisor_retry_at,
    register_channel_wss_supervisor, register_deferred_channel_wss_supervisor,
    service_channel_wss_supervisors,
};
pub use continuity_flush::{
    flush_reboot_continuity_bundle, request_restart_with_continuity_flush,
    schedule_restart_with_continuity_flush,
};
pub use delayed_task::{
    next_delayed_task_wait, schedule_bounded_keyed_system_inbound_msg,
    schedule_critical_delayed_task, schedule_delayed_task, schedule_keyed_system_inbound_msg,
    schedule_system_inbound_msg, service_delayed_tasks,
};
pub use execution_budget::ExecutionBudgetSnapshot;
pub use foreground::{
    finish_runtime_foreground, renew_runtime_foreground, renew_runtime_foreground_now,
    runtime_foreground_active, runtime_foreground_overlay, runtime_foreground_snapshot,
    RuntimeForegroundOverlay, RuntimeForegroundSnapshot, RuntimeForegroundSource,
    RuntimeForegroundTicket, RuntimeForegroundTicketState, RUNTIME_FOREGROUND_IDLE_SECS,
    RUNTIME_FOREGROUND_RECOVERY_SECS,
};
pub use frame_lease::{
    admit_current_camera_frame_capture, try_borrow_frame, try_borrow_frame_with_admission,
    FrameLease, FrameLeaseAdmission,
};
pub use governance::{
    config_activity_active, config_activity_snapshot, set_recovery_safe_mode_active,
    sync_pairing_state_from_store, BackgroundMaintenanceGuard, ConfigActivityGuard,
    ConfigActivityPhase, ConfigActivitySnapshot, ConfigPlaneGuard, ConfigReadBurstGuard,
    CONFIG_ACTIVITY_WINDOW_SECS,
};
pub use initiative::{
    initiative_tick, inspect_platform_initiative, InitiativeAction, InitiativeSignalSnapshot,
    InitiativeSnapshot, InitiativeSuppressionReason, InitiativeTarget,
};
pub use lease::{LeaseKind, LeaseOwner, LeaseSnapshot};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use linux_release::{
    ensure_state_schema, inspect_platform_linux_release, mark_current_release_steady,
    rollback_current_release, sync_platform_release_state, LinuxReleasePointer,
    LinuxReleaseRolloutState, LinuxReleaseState, LinuxReleaseStatus, StateSchemaStatus,
    BEETLE_STATE_SCHEMA_VERSION, LINUX_RELEASE_STATE_VERSION, REL_PATH_LINUX_RELEASE_STATE,
    REL_PATH_STATE_SCHEMA_STATUS,
};
pub use mode::{RuntimeMode, RuntimeModeActionBudget, RuntimeModeSnapshot};
#[cfg(test)]
pub use operator_maintenance::operator_maintenance_test_guard;
pub use operator_maintenance::{
    drain_persisted_operator_maintenance_requests, submit_operator_maintenance_request,
    OperatorMaintenanceAction, OperatorMaintenanceRequest, OperatorMaintenanceSubmission,
    CHANNEL_OPERATOR_MAINTENANCE,
};
pub use plane::{PlaneId, PlaneProfile, PlaneRegistrySnapshot};
pub use plane_lifecycle::{PlaneLifecycleSnapshot, PlaneLifecycleState};
pub use presence::{
    inspect_platform_display_projection, inspect_platform_display_projection_with_resource,
    inspect_platform_presence, PresenceDisplayProjection, PresenceSnapshot, PresenceState,
};
pub use scheduler::{
    admit_current_runtime_work, admit_runtime_work, current_runtime_scheduler_context,
    default_runtime_scheduler_profile, RuntimePlanePolicyProfile, RuntimeSchedulerContext,
    RuntimeSchedulerSnapshot, RuntimeWorkClass, RuntimeWorkDecision, RuntimeWorkPriority,
    RuntimeWorkRequest, RuntimeWorkSource,
};
pub use soul_kernel::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, SoulKernelPromptProjection,
    SoulKernelRecoveryAction, SoulKernelRecoveryReport, SoulKernelStatus,
};
pub use thread_registry::ThreadRegistrySnapshot;
pub use thread_util::{spawn_planned, spawn_planned_handle, thread_plan, ThreadPlan};
#[cfg(test)]
pub use workflow::workflow_audit_test_guard;
pub use workflow::{
    append_workflow_audit, recent_workflow_audits, workflow_audit_snapshot,
    WorkflowAdmissionSnapshot, WorkflowAuditRecord, WorkflowAuditSnapshot, WorkflowAuditSummary,
    WorkflowDisposition, WorkflowEffect, WorkflowKind, WorkflowRecoveryPolicy, WorkflowTrigger,
};
pub use write_back::service_write_back_tasks;
