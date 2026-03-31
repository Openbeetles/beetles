//! files 工具：在状态根下列出、读取或删除文件；路径禁止 `..`，结果截断至 MAX_TOOL_RESULT_LEN。
//! files tool: list, read, or delete under state root; no `..` in path.

use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata, MAX_TOOL_RESULT_LEN};
use crate::util::normalize_state_rel_path;
use serde_json::json;
use std::sync::Arc;

const MAX_LIST_ENTRIES: usize = 256;
/// read 模式下单文件原始字节上限（UTF-8 校验前），避免超大 Vec。
const MAX_READ_RAW_BYTES: usize = MAX_TOOL_RESULT_LEN * 2;

/// 受保护路径黑名单：禁止通过工具删除的关键文件。
const PROTECTED_PATHS: &[&str] = &[
    "config/llm.json",
    "config/channels.json",
    "config/wifi.json",
    "config/SOUL.md",
    "config/USER.md",
    "memory/MEMORY.md",
];

pub struct FilesTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

impl FilesTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for FilesTool {
    fn name(&self) -> &'static str {
        "files"
    }
    fn description(&self) -> &'static str {
        "List, read, or delete files from storage (SPIFFS). Args: path (string), mode (optional: 'list', 'read', or 'delete', default 'read'). Read returns content truncated to limit; list returns entry names, max 256; delete removes the file."
    }
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path under storage root, e.g. skills/foo.md" },
                "mode": { "type": "string", "description": "list, read, or delete (default read)" }
            },
            "required": ["path"]
        })
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_files")?;
        let path_arg = obj
            .get("path")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_files", "missing path"))?;
        let mode = obj
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("read")
            .trim()
            .to_lowercase();

        let rel = normalize_state_rel_path(path_arg)
            .map_err(|_| Error::config("tool_files", "invalid path"))?;

        if mode == "list" {
            let entries = self.state_fs.list_dir(&rel)?;
            let truncated = entries.len() >= MAX_LIST_ENTRIES;
            let out = json!({
                "mode": "list",
                "path": path_arg,
                "entries": entries,
                "truncated": truncated
            });
            return serde_json::to_string(&out)
                .map_err(|e| Error::config("tool_files", e.to_string()));
        }

        if mode == "delete" {
            if PROTECTED_PATHS.contains(&rel.as_str()) {
                return Err(Error::config("tool_files", "cannot delete protected file"));
            }
            self.state_fs.remove(&rel)?;
            let out = json!({
                "mode": "delete",
                "path": path_arg,
                "success": true
            });
            return serde_json::to_string(&out)
                .map_err(|e| Error::config("tool_files", e.to_string()));
        }

        if mode != "read" {
            return Err(Error::config(
                "tool_files",
                "mode must be 'list', 'read', or 'delete'",
            ));
        }

        match self.state_fs.read(&rel)? {
            Some(raw) => {
                if raw.len() > MAX_READ_RAW_BYTES {
                    return Err(Error::config("tool_files", "file too large"));
                }
                let content = std::str::from_utf8(&raw)
                    .map_err(|_| Error::config("tool_files", "file is not valid UTF-8"))?
                    .to_string();
                let (content, truncated) = if content.len() > MAX_TOOL_RESULT_LEN {
                    let mut c = content
                        .chars()
                        .take(MAX_TOOL_RESULT_LEN)
                        .collect::<String>();
                    c.push('…');
                    (c, true)
                } else {
                    (content, false)
                };
                let out = json!({
                    "mode": "read",
                    "path": path_arg,
                    "content": content,
                    "truncated": truncated
                });
                serde_json::to_string(&out).map_err(|e| Error::config("tool_files", e.to_string()))
            }
            None => match self.state_fs.list_dir(&rel) {
                Ok(_) => Err(Error::config(
                    "tool_files",
                    "path is a directory, use list mode",
                )),
                Err(e) => Err(e),
            },
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}
