//! Runtime utilities for beetle application.
//! 运行时工具模块。

pub mod continuity_flush;
pub mod delayed_task;
pub mod stream_http;
pub mod system_work;
pub mod thread_registry;
pub mod thread_util;
pub mod write_back;

pub use continuity_flush::{flush_reboot_continuity_bundle, request_restart_with_continuity_flush};
pub use delayed_task::{
    next_delayed_task_wait, schedule_critical_delayed_task, schedule_delayed_task,
    service_delayed_tasks,
};
pub use stream_http::{execute_stream_http_op, invalidate_stream_http_slot};
pub use thread_registry::ThreadRegistrySnapshot;
pub use thread_util::{spawn_planned, thread_plan, ThreadPlan};
