//! Runtime utilities for beetle application.
//! 运行时工具模块。

pub mod stream_http;
pub mod thread_registry;
pub mod thread_util;

pub use stream_http::{execute_stream_http_op, invalidate_stream_http_slot};
pub use thread_registry::ThreadRegistrySnapshot;
pub use thread_util::{spawn_planned, thread_plan, ThreadPlan};
