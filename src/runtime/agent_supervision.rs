//! Agent loop supervision shared by lightweight runtime planes.
//! Agent 主循环监管：ESP 侧复用 bg_timer，避免额外长驻线程栈。

use crate::Platform;
use std::sync::{Arc, Mutex, OnceLock};

struct AgentLoopGuardState {
    platform: Option<Arc<dyn Platform>>,
    handle: Option<crate::util::TaskHandle>,
    restart_requested: bool,
}

fn guard_state() -> &'static Mutex<AgentLoopGuardState> {
    static STATE: OnceLock<Mutex<AgentLoopGuardState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(AgentLoopGuardState {
            platform: None,
            handle: None,
            restart_requested: false,
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
    state.handle = handle;
    state.restart_requested = false;
}

/// Poll the registered agent loop handle and request a restart if it has exited.
pub fn service_agent_loop_guard(tag: &str) {
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
