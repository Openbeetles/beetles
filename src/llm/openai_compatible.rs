//! OpenAI Chat Completions 兼容客户端；可对接 OpenAI / OpenRouter / 本地兼容服务。
//! 错误带 stage（llm_request / llm_parse）；HTTP 由 main 注入。
//! Supports both non-streaming (default) and SSE streaming modes.

use crate::config::{AppConfig, LlmSource};
use crate::error::{Error, Result};
use crate::llm::compat::model_compat_for_source;
use crate::llm::request_body::LlmRequestBody;
use crate::llm::types::{LlmResponse, StopReason, ToolCall, MAX_REQUEST_BODY_LEN};
use crate::llm::{LlmClient, LlmHttpClient, LlmModelCompat, Message, ToolChoicePolicy, ToolSpec};
use serde::Deserialize;

const TAG: &str = "llm::openai_compat";
const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
const CHAT_PATH: &str = "/chat/completions";
const DEFAULT_MAX_TOKENS: u32 = 2048;

/// OpenAI 兼容客户端；持 config 只读，HTTP 由 chat 时注入。
pub struct OpenAiCompatibleClient {
    /// 预拼接 base + `/chat/completions`，避免每次请求分配。
    chat_url: String,
    /// `Authorization: Bearer …` 完整值；空密钥时为 `None`（不发送头）。
    auth_bearer: Option<String>,
    model: String,
    max_tokens: u32,
    stream: bool,
    compat: LlmModelCompat,
}

impl OpenAiCompatibleClient {
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

    /// 从单源配置构造，供多源回退使用。
    pub fn from_source(source: &LlmSource, stream: bool) -> Self {
        let api_base = if source.api_url.is_empty() {
            match source.provider.as_str() {
                "gemini" => "https://generativelanguage.googleapis.com/v1beta",
                "glm" => "https://open.bigmodel.cn/api/paas/v4",
                "qwen" => "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "deepseek" => "https://api.deepseek.com/v1",
                "moonshot" => "https://api.moonshot.cn/v1",
                "ollama" => "http://localhost:11434/v1",
                _ => DEFAULT_API_BASE,
            }
            .to_string()
        } else {
            source.api_url.trim_end_matches('/').to_string()
        };
        let chat_url = format!("{}{}", api_base, CHAT_PATH);
        let key = source.api_key.clone();
        let auth_bearer = if key.is_empty() {
            None
        } else {
            Some(format!("Bearer {}", key))
        };
        Self {
            chat_url,
            auth_bearer,
            model: source.model.clone(),
            max_tokens: source.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            stream,
            compat: model_compat_for_source(source),
        }
    }
}

// --- OpenAI Chat Completions 响应 DTO ---

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    _type: Option<String>,
    function: Option<OpenAiToolCallFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCallFunction {
    name: Option<String>,
    arguments: Option<String>,
}

fn finish_reason_to_stop_reason(s: Option<&str>) -> StopReason {
    match s {
        Some("stop") => StopReason::EndTurn,
        Some("tool_calls") => StopReason::ToolUse,
        Some("length") => StopReason::MaxTokens,
        _ => StopReason::Other,
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
) -> Result<LlmRequestBody> {
    fn push_json_string_field(out: &mut LlmRequestBody, key: &str, value: &str) -> Result<()> {
        out.push_json_string_field(key, value)
    }

    fn push_messages_field(
        out: &mut LlmRequestBody,
        system: &str,
        messages: &[Message],
    ) -> Result<()> {
        out.push_str("\"messages\":[")?;
        let mut wrote_any = false;
        if !system.is_empty() {
            out.push_byte(b'{')?;
            push_json_string_field(out, "role", "system")?;
            out.push_byte(b',')?;
            push_json_string_field(out, "content", system)?;
            out.push_byte(b'}')?;
            wrote_any = true;
        }
        for message in messages {
            if wrote_any {
                out.push_byte(b',')?;
            }
            out.push_byte(b'{')?;
            push_json_string_field(out, "role", &message.role)?;
            out.push_byte(b',')?;
            push_json_string_field(out, "content", &message.content)?;
            out.push_byte(b'}')?;
            wrote_any = true;
        }
        out.push_byte(b']')
    }

    fn push_tools_field(out: &mut LlmRequestBody, tools: &[ToolSpec]) -> Result<()> {
        out.push_str(",\"tools\":[")?;
        for (idx, tool) in tools.iter().enumerate() {
            if idx > 0 {
                out.push_byte(b',')?;
            }
            out.push_str("{\"type\":\"function\",\"function\":{")?;
            push_json_string_field(out, "name", &tool.name)?;
            out.push_byte(b',')?;
            push_json_string_field(out, "description", &tool.description)?;
            out.push_str(",\"parameters\":")?;
            out.push_str(tool.parameters_json())?;
            out.push_str("}}")?;
        }
        out.push_byte(b']')
    }

    let mut num_buf = [0u8; 20];
    let max_tokens_str = crate::util::usize_to_decimal_buf(&mut num_buf, max_tokens as usize);
    let mut body = LlmRequestBody::with_estimated_capacity(
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
                                + 64
                        })
                        .sum::<usize>()
                })
                .unwrap_or(0)
            + 128,
        MAX_REQUEST_BODY_LEN,
    );
    body.push_byte(b'{')?;
    push_json_string_field(&mut body, "model", model)?;
    body.push_str(",\"max_tokens\":")?;
    body.push_str(max_tokens_str)?;
    body.push_byte(b',')?;
    push_messages_field(&mut body, system, messages)?;
    let has_tools = tools.is_some_and(|items| !items.is_empty());
    if let Some(items) = tools.filter(|items| !items.is_empty()) {
        push_tools_field(&mut body, items)?;
    }
    if has_tools && tool_choice == ToolChoicePolicy::Require {
        body.push_str(",\"tool_choice\":\"required\"")?;
    }
    if stream {
        body.push_str(",\"stream\":true")?;
    }
    body.push_byte(b'}')?;

    body.finish(MAX_REQUEST_BODY_LEN)
}

impl LlmClient for OpenAiCompatibleClient {
    fn model_compat(&self) -> LlmModelCompat {
        self.compat
    }

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
                do_request_streaming(
                    http,
                    &self.chat_url,
                    self.auth_bearer.as_deref(),
                    body.as_ref(),
                    None,
                )
            })
        } else {
            crate::llm::retry::with_retry(2, 500, TAG, http, |http| {
                do_request(
                    http,
                    &self.chat_url,
                    self.auth_bearer.as_deref(),
                    body.as_ref(),
                )
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
            do_request_streaming(
                http,
                &self.chat_url,
                self.auth_bearer.as_deref(),
                body.as_ref(),
                progress.take(),
            )
        })
    }
}

fn do_request(
    http: &mut dyn LlmHttpClient,
    url: &str,
    auth_bearer: Option<&str>,
    body: &[u8],
) -> Result<LlmResponse> {
    let mut cl_buf = [0u8; 20];
    let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
    let mut headers: Vec<(&str, &str)> = vec![
        ("Content-Type", "application/json"),
        ("Content-Length", content_length),
    ];
    if let Some(bearer) = auth_bearer {
        headers.insert(0, ("Authorization", bearer));
    }
    let (status, resp_body) = http
        .do_post(url, &headers, body)
        .map_err(|e| crate::llm::map_transport_error(e, "llm_request"))?;

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

    let choice = {
        let parsed: OpenAiResponse =
            serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "llm_parse",
            })?;
        parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "openai response has no choices",
                )),
                stage: "llm_parse",
            })?
    };
    drop(resp_body);

    let content = choice.message.content.unwrap_or_default();
    let stop_reason = finish_reason_to_stop_reason(choice.finish_reason.as_deref());
    let tool_calls = choice
        .message
        .tool_calls
        .map(|tc_list| {
            tc_list
                .into_iter()
                .filter_map(|tc| {
                    let id = tc.id.unwrap_or_default();
                    let func = tc.function?;
                    let name = func.name.unwrap_or_default();
                    let input = func.arguments.unwrap_or_else(|| "{}".to_string());
                    Some(ToolCall { id, name, input })
                })
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());

    let has_tool_calls = tool_calls.as_ref().is_some_and(|v| !v.is_empty());
    let stop_reason = if has_tool_calls && stop_reason != StopReason::ToolUse {
        StopReason::ToolUse
    } else {
        stop_reason
    };

    Ok(LlmResponse {
        content,
        stop_reason,
        tool_calls: if tool_calls.as_ref().is_none_or(|v| v.is_empty()) {
            None
        } else {
            tool_calls
        },
    })
}

// ---------- SSE streaming ----------

/// OpenAI Chat Completions 流式 chunk（借用 `event.data`，零拷贝字符串）。
#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk<'a> {
    #[serde(borrow, default)]
    choices: Vec<OpenAiStreamChoice<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice<'a> {
    finish_reason: Option<&'a str>,
    delta: Option<OpenAiStreamDelta<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamDelta<'a> {
    content: Option<&'a str>,
    #[serde(borrow, default)]
    tool_calls: Option<Vec<OpenAiStreamToolCallDelta<'a>>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamToolCallDelta<'a> {
    index: Option<u64>,
    id: Option<&'a str>,
    function: Option<OpenAiStreamFunctionDelta<'a>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamFunctionDelta<'a> {
    name: Option<&'a str>,
    arguments: Option<&'a str>,
}

/// OpenAI SSE 流式累加器：逐事件拼接 content / tool_calls。
struct OpenAiStreamAccumulator {
    content: String,
    stop_reason: StopReason,
    /// 按 index 累积 tool_calls（OpenAI streaming delta 中 tool_calls 带 index 字段）。
    tool_calls: Vec<OpenAiToolCallBuilder>,
}

struct OpenAiToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAiStreamAccumulator {
    fn new() -> Self {
        Self {
            content: String::new(),
            stop_reason: StopReason::Other,
            tool_calls: Vec::new(),
        }
    }

    /// 处理单条 SSE data 的已解析 JSON chunk；返回 content_delta 借用供进度回调（零分配）。
    fn handle_chunk<'a>(&mut self, chunk: &OpenAiStreamChunk<'a>) -> Option<&'a str> {
        let mut delta_text: Option<&'a str> = None;

        for choice in &chunk.choices {
            if let Some(fr) = choice.finish_reason {
                self.stop_reason = finish_reason_to_stop_reason(Some(fr));
            }

            let delta = match &choice.delta {
                Some(d) => d,
                None => continue,
            };

            if let Some(text) = delta.content {
                self.content.push_str(text);
                delta_text = Some(text);
            }

            if let Some(ref tc_arr) = delta.tool_calls {
                for tc in tc_arr {
                    let index = tc.index.unwrap_or(0) as usize;
                    const MAX_TOOL_CALL_INDEX: usize = 128;
                    if index > MAX_TOOL_CALL_INDEX {
                        log::warn!(
                            "[openai_stream] tool_calls index {} exceeds max {}, skipping",
                            index,
                            MAX_TOOL_CALL_INDEX
                        );
                        continue;
                    }
                    while self.tool_calls.len() <= index {
                        self.tool_calls.push(OpenAiToolCallBuilder {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                    }
                    let builder = &mut self.tool_calls[index];
                    if let Some(id) = tc.id {
                        builder.id = id.to_string();
                    }
                    if let Some(ref func) = tc.function {
                        if let Some(name) = func.name {
                            builder.name.push_str(name);
                        }
                        if let Some(args) = func.arguments {
                            builder.arguments.push_str(args);
                        }
                    }
                }
            }
        }
        delta_text
    }

    fn finish(self) -> LlmResponse {
        let tool_calls: Vec<ToolCall> = self
            .tool_calls
            .into_iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| ToolCall {
                id: tc.id,
                name: tc.name,
                input: if tc.arguments.is_empty() {
                    "{}".to_string()
                } else {
                    tc.arguments
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
    auth_bearer: Option<&str>,
    body: &[u8],
    on_progress: Option<crate::llm::StreamProgressFn>,
) -> Result<LlmResponse> {
    let mut cl_buf = [0u8; 20];
    let content_length = crate::util::usize_to_decimal_buf(&mut cl_buf, body.len());
    let mut headers: Vec<(&str, &str)> = vec![
        ("Content-Type", "application/json"),
        ("Content-Length", content_length),
    ];
    if let Some(bearer) = auth_bearer {
        headers.insert(0, ("Authorization", bearer));
    }

    let mut accumulator = OpenAiStreamAccumulator::new();
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
                    if event.data == "[DONE]" {
                        continue;
                    }
                    let parsed = match serde_json::from_str::<OpenAiStreamChunk>(&event.data) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let delta_text = accumulator.handle_chunk(&parsed);

                    if let (Some(delta), Some(ref mut cb)) = (delta_text, &mut progress_cb) {
                        cb(delta, &accumulator.content);
                    }
                }
                Ok(())
            },
        )
        .map_err(|e| crate::llm::map_transport_error(e, "llm_request"))?;

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
    use super::{build_request_body, OpenAiCompatibleClient};
    use crate::config::LlmSource;
    use crate::llm::{Message, ToolCallSupport, ToolChoicePolicy, ToolSpec};

    #[test]
    fn request_contains_required_tool_choice_when_forced() {
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
            v.get("tool_choice").and_then(|x| x.as_str()),
            Some("required")
        );
    }

    #[test]
    fn large_request_body_uses_shared_external_preferred_buffer() {
        let content = "x".repeat(crate::platform::ByteBuffer::EXTERNAL_PREFERRED_THRESHOLD + 1);
        let body = build_request_body(
            "m",
            128,
            "",
            &[Message {
                role: std::borrow::Cow::Borrowed("user"),
                content,
            }],
            None,
            ToolChoicePolicy::Auto,
            false,
        )
        .expect("ok");

        assert!(body.is_external_preferred());
        let v: serde_json::Value = serde_json::from_slice(body.as_ref()).expect("json");
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn ollama_uses_prompt_guided_tools() {
        let client = OpenAiCompatibleClient::from_source(
            &LlmSource {
                provider: "ollama".to_string(),
                api_key: "k".to_string(),
                model: "qwen2.5".to_string(),
                api_url: String::new(),
                max_tokens: None,
            },
            false,
        );
        assert_eq!(
            crate::llm::LlmClient::model_compat(&client).tool_call_support,
            ToolCallSupport::PromptGuided
        );
    }
}
