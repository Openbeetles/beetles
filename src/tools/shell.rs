//! Shell 工具：仅 Linux，白名单命令。

use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext, ToolMetadata};
use serde_json::Value;
use std::process::Command;

/// 允许执行的命令白名单
const ALLOWED_COMMANDS: &[&str] = &[
    "ls", "cat", "grep", "ps", "df", "free", "uptime", "whoami", "pwd", "date", "uname",
];

#[derive(Default)]
pub struct ShellTool;

impl ShellTool {
    fn is_command_allowed(cmd: &str) -> bool {
        ALLOWED_COMMANDS.contains(&cmd)
    }
}

impl Tool for ShellTool {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn description(&self) -> &str {
        "执行 shell 命令（仅限白名单命令：ls, cat, grep, ps, df, free, uptime, whoami, pwd, date, uname）"
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "要执行的命令（仅限白名单）"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "命令参数"
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        #[derive(serde::Deserialize)]
        struct ShellArgs {
            command: String,
            #[serde(default)]
            args: Vec<String>,
        }

        let parsed: ShellArgs = serde_json::from_str(args).map_err(|e| Error::Config {
            stage: "shell_tool",
            message: format!("invalid args: {}", e),
        })?;

        if !Self::is_command_allowed(&parsed.command) {
            return Err(Error::Config {
                stage: "shell_tool",
                message: format!("command '{}' not in whitelist", parsed.command),
            });
        }

        let output = Command::new(&parsed.command)
            .args(&parsed.args)
            .output()
            .map_err(|e| Error::Other {
                source: Box::new(e),
                stage: "shell_execute",
            })?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(Error::Config {
                stage: "shell_tool",
                message: format!("command failed: {}", stderr),
            })
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::debug()
    }
}
