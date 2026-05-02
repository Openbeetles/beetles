//! Orchestrator-facing transport permit types and wrappers.
//! 编排器对外的 transport permit 类型与薄封装；真正权威在 `crate::network`。

use crate::error::Result;
use std::cell::Cell;
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

thread_local! {
    static HTTP_PRIORITY_OVERRIDE: Cell<Option<Priority>> = const { Cell::new(None) };
}

/// RAII guard：持有期间当前线程的 HTTP 请求至少使用指定优先级。
pub struct HttpPriorityOverrideGuard {
    previous: Option<Priority>,
}

impl HttpPriorityOverrideGuard {
    pub(super) fn new(priority: Priority) -> Self {
        let previous = HTTP_PRIORITY_OVERRIDE.with(|slot| {
            let previous = slot.get();
            slot.set(Some(previous.map_or(priority, |old| old.max(priority))));
            previous
        });
        Self { previous }
    }
}

impl Drop for HttpPriorityOverrideGuard {
    fn drop(&mut self) {
        HTTP_PRIORITY_OVERRIDE.with(|slot| slot.set(self.previous));
    }
}

fn effective_http_priority(priority: Priority) -> Priority {
    HTTP_PRIORITY_OVERRIDE.with(|slot| {
        slot.get().map_or(priority, |override_priority| {
            priority.max(override_priority)
        })
    })
}

#[cfg(test)]
pub(crate) fn effective_http_priority_for_tests(priority: Priority) -> Priority {
    effective_http_priority(priority)
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

/// RAII guard：真实前台用户 turn 持有 AgentHeavyTurn lease，并同时维护 agent task 指标。
pub struct ForegroundTurnGuard {
    _agent_task_guard: AgentTaskGuard,
    lease_owner: crate::runtime::lease::LeaseOwner,
    lease_token: u64,
}

impl ForegroundTurnGuard {
    pub(super) fn new(state: &'static OrchestratorState) -> Result<Self> {
        let lease_owner = crate::runtime::lease::LeaseOwner::new("agent", "foreground_turn");
        match crate::runtime::lease::try_acquire_exclusive_once(
            crate::runtime::lease::LeaseKind::AgentHeavyTurn,
            lease_owner,
            None,
            crate::runtime::lease::LeaseReplacePolicy::ReplaceExpired,
        ) {
            crate::runtime::lease::LeaseDecision::Acquired(record)
            | crate::runtime::lease::LeaseDecision::ReplacedExpired {
                current: record, ..
            } => Ok(Self {
                _agent_task_guard: AgentTaskGuard::new(state),
                lease_owner,
                lease_token: record.token,
            }),
            crate::runtime::lease::LeaseDecision::Reentered(record) => Ok(Self {
                _agent_task_guard: AgentTaskGuard::new(state),
                lease_owner,
                lease_token: record.token,
            }),
            crate::runtime::lease::LeaseDecision::Denied(denial) => {
                Err(crate::error::Error::config(
                    "foreground_turn_lease",
                    format!(
                        "AgentHeavyTurn denied reason={} held_by={:?}",
                        denial.reason, denial.held_by
                    ),
                ))
            }
        }
    }
}

impl Drop for ForegroundTurnGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::lease::release_token(
            crate::runtime::lease::LeaseKind::AgentHeavyTurn,
            self.lease_owner,
            self.lease_token,
        );
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
    crate::network::request_http_permit(
        effective_http_priority(priority),
        timeout,
        pressure,
        active_agent_tasks,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn foreground_turn_acquires_agent_heavy_turn_lease() {
        let _guard = crate::runtime::lease::lease_test_guard();

        let foreground_turn = crate::orchestrator::begin_foreground_turn()
            .expect("foreground turn should acquire AgentHeavyTurn");

        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::AgentHeavyTurn
            ),
            1,
            "foreground user turns must hold the AgentHeavyTurn lease; active_agent_tasks is only a metric"
        );

        drop(foreground_turn);
        assert_eq!(
            crate::runtime::lease::active_count_for_kind(
                crate::runtime::lease::LeaseKind::AgentHeavyTurn
            ),
            0
        );
    }

    #[test]
    fn reply_critical_scope_raises_effective_http_priority() {
        assert_eq!(
            super::effective_http_priority_for_tests(crate::orchestrator::Priority::Normal),
            crate::orchestrator::Priority::Normal
        );
        let _guard = crate::orchestrator::begin_reply_critical_http_scope();
        assert_eq!(
            super::effective_http_priority_for_tests(crate::orchestrator::Priority::Normal),
            crate::orchestrator::Priority::Critical
        );
    }
}
