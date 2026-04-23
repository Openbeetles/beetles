//! file_edit 工具：对状态根中的文本文件执行确定性局部改写。
//! file_edit tool: perform deterministic localized edits on text files under storage.

use crate::constants::FILE_WRITE_MAX_CONTENT_LEN;
use crate::error::{Error, Result};
use crate::tools::state_file_guard::{ensure_state_path_mutable, normalize_state_tool_path};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolClarificationField, ToolClarificationOption,
    ToolContext, ToolExecutionBlocker, ToolExecutionOutcome, ToolMetadata, ToolRiskLevel,
    ToolRollbackKind,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_EDIT_FILE_BYTES: usize = 64 * 1024;

pub struct FileEditTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

#[derive(Serialize)]
struct FileEditResponse<'a> {
    path: &'a str,
    ok: bool,
    mode: &'a str,
    match_count: usize,
    bytes_written: usize,
}

impl FileEditTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for FileEditTool {
    fn name(&self) -> &'static str {
        "file_edit"
    }

    fn description(&self) -> &'static str {
        "Edit an existing text file in storage using deterministic operations: replace_once, replace_all, insert_before, insert_after, or prepend. Use this when a localized file change is needed instead of rewriting the whole file."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"path":{"type":"string","description":"File path under storage root, e.g. notes/todo.txt"},"mode":{"type":"string","enum":["replace_once","replace_all","insert_before","insert_after","prepend"],"description":"Edit operation to perform"},"match_text":{"type":"string","description":"Exact text to locate for replace/insert modes"},"content":{"type":"string","description":"Replacement or inserted content"}},"required":["path","mode","content"]}"#
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
        let obj = parse_tool_args(args, "tool_file_edit")?;
        let Some(path_arg) = obj
            .get("path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return missing_field_outcome(
                "path",
                "A file path is still required before file_edit can continue.",
            );
        };
        let Some(mode) = obj
            .get("mode")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return missing_field_outcome(
                "mode",
                "An edit mode is still required before file_edit can continue.",
            );
        };
        let Some(content) = obj.get("content").and_then(|x| x.as_str()) else {
            return missing_field_outcome(
                "content",
                "Edit content is still required before file_edit can continue.",
            );
        };

        if content.len() > FILE_WRITE_MAX_CONTENT_LEN {
            return Err(Error::config(
                "tool_file_edit",
                format!("content exceeds {} bytes", FILE_WRITE_MAX_CONTENT_LEN),
            ));
        }

        let rel = normalize_state_tool_path(path_arg, "tool_file_edit")?;
        ensure_state_path_mutable(&rel, "tool_file_edit")?;

        let Some(raw) = self.state_fs.read(&rel)? else {
            return missing_field_outcome(
                "path",
                "The target file does not exist; a valid file path is still required.",
            );
        };
        if raw.len() > MAX_EDIT_FILE_BYTES {
            return Err(Error::config("tool_file_edit", "file too large"));
        }
        let current = std::str::from_utf8(&raw)
            .map_err(|_| Error::config("tool_file_edit", "file is not valid UTF-8"))?;

        let edit = match mode {
            "replace_once" => {
                let Some(match_text) = parse_match_text(&obj) else {
                    return missing_field_outcome(
                        "match_text",
                        "match_text is still required for this edit mode.",
                    );
                };
                match apply_replace_once(current, match_text, content) {
                    Ok(edit) => edit,
                    Err(error) if match_text_not_found(&error) => {
                        return match_text_not_found_outcome();
                    }
                    Err(error) => return Err(error),
                }
            }
            "replace_all" => {
                let Some(match_text) = parse_match_text(&obj) else {
                    return missing_field_outcome(
                        "match_text",
                        "match_text is still required for this edit mode.",
                    );
                };
                match apply_replace_all(current, match_text, content) {
                    Ok(edit) => edit,
                    Err(error) if match_text_not_found(&error) => {
                        return match_text_not_found_outcome();
                    }
                    Err(error) => return Err(error),
                }
            }
            "insert_before" => {
                let Some(match_text) = parse_match_text(&obj) else {
                    return missing_field_outcome(
                        "match_text",
                        "match_text is still required for this edit mode.",
                    );
                };
                match apply_insert_before(current, match_text, content) {
                    Ok(edit) => edit,
                    Err(error) if match_text_not_found(&error) => {
                        return match_text_not_found_outcome();
                    }
                    Err(error) => return Err(error),
                }
            }
            "insert_after" => {
                let Some(match_text) = parse_match_text(&obj) else {
                    return missing_field_outcome(
                        "match_text",
                        "match_text is still required for this edit mode.",
                    );
                };
                match apply_insert_after(current, match_text, content) {
                    Ok(edit) => edit,
                    Err(error) if match_text_not_found(&error) => {
                        return match_text_not_found_outcome();
                    }
                    Err(error) => return Err(error),
                }
            }
            "prepend" => apply_prepend(current, content),
            _ => return invalid_mode_outcome(),
        };

        self.state_fs.write(&rel, edit.content.as_bytes())?;

        Ok(ToolExecutionOutcome::text(serialize_tool_output(
            "tool_file_edit",
            &FileEditResponse {
                path: path_arg,
                ok: true,
                mode,
                match_count: edit.match_count,
                bytes_written: edit.content.len(),
            },
        )?))
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_risk_level(ToolRiskLevel::High)
            .with_rollback_kind(ToolRollbackKind::CompensatingWrite)
    }
}

struct AppliedEdit {
    content: String,
    match_count: usize,
}

fn parse_match_text(obj: &serde_json::Map<String, serde_json::Value>) -> Option<&str> {
    obj.get("match_text")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

fn apply_replace_once(current: &str, match_text: &str, replacement: &str) -> Result<AppliedEdit> {
    let Some(pos) = current.find(match_text) else {
        return Err(Error::config("tool_file_edit", "match_text not found"));
    };
    let mut out = String::with_capacity(current.len() - match_text.len() + replacement.len());
    out.push_str(&current[..pos]);
    out.push_str(replacement);
    out.push_str(&current[pos + match_text.len()..]);
    Ok(AppliedEdit {
        content: out,
        match_count: 1,
    })
}

fn apply_replace_all(current: &str, match_text: &str, replacement: &str) -> Result<AppliedEdit> {
    let count = current.matches(match_text).count();
    if count == 0 {
        return Err(Error::config("tool_file_edit", "match_text not found"));
    }
    Ok(AppliedEdit {
        content: current.replace(match_text, replacement),
        match_count: count,
    })
}

fn apply_insert_before(current: &str, match_text: &str, content: &str) -> Result<AppliedEdit> {
    let Some(pos) = current.find(match_text) else {
        return Err(Error::config("tool_file_edit", "match_text not found"));
    };
    let mut out = String::with_capacity(current.len() + content.len());
    out.push_str(&current[..pos]);
    out.push_str(content);
    out.push_str(&current[pos..]);
    Ok(AppliedEdit {
        content: out,
        match_count: 1,
    })
}

fn apply_insert_after(current: &str, match_text: &str, content: &str) -> Result<AppliedEdit> {
    let Some(pos) = current.find(match_text) else {
        return Err(Error::config("tool_file_edit", "match_text not found"));
    };
    let end = pos + match_text.len();
    let mut out = String::with_capacity(current.len() + content.len());
    out.push_str(&current[..end]);
    out.push_str(content);
    out.push_str(&current[end..]);
    Ok(AppliedEdit {
        content: out,
        match_count: 1,
    })
}

fn apply_prepend(current: &str, content: &str) -> AppliedEdit {
    let mut out = String::with_capacity(current.len() + content.len());
    out.push_str(content);
    out.push_str(current);
    AppliedEdit {
        content: out,
        match_count: 0,
    }
}

fn match_text_not_found(error: &Error) -> bool {
    matches!(error, Error::Config { message, .. } if message == "match_text not found")
}

fn missing_field_outcome(field: &str, summary: &str) -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "path": Value::Null,
            "mode": Value::Null,
            "warning": format!("file_edit: missing {}", field),
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        summary,
        vec![field.to_string()],
        vec![ToolClarificationField {
            key: field.to_string(),
            label: field.to_string(),
            description: format!("Provide {} for file_edit.", field),
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
            "ok": false,
            "warning": "file_edit: unsupported mode",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "A valid file_edit mode is still required before this tool can continue.",
        vec!["mode".to_string()],
        vec![ToolClarificationField {
            key: "mode".to_string(),
            label: "Mode".to_string(),
            description: "Choose which deterministic file edit to perform.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![
                ToolClarificationOption {
                    value: "replace_once".to_string(),
                    label: "replace_once".to_string(),
                },
                ToolClarificationOption {
                    value: "replace_all".to_string(),
                    label: "replace_all".to_string(),
                },
                ToolClarificationOption {
                    value: "insert_before".to_string(),
                    label: "insert_before".to_string(),
                },
                ToolClarificationOption {
                    value: "insert_after".to_string(),
                    label: "insert_after".to_string(),
                },
                ToolClarificationOption {
                    value: "prepend".to_string(),
                    label: "prepend".to_string(),
                },
            ],
        }],
    )))
}

fn match_text_not_found_outcome() -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "warning": "file_edit: match_text not found",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "match_text was not found in the current file; a valid match_text is still required.",
        vec!["match_text".to_string()],
        vec![ToolClarificationField {
            key: "match_text".to_string(),
            label: "Match text".to_string(),
            description: "Provide text that already exists in the target file.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    )))
}

#[cfg(test)]
mod tests {
    use super::FileEditTool;
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

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files.lock().unwrap().remove(rel_path);
            Ok(())
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
    fn replace_once_updates_first_match_only() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"alpha beta beta").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let result = tool
            .execute(
                r#"{"path":"notes/a.txt","mode":"replace_once","match_text":"beta","content":"B"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        assert!(result.contains("\"match_count\":1"));
        assert_eq!(
            String::from_utf8(fs.read("notes/a.txt").unwrap().unwrap()).unwrap(),
            "alpha B beta"
        );
    }

    #[test]
    fn replace_all_updates_every_match() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"beta beta beta").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let result = tool
            .execute(
                r#"{"path":"notes/a.txt","mode":"replace_all","match_text":"beta","content":"B"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        assert!(result.contains("\"match_count\":3"));
        assert_eq!(
            String::from_utf8(fs.read("notes/a.txt").unwrap().unwrap()).unwrap(),
            "B B B"
        );
    }

    #[test]
    fn insert_before_and_after_work() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"hello world").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        tool.execute(
            r#"{"path":"notes/a.txt","mode":"insert_before","match_text":"world","content":"big "}"#,
            &mut MockToolContext,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(fs.read("notes/a.txt").unwrap().unwrap()).unwrap(),
            "hello big world"
        );

        tool.execute(
            r#"{"path":"notes/a.txt","mode":"insert_after","match_text":"hello","content":" there"}"#,
            &mut MockToolContext,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(fs.read("notes/a.txt").unwrap().unwrap()).unwrap(),
            "hello there big world"
        );
    }

    #[test]
    fn prepend_adds_prefix() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"world").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        tool.execute(
            r#"{"path":"notes/a.txt","mode":"prepend","content":"hello "}"#,
            &mut MockToolContext,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(fs.read("notes/a.txt").unwrap().unwrap()).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn protected_paths_are_rejected() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("config/channels.json", br#"{"enabled_channel":"telegram"}"#)
            .unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let err = tool
            .execute(
                r#"{"path":"config/channels.json","mode":"prepend","content":"x"}"#,
                &mut MockToolContext,
            )
            .unwrap_err();
        assert!(format!("{err}").contains("protected"));
    }

    #[test]
    fn file_edit_missing_path_returns_facts_blocker() {
        let fs = Arc::new(MockStateFs::default());
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(
                r#"{"mode":"prepend","content":"hello "}"#,
                &mut MockToolContext,
            )
            .expect("missing path should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker.missing_fields.iter().any(|item| item == "path"));
    }

    #[test]
    fn file_edit_invalid_mode_returns_choice_blocker() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"hello").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(
                r#"{"path":"notes/a.txt","mode":"append","content":" world"}"#,
                &mut MockToolContext,
            )
            .expect("invalid mode should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
    }

    #[test]
    fn file_edit_missing_match_text_returns_facts_blocker() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"hello").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(
                r#"{"path":"notes/a.txt","mode":"replace_once","content":"world"}"#,
                &mut MockToolContext,
            )
            .expect("missing match_text should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker
            .missing_fields
            .iter()
            .any(|item| item == "match_text"));
    }

    #[test]
    fn file_edit_file_not_found_returns_facts_blocker() {
        let fs = Arc::new(MockStateFs::default());
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(
                r#"{"path":"notes/a.txt","mode":"prepend","content":"hello "}"#,
                &mut MockToolContext,
            )
            .expect("missing file should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker.missing_fields.iter().any(|item| item == "path"));
    }

    #[test]
    fn file_edit_missing_match_target_returns_facts_blocker() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("notes/a.txt", b"hello world").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let outcome = tool
            .execute_outcome(
                r#"{"path":"notes/a.txt","mode":"replace_once","match_text":"beta","content":"B"}"#,
                &mut MockToolContext,
            )
            .expect("missing match target should return blocker");
        let blocker = outcome.blocker.as_ref().expect("blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert!(blocker
            .missing_fields
            .iter()
            .any(|item| item == "match_text"));
    }
}
