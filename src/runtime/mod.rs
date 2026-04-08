//! Runtime utilities for beetle application.
//! 运行时工具模块。

pub mod acceptance;
pub mod continuity_flush;
pub mod delayed_task;
pub mod initiative;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_control_plane;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_release;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux_supervisor;
pub mod mode;
pub mod presence;
pub mod soul_kernel;
pub mod stream_http;
pub mod system_work;
pub mod thread_registry;
pub mod thread_util;
pub mod write_back;

pub use acceptance::{
    inspect_beetle_os_closure, inspect_platform_beetle_os_closure, BeetleOsClosureReport,
    BeetleOsPlane, BeetleOsPlaneReport,
};
pub use continuity_flush::{flush_reboot_continuity_bundle, request_restart_with_continuity_flush};
pub use delayed_task::{
    next_delayed_task_wait, schedule_critical_delayed_task, schedule_delayed_task,
    service_delayed_tasks,
};
pub use initiative::{
    initiative_tick, inspect_platform_initiative, InitiativeAction, InitiativeSignalSnapshot,
    InitiativeSnapshot, InitiativeSuppressionReason, InitiativeTarget,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use linux_release::{
    ensure_state_schema, inspect_platform_linux_release, mark_current_release_steady,
    rollback_current_release, sync_platform_release_state, LinuxReleasePointer,
    LinuxReleaseRolloutState, LinuxReleaseState, LinuxReleaseStatus, StateSchemaStatus,
    BEETLE_STATE_SCHEMA_VERSION, LINUX_RELEASE_STATE_VERSION, REL_PATH_LINUX_RELEASE_STATE,
    REL_PATH_STATE_SCHEMA_STATUS,
};
pub use mode::{RuntimeMode, RuntimeModeActionBudget, RuntimeModeSnapshot};
pub use presence::{
    inspect_platform_presence, PresenceDisplayProjection, PresenceSnapshot, PresenceState,
};
pub use soul_kernel::{
    ensure_platform_soul_kernel_recovery, inspect_platform_soul_kernel, SoulKernelPromptProjection,
    SoulKernelRecoveryAction, SoulKernelRecoveryReport, SoulKernelStatus,
};
pub use stream_http::{execute_stream_http_op, invalidate_stream_http_slot};
pub use thread_registry::ThreadRegistrySnapshot;
pub use thread_util::{spawn_planned, spawn_planned_handle, thread_plan, ThreadPlan};
