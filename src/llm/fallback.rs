//! 多 LLM 源顺序回退：chat 时依次尝试各 client，首次成功即返回，全部失败返回最后一 Err。
//! Fallback LLM client: try each client in order, return first Ok or last Err.

use crate::error::Result;
use crate::llm::{
    tool_fallback::{append_tool_fallback_instructions, recover_text_tool_calls},
    LlmClient, LlmHttpClient, LlmModelCompat, LlmResponse, Message, ToolCallSupport,
    ToolChoicePolicy, ToolSpec,
};
use std::borrow::Cow;
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
    fn model_compat(&self) -> LlmModelCompat {
        self.clients
            .first()
            .map(|client| client.model_compat())
            .unwrap_or_default()
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
            let (effective_system, effective_tools) =
                prepare_request_for_client(client.model_compat(), system, tools);
            match client.chat(
                http,
                effective_system.as_ref(),
                messages,
                effective_tools,
                tool_choice,
            ) {
                Ok(r) => {
                    crate::platform::task_wdt::feed_current_task();
                    return Ok(finalize_response_for_client(
                        client.model_compat(),
                        tools,
                        r,
                    ));
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
        let first_compat = self.clients[0].model_compat();
        let (first_system, first_tools) = prepare_request_for_client(first_compat, system, tools);
        let first_result = self.clients[0].chat_with_progress(
            http,
            first_system.as_ref(),
            messages,
            first_tools,
            tool_choice,
            on_progress,
        );
        match first_result {
            Ok(r) => {
                crate::platform::task_wdt::feed_current_task();
                return Ok(finalize_response_for_client(first_compat, tools, r));
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
            let compat = client.model_compat();
            let (effective_system, effective_tools) =
                prepare_request_for_client(compat, system, tools);
            match client.chat(
                http,
                effective_system.as_ref(),
                messages,
                effective_tools,
                tool_choice,
            ) {
                Ok(r) => {
                    crate::platform::task_wdt::feed_current_task();
                    return Ok(finalize_response_for_client(compat, tools, r));
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

fn prepare_request_for_client<'a>(
    compat: LlmModelCompat,
    system: &'a str,
    tools: Option<&'a [ToolSpec]>,
) -> (Cow<'a, str>, Option<&'a [ToolSpec]>) {
    match (tools, compat.tool_call_support) {
        (Some(tool_specs), ToolCallSupport::PromptGuided) if !tool_specs.is_empty() => {
            let mut prompt = system.to_string();
            append_tool_fallback_instructions(&mut prompt, usize::MAX, tool_specs);
            (Cow::Owned(prompt), None)
        }
        _ => (Cow::Borrowed(system), tools),
    }
}

fn finalize_response_for_client(
    compat: LlmModelCompat,
    tools: Option<&[ToolSpec]>,
    response: LlmResponse,
) -> LlmResponse {
    if tools.is_some() && matches!(compat.tool_call_support, ToolCallSupport::PromptGuided) {
        recover_text_tool_calls(response)
    } else {
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::StopReason;
    use crate::Result;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    struct ObservedRequest {
        system: String,
        tool_count: usize,
    }

    struct StubClient {
        compat: LlmModelCompat,
        outcome: StubOutcome,
        observed: Arc<Mutex<Vec<ObservedRequest>>>,
    }

    enum StubOutcome {
        Success(LlmResponse),
        Fail(&'static str),
    }

    struct DummyHttpClient;

    impl StubClient {
        fn new(
            compat: LlmModelCompat,
            outcome: StubOutcome,
            observed: Arc<Mutex<Vec<ObservedRequest>>>,
        ) -> Self {
            Self {
                compat,
                outcome,
                observed,
            }
        }

        fn record_request(&self, system: &str, tools: Option<&[ToolSpec]>) {
            self.observed
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(ObservedRequest {
                    system: system.to_string(),
                    tool_count: tools.map_or(0, |specs| specs.len()),
                });
        }

        fn produce_response(&self) -> Result<LlmResponse> {
            match &self.outcome {
                StubOutcome::Success(response) => Ok(response.clone()),
                StubOutcome::Fail(message) => {
                    Err(crate::error::Error::config("stub_llm", *message))
                }
            }
        }
    }

    impl crate::llm::LlmClient for StubClient {
        fn model_compat(&self) -> LlmModelCompat {
            self.compat
        }

        fn chat(
            &self,
            _http: &mut dyn crate::llm::LlmHttpClient,
            system: &str,
            _messages: &[Message],
            tools: Option<&[ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            self.record_request(system, tools);
            self.produce_response()
        }

        fn chat_with_progress(
            &self,
            _http: &mut dyn crate::llm::LlmHttpClient,
            system: &str,
            _messages: &[Message],
            tools: Option<&[ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
            on_progress: crate::llm::StreamProgressFn,
        ) -> Result<LlmResponse> {
            self.record_request(system, tools);
            let response = self.produce_response()?;
            if !response.content.is_empty() {
                on_progress(&response.content, &response.content);
            }
            Ok(response)
        }
    }

    impl crate::llm::LlmHttpClient for DummyHttpClient {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            unreachable!("stub fallback tests never hit transport")
        }
    }

    #[test]
    fn fallback_compat_uses_primary_source_mode() {
        let compat = FallbackLlmClient::new(vec![
            Box::new(StubClient::new(
                LlmModelCompat::native(),
                StubOutcome::Fail("primary"),
                Arc::new(Mutex::new(Vec::new())),
            )),
            Box::new(StubClient::new(
                LlmModelCompat::prompt_guided(),
                StubOutcome::Fail("secondary"),
                Arc::new(Mutex::new(Vec::new())),
            )),
        ])
        .model_compat();
        assert_eq!(compat, LlmModelCompat::native());
    }

    #[test]
    fn empty_fallback_defaults_to_native_compat() {
        let compat = FallbackLlmClient::new(vec![]).model_compat();
        assert_eq!(compat, LlmModelCompat::default());
    }

    #[test]
    fn prompt_guided_prepare_drops_native_tools_and_appends_protocol() {
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let (system, request_tools) =
            prepare_request_for_client(LlmModelCompat::prompt_guided(), "base", Some(&tools));
        assert!(system.contains("Tool Use Protocol"));
        assert!(request_tools.is_none());
    }

    #[test]
    fn prompt_guided_finalize_recovers_text_tool_calls() {
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let response = finalize_response_for_client(
            LlmModelCompat::prompt_guided(),
            Some(&tools),
            LlmResponse {
                content: "<tool_call>{\"name\":\"get_time\",\"arguments\":{}}</tool_call>"
                    .to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            },
        );
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.tool_calls.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn primary_native_source_keeps_native_tool_request_shape() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let client = FallbackLlmClient::new(vec![Box::new(StubClient::new(
            LlmModelCompat::native(),
            StubOutcome::Success(LlmResponse {
                content: "done".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }),
            Arc::clone(&observed),
        ))]);
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];

        let response = client
            .chat(
                &mut DummyHttpClient,
                "base system",
                &[],
                Some(&tools),
                ToolChoicePolicy::Auto,
            )
            .expect("fallback chat");

        assert_eq!(response.stop_reason, StopReason::EndTurn);
        let calls = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_count, 1);
        assert!(!calls[0].system.contains("Tool Use Protocol"));
    }

    #[test]
    fn prompt_guided_fallback_adapts_request_and_recovers_response() {
        let primary_observed = Arc::new(Mutex::new(Vec::new()));
        let secondary_observed = Arc::new(Mutex::new(Vec::new()));
        let client = FallbackLlmClient::new(vec![
            Box::new(StubClient::new(
                LlmModelCompat::native(),
                StubOutcome::Fail("primary failed"),
                Arc::clone(&primary_observed),
            )),
            Box::new(StubClient::new(
                LlmModelCompat::prompt_guided(),
                StubOutcome::Success(LlmResponse {
                    content:
                        "先查一下。<tool_call>{\"name\":\"get_time\",\"arguments\":{}}</tool_call>"
                            .to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                }),
                Arc::clone(&secondary_observed),
            )),
        ]);
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];

        let response = client
            .chat(
                &mut DummyHttpClient,
                "base system",
                &[],
                Some(&tools),
                ToolChoicePolicy::Auto,
            )
            .expect("fallback chat");

        let primary_calls = primary_observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(primary_calls.len(), 1);
        assert_eq!(primary_calls[0].tool_count, 1);
        assert!(!primary_calls[0].system.contains("Tool Use Protocol"));

        let secondary_calls = secondary_observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(secondary_calls.len(), 1);
        assert_eq!(secondary_calls[0].tool_count, 0);
        assert!(secondary_calls[0].system.contains("Tool Use Protocol"));

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.tool_calls.as_ref().map(Vec::len), Some(1));
        assert!(response.content.contains("先查一下。"));
    }

    #[test]
    fn progress_path_keeps_primary_native_request_shape() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let client = FallbackLlmClient::new(vec![Box::new(StubClient::new(
            LlmModelCompat::native(),
            StubOutcome::Success(LlmResponse {
                content: "stream ok".to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            }),
            Arc::clone(&observed),
        ))]);
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let mut progress_updates = Vec::new();

        let response = client
            .chat_with_progress(
                &mut DummyHttpClient,
                "base system",
                &[],
                Some(&tools),
                ToolChoicePolicy::Auto,
                &mut |delta, accumulated| {
                    progress_updates.push((delta.to_string(), accumulated.to_string()));
                },
            )
            .expect("fallback progress chat");

        assert_eq!(response.stop_reason, StopReason::EndTurn);
        assert_eq!(progress_updates.len(), 1);
        assert_eq!(progress_updates[0].0, "stream ok");
        let calls = observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_count, 1);
        assert!(!calls[0].system.contains("Tool Use Protocol"));
    }

    #[test]
    fn progress_path_adapts_prompt_guided_secondary_fallback() {
        let primary_observed = Arc::new(Mutex::new(Vec::new()));
        let secondary_observed = Arc::new(Mutex::new(Vec::new()));
        let client = FallbackLlmClient::new(vec![
            Box::new(StubClient::new(
                LlmModelCompat::native(),
                StubOutcome::Fail("primary failed"),
                Arc::clone(&primary_observed),
            )),
            Box::new(StubClient::new(
                LlmModelCompat::prompt_guided(),
                StubOutcome::Success(LlmResponse {
                    content: "<tool_call>{\"name\":\"get_time\",\"arguments\":{}}</tool_call>"
                        .to_string(),
                    stop_reason: StopReason::EndTurn,
                    tool_calls: None,
                }),
                Arc::clone(&secondary_observed),
            )),
        ]);
        let tools = [ToolSpec {
            name: "get_time".to_string(),
            description: "time".to_string(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let mut progress_updates = Vec::new();

        let response = client
            .chat_with_progress(
                &mut DummyHttpClient,
                "base system",
                &[],
                Some(&tools),
                ToolChoicePolicy::Auto,
                &mut |delta, accumulated| {
                    progress_updates.push((delta.to_string(), accumulated.to_string()));
                },
            )
            .expect("fallback progress chat");

        assert!(progress_updates.is_empty());

        let primary_calls = primary_observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(primary_calls.len(), 1);
        assert_eq!(primary_calls[0].tool_count, 1);
        assert!(!primary_calls[0].system.contains("Tool Use Protocol"));

        let secondary_calls = secondary_observed.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(secondary_calls.len(), 1);
        assert_eq!(secondary_calls[0].tool_count, 0);
        assert!(secondary_calls[0].system.contains("Tool Use Protocol"));

        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.tool_calls.as_ref().map(Vec::len), Some(1));
    }
}
