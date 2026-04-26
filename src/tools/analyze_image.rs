//! analyze_image 工具：通过 LLM 多模态能力分析图片 URL；HTTP 经 ToolContext 注入，不依赖 platform。
//! 支持多源顺序回退，与 FallbackLlmClient 降级行为一致。
//! analyze_image tool: analyzes image URLs via LLM vision; HTTP injected through ToolContext.
//! Supports multi-source fallback, consistent with FallbackLlmClient.

use crate::config::LlmSource;
use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolEffectClass, ToolMetadata};
use serde_json::json;

const TAG: &str = "tools::analyze_image";
const STAGE: &str = "tool_analyze_image";
const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
const VISION_MAX_TOKENS: u32 = 1024;

pub struct AnalyzeImageTool {
    sources: Vec<LlmSource>,
}

impl AnalyzeImageTool {
    pub fn new(config: &crate::config::AppConfig) -> Self {
        let sources: Vec<LlmSource> = config
            .llm_sources
            .iter()
            .filter(|s| !s.api_key.trim().is_empty())
            .cloned()
            .collect();
        Self { sources }
    }

    /// 用单个源执行一次 vision 请求，返回 Ok(text) 或 Err。
    fn try_source(
        source: &LlmSource,
        image_url: &str,
        question: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<String> {
        let is_anthropic = source.provider == "anthropic";

        let body = if is_anthropic {
            Self::build_anthropic_request(&source.model, image_url, question)
        } else {
            Self::build_openai_request(&source.model, image_url, question)
        };

        let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: STAGE,
        })?;

        let url = if is_anthropic {
            if source.api_url.trim().is_empty() {
                ANTHROPIC_API_URL.to_string()
            } else {
                source.api_url.trim_end_matches('/').to_string()
            }
        } else {
            let base = if source.api_url.trim().is_empty() {
                OPENAI_DEFAULT_API_BASE
            } else {
                source.api_url.trim_end_matches('/')
            };
            format!("{}/chat/completions", base)
        };

        let bearer;
        let headers: Vec<(&str, &str)> = if is_anthropic {
            vec![
                ("Content-Type", "application/json"),
                ("x-api-key", &source.api_key),
                ("anthropic-version", "2023-06-01"),
            ]
        } else {
            bearer = format!("Bearer {}", source.api_key);
            vec![
                ("Content-Type", "application/json"),
                ("Authorization", &bearer),
            ]
        };

        log::info!(
            "[{}] POST {} provider={} model={} image_url_len={}",
            TAG,
            url,
            source.provider,
            source.model,
            image_url.len()
        );

        let (status, resp_body) = ctx.post_with_headers(&url, &headers, &body_bytes)?;

        if status >= 400 {
            let err_bytes = resp_body.as_ref();
            let preview_len = err_bytes.len().min(256);
            let err_text = String::from_utf8_lossy(&err_bytes[..preview_len]);
            log::warn!("[{}] API returned status={}: {}", TAG, status, err_text);
            return Err(Error::Http {
                status_code: status,
                stage: STAGE,
            });
        }

        let parsed: serde_json::Value =
            serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
                source: Box::new(e),
                stage: STAGE,
            })?;

        let text = if is_anthropic {
            Self::extract_anthropic_text(&parsed)
        } else {
            Self::extract_openai_text(&parsed)
        };

        match text {
            Some(t) => {
                log::info!("[{}] result len={}", TAG, t.len());
                Ok(t)
            }
            None => {
                log::warn!("[{}] failed to extract text from response", TAG);
                Ok("analyze_image: could not extract text from API response".to_string())
            }
        }
    }

    fn build_anthropic_request(model: &str, image_url: &str, question: &str) -> serde_json::Value {
        json!({
            "model": model,
            "max_tokens": VISION_MAX_TOKENS,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": image_url
                        }
                    },
                    {
                        "type": "text",
                        "text": question
                    }
                ]
            }]
        })
    }

    fn build_openai_request(model: &str, image_url: &str, question: &str) -> serde_json::Value {
        json!({
            "model": model,
            "max_tokens": VISION_MAX_TOKENS,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": image_url }
                    },
                    {
                        "type": "text",
                        "text": question
                    }
                ]
            }]
        })
    }

    fn extract_anthropic_text(resp: &serde_json::Value) -> Option<String> {
        resp.get("content")?
            .as_array()?
            .iter()
            .filter_map(|block| {
                if block.get("type")?.as_str()? == "text" {
                    block.get("text")?.as_str().map(String::from)
                } else {
                    None
                }
            })
            .next()
    }

    fn extract_openai_text(resp: &serde_json::Value) -> Option<String> {
        resp.get("choices")?
            .as_array()?
            .first()?
            .get("message")?
            .get("content")?
            .as_str()
            .map(String::from)
    }
}

impl Tool for AnalyzeImageTool {
    fn name(&self) -> &'static str {
        "analyze_image"
    }

    fn description(&self) -> &'static str {
        "Analyze an image from a URL using vision AI. Use this when a user sends an image URL and you need to understand its content."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"image_url":{"type":"string","description":"The HTTP/HTTPS URL of the image to analyze"},"question":{"type":"string","description":"A specific question about the image (default: describe the image in detail)"}},"required":["image_url"]}"#
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_effect_class(ToolEffectClass::NetworkSearch)
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let m = parse_tool_args(args, STAGE)?;

        let image_url = m
            .get("image_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config(STAGE, "missing or invalid image_url"))?;

        if !image_url.starts_with("http://") && !image_url.starts_with("https://") {
            return Err(Error::config(
                STAGE,
                "image_url must start with http:// or https://",
            ));
        }
        if image_url.len() > 2048 {
            return Err(Error::config(STAGE, "image_url too long (max 2048)"));
        }

        let question = m
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail");
        if question.len() > 1024 {
            return Err(Error::config(STAGE, "question too long (max 1024)"));
        }

        if self.sources.is_empty() {
            return Ok("analyze_image: no API key configured".to_string());
        }

        // 多源顺序回退，与 FallbackLlmClient 行为一致
        let mut last_err = None;
        for (i, source) in self.sources.iter().enumerate() {
            match Self::try_source(source, image_url, question, ctx) {
                Ok(text) => return Ok(text),
                Err(e) => {
                    if i + 1 < self.sources.len() {
                        log::warn!(
                            "[{}] source {} ({}/{}) failed, trying next: {}",
                            TAG,
                            i,
                            source.provider,
                            source.model,
                            e
                        );
                    }
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| Error::config(STAGE, "all sources failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::AnalyzeImageTool;
    use crate::tools::{Tool, ToolEffectClass};

    #[test]
    fn analyze_image_metadata_marks_url_vision_as_network_search() {
        let tool = AnalyzeImageTool {
            sources: Vec::new(),
        };

        assert_eq!(tool.metadata().effect_class, ToolEffectClass::NetworkSearch);
        assert!(tool.requires_network());
    }
}
