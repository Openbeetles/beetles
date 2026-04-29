//! files 工具：在状态根下列出、读取或删除文件；路径禁止 `..`，结果截断至 MAX_TOOL_RESULT_LEN。
//! files tool: list, read, or delete under state root; no `..` in path.

use crate::error::{Error, Result};
use crate::tools::state_file_guard::{
    ensure_state_path_mutable, normalize_state_tool_path, sanitize_state_file_read,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolCapabilityContract,
    ToolClarificationField, ToolClarificationOption, ToolContext, ToolEffectClass,
    ToolExecutionBlocker, ToolExecutionOutcome, ToolExecutionShape, ToolMetadata, ToolRiskLevel,
    ToolRollbackKind, MAX_TOOL_RESULT_LEN,
};
use serde::Serialize;
use serde_json::{json, Value};
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
        "List, read, or delete files under storage. Args: path (string), mode (optional: 'list', 'read', or 'delete', default 'read'). Read returns content truncated to limit; when path is a directory, read falls back to list. Sensitive config values are automatically redacted in read output."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"path":{"type":"string","description":"Path under storage root, e.g. skills/foo.md"},"mode":{"type":"string","description":"list, read, or delete (default read)"}},"required":["path"]}"#
    }
    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        self.execute_outcome(args, ctx)
            .map(|outcome| outcome.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        _ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_files")?;
        let Some(path_arg) = obj
            .get("path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return missing_path_outcome();
        };
        let mode = obj
            .get("mode")
            .and_then(|x| x.as_str())
            .unwrap_or("read")
            .trim()
            .to_lowercase();

        let rel = normalize_state_tool_path(path_arg, "tool_files")?;

        if mode == "list" {
            let mut entries = match self.state_fs.list_dir(&rel) {
                Ok(entries) => entries,
                Err(error) if is_not_found_error(&error) => {
                    return missing_target_outcome(path_arg, "list");
                }
                Err(error) => return Err(error),
            };
            let truncated = entries.len() > MAX_LIST_ENTRIES;
            if truncated {
                entries.truncate(MAX_LIST_ENTRIES);
            }
            return Ok(ToolExecutionOutcome::text(serialize_tool_output(
                "tool_files",
                &FilesListResponse {
                    mode: "list",
                    path: path_arg,
                    entries,
                    truncated,
                },
            )?));
        }

        if mode == "delete" {
            if !state_path_exists(self.state_fs.as_ref(), &rel)? {
                return missing_target_outcome(path_arg, "delete");
            }
            ensure_state_path_mutable(&rel, "tool_files")?;
            self.state_fs.remove(&rel)?;
            return Ok(ToolExecutionOutcome::text(serialize_tool_output(
                "tool_files",
                &FilesDeleteResponse {
                    mode: "delete",
                    path: path_arg,
                    success: true,
                },
            )?));
        }

        if mode != "read" {
            return invalid_mode_outcome();
        }

        match self.state_fs.read_bytes(&rel)? {
            Some(raw) => {
                if raw.len() > MAX_READ_RAW_BYTES {
                    return Err(Error::config("tool_files", "file too large"));
                }
                let sanitized = sanitize_state_file_read(&rel, raw.as_ref(), "tool_files")?;
                let content = std::str::from_utf8(sanitized.as_ref())
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
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_files",
                    &FilesReadResponse {
                        mode: "read",
                        path: path_arg,
                        content,
                        truncated,
                    },
                )?))
            }
            None => match self.state_fs.list_dir(&rel) {
                Ok(mut entries) => {
                    let truncated = entries.len() > MAX_LIST_ENTRIES;
                    if truncated {
                        entries.truncate(MAX_LIST_ENTRIES);
                    }
                    Ok(ToolExecutionOutcome::text(serialize_tool_output(
                        "tool_files",
                        &FilesListResponse {
                            mode: "list",
                            path: path_arg,
                            entries,
                            truncated,
                        },
                    )?))
                }
                Err(error) if is_not_found_error(&error) => {
                    missing_target_outcome(path_arg, "read")
                }
                Err(error) => Err(error),
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
                .with_effect_class(ToolEffectClass::StorageWrite)
                .with_risk_level(ToolRiskLevel::High)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(true)
                .with_rollback_kind(ToolRollbackKind::Irreversible),
            _ => self.metadata().default_execution_shape("files_unknown"),
        };
        Ok(shape)
    }

    fn governance_examples(&self) -> &'static [&'static str] {
        &[
            r#"{"path":"skills","mode":"list"}"#,
            r#"{"path":"skills/demo.md","mode":"read"}"#,
            r#"{"path":"skills/demo.md","mode":"delete"}"#,
        ]
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[
            crate::orchestrator::RUNTIME_CAPABILITY_STORAGE_STATE_FS,
        ])
    }
}

fn state_path_exists(state_fs: &(dyn crate::StateFs + Send + Sync), rel: &str) -> Result<bool> {
    if state_fs.exists(rel)? {
        return Ok(true);
    }
    match state_fs.list_dir(rel) {
        Ok(_) => Ok(true),
        Err(error) if is_not_found_error(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_not_found_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Io {
            source,
            ..
        } if source.kind() == std::io::ErrorKind::NotFound
    )
}

fn missing_path_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        json!({
            "mode": Value::Null,
            "path": Value::Null,
            "warning": "files: missing path",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "A storage path is still required before files can continue.",
        vec!["path".to_string()],
        vec![ToolClarificationField {
            key: "path".to_string(),
            label: "Path".to_string(),
            description: "Provide a path under the storage root, such as notes/todo.txt."
                .to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    )))
}

fn invalid_mode_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        json!({
            "mode": Value::Null,
            "path": Value::Null,
            "warning": "files: mode must be list, read, or delete",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "A valid files mode is still required before this tool can continue.",
        vec!["mode".to_string()],
        vec![ToolClarificationField {
            key: "mode".to_string(),
            label: "Mode".to_string(),
            description: "Choose how to interact with the path.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![
                ToolClarificationOption {
                    value: "list".to_string(),
                    label: "list".to_string(),
                },
                ToolClarificationOption {
                    value: "read".to_string(),
                    label: "read".to_string(),
                },
                ToolClarificationOption {
                    value: "delete".to_string(),
                    label: "delete".to_string(),
                },
            ],
        }],
    )))
}

fn missing_target_outcome(path: &str, mode: &str) -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        json!({
            "mode": mode,
            "path": path,
            "warning": "files: path not found",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "The requested path does not exist; a valid file or directory path is still required.",
        vec!["path".to_string()],
        vec![ToolClarificationField {
            key: "path".to_string(),
            label: "Path".to_string(),
            description: "Provide a valid existing path under the storage root.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    )))
}

#[cfg(test)]
mod tests {
    use super::FilesTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::{Tool, ToolContext, ToolExecutionBlockerKind};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;

    struct MockStateFs {
        files: HashMap<String, Vec<u8>>,
        dirs: HashMap<String, Vec<String>>,
    }

    impl StateFs for MockStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.files.get(rel_path).cloned())
        }

        fn write(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
            unreachable!()
        }

        fn remove(&self, _rel_path: &str) -> Result<()> {
            unreachable!()
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            self.dirs.get(rel_path).cloned().ok_or_else(|| {
                crate::Error::io(
                    "mock_state_fs",
                    std::io::Error::new(std::io::ErrorKind::NotFound, "missing directory"),
                )
            })
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
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::new(),
            dirs: HashMap::from([("notes".to_string(), entries)]),
        }));

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

    #[test]
    fn read_mode_falls_back_to_directory_listing() {
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::new(),
            dirs: HashMap::from([(
                "skills".to_string(),
                vec!["a.md".to_string(), "b.md".to_string()],
            )]),
        }));

        let result = tool
            .execute(r#"{"path":"skills","mode":"read"}"#, &mut MockToolContext)
            .unwrap();
        let value: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(value["mode"].as_str(), Some("list"));
        assert_eq!(value["path"].as_str(), Some("skills"));
        assert_eq!(value["entries"][0].as_str(), Some("a.md"));
        assert_eq!(value["entries"][1].as_str(), Some("b.md"));
    }

    #[test]
    fn read_mode_redacts_sensitive_config_values() {
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::from([(
                "config/channels.json".to_string(),
                br#"{"tg_token":"123456:live-secret","feishu_app_secret":"fs-secret-value","enabled_channel":"telegram"}"#.to_vec(),
            )]),
            dirs: HashMap::new(),
        }));

        let result = tool
            .execute(
                r#"{"path":"config/channels.json","mode":"read"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let value: Value = serde_json::from_str(&result).unwrap();
        let content = value["content"].as_str().unwrap();

        assert!(content.contains("[REDACTED]"));
        assert!(content.contains("\"enabled_channel\":\"telegram\""));
        assert!(!content.contains("123456:live-secret"));
        assert!(!content.contains("fs-secret-value"));
    }

    #[test]
    fn files_missing_path_returns_facts_blocker() {
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::new(),
            dirs: HashMap::new(),
        }));

        let outcome = tool
            .execute_outcome(r#"{}"#, &mut MockToolContext)
            .expect("missing path should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker.missing_fields.iter().any(|item| item == "path"));
    }

    #[test]
    fn files_invalid_mode_returns_choice_blocker() {
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::new(),
            dirs: HashMap::new(),
        }));

        let outcome = tool
            .execute_outcome(r#"{"path":"notes","mode":"move"}"#, &mut MockToolContext)
            .expect("invalid mode should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn files_missing_target_returns_facts_blocker() {
        let tool = FilesTool::new(Arc::new(MockStateFs {
            files: HashMap::new(),
            dirs: HashMap::new(),
        }));

        let outcome = tool
            .execute_outcome(
                r#"{"path":"notes/missing.txt","mode":"read"}"#,
                &mut MockToolContext,
            )
            .expect("missing target should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker.missing_fields.iter().any(|item| item == "path"));
    }
}
