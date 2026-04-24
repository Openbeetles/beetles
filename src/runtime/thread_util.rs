//! Thread management utilities.
//! 线程管理工具。

use crate::util::{spawn_guarded_with_profile_handle, HttpThreadRole, SpawnCore, TaskHandle};

#[derive(Clone, Copy)]
pub struct ThreadPlan {
    pub core: Option<SpawnCore>,
    pub role: HttpThreadRole,
}

pub fn thread_plan(name: &str) -> ThreadPlan {
    match name {
        "wifi_worker" | "dispatch" | "tg_poll" | "feishu_ws" | "qq_ws" | "wecom_aibot"
        | "dingtalk_stream" | "tg_sender" | "fs_sender" | "dt_sender" | "wc_sender"
        | "qq_sender" | "config_plane_watch" | "restart_defer" => ThreadPlan {
            core: Some(SpawnCore::Core0),
            role: HttpThreadRole::Io,
        },
        "http_config_exec" | "http_diag_exec" | "http_ota_exec" | "http_snapshot_exec" => {
            ThreadPlan {
                core: Some(SpawnCore::Core1),
                role: HttpThreadRole::Io,
            }
        }
        "agent_loop" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Interactive,
        },
        "audio_io_worker" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Background,
        },
        "voice_session" | "voice_session_worker" => ThreadPlan {
            core: Some(SpawnCore::Core1),
            role: HttpThreadRole::Background,
        },
        "display" | "cron" | "heartbeat" | "heartbeat_tasks" | "remind" | "runtime_guard"
        | "cli_repl" => ThreadPlan {
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
    let _ = spawn_planned_handle(name, stack_size, f);
}

pub fn spawn_planned_handle<F>(name: &str, stack_size: usize, f: F) -> std::io::Result<TaskHandle>
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
    spawn_guarded_with_profile_handle(name, stack_size, plan.core, plan.role, f)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn esp_native_idle_waits_are_bounded_for_watchdog_feeds() {
        let wait = crate::platform::esp_runtime_policy::bounded_watchdog_wait(None);
        assert!(wait <= crate::platform::esp_runtime_policy::ESP_TASK_WDT_IDLE_POLL);

        let long = crate::platform::esp_runtime_policy::bounded_watchdog_wait(Some(
            Duration::from_secs(30),
        ));
        assert_eq!(
            long,
            crate::platform::esp_runtime_policy::ESP_TASK_WDT_IDLE_POLL
        );
    }

    #[test]
    fn esp_task_wdt_pause_guard_is_constructible_for_opaque_dispatch() {
        let _guard = crate::platform::esp_runtime_policy::TaskWdtSubscriptionPause::current_task();
    }
}
