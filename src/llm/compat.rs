use crate::config::LlmSource;

/// 工具调用兼容模式：Native 直接传 tools schema，PromptGuided 则由 prompt 引导输出文本工具块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCallSupport {
    Native,
    PromptGuided,
}

/// 最小模型兼容描述。
/// 当前只先收口工具调用模式，避免过早扩成一整套大 capability 矩阵。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LlmModelCompat {
    pub tool_call_support: ToolCallSupport,
}

impl LlmModelCompat {
    pub const fn native() -> Self {
        Self {
            tool_call_support: ToolCallSupport::Native,
        }
    }

    pub const fn prompt_guided() -> Self {
        Self {
            tool_call_support: ToolCallSupport::PromptGuided,
        }
    }
}

impl Default for LlmModelCompat {
    fn default() -> Self {
        Self::native()
    }
}

fn source_targets_ollama_api(source: &LlmSource) -> bool {
    if source.provider == "ollama" {
        return true;
    }
    let api_url = source.api_url.trim().to_ascii_lowercase();
    if api_url.is_empty() {
        return false;
    }
    api_url.contains("://ollama")
        || api_url.contains(".ollama")
        || api_url.ends_with(":11434")
        || api_url.contains(":11434/")
}

pub(crate) fn model_compat_for_source(source: &LlmSource) -> LlmModelCompat {
    if source_targets_ollama_api(source) {
        LlmModelCompat::prompt_guided()
    } else {
        LlmModelCompat::native()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LlmHeaderEntry, LlmModelKind};

    #[test]
    fn source_compat_marks_ollama_as_prompt_guided() {
        let compat = model_compat_for_source(&LlmSource {
            id: "ollama".to_string(),
            provider: "ollama".to_string(),
            api_key: "k".to_string(),
            model: "qwen2.5".to_string(),
            api_url: String::new(),
            max_tokens: None,
            model_kind: LlmModelKind::Text,
            custom_headers: Vec::<LlmHeaderEntry>::new(),
        });
        assert_eq!(compat.tool_call_support, ToolCallSupport::PromptGuided);
    }

    #[test]
    fn source_compat_marks_compat_ollama_endpoint_as_prompt_guided() {
        let compat = model_compat_for_source(&LlmSource {
            id: "compat-ollama".to_string(),
            provider: "openai_compatible".to_string(),
            api_key: "k".to_string(),
            model: "qwen2.5".to_string(),
            api_url: "http://192.168.1.100:11434/v1".to_string(),
            max_tokens: None,
            model_kind: LlmModelKind::Text,
            custom_headers: Vec::<LlmHeaderEntry>::new(),
        });
        assert_eq!(compat.tool_call_support, ToolCallSupport::PromptGuided);
    }
}
