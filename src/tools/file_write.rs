//! file_write 工具：向状态根写入文件，支持覆写与追加（经 StateFs）。
//! file_write tool: write files under state root via StateFs (overwrite / append).

use crate::constants::FILE_WRITE_MAX_CONTENT_LEN;
use crate::error::{Error, Result};
use crate::tools::state_file_guard::{ensure_state_path_mutable, normalize_state_tool_path};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolClarificationField, ToolContext,
    ToolExecutionBlocker, ToolExecutionOutcome, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::sync::Arc;

pub struct FileWriteTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

#[derive(Serialize)]
struct FileWriteResponse<'a> {
    path: &'a str,
    ok: bool,
    append: bool,
    bytes_written: usize,
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
        "Write content to a file under storage. Supports overwrite and append modes. Protected system files cannot be written. Max final file size: 16KB."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"path":{"type":"string","description":"File path under storage root, e.g. notes/todo.txt"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"If true, append to existing file (default false, overwrite)"}},"required":["path","content"]}"#
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
        let obj = parse_tool_args(args, "tool_file_write")?;
        let Some(path_arg) = obj
            .get("path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(file_write_missing_field_outcome(
                "path",
                "A storage path is still required before file_write can continue.",
            ));
        };
        let Some(content) = obj.get("content").and_then(|x| x.as_str()) else {
            return Ok(file_write_missing_field_outcome(
                "content",
                "File content is still required before file_write can continue.",
            ));
        };
        let append = obj.get("append").and_then(|x| x.as_bool()).unwrap_or(false);

        if content.len() > FILE_WRITE_MAX_CONTENT_LEN {
            return Err(Error::config(
                "tool_file_write",
                format!("content exceeds {} bytes", FILE_WRITE_MAX_CONTENT_LEN),
            ));
        }

        let rel = normalize_state_tool_path(path_arg, "tool_file_write")?;
        ensure_state_path_mutable(&rel, "tool_file_write")?;

        let bytes_written = if append {
            let mut existing = self.state_fs.read_bytes(&rel)?.unwrap_or_default();
            if std::str::from_utf8(existing.as_ref()).is_err() {
                return Err(Error::config(
                    "tool_file_write",
                    "existing file is not valid UTF-8",
                ));
            }
            let final_len = existing
                .len()
                .checked_add(content.len())
                .ok_or_else(|| Error::config("tool_file_write", "final content size overflow"))?;
            if final_len > FILE_WRITE_MAX_CONTENT_LEN {
                return Err(Error::config(
                    "tool_file_write",
                    format!("final content exceeds {} bytes", FILE_WRITE_MAX_CONTENT_LEN),
                ));
            }
            existing
                .write_all(content.as_bytes())
                .map_err(|error| Error::io("tool_file_write", error))?;
            self.state_fs.write(&rel, existing.as_ref())?;
            existing.len()
        } else {
            let bytes = overwrite_content_bytes(content);
            self.state_fs.write(&rel, bytes)?;
            bytes.len()
        };

        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "tool_file_write",
            &FileWriteResponse {
                path: path_arg,
                ok: true,
                append,
                bytes_written,
            },
        )?))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_risk_level(ToolRiskLevel::High)
            .with_rollback_kind(ToolRollbackKind::CompensatingWrite)
    }
}

fn overwrite_content_bytes(content: &str) -> &[u8] {
    content.as_bytes()
}

fn file_write_missing_field_outcome(field: &str, summary: &str) -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "path": if field == "path" { Value::Null } else { Value::String(String::new()) },
            "append": false,
            "warning": format!("file_write: missing {}", field),
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        summary,
        vec![field.to_string()],
        vec![ToolClarificationField {
            key: field.to_string(),
            label: field.to_string(),
            description: format!("Provide {} for file_write.", field),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    ))
}

#[cfg(test)]
mod tests {
    use super::{overwrite_content_bytes, FileWriteTool};
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::{Tool, ToolContext, ToolExecutionBlockerKind};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl StateFs for MockStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.files.lock().unwrap().get(rel_path).cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, _rel_path: &str) -> Result<()> {
            unreachable!()
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
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
    fn overwrite_content_bytes_borrows_content_without_copy() {
        let content = "direct overwrite content";
        let bytes = overwrite_content_bytes(content);

        assert_eq!(bytes, content.as_bytes());
        assert_eq!(bytes.as_ptr(), content.as_bytes().as_ptr());
    }

    #[test]
    fn append_rejects_non_utf8_existing_file() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", &[0xff, 0xfe, b'x']).unwrap();
        let tool = FileWriteTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let err = tool
            .execute(
                r#"{"path":"notes/a.txt","content":"tail","append":true}"#,
                &mut MockToolContext,
            )
            .unwrap_err();

        assert!(format!("{err}").contains("UTF-8"));
    }

    #[test]
    fn append_rejects_when_final_size_exceeds_limit() {
        let fs = Arc::new(MockStateFs::default());
        let existing = vec![b'a'; crate::constants::FILE_WRITE_MAX_CONTENT_LEN - 4];
        fs.write("notes/a.txt", &existing).unwrap();
        let tool = FileWriteTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let err = tool
            .execute(
                r#"{"path":"notes/a.txt","content":"12345","append":true}"#,
                &mut MockToolContext,
            )
            .unwrap_err();

        assert!(format!("{err}").contains("final content exceeds"));
    }

    #[test]
    fn missing_path_returns_facts_blocker() {
        let fs = Arc::new(MockStateFs::default());
        let tool = FileWriteTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(r#"{"content":"hello"}"#, &mut MockToolContext)
            .expect("path blocker");
        let blocker = outcome.blocker.expect("path blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert_eq!(blocker.missing_fields, vec!["path".to_string()]);
    }
}
