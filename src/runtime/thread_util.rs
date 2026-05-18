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
        "wifi_worker"
        | "dispatch"
        | "os_outbound"
        | "os_outbound_supervisor"
        | "tg_poll"
        | "feishu_ws"
        | "qq_ws"
        | "wecom_aibot"
        | "dingtalk_stream"
        | "tg_sender"
        | "fs_sender"
        | "dt_sender"
        | "wc_sender"
        | "qq_sender"
        | "config_plane_watch" => ThreadPlan {
            core: Some(SpawnCore::Core0),
            role: HttpThreadRole::Io,
        },
        "http_snapshot_exec" | "http_chat_history_exec" | "http_config_exec" | "http_diag_exec" => {
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
        "display" | "cron" | "heartbeat" | "heartbeat_tasks" | "remind" | "cli_repl" => {
            ThreadPlan {
                core: Some(SpawnCore::Core1),
                role: HttpThreadRole::Background,
            }
        }
        _ => ThreadPlan {
            core: None,
            role: HttpThreadRole::Background,
        },
    }
}

/// Return the declared stack budget for a planned runtime thread name.
pub fn stack_budget_for_thread(name: &str) -> Option<usize> {
    match name {
        "runtime_bootstrap" => Some(crate::util::STACK_ESP_RUNTIME_BOOT),
        "startup_recovery" => Some(crate::util::STACK_STARTUP_RECOVERY),
        "config_plane_watch" => Some(crate::util::STACK_CONFIG_PLANE_WATCH),
        "http_snapshot_exec" => Some(crate::util::STACK_HTTP_SNAPSHOT_WORKER),
        "http_chat_history_exec" => Some(crate::util::STACK_HTTP_CHAT_HISTORY_WORKER),
        "http_config_exec" => Some(crate::util::STACK_HTTP_CONFIG_WORKER),
        "http_diag_exec" => Some(crate::util::STACK_HTTP_DIAG_WORKER),
        "qq_ws" | "feishu_ws" | "wecom_aibot" | "dingtalk_stream" => {
            Some(crate::util::STACK_CHANNEL_WS)
        }
        "qq_sender" | "tg_sender" | "fs_sender" | "dt_sender" | "wc_sender" | "tg_poll" => {
            Some(crate::util::STACK_CHANNEL_SENDER)
        }
        "os_outbound" => Some(crate::util::STACK_OS_OUTBOUND),
        "os_outbound_supervisor" => Some(crate::util::STACK_DISPATCH),
        "dispatch" => Some(crate::util::STACK_DISPATCH),
        "agent_loop" => Some(crate::util::STACK_AGENT_LOOP),
        "display" => Some(crate::util::STACK_DISPLAY),
        "voice_session" => Some(crate::util::STACK_VOICE_CONTROL),
        "voice_session_worker" => Some(crate::util::STACK_VOICE_SESSION),
        "voice_realtime" => Some(crate::util::STACK_VOICE_REALTIME),
        "voice_realtime_connect" => Some(crate::util::STACK_VOICE_REALTIME_CONNECT),
        "write_back" => Some(crate::runtime::write_back::WRITE_BACK_WORKER_STACK),
        "wifi_worker" => Some(crate::util::STACK_WIFI_WORKER),
        "audio_io_worker" => Some(crate::util::STACK_AUDIO_IO_STD_COMPAT),
        "bg_timer" => Some(crate::util::STACK_BG_TIMER),
        "sntp" => Some(crate::util::STACK_SNTP_WORKER),
        "cli_repl" => Some(crate::util::STACK_CLI_REPL),
        _ => None,
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

    #[test]
    fn snapshot_route_worker_has_explicit_core_and_io_role() {
        let plan = super::thread_plan("http_snapshot_exec");

        assert_eq!(plan.core, Some(crate::util::SpawnCore::Core1));
        assert_eq!(plan.role, crate::util::HttpThreadRole::Io);
        assert_eq!(
            super::stack_budget_for_thread("http_snapshot_exec"),
            Some(crate::util::STACK_HTTP_SNAPSHOT_WORKER)
        );
    }

    #[test]
    fn startup_io_workers_have_explicit_core_and_io_role() {
        for name in [
            "wifi_worker",
            "config_plane_watch",
            "os_outbound_supervisor",
        ] {
            let plan = super::thread_plan(name);

            assert_eq!(plan.core, Some(crate::util::SpawnCore::Core0));
            assert_eq!(plan.role, crate::util::HttpThreadRole::Io);
            assert!(super::stack_budget_for_thread(name).is_some());
        }
    }
}
