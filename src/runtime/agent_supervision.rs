//! Agent loop supervision shared by lightweight runtime planes.
//! Agent 主循环监管：ESP 侧复用 bg_timer，避免额外长驻线程栈。

use crate::Platform;
use std::sync::{Arc, Mutex, OnceLock};

pub type AgentLoopSpawner =
    dyn Fn() -> crate::Result<crate::util::TaskHandle> + Send + Sync + 'static;

struct AgentLoopGuardState {
    platform: Option<Arc<dyn Platform>>,
    handle: Option<crate::util::TaskHandle>,
    spawner: Option<Arc<AgentLoopSpawner>>,
    restart_requested: bool,
    eager_start: bool,
    start_requested: bool,
}

fn guard_state() -> &'static Mutex<AgentLoopGuardState> {
    static STATE: OnceLock<Mutex<AgentLoopGuardState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(AgentLoopGuardState {
            platform: None,
            handle: None,
            spawner: None,
            restart_requested: false,
            eager_start: false,
            start_requested: false,
        })
    })
}

/// Register the long-lived agent loop for supervision by an existing runtime plane.
pub fn register_agent_loop_guard(
    platform: Arc<dyn Platform>,
    handle: Option<crate::util::TaskHandle>,
) {
    let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
    state.platform = Some(platform);
    if handle.is_some() {
        state.spawner = None;
        state.handle = handle;
    } else if state.handle.is_none() {
        state.handle = None;
    }
    state.restart_requested = false;
    state.eager_start = false;
    state.start_requested = false;
}

/// Register the logical agent plane and defer the heavy agent_loop thread until startup readiness allows it.
pub fn register_deferred_agent_loop_guard(
    platform: Arc<dyn Platform>,
    spawner: Arc<AgentLoopSpawner>,
    eager_start: bool,
) {
    let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
    state.platform = Some(platform);
    state.handle = None;
    state.spawner = Some(spawner);
    state.restart_requested = false;
    state.eager_start = eager_start;
    state.start_requested = false;
    let _ = crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::AgentMain,
        "agent_loop",
        crate::runtime::PlaneLifecycleState::Registered,
        "logical_owner_registered",
    );
    crate::bg_timer::notify_deadline_changed();
}

/// Request the deferred ESP agent loop to start because real inbound work exists.
pub fn request_deferred_agent_loop_start(reason: &'static str) {
    let should_notify = {
        let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
        if state.restart_requested || state.handle.is_some() || state.spawner.is_none() {
            false
        } else {
            state.start_requested = true;
            true
        }
    };
    if should_notify {
        let _ = crate::runtime::plane_lifecycle::mark(
            crate::runtime::PlaneId::AgentMain,
            "agent_loop",
            crate::runtime::PlaneLifecycleState::Registered,
            reason,
        );
        crate::bg_timer::notify_deadline_changed();
    }
}

/// Poll the registered agent loop handle and request a restart if it has exited.
pub fn service_agent_loop_guard(tag: &str) {
    if service_deferred_agent_loop_start(tag) {
        return;
    }

    let restart = {
        let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
        if state.restart_requested {
            None
        } else if state
            .handle
            .as_ref()
            .is_some_and(crate::util::TaskHandle::is_finished)
        {
            state.restart_requested = true;
            let platform = state.platform.as_ref().map(Arc::clone);
            let handle = state.handle.take();
            platform.map(|platform| (platform, handle))
        } else {
            None
        }
    };

    let Some((platform, handle)) = restart else {
        return;
    };
    if let Some(done) = handle {
        let _ = done.join();
    }
    log::error!("[{}] agent_loop exited; restart requested", tag);
    crate::runtime::request_restart_with_continuity_flush(platform, None, "agent_loop_join_exit");
}

fn service_deferred_agent_loop_start(tag: &str) -> bool {
    let readiness = crate::runtime::runtime_startup_readiness_snapshot();
    let spawner = {
        let state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
        if state.restart_requested || state.handle.is_some() {
            return false;
        }
        let Some(spawner) = state.spawner.as_ref().map(Arc::clone) else {
            return false;
        };
        if !readiness.allow_agent_heavy_execution {
            drop(state);
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::AgentMain,
                "agent_loop",
                crate::runtime::PlaneLifecycleState::Suspended,
                readiness.worker_block_reason(),
            );
            return false;
        }
        if !state.eager_start && !state.start_requested {
            drop(state);
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::AgentMain,
                "agent_loop",
                crate::runtime::PlaneLifecycleState::Suspended,
                "waiting_for_agent_work",
            );
            return false;
        }
        spawner
    };

    let _ = crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::AgentMain,
        "agent_loop",
        crate::runtime::PlaneLifecycleState::Starting,
        "startup_ready",
    );
    match spawner() {
        Ok(handle) => {
            let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
            state.handle = Some(handle);
            state.spawner = None;
            state.start_requested = false;
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::AgentMain,
                "agent_loop",
                crate::runtime::PlaneLifecycleState::Active,
                "started",
            );
            log::info!(
                "[{}] deferred agent_loop started after startup readiness",
                tag
            );
            true
        }
        Err(error) => {
            crate::metrics::record_runtime_spawn_failure();
            let platform = {
                let mut state = guard_state().lock().unwrap_or_else(|e| e.into_inner());
                state.restart_requested = true;
                state.platform.as_ref().map(Arc::clone)
            };
            let _ = crate::runtime::plane_lifecycle::mark(
                crate::runtime::PlaneId::AgentMain,
                "agent_loop",
                crate::runtime::PlaneLifecycleState::Failed,
                "spawn_failed",
            );
            log::error!("[{}] deferred agent_loop spawn failed: {}", tag, error);
            if let Some(platform) = platform {
                crate::runtime::request_restart_with_continuity_flush(
                    platform,
                    None,
                    "agent_loop_deferred_spawn_failed",
                );
            }
            false
        }
    }
}
