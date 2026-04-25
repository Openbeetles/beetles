//! 线程注册与 SRAM 审计快照。
//! Thread registry and SRAM audit snapshots for runtime observability.

use crate::orchestrator::HttpThreadRole;
use crate::platform::task_affinity::TaskSpawnSurface;
use crate::util::SpawnCore;
use std::sync::{Mutex, OnceLock};

const LOW_STACK_MARGIN_BYTES: usize = 2 * 1024;

#[derive(Clone)]
struct ThreadEntry {
    name: String,
    stack_size: usize,
    core_target: Option<SpawnCore>,
    role: HttpThreadRole,
    spawn_surface: TaskSpawnSurface,
    starts: u32,
    alive: bool,
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    task_handle_key: usize,
}

#[derive(Clone, Copy)]
struct ThreadProfile {
    execution_class: ThreadExecutionClass,
    risk_class: ThreadRiskClass,
    tls_capable: bool,
    http_capable: bool,
    wss_capable: bool,
    mode_sensitive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// 线程执行面分类，用于识别当前运行态到底在常驻哪些平面。
pub enum ThreadExecutionClass {
    Agent,
    Channel,
    Voice,
    Config,
    Platform,
    Runtime,
    Ui,
    Unknown,
}

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
/// 线程风险等级，反映其对 ESP SRAM / TLS / 模式冲突的潜在影响。
pub enum ThreadRiskClass {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// 线程绑核快照表示。
pub enum ThreadCoreTarget {
    Core0,
    Core1,
    Unpinned,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
/// 线程角色快照，对齐 orchestrator TLS admission 角色。
pub enum ThreadRoleSnapshot {
    Interactive,
    Io,
    Background,
}

#[derive(Clone, serde::Serialize)]
/// 单条线程的运行时审计快照。
pub struct ThreadRuntimeSnapshot {
    pub name: String,
    pub starts: u32,
    pub alive: bool,
    pub stack_budget_bytes: usize,
    pub stack_high_water_free_bytes: Option<usize>,
    pub stack_high_water_used_bytes: Option<usize>,
    pub stack_margin_percent: Option<u8>,
    pub core_target: ThreadCoreTarget,
    pub role: ThreadRoleSnapshot,
    pub spawn_surface: TaskSpawnSurface,
    pub native_allowlist_hit: bool,
    pub native_std_sync_forbidden: bool,
    pub task_wdt_policy: crate::platform::task_wdt::TaskWdtThreadPolicy,
    pub execution_class: ThreadExecutionClass,
    pub risk_class: ThreadRiskClass,
    pub tls_capable: bool,
    pub http_capable: bool,
    pub wss_capable: bool,
    pub mode_sensitive: bool,
}

#[derive(Clone, serde::Serialize)]
/// 全量线程注册表快照，含汇总计数与明细。
pub struct ThreadRegistrySnapshot {
    pub alive_threads: usize,
    pub historical_threads: usize,
    pub total_stack_bytes: usize,
    pub io_threads: usize,
    pub interactive_threads: usize,
    pub background_threads: usize,
    pub core0_threads: usize,
    pub core1_threads: usize,
    pub unpinned_threads: usize,
    pub std_thread_compat_threads: usize,
    pub esp_native_task_threads: usize,
    pub esp_native_allowlist_hit_threads: usize,
    pub esp_native_std_sync_forbidden_threads: usize,
    pub task_wdt_owner_threads: usize,
    pub task_wdt_feed_only_threads: usize,
    pub task_wdt_unmanaged_threads: usize,
    pub tls_capable_threads: usize,
    pub http_capable_threads: usize,
    pub wss_capable_threads: usize,
    pub mode_sensitive_threads: usize,
    pub high_risk_threads: usize,
    pub critical_risk_threads: usize,
    pub low_stack_margin_threads: usize,
    pub stack_high_water_supported: bool,
    pub stack_high_water_sampled_threads: usize,
    pub details: Vec<ThreadRuntimeSnapshot>,
}

pub type RuntimeModeSnapshot = crate::runtime::mode::RuntimeModeSnapshot;

#[derive(Clone, Copy, Default)]
pub(crate) struct RuntimePlaneFlags {
    pub channel_plane_alive: bool,
    pub voice_plane_alive: bool,
    pub agent_plane_alive: bool,
}

static THREADS: OnceLock<Mutex<Vec<ThreadEntry>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<ThreadEntry>> {
    THREADS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 注册一个线程进入运行时注册表；同名线程重启时会刷新 starts 与当前画像。
pub fn register_thread(
    name: &str,
    stack_size: usize,
    core_target: Option<SpawnCore>,
    role: HttpThreadRole,
    spawn_surface: TaskSpawnSurface,
) {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.iter_mut().find(|entry| entry.name == name) {
        entry.stack_size = stack_size;
        entry.core_target = core_target;
        entry.role = role;
        entry.spawn_surface = spawn_surface;
        entry.starts = entry.starts.saturating_add(1);
        entry.alive = true;
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            entry.task_handle_key = current_task_handle_key();
        }
        return;
    }
    guard.push(ThreadEntry {
        name: name.to_string(),
        stack_size,
        core_target,
        role,
        spawn_surface,
        starts: 1,
        alive: true,
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        task_handle_key: current_task_handle_key(),
    });
}

/// 标记线程已停止，后续快照不再采样其运行态数据。
pub fn mark_thread_stopped(name: &str) {
    let mut guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = guard.iter_mut().find(|entry| entry.name == name) {
        entry.alive = false;
    }
}

/// 返回完整线程审计快照；供 `/api/health` 与诊断路径使用。
pub fn snapshot() -> ThreadRegistrySnapshot {
    build_snapshot(true)
}

/// 返回当前运行模式快照；用于识别 config/channel/voice/agent 平面是否常驻。
pub fn runtime_mode_snapshot() -> RuntimeModeSnapshot {
    crate::runtime::mode::snapshot_from_source(runtime_mode_source())
}

/// 返回未做额外平台补充的运行模式源信号。
///
/// 该函数只聚合线程注册表与全局运行态布尔值。
/// 若调用方需要把 pairing 等平台语义纳入同一 mode contract，
/// 应在此基础上补齐对应 source 字段，再交给 `snapshot_from_source(...)` 收口。
pub fn runtime_mode_source() -> crate::runtime::mode::RuntimeModeSource {
    let plane = runtime_plane_flags();
    let ext_wss = crate::network::external_wss_runtime_snapshot();
    let config_activity = crate::runtime::governance::config_activity_snapshot();
    crate::runtime::mode::RuntimeModeSource {
        wifi_sta_connected: crate::state::wifi_sta_connected(),
        boot_phase_active: crate::state::boot_phase_active(),
        pairing_required: crate::state::pairing_required(),
        pairing_state_known: crate::state::pairing_state_known(),
        voice_exclusive_active: crate::state::voice_exclusive_active(),
        background_maintenance_active: crate::state::background_maintenance_active(),
        config_plane_alive: crate::state::config_plane_active(),
        config_active: config_activity.active,
        config_activity_phase: config_activity.phase,
        channel_plane_alive: plane.channel_plane_alive,
        voice_plane_alive: plane.voice_plane_alive,
        agent_plane_alive: plane.agent_plane_alive,
        external_wss_managed_present: ext_wss.managed_present,
        external_wss_suspend_requested: ext_wss.suspend_requested,
        external_wss_suspended: ext_wss.suspended,
        recovery_safe_mode_active: crate::state::recovery_safe_mode_active(),
    }
}

fn runtime_plane_flags() -> RuntimePlaneFlags {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let channel_plane_alive = guard.iter().any(|entry| {
        entry.alive
            && thread_profile(entry.name.as_str()).execution_class == ThreadExecutionClass::Channel
    });
    let voice_plane_alive = guard.iter().any(|entry| {
        entry.alive
            && thread_profile(entry.name.as_str()).execution_class == ThreadExecutionClass::Voice
    });
    let agent_plane_alive = guard
        .iter()
        .any(|entry| entry.alive && entry.name == "agent_loop");
    RuntimePlaneFlags {
        channel_plane_alive,
        voice_plane_alive,
        agent_plane_alive,
    }
}

/// 返回线程汇总基线日志行。
pub fn format_baseline_log_line() -> String {
    let snapshot = build_snapshot(false);
    format!(
        "threads alive={} historical={} stack_total={} io={} interactive={} background={} core0={} core1={} unpinned={} std_compat={} native={} native_allowlist_hits={} native_std_sync_forbidden={} twdt_owner={} twdt_feed_only={} twdt_unmanaged={} tls={} http={} wss={} mode_sensitive={} high_risk={} critical={} low_margin={} hw_samples={}",
        snapshot.alive_threads,
        snapshot.historical_threads,
        snapshot.total_stack_bytes,
        snapshot.io_threads,
        snapshot.interactive_threads,
        snapshot.background_threads,
        snapshot.core0_threads,
        snapshot.core1_threads,
        snapshot.unpinned_threads,
        snapshot.std_thread_compat_threads,
        snapshot.esp_native_task_threads,
        snapshot.esp_native_allowlist_hit_threads,
        snapshot.esp_native_std_sync_forbidden_threads,
        snapshot.task_wdt_owner_threads,
        snapshot.task_wdt_feed_only_threads,
        snapshot.task_wdt_unmanaged_threads,
        snapshot.tls_capable_threads,
        snapshot.http_capable_threads,
        snapshot.wss_capable_threads,
        snapshot.mode_sensitive_threads,
        snapshot.high_risk_threads,
        snapshot.critical_risk_threads,
        snapshot.low_stack_margin_threads,
        snapshot.stack_high_water_sampled_threads,
    )
}

/// 返回线程栈风险日志行，包含 ESP stack high-water 采样摘要。
pub fn format_stack_risk_log_line() -> String {
    let snapshot = build_snapshot(true);
    let mut top = Vec::new();
    for detail in snapshot.details.iter().take(3) {
        let margin = detail
            .stack_high_water_free_bytes
            .map(|bytes| bytes.to_string())
            .unwrap_or_else(|| "n/a".to_string());
        top.push(format!(
            "{}:{:?}:budget={} free={}",
            detail.name, detail.risk_class, detail.stack_budget_bytes, margin
        ));
    }
    format!(
        "thread_stack stack_hw_supported={} sampled={} low_margin={} top={}",
        snapshot.stack_high_water_supported,
        snapshot.stack_high_water_sampled_threads,
        snapshot.low_stack_margin_threads,
        if top.is_empty() {
            "none".to_string()
        } else {
            top.join(",")
        }
    )
}

/// 返回当前运行模式日志行。
pub fn format_runtime_mode_log_line() -> String {
    let mode = runtime_mode_snapshot();
    format!(
        "runtime_mode current_mode={} wifi_sta={} booting={} pairing_known={} pairing_required={} voice_exclusive={} bg_maintenance={} recovery_safe_mode={} config_plane={} config_active={} config_phase={} channel_plane={} voice_plane={} agent_plane={} ext_wss_connecting={} timers={} periodic_maintenance={} non_voice_outbound={} realtime_voice={} ext_wss_connect={} ext_wss_suspend={}",
        mode.current_mode.as_str(),
        mode.wifi_sta_connected,
        mode.boot_phase_active,
        mode.pairing_state_known,
        mode.pairing_required,
        mode.voice_exclusive_active,
        mode.background_maintenance_active,
        mode.recovery_safe_mode_active,
        mode.config_plane_alive,
        mode.config_active,
        mode.config_activity_phase.as_str(),
        mode.channel_plane_alive,
        mode.voice_plane_alive,
        mode.agent_plane_alive,
        crate::network::external_wss_connecting_count(),
        mode.action_budget.allow_due_user_timers,
        mode.action_budget.allow_periodic_maintenance,
        mode.action_budget.allow_non_voice_outbound,
        mode.action_budget.allow_realtime_voice_connect,
        mode.action_budget.allow_external_wss_connect,
        mode.action_budget.require_external_wss_suspended,
    )
}

fn build_snapshot(include_details: bool) -> ThreadRegistrySnapshot {
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let mut alive_threads = 0usize;
    let mut total_stack_bytes = 0usize;
    let mut io_threads = 0usize;
    let mut interactive_threads = 0usize;
    let mut background_threads = 0usize;
    let mut core0_threads = 0usize;
    let mut core1_threads = 0usize;
    let mut unpinned_threads = 0usize;
    let mut std_thread_compat_threads = 0usize;
    let mut esp_native_task_threads = 0usize;
    let mut esp_native_allowlist_hit_threads = 0usize;
    let mut esp_native_std_sync_forbidden_threads = 0usize;
    let mut task_wdt_owner_threads = 0usize;
    let mut task_wdt_feed_only_threads = 0usize;
    let mut task_wdt_unmanaged_threads = 0usize;
    let mut tls_capable_threads = 0usize;
    let mut http_capable_threads = 0usize;
    let mut wss_capable_threads = 0usize;
    let mut mode_sensitive_threads = 0usize;
    let mut high_risk_threads = 0usize;
    let mut critical_risk_threads = 0usize;
    let mut low_stack_margin_threads = 0usize;
    let mut stack_high_water_sampled_threads = 0usize;
    let mut details = Vec::new();

    for entry in guard.iter().filter(|entry| entry.alive) {
        let profile = thread_profile(entry.name.as_str());
        let task_wdt_policy =
            crate::platform::task_wdt::thread_policy_for_name(entry.name.as_str());
        let stack_free = sample_stack_high_water_free_bytes(entry)
            .map(|free| normalize_stack_high_water_free_bytes(entry.stack_size, free));
        let stack_used = stack_free.map(|free| entry.stack_size.saturating_sub(free));
        let stack_margin_percent = stack_free.map(|free| {
            if entry.stack_size == 0 {
                0
            } else {
                ((free as u64 * 100) / entry.stack_size as u64).min(100) as u8
            }
        });

        alive_threads += 1;
        total_stack_bytes = total_stack_bytes.saturating_add(entry.stack_size);
        if profile.tls_capable {
            tls_capable_threads += 1;
        }
        if profile.http_capable {
            http_capable_threads += 1;
        }
        if profile.wss_capable {
            wss_capable_threads += 1;
        }
        if profile.mode_sensitive {
            mode_sensitive_threads += 1;
        }
        if profile.risk_class >= ThreadRiskClass::High {
            high_risk_threads += 1;
        }
        if profile.risk_class == ThreadRiskClass::Critical {
            critical_risk_threads += 1;
        }
        if let Some(free) = stack_free {
            stack_high_water_sampled_threads += 1;
            if free <= LOW_STACK_MARGIN_BYTES {
                low_stack_margin_threads += 1;
            }
        }

        match entry.role {
            HttpThreadRole::Io => io_threads += 1,
            HttpThreadRole::Interactive => interactive_threads += 1,
            HttpThreadRole::Background => background_threads += 1,
        }
        match entry.core_target {
            Some(SpawnCore::Core0) => core0_threads += 1,
            Some(SpawnCore::Core1) => core1_threads += 1,
            None => unpinned_threads += 1,
        }
        match entry.spawn_surface {
            TaskSpawnSurface::StdThreadCompat => std_thread_compat_threads += 1,
            TaskSpawnSurface::EspNativeTask => {
                esp_native_task_threads += 1;
                esp_native_std_sync_forbidden_threads += 1;
            }
        }
        let native_allowlist_hit =
            crate::platform::task_affinity::native_task_allowlist_hit(entry.name.as_str());
        if native_allowlist_hit {
            esp_native_allowlist_hit_threads += 1;
        }
        match task_wdt_policy {
            crate::platform::task_wdt::TaskWdtThreadPolicy::Owner => task_wdt_owner_threads += 1,
            crate::platform::task_wdt::TaskWdtThreadPolicy::FeedOnly => {
                task_wdt_feed_only_threads += 1
            }
            crate::platform::task_wdt::TaskWdtThreadPolicy::Unmanaged => {
                task_wdt_unmanaged_threads += 1
            }
        }

        if include_details {
            details.push(ThreadRuntimeSnapshot {
                name: entry.name.clone(),
                starts: entry.starts,
                alive: entry.alive,
                stack_budget_bytes: entry.stack_size,
                stack_high_water_free_bytes: stack_free,
                stack_high_water_used_bytes: stack_used,
                stack_margin_percent,
                core_target: core_target_snapshot(entry.core_target),
                role: role_snapshot(entry.role),
                spawn_surface: entry.spawn_surface,
                native_allowlist_hit,
                native_std_sync_forbidden: matches!(
                    entry.spawn_surface,
                    TaskSpawnSurface::EspNativeTask
                ),
                task_wdt_policy,
                execution_class: profile.execution_class,
                risk_class: profile.risk_class,
                tls_capable: profile.tls_capable,
                http_capable: profile.http_capable,
                wss_capable: profile.wss_capable,
                mode_sensitive: profile.mode_sensitive,
            });
        }
    }

    if include_details {
        details.sort_by(|left, right| {
            right
                .risk_class
                .cmp(&left.risk_class)
                .then_with(|| {
                    left.stack_margin_percent
                        .unwrap_or(255)
                        .cmp(&right.stack_margin_percent.unwrap_or(255))
                })
                .then_with(|| right.stack_budget_bytes.cmp(&left.stack_budget_bytes))
                .then_with(|| left.name.cmp(&right.name))
        });
    }

    ThreadRegistrySnapshot {
        alive_threads,
        historical_threads: guard.len(),
        total_stack_bytes,
        io_threads,
        interactive_threads,
        background_threads,
        core0_threads,
        core1_threads,
        unpinned_threads,
        std_thread_compat_threads,
        esp_native_task_threads,
        esp_native_allowlist_hit_threads,
        esp_native_std_sync_forbidden_threads,
        task_wdt_owner_threads,
        task_wdt_feed_only_threads,
        task_wdt_unmanaged_threads,
        tls_capable_threads,
        http_capable_threads,
        wss_capable_threads,
        mode_sensitive_threads,
        high_risk_threads,
        critical_risk_threads,
        low_stack_margin_threads,
        stack_high_water_supported: cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
        stack_high_water_sampled_threads,
        details,
    }
}

fn core_target_snapshot(core_target: Option<SpawnCore>) -> ThreadCoreTarget {
    match core_target {
        Some(SpawnCore::Core0) => ThreadCoreTarget::Core0,
        Some(SpawnCore::Core1) => ThreadCoreTarget::Core1,
        None => ThreadCoreTarget::Unpinned,
    }
}

fn role_snapshot(role: HttpThreadRole) -> ThreadRoleSnapshot {
    match role {
        HttpThreadRole::Interactive => ThreadRoleSnapshot::Interactive,
        HttpThreadRole::Io => ThreadRoleSnapshot::Io,
        HttpThreadRole::Background => ThreadRoleSnapshot::Background,
    }
}

fn thread_profile(name: &str) -> ThreadProfile {
    match name {
        "agent_loop" => ThreadProfile {
            execution_class: ThreadExecutionClass::Agent,
            risk_class: ThreadRiskClass::Critical,
            tls_capable: true,
            http_capable: true,
            wss_capable: false,
            mode_sensitive: true,
        },
        "voice_session" | "voice_session_worker" | "voice_realtime" | "voice_realtime_connect" => {
            ThreadProfile {
                execution_class: ThreadExecutionClass::Voice,
                risk_class: ThreadRiskClass::Critical,
                tls_capable: true,
                http_capable: true,
                wss_capable: true,
                mode_sensitive: true,
            }
        }
        "qq_ws" | "feishu_ws" | "wecom_aibot" | "dingtalk_stream" => ThreadProfile {
            execution_class: ThreadExecutionClass::Channel,
            risk_class: ThreadRiskClass::Critical,
            tls_capable: true,
            http_capable: true,
            wss_capable: true,
            mode_sensitive: true,
        },
        "qq_sender" | "tg_sender" | "fs_sender" | "dt_sender" | "wc_sender" | "tg_poll"
        | "os_outbound" => ThreadProfile {
            execution_class: ThreadExecutionClass::Channel,
            risk_class: ThreadRiskClass::High,
            tls_capable: true,
            http_capable: true,
            wss_capable: false,
            mode_sensitive: true,
        },
        "http_snapshot_exec" => ThreadProfile {
            execution_class: ThreadExecutionClass::Config,
            risk_class: ThreadRiskClass::Medium,
            tls_capable: false,
            http_capable: true,
            wss_capable: false,
            mode_sensitive: true,
        },
        "http_config_exec" | "http_diag_exec" | "http_ota_exec" => ThreadProfile {
            execution_class: ThreadExecutionClass::Config,
            risk_class: ThreadRiskClass::High,
            tls_capable: true,
            http_capable: true,
            wss_capable: false,
            mode_sensitive: true,
        },
        "config_plane_watch" => ThreadProfile {
            execution_class: ThreadExecutionClass::Runtime,
            risk_class: ThreadRiskClass::Low,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
        "wifi_worker" => ThreadProfile {
            execution_class: ThreadExecutionClass::Platform,
            risk_class: ThreadRiskClass::High,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
        "audio_io_worker" => ThreadProfile {
            execution_class: ThreadExecutionClass::Platform,
            risk_class: ThreadRiskClass::Medium,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
        "dispatch" => ThreadProfile {
            execution_class: ThreadExecutionClass::Runtime,
            risk_class: ThreadRiskClass::High,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: true,
        },
        "bg_timer" | "heartbeat" | "restart_defer" | "cron" | "remind" => ThreadProfile {
            execution_class: ThreadExecutionClass::Runtime,
            risk_class: ThreadRiskClass::Low,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
        "display" => ThreadProfile {
            execution_class: ThreadExecutionClass::Ui,
            risk_class: ThreadRiskClass::Low,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
        _ => ThreadProfile {
            execution_class: ThreadExecutionClass::Unknown,
            risk_class: ThreadRiskClass::Medium,
            tls_capable: false,
            http_capable: false,
            wss_capable: false,
            mode_sensitive: false,
        },
    }
}

fn normalize_stack_high_water_free_bytes(
    stack_budget_bytes: usize,
    sampled_free_bytes: usize,
) -> usize {
    sampled_free_bytes.min(stack_budget_bytes)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn current_task_handle_key() -> usize {
    esp_idf_hal::task::current()
        .map(|handle| handle as usize)
        .unwrap_or(0)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn sample_stack_high_water_free_bytes(entry: &ThreadEntry) -> Option<usize> {
    unsafe extern "C" {
        fn uxTaskGetStackHighWaterMark(task: esp_idf_hal::sys::TaskHandle_t) -> u32;
    }

    if entry.task_handle_key == 0 {
        return None;
    }
    let bytes = unsafe {
        uxTaskGetStackHighWaterMark(entry.task_handle_key as esp_idf_hal::sys::TaskHandle_t)
    } as usize;
    Some(bytes)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn sample_stack_high_water_free_bytes(_entry: &ThreadEntry) -> Option<usize> {
    None
}

#[cfg(test)]
pub fn reset_for_tests() {
    registry().lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn registry_test_guard() -> MutexGuard<'static, ()> {
        let guard = REGISTRY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_for_tests();
        guard
    }

    #[test]
    fn baseline_log_line_reports_historical_threads_separately() {
        let _guard = registry_test_guard();
        register_thread(
            "dispatch",
            8192,
            None,
            HttpThreadRole::Io,
            TaskSpawnSurface::StdThreadCompat,
        );
        register_thread(
            "http_config_exec",
            32768,
            None,
            HttpThreadRole::Io,
            TaskSpawnSurface::StdThreadCompat,
        );
        mark_thread_stopped("http_config_exec");

        let snapshot = snapshot();
        assert_eq!(snapshot.alive_threads, 1);
        assert_eq!(snapshot.historical_threads, 2);
        assert_eq!(snapshot.task_wdt_feed_only_threads, 1);
        assert_eq!(snapshot.task_wdt_unmanaged_threads, 0);

        let line = format_baseline_log_line();
        assert!(line.contains("alive=1"));
        assert!(line.contains("historical=2"));
        assert!(line.contains("std_compat=1"));
        assert!(line.contains("native=0"));
        assert!(line.contains("native_std_sync_forbidden=0"));
        assert!(line.contains("twdt_feed_only=1"));
        assert!(!line.contains("registered="));

        reset_for_tests();
    }

    #[test]
    fn native_surface_details_mark_std_sync_forbidden() {
        let _guard = registry_test_guard();
        register_thread(
            "native_probe",
            4096,
            Some(SpawnCore::Core0),
            HttpThreadRole::Background,
            TaskSpawnSurface::EspNativeTask,
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.esp_native_task_threads, 1);
        assert_eq!(snapshot.esp_native_std_sync_forbidden_threads, 1);
        assert_eq!(snapshot.std_thread_compat_threads, 0);
        assert_eq!(snapshot.details.len(), 1);
        assert!(snapshot.details[0].native_std_sync_forbidden);

        reset_for_tests();
    }

    #[test]
    fn snapshot_counts_task_wdt_owner_and_feed_only_threads() {
        let _guard = registry_test_guard();
        register_thread(
            "agent_loop",
            32768,
            Some(SpawnCore::Core1),
            HttpThreadRole::Interactive,
            TaskSpawnSurface::StdThreadCompat,
        );
        register_thread(
            "http_config_exec",
            32768,
            Some(SpawnCore::Core0),
            HttpThreadRole::Io,
            TaskSpawnSurface::StdThreadCompat,
        );
        register_thread(
            "config_plane_watch",
            8192,
            Some(SpawnCore::Core1),
            HttpThreadRole::Background,
            TaskSpawnSurface::StdThreadCompat,
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.task_wdt_owner_threads, 1);
        assert_eq!(snapshot.task_wdt_feed_only_threads, 1);
        assert_eq!(snapshot.task_wdt_unmanaged_threads, 1);
        assert_eq!(
            snapshot.details[0].task_wdt_policy,
            crate::platform::task_wdt::TaskWdtThreadPolicy::Owner
        );

        let line = format_baseline_log_line();
        assert!(line.contains("twdt_owner=1"));
        assert!(line.contains("twdt_feed_only=1"));
        assert!(line.contains("twdt_unmanaged=1"));

        reset_for_tests();
    }

    #[test]
    fn snapshot_route_worker_is_tracked_as_non_tls_config_plane() {
        let _guard = registry_test_guard();
        register_thread(
            "http_snapshot_exec",
            crate::util::STACK_HTTP_SNAPSHOT_WORKER,
            Some(SpawnCore::Core1),
            HttpThreadRole::Io,
            TaskSpawnSurface::StdThreadCompat,
        );

        let snapshot = snapshot();
        assert_eq!(snapshot.alive_threads, 1);
        assert_eq!(snapshot.http_capable_threads, 1);
        assert_eq!(snapshot.tls_capable_threads, 0);
        assert_eq!(snapshot.mode_sensitive_threads, 1);
        assert_eq!(snapshot.high_risk_threads, 0);
        assert_eq!(snapshot.task_wdt_feed_only_threads, 1);
        let detail = &snapshot.details[0];
        assert_eq!(detail.execution_class, ThreadExecutionClass::Config);
        assert_eq!(detail.risk_class, ThreadRiskClass::Medium);
        assert!(detail.http_capable);
        assert!(!detail.tls_capable);
        assert!(detail.mode_sensitive);

        reset_for_tests();
    }

    #[test]
    fn normalize_stack_high_water_caps_impossible_samples() {
        assert_eq!(normalize_stack_high_water_free_bytes(8192, 4096), 4096);
        assert_eq!(normalize_stack_high_water_free_bytes(8192, 32768), 8192);
    }

    #[test]
    fn runtime_mode_snapshot_reads_global_runtime_flags() {
        let _guard = registry_test_guard();
        let _state_guard = crate::state::test_state_guard();
        crate::runtime::governance::reset_runtime_governance_state_for_tests();
        crate::state::set_boot_phase_active(false);
        crate::state::set_pairing_state_known(true);
        crate::state::set_pairing_required(true);
        crate::state::set_recovery_safe_mode_active(false);

        let mode = runtime_mode_snapshot();
        assert!(mode.pairing_state_known);
        assert!(mode.pairing_required);
        assert_eq!(mode.current_mode, crate::runtime::RuntimeMode::Pairing);

        crate::state::set_pairing_required(false);
        crate::state::set_recovery_safe_mode_active(true);
        let recovery_mode = runtime_mode_snapshot();
        assert!(recovery_mode.recovery_safe_mode_active);
        assert_eq!(
            recovery_mode.current_mode,
            crate::runtime::RuntimeMode::RecoverySafeMode
        );

        crate::state::set_recovery_safe_mode_active(false);
        crate::state::set_pairing_state_known(false);
        reset_for_tests();
    }
}
