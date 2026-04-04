//! ESP 平台 WSS 传输：直接使用 `esp_websocket_client` C API，实现 `WssConnection`。
//! 仅 xtensa/riscv32 编译；供飞书/QQ WSS 入站共用。
//! 与根 `Cargo.toml` 中 `[package.metadata.esp-idf-sys] extra_components` 的
//! `espressif/esp_websocket_client` 配套；相对「仅 `platform/` 依赖 esp-idf-svc」为固定例外。

#![cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]

use crate::channels::wss_gateway::connection::{
    WssBinary, WssConnectProfile, WssConnection, WssEvent,
};
use crate::error::{Error, Result};
use esp_idf_svc::hal::delay::TickType;
use esp_idf_svc::sys;
use std::collections::VecDeque;
use std::ffi::CString;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

/// TCP keep-alive：30s idle + 3*10s probe = 最慢 60s 检测到死连接。
const KEEPALIVE_IDLE_SECS: u64 = 30;
const KEEPALIVE_INTERVAL_SECS: u64 = 10;
const KEEPALIVE_COUNT: u16 = 3;
/// close 超时 tick。
const CLOSE_TIMEOUT_TICKS: u32 = 200;
/// callback state 延迟释放窗口。
///
/// `esp_websocket_client_destroy()` 未提供显式 unregister API；为避免销毁尾声若仍有回调落到
/// 已释放指针上，Rust 侧将 callback state 再保留一小段宽限期。
const CALLBACK_STATE_RECLAIM_DELAY_MS: u64 = 2_000;
/// 缓冲池容量：WSS 事件负载复用池上限；覆盖网关 + realtime 并发场景，避免高频帧反复分配。
const EVENT_BUF_POOL_MAX: usize = 8;
const MAX_RECYCLED_EVENT_BUFFER_BYTES: usize = 8 * 1024;
static EVENT_BUF_POOL: OnceLock<Mutex<Vec<Vec<u8>>>> = OnceLock::new();

struct EspWssTuning {
    buffer_size: usize,
    max_send_payload_bytes: usize,
    connect_timeout: Duration,
    network_timeout_ms: u64,
    pingpong_timeout_sec: u64,
    ping_interval_sec: u32,
    event_queue_capacity: usize,
}

impl EspWssTuning {
    fn for_profile(profile: WssConnectProfile) -> Self {
        match profile {
            WssConnectProfile::Gateway => Self {
                buffer_size: 4096,
                max_send_payload_bytes: 4096 - 32,
                connect_timeout: Duration::from_secs(10),
                network_timeout_ms: 30_000,
                pingpong_timeout_sec: 60,
                ping_interval_sec: 10,
                event_queue_capacity: 32,
            },
            WssConnectProfile::Realtime => Self {
                buffer_size: 4096,
                max_send_payload_bytes: 4096 - 32,
                connect_timeout: Duration::from_secs(10),
                network_timeout_ms: 15_000,
                pingpong_timeout_sec: 20,
                ping_interval_sec: 5,
                event_queue_capacity: 64,
            },
        }
    }
}

fn take_event_buf(min_capacity: usize) -> Vec<u8> {
    let pool = EVENT_BUF_POOL.get_or_init(|| Mutex::new(Vec::with_capacity(EVENT_BUF_POOL_MAX)));
    if let Ok(mut guard) = pool.lock() {
        while let Some(mut buf) = guard.pop() {
            if buf.capacity() < min_capacity {
                buf.reserve(min_capacity.saturating_sub(buf.capacity()));
            }
            if buf.capacity() >= min_capacity {
                return buf;
            }
        }
    }
    Vec::with_capacity(min_capacity)
}

fn recycle_event_buf(mut buf: Vec<u8>) {
    if buf.capacity() > MAX_RECYCLED_EVENT_BUFFER_BYTES {
        return;
    }
    buf.clear();
    let pool = EVENT_BUF_POOL.get_or_init(|| Mutex::new(Vec::with_capacity(EVENT_BUF_POOL_MAX)));
    if let Ok(mut guard) = pool.lock() {
        if guard.len() < EVENT_BUF_POOL_MAX {
            guard.push(buf);
        }
    }
}

struct CallbackState {
    tx: mpsc::SyncSender<EspConnEvent>,
}

enum EspConnEvent {
    Connected,
    Event(WssEvent),
}

fn defer_callback_state_release(state: Box<CallbackState>) {
    if let Err(task) = crate::runtime::schedule_critical_delayed_task(
        Instant::now() + Duration::from_millis(CALLBACK_STATE_RECLAIM_DELAY_MS),
        Box::new(move || drop(state)),
    ) {
        std::mem::forget(task);
        log::error!("[wss] critical delayed release queue full; callback state retained");
    }
}

/// ESP 上的 WSS 连接。
///
/// `disable_auto_reconnect: true` 禁止 C 底层自动重连（否则它会绕过 TLS 准入做 TLS 握手）。
/// 这里不再依赖 `esp-idf-svc::ws::client::EspWebSocketClient` 的 Drop，
/// 避免其在断线重连路径上 `close().unwrap()` 触发 panic，并彻底消除 callback Box 泄漏。
pub struct EspWssConnection {
    handle: sys::esp_websocket_client_handle_t,
    send_timeout_ticks: sys::TickType_t,
    max_send_payload_bytes: usize,
    callback_state: Option<Box<CallbackState>>,
    rx: mpsc::Receiver<EspConnEvent>,
    pending_events: VecDeque<WssEvent>,
}

unsafe impl Send for EspWssConnection {}

impl Drop for EspWssConnection {
    fn drop(&mut self) {
        unsafe {
            let connected = sys::esp_websocket_client_is_connected(self.handle);
            let rc_shutdown = if connected {
                sys::esp_websocket_client_close(self.handle, CLOSE_TIMEOUT_TICKS as _)
            } else {
                sys::esp_websocket_client_stop(self.handle)
            };
            if rc_shutdown != sys::ESP_OK
                && rc_shutdown != sys::ESP_FAIL
                && rc_shutdown != sys::ESP_ERR_INVALID_STATE
            {
                log::warn!("[wss] websocket shutdown rc={}", rc_shutdown);
            }
            if connected && rc_shutdown != sys::ESP_OK {
                let rc_stop = sys::esp_websocket_client_stop(self.handle);
                if rc_stop != sys::ESP_OK
                    && rc_stop != sys::ESP_FAIL
                    && rc_stop != sys::ESP_ERR_INVALID_STATE
                {
                    log::warn!("[wss] websocket stop-after-close rc={}", rc_stop);
                }
            }
            let rc_destroy = sys::esp_websocket_client_destroy(self.handle);
            if rc_destroy != sys::ESP_OK {
                log::warn!("[wss] websocket destroy rc={}", rc_destroy);
            }
        }
        if let Some(state) = self.callback_state.take() {
            defer_callback_state_release(state);
        }
    }
}

impl EspWssConnection {
    fn recv_to_event(
        r: std::result::Result<EspConnEvent, mpsc::RecvTimeoutError>,
    ) -> Result<Option<WssEvent>> {
        match r {
            Ok(EspConnEvent::Event(ev)) => Ok(Some(ev)),
            Ok(EspConnEvent::Connected) => Ok(None),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "wss event channel disconnected",
                )),
                stage: "wss_esp_recv",
            }),
        }
    }

    fn wait_until_connected(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(wait) {
                Ok(EspConnEvent::Connected) => return Ok(()),
                Ok(EspConnEvent::Event(WssEvent::Binary(data))) => {
                    self.pending_events.push_back(WssEvent::Binary(data));
                }
                Ok(EspConnEvent::Event(WssEvent::Disconnected))
                | Ok(EspConnEvent::Event(WssEvent::Closed)) => {
                    return Err(Error::config(
                        "wss_esp_connect",
                        "websocket closed before handshake completed",
                    ));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(Error::config(
                        "wss_esp_connect",
                        "websocket event channel closed during connect",
                    ));
                }
            }
        }
        Err(Error::config(
            "wss_esp_connect",
            format!(
                "websocket connect timed out after {}ms",
                timeout.as_millis()
            ),
        ))
    }
}

impl WssConnection for EspWssConnection {
    fn send_binary(&mut self, data: &[u8]) -> Result<()> {
        if data.len() > self.max_send_payload_bytes {
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "wss payload too large: {} > {}",
                        data.len(),
                        self.max_send_payload_bytes
                    ),
                )),
                stage: "wss_esp_send",
            });
        }
        let rc = unsafe {
            sys::esp_websocket_client_send_bin(
                self.handle,
                data.as_ptr() as *const core::ffi::c_char,
                data.len() as i32,
                self.send_timeout_ticks,
            )
        };
        if rc < 0 {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "esp_websocket_client_send_bin rc={rc}"
                ))),
                stage: "wss_esp_send",
            });
        }
        Ok(())
    }

    fn send_text(&mut self, text: &str) -> Result<()> {
        if text.len() > self.max_send_payload_bytes {
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "wss payload too large: {} > {}",
                        text.len(),
                        self.max_send_payload_bytes
                    ),
                )),
                stage: "wss_esp_send",
            });
        }
        let rc = unsafe {
            sys::esp_websocket_client_send_text(
                self.handle,
                text.as_ptr() as *const core::ffi::c_char,
                text.len() as i32,
                self.send_timeout_ticks,
            )
        };
        if rc < 0 {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(format!(
                    "esp_websocket_client_send_text rc={rc}"
                ))),
                stage: "wss_esp_send",
            });
        }
        Ok(())
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WssEvent>> {
        if let Some(event) = self.pending_events.pop_front() {
            return Ok(Some(event));
        }
        Self::recv_to_event(self.rx.recv_timeout(timeout))
    }
}

extern "C" fn handle_ws_event(
    event_handler_arg: *mut core::ffi::c_void,
    _event_base: sys::esp_event_base_t,
    event_id: i32,
    event_data: *mut core::ffi::c_void,
) {
    let state = unsafe { (event_handler_arg as *mut CallbackState).as_ref() };
    let Some(state) = state else {
        return;
    };
    let event =
        unsafe { map_ws_event(event_id, event_data as *mut sys::esp_websocket_event_data_t) };
    if let Some(event) = event {
        if state.tx.try_send(event).is_err() {
            log::warn!("[wss] event channel full, dropping event");
        }
    }
}

unsafe fn map_ws_event(
    event_id: i32,
    event_data: *mut sys::esp_websocket_event_data_t,
) -> Option<EspConnEvent> {
    match event_id {
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_ERROR => {
            if let Some(data) = event_data.as_ref() {
                log::warn!(
                    "[wss] websocket error type={} tls_err={} stack_err={} errno={}",
                    data.error_handle.error_type,
                    data.error_handle.esp_tls_last_esp_err,
                    data.error_handle.esp_tls_stack_err,
                    data.error_handle.esp_transport_sock_errno
                );
            }
            Some(EspConnEvent::Event(WssEvent::Disconnected))
        }
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_CONNECTED => Some(EspConnEvent::Connected),
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_BEFORE_CONNECT
        | sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_BEGIN
        | sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_FINISH => None,
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_DISCONNECTED => {
            Some(EspConnEvent::Event(WssEvent::Disconnected))
        }
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_CLOSED => {
            Some(EspConnEvent::Event(WssEvent::Closed))
        }
        sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_DATA => {
            let data = event_data.as_ref()?;
            match data.op_code {
                1 | 2 => {
                    let len = data.data_len.max(0) as usize;
                    let ptr = data.data_ptr as *const u8;
                    if ptr.is_null() && len > 0 {
                        log::warn!("[wss] websocket data event has null payload pointer");
                        return Some(EspConnEvent::Event(WssEvent::Disconnected));
                    }
                    let mut buf = take_event_buf(len);
                    if len > 0 {
                        let bytes = std::slice::from_raw_parts(ptr, len);
                        buf.extend_from_slice(bytes);
                    }
                    Some(EspConnEvent::Event(WssEvent::Binary(
                        WssBinary::from_vec_with_recycler(buf, recycle_event_buf),
                    )))
                }
                8 => Some(EspConnEvent::Event(WssEvent::Closed)),
                9 | 10 => None,
                opcode => {
                    log::debug!("[wss] ignore websocket opcode={}", opcode);
                    None
                }
            }
        }
        _ => None,
    }
}

/// 与 HTTP 客户端 TLS 准入窗口对齐，避免 HTTP 长请求期间 WSS 过早饿死。
const WSS_TLS_ADMISSION_TIMEOUT_SECS: u64 = 30;

pub fn connect_esp_wss(url: &str) -> Result<EspWssConnection> {
    connect_esp_wss_with_profile(url, WssConnectProfile::Gateway)
}

pub fn connect_esp_wss_with_profile(
    url: &str,
    profile: WssConnectProfile,
) -> Result<EspWssConnection> {
    connect_esp_wss_with_headers_and_profile(url, &[], profile)
}

pub fn connect_esp_wss_with_headers_and_profile(
    url: &str,
    headers: &[(&str, &str)],
    profile: WssConnectProfile,
) -> Result<EspWssConnection> {
    let _permit = crate::orchestrator::request_http_permit(
        crate::orchestrator::Priority::High,
        Duration::from_secs(WSS_TLS_ADMISSION_TIMEOUT_SECS),
    )?;

    let url_c = CString::new(url).map_err(|e| Error::config("wss_esp_connect", e.to_string()))?;
    let header_block = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let headers_c = if header_block.is_empty() {
        None
    } else {
        Some(
            CString::new(header_block)
                .map_err(|e| Error::config("wss_esp_connect", e.to_string()))?,
        )
    };
    let tuning = EspWssTuning::for_profile(profile);
    let send_timeout_ticks = TickType::from(tuning.connect_timeout).0;

    let mut config = sys::esp_websocket_client_config_t::default();
    config.uri = url_c.as_ptr();
    if let Some(headers_c) = headers_c.as_ref() {
        config.headers = headers_c.as_ptr();
    }
    config.buffer_size = tuning.buffer_size as i32;
    config.transport = sys::esp_websocket_transport_t_WEBSOCKET_TRANSPORT_OVER_SSL;
    config.use_global_ca_store = false;
    config.disable_auto_reconnect = true;
    #[cfg(not(esp_idf_version_major = "4"))]
    {
        config.crt_bundle_attach = Some(sys::esp_crt_bundle_attach);
    }
    config.pingpong_timeout_sec = tuning.pingpong_timeout_sec as i32;
    config.network_timeout_ms = tuning.network_timeout_ms as i32;
    config.ping_interval_sec = tuning.ping_interval_sec as usize;
    config.keep_alive_enable = true;
    config.keep_alive_idle = KEEPALIVE_IDLE_SECS as i32;
    config.keep_alive_interval = KEEPALIVE_INTERVAL_SECS as i32;
    config.keep_alive_count = KEEPALIVE_COUNT as i32;

    let handle = unsafe { sys::esp_websocket_client_init(&config) };
    if handle.is_null() {
        return Err(Error::esp("wss_esp_connect", sys::ESP_FAIL));
    }

    let (tx, rx) = mpsc::sync_channel::<EspConnEvent>(tuning.event_queue_capacity);
    let mut callback_state = Box::new(CallbackState { tx });
    let callback_ptr = callback_state.as_mut() as *mut CallbackState as *mut core::ffi::c_void;

    let rc = unsafe {
        sys::esp_websocket_register_events(
            handle,
            sys::esp_websocket_event_id_t_WEBSOCKET_EVENT_ANY,
            Some(handle_ws_event),
            callback_ptr,
        )
    };
    if rc != sys::ESP_OK {
        unsafe {
            let _ = sys::esp_websocket_client_destroy(handle);
        }
        defer_callback_state_release(callback_state);
        return Err(Error::esp("wss_esp_connect", rc));
    }

    let rc = unsafe { sys::esp_websocket_client_start(handle) };
    if rc != sys::ESP_OK {
        unsafe {
            let _ = sys::esp_websocket_client_destroy(handle);
        }
        defer_callback_state_release(callback_state);
        return Err(Error::esp("wss_esp_connect", rc));
    }

    log::info!(
        "wss client started, waiting for handshake completion, url_len={}",
        url.len()
    );
    let mut conn = EspWssConnection {
        handle,
        send_timeout_ticks,
        max_send_payload_bytes: tuning.max_send_payload_bytes,
        callback_state: Some(callback_state),
        rx,
        pending_events: VecDeque::new(),
    };
    if let Err(e) = conn.wait_until_connected(tuning.connect_timeout) {
        drop(conn);
        return Err(e);
    }
    Ok(conn)
}
