//! Process 工具：仅 Linux，列出或终止进程。

use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext};
use serde_json::Value;
use std::process::Command;

pub struct ProcessTool;

impl ProcessTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for ProcessTool {
    fn name(&self) -> &'static str {
        "process"
    }

    fn description(&self) -> &str {
        "进程管理（list: 列出进程，kill: 终止进程）。Args: mode (\"list\" or \"kill\"), pid (仅 kill 模式), signal (可选，默认 TERM)"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "description": "list 或 kill"
                },
                "pid": {
                    "type": "integer",
                    "description": "进程 ID（kill 模式必需）"
                },
                "signal": {
                    "type": "string",
                    "description": "信号名称（TERM, KILL 等，默认 TERM）"
                }
            },
            "required": ["mode"]
        })
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct ProcessArgs {
            mode: String,
            #[serde(default)]
            pid: Option<i32>,
            #[serde(default)]
            signal: Option<String>,
        }

        let parsed: ProcessArgs = serde_json::from_str(args).map_err(|e| Error::Config {
            stage: "process_tool",
            message: format!("invalid args: {}", e),
        })?;

        match parsed.mode.as_str() {
            "list" => {
                let output =
                    Command::new("ps")
                        .args(&["aux"])
                        .output()
                        .map_err(|e| Error::Other {
                            source: Box::new(e),
                            stage: "process_list",
                        })?;

                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    Ok(stdout.to_string())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(Error::Config {
                        stage: "process_list",
                        message: format!("ps failed: {}", stderr),
                    })
                }
            }
            "kill" => {
                let pid = parsed.pid.ok_or_else(|| Error::Config {
                    stage: "process_tool",
                    message: "pid required for kill mode".to_string(),
                })?;

                let signal = parsed.signal.as_deref().unwrap_or("TERM");
                let signal_arg = format!("-{}", signal);

                let output = Command::new("kill")
                    .args(&[signal_arg.as_str(), &pid.to_string()])
                    .output()
                    .map_err(|e| Error::Other {
                        source: Box::new(e),
                        stage: "process_kill",
                    })?;

                if output.status.success() {
                    Ok(format!("Process {} killed with signal {}", pid, signal))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(Error::Config {
                        stage: "process_kill",
                        message: format!("kill failed: {}", stderr),
                    })
                }
            }
            _ => Err(Error::Config {
                stage: "process_tool",
                message: format!("invalid mode: {}", parsed.mode),
            }),
        }
    }
}
