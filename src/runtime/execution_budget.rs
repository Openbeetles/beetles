//! Runtime execution budget projection.
//! 将 plane/thread/transport 真源收口成可观测预算快照，供 heartbeat 与内部诊断使用。

use crate::runtime::mode::RuntimeMode;
use crate::runtime::plane::{self, PlaneResidency};
use crate::runtime::thread_registry;

/// Static runtime execution-budget snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ExecutionBudgetSnapshot {
    pub steady_stack_bytes: usize,
    pub lazy_stack_bytes: usize,
    pub transient_stack_bytes: usize,
    pub tls_capable_stack_bytes: usize,
    pub http_capable_stack_bytes: usize,
    pub wss_capable_stack_bytes: usize,
    pub largest_thread_stack_bytes: usize,
    pub budgeted_thread_count: usize,
    pub logical_thread_count: usize,
    pub unmapped_thread_count: usize,
    pub tls_floor_internal_bytes: usize,
    pub tls_floor_largest_block_bytes: usize,
    pub max_concurrent_http: usize,
    pub active_http_count: u32,
    pub active_wss_count: u32,
    pub external_wss_connecting_count: u32,
    pub current_mode: RuntimeMode,
    pub allow_non_voice_outbound: bool,
    pub allow_external_wss_connect: bool,
    pub require_external_wss_suspended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ThreadStackBudget {
    name: &'static str,
    stack_budget_bytes: usize,
}

const THREAD_STACK_BUDGETS: &[ThreadStackBudget] = &[
    ThreadStackBudget {
        name: "runtime_bootstrap",
        stack_budget_bytes: crate::util::STACK_ESP_RUNTIME_BOOT,
    },
    ThreadStackBudget {
        name: "startup_recovery",
        stack_budget_bytes: crate::util::STACK_STARTUP_RECOVERY,
    },
    ThreadStackBudget {
        name: "config_plane_watch",
        stack_budget_bytes: crate::util::STACK_CONFIG_PLANE_WATCH,
    },
    ThreadStackBudget {
        name: "http_snapshot_exec",
        stack_budget_bytes: crate::util::STACK_HTTP_SNAPSHOT_WORKER,
    },
    ThreadStackBudget {
        name: "http_config_exec",
        stack_budget_bytes: crate::util::STACK_HTTP_CONFIG_WORKER,
    },
    ThreadStackBudget {
        name: "http_diag_exec",
        stack_budget_bytes: crate::util::STACK_HTTP_DIAG_WORKER,
    },
    ThreadStackBudget {
        name: "qq_ws",
        stack_budget_bytes: crate::util::STACK_CHANNEL_WS,
    },
    ThreadStackBudget {
        name: "feishu_ws",
        stack_budget_bytes: crate::util::STACK_CHANNEL_WS,
    },
    ThreadStackBudget {
        name: "wecom_aibot",
        stack_budget_bytes: crate::util::STACK_CHANNEL_WS,
    },
    ThreadStackBudget {
        name: "dingtalk_stream",
        stack_budget_bytes: crate::util::STACK_CHANNEL_WS,
    },
    ThreadStackBudget {
        name: "qq_sender",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "tg_sender",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "fs_sender",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "dt_sender",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "wc_sender",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "tg_poll",
        stack_budget_bytes: crate::util::STACK_CHANNEL_SENDER,
    },
    ThreadStackBudget {
        name: "os_outbound",
        stack_budget_bytes: crate::util::STACK_OS_OUTBOUND,
    },
    ThreadStackBudget {
        name: "dispatch",
        stack_budget_bytes: crate::util::STACK_DISPATCH,
    },
    ThreadStackBudget {
        name: "agent_loop",
        stack_budget_bytes: crate::util::STACK_AGENT_LOOP,
    },
    ThreadStackBudget {
        name: "display",
        stack_budget_bytes: crate::util::STACK_DISPLAY,
    },
    ThreadStackBudget {
        name: "voice_session",
        stack_budget_bytes: crate::util::STACK_VOICE_CONTROL,
    },
    ThreadStackBudget {
        name: "voice_session_worker",
        stack_budget_bytes: crate::util::STACK_VOICE_SESSION,
    },
    ThreadStackBudget {
        name: "voice_realtime",
        stack_budget_bytes: crate::util::STACK_VOICE_REALTIME,
    },
    ThreadStackBudget {
        name: "voice_realtime_connect",
        stack_budget_bytes: crate::util::STACK_CHANNEL_WS,
    },
    ThreadStackBudget {
        name: "write_back",
        stack_budget_bytes: crate::runtime::write_back::WRITE_BACK_WORKER_STACK,
    },
    ThreadStackBudget {
        name: "wifi_worker",
        stack_budget_bytes: crate::util::STACK_WIFI_WORKER,
    },
    ThreadStackBudget {
        name: "audio_io_worker",
        stack_budget_bytes: crate::util::STACK_AUDIO_IO_STD_COMPAT,
    },
    ThreadStackBudget {
        name: "bg_timer",
        stack_budget_bytes: crate::util::STACK_BG_TIMER,
    },
    ThreadStackBudget {
        name: "sntp",
        stack_budget_bytes: crate::util::STACK_SNTP_WORKER,
    },
    ThreadStackBudget {
        name: "cli_repl",
        stack_budget_bytes: crate::util::STACK_CLI_REPL,
    },
];

const LOGICAL_THREAD_OWNERS: &[&str] = &["http_server", "heartbeat", "cron", "remind"];

/// Return the declared stack budget for a runtime plane thread name.
pub fn stack_budget_for_thread(name: &str) -> Option<usize> {
    THREAD_STACK_BUDGETS
        .iter()
        .find(|budget| budget.name == name)
        .map(|budget| budget.stack_budget_bytes)
}

fn is_logical_thread_owner(name: &str) -> bool {
    LOGICAL_THREAD_OWNERS.contains(&name)
}

/// Build the current execution-budget projection from plane, stack and transport truth sources.
pub fn static_budget_snapshot() -> ExecutionBudgetSnapshot {
    let mut steady_stack_bytes = 0usize;
    let mut lazy_stack_bytes = 0usize;
    let mut transient_stack_bytes = 0usize;
    let mut tls_capable_stack_bytes = 0usize;
    let mut http_capable_stack_bytes = 0usize;
    let mut wss_capable_stack_bytes = 0usize;
    let mut largest_thread_stack_bytes = 0usize;
    let mut budgeted_thread_count = 0usize;
    let mut logical_thread_count = 0usize;
    let mut unmapped_thread_count = 0usize;

    for profile in plane::profiles() {
        for thread_name in profile.thread_names {
            let Some(stack_budget_bytes) = stack_budget_for_thread(thread_name) else {
                if is_logical_thread_owner(thread_name) {
                    logical_thread_count = logical_thread_count.saturating_add(1);
                } else {
                    unmapped_thread_count = unmapped_thread_count.saturating_add(1);
                }
                continue;
            };

            budgeted_thread_count = budgeted_thread_count.saturating_add(1);
            largest_thread_stack_bytes = largest_thread_stack_bytes.max(stack_budget_bytes);
            match profile.residency {
                PlaneResidency::Steady => {
                    steady_stack_bytes = steady_stack_bytes.saturating_add(stack_budget_bytes)
                }
                PlaneResidency::Lazy => {
                    lazy_stack_bytes = lazy_stack_bytes.saturating_add(stack_budget_bytes)
                }
                PlaneResidency::Transient => {
                    transient_stack_bytes = transient_stack_bytes.saturating_add(stack_budget_bytes)
                }
            }
            if profile.tls_capable {
                tls_capable_stack_bytes =
                    tls_capable_stack_bytes.saturating_add(stack_budget_bytes);
            }
            if profile.http_capable {
                http_capable_stack_bytes =
                    http_capable_stack_bytes.saturating_add(stack_budget_bytes);
            }
            if profile.wss_capable {
                wss_capable_stack_bytes =
                    wss_capable_stack_bytes.saturating_add(stack_budget_bytes);
            }
        }
    }

    let mode = thread_registry::runtime_mode_snapshot();
    ExecutionBudgetSnapshot {
        steady_stack_bytes,
        lazy_stack_bytes,
        transient_stack_bytes,
        tls_capable_stack_bytes,
        http_capable_stack_bytes,
        wss_capable_stack_bytes,
        largest_thread_stack_bytes,
        budgeted_thread_count,
        logical_thread_count,
        unmapped_thread_count,
        tls_floor_internal_bytes: crate::constants::TLS_ADMISSION_MIN_INTERNAL_BYTES,
        tls_floor_largest_block_bytes: crate::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
        max_concurrent_http: crate::constants::MAX_CONCURRENT_HTTP,
        active_http_count: crate::network::active_http_count(),
        active_wss_count: crate::network::active_wss_count(),
        external_wss_connecting_count: crate::network::external_wss_connecting_count(),
        current_mode: mode.current_mode,
        allow_non_voice_outbound: mode.action_budget.allow_non_voice_outbound,
        allow_external_wss_connect: mode.action_budget.allow_external_wss_connect,
        require_external_wss_suspended: mode.action_budget.require_external_wss_suspended,
    }
}

/// Return the compact execution-budget baseline line used by heartbeat logs.
pub fn format_baseline_log_line() -> String {
    let snapshot = static_budget_snapshot();
    format!(
        "execution_budget steady_stack={} lazy_stack={} transient_stack={} tls_stack={} http_stack={} wss_stack={} largest_thread_stack={} budgeted_threads={} logical_threads={} unmapped_threads={} tls_floor_internal={} tls_floor_largest={} max_http={} active_http={} active_wss={} ext_wss_connecting={} mode={} non_voice_outbound={} ext_wss_connect={} ext_wss_suspend={}",
        snapshot.steady_stack_bytes,
        snapshot.lazy_stack_bytes,
        snapshot.transient_stack_bytes,
        snapshot.tls_capable_stack_bytes,
        snapshot.http_capable_stack_bytes,
        snapshot.wss_capable_stack_bytes,
        snapshot.largest_thread_stack_bytes,
        snapshot.budgeted_thread_count,
        snapshot.logical_thread_count,
        snapshot.unmapped_thread_count,
        snapshot.tls_floor_internal_bytes,
        snapshot.tls_floor_largest_block_bytes,
        snapshot.max_concurrent_http,
        snapshot.active_http_count,
        snapshot.active_wss_count,
        snapshot.external_wss_connecting_count,
        snapshot.current_mode.as_str(),
        snapshot.allow_non_voice_outbound,
        snapshot.allow_external_wss_connect,
        snapshot.require_external_wss_suspended,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_budget_baseline_reports_mode_and_unmapped_count() {
        let line = format_baseline_log_line();

        assert!(line.contains("execution_budget steady_stack="));
        assert!(line.contains("unmapped_threads=0"));
        assert!(line.contains("mode="));
    }
}
