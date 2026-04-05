//! Linux / host：基于 `tungstenite` + rustls 的 WSS 客户端，实现 `WssConnection`。
//! 与 `esp_conn` 事件语义对齐：Binary/Text → `WssEvent::Binary`，Close/错误 → Disconnected。
//!
//! **不得**用单独读线程在 `read()` 持有 `Mutex` 的同时由主线程 `send`：QQ 等协议在 Hello 后需先发
//! Identify，服务端才会继续下帧；否则读线程永久占锁 → 与 `send_binary` 死锁。网关循环单线程交替
//! `recv_timeout` / `send_binary`，故此处直接在调用线程上读、写同一 `WebSocket`。
//!
//! `TcpStream::set_read_timeout` 单次等待上限与 `loop.rs` 中 `WDT_RECV_CHUNK_SECS` 同量级；
//! `recv_timeout` 用截止时间聚合多次短读，避免 Ping 处理或分片读越过调用方超时。

#![cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]

use std::io::ErrorKind;
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

use crate::channels::wss_gateway::connection::{
    MAX_WSS_SEND_PAYLOAD_BYTES, WssBinary, WssCloseInfo, WssConnectProfile, WssConnection, WssEvent,
};
use crate::error::{Error, Result};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{WebSocket, client::IntoClientRequest, protocol::Message};

struct LinuxWssTuning {
    tls_admission_timeout_secs: u64,
    socket_read_timeout_secs: u64,
    socket_write_timeout_secs: u64,
    tcp_connect_timeout_secs: u64,
}

impl LinuxWssTuning {
    fn for_profile(profile: WssConnectProfile) -> Self {
        match profile {
            WssConnectProfile::Gateway => Self {
                tls_admission_timeout_secs: 10,
                socket_read_timeout_secs: 25,
                socket_write_timeout_secs: 15,
                tcp_connect_timeout_secs: 15,
            },
            WssConnectProfile::Realtime => Self {
                tls_admission_timeout_secs: 10,
                socket_read_timeout_secs: 10,
                socket_write_timeout_secs: 10,
                tcp_connect_timeout_secs: 10,
            },
        }
    }
}

fn map_io(stage: &'static str, e: std::io::Error) -> Error {
    Error::Other {
        source: Box::new(e),
        stage,
    }
}

fn map_tungstenite(stage: &'static str, e: tungstenite::Error) -> Error {
    Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage,
    }
}

fn set_tcp_read_timeout(
    stream: &mut MaybeTlsStream<TcpStream>,
    d: Option<Duration>,
) -> std::io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(d),
        MaybeTlsStream::Rustls(s) => s.sock.set_read_timeout(d),
        _ => Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "unexpected MaybeTlsStream variant for WSS",
        )),
    }
}

fn shutdown_tcp(stream: &mut MaybeTlsStream<TcpStream>) {
    let r = match stream {
        MaybeTlsStream::Plain(s) => s.shutdown(Shutdown::Both),
        MaybeTlsStream::Rustls(s) => s.sock.shutdown(Shutdown::Both),
        _ => Ok(()),
    };
    if let Err(e) = r {
        log::debug!("[wss_linux] tcp shutdown: {}", e);
    }
}

fn is_timed_out_or_would_block(e: &tungstenite::Error) -> bool {
    match e {
        tungstenite::Error::Io(io) => {
            matches!(io.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
        }
        _ => false,
    }
}

/// Linux 上基于 tungstenite 的 WSS 连接（单线程读/写，与 `run_wss_gateway_loop` 用法一致）。
pub struct LinuxWssConnection {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    last_read_timeout: Option<Duration>,
    read_timeout_cap: Duration,
    _wss_session_guard: Option<crate::orchestrator::WssSessionGuard>,
}

impl Drop for LinuxWssConnection {
    fn drop(&mut self) {
        shutdown_tcp(self.ws.get_mut());
    }
}

impl WssConnection for LinuxWssConnection {
    fn send_binary(&mut self, data: &[u8]) -> Result<()> {
        self.send_binary_owned(data.to_vec())
    }

    fn send_text(&mut self, text: &str) -> Result<()> {
        self.ws
            .send(Message::Text(text.to_string().into()))
            .map_err(|e| map_tungstenite("wss_linux_send", e))?;
        Ok(())
    }

    fn send_binary_owned(&mut self, data: Vec<u8>) -> Result<()> {
        if data.len() > MAX_WSS_SEND_PAYLOAD_BYTES {
            return Err(Error::Other {
                source: Box::new(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "wss payload too large: {} > {}",
                        data.len(),
                        MAX_WSS_SEND_PAYLOAD_BYTES
                    ),
                )),
                stage: "wss_linux_send",
            });
        }
        self.ws
            .send(Message::Binary(data))
            .map_err(|e| map_tungstenite("wss_linux_send", e))?;
        Ok(())
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WssEvent>> {
        let deadline = Instant::now() + timeout;
        let chunk_cap = self.read_timeout_cap;

        loop {
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let remaining = deadline.saturating_duration_since(now);
            let read_wait = remaining.min(chunk_cap);
            if read_wait.is_zero() {
                return Ok(None);
            }
            if self.last_read_timeout != Some(read_wait) {
                set_tcp_read_timeout(self.ws.get_mut(), Some(read_wait))
                    .map_err(|e| map_io("wss_linux_recv", e))?;
                self.last_read_timeout = Some(read_wait);
            }

            match self.ws.read() {
                Ok(Message::Binary(b)) => {
                    return Ok(Some(WssEvent::Binary(WssBinary::from_vec(b))));
                }
                Ok(Message::Text(t)) => {
                    return Ok(Some(WssEvent::Binary(WssBinary::from_vec(t.into_bytes()))));
                }
                Ok(Message::Ping(payload)) => {
                    if let Err(e) = self.ws.send(Message::Pong(payload)) {
                        log::debug!("[wss_linux] pong reply failed: {}", e);
                    }
                    if let Err(e) = self.ws.flush() {
                        log::debug!("[wss_linux] flush after pong failed: {}", e);
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    return Ok(Some(WssEvent::Closed(frame.map(|frame| WssCloseInfo {
                        code: Some(frame.code.into()),
                        reason: (!frame.reason.trim().is_empty()).then(|| frame.reason.to_string()),
                    }))));
                }
                Ok(Message::Frame(_)) => {}
                Err(e) if is_timed_out_or_would_block(&e) => continue,
                Err(e @ tungstenite::Error::ConnectionClosed) => {
                    log::debug!("[wss_linux] read ended: {}", e);
                    return Ok(Some(WssEvent::Closed(None)));
                }
                Err(e @ tungstenite::Error::AlreadyClosed) => {
                    log::debug!("[wss_linux] read ended: {}", e);
                    return Ok(Some(WssEvent::Disconnected));
                }
                Err(e) => {
                    log::debug!("[wss_linux] read ended: {}", e);
                    return Ok(Some(WssEvent::Disconnected));
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn connect_linux_wss_with_headers(
    url: &str,
    headers: &[(&str, &str)],
) -> Result<LinuxWssConnection> {
    connect_linux_wss_with_headers_and_profile(url, headers, WssConnectProfile::Gateway)
}

pub fn connect_linux_wss_with_profile(
    url: &str,
    profile: WssConnectProfile,
) -> Result<LinuxWssConnection> {
    connect_linux_wss_with_headers_and_profile(url, &[], profile)
}

pub fn connect_linux_wss_with_headers_and_profile(
    url: &str,
    headers: &[(&str, &str)],
    profile: WssConnectProfile,
) -> Result<LinuxWssConnection> {
    let tuning = LinuxWssTuning::for_profile(profile);
    let _permit = crate::orchestrator::request_http_permit(
        crate::orchestrator::Priority::Normal,
        Duration::from_secs(tuning.tls_admission_timeout_secs),
    )?;

    let tcp = tcp_connect_with_timeout(url, tuning.tcp_connect_timeout_secs)?;
    tcp.set_read_timeout(Some(Duration::from_secs(tuning.socket_read_timeout_secs)))
        .map_err(|e| map_io("wss_linux_connect", e))?;
    tcp.set_write_timeout(Some(Duration::from_secs(tuning.socket_write_timeout_secs)))
        .map_err(|e| map_io("wss_linux_connect", e))?;
    tcp.set_nodelay(true)
        .map_err(|e| map_io("wss_linux_connect", e))?;

    let mut request = url
        .into_client_request()
        .map_err(|e| Error::config("wss_linux_connect", e.to_string()))?;
    {
        let req_headers = request.headers_mut();
        for (name, value) in headers {
            let header_name = tungstenite::http::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| Error::config("wss_linux_connect", e.to_string()))?;
            let header_value = tungstenite::http::HeaderValue::from_str(value)
                .map_err(|e| Error::config("wss_linux_connect", e.to_string()))?;
            req_headers.insert(header_name, header_value);
        }
    }

    let (ws, _resp) = tungstenite::client_tls(request, tcp).map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "wss_linux_connect",
    })?;

    Ok(LinuxWssConnection {
        ws,
        last_read_timeout: Some(Duration::from_secs(tuning.socket_read_timeout_secs)),
        read_timeout_cap: Duration::from_secs(tuning.socket_read_timeout_secs),
        _wss_session_guard: Some(crate::orchestrator::begin_wss_session()),
    })
}

/// 建立 WSS 连接（`wss://`）；与 ESP 相同在握手前申请 orchestrator TLS 准入。
///
/// 使用 `TcpStream::connect_timeout` 限制 TCP 建连时间（避免默认 127s+ SYN 超时），
/// 并设置 read / write timeout 防止后续 `ws.send()` / `ws.read()` 无限阻塞。
pub fn connect_linux_wss(url: &str) -> Result<LinuxWssConnection> {
    connect_linux_wss_with_profile(url, WssConnectProfile::Gateway)
}

/// 从 `wss://host:port/path` 中提取 `host:port`（默认 443）。
fn parse_wss_host_port(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return None;
    }
    if authority.contains(':') {
        Some(authority.to_string())
    } else {
        Some(format!("{}:443", authority))
    }
}

fn tcp_connect_with_timeout(url: &str, timeout_secs: u64) -> Result<TcpStream> {
    use std::net::ToSocketAddrs;

    let host_port = parse_wss_host_port(url).ok_or_else(|| Error::Other {
        source: Box::new(std::io::Error::new(
            ErrorKind::InvalidInput,
            "cannot parse host from wss url",
        )),
        stage: "wss_linux_connect",
    })?;
    let addr = host_port
        .to_socket_addrs()
        .map_err(|e| map_io("wss_linux_dns", e))?
        .next()
        .ok_or_else(|| Error::Other {
            source: Box::new(std::io::Error::new(
                ErrorKind::AddrNotAvailable,
                "dns resolved to nothing",
            )),
            stage: "wss_linux_dns",
        })?;

    TcpStream::connect_timeout(&addr, Duration::from_secs(timeout_secs))
        .map_err(|e| map_io("wss_linux_tcp_connect", e))
}
