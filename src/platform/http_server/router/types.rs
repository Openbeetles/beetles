//! 路由层请求/响应类型（无 esp-idf 类型）。
//! Request/response types for the router layer (no esp-idf types).

use crate::bus::UserInboundTx;
use crate::platform::ByteBuffer;

pub type IncomingBody = crate::platform::ByteBuffer;

/// Router resources shared by HTTP handlers.
/// 路由层共享资源；社交通道入站不再通过 HTTP callback 注入。
#[derive(Clone)]
pub struct RouterEnv {
    pub user_inbound_tx: UserInboundTx,
}

impl RouterEnv {
    pub fn new(user_inbound_tx: UserInboundTx) -> Self {
        Self { user_inbound_tx }
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
    pub body: IncomingBody,
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
    pub body: OutgoingBody,
    pub restart: RestartAction,
}

/// HTTP router response payload.
/// 普通字节响应与 SSE 流式响应互斥，避免输出层继续维护 `body + stream` 双态。
#[derive(Debug)]
pub enum OutgoingBody {
    Bytes(ByteBuffer),
    Stream(crate::chat_stream::ChatStreamReceiver),
}

impl OutgoingBody {
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes.as_ref()),
            Self::Stream(_) => None,
        }
    }

    #[cfg_attr(
        not(any(target_arch = "xtensa", target_arch = "riscv32")),
        allow(dead_code)
    )]
    pub fn bytes_len(&self) -> Option<usize> {
        self.as_bytes().map(<[u8]>::len)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartAction {
    None,
    After300Ms,
}

impl OutgoingResponse {
    pub fn json<B>(
        status: u16,
        status_text: &'static str,
        headers: &'static [(&'static str, &str)],
        body: B,
    ) -> Self
    where
        B: Into<ByteBuffer>,
    {
        Self {
            status,
            status_text,
            headers,
            body: OutgoingBody::Bytes(body.into()),
            restart: RestartAction::None,
        }
    }

    pub fn stream(
        status: u16,
        status_text: &'static str,
        headers: &'static [(&'static str, &str)],
        stream: crate::chat_stream::ChatStreamReceiver,
    ) -> Self {
        Self {
            status,
            status_text,
            headers,
            body: OutgoingBody::Stream(stream),
            restart: RestartAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::sync::Arc;

    #[test]
    fn json_response_body_uses_single_byte_buffer_variant() {
        let mut body = crate::platform::ByteBuffer::with_capacity(
            crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD,
        );
        let bytes = vec![b'x'; crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD + 1];
        body.write_all(&bytes).expect("write body");

        let response = OutgoingResponse::json(200, "OK", &[], body);

        match response.body {
            OutgoingBody::Bytes(bytes) => {
                assert!(bytes.is_external_preferred());
                assert_eq!(
                    bytes.len(),
                    crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD + 1
                );
            }
            OutgoingBody::Stream(_) => panic!("JSON response must not carry a stream body"),
        }
    }

    #[test]
    fn stream_response_body_uses_single_stream_variant() {
        let broker =
            Arc::new(crate::chat_stream::ChatStreamBroker::new_with_max_active_for_test(1));
        let opened = broker.try_open().expect("open stream");

        let response = OutgoingResponse::stream(200, "OK", &[], opened.receiver);

        assert!(matches!(response.body, OutgoingBody::Stream(_)));
    }
}
