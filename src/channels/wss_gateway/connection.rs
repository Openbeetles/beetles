//! WSS 传输抽象：发文本/二进制帧、带超时收事件。实现由 platform 或 cfg 模块提供。
//! Transport abstraction for WSS: send text/binary frames, receive events with timeout.

use crate::error::Result;
use std::time::Duration;

/// 与 ESP `esp_websocket_client` 默认 buffer 对齐（仅嵌入式构建使用）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const DEFAULT_WSS_BUFFER_SIZE: usize = 4096;
/// 应用层 `send_binary` 最大字节数（ESP 与客户端 buffer 对齐；主机/Linux 需容纳 QQ Identify 等较长 JSON）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) const MAX_WSS_SEND_PAYLOAD_BYTES: usize = DEFAULT_WSS_BUFFER_SIZE - 32;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) const MAX_WSS_SEND_PAYLOAD_BYTES: usize = 64 * 1024;

/// 单次收到的 WSS 二进制负载。可选回收器用于高频路径复用缓冲，降低分配抖动。
pub struct WssBinary {
    data: Vec<u8>,
    recycler: Option<fn(Vec<u8>)>,
}

impl WssBinary {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self {
            data,
            recycler: None,
        }
    }

    pub fn from_vec_with_recycler(data: Vec<u8>, recycler: fn(Vec<u8>)) -> Self {
        Self {
            data,
            recycler: Some(recycler),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.data.as_slice()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}

impl std::fmt::Debug for WssBinary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WssBinary")
            .field("len", &self.data.len())
            .finish()
    }
}

impl AsRef<[u8]> for WssBinary {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for WssBinary {
    fn drop(&mut self) {
        if let Some(recycler) = self.recycler.take() {
            let mut data = std::mem::take(&mut self.data);
            data.clear();
            recycler(data);
        }
    }
}

/// 单次收到的 WSS 事件。
#[derive(Debug)]
pub enum WssEvent {
    Binary(WssBinary),
    Disconnected,
    Closed,
}

/// 带超时收一条事件：有数据返回 Some(ev)，超时返回 None；连接断开等错误返回 Err。
pub trait WssConnection {
    fn send_binary(&mut self, data: &[u8]) -> Result<()>;
    /// 发送 UTF-8 文本帧；默认退化为按字节发送。
    fn send_text(&mut self, text: &str) -> Result<()> {
        self.send_binary(text.as_bytes())
    }
    /// 发送已拥有所有权的二进制负载；默认实现转调 `send_binary`。
    /// Send owned binary payload; default implementation forwards to `send_binary`.
    fn send_binary_owned(&mut self, data: Vec<u8>) -> Result<()> {
        self.send_binary(&data)
    }
    /// 阻塞最多 timeout，返回收到的事件或 None 表示超时。
    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WssEvent>>;
}
