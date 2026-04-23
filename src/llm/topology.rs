//! LLM source topology helpers.
//! LLM 源拓扑辅助函数：legacy 单源回退、provider 分类、fallback 顺序。

use crate::config::{AppConfig, LlmSource};
use std::collections::HashSet;

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
        provider: config.model_provider.clone(),
        api_key: config.api_key.clone(),
        model: config.model.clone(),
        api_url: config.api_url.clone(),
        max_tokens: None,
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
    let has_key = !source.api_key.trim().is_empty();
    let has_model = !source.model.trim().is_empty();
    let has_provider = !source.provider.trim().is_empty();
    let has_url = !source.api_url.trim().is_empty()
        || provider_uses_openai_compatible_client(source.provider.as_str());
    has_key && has_model && has_provider && has_url
}

pub(crate) fn llm_fallback_source_indices(config: &AppConfig) -> Vec<usize> {
    let n = config.llm_sources.len();
    let valid: Vec<bool> = config
        .llm_sources
        .iter()
        .map(llm_source_is_usable)
        .collect();
    let valid_list: Vec<usize> = (0..n).filter(|&i| valid[i]).collect();
    let router = config.llm_router_source_index.map(|u| u as usize);
    let worker = config.llm_worker_source_index.map(|u| u as usize);
    match router {
        None => valid_list,
        Some(r) if r < n && valid.get(r).copied().unwrap_or(false) => {
            let mut out = vec![r];
            let mut seen = HashSet::from([r]);
            if let Some(w) = worker {
                if w < n && w != r && valid.get(w).copied().unwrap_or(false) {
                    out.push(w);
                    seen.insert(w);
                }
            }
            for i in valid_list {
                if !seen.contains(&i) {
                    out.push(i);
                }
            }
            out
        }
        Some(_) => valid_list,
    }
}
