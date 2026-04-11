//! Orchestrator-facing transport permit types and wrappers.
//! 编排器对外的 transport permit 类型与薄封装；真正权威在 `crate::network`。

use crate::error::Result;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::state::OrchestratorState;

/// 线程角色：用于 TLS 准入前降噪与优先级偏置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpThreadRole {
    Interactive,
    Io,
    Background,
}

pub fn set_current_http_thread_role(role: HttpThreadRole) {
    crate::network::set_current_http_thread_role(role);
}

pub fn current_http_thread_role() -> HttpThreadRole {
    crate::network::current_http_thread_role()
}

/// HTTP 请求优先级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// RAII guard：Drop 时递减 active_http_count + 释放 TLS 令牌。
pub type HttpPermitGuard = crate::network::TransportHttpPermitGuard;

/// RAII guard：持有期间表示一个已建立的 WSS 长连接存活。
pub type WssSessionGuard = crate::network::TransportWssSessionGuard;

/// RAII guard：持有期间 `active_agent_tasks` 非零，Drop 时递减。
pub struct AgentTaskGuard {
    state: &'static OrchestratorState,
}

impl AgentTaskGuard {
    pub(super) fn new(state: &'static OrchestratorState) -> Self {
        state.active_agent_tasks.fetch_add(1, Ordering::Relaxed);
        Self { state }
    }
}

impl Drop for AgentTaskGuard {
    fn drop(&mut self) {
        self.state
            .active_agent_tasks
            .fetch_sub(1, Ordering::Relaxed);
    }
}

/// 请求 HTTP 准入令牌。
pub fn request_http_permit(
    state: &'static OrchestratorState,
    _tls_permit: &'static std::sync::Mutex<()>,
    priority: Priority,
    timeout: Duration,
) -> Result<HttpPermitGuard> {
    let pressure =
        super::pressure::PressureLevel::from_byte(state.pressure_level.load(Ordering::Relaxed));
    let active_agent_tasks = state.active_agent_tasks.load(Ordering::Relaxed);
    crate::network::request_http_permit(priority, timeout, pressure, active_agent_tasks)
}
