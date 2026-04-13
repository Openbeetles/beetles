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
use crate::tools::{ToolContext, ToolPolicyContext};
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
    /// 当前入站消息的 ingress。
    pub(crate) ingress: crate::bus::IngressKind,
    /// 当前入站消息的通道名称（如 `"telegram"`）；系统内部路径为 `None`。
    pub(crate) channel: Option<Arc<str>>,
    /// 当前注册表；供 capability-scoped tool bridge 做 catalog/assessment。
    pub(crate) tool_registry: Option<&'a crate::tools::ToolRegistry>,
    /// 当前运行时的通道能力合同表。
    pub(crate) channel_capability_registry: Arc<crate::ChannelCapabilityRegistry>,
    /// 当前运行时是否允许工具向当前聊天提交用户可见消息意图。
    pub(crate) supports_current_chat_outbound_message: bool,
    /// 当前运行时是否允许工具声明“当前聊天主答复已由工具交付”。
    pub(crate) supports_current_chat_primary_reply: bool,
    /// 当前运行时是否允许工具向显式指定的其他聊天发消息。
    pub(crate) supports_explicit_outbound_message: bool,
    /// 单轮工具外发消息总额度；用于抑制模型刷屏。
    pub(crate) outbound_message_budget: u8,
    /// 当前轮已占用的工具外发消息额度。
    pub(crate) outbound_message_count: u8,
    /// 当前轮是否已经声明过一次 current+primary 主答复。
    pub(crate) current_primary_message_delivered: bool,
    /// 当前用户界面语言；来自设备 NVS，不硬编码。
    pub(crate) locale: Locale,
}

impl PlatformHttpClient for HttpClientToolContext<'_> {
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        self.http.request(method, url, headers, body)
    }

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
    fn request_with_headers(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        self.http.request(method, url, headers, body)
    }

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

    fn current_ingress(&self) -> Option<crate::bus::IngressKind> {
        Some(self.ingress)
    }

    fn channel_capability(
        &self,
        channel: &str,
    ) -> Option<crate::channel_capability::ChannelCapabilityEntry> {
        self.channel_capability_registry.get(channel)
    }

    fn supports_current_chat_outbound_message(&self) -> bool {
        self.supports_current_chat_outbound_message
    }

    fn supports_current_chat_primary_reply(&self) -> bool {
        self.supports_current_chat_primary_reply
    }

    fn supports_explicit_outbound_message(&self) -> bool {
        self.supports_explicit_outbound_message
    }

    fn claim_outbound_message_delivery(
        &mut self,
        target_is_current: bool,
        primary: bool,
    ) -> Result<()> {
        if !target_is_current && !self.supports_explicit_outbound_message {
            return Err(crate::error::Error::config(
                "tool_message",
                "explicit outbound target is not allowed in this runtime context",
            ));
        }
        if primary && target_is_current {
            if self.current_primary_message_delivered {
                return Err(crate::error::Error::config(
                    "tool_message",
                    "current-chat primary reply has already been claimed in this turn",
                ));
            }
            self.current_primary_message_delivered = true;
        }
        if self.outbound_message_count >= self.outbound_message_budget {
            return Err(crate::error::Error::config(
                "tool_message",
                "tool outbound message budget exhausted for this turn",
            ));
        }
        self.outbound_message_count = self.outbound_message_count.saturating_add(1);
        Ok(())
    }

    fn tool_bridge_catalog(&self) -> Result<Vec<crate::tools::ToolBridgeCatalogEntry>> {
        let registry = self.tool_registry.ok_or_else(|| {
            crate::error::Error::config(
                "tool_bridge_catalog",
                "tool registry unavailable in this runtime context",
            )
        })?;
        let channel = self.channel.as_deref().unwrap_or("system");
        Ok(registry.tool_bridge_catalog_for_policy(&ToolPolicyContext::new(self.ingress, channel)))
    }

    fn assess_tool_request_proposal(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<crate::tools::ToolBridgeProposalAssessment> {
        let registry = self.tool_registry.ok_or_else(|| {
            crate::error::Error::config(
                "tool_bridge_assess",
                "tool registry unavailable in this runtime context",
            )
        })?;
        let channel = self.channel.as_deref().unwrap_or("system");
        Ok(registry.assess_tool_request_proposal(
            tool_name,
            args,
            &ToolPolicyContext::new(self.ingress, channel),
        ))
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
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        self.ctx.request_with_headers(method, url, headers, body)
    }

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
