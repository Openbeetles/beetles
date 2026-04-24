//! 路由层请求/响应类型（无 esp-idf 类型）。
//! Request/response types for the router layer (no esp-idf types).

use crate::bus::InboundTx;

/// Router resources shared by HTTP handlers.
/// 路由层共享资源；社交通道入站不再通过 HTTP callback 注入。
#[derive(Clone)]
pub struct RouterEnv {
    pub inbound_tx: InboundTx,
}

impl RouterEnv {
    pub fn new(inbound_tx: InboundTx) -> Self {
        Self { inbound_tx }
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
