//! HTTP 适配层：两方向桥接 PlatformHttpClient 与 ToolContext。
//!
//! - `HttpClientToolContext`（正向）：持有 `&mut dyn PlatformHttpClient` + 会话元数据，
//!   同时实现 `PlatformHttpClient`（委托内部 `http` 字段）和 `ToolContext`。
//!   因其实现了 `PlatformHttpClient`，`lib.rs` 的 blanket `impl<T: PlatformHttpClient> LlmHttpClient for T`
//!   自动覆盖，使同一实例可服务 LLM 请求、工具执行，无需维护独立连接。
//!
//! - `ToolContextHttpClient`（逆向）：持有 `&mut dyn ToolContext`，实现 `PlatformHttpClient`，
//!   供 STT/TTS 等只接受 `PlatformHttpClient` 的音频模块使用。

use crate::error::Result;
use crate::i18n::Locale;
use crate::platform::{PlatformHttpClient, ResponseBody};
use crate::tools::ToolContext;
use std::sync::Arc;

/// 正向适配器：`PlatformHttpClient + 会话元数据` → `PlatformHttpClient + ToolContext`。
///
/// 持有 agent 线程内独占的 HTTP 客户端引用与当前入站消息的会话元数据。
/// 实现 `PlatformHttpClient`（委托 `http`），因此自动获得 blanket 的 `LlmHttpClient` 实现，
/// 无需在调用点分别维护独立连接。
pub(crate) struct HttpClientToolContext<'a> {
    /// 底层 HTTP 连接，agent 线程独占；LLM 与工具共享同一物理连接。
    pub(crate) http: &'a mut dyn PlatformHttpClient,
    /// 当前入站消息的 chat_id，始终由调用方提供：
    /// 正常对话路径为 `Some(Arc::from(chat_id))`，摘要生成路径为 `Some(Arc::from(chat_id))`，
    /// 系统内部路径为 `Some(Arc::from("system"))`。
    pub(crate) chat_id: Option<Arc<str>>,
    /// 当前入站消息的通道名称（如 `"telegram"`）；系统内部路径为 `None`。
    pub(crate) channel: Option<Arc<str>>,
    /// 当前用户界面语言；来自设备 NVS，不硬编码。
    pub(crate) locale: Locale,
}

impl PlatformHttpClient for HttpClientToolContext<'_> {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.http.get(url, headers)
    }

    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.http.post(url, headers, body)
    }

    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        self.http
            .post_streaming(url, headers, body, max_response_bytes, on_chunk)
    }

    fn reset_connection_for_retry(&mut self) {
        self.http.reset_connection_for_retry();
    }
}

impl ToolContext for HttpClientToolContext<'_> {
    fn get_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        self.http.get(url, headers)
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.http.post(url, headers, body)
    }

    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        self.http
            .post_streaming(url, headers, body, max_response_bytes, on_chunk)
    }

    fn patch_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.http.patch(url, headers, body)
    }

    fn put_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.http.put(url, headers, body)
    }

    fn delete_with_headers(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<(u16, ResponseBody)> {
        self.http.delete(url, headers)
    }

    fn current_chat_id(&self) -> Option<&str> {
        self.chat_id.as_deref()
    }

    fn current_channel(&self) -> Option<&str> {
        self.channel.as_deref()
    }

    fn user_locale(&self) -> Locale {
        self.locale
    }
}

/// 逆向适配器：`ToolContext` → `PlatformHttpClient`。
///
/// 供 STT/TTS 等只接受 `PlatformHttpClient` 接口的音频模块使用；
/// 工具 execute 收到 `&mut dyn ToolContext` 后，用 `new(ctx)` 包装为 `PlatformHttpClient`。
pub(crate) struct ToolContextHttpClient<'a> {
    ctx: &'a mut dyn ToolContext,
}

impl<'a> ToolContextHttpClient<'a> {
    pub(crate) fn new(ctx: &'a mut dyn ToolContext) -> Self {
        Self { ctx }
    }
}

impl PlatformHttpClient for ToolContextHttpClient<'_> {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.ctx.get_with_headers(url, headers)
    }

    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.post_with_headers(url, headers, body)
    }

    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        self.ctx
            .post_streaming(url, headers, body, max_response_bytes, on_chunk)
    }

    fn patch(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.patch_with_headers(url, headers, body)
    }

    fn put(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.put_with_headers(url, headers, body)
    }

    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.ctx.delete_with_headers(url, headers)
    }
}
