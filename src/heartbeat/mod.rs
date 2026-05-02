//! 周期打日志（版本、运行时长、可选 heap），供外部监控存活；可读 HEARTBEAT.md 待办并注入入站。
//! Heartbeat: periodic log (version, uptime, optional heap) for liveness monitoring.

use std::sync::mpsc::TrySendError;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::channels::inbound_backpressure::{self, EventIngressSource, InboundBackpressureOutcome};
use crate::i18n::{tr, Locale, Message as UiMessage};

const TAG: &str = "heartbeat";
const TASK_THROTTLE_SECS: u64 = 30;

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

/// Heartbeat tick 的可变状态，供 bg_timer 跨轮次复用。
#[derive(Default)]
pub struct HeartbeatTickState {
    pub(crate) round: u32,
}

impl HeartbeatTickState {
    pub fn new() -> Self {
        Self { round: 0 }
    }

    pub(crate) fn should_schedule_session_gc(&self) -> bool {
        self.round
            .is_multiple_of(crate::constants::SESSION_GC_INTERVAL_ROUNDS)
    }
}

/// 单次 heartbeat tick：日志、队列深度、轻量存储指标、待办注入等。
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
    config: &crate::config::AppConfig,
    resolve_locale: &Arc<dyn Fn() -> Locale + Send + Sync>,
    state: &mut HeartbeatTickState,
) {
    state.round = state.round.wrapping_add(1);
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();

    let mut storage_state_fs_ready = None;

    // Session/storage metrics: seed once, then refresh every SESSION_METRICS_INTERVAL_ROUNDS rounds.
    if runtime_mode.action_budget.allow_periodic_maintenance
        && should_collect_session_storage(state.round)
    {
        let sess_count = session_store
            .list_chat_ids()
            .map(|v| v.len() as u32)
            .unwrap_or(0);
        let state_fs_ready = crate::orchestrator::update_session_storage_from_bytes(
            sess_count,
            platform.spiffs_usage(),
        );
        storage_state_fs_ready =
            storage_state_fs_ready_for_runtime_capability_refresh(state.round, state_fs_ready);
    }

    let outbound_transport_ready = match platform.memory_system_kind() {
        crate::memory::MemorySystemKind::LinuxFull => true,
        crate::memory::MemorySystemKind::EspCompact => crate::state::wifi_sta_connected(),
    };
    crate::orchestrator::observe_runtime_capabilities_from_platform_with_config(
        platform,
        Some(config),
        outbound_transport_ready,
        storage_state_fs_ready,
    );

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
    let network_snapshot = crate::state::network_runtime_snapshot(
        crate::platform::time::wall_clock_is_trustworthy(),
        3,
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::state::format_network_runtime_baseline_line(&network_snapshot)
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::thread_registry::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::plane::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::plane_lifecycle::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::lease::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::execution_budget::format_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::display::format_display_lease_baseline_log_line()
    );
    log::info!(
        "[{}] {}",
        TAG,
        crate::runtime::write_back::format_baseline_log_line()
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
        inbound_backpressure::record_stale_drop(EventIngressSource::Heartbeat);
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
    match inbound_tx.try_send(msg) {
        Ok(()) => {
            inbound_backpressure::record_enqueued(EventIngressSource::Heartbeat);
        }
        Err(TrySendError::Full(_)) => {
            inbound_backpressure::record_queue_full_for_source(
                EventIngressSource::Heartbeat,
                InboundBackpressureOutcome::Dropped,
            );
            log::warn!("[{}] inbound_tx.try_send failed (system queue full)", TAG);
        }
        Err(TrySendError::Disconnected(_)) => {
            inbound_backpressure::record_disconnected_drop_for_source(
                EventIngressSource::Heartbeat,
            );
            log::warn!("[{}] inbound_tx.try_send failed (channel closed?)", TAG);
        }
    }
}

fn should_collect_session_storage(round: u32) -> bool {
    round == 1 || round.is_multiple_of(crate::constants::SESSION_METRICS_INTERVAL_ROUNDS)
}

fn storage_state_fs_ready_for_runtime_capability_refresh(
    round: u32,
    state_fs_ready: bool,
) -> Option<bool> {
    should_collect_session_storage(round).then_some(state_fs_ready)
}

#[cfg(test)]
mod tests {
    #[test]
    fn storage_probe_seeds_first_round_then_stays_throttled() {
        assert_eq!(
            super::storage_state_fs_ready_for_runtime_capability_refresh(1, true),
            Some(true)
        );
        assert_eq!(
            super::storage_state_fs_ready_for_runtime_capability_refresh(
                crate::constants::SESSION_METRICS_INTERVAL_ROUNDS,
                false
            ),
            Some(false)
        );
        assert_eq!(
            super::storage_state_fs_ready_for_runtime_capability_refresh(2, true),
            None
        );
    }
}
