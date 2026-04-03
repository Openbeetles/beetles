//! Anthropic Messages API 客户端；依赖注入 HTTP，不依赖 platform。
//! 错误均带 stage（llm_request / llm_parse）；后续健康检查可统计最近一次失败或计数。
//! Anthropic client; HTTP injected, no platform dependency.
//! Supports both non-streaming (default) and SSE streaming modes.

use crate::config::{AppConfig, LlmSource};
use crate::error::{Error, Result};
use crate::llm::types::MAX_REQUEST_BODY_LEN;
use crate::llm::types::{AnthropicResponse, StopReason, ToolCall};
use crate::llm::{LlmClient, LlmHttpClient, LlmResponse, Message, ToolChoicePolicy, ToolSpec};
use serde::Deserialize;
use serde_json;

const TAG: &str = "llm::anthropic";
const API_BASE: &str = "https://api.anthropic.com/v1/messages";
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// Anthropic Messages API 客户端；持 config 只读，HTTP 由 chat 时注入。
/// `api_base` 为完整 Messages URL（构造时解析），请求路径零额外分配。
pub struct AnthropicClient {
    model: String,
    api_key: String,
    max_tokens: u32,
    api_base: String,
    stream: bool,
}

impl AnthropicClient {
    pub fn new(config: &AppConfig) -> Self {
        Self::from_source(
            &LlmSource {
                provider: config.model_provider.clone(),
                api_key: config.api_key.clone(),
                model: config.model.clone(),
                api_url: config.api_url.clone(),
                max_tokens: None,
            },
            false,
        )
    }

    /// 从单源配置构造，供多源回退使用。api_url 非空时替代默认 API_BASE。
    pub fn from_source(source: &LlmSource, stream: bool) -> Self {
        let api_base = if source.api_url.trim().is_empty() {
            API_BASE.to_string()
        } else {
            source.api_url.trim_end_matches('/').to_string()
        };
        Self {
            model: source.model.clone(),
            api_key: source.api_key.clone(),
            max_tokens: source.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            api_base,
            stream,
        }
    }
}

impl LlmClient for AnthropicClient {
    fn chat(
        &self,
        http: &mut dyn LlmHttpClient,
        system: &str,
        messages: &[Message],
        tools: Option<&[ToolSpec]>,
        tool_choice: ToolChoicePolicy,
    ) -> Result<LlmResponse> {
        let body = build_request_body(
            &self.model,
            self.max_tokens,
            system,
            messages,
            tools,
            tool_choice,
            self.stream,
        )?;

        if self.stream {
            crate::llm::retry::with_retry(2, 500, TAG, http, |http| {
                do_request_streaming(http, &self.api_base, &self.api_key, &body, None)
            })
        } else {
            crate::llm::retry::with_retry(2, 500, TAG, http, |http| {
                do_request(http, &self.api_base, &self.api_key, &body)
            })
        }
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
        if !self.stream {
            return self.chat(http, system, messages, tools, tool_choice);
        }
        let body = build_request_body(
            &self.model,
            self.max_tokens,
            system,
            messages,
            tools,
            tool_choice,
            true,
        )?;
        // 与 chat() 保持同一重试策略；仅首轮传递 progress 回调，后续重试避免重复回放增量。
        let mut progress = Some(on_progress);
        crate::llm::retry::with_retry(2, 500, TAG, http, |http| {
            do_request_streaming(http, &self.api_base, &self.api_key, &body, progress.take())
        })
    }
}

fn build_request_body(
    model: &str,
    max_tokens: u32,
    system: &str,
    messages: &[Message],
    tools: Option<&[ToolSpec]>,
    tool_choice: ToolChoicePolicy,
    stream: bool,
) -> Result<Vec<u8>> {
    fn push_json_string_field(out: &mut String, key: &str, value: &str) {
        out.push('"');
        out.push_str(key);
        out.push_str("\":");
        crate::util::push_json_string_escaped(out, value);
    }

    fn push_messages_field(out: &mut String, messages: &[Message]) {
        out.push_str("\"messages\":[");
        for (idx, message) in messages.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push('{');
            push_json_string_field(out, "role", &message.role);
            out.push(',');
            push_json_string_field(out, "content", &message.content);
            out.push('}');
        }
        out.push(']');
    }

    fn push_tools_field(out: &mut String, tools: &[ToolSpec]) {
        out.push_str(",\"tools\":[");
        for (idx, tool) in tools.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push('{');
            push_json_string_field(out, "name", &tool.name);
            out.push(',');
            push_json_string_field(out, "description", &tool.description);
            out.push_str(",\"input_schema\":");
            out.push_str(tool.parameters_json());
            out.push('}');
        }
        out.push(']');
    }

    let mut num_buf = [0u8; 20];
    let max_tokens_str = crate::util::usize_to_decimal_buf(&mut num_buf, max_tokens as usize);
    let mut body = String::with_capacity(
        model.len()
            + system.len()
            + messages
                .iter()
                .map(|m| m.role.len() + m.content.len() + 32)
                .sum::<usize>()
            + tools
                .map(|items| {
                    items
                        .iter()
                        .map(|tool| {
                            tool.name.len()
                                + tool.description.len()
                                + tool.parameters_json().len()
                                + 48
                        })
                        .sum::<usize>()
                })
                .unwrap_or(0)
            + 128,
    );
    body.push('{');
    push_json_string_field(&mut body, "model", model);
    body.push_str(",\"max_tokens\":");
    body.push_str(max_tokens_str);
    if !system.is_empty() {
        body.push(',');
        push_json_string_field(&mut body, "system", system);
    }
    body.push(',');
    push_messages_field(&mut body, messages);
    let has_tools = tools.is_some_and(|items| !items.is_empty());
    if let Some(items) = tools.filter(|items| !items.is_empty()) {
        push_tools_field(&mut body, items);
    }
    if has_tools && tool_choice == ToolChoicePolicy::Require {
        body.push_str(",\"tool_choice\":{\"type\":\"any\"}");
    }
    if stream {
        body.push_str(",\"stream\":true");
    }
    body.push('}');
    if body.len() > MAX_REQUEST_BODY_LEN {
        return Err(Error::config(
            "llm_request",
            format!("request body exceeds {} bytes", MAX_REQUEST_BODY_LEN),
        ));
    }
    Ok(body.into_bytes())
}

fn do_request(
    http: &mut dyn LlmHttpClient,
    url: &str,
    api_key: &str,
    body: &[u8],
) -> Result<LlmResponse> {
    const ANTHROPIC_VERSION: &str = "2023-06-01";
    let mut cl_buf = [0u8; 20];
    let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
    let headers = [
        ("x-api-key", api_key),
        ("anthropic-version", ANTHROPIC_VERSION),
        ("content-type", "application/json"),
        ("content-length", content_length),
    ];
    let (status, resp_body) = http.do_post(url, &headers, body).map_err(|e| match e {
        Error::Http { status_code, .. } => Error::Http {
            status_code,
            stage: "llm_request",
        },
        _ => Error::Other {
            source: Box::new(std::io::Error::other(format!("{:?}", e))),
            stage: "llm_request",
        },
    })?;

    if status == 429 {
        log::warn!("[{}] rate limited (429)", TAG);
        return Err(Error::Http {
            status_code: 429,
            stage: "llm_request",
        });
    }
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "llm_request",
        });
    }

    let parsed: AnthropicResponse =
        serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "llm_parse",
        })?;
    drop(resp_body);

    Ok(LlmResponse::from_anthropic(parsed))
}

// ---------- SSE streaming ----------

#[derive(Deserialize)]
struct AnthropicContentBlockStart<'a> {
    #[serde(borrow)]
    content_block: Option<AnthropicContentBlock<'a>>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock<'a> {
    #[serde(rename = "type")]
    block_type: &'a str,
    id: Option<&'a str>,
    name: Option<&'a str>,
}

#[derive(Deserialize)]
struct AnthropicContentBlockDeltaEvent<'a> {
    #[serde(borrow)]
    delta: Option<AnthropicDeltaInner<'a>>,
}

#[derive(Deserialize)]
struct AnthropicDeltaInner<'a> {
    #[serde(rename = "type")]
    delta_type: &'a str,
    text: Option<&'a str>,
    partial_json: Option<&'a str>,
}

#[derive(Deserialize)]
struct AnthropicMessageDeltaEvent<'a> {
    #[serde(borrow)]
    delta: Option<AnthropicMessageDelta<'a>>,
}

#[derive(Deserialize)]
struct AnthropicMessageDelta<'a> {
    stop_reason: Option<&'a str>,
}

/// Anthropic SSE 流式累加器：逐事件拼接 content / tool_calls，最终产出 LlmResponse。
struct AnthropicStreamAccumulator {
    content: String,
    stop_reason: StopReason,
    /// 当前正在累积的 tool_use 块列表。
    tool_calls: Vec<ToolCallBuilder>,
}

struct ToolCallBuilder {
    id: String,
    name: String,
    input_json: String,
}

impl AnthropicStreamAccumulator {
    fn new() -> Self {
        Self {
            content: String::new(),
            stop_reason: StopReason::Other,
            tool_calls: Vec::new(),
        }
    }

    /// 处理单条 SSE 事件（event type + JSON data），返回 text_delta 借用供进度回调（零分配）。
    fn handle_event_value<'a>(&mut self, event_type: &str, data: &'a str) -> Option<&'a str> {
        match event_type {
            "content_block_start" => {
                if let Ok(v) = serde_json::from_str::<AnthropicContentBlockStart>(data) {
                    if let Some(cb) = v.content_block {
                        if cb.block_type == "tool_use" {
                            self.tool_calls.push(ToolCallBuilder {
                                id: cb.id.unwrap_or("").to_string(),
                                name: cb.name.unwrap_or("").to_string(),
                                input_json: String::new(),
                            });
                        }
                    }
                }
                None
            }
            "content_block_delta" => {
                if let Ok(v) = serde_json::from_str::<AnthropicContentBlockDeltaEvent>(data) {
                    if let Some(delta) = v.delta {
                        match delta.delta_type {
                            "text_delta" => {
                                if let Some(text) = delta.text {
                                    self.content.push_str(text);
                                    return Some(text);
                                }
                            }
                            "input_json_delta" => {
                                if let Some(partial) = delta.partial_json {
                                    if let Some(tc) = self.tool_calls.last_mut() {
                                        tc.input_json.push_str(partial);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                None
            }
            "message_delta" => {
                if let Ok(v) = serde_json::from_str::<AnthropicMessageDeltaEvent>(data) {
                    if let Some(d) = v.delta {
                        if let Some(sr) = d.stop_reason {
                            self.stop_reason = match sr {
                                "end_turn" => StopReason::EndTurn,
                                "tool_use" => StopReason::ToolUse,
                                "max_tokens" => StopReason::MaxTokens,
                                _ => StopReason::Other,
                            };
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// 流结束，产出最终 LlmResponse。
    fn finish(self) -> LlmResponse {
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.name,
                input: if tc.input_json.is_empty() {
                    "{}".to_string()
                } else {
                    tc.input_json
                },
            })
            .collect();
        let has_tool_calls = !tool_calls.is_empty();
        let stop_reason = if has_tool_calls && self.stop_reason != StopReason::ToolUse {
            StopReason::ToolUse
        } else {
            self.stop_reason
        };
        LlmResponse {
            content: self.content,
            stop_reason,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        }
    }
}

fn do_request_streaming(
    http: &mut dyn LlmHttpClient,
    url: &str,
    api_key: &str,
    body: &[u8],
    on_progress: Option<crate::llm::StreamProgressFn>,
) -> Result<LlmResponse> {
    const ANTHROPIC_VERSION: &str = "2023-06-01";
    let mut cl_buf = [0u8; 20];
    let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
    let headers = [
        ("x-api-key", api_key),
        ("anthropic-version", ANTHROPIC_VERSION),
        ("content-type", "application/json"),
        ("content-length", content_length),
    ];

    let mut accumulator = AnthropicStreamAccumulator::new();
    let mut sse_reader = crate::llm::sse::SseLineReader::new();
    let mut progress_cb = on_progress;

    let status = http
        .do_post_streaming(
            url,
            &headers,
            body,
            Some(crate::orchestrator::current_budget().response_body_max),
            &mut |chunk| {
                sse_reader.feed(chunk);
                while let Some(event) = sse_reader.next_event() {
                    let delta_text = accumulator.handle_event_value(&event.event, &event.data);

                    if let (Some(delta), Some(ref mut cb)) = (delta_text, &mut progress_cb) {
                        cb(delta, &accumulator.content);
                    }
                }
                Ok(())
            },
        )
        .map_err(|e| match e {
            Error::Http { status_code, .. } => Error::Http {
                status_code,
                stage: "llm_request",
            },
            _ => Error::Other {
                source: Box::new(std::io::Error::other(format!("{:?}", e))),
                stage: "llm_request",
            },
        })?;

    if status == 429 {
        log::warn!("[{}] rate limited (429)", TAG);
        return Err(Error::Http {
            status_code: 429,
            stage: "llm_request",
        });
    }
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "llm_request",
        });
    }

    Ok(accumulator.finish())
}

#[cfg(test)]
mod tests {
    use super::build_request_body;
    use crate::llm::{Message, ToolChoicePolicy, ToolSpec};

    #[test]
    fn request_contains_any_tool_choice_when_forced() {
        let body = build_request_body(
            "m",
            128,
            "",
            &[Message {
                role: std::borrow::Cow::Borrowed("user"),
                content: "hi".to_string(),
            }],
            Some(&[ToolSpec {
                name: "t".to_string(),
                description: "d".to_string(),
                parameters_json: r#"{"type":"object"}"#.into(),
            }]),
            ToolChoicePolicy::Require,
            false,
        )
        .expect("ok");
        let v: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            v.get("tool_choice")
                .and_then(|x| x.get("type"))
                .and_then(|x| x.as_str()),
            Some("any")
        );
    }
}
