//! model_config 工具：运行时 LLM 模型配置管理。
//! model_config tool: runtime LLM model configuration management.

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::json;
use std::sync::Arc;

const LLM_CONFIG_PATH: &str = "config/llm.json";

pub struct ModelConfigTool {
    platform: Arc<dyn Platform>,
}

impl ModelConfigTool {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl Tool for ModelConfigTool {
    fn name(&self) -> &'static str {
        "model_config"
    }
    fn description(&self) -> &'static str {
        "View or update LLM model configuration. Op: get (show current config, api_key excluded), set (update provider/model/api_url/max_tokens). Changes take effect after restart."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: get|set"},"provider":{"type":"string","description":"LLM provider name (for set)"},"model":{"type":"string","description":"Model name (for set)"},"api_url":{"type":"string","description":"API base URL (for set)"},"max_tokens":{"type":"integer","description":"Max tokens (for set)"}},"required":["op"]}"#
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_model_config")?;
        let op = obj
            .get("op")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_model_config", "missing op"))?;

        match op {
            "get" => {
                let config_data = self.platform.read_config_file(LLM_CONFIG_PATH)?;
                match config_data {
                    Some(data) => {
                        let text = String::from_utf8_lossy(&data);
                        // Parse JSON and strip api_key for security
                        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&text) {
                            strip_api_keys(&mut v);
                            Ok(json!({"op": "get", "config": v, "note": "api_key fields are hidden for security"}).to_string())
                        } else {
                            Ok(json!({"op": "get", "raw": text.to_string(), "note": "Could not parse as JSON"}).to_string())
                        }
                    }
                    None => Ok(json!({"op": "get", "config": null, "note": "No LLM config file found (using defaults)"}).to_string()),
                }
            }
            "set" => {
                // Read existing config
                let existing = self
                    .platform
                    .read_config_file(LLM_CONFIG_PATH)?
                    .and_then(|d| serde_json::from_slice::<serde_json::Value>(&d).ok())
                    .unwrap_or_else(|| json!({}));

                let mut config = if let Some(obj) = existing.as_object() {
                    obj.clone()
                } else {
                    serde_json::Map::new()
                };

                // Update only provided fields
                let mut updated = Vec::new();
                if let Some(v) = obj.get("provider").and_then(|x| x.as_str()) {
                    config.insert("provider".to_string(), json!(v));
                    updated.push("provider");
                }
                if let Some(v) = obj.get("model").and_then(|x| x.as_str()) {
                    config.insert("model".to_string(), json!(v));
                    updated.push("model");
                }
                if let Some(v) = obj.get("api_url").and_then(|x| x.as_str()) {
                    config.insert("api_url".to_string(), json!(v));
                    updated.push("api_url");
                }
                if let Some(v) = obj.get("max_tokens").and_then(|x| x.as_u64()) {
                    config.insert("max_tokens".to_string(), json!(v));
                    updated.push("max_tokens");
                }

                if updated.is_empty() {
                    return Ok(json!({
                        "op": "set",
                        "ok": false,
                        "error": "no fields to update (provide provider, model, api_url, or max_tokens)"
                    })
                    .to_string());
                }

                let data = serde_json::to_vec_pretty(&json!(config))
                    .map_err(|e| Error::config("tool_model_config", e.to_string()))?;
                self.platform.write_config_file(LLM_CONFIG_PATH, &data)?;

                Ok(json!({
                    "op": "set",
                    "ok": true,
                    "updated_fields": updated,
                    "note": "Restart required for changes to take effect."
                })
                .to_string())
            }
            _ => Err(Error::config(
                "tool_model_config",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::admin()
    }
}

/// Recursively strip api_key fields from JSON value.
fn strip_api_keys(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if key.contains("api_key") || key.contains("secret") {
                    *val = json!("***REDACTED***");
                } else {
                    strip_api_keys(val);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_api_keys(item);
            }
        }
        _ => {}
    }
}
