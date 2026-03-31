//! 多 LLM 源顺序回退：chat 时依次尝试各 client，首次成功即返回，全部失败返回最后一 Err。
//! Fallback LLM client: try each client in order, return first Ok or last Err.

use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient, LlmResponse, Message, ToolChoicePolicy, ToolSpec};
use std::sync::Mutex;

/// 多源回退客户端；持有一组 LlmClient，chat 时按序尝试。
/// last_error 用 Mutex 以支持多线程安全访问。
pub struct FallbackLlmClient {
    clients: Vec<Box<dyn LlmClient + Send + Sync>>,
    last_error: Mutex<Option<String>>,
}

impl FallbackLlmClient {
    /// 使用给定的 client 列表构造；空列表会导致 chat 时返回错误。
    pub fn new(clients: Vec<Box<dyn LlmClient + Send + Sync>>) -> Self {
        Self {
            clients,
            last_error: Mutex::new(None),
        }
    }

    /// 源数量。
    pub fn len(&self) -> usize {
        self.clients.len()
    }

    /// 是否没有配置任何源。
    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// 最近一次失败错误。
    pub fn last_error(&self) -> Option<String> {
        self.last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_last_error(&self, err: &str) {
        if let Ok(mut g) = self.last_error.lock() {
            *g = Some(err.to_string());
        }
    }
}

impl LlmClient for FallbackLlmClient {
    fn supports_native_tools(&self) -> bool {
        self.clients
            .iter()
            .all(|client| client.supports_native_tools())
    }

    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        if self.clients.is_empty() {
            let err = crate::error::Error::config("fallback_llm", "no LLM sources configured");
            self.set_last_error(&err.to_string());
            return Err(err);
        }
        let mut last_err = None;
        for (i, client) in self.clients.iter().enumerate() {
            match client.chat(http, system, messages, tools, tool_choice) {
                Ok(r) => {
                    crate::platform::task_wdt::feed_current_task();
                    return Ok(r);
                }
                Err(e) => {
                    crate::platform::task_wdt::feed_current_task();
                    if i + 1 < self.clients.len() {
                        log::warn!("[fallback_llm] source {} failed, trying next: {}", i, e);
                    }
                    last_err = Some(e);
                }
            }
        }
        let err = last_err.unwrap_or_else(|| {
            crate::error::Error::config("fallback_llm", "llm fallback returned no result")
        });
        self.set_last_error(&err.to_string());
        Err(err)
    }

    fn chat_with_progress(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        tool_choice: ToolChoicePolicy,
        on_progress: crate::llm::StreamProgressFn,
    ) -> Result<LlmResponse> {
        if self.clients.is_empty() {
            let err = crate::error::Error::config("fallback_llm", "no LLM sources configured");
            self.set_last_error(&err.to_string());
            return Err(err);
        }
        // 第一个源使用 progress 回调。
        let first_result = self.clients[0].chat_with_progress(
            http,
            system,
            messages,
            tools,
            tool_choice,
            on_progress,
        );
        match first_result {
            Ok(r) => {
                crate::platform::task_wdt::feed_current_task();
                return Ok(r);
            }
            Err(e) => {
                crate::platform::task_wdt::feed_current_task();
                if self.clients.len() > 1 {
                    log::warn!("[fallback_llm] source 0 failed, trying next: {}", e);
                } else {
                    self.set_last_error(&e.to_string());
                    return Err(e);
                }
            }
        }
        // 后续源降级为普通 chat。
        let mut last_err = None;
        for (i, client) in self.clients.iter().enumerate().skip(1) {
            match client.chat(http, system, messages, tools, tool_choice) {
                Ok(r) => {
                    crate::platform::task_wdt::feed_current_task();
                    return Ok(r);
                }
                Err(e) => {
                    crate::platform::task_wdt::feed_current_task();
                    if i + 1 < self.clients.len() {
                        log::warn!("[fallback_llm] source {} failed, trying next: {}", i, e);
                    }
                    last_err = Some(e);
                }
            }
        }
        let err = last_err.unwrap_or_else(|| {
            crate::error::Error::config("fallback_llm", "llm fallback returned no result")
        });
        self.set_last_error(&err.to_string());
        Err(err)
    }
}
