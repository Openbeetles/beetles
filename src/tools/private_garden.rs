//! private_garden 工具：板级主体拥有的自由内部工作区。

use crate::error::{Error, Result};
use crate::memory::{
    build_private_garden_usage, private_garden_scope_id, summarize_private_garden_directories,
    PrivateGardenStore, PRIVATE_GARDEN_MAX_DOCS_PER_CHAT,
};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolClarificationField, ToolClarificationOption,
    ToolContext, ToolExecutionBlocker, ToolExecutionOutcome, ToolMetadata,
};
use crate::util::current_unix_secs;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;

const PRIVATE_GARDEN_MAX_LIST_LIMIT: usize = 8;
const PRIVATE_GARDEN_MAX_TREE_LIMIT: usize = PRIVATE_GARDEN_MAX_DOCS_PER_CHAT;
const PRIVATE_GARDEN_MAX_CONTENT_LEN: usize = 8 * 1024;

pub struct PrivateGardenTool {
    store: Arc<dyn PrivateGardenStore + Send + Sync>,
}

#[derive(Serialize)]
struct PrivateGardenDocsResponse<T> {
    ok: bool,
    op: &'static str,
    docs: T,
}

#[derive(Serialize)]
struct PrivateGardenTreeResponse {
    ok: bool,
    op: &'static str,
    usage: crate::memory::PrivateGardenUsage,
    directories: Vec<crate::memory::PrivateGardenDirectorySummary>,
    docs: Vec<crate::memory::PrivateGardenDocRecord>,
}

#[derive(Serialize)]
struct PrivateGardenDocResponse<T> {
    ok: bool,
    op: &'static str,
    doc: T,
}

#[derive(Serialize)]
struct PrivateGardenMoveResponse<'a> {
    ok: bool,
    op: &'static str,
    from_path: &'a str,
    to_path: &'a str,
    doc: Option<crate::memory::PrivateGardenDocRecord>,
}

#[derive(Serialize)]
struct PrivateGardenDeleteResponse<'a> {
    ok: bool,
    op: &'static str,
    deleted: bool,
    path: &'a str,
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
        "Manage your board-owned free private garden. Use it for self-owned internal notes, drafts, and temporary organization that do not belong in shared factual memory or the governed private kernel. The garden belongs to the board-level self, not to the current user or a single chat. Prefer updating existing docs in place instead of accumulating per-turn history."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["list","tree","read","write","move","delete"],"description":"Operation to perform inside the board-owned private garden. Use list/tree/read before write or move when you need to inspect or reorganize existing material."},"path":{"type":"string","description":"Relative document path, e.g. journal/afterglow.md."},"from_path":{"type":"string","description":"Existing relative document path to move from."},"to_path":{"type":"string","description":"Target relative document path to move to."},"content":{"type":"string","description":"Complete document content for write. Writes replace the current document body, so prefer compact rewrites over appending historical notes."},"limit":{"type":"integer","description":"Max docs to list; for list default 4 max 8, for tree default 16 max 16."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        self.execute_outcome(args, ctx)
            .map(|outcome| outcome.content)
    }

    fn execute_outcome(
        &self,
        args: &str,
        ctx: &mut dyn ToolContext,
    ) -> Result<ToolExecutionOutcome> {
        let obj = parse_tool_args(args, "tool_private_garden")?;
        let Some(_chat_id) = ctx.current_chat_id() else {
            return Ok(private_garden_missing_scope_outcome());
        };
        let scope_id = private_garden_scope_id();
        let Some(op) = obj
            .get("op")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(private_garden_invalid_op_outcome());
        };

        match op {
            "list" => {
                let limit = obj
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(4)
                    .clamp(1, PRIVATE_GARDEN_MAX_LIST_LIMIT as u64)
                    as usize;
                let docs = self.store.list(scope_id, limit)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenDocsResponse {
                        ok: true,
                        op: "list",
                        docs,
                    },
                )?))
            }
            "tree" => {
                let limit = obj
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(PRIVATE_GARDEN_MAX_TREE_LIMIT as u64)
                    .clamp(1, PRIVATE_GARDEN_MAX_TREE_LIMIT as u64)
                    as usize;
                let all_docs = self.store.list(scope_id, usize::MAX)?;
                let docs = all_docs.iter().take(limit).cloned().collect::<Vec<_>>();
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenTreeResponse {
                        ok: true,
                        op: "tree",
                        usage: build_private_garden_usage(&all_docs),
                        directories: summarize_private_garden_directories(&all_docs, 8),
                        docs,
                    },
                )?))
            }
            "read" => {
                let Some(path) = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(private_garden_missing_field_outcome("path"));
                };
                let doc = self.store.read(scope_id, path)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenDocResponse {
                        ok: true,
                        op: "read",
                        doc,
                    },
                )?))
            }
            "write" => {
                let Some(path) = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(private_garden_missing_field_outcome("path"));
                };
                let Some(content) = obj.get("content").and_then(Value::as_str) else {
                    return Ok(private_garden_missing_field_outcome("content"));
                };
                if content.len() > PRIVATE_GARDEN_MAX_CONTENT_LEN {
                    return Err(Error::config(
                        "tool_private_garden",
                        format!("content exceeds {} bytes", PRIVATE_GARDEN_MAX_CONTENT_LEN),
                    ));
                }
                let record = self
                    .store
                    .write(scope_id, path, content, current_unix_secs())?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenDocResponse {
                        ok: true,
                        op: "write",
                        doc: record,
                    },
                )?))
            }
            "move" => {
                let Some(from_path) = obj
                    .get("from_path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(private_garden_missing_field_outcome("from_path"));
                };
                let Some(to_path) = obj
                    .get("to_path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(private_garden_missing_field_outcome("to_path"));
                };
                let moved =
                    self.store
                        .move_doc(scope_id, from_path, to_path, current_unix_secs())?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenMoveResponse {
                        ok: true,
                        op: "move",
                        from_path,
                        to_path,
                        doc: moved,
                    },
                )?))
            }
            "delete" => {
                let Some(path) = obj
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return Ok(private_garden_missing_field_outcome("path"));
                };
                let deleted = self.store.delete(scope_id, path)?;
                Ok(ToolExecutionOutcome::text(serialize_tool_output(
                    "tool_private_garden",
                    &PrivateGardenDeleteResponse {
                        ok: true,
                        op: "delete",
                        deleted,
                        path,
                    },
                )?))
            }
            _ => Ok(private_garden_invalid_op_outcome()),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

fn private_garden_missing_scope_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "warning": "private_garden: current chat scope is unavailable",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::runtime_blocked(
        "private_garden can only run inside an active conversation scope.",
    ))
}

fn private_garden_invalid_op_outcome() -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "warning": "private_garden: choose list, tree, read, write, move, or delete",
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_choice(
        "An operation is still required before private_garden can continue.",
        vec!["op".to_string()],
        vec![ToolClarificationField {
            key: "op".to_string(),
            label: "Operation".to_string(),
            description: "Choose how to interact with the private garden.".to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: vec![
                ToolClarificationOption {
                    value: "list".to_string(),
                    label: "list".to_string(),
                },
                ToolClarificationOption {
                    value: "tree".to_string(),
                    label: "tree".to_string(),
                },
                ToolClarificationOption {
                    value: "read".to_string(),
                    label: "read".to_string(),
                },
                ToolClarificationOption {
                    value: "write".to_string(),
                    label: "write".to_string(),
                },
                ToolClarificationOption {
                    value: "move".to_string(),
                    label: "move".to_string(),
                },
                ToolClarificationOption {
                    value: "delete".to_string(),
                    label: "delete".to_string(),
                },
            ],
        }],
    ))
}

fn private_garden_missing_field_outcome(field: &str) -> ToolExecutionOutcome {
    ToolExecutionOutcome::text(
        json!({
            "ok": false,
            "warning": format!("private_garden: missing {}", field),
        })
        .to_string(),
    )
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        format!("{field} is still required before private_garden can continue."),
        vec![field.to_string()],
        vec![ToolClarificationField {
            key: field.to_string(),
            label: field.to_string(),
            description: format!("Provide {} for private_garden.", field),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use crate::memory::{PrivateGardenDoc, PrivateGardenDocRecord, BOARD_SUBJECT_SCOPE_ID};
    use crate::platform::ResponseBody;
    use crate::tools::ToolExecutionBlockerKind;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<Vec<PrivateGardenDoc>>,
        scope_calls: Mutex<Vec<String>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
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

        fn read(&self, chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
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
            chat_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
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

        fn delete(&self, chat_id: &str, doc_path: &str) -> Result<bool> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let before = docs.len();
            docs.retain(|doc| doc.path != doc_path);
            Ok(docs.len() != before)
        }

        fn move_doc(
            &self,
            chat_id: &str,
            from_path: &str,
            to_path: &str,
            now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            self.scope_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(chat_id.to_string());
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

    #[test]
    fn private_garden_tool_uses_board_subject_scope_instead_of_current_chat_id() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool =
            PrivateGardenTool::new(Arc::clone(&store) as Arc<dyn PrivateGardenStore + Send + Sync>);
        let mut ctx = StubToolContext {
            chat_id: Some("chat-1".to_string()),
        };

        tool.execute(
            r#"{"op":"write","path":"journal/afterglow.md","content":"这是我的内部私域"}"#,
            &mut ctx,
        )
        .unwrap();
        tool.execute(r#"{"op":"read","path":"journal/afterglow.md"}"#, &mut ctx)
            .unwrap();
        tool.execute(r#"{"op":"list","limit":1}"#, &mut ctx)
            .unwrap();

        let scopes = store
            .scope_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        assert!(
            scopes.iter().all(|scope| scope == BOARD_SUBJECT_SCOPE_ID),
            "private_garden tool must persist under board subject scope, got {scopes:?}"
        );
    }

    #[test]
    fn private_garden_tool_missing_session_scope_returns_runtime_blocker() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool =
            PrivateGardenTool::new(Arc::clone(&store) as Arc<dyn PrivateGardenStore + Send + Sync>);
        let mut ctx = StubToolContext { chat_id: None };

        let outcome = tool
            .execute_outcome(r#"{"op":"list"}"#, &mut ctx)
            .expect("runtime blocker");
        let blocker = outcome.blocker.expect("runtime blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::RuntimeBlocked);
    }

    #[test]
    fn private_garden_tool_invalid_op_returns_choice_blocker() {
        let store = Arc::new(StubPrivateGardenStore::default());
        let tool =
            PrivateGardenTool::new(Arc::clone(&store) as Arc<dyn PrivateGardenStore + Send + Sync>);
        let mut ctx = StubToolContext {
            chat_id: Some("chat-1".to_string()),
        };

        let outcome = tool
            .execute_outcome(r#"{"op":"weird"}"#, &mut ctx)
            .expect("choice blocker");
        let blocker = outcome.blocker.expect("choice blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserChoice);
        assert_eq!(blocker.missing_fields, vec!["op".to_string()]);
    }
}
