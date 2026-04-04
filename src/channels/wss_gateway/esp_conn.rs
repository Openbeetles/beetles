//! ESP 平台 WSS 传输：使用本地 `beetle_wss` C 组件，实现同步、单所有权的 `WssConnection`。
//! 仅 xtensa/riscv32 编译；供飞书/QQ 网关与 realtime 语音复用。

#![cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]

use crate::channels::wss_gateway::connection::{
    WssBinary, WssConnectProfile, WssConnection, WssEvent,
};
use crate::error::{Error, Result};
use std::ffi::{c_char, CString};
use std::time::Duration;

const WSS_TLS_ADMISSION_TIMEOUT_SECS: u64 = 30;
const WSS_CLOSE_TIMEOUT_MS: u32 = 1_000;

const BEETLE_WSS_OK: i32 = 0;
const BEETLE_WSS_TIMEOUT: i32 = 1;
const BEETLE_WSS_CLOSED: i32 = 2;
const BEETLE_WSS_DISCONNECTED: i32 = 3;
const BEETLE_WSS_ERR_INVALID_ARG: i32 = -1;
const BEETLE_WSS_ERR_NOMEM: i32 = -2;
const BEETLE_WSS_ERR_TLS: i32 = -3;
const BEETLE_WSS_ERR_IO: i32 = -4;
const BEETLE_WSS_ERR_PROTOCOL: i32 = -5;
const BEETLE_WSS_ERR_STATE: i32 = -6;

#[repr(C)]
struct BeetleWssClient {
    _private: [u8; 0],
}

#[repr(C)]
struct BeetleWssConfig {
    url: *const c_char,
    extra_headers: *const c_char,
    connect_timeout_ms: u32,
    io_timeout_ms: u32,
    max_message_bytes: u32,
}

#[repr(C)]
struct BeetleWssEvent {
    data: *mut u8,
    len: usize,
}

unsafe extern "C" {
    fn beetle_wss_connect(
        config: *const BeetleWssConfig,
        out_client: *mut *mut BeetleWssClient,
    ) -> i32;
    fn beetle_wss_send_binary(
        client: *mut BeetleWssClient,
        data: *const u8,
        len: usize,
        timeout_ms: u32,
    ) -> i32;
    fn beetle_wss_send_text(
        client: *mut BeetleWssClient,
        text: *const c_char,
        len: usize,
        timeout_ms: u32,
    ) -> i32;
    fn beetle_wss_recv(
        client: *mut BeetleWssClient,
        timeout_ms: u32,
        out_event: *mut BeetleWssEvent,
    ) -> i32;
    fn beetle_wss_free_event(event: *mut BeetleWssEvent);
    fn beetle_wss_close(client: *mut BeetleWssClient, timeout_ms: u32);
    fn beetle_wss_destroy(client: *mut BeetleWssClient);
}

struct EspWssTuning {
    connect_timeout: Duration,
    io_timeout: Duration,
    send_timeout: Duration,
    max_message_bytes: usize,
    max_send_payload_bytes: usize,
}

impl EspWssTuning {
    fn for_profile(profile: WssConnectProfile) -> Self {
        match profile {
            WssConnectProfile::Gateway => Self {
                connect_timeout: Duration::from_secs(10),
                io_timeout: Duration::from_secs(30),
                send_timeout: Duration::from_secs(10),
                max_message_bytes: 128 * 1024,
                max_send_payload_bytes: 64 * 1024,
            },
            WssConnectProfile::Realtime => Self {
                connect_timeout: Duration::from_secs(10),
                io_timeout: Duration::from_secs(15),
                send_timeout: Duration::from_secs(5),
                max_message_bytes: 256 * 1024,
                max_send_payload_bytes: 128 * 1024,
            },
        }
    }
}

fn duration_to_timeout_ms(timeout: Duration) -> u32 {
    timeout.as_millis().min(u32::MAX as u128) as u32
}

fn status_to_error(stage: &'static str, status: i32) -> Error {
    match status {
        BEETLE_WSS_ERR_INVALID_ARG => Error::config(stage, "invalid beetle_wss argument"),
        BEETLE_WSS_ERR_NOMEM => Error::Other {
            source: Box::new(std::io::Error::other("beetle_wss out of memory")),
            stage,
        },
        BEETLE_WSS_ERR_TLS => Error::Other {
            source: Box::new(std::io::Error::other("beetle_wss tls connect failed")),
            stage,
        },
        BEETLE_WSS_ERR_IO => Error::Other {
            source: Box::new(std::io::Error::other("beetle_wss io failed")),
            stage,
        },
        BEETLE_WSS_ERR_PROTOCOL => Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "beetle_wss protocol violation",
            )),
            stage,
        },
        BEETLE_WSS_ERR_STATE => Error::Other {
            source: Box::new(std::io::Error::other("beetle_wss invalid state")),
            stage,
        },
        BEETLE_WSS_CLOSED | BEETLE_WSS_DISCONNECTED => Error::Other {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "beetle_wss peer closed",
            )),
            stage,
        },
        other => Error::Other {
            source: Box::new(std::io::Error::other(format!(
                "beetle_wss unexpected status={other}"
            ))),
            stage,
        },
    }
}

fn validate_headers(headers: &[(&str, &str)]) -> Result<()> {
    for (name, value) in headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err(Error::config(
                "wss_esp_connect",
                "header contains forbidden newline",
            ));
        }
    }
    Ok(())
}

fn build_header_block(headers: &[(&str, &str)]) -> Result<Option<CString>> {
    validate_headers(headers)?;
    if headers.is_empty() {
        return Ok(None);
    }

    let header_block = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    Ok(Some(CString::new(header_block).map_err(|e| {
        Error::config("wss_esp_connect", e.to_string())
    })?))
}

pub struct EspWssConnection {
    raw: *mut BeetleWssClient,
    send_timeout_ms: u32,
    max_send_payload_bytes: usize,
    _wss_session_guard: Option<crate::orchestrator::WssSessionGuard>,
}

unsafe impl Send for EspWssConnection {}

impl Drop for EspWssConnection {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        unsafe {
            beetle_wss_close(self.raw, WSS_CLOSE_TIMEOUT_MS);
            beetle_wss_destroy(self.raw);
        }
        self.raw = std::ptr::null_mut();
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

        let status = unsafe {
            beetle_wss_send_binary(self.raw, data.as_ptr(), data.len(), self.send_timeout_ms)
        };
        if status == BEETLE_WSS_OK {
            return Ok(());
        }
        Err(status_to_error("wss_esp_send", status))
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

        let status = unsafe {
            beetle_wss_send_text(
                self.raw,
                text.as_ptr().cast::<c_char>(),
                text.len(),
                self.send_timeout_ms,
            )
        };
        if status == BEETLE_WSS_OK {
            return Ok(());
        }
        Err(status_to_error("wss_esp_send", status))
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WssEvent>> {
        let mut event = BeetleWssEvent {
            data: std::ptr::null_mut(),
            len: 0,
        };
        let status =
            unsafe { beetle_wss_recv(self.raw, duration_to_timeout_ms(timeout), &mut event) };

        match status {
            BEETLE_WSS_OK => {
                let data = if event.len == 0 {
                    Vec::new()
                } else if event.data.is_null() {
                    unsafe {
                        beetle_wss_free_event(&mut event);
                    }
                    return Err(Error::Other {
                        source: Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "beetle_wss returned null payload for non-empty event",
                        )),
                        stage: "wss_esp_recv",
                    });
                } else {
                    unsafe { std::slice::from_raw_parts(event.data, event.len) }.to_vec()
                };
                unsafe {
                    beetle_wss_free_event(&mut event);
                }
                Ok(Some(WssEvent::Binary(WssBinary::from_vec(data))))
            }
            BEETLE_WSS_TIMEOUT => Ok(None),
            BEETLE_WSS_CLOSED => Ok(Some(WssEvent::Closed)),
            BEETLE_WSS_DISCONNECTED => Ok(Some(WssEvent::Disconnected)),
            other => {
                unsafe {
                    beetle_wss_free_event(&mut event);
                }
                Err(status_to_error("wss_esp_recv", other))
            }
        }
    }
}

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

    let tuning = EspWssTuning::for_profile(profile);
    let url_c = CString::new(url).map_err(|e| Error::config("wss_esp_connect", e.to_string()))?;
    let headers_c = build_header_block(headers)?;
    let config = BeetleWssConfig {
        url: url_c.as_ptr(),
        extra_headers: headers_c.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
        connect_timeout_ms: duration_to_timeout_ms(tuning.connect_timeout),
        io_timeout_ms: duration_to_timeout_ms(tuning.io_timeout),
        max_message_bytes: tuning.max_message_bytes.min(u32::MAX as usize) as u32,
    };

    let mut raw = std::ptr::null_mut::<BeetleWssClient>();
    let status = unsafe { beetle_wss_connect(&config, &mut raw) };
    if status != BEETLE_WSS_OK {
        return Err(status_to_error("wss_esp_connect", status));
    }
    if raw.is_null() {
        return Err(Error::Other {
            source: Box::new(std::io::Error::other(
                "beetle_wss returned null client handle",
            )),
            stage: "wss_esp_connect",
        });
    }

    Ok(EspWssConnection {
        raw,
        send_timeout_ms: duration_to_timeout_ms(tuning.send_timeout),
        max_send_payload_bytes: tuning.max_send_payload_bytes,
        _wss_session_guard: Some(crate::orchestrator::begin_wss_session()),
    })
}
