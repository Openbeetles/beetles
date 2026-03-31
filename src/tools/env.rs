//! Env 工具：环境变量访问（get/list）。

use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext, ToolMetadata};
use serde_json::json;
use std::env;

#[derive(Default)]
pub struct EnvTool;

impl Tool for EnvTool {
    fn name(&self) -> &'static str {
        "env"
    }

    fn description(&self) -> &str {
        "环境变量访问（get: 获取单个变量，list: 列出所有变量）。Args: mode (\"get\" or \"list\"), key (get 模式必需）"
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "description": "get 或 list"
                },
                "key": {
                    "type": "string",
                    "description": "环境变量名称（get 模式必需）"
                }
            },
            "required": ["mode"]
        })
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct EnvArgs {
            mode: String,
            #[serde(default)]
            key: Option<String>,
        }

        let parsed: EnvArgs = serde_json::from_str(args).map_err(|e| Error::Config {
            stage: "env_tool",
            message: format!("invalid args: {}", e),
        })?;

        match parsed.mode.as_str() {
            "get" => {
                let key = parsed.key.ok_or_else(|| Error::Config {
                    stage: "env_tool",
                    message: "key required for get mode".to_string(),
                })?;

                match env::var(&key) {
                    Ok(value) => Ok(json!({
                        "mode": "get",
                        "key": key,
                        "value": value,
                        "found": true
                    })
                    .to_string()),
                    Err(_) => Ok(json!({
                        "mode": "get",
                        "key": key,
                        "found": false
                    })
                    .to_string()),
                }
            }
            "list" => {
                let vars: Vec<(String, String)> = env::vars().collect();
                Ok(json!({
                    "mode": "list",
                    "count": vars.len(),
                    "vars": vars.into_iter().map(|(k, v)| json!({"key": k, "value": v})).collect::<Vec<_>>()
                })
                .to_string())
            }
            _ => Err(Error::Config {
                stage: "env_tool",
                message: format!("invalid mode: {}", parsed.mode),
            }),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::admin()
    }
}
