//! 配置 HTTP API 的传输无关路由：ESP 与 Linux 共用同一 `dispatch`。
//! Transport-agnostic routing for the config HTTP API; shared by ESP and Linux.

pub(crate) mod auth;
pub(crate) mod catalog;
mod dispatch;
mod types;

pub use dispatch::dispatch;
pub use types::{IncomingRequest, OutgoingResponse, RestartAction, RouterEnv};
