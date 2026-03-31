//! Prompt-guided tool fallback helpers.
//! Shared by agent planning and heterogeneous fallback chains.

use crate::llm::{LlmResponse, StopReason, ToolCall, ToolSpec};
use crate::util::truncate_content_to_max;
use serde_json::Value;
use std::borrow::Cow;
use std::fmt::Write as _;

const TOOL_CALL_OPEN: &str = "<tool_call>";
const TOOL_CALL_CLOSE: &str = "</tool_call>";
const TOOL_PROTOCOL_HEADER: &str = "\n\n## Tool Use Protocol\nWhen you need a tool, output a JSON object inside <tool_call>...</tool_call> tags.\nFormat:\n<tool_call>\n{\"name\":\"tool_name\",\"arguments\":{}}\n</tool_call>\nYou may include normal assistant text outside tool_call blocks. After tool results arrive, continue reasoning and then answer normally.\n\n### Available Tools\n";
const TOOL_LINE_MAX_CHARS: usize = 160;
const TOOL_ARGS_MAX_CHARS: usize = 240;

pub(crate) fn append_tool_fallback_instructions(
    system: &mut String,
    max_len: usize,
    tool_specs: &[ToolSpec],
) {
    if tool_specs.is_empty() || system.len().saturating_add(TOOL_PROTOCOL_HEADER.len()) > max_len {
        return;
    }
    system.push_str(TOOL_PROTOCOL_HEADER);
    for spec in tool_specs {
        let description = truncate_content_to_max(spec.description.trim(), TOOL_LINE_MAX_CHARS);
        let parameters_json = spec.parameters.to_string();
        let args = truncate_content_to_max(&parameters_json, TOOL_ARGS_MAX_CHARS);
        let mut line = String::with_capacity(spec.name.len() + description.len() + args.len() + 32);
        let _ = writeln!(&mut line, "- {}: {}", spec.name, description);
        let _ = writeln!(&mut line, "  arguments: {}", args);
        if system.len().saturating_add(line.len()) > max_len {
            break;
        }
        system.push_str(&line);
    }
}

pub(crate) fn recover_text_tool_calls(mut response: LlmResponse) -> LlmResponse {
    if response.stop_reason == StopReason::ToolUse || response.content.trim().is_empty() {
        return response;
    }
    let (content, tool_calls) = parse_tool_calls_from_text(&response.content);
    if tool_calls.is_empty() {
        return response;
    }
    response.content = content;
    response.stop_reason = StopReason::ToolUse;
    response.tool_calls = Some(tool_calls);
    response
}

fn parse_tool_calls_from_text(text: &str) -> (String, Vec<ToolCall>) {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        let mut next_id = 1usize;
        let mut tool_calls = Vec::new();
        collect_tool_calls_from_value(&value, &mut next_id, &mut tool_calls);
        if !tool_calls.is_empty() {
            let content = extract_text_from_json_value(&value).unwrap_or_default();
            return (content, tool_calls);
        }
    }

    let mut next_id = 1usize;
    let mut tool_calls = Vec::new();
    let mut plain_text = String::with_capacity(text.len());
    let mut cursor = 0usize;

    while let Some(start_rel) = text[cursor..].find(TOOL_CALL_OPEN) {
        let start = cursor + start_rel;
        plain_text.push_str(text[cursor..start].trim_end());
        let body_start = start + TOOL_CALL_OPEN.len();
        let Some(end_rel) = text[body_start..].find(TOOL_CALL_CLOSE) else {
            plain_text.push_str(&text[start..]);
            cursor = text.len();
            break;
        };
        let body_end = body_start + end_rel;
        let body = normalize_tool_payload(&text[body_start..body_end]);
        if let Ok(value) = serde_json::from_str::<Value>(body.as_ref()) {
            collect_tool_calls_from_value(&value, &mut next_id, &mut tool_calls);
        }
        cursor = body_end + TOOL_CALL_CLOSE.len();
        if cursor < text.len() && !plain_text.is_empty() && !plain_text.ends_with('\n') {
            plain_text.push('\n');
        }
    }
    if cursor < text.len() {
        plain_text.push_str(text[cursor..].trim_start());
    }

    (plain_text.trim().to_string(), tool_calls)
}

fn normalize_tool_payload(raw: &str) -> Cow<'_, str> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("```") {
        return Cow::Borrowed(trimmed);
    }
    let mut lines: Vec<&str> = trimmed.lines().collect();
    if lines
        .first()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        lines.remove(0);
    }
    if lines
        .last()
        .is_some_and(|line| line.trim_start().starts_with("```"))
    {
        lines.pop();
    }
    Cow::Owned(lines.join("\n"))
}

fn extract_text_from_json_value(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|obj| obj.get("content"))
        .and_then(Value::as_str)
        .map(|content| content.trim().to_string())
}

fn collect_tool_calls_from_value(
    value: &Value,
    next_id: &mut usize,
    tool_calls: &mut Vec<ToolCall>,
) {
    match value {
        Value::Object(obj) => {
            if let Some(items) = obj.get("tool_calls").and_then(Value::as_array) {
                for item in items {
                    collect_tool_calls_from_value(item, next_id, tool_calls);
                }
                return;
            }
            if let Some(call) = build_tool_call_from_object(obj, next_id) {
                tool_calls.push(call);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tool_calls_from_value(item, next_id, tool_calls);
            }
        }
        _ => {}
    }
}

fn build_tool_call_from_object(
    obj: &serde_json::Map<String, Value>,
    next_id: &mut usize,
) -> Option<ToolCall> {
    let explicit_name = obj.get("name").and_then(Value::as_str);
    let explicit_args = obj.get("arguments").or_else(|| obj.get("input"));
    let nested_function = obj.get("function").and_then(Value::as_object);
    let name = explicit_name.or_else(|| {
        nested_function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
    })?;
    let args_value = explicit_args.or_else(|| {
        nested_function
            .and_then(|function| function.get("arguments").or_else(|| function.get("input")))
    });
    let input = normalize_arguments_json(args_value)?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let id = format!("text_tool_{}", *next_id);
            *next_id += 1;
            id
        });
    Some(ToolCall {
        id,
        name: name.to_string(),
        input,
    })
}

fn normalize_arguments_json(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => Some("{}".to_string()),
        Some(Value::Object(_)) => value.map(Value::to_string),
        Some(Value::String(raw)) => {
            let trimmed = raw.trim();
            if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                if parsed.is_object() {
                    return Some(parsed.to_string());
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_call_tag_keeps_surrounding_text() {
        let (text, tool_calls) = parse_tool_calls_from_text(
            "我先查一下。\n<tool_call>\n{\"name\":\"get_time\",\"arguments\":{}}\n</tool_call>\n稍等。",
        );
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "get_time");
        assert_eq!(tool_calls[0].input, "{}");
        assert!(text.contains("我先查一下。"));
        assert!(text.contains("稍等。"));
    }

    #[test]
    fn parse_openai_style_tool_call_json() {
        let (text, tool_calls) = parse_tool_calls_from_text(
            r#"{"content":"Let me check.","tool_calls":[{"id":"call_1","function":{"name":"get_time","arguments":"{}"}}]}"#,
        );
        assert_eq!(text, "Let me check.");
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].id, "call_1");
        assert_eq!(tool_calls[0].name, "get_time");
    }

    #[test]
    fn recover_text_tool_calls_promotes_end_turn_response() {
        let response = recover_text_tool_calls(LlmResponse {
            content: "<tool_call>{\"name\":\"get_time\",\"arguments\":{}}</tool_call>".to_string(),
            stop_reason: StopReason::EndTurn,
            tool_calls: None,
        });
        assert_eq!(response.stop_reason, StopReason::ToolUse);
        assert_eq!(response.tool_calls.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn tool_fallback_instructions_respect_budget() {
        let mut system = "base".to_string();
        append_tool_fallback_instructions(
            &mut system,
            12,
            &[ToolSpec {
                name: "get_time".to_string(),
                description: "time".to_string(),
                parameters: serde_json::json!({"type":"object"}),
            }],
        );
        assert_eq!(system, "base");
    }
}
