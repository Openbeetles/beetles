//! WSS 传输抽象：发文本/二进制帧、带超时收事件。实现由 platform 或 cfg 模块提供。
//! Transport abstraction for WSS: send text/binary frames, receive events with timeout.

use crate::error::Result;
use std::time::Duration;

/// WSS 连接使用场景。当前只区分“网关长连”和“实时语音”，避免把超时、buffer 与节奏策略硬编码死。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WssConnectProfile {
    Gateway,
    Realtime,
}

/// Linux 侧单次发送上限；ESP 上由各连接实例按 profile 决定。
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) const MAX_WSS_SEND_PAYLOAD_BYTES: usize = 64 * 1024;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
type RawDrop = unsafe fn(*mut u8, usize);

enum WssBinaryStorage {
    Vec {
        data: Vec<u8>,
        recycler: Option<fn(Vec<u8>)>,
    },
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    Raw {
        ptr: *mut u8,
        len: usize,
        drop_fn: RawDrop,
    },
}

/// 单次收到的 WSS 二进制负载。可选回收器用于高频路径复用缓冲，降低分配抖动。
pub struct WssBinary {
    storage: WssBinaryStorage,
}

impl WssBinary {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self {
            storage: WssBinaryStorage::Vec {
                data,
                recycler: None,
            },
        }
    }

    pub fn from_vec_with_recycler(data: Vec<u8>, recycler: fn(Vec<u8>)) -> Self {
        Self {
            storage: WssBinaryStorage::Vec {
                data,
                recycler: Some(recycler),
            },
        }
    }

    /// Build a payload view over an externally allocated buffer.
    ///
    /// # Safety
    ///
    /// `ptr` must remain valid for `len` bytes until this value is dropped. `drop_fn`
    /// must free that exact allocation once.
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
    pub(crate) unsafe fn from_raw_parts_with_drop(
        ptr: *mut u8,
        len: usize,
        drop_fn: RawDrop,
    ) -> Self {
        Self {
            storage: WssBinaryStorage::Raw { ptr, len, drop_fn },
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.storage {
            WssBinaryStorage::Vec { data, .. } => data.as_slice(),
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
            WssBinaryStorage::Raw { ptr, len, .. } => {
                if ptr.is_null() || *len == 0 {
                    &[]
                } else {
                    unsafe { std::slice::from_raw_parts(*ptr, *len) }
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            WssBinaryStorage::Vec { data, .. } => data.len(),
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
            WssBinaryStorage::Raw { len, .. } => *len,
        }
    }
}

unsafe impl Send for WssBinary {}

impl std::fmt::Debug for WssBinary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WssBinary")
            .field("len", &self.len())
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
        match &mut self.storage {
            WssBinaryStorage::Vec { data, recycler } => {
                if let Some(recycler) = recycler.take() {
                    let mut data = std::mem::take(data);
                    data.clear();
                    recycler(data);
                }
            }
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
            WssBinaryStorage::Raw { ptr, len, drop_fn } => {
                if !ptr.is_null() {
                    unsafe {
                        drop_fn(*ptr, *len);
                    }
                    *ptr = std::ptr::null_mut();
                    *len = 0;
                }
            }
        }
    }
}

/// 单次收到的 WSS 事件。
#[derive(Debug)]
pub struct WssCloseInfo {
    pub code: Option<u16>,
    pub reason: Option<String>,
}

impl WssCloseInfo {
    pub fn summary(&self) -> String {
        match (self.code, self.reason.as_deref()) {
            (Some(code), Some(reason)) if !reason.trim().is_empty() => {
                format!("code={} reason={}", code, reason.trim())
            }
            (Some(code), _) => format!("code={}", code),
            (None, Some(reason)) if !reason.trim().is_empty() => {
                format!("reason={}", reason.trim())
            }
            _ => "peer closed".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum WssEvent {
    Binary(WssBinary),
    Disconnected,
    Closed(Option<WssCloseInfo>),
}

/// 带超时收一条事件：有数据返回 Some(ev)，超时返回 None；连接断开等错误返回 Err。
pub trait WssConnection: Send {
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

impl WssConnection for Box<dyn WssConnection> {
    fn send_binary(&mut self, data: &[u8]) -> Result<()> {
        (**self).send_binary(data)
    }

    fn send_text(&mut self, text: &str) -> Result<()> {
        (**self).send_text(text)
    }

    fn send_binary_owned(&mut self, data: Vec<u8>) -> Result<()> {
        (**self).send_binary_owned(data)
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<WssEvent>> {
        (**self).recv_timeout(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn boxed_wss_connection_can_cross_threads() {
        assert_send::<Box<dyn WssConnection>>();
    }

    #[test]
    fn raw_wss_binary_uses_drop_callback_without_vec_copy() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DROPPED: AtomicUsize = AtomicUsize::new(0);

        unsafe fn drop_raw(ptr: *mut u8, len: usize) {
            DROPPED.fetch_add(len, Ordering::SeqCst);
            let slice = std::ptr::slice_from_raw_parts_mut(ptr, len);
            let _ = unsafe { Box::from_raw(slice) };
        }

        let data: Box<[u8]> = Box::new([1u8, 2, 3, 4]);
        let len = data.len();
        let ptr = Box::into_raw(data) as *mut u8;

        let binary = unsafe { WssBinary::from_raw_parts_with_drop(ptr, len, drop_raw) };
        assert_eq!(binary.as_slice(), &[1, 2, 3, 4]);
        assert_eq!(binary.len(), 4);
        drop(binary);
        assert_eq!(DROPPED.load(Ordering::SeqCst), 4);
    }
}
