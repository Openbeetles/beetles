//! 周期打日志（版本、运行时长、可选 heap），供外部监控存活；可读 HEARTBEAT.md 待办并注入入站。
//! Heartbeat: periodic log (version, uptime, optional heap) for liveness monitoring.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::i18n::{tr, Locale, Message as UiMessage};

const TAG: &str = "heartbeat";
const TASK_THROTTLE_SECS: u64 = 30;

/// HEARTBEAT.md 中是否存在未勾选任务行（`- [ ]`）。空行、`#` 标题、`- [x]` 忽略。
pub fn has_pending_tasks(content: &str) -> bool {
    first_pending_task(content).is_some()
}

/// 返回第一个未完成任务行去掉 `- [ ]` 后的 trim 文本；无则 None。
pub fn first_pending_task(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains("- [ ]") {
            let t = line.split("- [ ]").nth(1).map(|s| s.trim()).unwrap_or("");
            return Some(t.to_string());
        }
    }
    None
}

/// 待办注入限频：同一内容 30s 内不重复注入。(content, last_inject_time)
static LAST_TASK_INJECT: OnceLock<Mutex<(String, Option<Instant>)>> = OnceLock::new();

/// 周期（秒）打一条日志：版本、运行时长、可选 heap；可被外部脚本/串口抓取判断存活。
pub fn run_heartbeat_loop(version: &'static str, interval_secs: u64) {
    let v = version;
    crate::util::spawn_guarded_with_profile(
        "heartbeat",
        8192,
        Some(crate::util::SpawnCore::Core1),
        crate::util::HttpThreadRole::Background,
        move || {
            let interval = std::time::Duration::from_secs(interval_secs);
            loop {
                std::thread::sleep(interval);
                crate::orchestrator::update_heap_state();
                let uptime_secs = crate::platform::time::app_uptime_secs();
                log::info!(
                    "[{}] HEARTBEAT version={} uptime_secs={} {}",
                    TAG,
                    v,
                    uptime_secs,
                    crate::orchestrator::format_resource_baseline_line()
                );
            }
        },
    );
    log::info!(
        "[{}] heartbeat loop started (interval {}s)",
        TAG,
        interval_secs
    );
}

/// Heartbeat tick 的可变状态，供 bg_timer 跨轮次复用。
#[derive(Default)]
pub struct HeartbeatTickState {
    pub(crate) round: u32,
}

impl HeartbeatTickState {
    pub fn new() -> Self {
        Self { round: 0 }
    }
}

/// 单次 heartbeat tick：日志、队列深度、会话 GC、待办注入等。
/// 由 bg_timer 每 30s 调用一次。
#[allow(clippy::too_many_arguments)]
pub(crate) fn heartbeat_tick(
    version: &str,
    inbound_tx: &crate::bus::SystemInboundTx,
    read_heartbeat: &dyn Fn() -> String,
    user_inbound_depth: &std::sync::atomic::AtomicUsize,
    system_inbound_depth: &std::sync::atomic::AtomicUsize,
    outbound_depth: &std::sync::atomic::AtomicUsize,
    session_store: &dyn crate::memory::SessionStore,
    platform: &dyn crate::Platform,
    resolve_locale: &Arc<dyn Fn() -> Locale + Send + Sync>,
    state: &mut HeartbeatTickState,
) {
    state.round = state.round.wrapping_add(1);
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();

    // Session GC: run every SESSION_GC_INTERVAL_ROUNDS rounds.
    if runtime_mode.action_budget.allow_periodic_maintenance
        && state
            .round
            .is_multiple_of(crate::constants::SESSION_GC_INTERVAL_ROUNDS)
    {
        match session_store.gc_stale(crate::constants::SESSION_GC_MAX_AGE_SECS) {
            Ok(n) if n > 0 => {
                log::info!("[{}] session GC removed {} stale files", TAG, n)
            }
            Err(e) => log::warn!("[{}] session GC error: {}", TAG, e),
            _ => {}
        }
    }

    // Session/storage metrics: collect every SESSION_METRICS_INTERVAL_ROUNDS rounds.
    if runtime_mode.action_budget.allow_periodic_maintenance
        && state
            .round
            .is_multiple_of(crate::constants::SESSION_METRICS_INTERVAL_ROUNDS)
    {
        let sess_count = session_store
            .list_chat_ids()
            .map(|v| v.len() as u32)
            .unwrap_or(0);
        let (s_used, s_total, state_fs_ready) = storage_usage_kb(platform);
        crate::orchestrator::update_session_storage(sess_count, s_used, s_total);
        let outbound_transport_ready = match platform.memory_system_kind() {
            crate::memory::MemorySystemKind::LinuxFull => true,
            crate::memory::MemorySystemKind::EspCompact => crate::state::wifi_sta_connected(),
        };
        crate::orchestrator::observe_runtime_capabilities_from_platform(
            platform,
            outbound_transport_ready,
            Some(state_fs_ready),
        );
    }

    // Update queue depth snapshot for pressure computation.
    let in_user = user_inbound_depth.load(std::sync::atomic::Ordering::Relaxed) as u32;
    let in_system = system_inbound_depth.load(std::sync::atomic::Ordering::Relaxed) as u32;
    let in_d = in_user.saturating_add(in_system);
    let out_d = outbound_depth.load(std::sync::atomic::Ordering::Relaxed) as u32;
    crate::orchestrator::update_queue_depth(in_d, out_d);
    crate::orchestrator::update_heap_state();
    let uptime_secs = crate::platform::time::app_uptime_secs();
    log::info!(
        "[{}] HEARTBEAT version={} uptime_secs={} {}",
        TAG,
        version,
        uptime_secs,
        crate::orchestrator::format_resource_baseline_line()
    );
    let baseline = crate::metrics::snapshot().to_baseline_log_line();
    log::info!("[{}] {}", TAG, baseline);
    log::info!(
        "[{}] {}",
        TAG,
        crate::orchestrator::format_runtime_capability_baseline_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::thread_registry::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::thread_registry::format_stack_risk_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::thread_registry::format_runtime_mode_log_line()
    );

    if !runtime_mode.action_budget.allow_heartbeat_injection {
        return;
    }

    let content = read_heartbeat();
    let Some(task_content) = first_pending_task(&content) else {
        return;
    };
    let should_inject = {
        let guard = LAST_TASK_INJECT.get_or_init(|| Mutex::new((String::new(), None)));
        let mut g = guard.lock().unwrap_or_else(|e| e.into_inner());
        let (last_content, last_time) = (&g.0, g.1);
        let same = last_content == &task_content;
        let within = last_time
            .map(|t| t.elapsed() < Duration::from_secs(TASK_THROTTLE_SECS))
            .unwrap_or(false);
        if same && within {
            false
        } else {
            *g = (task_content.clone(), Some(Instant::now()));
            true
        }
    };
    if !should_inject {
        return;
    }
    let loc = resolve_locale();
    let body = tr(UiMessage::HeartbeatPendingTasksReminder, loc);
    let msg = match crate::bus::PcMsg::new_system("heartbeat", "heartbeat", body) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("[{}] PcMsg::new failed: {}", TAG, e);
            return;
        }
    };
    if inbound_tx.send(msg).is_err() {
        log::warn!("[{}] inbound_tx.send failed (channel closed?)", TAG);
    }
}

/// 存储用量（KB）。经 [`crate::Platform::spiffs_usage`]；无数据时为 (0, 0)。
fn storage_usage_kb(platform: &dyn crate::Platform) -> (u32, u32, bool) {
    match platform.spiffs_usage() {
        Some((total, used)) => {
            let used_kb = (used / 1024).min(u32::MAX as u64) as u32;
            let total_kb = (total / 1024).min(u32::MAX as u64) as u32;
            (used_kb, total_kb, true)
        }
        None => (0, 0, false),
    }
}
