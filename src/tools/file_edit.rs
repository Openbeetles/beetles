//! file_edit 工具：对状态根中的文本文件执行确定性局部改写。
//! file_edit tool: perform deterministic localized edits on text files under storage.

use crate::constants::FILE_WRITE_MAX_CONTENT_LEN;
use crate::error::{Error, Result};
use crate::tools::state_file_guard::{ensure_state_path_mutable, normalize_state_tool_path};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::json;
use std::sync::Arc;

const MAX_EDIT_FILE_BYTES: usize = 64 * 1024;

pub struct FileEditTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
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

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path under storage root, e.g. notes/todo.txt"
                },
                "mode": {
                    "type": "string",
                    "enum": ["replace_once", "replace_all", "insert_before", "insert_after", "prepend"],
                    "description": "Edit operation to perform"
                },
                "match_text": {
                    "type": "string",
                    "description": "Exact text to locate for replace/insert modes"
                },
                "content": {
                    "type": "string",
                    "description": "Replacement or inserted content"
                }
            },
            "required": ["path", "mode", "content"]
        })
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_file_edit")?;
        let path_arg = obj
            .get("path")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_file_edit", "missing path"))?;
        let mode = obj
            .get("mode")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_file_edit", "missing mode"))?;
        let content = obj
            .get("content")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::config("tool_file_edit", "missing content"))?;

        if content.len() > FILE_WRITE_MAX_CONTENT_LEN {
            return Err(Error::config(
                "tool_file_edit",
                format!("content exceeds {} bytes", FILE_WRITE_MAX_CONTENT_LEN),
            ));
        }

        let rel = normalize_state_tool_path(path_arg, "tool_file_edit")?;
        ensure_state_path_mutable(&rel, "tool_file_edit")?;

        let raw = self
            .state_fs
            .read(&rel)?
            .ok_or_else(|| Error::config("tool_file_edit", "file not found"))?;
        if raw.len() > MAX_EDIT_FILE_BYTES {
            return Err(Error::config("tool_file_edit", "file too large"));
        }
        let current = std::str::from_utf8(&raw)
            .map_err(|_| Error::config("tool_file_edit", "file is not valid UTF-8"))?;

        let edit = match mode {
            "replace_once" => {
                let match_text = parse_match_text(&obj)?;
                apply_replace_once(current, match_text, content)?
            }
            "replace_all" => {
                let match_text = parse_match_text(&obj)?;
                apply_replace_all(current, match_text, content)?
            }
            "insert_before" => {
                let match_text = parse_match_text(&obj)?;
                apply_insert_before(current, match_text, content)?
            }
            "insert_after" => {
                let match_text = parse_match_text(&obj)?;
                apply_insert_after(current, match_text, content)?
            }
            "prepend" => apply_prepend(current, content),
            _ => {
                return Err(Error::config(
                    "tool_file_edit",
                    "mode must be one of: replace_once, replace_all, insert_before, insert_after, prepend",
                ))
            }
        };

        self.state_fs.write(&rel, edit.content.as_bytes())?;

        Ok(json!({
            "path": path_arg,
            "ok": true,
            "mode": mode,
            "match_count": edit.match_count,
            "bytes_written": edit.content.len(),
        })
        .to_string())
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

struct AppliedEdit {
    content: String,
    match_count: usize,
}

fn parse_match_text<'a>(obj: &'a serde_json::Map<String, serde_json::Value>) -> Result<&'a str> {
    obj.get("match_text")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::config("tool_file_edit", "missing match_text"))
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

#[cfg(test)]
mod tests {
    use super::FileEditTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::{Tool, ToolContext};
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
        fs.write("config/USER.md", b"name: test").unwrap();
        let tool = FileEditTool::new(Arc::clone(&fs) as Arc<dyn StateFs + Send + Sync>);

        let err = tool
            .execute(
                r#"{"path":"config/USER.md","mode":"prepend","content":"x"}"#,
                &mut MockToolContext,
            )
            .unwrap_err();
        assert!(format!("{err}").contains("protected"));
    }
}
