//! LLM source topology helpers.
//! LLM 源拓扑辅助函数：legacy 单源回退、provider 分类、fallback 顺序。

use crate::config::{AppConfig, LlmModelKind, LlmSource};
const OPENAI_COMPATIBLE_PROVIDER_FAMILY: &[&str] = &[
    "openai",
    "openai_compatible",
    "gemini",
    "glm",
    "qwen",
    "deepseek",
    "moonshot",
    "ollama",
];

pub(crate) fn provider_uses_openai_compatible_client(provider: &str) -> bool {
    OPENAI_COMPATIBLE_PROVIDER_FAMILY.contains(&provider)
}

fn legacy_single_source(config: &AppConfig) -> LlmSource {
    LlmSource {
        id: "env_default".to_string(),
        provider: config.model_provider.clone(),
        api_key: config.api_key.clone(),
        model: config.model.clone(),
        api_url: config.api_url.clone(),
        max_tokens: None,
        model_kind: LlmModelKind::Text,
        custom_headers: Vec::new(),
    }
}

pub(crate) fn ensure_legacy_llm_sources(config: &mut AppConfig) {
    if config.llm_sources.is_empty() {
        config.llm_sources = vec![legacy_single_source(config)];
    }
}

pub(crate) fn llm_sources_or_legacy(config: &AppConfig) -> Vec<LlmSource> {
    if config.llm_sources.is_empty() {
        vec![legacy_single_source(config)]
    } else {
        config.llm_sources.clone()
    }
}

pub(crate) fn llm_source_is_usable(source: &LlmSource) -> bool {
    let chat_capable = matches!(
        source.model_kind,
        LlmModelKind::Text | LlmModelKind::Multimodal
    );
    let has_key = !source.api_key.trim().is_empty();
    let has_model = !source.model.trim().is_empty();
    let has_provider = !source.provider.trim().is_empty();
    let has_url = !source.api_url.trim().is_empty()
        || provider_uses_openai_compatible_client(source.provider.as_str());
    chat_capable && has_key && has_model && has_provider && has_url
}

pub(crate) fn llm_fallback_source_indices(config: &AppConfig) -> Vec<usize> {
    config
        .llm_sources
        .iter()
        .enumerate()
        .filter_map(|(i, source)| llm_source_is_usable(source).then_some(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::llm_fallback_source_indices;
    use crate::config::{AppConfig, LlmHeaderEntry, LlmModelKind, LlmSource};

    fn source(model: &str, api_key: &str) -> LlmSource {
        LlmSource {
            id: format!("source-{model}"),
            provider: "openai".to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            api_url: "https://example.test/v1".to_string(),
            max_tokens: None,
            model_kind: LlmModelKind::Text,
            custom_headers: Vec::<LlmHeaderEntry>::new(),
        }
    }

    #[test]
    fn fallback_order_is_list_order_after_filtering_unusable_sources() {
        let mut config = AppConfig::load_from_env();
        config.llm_sources = vec![
            source("first", "k1"),
            source("missing-key", ""),
            source("second", "k2"),
            source("third", "k3"),
        ];
        assert_eq!(llm_fallback_source_indices(&config), vec![0, 2, 3]);
    }

    #[test]
    fn fallback_order_excludes_generation_only_sources_from_chat() {
        let mut config = AppConfig::load_from_env();
        let mut image = source("image-model", "k-image");
        image.model_kind = LlmModelKind::ImageGeneration;
        let mut video = source("video-model", "k-video");
        video.model_kind = LlmModelKind::VideoGeneration;
        let mut multimodal = source("vision-chat", "k-vision");
        multimodal.model_kind = LlmModelKind::Multimodal;
        config.llm_sources = vec![image, source("text-chat", "k-text"), video, multimodal];

        assert_eq!(llm_fallback_source_indices(&config), vec![1, 3]);
    }
}
