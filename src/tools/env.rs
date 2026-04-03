//! Env 工具：环境变量访问（get/list）。

use crate::error::{Error, Result};
use crate::tools::{serialize_tool_output, Tool, ToolContext, ToolMetadata};
use serde::Serialize;
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

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"mode":{"type":"string","description":"get 或 list"},"key":{"type":"string","description":"环境变量名称（get 模式必需）"}},"required":["mode"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct EnvArgs {
            mode: String,
            #[serde(default)]
            key: Option<String>,
        }

        #[derive(Serialize)]
        struct EnvVarEntry {
            key: String,
            value: String,
        }

        #[derive(Serialize)]
        struct EnvGetResponse {
            mode: &'static str,
            key: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            value: Option<String>,
            found: bool,
        }

        #[derive(Serialize)]
        struct EnvListResponse {
            mode: &'static str,
            count: usize,
            vars: Vec<EnvVarEntry>,
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
                    Ok(value) => serialize_tool_output(
                        "env_tool",
                        &EnvGetResponse {
                            mode: "get",
                            key,
                            value: Some(value),
                            found: true,
                        },
                    ),
                    Err(_) => serialize_tool_output(
                        "env_tool",
                        &EnvGetResponse {
                            mode: "get",
                            key,
                            value: None,
                            found: false,
                        },
                    ),
                }
            }
            "list" => {
                let vars = env::vars()
                    .map(|(key, value)| EnvVarEntry { key, value })
                    .collect::<Vec<_>>();
                serialize_tool_output(
                    "env_tool",
                    &EnvListResponse {
                        mode: "list",
                        count: vars.len(),
                        vars,
                    },
                )
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
