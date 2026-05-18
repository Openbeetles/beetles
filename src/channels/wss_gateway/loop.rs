//! 统一 WSS 网关循环：取 URL → 建连 → Hello/鉴权 → 心跳 + 收包入队，退避重连。
//! WiFi 断连时先等 WiFi 恢复再尝试重连 WSS，避免无网络时反复做 TLS 握手。

use crate::bus::InboundTx;
use crate::channels::inbound_backpressure::{self, EventIngressSource, InboundBackpressureOutcome};
use crate::channels::wss_gateway::connection::{WssConnection, WssEvent};
use crate::channels::wss_gateway::driver::{WssGatewayDriver, WssRecvAction, WssSessionState};
use crate::channels::ChannelHttpClient;
use crate::error::Result;
use crate::memory::PendingRetryStore;
use std::time::{Duration, Instant};

const BACKOFF_MAX_SECS: u64 = 120;
const HELLO_RECV_TIMEOUT_MS: u64 = 15_000;
const TLS_ADMISSION_RETRY_SLEEP_SECS: u64 = 5;
const HEARTBEAT_INTERVAL_MIN_MS: u64 = 10_000;
const HEARTBEAT_INTERVAL_MAX_MS: u64 = 300_000;
const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 120_000;
#[cfg(feature = "feishu")]
const ACK_SEND_DELAY_MS: u64 = 20;
/// recv_timeout 单次上限（秒）；须小于 TWDT 超时（sdkconfig 60s），
/// 避免长心跳间隔通道（如飞书 120s）在空闲时触发看门狗。
const WDT_RECV_CHUNK_SECS: u64 = 25;
/// WiFi 就绪等待上限（秒）；运行中网络断开后重连时，超出后仍尝试连接。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const WIFI_WAIT_MAX_SECS: u64 = 60;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const VOICE_EXCLUSIVE_WAIT_MS: u64 = 500;

fn format_wss_close_event(event: &WssEvent) -> String {
    match event {
        WssEvent::Closed(Some(info)) => info.summary(),
        WssEvent::Closed(None) => "peer closed".to_string(),
        WssEvent::Disconnected => "transport disconnected".to_string(),
        WssEvent::Binary(_) => "binary frame".to_string(),
    }
}

fn tls_admission_retry_sleep_secs_for_pressure(
    pressure: crate::orchestrator::PressureLevel,
) -> u64 {
    crate::orchestrator::pressure::budget_for_level(pressure)
        .reconnect_backoff_secs
        .max(TLS_ADMISSION_RETRY_SLEEP_SECS)
}

fn should_pause_external_wss_connect_for_pressure(
    pressure: crate::orchestrator::PressureLevel,
) -> bool {
    pressure == crate::orchestrator::PressureLevel::Critical
}

fn should_save_plain_dispatch_to_pending_retry_on_pressure(
    pressure: crate::orchestrator::PressureLevel,
) -> bool {
    pressure == crate::orchestrator::PressureLevel::Critical
}

#[cfg(any(not(any(target_arch = "xtensa", target_arch = "riscv32")), test))]
fn should_defer_external_wss_for_wall_clock(wall_clock_valid: bool) -> bool {
    !wall_clock_valid
}

#[cfg(test)]
mod network_gate_tests {
    #[test]
    fn external_wss_reports_wifi_before_wall_clock() {
        let _guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(true, true);
        crate::state::set_network_wifi_stage(crate::state::NetworkWifiStage::StaConnecting, None);
        crate::state::clear_wifi_sta_state();

        let snapshot = crate::state::network_runtime_snapshot(false, 3);

        assert_eq!(
            crate::network::external_wss_network_suspend_reason(&snapshot),
            Some("wifi_not_ready")
        );
    }

    #[test]
    fn external_wss_reports_wall_clock_only_after_network_ready() {
        let _guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(true, true);
        crate::state::set_wifi_sta_state(true, Some("192.168.1.2".to_string()));

        let snapshot = crate::state::network_runtime_snapshot(false, 0);

        assert_eq!(
            crate::network::external_wss_network_suspend_reason(&snapshot),
            Some("wall_clock_untrusted")
        );
    }

    #[test]
    fn external_wss_keeps_waiting_during_gateway_settle_window() {
        let _guard = crate::state::test_state_guard();
        crate::state::set_network_sta_expected(true, true);
        crate::state::set_wifi_sta_state(true, Some("192.168.1.2".to_string()));

        let snapshot = crate::state::network_runtime_snapshot(
            true,
            crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
        );

        assert_eq!(
            crate::network::external_wss_network_suspend_reason(&snapshot),
            Some("wifi_not_ready")
        );
    }
}

fn wss_lifecycle_owner(tag: &str) -> &'static str {
    match tag {
        "qq_ws" => "qq_ws",
        "feishu_ws" => "feishu_ws",
        "dingtalk_stream" => "dingtalk_stream",
        "wecom_aibot" => "wecom_aibot",
        _ => "external_wss",
    }
}

fn mark_wss_lifecycle(
    owner: &'static str,
    state: crate::runtime::PlaneLifecycleState,
    reason: &'static str,
) {
    let _ = crate::runtime::plane_lifecycle::mark(
        crate::runtime::PlaneId::ChannelWss,
        owner,
        state,
        reason,
    );
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
struct ExternalWssWorkerPresenceGuard;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl ExternalWssWorkerPresenceGuard {
    fn new() -> Self {
        crate::network::set_external_wss_managed_present(true);
        Self
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for ExternalWssWorkerPresenceGuard {
    fn drop(&mut self) {
        crate::network::mark_external_wss_worker_unloaded();
    }
}

pub(crate) fn external_wss_worker_should_exit_for_evict(
    tag: &str,
    lifecycle_owner: &'static str,
) -> bool {
    let Some(reason) = external_wss_worker_evict_lifecycle_reason() else {
        return false;
    };
    log::info!(
        "[{}] unloading external WSS worker for resource window reason={}",
        tag,
        reason
    );
    crate::network::mark_external_wss_worker_unloaded();
    mark_wss_lifecycle(
        lifecycle_owner,
        crate::runtime::PlaneLifecycleState::Unloaded,
        reason,
    );
    true
}

fn external_wss_worker_evict_lifecycle_reason() -> Option<&'static str> {
    crate::network::external_wss_worker_evict_reason()
        .map(crate::network::ExternalWssSuspendReason::as_str)
        .or_else(|| {
            crate::network::external_wss_worker_evict_requested()
                .then_some("external_wss_worker_evict")
        })
}

fn wss_runtime_gate_suspend_reason(mode: crate::runtime::RuntimeModeSnapshot) -> &'static str {
    if mode.current_mode == crate::runtime::RuntimeMode::VoiceExclusive {
        "voice_exclusive_suspend"
    } else if mode.current_mode == crate::runtime::RuntimeMode::ConfigActive
        && mode.action_budget.require_external_wss_suspended
    {
        "config_persisting_suspend"
    } else {
        "runtime_mode_gate"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WssSessionEndLifecycle {
    Stopping(&'static str),
    Failed(&'static str),
}

pub(crate) fn external_wss_connect_gate(
    tag: &str,
    lifecycle_owner: &'static str,
    waiting_for_wall_clock: &mut bool,
) -> bool {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_external_wss_connect {
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Suspended,
            wss_runtime_gate_suspend_reason(runtime_mode),
        );
        if runtime_mode.action_budget.require_external_wss_suspended {
            crate::network::wait_for_external_wss_resume(tag);
        } else {
            sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
        }
        return false;
    }

    let pressure = crate::orchestrator::refresh_heap_if_stale();
    if should_pause_external_wss_connect_for_pressure(pressure) {
        let sleep_secs = tls_admission_retry_sleep_secs_for_pressure(pressure);
        log::info!(
            "[{}] skip external WSS connect under {:?} pressure; retry in {}s",
            tag,
            pressure,
            sleep_secs
        );
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Suspended,
            "critical_pressure",
        );
        sleep_with_wdt(sleep_secs);
        return false;
    }

    let wall_clock_valid = crate::platform::time::wall_clock_is_trustworthy();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let network_snapshot = crate::state::network_runtime_snapshot(
            wall_clock_valid,
            crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
        );
        if let Some(reason) = crate::network::external_wss_network_suspend_reason(&network_snapshot)
        {
            mark_wss_lifecycle(
                lifecycle_owner,
                crate::runtime::PlaneLifecycleState::Suspended,
                reason,
            );
            if reason == "wall_clock_untrusted" {
                if !*waiting_for_wall_clock {
                    log::info!(
                        "[{}] defer external WSS connect until wall clock is trustworthy",
                        tag
                    );
                    *waiting_for_wall_clock = true;
                }
                if !crate::platform::time::wait_for_wall_clock_trustworthy(Duration::from_secs(
                    TLS_ADMISSION_RETRY_SLEEP_SECS,
                )) {
                    return false;
                }
            } else {
                *waiting_for_wall_clock = false;
                if reason == "wifi_not_ready" {
                    wait_for_wifi(tag);
                } else {
                    log::info!(
                        "[{}] defer external WSS connect: {} stage={:?}",
                        tag,
                        reason,
                        network_snapshot.last_wifi_stage
                    );
                    sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
                }
                return false;
            }
        }
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    if should_defer_external_wss_for_wall_clock(wall_clock_valid) {
        if !*waiting_for_wall_clock {
            log::info!(
                "[{}] defer external WSS connect until wall clock is trustworthy",
                tag
            );
            *waiting_for_wall_clock = true;
        }
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Suspended,
            "wall_clock_untrusted",
        );
        if !crate::platform::time::wait_for_wall_clock_trustworthy(Duration::from_secs(
            TLS_ADMISSION_RETRY_SLEEP_SECS,
        )) {
            return false;
        }
    }
    if *waiting_for_wall_clock {
        log::info!(
            "[{}] wall clock trustworthy; resuming external WSS connect",
            tag
        );
        *waiting_for_wall_clock = false;
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if crate::network::external_wss_suspend_requested() {
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Suspended,
            crate::network::external_wss_suspend_reason()
                .map(crate::network::ExternalWssSuspendReason::as_str)
                .unwrap_or("external_wss_suspend"),
        );
    }
    crate::network::wait_for_external_wss_resume(tag);
    if crate::network::external_wss_worker_evict_requested() {
        return false;
    }
    if !wait_for_wifi(tag) {
        sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
        return false;
    }

    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    if !runtime_mode.action_budget.allow_external_wss_connect {
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Suspended,
            wss_runtime_gate_suspend_reason(runtime_mode),
        );
        if runtime_mode.action_budget.require_external_wss_suspended {
            crate::network::wait_for_external_wss_resume(tag);
        } else {
            sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
        }
        return false;
    }

    true
}

pub(crate) fn external_wss_session_stop_reason(
    tag: &str,
    lifecycle_owner: &'static str,
) -> Option<&'static str> {
    let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
    let keep_existing_for_config_write = runtime_mode.current_mode
        == crate::runtime::RuntimeMode::ConfigActive
        && !runtime_mode.action_budget.require_external_wss_suspended;
    if !runtime_mode.action_budget.allow_external_wss_connect && !keep_existing_for_config_write {
        let reason = if crate::network::external_wss_suspend_requested() {
            crate::network::external_wss_suspend_reason()
                .map(crate::network::ExternalWssSuspendReason::as_str)
                .unwrap_or("external_wss_suspend")
        } else {
            "runtime_mode_gate"
        };
        log::info!(
            "[{}] disconnecting external WSS under runtime mode gate current_mode={} reason={}",
            tag,
            runtime_mode.current_mode.as_str(),
            reason
        );
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Draining,
            reason,
        );
        return Some(reason);
    }
    None
}

/// 阻塞等待 WiFi STA 就绪，每 2s 轮询，最多 `WIFI_WAIT_MAX_SECS`。返回 true 表示已就绪，false 表示超时仍继续尝试。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_wifi(tag: &str) -> bool {
    if crate::state::wifi_sta_settled_for_outbound(
        crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
    ) {
        return true;
    }
    log::info!(
        "[{}] WiFi STA not ready, waiting up to {}s",
        tag,
        WIFI_WAIT_MAX_SECS
    );
    let deadline = Instant::now() + Duration::from_secs(WIFI_WAIT_MAX_SECS);
    while Instant::now() < deadline {
        crate::platform::task_wdt::feed_current_task();
        std::thread::sleep(Duration::from_secs(2));
        if crate::state::wifi_sta_settled_for_outbound(
            crate::network::EXTERNAL_WSS_OUTBOUND_SETTLE_SECS,
        ) {
            log::info!("[{}] WiFi STA ready", tag);
            return true;
        }
    }
    log::warn!(
        "[{}] WiFi STA still not ready after {}s, connect deferred",
        tag,
        WIFI_WAIT_MAX_SECS
    );
    false
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn wait_for_wifi(_tag: &str) -> bool {
    true
}

/// 各通道在 main 中独立线程调用，泛型 `D`/`H`/`C` 为不同实现；有意保留多组单态以隔离 TLS/HTTP 与重连语义，
/// 而非合并为 enum（体积换可维护性；若前序优化仍不足再评估）。
pub fn run_wss_gateway_loop<D, H, C, CreateHttp, Conn>(
    tag: &str,
    mut driver: D,
    inbound_tx: InboundTx,
    pending_retry: &dyn PendingRetryStore,
    mut create_http: CreateHttp,
    mut connect: Conn,
) where
    D: WssGatewayDriver,
    H: ChannelHttpClient,
    C: WssConnection,
    CreateHttp: FnMut() -> Result<H>,
    Conn: FnMut(&str) -> Result<C>,
{
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let _presence_guard = ExternalWssWorkerPresenceGuard::new();
    let lifecycle_owner = wss_lifecycle_owner(tag);
    let mut backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
    let mut waiting_for_wall_clock = false;
    loop {
        if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
            return;
        }
        if !external_wss_connect_gate(tag, lifecycle_owner, &mut waiting_for_wall_clock) {
            continue;
        }
        if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
            return;
        }

        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Starting,
            "connect_attempt",
        );
        let mut http = match create_http() {
            Ok(h) => h,
            Err(e) => {
                mark_wss_lifecycle(
                    lifecycle_owner,
                    crate::runtime::PlaneLifecycleState::Failed,
                    "create_http_failed",
                );
                log::warn!("[{}] create_http failed: {}", tag, e);
                if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
                    return;
                }
                sleep_with_wdt(backoff_secs);
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
        if !runtime_mode.action_budget.allow_external_wss_connect {
            mark_wss_lifecycle(
                lifecycle_owner,
                crate::runtime::PlaneLifecycleState::Suspended,
                wss_runtime_gate_suspend_reason(runtime_mode),
            );
            if runtime_mode.action_budget.require_external_wss_suspended {
                crate::network::wait_for_external_wss_resume(tag);
            } else {
                sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
            }
            continue;
        }
        let url = match driver.get_url(&mut http) {
            Ok(u) => u,
            Err(e) => {
                mark_wss_lifecycle(
                    lifecycle_owner,
                    crate::runtime::PlaneLifecycleState::Failed,
                    "get_url_failed",
                );
                crate::metrics::record_error_by_stage(e.metrics_stage());
                log::warn!("[{}] get_url failed: {}", tag, e);
                if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
                    return;
                }
                if e.is_tls_admission() {
                    let sleep_secs = tls_admission_retry_sleep_secs_for_pressure(
                        crate::orchestrator::refresh_heap_if_stale(),
                    );
                    sleep_with_wdt(sleep_secs);
                } else {
                    sleep_with_wdt(backoff_secs);
                    backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                }
                continue;
            }
        };
        log::info!("[{}] wss url obtained, connecting", tag);
        log::debug!("[{}] gateway url len={}", tag, url.len());

        let mut conn = match connect(&url) {
            Ok(c) => c,
            Err(e) => {
                mark_wss_lifecycle(
                    lifecycle_owner,
                    crate::runtime::PlaneLifecycleState::Failed,
                    "connect_failed",
                );
                crate::metrics::record_error_by_stage(e.metrics_stage());
                log::warn!("[{}] connect failed: {}", tag, e);
                if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
                    return;
                }
                sleep_with_wdt(backoff_secs);
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };

        let state = if driver.expects_hello() {
            match conn.recv_timeout(Duration::from_millis(HELLO_RECV_TIMEOUT_MS)) {
                Ok(Some(WssEvent::Binary(data))) => match driver.on_hello(data.as_slice()) {
                    Ok(s) => s,
                    Err(e) => {
                        mark_wss_lifecycle(
                            lifecycle_owner,
                            crate::runtime::PlaneLifecycleState::Failed,
                            "hello_failed",
                        );
                        log::warn!("[{}] on_hello parse failed, reconnecting: {}", tag, e);
                        drop(conn);
                        sleep_with_wdt(backoff_secs);
                        backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                        continue;
                    }
                },
                Ok(Some(event @ WssEvent::Disconnected))
                | Ok(Some(event @ WssEvent::Closed(_))) => {
                    log::info!(
                        "[{}] disconnected before hello: {}",
                        tag,
                        format_wss_close_event(&event)
                    );
                    mark_wss_lifecycle(
                        lifecycle_owner,
                        crate::runtime::PlaneLifecycleState::Failed,
                        "hello_closed",
                    );
                    drop(conn);
                    sleep_with_wdt(backoff_secs);
                    backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                    continue;
                }
                Ok(None) => WssSessionState {
                    heartbeat_interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
                    identify_payload: None,
                },
                Err(e) => {
                    mark_wss_lifecycle(
                        lifecycle_owner,
                        crate::runtime::PlaneLifecycleState::Failed,
                        "hello_failed",
                    );
                    crate::metrics::record_error_by_stage(e.metrics_stage());
                    log::warn!("[{}] recv hello failed: {}", tag, e);
                    drop(conn);
                    sleep_with_wdt(backoff_secs);
                    backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                    continue;
                }
            }
        } else {
            WssSessionState {
                heartbeat_interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
                identify_payload: None,
            }
        };

        let WssSessionState {
            heartbeat_interval_ms,
            identify_payload,
        } = state;

        if let Some(payload) = identify_payload {
            log::debug!("[{}] send identify len={}", tag, payload.len());
            if let Err(e) = conn.send_binary_owned(payload) {
                mark_wss_lifecycle(
                    lifecycle_owner,
                    crate::runtime::PlaneLifecycleState::Failed,
                    "identify_failed",
                );
                log::warn!("[{}] send identify failed: {}", tag, e);
                drop(conn);
                sleep_with_wdt(backoff_secs);
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        }
        mark_wss_lifecycle(
            lifecycle_owner,
            crate::runtime::PlaneLifecycleState::Active,
            "session_started",
        );
        driver.on_session_started();

        let interval_ms =
            heartbeat_interval_ms.clamp(HEARTBEAT_INTERVAL_MIN_MS, HEARTBEAT_INTERVAL_MAX_MS);
        let heartbeat_interval = Duration::from_millis(interval_ms);
        let recv_chunk = heartbeat_interval.min(Duration::from_secs(WDT_RECV_CHUNK_SECS));
        let mut last_seq: Option<u64> = None;
        let mut last_heartbeat = Instant::now();
        let mut session_ended = false;
        let mut session_end_lifecycle = WssSessionEndLifecycle::Stopping("session_ended");

        while !session_ended {
            crate::platform::task_wdt::feed_current_task();
            if let Some(reason) = external_wss_worker_evict_lifecycle_reason() {
                session_end_lifecycle = WssSessionEndLifecycle::Stopping(reason);
                session_ended = true;
                continue;
            }
            if let Some(reason) = external_wss_session_stop_reason(tag, lifecycle_owner) {
                session_end_lifecycle = WssSessionEndLifecycle::Stopping(reason);
                session_ended = true;
                continue;
            }

            let recv_wait = {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                {
                    recv_chunk.min(Duration::from_millis(VOICE_EXCLUSIVE_WAIT_MS))
                }
                #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
                {
                    recv_chunk
                }
            };

            if last_heartbeat.elapsed() >= heartbeat_interval {
                let payload = match driver.build_heartbeat(last_seq) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!("[{}] build_heartbeat failed: {}", tag, e);
                        Vec::new()
                    }
                };
                if !payload.is_empty() {
                    log::debug!("[{}] send heartbeat len={}", tag, payload.len());
                    if conn.send_binary_owned(payload).is_err() {
                        log::warn!("[{}] send heartbeat failed", tag);
                        session_end_lifecycle =
                            WssSessionEndLifecycle::Failed("heartbeat_send_failed");
                        session_ended = true;
                        continue;
                    }
                }
                last_heartbeat = Instant::now();
            }

            match conn.recv_timeout(recv_wait) {
                Ok(Some(WssEvent::Binary(data))) => {
                    log::debug!("[{}] recv binary len={}", tag, data.len());
                    match driver.on_recv(data.as_slice()) {
                        Ok(WssRecvAction::Dispatch(Some(msg))) => {
                            let msg = *msg;
                            let chat_id = msg.chat_id.clone();
                            if should_save_plain_dispatch_to_pending_retry_on_pressure(
                                crate::orchestrator::current_pressure(),
                            ) {
                                log::warn!(
                                    "[{}] pressure critical, saving msg to pending retry chat_id={}",
                                    tag,
                                    chat_id
                                );
                                match pending_retry.save_pending_retry(&msg) {
                                    Ok(()) => {
                                        inbound_backpressure::record_deferred_without_queue_full_for_source(
                                            EventIngressSource::WssGateway,
                                        );
                                    }
                                    Err(error) => {
                                        crate::metrics::record_error_by_stage(
                                            error.metrics_stage(),
                                        );
                                        log::error!(
                                            "[{}] pressure pending_retry save failed chat_id={}: {}",
                                            tag,
                                            chat_id,
                                            error
                                        );
                                        inbound_backpressure::record_drop_without_queue_full_for_source(
                                            EventIngressSource::WssGateway,
                                        );
                                    }
                                }
                            } else {
                                let mut enqueued = false;
                                let mut disconnected = false;
                                let mut pending_msg = Some(msg);
                                for _ in 0..3 {
                                    let Some(try_msg) = pending_msg.take() else {
                                        log::warn!("[{}] pending msg missing before try_send", tag);
                                        break;
                                    };
                                    match inbound_tx.try_send(try_msg) {
                                        Ok(()) => {
                                            enqueued = true;
                                            break;
                                        }
                                        Err(std::sync::mpsc::TrySendError::Full(m)) => {
                                            pending_msg = Some(m);
                                            std::thread::sleep(Duration::from_millis(200));
                                        }
                                        Err(std::sync::mpsc::TrySendError::Disconnected(m)) => {
                                            pending_msg = Some(m);
                                            disconnected = true;
                                            log::warn!(
                                                "[{}] inbound disconnected, dropping msg chat_id={}",
                                                tag,
                                                chat_id
                                            );
                                            break;
                                        }
                                    }
                                }
                                if enqueued {
                                    inbound_backpressure::record_enqueued(
                                        EventIngressSource::WssGateway,
                                    );
                                    log::info!("[{}] message enqueued, chat_id={}", tag, chat_id);
                                } else if disconnected {
                                    inbound_backpressure::record_disconnected_drop_for_source(
                                        EventIngressSource::WssGateway,
                                    );
                                } else if let Some(m) = pending_msg.as_ref() {
                                    match pending_retry.save_pending_retry(m) {
                                        Ok(()) => {
                                            log::warn!(
                                                "[{}] inbound queue full, saved pending retry chat_id={}",
                                                tag,
                                                chat_id
                                            );
                                            inbound_backpressure::record_queue_full_for_source(
                                                EventIngressSource::WssGateway,
                                                InboundBackpressureOutcome::DeferredToPendingRetry,
                                            );
                                        }
                                        Err(error) => {
                                            crate::metrics::record_error_by_stage(
                                                error.metrics_stage(),
                                            );
                                            log::error!(
                                                "[{}] queue-full pending_retry save failed chat_id={}: {}",
                                                tag,
                                                chat_id,
                                                error
                                            );
                                            inbound_backpressure::record_queue_full_for_source(
                                                EventIngressSource::WssGateway,
                                                InboundBackpressureOutcome::Dropped,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Ok(WssRecvAction::Dispatch(None)) => {
                            log::debug!("[{}] dispatch ignored (no msg)", tag);
                        }
                        #[cfg(feature = "feishu")]
                        Ok(WssRecvAction::DispatchAndAck(msg, ack)) => {
                            let enqueued = if let Some(msg) = msg {
                                let msg = *msg;
                                let chat_id = msg.chat_id.clone();
                                if crate::orchestrator::current_pressure()
                                    == crate::orchestrator::PressureLevel::Critical
                                {
                                    log::warn!(
                                        "[{}] pressure critical, dropping msg chat_id={}, skip ack to trigger re-delivery",
                                        tag,
                                        chat_id
                                    );
                                    inbound_backpressure::record_deferred_without_queue_full_for_source(
                                        EventIngressSource::WssGateway,
                                    );
                                    false
                                } else {
                                    match inbound_tx.try_send(msg) {
                                        Ok(()) => {
                                            inbound_backpressure::record_enqueued(
                                                EventIngressSource::WssGateway,
                                            );
                                            log::info!(
                                                "[{}] message enqueued, chat_id={}",
                                                tag,
                                                chat_id
                                            );
                                            true
                                        }
                                        Err(std::sync::mpsc::TrySendError::Full(_)) => {
                                            log::warn!(
                                                "[{}] inbound queue full, skip ack to trigger re-delivery, chat_id={}",
                                                tag,
                                                chat_id
                                            );
                                            inbound_backpressure::record_queue_full_for_source(
                                                EventIngressSource::WssGateway,
                                                InboundBackpressureOutcome::RedeliveryRequested,
                                            );
                                            false
                                        }
                                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                                            log::error!("[{}] inbound_tx disconnected", tag);
                                            inbound_backpressure::record_disconnected_drop_for_source(
                                                EventIngressSource::WssGateway,
                                            );
                                            false
                                        }
                                    }
                                }
                            } else {
                                true
                            };
                            if enqueued {
                                std::thread::sleep(Duration::from_millis(ACK_SEND_DELAY_MS));
                                log::debug!("[{}] send ack len={}", tag, ack.len());
                                if conn.send_binary_owned(ack).is_err() {
                                    log::warn!("[{}] send ack failed", tag);
                                }
                            }
                        }
                        Ok(WssRecvAction::SendHeartbeat(seq)) => {
                            last_seq = Some(seq);
                            log::debug!("[{}] heartbeat ack seq={}", tag, seq);
                        }
                        Ok(WssRecvAction::Ignore) => {}
                        Ok(WssRecvAction::Disconnect) => {
                            log::info!("[{}] driver requested disconnect", tag);
                            session_end_lifecycle =
                                WssSessionEndLifecycle::Stopping("driver_disconnect");
                            session_ended = true;
                        }
                        Err(e) => {
                            crate::metrics::record_error_by_stage(e.metrics_stage());
                            log::warn!("[{}] on_recv failed: {}", tag, e);
                        }
                    }
                }
                Ok(Some(event @ WssEvent::Disconnected))
                | Ok(Some(event @ WssEvent::Closed(_))) => {
                    log::info!(
                        "[{}] wss disconnected or closed: {}",
                        tag,
                        format_wss_close_event(&event)
                    );
                    session_end_lifecycle = WssSessionEndLifecycle::Failed("recv_closed");
                    session_ended = true;
                }
                Ok(None) => {}
                Err(e) => {
                    session_end_lifecycle = WssSessionEndLifecycle::Failed("recv_failed");
                    crate::metrics::record_error_by_stage(e.metrics_stage());
                    log::warn!("[{}] recv failed: {}", tag, e);
                    session_ended = true;
                }
            }
        }

        match session_end_lifecycle {
            WssSessionEndLifecycle::Stopping(reason) => mark_wss_lifecycle(
                lifecycle_owner,
                crate::runtime::PlaneLifecycleState::Stopping,
                reason,
            ),
            WssSessionEndLifecycle::Failed(reason) => mark_wss_lifecycle(
                lifecycle_owner,
                crate::runtime::PlaneLifecycleState::Failed,
                reason,
            ),
        }

        log::info!(
            "[{}] disconnected, dropping connection before reconnect",
            tag
        );
        driver.on_session_ended();
        drop(conn);
        if external_wss_worker_should_exit_for_evict(tag, lifecycle_owner) {
            return;
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        if crate::network::external_wss_suspend_requested() {
            crate::network::set_external_wss_suspended(true);
            mark_wss_lifecycle(
                lifecycle_owner,
                crate::runtime::PlaneLifecycleState::Suspended,
                crate::network::external_wss_suspend_reason()
                    .map(crate::network::ExternalWssSuspendReason::as_str)
                    .unwrap_or("external_wss_suspend"),
            );
            continue;
        }
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        crate::network::set_external_wss_suspended(false);
        backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
        log::info!(
            "[{}] will reconnect after WiFi check + {}s backoff",
            tag,
            backoff_secs
        );
        sleep_with_wdt(backoff_secs);
    }
}

/// sleep 期间定期喂看门狗，避免长 sleep 触发 TWDT 复位。
fn sleep_with_wdt(secs: u64) {
    let total = Duration::from_secs(secs);
    let chunk = Duration::from_millis(50);
    let start = Instant::now();
    while start.elapsed() < total {
        if crate::network::external_wss_worker_evict_requested() {
            break;
        }
        let remaining = total.saturating_sub(start.elapsed());
        std::thread::sleep(remaining.min(chunk));
        crate::platform::task_wdt::feed_current_task();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        should_defer_external_wss_for_wall_clock, should_pause_external_wss_connect_for_pressure,
        should_save_plain_dispatch_to_pending_retry_on_pressure,
        tls_admission_retry_sleep_secs_for_pressure, wss_lifecycle_owner,
        wss_runtime_gate_suspend_reason,
    };
    use crate::{
        orchestrator::PressureLevel,
        runtime::mode::{snapshot_from_source, RuntimeModeSource},
        runtime::ConfigActivityPhase,
    };

    #[test]
    fn critical_pressure_pauses_external_wss_connect_attempts() {
        assert!(should_pause_external_wss_connect_for_pressure(
            PressureLevel::Critical
        ));
        assert!(!should_pause_external_wss_connect_for_pressure(
            PressureLevel::Cautious
        ));
        assert!(!should_pause_external_wss_connect_for_pressure(
            PressureLevel::Normal
        ));
    }

    #[test]
    fn critical_pressure_persists_plain_wss_dispatch_for_retry() {
        assert!(should_save_plain_dispatch_to_pending_retry_on_pressure(
            PressureLevel::Critical
        ));
        assert!(!should_save_plain_dispatch_to_pending_retry_on_pressure(
            PressureLevel::Cautious
        ));
        assert!(!should_save_plain_dispatch_to_pending_retry_on_pressure(
            PressureLevel::Normal
        ));
    }

    #[test]
    fn tls_admission_retry_delay_tracks_pressure_budget() {
        assert_eq!(
            tls_admission_retry_sleep_secs_for_pressure(PressureLevel::Normal),
            5
        );
        assert_eq!(
            tls_admission_retry_sleep_secs_for_pressure(PressureLevel::Cautious),
            15
        );
        assert_eq!(
            tls_admission_retry_sleep_secs_for_pressure(PressureLevel::Critical),
            30
        );
    }

    #[test]
    fn untrusted_wall_clock_defers_external_wss_connect() {
        assert!(should_defer_external_wss_for_wall_clock(false));
        assert!(!should_defer_external_wss_for_wall_clock(true));
    }

    #[test]
    fn known_wss_tags_are_lifecycle_owners() {
        assert_eq!(wss_lifecycle_owner("qq_ws"), "qq_ws");
        assert_eq!(wss_lifecycle_owner("feishu_ws"), "feishu_ws");
        assert_eq!(wss_lifecycle_owner("dingtalk_stream"), "dingtalk_stream");
        assert_eq!(wss_lifecycle_owner("wecom_aibot"), "wecom_aibot");
    }

    #[test]
    fn unknown_wss_tag_uses_shared_lifecycle_owner() {
        assert_eq!(wss_lifecycle_owner("custom_ws"), "external_wss");
    }

    #[test]
    fn runtime_gate_suspend_reason_distinguishes_voice_exclusive() {
        assert_eq!(
            wss_runtime_gate_suspend_reason(snapshot_from_source(RuntimeModeSource {
                voice_exclusive_active: true,
                ..RuntimeModeSource::default()
            })),
            "voice_exclusive_suspend"
        );
        assert_eq!(
            wss_runtime_gate_suspend_reason(snapshot_from_source(RuntimeModeSource {
                recovery_safe_mode_active: true,
                ..RuntimeModeSource::default()
            })),
            "runtime_mode_gate"
        );
        assert_eq!(
            wss_runtime_gate_suspend_reason(snapshot_from_source(RuntimeModeSource {
                config_active: true,
                config_activity_phase: ConfigActivityPhase::Persisting,
                ..RuntimeModeSource::default()
            })),
            "config_persisting_suspend"
        );
    }

    #[test]
    fn external_wss_worker_evict_marks_unloaded_and_exits_loop() {
        let _guard = crate::state::test_state_guard();
        crate::network::set_external_wss_managed_present(true);
        let evict = crate::network::begin_external_wss_worker_evict_request(
            crate::network::ExternalWssSuspendReason::VoiceExclusive,
        );

        assert!(super::external_wss_worker_should_exit_for_evict(
            "qq_ws", "qq_ws"
        ));
        assert!(!crate::network::external_wss_managed_present());
        assert!(crate::network::external_wss_suspended());

        drop(evict);
    }

    #[test]
    fn external_wss_session_stop_reason_includes_worker_evict() {
        let _guard = crate::state::test_state_guard();
        let evict = crate::network::begin_external_wss_worker_evict_request(
            crate::network::ExternalWssSuspendReason::OutboundHttpRecovery,
        );

        assert_eq!(
            super::external_wss_worker_evict_lifecycle_reason(),
            Some("outbound_http_recovery_suspend")
        );

        drop(evict);
    }

    #[test]
    fn wss_backoff_sleep_returns_early_when_worker_evict_is_requested() {
        let _guard = crate::state::test_state_guard();
        crate::network::set_external_wss_managed_present(true);
        let evict = crate::network::begin_external_wss_worker_evict_request(
            crate::network::ExternalWssSuspendReason::VoiceExclusive,
        );
        let started = std::time::Instant::now();

        super::sleep_with_wdt(1);

        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "evict-aware WSS backoff sleep must not block realtime admission"
        );
        drop(evict);
    }
}
