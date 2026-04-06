//! 未配置 LLM 时的占位实现；chat 返回固定提示文案。
//! No-op LLM when no source is configured; chat returns a fixed message.

use std::sync::Arc;

use crate::error::Result;
use crate::i18n::{tr, Locale, Message as UiMessage};
use crate::llm::{
    LlmClient, LlmHttpClient, LlmResponse, Message, StopReason, ToolChoicePolicy, ToolSpec,
};

pub struct NoopLlmClient {
    resolve_locale: Arc<dyn Fn() -> Locale + Send + Sync>,
}

impl NoopLlmClient {
    pub fn new(resolve_locale: Arc<dyn Fn() -> Locale + Send + Sync>) -> Self {
        Self { resolve_locale }
    }
}

impl LlmClient for NoopLlmClient {
    fn chat(
        &self,
        _http: &mut dyn LlmHttpClient,
        _system: &str,
        _messages: &[Message],
        _tools: Option<&[ToolSpec]>,
        _tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let loc = (self.resolve_locale)();
        Ok(LlmResponse {
            content: tr(UiMessage::LlmNotConfigured, loc),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        })
    }
}
