//! files 工具：在状态根下列出、读取或删除文件；路径禁止 `..`，结果截断至 MAX_TOOL_RESULT_LEN。
//! files tool: list, read, or delete under state root; no `..` in path.

use crate::error::{Error, Result};
use crate::tools::state_file_guard::{ensure_state_path_mutable, normalize_state_tool_path};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolContext, ToolEffectClass,
    ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind, MAX_TOOL_RESULT_LEN,
};
use serde::Serialize;
use std::sync::Arc;

const MAX_LIST_ENTRIES: usize = 256;
/// read 模式下单文件原始字节上限（UTF-8 校验前），避免超大 Vec。
const MAX_READ_RAW_BYTES: usize = MAX_TOOL_RESULT_LEN * 2;

pub struct FilesTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

#[derive(Serialize)]
struct FilesListResponse<'a> {
    mode: &'static str,
    path: &'a str,
    entries: Vec<String>,
    truncated: bool,
}

#[derive(Serialize)]
struct FilesDeleteResponse<'a> {
    mode: &'static str,
    path: &'a str,
    success: bool,
}

#[derive(Serialize)]
struct FilesReadResponse<'a> {
    mode: &'static str,
    path: &'a str,
    content: String,
    truncated: bool,
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
        "List, read, or delete files under storage. Args: path (string), mode (optional: 'list', 'read', or 'delete', default 'read'). Read returns content truncated to limit; list returns entry names, max 256; delete removes the file."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"path":{"type":"string","description":"Path under storage root, e.g. skills/foo.md"},"mode":{"type":"string","description":"list, read, or delete (default read)"}},"required":["path"]}"#
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

        let rel = normalize_state_tool_path(path_arg, "tool_files")?;

        if mode == "list" {
            let mut entries = self.state_fs.list_dir(&rel)?;
            let truncated = entries.len() > MAX_LIST_ENTRIES;
            if truncated {
                entries.truncate(MAX_LIST_ENTRIES);
            }
            return serialize_tool_output(
                "tool_files",
                &FilesListResponse {
                    mode: "list",
                    path: path_arg,
                    entries,
                    truncated,
                },
            );
        }

        if mode == "delete" {
            ensure_state_path_mutable(&rel, "tool_files")?;
            self.state_fs.remove(&rel)?;
            return serialize_tool_output(
                "tool_files",
                &FilesDeleteResponse {
                    mode: "delete",
                    path: path_arg,
                    success: true,
                },
            );
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
                serialize_tool_output(
                    "tool_files",
                    &FilesReadResponse {
                        mode: "read",
                        path: path_arg,
                        content,
                        truncated,
                    },
                )
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
            .with_risk_level(ToolRiskLevel::High)
            .with_rollback_kind(ToolRollbackKind::Irreversible)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_files_governance")?;
        let mode = obj
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("read")
            .trim()
            .to_ascii_lowercase();
        let shape = match mode.as_str() {
            "list" | "read" => self
                .metadata()
                .default_execution_shape("files_read")
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_rollback_kind(ToolRollbackKind::None),
            "delete" => self
                .metadata()
                .default_execution_shape("files_delete")
                .with_effect_class(ToolEffectClass::PersistentStateWrite)
                .with_risk_level(ToolRiskLevel::High)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(true)
                .with_rollback_kind(ToolRollbackKind::Irreversible),
            _ => self.metadata().default_execution_shape("files_unknown"),
        };
        Ok(shape)
    }
}

#[cfg(test)]
mod tests {
    use super::FilesTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::{Tool, ToolContext};
    use serde_json::Value;
    use std::sync::Arc;

    struct MockStateFs {
        entries: Vec<String>,
    }

    impl StateFs for MockStateFs {
        fn read(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }

        fn write(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
            unreachable!()
        }

        fn remove(&self, _rel_path: &str) -> Result<()> {
            unreachable!()
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(self.entries.clone())
        }
    }

    struct MockToolContext;

    impl ToolContext for MockToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            unreachable!()
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn list_mode_truncates_to_declared_limit() {
        let entries = (0..300).map(|i| format!("entry-{i:03}.txt")).collect();
        let tool = FilesTool::new(Arc::new(MockStateFs { entries }));

        let result = tool
            .execute(r#"{"path":"notes","mode":"list"}"#, &mut MockToolContext)
            .unwrap();
        let value: Value = serde_json::from_str(&result).unwrap();
        let items = value["entries"].as_array().unwrap();

        assert_eq!(items.len(), 256);
        assert_eq!(items[0].as_str(), Some("entry-000.txt"));
        assert_eq!(items[255].as_str(), Some("entry-255.txt"));
        assert_eq!(value["truncated"].as_bool(), Some(true));
    }
}
