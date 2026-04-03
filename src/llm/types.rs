//! LLM 请求/响应 DTO，与 Anthropic Messages API 对齐。
//! Request/response DTOs aligned with Anthropic Messages API.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// 请求体大小上界（字节），与 constants::MAX_RESPONSE_BODY_LEN 单源一致。
pub use crate::constants::MAX_REQUEST_BODY_LEN;
/// 单条 message content 建议上界（字符），超限由调用方截断或返回 Error。
pub const MAX_MESSAGE_CONTENT_LEN: usize = 64 * 1024;

/// 流式输出进度回调；delta 为本次新增文本，accumulated 为累计全文。
/// Stream progress callback; delta = new text, accumulated = full text so far.
pub type StreamProgressFn<'a> = &'a mut dyn FnMut(&str, &str);

/// 工具选择策略：Auto 由模型决定；Require 强制本轮必须产出结构化 tool call。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChoicePolicy {
    Auto,
    Require,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Cow<'static, str>,
    pub content: String,
}

/// 工具规格；parameters_json 为预序列化 JSON Schema，避免在热路径反复构造 Value。
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters_json: Box<str>,
}

impl ToolSpec {
    pub fn parameters_json(&self) -> &str {
        self.parameters_json.as_ref()
    }
}

/// 响应 stop_reason 枚举。
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    #[serde(other)]
    Other,
}

/// Anthropic 响应体（Messages API）。
#[derive(Debug, Deserialize)]
pub struct AnthropicResponse {
    pub id: Option<String>,
    pub content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
}

#[derive(Debug, Deserialize)]
pub struct AnthropicContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// 单次 tool_use 调用（与 API content 中 type="tool_use" 对应）。
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON 对象字符串，供 ToolRegistry::execute(name, args, ctx) 的 args。
    pub input: String,
}

/// 对外暴露的 LLM 响应：文本内容 + 停止原因 + 可选 tool_calls。
#[derive(Clone, Debug)]
pub struct LlmResponse {
    pub content: String,
    pub stop_reason: StopReason,
    /// stop_reason == ToolUse 时由解析填充。
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl LlmResponse {
    /// 从 API 响应提取 text 块合并为 content，tool_use 块收集为 tool_calls。
    pub fn from_anthropic(r: AnthropicResponse) -> Self {
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        for b in r.content {
            if b.block_type == "text" {
                if let Some(t) = b.text {
                    if !content.is_empty() {
                        content.push('\n');
                    }
                    content.push_str(&t);
                }
            } else if b.block_type == "tool_use" {
                let id = b.id.unwrap_or_default();
                let name = b.name.unwrap_or_default();
                let input = b
                    .input
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "{}".to_string());
                tool_calls.push(ToolCall { id, name, input });
            }
        }
        let stop_reason = r.stop_reason.unwrap_or(StopReason::Other);
        let has_tool_calls = !tool_calls.is_empty();
        // If tool_calls present but stop_reason is not ToolUse (e.g. null from API),
        // force ToolUse so the agent loop executes them instead of returning text.
        let stop_reason = if has_tool_calls && stop_reason != StopReason::ToolUse {
            StopReason::ToolUse
        } else {
            stop_reason
        };
        LlmResponse {
            content,
            stop_reason,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        }
    }
}
