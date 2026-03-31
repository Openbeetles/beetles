//! file_write 工具：向状态根写入文件，支持覆写与追加（经 StateFs）。
//! file_write tool: write files under state root via StateFs (overwrite / append).

use crate::constants::FILE_WRITE_MAX_CONTENT_LEN;
use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::util::normalize_state_rel_path;
use serde_json::json;
use std::sync::Arc;

/// 受保护路径黑名单：禁止通过工具写入的关键配置文件。
const PROTECTED_PATHS: &[&str] = &[
    "config/llm.json",
    "config/channels.json",
    "config/wifi.json",
    "config/SOUL.md",
    "config/USER.md",
    "memory/MEMORY.md",
];

pub struct FileWriteTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

impl FileWriteTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for FileWriteTool {
    fn name(&self) -> &'static str {
        "file_write"
    }
    fn description(&self) -> &'static str {
        "Write content to a file in storage (SPIFFS). Supports overwrite and append modes. Protected system files cannot be written. Max content size: 16KB."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path under storage root, e.g. notes/todo.txt" },
                "content": { "type": "string", "description": "Content to write" },
                "append": { "type": "boolean", "description": "If true, append to existing file (default false, overwrite)" }
            },
            "required": ["path", "content"]
        })
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_file_write")?;
        let path_arg = obj
            .get("path")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_file_write", "missing path"))?;
        let content = obj
            .get("content")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_file_write", "missing content"))?;
        let append = obj.get("append").and_then(|x| x.as_bool()).unwrap_or(false);

        if content.len() > FILE_WRITE_MAX_CONTENT_LEN {
            return Err(Error::config(
                "tool_file_write",
                format!("content exceeds {} bytes", FILE_WRITE_MAX_CONTENT_LEN),
            ));
        }

        let normalized = path_arg.trim().trim_start_matches('/');
        for &protected in PROTECTED_PATHS {
            if normalized == protected {
                return Err(Error::config(
                    "tool_file_write",
                    format!(
                        "path '{}' is protected and cannot be written via this tool",
                        protected
                    ),
                ));
            }
        }

        let rel = normalize_state_rel_path(path_arg)
            .map_err(|_| Error::config("tool_file_write", "invalid path"))?;

        let final_bytes = if append {
            let existing = self
                .state_fs
                .read(&rel)?
                .map(|v| String::from_utf8_lossy(&v).into_owned())
                .unwrap_or_default();
            format!("{}{}", existing, content).into_bytes()
        } else {
            content.as_bytes().to_vec()
        };

        self.state_fs.write(&rel, &final_bytes)?;

        Ok(json!({
            "path": path_arg,
            "ok": true,
            "append": append,
            "bytes_written": content.len()
        })
        .to_string())
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}
