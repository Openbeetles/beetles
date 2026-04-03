//! 统一 WSS 网关循环：取 URL → 建连 → Hello/鉴权 → 心跳 + 收包入队，退避重连。
//! WiFi 断连时先等 WiFi 恢复再尝试重连 WSS，避免无网络时反复做 TLS 握手。

use crate::bus::InboundTx;
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
const ACK_SEND_DELAY_MS: u64 = 20;
/// recv_timeout 单次上限（秒）；须小于 TWDT 超时（sdkconfig 60s），
/// 避免长心跳间隔通道（如飞书 120s）在空闲时触发看门狗。
const WDT_RECV_CHUNK_SECS: u64 = 25;
/// WiFi 就绪等待上限（秒）；运行中网络断开后重连时，超出后仍尝试连接。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const WIFI_WAIT_MAX_SECS: u64 = 60;

/// 阻塞等待 WiFi STA 就绪，每 2s 轮询，最多 `WIFI_WAIT_MAX_SECS`。返回 true 表示已就绪，false 表示超时仍继续尝试。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_wifi(tag: &str) -> bool {
    if crate::state::wifi_sta_connected() {
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
        if crate::state::wifi_sta_connected() {
            log::info!("[{}] WiFi STA ready", tag);
            return true;
        }
    }
    log::warn!(
        "[{}] WiFi STA still not ready after {}s, proceeding anyway",
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
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    let mut backoff_secs = crate::orchestrator::current_budget().reconnect_backoff_secs;
    loop {
        wait_for_wifi(tag);

        let mut http = match create_http() {
            Ok(h) => h,
            Err(e) => {
                log::warn!("[{}] create_http failed: {}", tag, e);
                sleep_with_wdt(backoff_secs);
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        };
        let url = match driver.get_url(&mut http) {
            Ok(u) => u,
            Err(e) => {
                crate::metrics::record_error_by_stage(e.stage());
                log::warn!("[{}] get_url failed: {}", tag, e);
                if e.is_tls_admission() {
                    sleep_with_wdt(TLS_ADMISSION_RETRY_SLEEP_SECS);
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
                crate::metrics::record_error_by_stage(e.stage());
                log::warn!("[{}] connect failed: {}", tag, e);
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
                        log::warn!("[{}] on_hello parse failed, reconnecting: {}", tag, e);
                        drop(conn);
                        sleep_with_wdt(backoff_secs);
                        backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                        continue;
                    }
                },
                Ok(Some(WssEvent::Disconnected)) | Ok(Some(WssEvent::Closed)) => {
                    log::info!("[{}] disconnected before hello", tag);
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
                    crate::metrics::record_error_by_stage(e.stage());
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
                log::warn!("[{}] send identify failed: {}", tag, e);
                drop(conn);
                sleep_with_wdt(backoff_secs);
                backoff_secs = (backoff_secs * 2).min(BACKOFF_MAX_SECS);
                continue;
            }
        }

        let interval_ms =
            heartbeat_interval_ms.clamp(HEARTBEAT_INTERVAL_MIN_MS, HEARTBEAT_INTERVAL_MAX_MS);
        let heartbeat_interval = Duration::from_millis(interval_ms);
        let recv_chunk = heartbeat_interval.min(Duration::from_secs(WDT_RECV_CHUNK_SECS));
        let mut last_seq: Option<u64> = None;
        let mut last_heartbeat = Instant::now();
        let mut session_ended = false;

        while !session_ended {
            crate::platform::task_wdt::feed_current_task();
            match conn.recv_timeout(recv_chunk) {
                Ok(Some(WssEvent::Binary(data))) => {
                    last_heartbeat = Instant::now();
                    log::debug!("[{}] recv binary len={}", tag, data.len());
                    match driver.on_recv(data.as_slice()) {
                        Ok(WssRecvAction::Dispatch(Some(msg))) => {
                            let chat_id = msg.chat_id.clone();
                            if crate::orchestrator::current_pressure()
                                == crate::orchestrator::PressureLevel::Critical
                            {
                                log::warn!(
                                    "[{}] pressure critical, dropping msg chat_id={}",
                                    tag,
                                    chat_id
                                );
                            } else {
                                let mut enqueued = false;
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
                                    log::info!("[{}] message enqueued, chat_id={}", tag, chat_id);
                                } else {
                                    log::warn!(
                                        "[{}] inbound queue full, dropping msg chat_id={}",
                                        tag,
                                        chat_id
                                    );
                                    if let Some(m) = pending_msg.as_ref() {
                                        let _ = pending_retry.save_pending_retry(m);
                                    }
                                }
                            }
                        }
                        Ok(WssRecvAction::Dispatch(None)) => {
                            log::debug!("[{}] dispatch ignored (no msg)", tag);
                        }
                        Ok(WssRecvAction::DispatchAndAck(msg, ack)) => {
                            let enqueued = if let Some(msg) = msg {
                                let chat_id = msg.chat_id.clone();
                                if crate::orchestrator::current_pressure()
                                    == crate::orchestrator::PressureLevel::Critical
                                {
                                    log::warn!(
                                        "[{}] pressure critical, dropping msg chat_id={}, skip ack to trigger re-delivery",
                                        tag,
                                        chat_id
                                    );
                                    false
                                } else {
                                    match inbound_tx.try_send(msg) {
                                        Ok(()) => {
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
                                            false
                                        }
                                        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                                            log::error!("[{}] inbound_tx disconnected", tag);
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
                            session_ended = true;
                        }
                        Err(e) => {
                            crate::metrics::record_error_by_stage(e.stage());
                            log::warn!("[{}] on_recv failed: {}", tag, e);
                        }
                    }
                }
                Ok(Some(WssEvent::Disconnected)) | Ok(Some(WssEvent::Closed)) => {
                    log::info!("[{}] wss disconnected or closed", tag);
                    session_ended = true;
                }
                Ok(None) => {
                    if last_heartbeat.elapsed() >= heartbeat_interval {
                        let payload = match driver.build_heartbeat(last_seq) {
                            Ok(p) => p,
                            Err(e) => {
                                log::warn!("[{}] build_heartbeat failed: {}", tag, e);
                                continue;
                            }
                        };
                        if !payload.is_empty() {
                            log::debug!("[{}] send heartbeat len={}", tag, payload.len());
                            if conn.send_binary_owned(payload).is_err() {
                                log::warn!("[{}] send heartbeat failed", tag);
                                session_ended = true;
                            }
                        }
                        last_heartbeat = Instant::now();
                    }
                }
                Err(e) => {
                    crate::metrics::record_error_by_stage(e.stage());
                    log::warn!("[{}] recv failed: {}", tag, e);
                    session_ended = true;
                }
            }
        }

        log::info!(
            "[{}] disconnected, dropping connection before reconnect",
            tag
        );
        drop(conn);
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
    let chunk = Duration::from_secs(10);
    let start = Instant::now();
    while start.elapsed() < total {
        let remaining = total.saturating_sub(start.elapsed());
        std::thread::sleep(remaining.min(chunk));
        crate::platform::task_wdt::feed_current_task();
    }
}
