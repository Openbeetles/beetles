//! Thread management utilities.
//! 线程管理工具。

use crate::util::{spawn_guarded_with_profile, HttpThreadRole, SpawnCore};

#[derive(Clone, Copy)]
pub struct ThreadPlan {
    pub core: Option<SpawnCore>,
    pub role: HttpThreadRole,
}

pub fn thread_plan(name: &str) -> ThreadPlan {
    match name {
        "wifi_worker" | "dispatch" | "tg_poll" | "feishu_ws" | "qq_ws" | "tg_sender"
        | "fs_sender" | "dt_sender" | "wc_sender" | "qq_sender" | "http_server"
        | "restart_defer" => ThreadPlan {
            core: Some(SpawnCore::Core0),
            role: HttpThreadRole::Io,
        },
        "agent_user_loop" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Interactive,
        },
        "agent_system_loop" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Background,
        },
        "display"
        | "cron"
        | "heartbeat"
        | "heartbeat_tasks"
        | "remind"
        | "cli_repl"
        | "agent_waiting_notice" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Background,
        },
        _ => ThreadPlan {
            core: None,
            role: HttpThreadRole::Background,
        },
    }
}

pub fn spawn_planned<F>(name: &str, stack_size: usize, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let plan = thread_plan(name);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if plan.core.is_none() {
        log::error!(
            "[runtime::thread_util] thread plan missing core mapping for '{}', falling back to scheduler default",
            name
        );
    }
    spawn_guarded_with_profile(name, stack_size, plan.core, plan.role, f);
}
