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
    WssBinary, WssConnection, WssEvent, MAX_WSS_SEND_PAYLOAD_BYTES,
};
use crate::error::{Error, Result};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{protocol::Message, WebSocket};

/// 与 `esp_conn` 一致。
const WSS_TLS_ADMISSION_TIMEOUT_SECS: u64 = 10;
/// 与 `wss_gateway/loop.rs` 中 `WDT_RECV_CHUNK_SECS` 一致（单次 `read` 阻塞上限）。
const SOCKET_READ_TIMEOUT_SECS: u64 = 25;
/// TCP write 超时；防止网络异常时 `ws.send()` 无限阻塞。
const SOCKET_WRITE_TIMEOUT_SECS: u64 = 15;
/// TCP connect 超时；避免 DNS/路由不可达时阻塞 127s+。
const TCP_CONNECT_TIMEOUT_SECS: u64 = 15;

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
        let chunk_cap = Duration::from_secs(SOCKET_READ_TIMEOUT_SECS);

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
                Ok(Message::Close(_)) => return Ok(Some(WssEvent::Closed)),
                Ok(Message::Frame(_)) => {}
                Err(e) if is_timed_out_or_would_block(&e) => continue,
                Err(e @ tungstenite::Error::ConnectionClosed) => {
                    log::debug!("[wss_linux] read ended: {}", e);
                    return Ok(Some(WssEvent::Closed));
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

/// 建立 WSS 连接（`wss://`）；与 ESP 相同在握手前申请 orchestrator TLS 准入。
///
/// 使用 `TcpStream::connect_timeout` 限制 TCP 建连时间（避免默认 127s+ SYN 超时），
/// 并设置 read / write timeout 防止后续 `ws.send()` / `ws.read()` 无限阻塞。
pub fn connect_linux_wss(url: &str) -> Result<LinuxWssConnection> {
    let _permit = crate::orchestrator::request_http_permit(
        crate::orchestrator::Priority::Normal,
        Duration::from_secs(WSS_TLS_ADMISSION_TIMEOUT_SECS),
    )?;

    let tcp = tcp_connect_with_timeout(url)?;
    tcp.set_read_timeout(Some(Duration::from_secs(SOCKET_READ_TIMEOUT_SECS)))
        .map_err(|e| map_io("wss_linux_connect", e))?;
    tcp.set_write_timeout(Some(Duration::from_secs(SOCKET_WRITE_TIMEOUT_SECS)))
        .map_err(|e| map_io("wss_linux_connect", e))?;

    let (ws, _resp) = tungstenite::client_tls(url, tcp).map_err(|e| Error::Other {
        source: Box::new(std::io::Error::other(e.to_string())),
        stage: "wss_linux_connect",
    })?;

    Ok(LinuxWssConnection {
        ws,
        last_read_timeout: Some(Duration::from_secs(SOCKET_READ_TIMEOUT_SECS)),
    })
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

fn tcp_connect_with_timeout(url: &str) -> Result<TcpStream> {
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

    TcpStream::connect_timeout(&addr, Duration::from_secs(TCP_CONNECT_TIMEOUT_SECS))
        .map_err(|e| map_io("wss_linux_tcp_connect", e))
}
