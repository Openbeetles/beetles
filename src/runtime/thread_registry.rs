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
    pub registered_threads: usize,
    pub total_stack_bytes: usize,
    pub io_threads: usize,
    pub interactive_threads: usize,
    pub background_threads: usize,
    pub core0_threads: usize,
    pub core1_threads: usize,
    pub unpinned_threads: usize,
    pub std_thread_threads: usize,
    pub esp_native_task_threads: usize,
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

#[derive(Clone, serde::Serialize)]
/// 当前运行模式快照，用来判断多 plane / 双 lane 是否同时常驻。
pub struct RuntimeModeSnapshot {
    pub wifi_sta_connected: bool,
    pub voice_exclusive_active: bool,
    pub background_maintenance_active: bool,
    pub config_plane_alive: bool,
    pub channel_plane_alive: bool,
    pub voice_plane_alive: bool,
    pub agent_plane_alive: bool,
    pub user_agent_lane_alive: bool,
    pub system_agent_lane_alive: bool,
    pub dual_agent_lanes_alive: bool,
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
    let guard = registry().lock().unwrap_or_else(|e| e.into_inner());
    let background_maintenance_active = crate::state::background_maintenance_active();
    let config_plane_alive = crate::state::config_plane_active();
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
    let user_agent_lane_alive = guard
        .iter()
        .any(|entry| entry.alive && entry.name == "agent_loop");
    let system_agent_lane_alive = agent_plane_alive && background_maintenance_active;
    RuntimeModeSnapshot {
        wifi_sta_connected: crate::state::wifi_sta_connected(),
        voice_exclusive_active: crate::state::voice_exclusive_active(),
        background_maintenance_active,
        config_plane_alive,
        channel_plane_alive,
        voice_plane_alive,
        agent_plane_alive,
        user_agent_lane_alive,
        system_agent_lane_alive,
        dual_agent_lanes_alive: false,
    }
}

/// 返回线程汇总基线日志行。
pub fn format_baseline_log_line() -> String {
    let snapshot = build_snapshot(false);
    format!(
        "threads alive={} registered={} stack_total={} io={} interactive={} background={} core0={} core1={} unpinned={} std={} native={} tls={} http={} wss={} mode_sensitive={} high_risk={} critical={} low_margin={} hw_samples={}",
        snapshot.alive_threads,
        snapshot.registered_threads,
        snapshot.total_stack_bytes,
        snapshot.io_threads,
        snapshot.interactive_threads,
        snapshot.background_threads,
        snapshot.core0_threads,
        snapshot.core1_threads,
        snapshot.unpinned_threads,
        snapshot.std_thread_threads,
        snapshot.esp_native_task_threads,
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
        "runtime_mode wifi_sta={} voice_exclusive={} bg_maintenance={} config_plane={} channel_plane={} voice_plane={} agent_plane={} user_agent={} system_agent={} dual_agent={}",
        mode.wifi_sta_connected,
        mode.voice_exclusive_active,
        mode.background_maintenance_active,
        mode.config_plane_alive,
        mode.channel_plane_alive,
        mode.voice_plane_alive,
        mode.agent_plane_alive,
        mode.user_agent_lane_alive,
        mode.system_agent_lane_alive,
        mode.dual_agent_lanes_alive,
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
    let mut std_thread_threads = 0usize;
    let mut esp_native_task_threads = 0usize;
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
        let stack_free = sample_stack_high_water_free_bytes(entry);
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
            TaskSpawnSurface::StdThread => std_thread_threads += 1,
            TaskSpawnSurface::EspNativeTask => esp_native_task_threads += 1,
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
        registered_threads: guard.len(),
        total_stack_bytes,
        io_threads,
        interactive_threads,
        background_threads,
        core0_threads,
        core1_threads,
        unpinned_threads,
        std_thread_threads,
        esp_native_task_threads,
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
        "voice_session" | "voice_session_worker" => ThreadProfile {
            execution_class: ThreadExecutionClass::Voice,
            risk_class: ThreadRiskClass::Critical,
            tls_capable: true,
            http_capable: true,
            wss_capable: true,
            mode_sensitive: true,
        },
        "qq_ws" | "feishu_ws" => ThreadProfile {
            execution_class: ThreadExecutionClass::Channel,
            risk_class: ThreadRiskClass::Critical,
            tls_capable: true,
            http_capable: true,
            wss_capable: true,
            mode_sensitive: true,
        },
        "qq_sender" | "tg_sender" | "fs_sender" | "dt_sender" | "wc_sender" | "tg_poll" => {
            ThreadProfile {
                execution_class: ThreadExecutionClass::Channel,
                risk_class: ThreadRiskClass::High,
                tls_capable: true,
                http_capable: true,
                wss_capable: false,
                mode_sensitive: true,
            }
        }
        "http_route_exec" => ThreadProfile {
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
        "dispatch" | "bg_timer" | "heartbeat" | "restart_defer" | "cron" | "remind" => {
            ThreadProfile {
                execution_class: ThreadExecutionClass::Runtime,
                risk_class: ThreadRiskClass::Low,
                tls_capable: false,
                http_capable: false,
                wss_capable: false,
                mode_sensitive: false,
            }
        }
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

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn current_task_handle_key() -> usize {
    esp_idf_hal::task::current()
        .map(|handle| handle as usize)
        .unwrap_or(0)
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
#[allow(dead_code)]
fn current_task_handle_key() -> usize {
    0
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn sample_stack_high_water_free_bytes(entry: &ThreadEntry) -> Option<usize> {
    unsafe extern "C" {
        fn uxTaskGetStackHighWaterMark(task: esp_idf_hal::sys::TaskHandle_t) -> u32;
    }

    if entry.task_handle_key == 0 {
        return None;
    }
    let words = unsafe {
        uxTaskGetStackHighWaterMark(entry.task_handle_key as esp_idf_hal::sys::TaskHandle_t)
    } as usize;
    Some(words.saturating_mul(core::mem::size_of::<u32>()))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn sample_stack_high_water_free_bytes(_entry: &ThreadEntry) -> Option<usize> {
    None
}
