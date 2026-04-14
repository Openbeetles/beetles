//! document_search 工具：在状态根内递归检索文档名与正文，返回命中的路径与摘要片段。
//! document_search tool: recursively search document names and readable content under storage.

use crate::documents::{
    build_search_snippet, contains_query_text, decode_searchable_document_text,
    detect_document_kind,
};
use crate::error::{Error, Result};
use crate::tools::state_file_guard::sanitize_state_file_read;
use crate::tools::{parse_tool_args, Tool, ToolContext};
use crate::util::normalize_state_rel_path;
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::VecDeque;
use std::sync::Arc;

const TAG: &str = "tools::document_search";
const DEFAULT_LIMIT: usize = 6;
const MAX_LIMIT: usize = 12;
const MAX_SCAN_FILES: usize = 128;
const MAX_TOTAL_RAW_BYTES: usize = 1024 * 1024;
const MAX_FILE_RAW_BYTES: usize = 256 * 1024;

pub struct DocumentSearchTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

impl DocumentSearchTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for DocumentSearchTool {
    fn name(&self) -> &'static str {
        "document_search"
    }

    fn description(&self) -> &'static str {
        "Search document names and readable content under storage. Use this first when you know a phrase or topic but do not know which file contains it, then use document_read to inspect the matched path."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"query":{"type":"string","description":"Phrase to search for in file paths and readable document text"},"path":{"type":"string","description":"Optional storage subpath or file to search under, e.g. docs or notes/todo.md"},"limit":{"type":"integer","description":"Maximum number of matches to return (default 6, max 12)"},"case_sensitive":{"type":"boolean","description":"Whether matching is case-sensitive (default false)"}},"required":["query"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_document_search")?;
        let query = obj
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::config("tool_document_search", "missing query"))?;
        let scope_arg = obj.get("path").and_then(Value::as_str).unwrap_or("");
        let scope = normalize_state_rel_path(scope_arg)
            .map_err(|_| Error::config("tool_document_search", "invalid path"))?;
        let limit = parse_limit(obj.get("limit"));
        let case_sensitive = obj
            .get("case_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let response = if scope.is_empty() || self.state_fs.list_dir(&scope).is_ok() {
            self.search_directory(query, scope_arg, &scope, limit, case_sensitive)?
        } else if let Some(raw) = self.state_fs.read(&scope)? {
            let sanitized = sanitize_state_file_read(&scope, &raw, "tool_document_search")?;
            let stats = SearchStats {
                scanned_files: 1,
                scanned_raw_bytes: raw.len().min(MAX_TOTAL_RAW_BYTES),
                ..SearchStats::default()
            };
            let matches = search_file(&scope, sanitized.as_ref(), query, case_sensitive)
                .into_iter()
                .take(limit)
                .collect::<Vec<_>>();
            build_response(query, scope_arg, matches, stats)
        } else {
            return Err(Error::config("tool_document_search", "path not found"));
        };

        Ok(response.to_string())
    }
}

impl DocumentSearchTool {
    fn search_directory(
        &self,
        query: &str,
        scope_arg: &str,
        scope: &str,
        limit: usize,
        case_sensitive: bool,
    ) -> Result<Value> {
        let mut queue = VecDeque::from([scope.to_string()]);
        let mut matches = Vec::new();
        let mut stats = SearchStats::default();

        while let Some(dir) = queue.pop_front() {
            let mut entries = self.state_fs.list_dir(&dir)?;
            entries.sort();
            for entry in entries {
                let child = join_rel_path(&dir, entry.trim_end_matches('/'));
                if entry.ends_with('/') {
                    queue.push_back(child);
                    continue;
                }
                if stats.scanned_files >= MAX_SCAN_FILES
                    || stats.scanned_raw_bytes >= MAX_TOTAL_RAW_BYTES
                    || matches.len() >= limit
                {
                    stats.truncated = true;
                    break;
                }

                let Some(raw) = self.state_fs.read(&child)? else {
                    continue;
                };
                let sanitized = sanitize_state_file_read(&child, &raw, "tool_document_search")?;
                stats.scanned_files = stats.scanned_files.saturating_add(1);
                stats.scanned_raw_bytes = stats
                    .scanned_raw_bytes
                    .saturating_add(raw.len())
                    .min(MAX_TOTAL_RAW_BYTES);
                if let Some(hit) = search_file(&child, sanitized.as_ref(), query, case_sensitive) {
                    matches.push(hit);
                    if matches.len() >= limit {
                        stats.truncated = true;
                        break;
                    }
                }
            }
            if stats.truncated {
                break;
            }
        }

        log::info!(
            "[{}] query_len={} scope={} scanned_files={} matches={} truncated={}",
            TAG,
            query.chars().count(),
            if scope_arg.is_empty() { "." } else { scope_arg },
            stats.scanned_files,
            matches.len(),
            stats.truncated
        );
        Ok(build_response(query, scope_arg, matches, stats))
    }
}

#[derive(Default)]
struct SearchStats {
    scanned_files: usize,
    scanned_raw_bytes: usize,
    truncated: bool,
}

fn build_response(
    query: &str,
    scope_arg: &str,
    mut matches: Vec<Value>,
    stats: SearchStats,
) -> Value {
    matches.sort_by_key(|item| {
        let score = item.get("score").and_then(Value::as_u64).unwrap_or(0);
        let path = item
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        (Reverse(score), path)
    });
    for item in &mut matches {
        if let Some(obj) = item.as_object_mut() {
            obj.remove("score");
        }
    }
    json!({
        "query": query,
        "path": if scope_arg.is_empty() { "." } else { scope_arg },
        "matches": matches,
        "scanned_files": stats.scanned_files,
        "scanned_raw_bytes": stats.scanned_raw_bytes,
        "truncated": stats.truncated,
    })
}

fn parse_limit(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|raw| raw.clamp(1, MAX_LIMIT as u64) as usize)
        .unwrap_or(DEFAULT_LIMIT)
}

fn join_rel_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

fn search_file(path: &str, raw: &[u8], query: &str, case_sensitive: bool) -> Option<Value> {
    let path_hit = contains_query_text(path, query, case_sensitive);
    let kind = detect_document_kind(path, raw);

    if raw.len() > MAX_FILE_RAW_BYTES {
        return path_hit.then(|| {
            json!({
                "path": path,
                "kind": kind,
                "match": "path",
                "snippet": Value::Null,
                "warning": "path matched but content was not inspected because the file is too large",
                "raw_bytes": raw.len(),
                "score": 2u64,
            })
        });
    }

    let Some(text) = decode_searchable_document_text(path, raw) else {
        return path_hit.then(|| {
            json!({
                "path": path,
                "kind": kind,
                "match": "path",
                "snippet": Value::Null,
                "warning": "path matched but the file content is not readable text",
                "raw_bytes": raw.len(),
                "score": 2u64,
            })
        });
    };

    let content_hit = contains_query_text(&text, query, case_sensitive);
    if !path_hit && !content_hit {
        return None;
    }

    let match_kind = match (path_hit, content_hit) {
        (true, true) => "path+content",
        (true, false) => "path",
        (false, true) => "content",
        (false, false) => unreachable!(),
    };
    let score = match match_kind {
        "path+content" => 3,
        "path" => 2,
        _ => 1,
    };
    let snippet = if content_hit {
        Value::String(build_search_snippet(&text, query, case_sensitive))
    } else {
        Value::Null
    };

    Some(json!({
        "path": path,
        "kind": kind,
        "match": match_kind,
        "snippet": snippet,
        "warning": Value::Null,
        "raw_bytes": raw.len(),
        "score": score,
    }))
}

#[cfg(test)]
mod tests {
    use super::DocumentSearchTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::pdf_read::test_pdf_fixture_bytes;
    use crate::tools::{Tool, ToolContext};
    use serde_json::Value;
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

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{rel_path}/")
            };
            let mut out = Vec::new();
            for key in self.files.lock().unwrap().keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let rest = &key[prefix.len()..];
                if rest.is_empty() {
                    continue;
                }
                if let Some((head, _tail)) = rest.split_once('/') {
                    let entry = format!("{head}/");
                    if !out.contains(&entry) {
                        out.push(entry);
                    }
                } else if !out.iter().any(|item| item == rest) {
                    out.push(rest.to_string());
                }
            }
            out.sort();
            Ok(out)
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
    fn searches_nested_text_content() {
        let fs = Arc::new(MockStateFs::default());
        fs.write(
            "docs/project/notes.md",
            b"Alpha roadmap\n\nThe beetle device should support LAN automation.",
        )
        .unwrap();
        fs.write("docs/other.txt", b"No relevant content").unwrap();

        let tool = DocumentSearchTool::new(fs);
        let result = tool
            .execute(
                r#"{"query":"LAN automation","path":"docs"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let matches = parsed["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["path"], "docs/project/notes.md");
        assert_eq!(matches[0]["match"], "content");
        assert!(matches[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("LAN automation"));
    }

    #[test]
    fn searches_pdf_content() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("manuals/hello.pdf", &test_pdf_fixture_bytes())
            .unwrap();

        let tool = DocumentSearchTool::new(fs);
        let result = tool
            .execute(r#"{"query":"hello"}"#, &mut MockToolContext)
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let matches = parsed["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["path"], "manuals/hello.pdf");
        assert_eq!(matches[0]["kind"], "pdf");
        assert_eq!(matches[0]["match"], "path+content");
    }

    #[test]
    fn returns_path_match_for_unreadable_binary() {
        let fs = Arc::new(MockStateFs::default());
        fs.write("assets/beetle-logo.bin", &[0, 159, 146, 150])
            .unwrap();

        let tool = DocumentSearchTool::new(fs);
        let result = tool
            .execute(r#"{"query":"logo"}"#, &mut MockToolContext)
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let matches = parsed["matches"].as_array().unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["match"], "path");
        assert!(matches[0]["warning"]
            .as_str()
            .unwrap()
            .contains("not readable text"));
    }

    #[test]
    fn search_redacts_sensitive_config_snippets() {
        let fs = Arc::new(MockStateFs::default());
        fs.write(
            "config/channels.json",
            br#"{"tg_token":"123456:live-secret","enabled_channel":"telegram"}"#,
        )
        .unwrap();

        let tool = DocumentSearchTool::new(fs);
        let result = tool
            .execute(
                r#"{"query":"tg_token","path":"config"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let matches = parsed["matches"].as_array().unwrap();
        let snippet = matches[0]["snippet"].as_str().unwrap();

        assert!(snippet.contains("[REDACTED]"));
        assert!(!snippet.contains("123456:live-secret"));
    }
}
