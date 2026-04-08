use serde_json::{Map, Value};

#[derive(Debug)]
pub(crate) enum LlmJsonPayload {
    Absent,
    Null,
    Value(Value),
}

pub(crate) fn parse_llm_json_payload(raw: &str) -> LlmJsonPayload {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return LlmJsonPayload::Absent;
    }
    if let Some(value) = parse_jsonish_text(trimmed) {
        return map_payload(value);
    }
    if let Some(fenced) = extract_first_code_fence_body(trimmed) {
        if let Some(value) = parse_jsonish_text(fenced) {
            return map_payload(value);
        }
    }
    LlmJsonPayload::Absent
}

pub(crate) fn get_object_text(object: &Map<String, Value>, key: &str) -> String {
    object.get(key).map(coerce_json_text).unwrap_or_default()
}

pub(crate) fn get_object_bool(object: &Map<String, Value>, key: &str) -> Option<bool> {
    object.get(key).and_then(coerce_json_bool)
}

pub(crate) fn get_object_u64(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(coerce_json_u64)
}

pub(crate) fn get_object_string_list(object: &Map<String, Value>, key: &str) -> Vec<String> {
    object
        .get(key)
        .map(coerce_json_string_list)
        .unwrap_or_default()
}

pub(crate) fn coerce_json_text(value: &Value) -> String {
    coerce_json_text_with_depth(value, 0)
}

pub(crate) fn coerce_json_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Some(int != 0)
            } else {
                number.as_u64().map(|int| int != 0)
            }
        }
        Value::String(text) => parse_bool_text(text),
        Value::Array(items) => items.iter().find_map(coerce_json_bool),
        Value::Object(object) => ["value", "enabled", "idle_enabled", "applies"]
            .iter()
            .find_map(|key| object.get(*key).and_then(coerce_json_bool)),
        Value::Null => None,
    }
}

pub(crate) fn coerce_json_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .or_else(|| number.as_i64().and_then(|value| u64::try_from(value).ok())),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Ok(value) = trimmed.parse::<u64>() {
                return Some(value);
            }
            let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
            (!digits.is_empty())
                .then_some(digits)
                .and_then(|digits| digits.parse::<u64>().ok())
        }
        Value::Array(items) => items.iter().find_map(coerce_json_u64),
        Value::Object(object) => ["value", "seconds", "idle_interval_secs", "interval_secs"]
            .iter()
            .find_map(|key| object.get(*key).and_then(coerce_json_u64)),
        Value::Bool(flag) => Some(u64::from(*flag)),
        Value::Null => None,
    }
}

pub(crate) fn coerce_json_string_list(value: &Value) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        Value::Number(_) | Value::Bool(_) => vec![value.to_string()],
        Value::Array(items) => items
            .iter()
            .flat_map(coerce_json_string_list)
            .collect::<Vec<_>>(),
        Value::Object(object) => {
            for key in ["target", "targets", "id", "path", "name", "value"] {
                if let Some(value) = object.get(key) {
                    let nested = coerce_json_string_list(value);
                    if !nested.is_empty() {
                        return nested;
                    }
                }
            }
            let text = coerce_json_text(value);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text]
            }
        }
    }
}

fn map_payload(value: Value) -> LlmJsonPayload {
    if value.is_null() {
        LlmJsonPayload::Null
    } else {
        LlmJsonPayload::Value(value)
    }
}

fn parse_jsonish_text(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(trimmed).ok().or_else(|| {
        extract_first_json_slice(trimmed)
            .and_then(|slice| serde_json::from_str::<Value>(slice).ok())
    })
}

fn extract_first_code_fence_body(raw: &str) -> Option<&str> {
    let fence_start = raw.find("```")?;
    let after_fence = &raw[fence_start + 3..];
    let newline = after_fence.find('\n')?;
    let body = &after_fence[newline + 1..];
    let fence_end = body.find("```")?;
    Some(body[..fence_end].trim())
}

fn extract_first_json_slice(raw: &str) -> Option<&str> {
    for (start, ch) in raw.char_indices() {
        if ch != '{' && ch != '[' {
            continue;
        }
        if let Some(len) = balanced_json_prefix_len(&raw[start..]) {
            return Some(&raw[start..start + len]);
        }
    }
    None
}

fn balanced_json_prefix_len(raw: &str) -> Option<usize> {
    let mut object_depth = 0usize;
    let mut array_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => object_depth = object_depth.saturating_add(1),
            '}' => {
                if object_depth == 0 {
                    return None;
                }
                object_depth -= 1;
            }
            '[' => array_depth = array_depth.saturating_add(1),
            ']' => {
                if array_depth == 0 {
                    return None;
                }
                array_depth -= 1;
            }
            _ => {}
        }
        if object_depth == 0 && array_depth == 0 {
            return Some(idx + ch.len_utf8());
        }
    }
    None
}

fn coerce_json_text_with_depth(value: &Value, depth: usize) -> String {
    if depth >= 4 {
        return String::new();
    }
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.trim().to_string(),
        Value::Number(_) | Value::Bool(_) => value.to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| coerce_json_text_with_depth(item, depth + 1))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
        Value::Object(object) => object
            .iter()
            .filter_map(|(key, value)| {
                let text = coerce_json_text_with_depth(value, depth + 1);
                (!text.is_empty()).then(|| format!("{key}: {text}"))
            })
            .collect::<Vec<_>>()
            .join("; "),
    }
}

fn parse_bool_text(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "y" | "1" | "on" | "enabled" | "allow" | "allowed" => Some(true),
        "false" | "no" | "n" | "0" | "off" | "disabled" | "deny" | "denied" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_null_payload() {
        let payload = parse_llm_json_payload("```json\nnull\n```");
        assert!(matches!(payload, LlmJsonPayload::Null));
    }

    #[test]
    fn extracts_wrapped_json_object() {
        let payload = parse_llm_json_payload("Here is the update:\n{\"field\":\"value\"}\nDone.");
        match payload {
            LlmJsonPayload::Value(Value::Object(object)) => {
                assert_eq!(get_object_text(&object, "field"), "value");
            }
            other => panic!("unexpected payload: {other:?}"),
        }
    }

    #[test]
    fn coerces_nested_text_shapes() {
        let value = serde_json::json!({
            "scene": { "place": "desk", "status": ["quiet", "focused"] },
            "battery": 82
        });
        let text = coerce_json_text(&value);
        assert!(text.contains("scene:"));
        assert!(text.contains("place: desk"));
        assert!(text.contains("status: quiet; focused"));
        assert!(text.contains("battery: 82"));
    }
}
