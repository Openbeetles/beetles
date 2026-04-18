//! 路由层请求/响应类型（无 esp-idf 类型）。
//! Request/response types for the router layer (no esp-idf types).

use crate::bus::InboundTx;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use crate::channels::{QqInboundDedupStore, QqMsgIdCache};

/// 与 webhook、QQ 回调相关的跨 handler 资源。
/// Cross-handler resources for webhooks and QQ callbacks.
#[derive(Clone)]
pub struct RouterEnv {
    pub inbound_tx: InboundTx,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub qq_msg_id_cache: QqMsgIdCache,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub qq_inbound_dedup_store: QqInboundDedupStore,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub qq_webhook_enabled: bool,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub qq_app_id: String,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub qq_secret: String,
}

impl RouterEnv {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    pub fn new(inbound_tx: InboundTx) -> Self {
        Self { inbound_tx }
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    pub fn new(
        inbound_tx: InboundTx,
        qq_msg_id_cache: QqMsgIdCache,
        qq_inbound_dedup_store: QqInboundDedupStore,
        qq_webhook_enabled: bool,
        qq_app_id: String,
        qq_secret: String,
    ) -> Self {
        Self {
            inbound_tx,
            qq_msg_id_cache,
            qq_inbound_dedup_store,
            qq_webhook_enabled,
            qq_app_id,
            qq_secret,
        }
    }
}

/// 已进入路由层的 HTTP 请求（body 已按上限读完）。
/// HTTP request after body has been read (bounded).
#[derive(Debug)]
pub struct IncomingRequest {
    pub method: String,
    /// 完整 URI（含 query），与 ESP `uri()` 一致；路由用 `path_only(uri)` 解析路径。
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl IncomingRequest {
    pub fn header_ci(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// 路由输出；ESP/Linux 适配层负责写入传输。
/// Router output; transport adapters write to wire.
#[derive(Debug)]
pub struct OutgoingResponse {
    pub status: u16,
    #[allow(dead_code)]
    pub status_text: &'static str,
    /// 与 `common::CORS_HEADERS` 等一致；空则使用默认 CORS JSON
    pub headers: &'static [(&'static str, &'static str)],
    pub body: Vec<u8>,
    pub restart: RestartAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartAction {
    None,
    After300Ms,
}

impl OutgoingResponse {
    pub fn json(
        status: u16,
        status_text: &'static str,
        headers: &'static [(&'static str, &str)],
        body: Vec<u8>,
    ) -> Self {
        Self {
            status,
            status_text,
            headers,
            body,
            restart: RestartAction::None,
        }
    }
}
