//! Network 工具：仅 Linux，ping 和 curl 功能。

use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext};
use serde_json::Value;
use std::process::Command;

const PING_MAX_COUNT: u32 = 10;
const CURL_TIMEOUT_SECS: u32 = 30;

#[derive(Default)]
pub struct NetworkTool;

impl Tool for NetworkTool {
    fn name(&self) -> &'static str {
        "network"
    }

    fn description(&self) -> &str {
        "网络工具（ping: 测试连通性，curl: HTTP 请求）。Args: mode (\"ping\" or \"curl\"), host/url, count (ping 可选，默认 4), method (curl 可选，默认 GET)"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "description": "ping 或 curl"
                },
                "host": {
                    "type": "string",
                    "description": "ping 的目标主机"
                },
                "url": {
                    "type": "string",
                    "description": "curl 的目标 URL"
                },
                "count": {
                    "type": "integer",
                    "description": "ping 次数（默认 4，最大 10）"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP 方法（GET, POST 等，默认 GET）"
                }
            },
            "required": ["mode"]
        })
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct NetworkArgs {
            mode: String,
            #[serde(default)]
            host: Option<String>,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            count: Option<u32>,
            #[serde(default)]
            method: Option<String>,
        }

        let parsed: NetworkArgs = serde_json::from_str(args).map_err(|e| Error::Config {
            stage: "network_tool",
            message: format!("invalid args: {}", e),
        })?;

        match parsed.mode.as_str() {
            "ping" => {
                let host = parsed.host.ok_or_else(|| Error::Config {
                    stage: "network_tool",
                    message: "host required for ping mode".to_string(),
                })?;

                let count = parsed.count.unwrap_or(4).min(PING_MAX_COUNT);

                let output = Command::new("ping")
                    .args(["-c", &count.to_string(), &host])
                    .output()
                    .map_err(|e| Error::Other {
                        source: Box::new(e),
                        stage: "network_ping",
                    })?;

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(stdout.to_string())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(Error::Config {
                        stage: "network_ping",
                        message: format!("ping failed: {}", stderr),
                    })
                }
            }
            "curl" => {
                let url = parsed.url.ok_or_else(|| Error::Config {
                    stage: "network_tool",
                    message: "url required for curl mode".to_string(),
                })?;

                let method = parsed.method.as_deref().unwrap_or("GET");

                let output = Command::new("curl")
                    .args([
                        "-X",
                        method,
                        "--max-time",
                        &CURL_TIMEOUT_SECS.to_string(),
                        "-i",
                        &url,
                    ])
                    .output()
                    .map_err(|e| Error::Other {
                        source: Box::new(e),
                        stage: "network_curl",
                    })?;

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(stdout.to_string())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(Error::Config {
                        stage: "network_curl",
                        message: format!("curl failed: {}", stderr),
                    })
                }
            }
            _ => Err(Error::Config {
                stage: "network_tool",
                message: format!("invalid mode: {}", parsed.mode),
            }),
        }
    }

    fn requires_network(&self) -> bool {
        true
    }
}
