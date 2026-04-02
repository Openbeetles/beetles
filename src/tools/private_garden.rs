//! private_garden 工具：当前 chat 作用域内的自由内部工作区。

use crate::error::{Error, Result};
use crate::memory::{
    build_private_garden_usage, summarize_private_garden_directories, PrivateGardenStore,
    PRIVATE_GARDEN_MAX_DOCS_PER_CHAT,
};
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::util::current_unix_secs;
use serde_json::{json, Value};
use std::sync::Arc;

const PRIVATE_GARDEN_MAX_LIST_LIMIT: usize = 8;
const PRIVATE_GARDEN_MAX_TREE_LIMIT: usize = PRIVATE_GARDEN_MAX_DOCS_PER_CHAT;
const PRIVATE_GARDEN_MAX_CONTENT_LEN: usize = 8 * 1024;

pub struct PrivateGardenTool {
    store: Arc<dyn PrivateGardenStore + Send + Sync>,
}

impl PrivateGardenTool {
    pub(crate) fn new(store: Arc<dyn PrivateGardenStore + Send + Sync>) -> Self {
        Self { store }
    }
}

impl Tool for PrivateGardenTool {
    fn name(&self) -> &'static str {
        "private_garden"
    }

    fn description(&self) -> &str {
        "Manage your current chat's free private workspace. Use it for self-owned internal notes, drafts, and temporary organization that do not belong in shared factual memory or the governed private kernel. Prefer updating existing docs in place instead of accumulating per-turn history."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": {
                    "type": "string",
                    "enum": ["list", "tree", "read", "write", "move", "delete"],
                    "description": "Operation to perform inside the current chat's private garden. Use list/tree/read before write or move when you need to inspect or reorganize existing material."
                },
                "path": {
                    "type": "string",
                    "description": "Relative document path, e.g. journal/afterglow.md."
                },
                "from_path": {
                    "type": "string",
                    "description": "Existing relative document path to move from."
                },
                "to_path": {
                    "type": "string",
                    "description": "Target relative document path to move to."
                },
                "content": {
                    "type": "string",
                    "description": "Complete document content for write. Writes replace the current document body, so prefer compact rewrites over appending historical notes."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max docs to list; for list default 4 max 8, for tree default 16 max 16."
                }
            },
            "required": ["op"]
        })
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_private_garden")?;
        let chat_id = ctx.current_chat_id().ok_or_else(|| {
            Error::config("tool_private_garden", "current chat_id is unavailable")
        })?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_private_garden", "missing op"))?;

        match op {
            "list" => {
                let limit = obj
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(4)
                    .clamp(1, PRIVATE_GARDEN_MAX_LIST_LIMIT as u64)
                    as usize;
                let docs = self.store.list(chat_id, limit)?;
                Ok(json!({
                    "ok": true,
                    "op": "list",
                    "docs": docs,
                })
                .to_string())
            }
            "tree" => {
                let limit = obj
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(PRIVATE_GARDEN_MAX_TREE_LIMIT as u64)
                    .clamp(1, PRIVATE_GARDEN_MAX_TREE_LIMIT as u64)
                    as usize;
                let all_docs = self.store.list(chat_id, usize::MAX)?;
                let docs = all_docs.iter().take(limit).cloned().collect::<Vec<_>>();
                Ok(json!({
                    "ok": true,
                    "op": "tree",
                    "usage": build_private_garden_usage(&all_docs),
                    "directories": summarize_private_garden_directories(&all_docs, 8),
                    "docs": docs,
                })
                .to_string())
            }
            "read" => {
                let path = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing path"))?;
                let doc = self.store.read(chat_id, path)?;
                Ok(json!({
                    "ok": true,
                    "op": "read",
                    "doc": doc,
                })
                .to_string())
            }
            "write" => {
                let path = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing path"))?;
                let content = obj
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing content"))?;
                if content.len() > PRIVATE_GARDEN_MAX_CONTENT_LEN {
                    return Err(Error::config(
                        "tool_private_garden",
                        format!("content exceeds {} bytes", PRIVATE_GARDEN_MAX_CONTENT_LEN),
                    ));
                }
                let record = self
                    .store
                    .write(chat_id, path, content, current_unix_secs())?;
                Ok(json!({
                    "ok": true,
                    "op": "write",
                    "doc": record,
                })
                .to_string())
            }
            "move" => {
                let from_path = obj
                    .get("from_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing from_path"))?;
                let to_path = obj
                    .get("to_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing to_path"))?;
                let moved =
                    self.store
                        .move_doc(chat_id, from_path, to_path, current_unix_secs())?;
                Ok(json!({
                    "ok": true,
                    "op": "move",
                    "from_path": from_path,
                    "to_path": to_path,
                    "doc": moved,
                })
                .to_string())
            }
            "delete" => {
                let path = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::config("tool_private_garden", "missing path"))?;
                let deleted = self.store.delete(chat_id, path)?;
                Ok(json!({
                    "ok": true,
                    "op": "delete",
                    "deleted": deleted,
                    "path": path,
                })
                .to_string())
            }
            _ => Err(Error::config(
                "tool_private_garden",
                "op must be list, tree, read, write, move, or delete",
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use crate::memory::{PrivateGardenDoc, PrivateGardenDocRecord};
    use crate::platform::ResponseBody;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<Vec<PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(limit)
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: doc.content.clone(),
                })
                .collect())
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|doc| doc.path == doc_path)
                .cloned())
        }

        fn write(
            &self,
            _chat_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let revision = docs
                .iter()
                .find(|doc| doc.path == doc_path)
                .map(|doc| doc.revision.saturating_add(1))
                .unwrap_or(1);
            let doc = PrivateGardenDoc {
                path: doc_path.to_string(),
                content: content.to_string(),
                updated_at: now_secs,
                revision,
            };
            docs.retain(|existing| existing.path != doc_path);
            docs.push(doc.clone());
            Ok(PrivateGardenDocRecord {
                path: doc.path,
                updated_at: doc.updated_at,
                revision: doc.revision,
                bytes: doc.content.len(),
                preview: doc.content,
            })
        }

        fn delete(&self, _chat_id: &str, doc_path: &str) -> Result<bool> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let before = docs.len();
            docs.retain(|doc| doc.path != doc_path);
            Ok(docs.len() != before)
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            from_path: &str,
            to_path: &str,
            now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let Some(doc) = docs.iter().find(|doc| doc.path == from_path).cloned() else {
                return Ok(None);
            };
            docs.retain(|existing| existing.path != from_path && existing.path != to_path);
            let moved = PrivateGardenDoc {
                path: to_path.to_string(),
                content: doc.content,
                updated_at: now_secs,
                revision: doc.revision.saturating_add(1),
            };
            docs.push(moved.clone());
            Ok(Some(PrivateGardenDocRecord {
                path: moved.path,
                updated_at: moved.updated_at,
                revision: moved.revision,
                bytes: moved.content.len(),
                preview: moved.content,
            }))
        }
    }

    struct StubToolContext {
        chat_id: Option<String>,
    }

    impl ToolContext for StubToolContext {
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

        fn current_chat_id(&self) -> Option<&str> {
            self.chat_id.as_deref()
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn private_garden_tool_writes_and_reads_current_chat_docs() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool = PrivateGardenTool::new(store);
        let mut ctx = StubToolContext {
            chat_id: Some("chat-1".to_string()),
        };

        let write = tool
            .execute(
                r#"{"op":"write","path":"journal/afterglow.md","content":"把自由空间交给模型自己整理"}"#,
                &mut ctx,
            )
            .unwrap();
        assert!(write.contains("\"op\":\"write\""));

        let read = tool
            .execute(r#"{"op":"read","path":"journal/afterglow.md"}"#, &mut ctx)
            .unwrap();
        assert!(read.contains("把自由空间交给模型自己整理"));

        let listed = tool
            .execute(r#"{"op":"list","limit":2}"#, &mut ctx)
            .unwrap();
        assert!(listed.contains("journal/afterglow.md"));
    }

    #[test]
    fn private_garden_tool_moves_current_chat_docs() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool = PrivateGardenTool::new(store);
        let mut ctx = StubToolContext {
            chat_id: Some("chat-1".to_string()),
        };

        tool.execute(
            r#"{"op":"write","path":"drafts/idea.md","content":"把自由空间交给模型自己整理"}"#,
            &mut ctx,
        )
        .unwrap();

        let moved = tool
            .execute(
                r#"{"op":"move","from_path":"drafts/idea.md","to_path":"journal/idea.md"}"#,
                &mut ctx,
            )
            .unwrap();
        assert!(moved.contains("\"op\":\"move\""));
        assert!(moved.contains("journal/idea.md"));

        let read = tool
            .execute(r#"{"op":"read","path":"journal/idea.md"}"#, &mut ctx)
            .unwrap();
        assert!(read.contains("把自由空间交给模型自己整理"));
    }

    #[test]
    fn private_garden_tool_reports_tree_shape() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool = PrivateGardenTool::new(store);
        let mut ctx = StubToolContext {
            chat_id: Some("chat-1".to_string()),
        };

        tool.execute(
            r#"{"op":"write","path":"journal/now.md","content":"当前关注自主治理"}"#,
            &mut ctx,
        )
        .unwrap();
        tool.execute(
            r#"{"op":"write","path":"scratch/raw.md","content":"临时草稿"}"#,
            &mut ctx,
        )
        .unwrap();

        let tree = tool.execute(r#"{"op":"tree"}"#, &mut ctx).unwrap();
        assert!(tree.contains("\"op\":\"tree\""));
        assert!(tree.contains("\"usage\""));
        assert!(tree.contains("\"directories\""));
        assert!(tree.contains("journal"));
        assert!(tree.contains("scratch"));
    }
}
