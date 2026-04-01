//! document_extract 工具：从统一文档源中提取命中行、章节或 JSON 字段。
//! document_extract tool: extract targeted lines, sections, or JSON fields from a document source.

use crate::error::{Error, Result};
use crate::tools::document_read::{read_document_source, DEFAULT_DOCUMENT_MAX_CHARS};
use crate::tools::web_fetch::parse_max_chars;
use crate::tools::{parse_tool_args, Tool, ToolContext};
use serde_json::{json, Value};
use std::sync::Arc;

const DEFAULT_ITEM_LIMIT: usize = 4;
const MAX_ITEM_LIMIT: usize = 8;
const DEFAULT_CONTEXT_LINES: usize = 1;
const MAX_CONTEXT_LINES: usize = 4;

pub struct DocumentExtractTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

impl DocumentExtractTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for DocumentExtractTool {
    fn name(&self) -> &'static str {
        "document_extract"
    }

    fn description(&self) -> &'static str {
        "Extract targeted content from a public URL or stored document. Supports matching lines, Markdown/text sections, and JSON fields. Use this after document_search or document_read when only part of a document is needed."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Public http(s) URL or storage path such as docs/readme.md"
                },
                "mode": {
                    "type": "string",
                    "enum": ["lines", "section", "json_field"],
                    "description": "Extraction mode"
                },
                "query": {
                    "type": "string",
                    "description": "Phrase to match for lines or sections"
                },
                "json_path": {
                    "type": "string",
                    "description": "JSON pointer (/a/b/0) or dot path (a.b[0]) for json_field mode"
                },
                "context_before": {
                    "type": "integer",
                    "description": "Context lines before each matched line (default 1, max 4)"
                },
                "context_after": {
                    "type": "integer",
                    "description": "Context lines after each matched line (default 1, max 4)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum extracted items to return (default 4, max 8)"
                },
                "case_sensitive": {
                    "type": "boolean",
                    "description": "Whether matching is case-sensitive (default false)"
                },
                "max_chars": {
                    "type": "integer",
                    "description": "Maximum source characters to load before extraction (default 16000, max 50000)"
                }
            },
            "required": ["source", "mode"]
        })
    }

    fn requires_network(&self) -> bool {
        false
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_document_extract")?;
        let source = obj
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_document_extract", "missing source"))?;
        let mode = obj
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_document_extract", "missing mode"))?;
        let limit = parse_limit(obj.get("limit"));
        let case_sensitive = obj
            .get("case_sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_chars = parse_max_chars(obj.get("max_chars"), DEFAULT_DOCUMENT_MAX_CHARS);
        let document = read_document_source(
            self.state_fs.as_ref(),
            source,
            max_chars,
            self.name(),
            "tool_document_extract",
            ctx,
        )?;

        let response = match mode {
            "lines" => {
                let query = parse_query(&obj, "tool_document_extract")?;
                let before = parse_context(obj.get("context_before"));
                let after = parse_context(obj.get("context_after"));
                build_lines_response(&document, query, before, after, limit, case_sensitive)
            }
            "section" => {
                let query = parse_query(&obj, "tool_document_extract")?;
                build_section_response(&document, query, limit, case_sensitive)
            }
            "json_field" => {
                let json_path = obj
                    .get("json_path")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| Error::config("tool_document_extract", "missing json_path"))?;
                build_json_field_response(&document, json_path)
            }
            _ => {
                return Err(Error::config(
                    "tool_document_extract",
                    "mode must be one of: lines, section, json_field",
                ))
            }
        }?;

        Ok(response.to_string())
    }
}

fn parse_query<'a>(
    obj: &'a serde_json::Map<String, Value>,
    stage: &'static str,
) -> Result<&'a str> {
    obj.get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::config(stage, "missing query"))
}

fn parse_limit(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|raw| raw.clamp(1, MAX_ITEM_LIMIT as u64) as usize)
        .unwrap_or(DEFAULT_ITEM_LIMIT)
}

fn parse_context(value: Option<&Value>) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|raw| raw.min(MAX_CONTEXT_LINES as u64) as usize)
        .unwrap_or(DEFAULT_CONTEXT_LINES)
}

fn build_lines_response(
    document: &crate::tools::document_read::ReadableDocument,
    query: &str,
    context_before: usize,
    context_after: usize,
    limit: usize,
    case_sensitive: bool,
) -> Result<Value> {
    let lines: Vec<&str> = document.content.lines().collect();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if contains_query(line, query, case_sensitive) {
            let start = idx.saturating_sub(context_before);
            let end = (idx + context_after + 1).min(lines.len());
            if let Some(last) = ranges.last_mut() {
                if start <= last.1 {
                    last.1 = last.1.max(end);
                    continue;
                }
            }
            ranges.push((start, end));
            if ranges.len() >= limit {
                break;
            }
        }
    }

    let items = ranges
        .iter()
        .map(|(start, end)| {
            json!({
                "start_line": start + 1,
                "end_line": *end,
                "text": lines[*start..*end].join("\n"),
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "source": document.source,
        "kind": document.kind,
        "mode": "lines",
        "query": query,
        "items": items,
        "match_count": ranges.len(),
        "content_truncated": document.truncated,
        "warning": document.warning,
    }))
}

fn build_section_response(
    document: &crate::tools::document_read::ReadableDocument,
    query: &str,
    limit: usize,
    case_sensitive: bool,
) -> Result<Value> {
    let items = if has_markdown_headings(&document.content) {
        extract_markdown_sections(&document.content, query, limit, case_sensitive)
    } else {
        extract_paragraph_sections(&document.content, query, limit, case_sensitive)
    };

    Ok(json!({
        "source": document.source,
        "kind": document.kind,
        "mode": "section",
        "query": query,
        "items": items,
        "match_count": items.len(),
        "content_truncated": document.truncated,
        "warning": document.warning,
    }))
}

fn build_json_field_response(
    document: &crate::tools::document_read::ReadableDocument,
    json_path: &str,
) -> Result<Value> {
    if document.kind != "json" {
        return Err(Error::config(
            "tool_document_extract",
            "json_field mode requires a JSON document",
        ));
    }
    let parsed: Value = serde_json::from_str(&document.content).map_err(|err| Error::Other {
        source: Box::new(err),
        stage: "tool_document_extract",
    })?;
    let pointer = normalize_json_pointer(json_path)
        .ok_or_else(|| Error::config("tool_document_extract", "invalid json_path"))?;
    let value = parsed
        .pointer(&pointer)
        .cloned()
        .ok_or_else(|| Error::config("tool_document_extract", "json_path not found"))?;
    Ok(json!({
        "source": document.source,
        "kind": document.kind,
        "mode": "json_field",
        "json_path": json_path,
        "resolved_pointer": pointer,
        "value": value,
        "content_truncated": document.truncated,
        "warning": document.warning,
    }))
}

fn contains_query(haystack: &str, needle: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

fn has_markdown_headings(text: &str) -> bool {
    text.lines()
        .any(|line| parse_markdown_heading(line).is_some())
}

fn extract_markdown_sections(
    text: &str,
    query: &str,
    limit: usize,
    case_sensitive: bool,
) -> Vec<Value> {
    let lines: Vec<&str> = text.lines().collect();
    let heading_positions = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| parse_markdown_heading(line).map(|title| (idx, title)))
        .collect::<Vec<_>>();
    let mut items = Vec::new();

    for (pos, (start, heading)) in heading_positions.iter().enumerate() {
        let end = heading_positions
            .get(pos + 1)
            .map(|(next, _)| *next)
            .unwrap_or(lines.len());
        let block = lines[*start..end].join("\n");
        let match_kind = if contains_query(heading, query, case_sensitive) {
            Some("heading")
        } else if contains_query(&block, query, case_sensitive) {
            Some("content")
        } else {
            None
        };
        if let Some(kind) = match_kind {
            items.push(json!({
                "heading": heading,
                "match": kind,
                "text": block,
            }));
            if items.len() >= limit {
                break;
            }
        }
    }

    if items.is_empty() {
        return extract_paragraph_sections(text, query, limit, case_sensitive);
    }
    items
}

fn extract_paragraph_sections(
    text: &str,
    query: &str,
    limit: usize,
    case_sensitive: bool,
) -> Vec<Value> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty() && contains_query(block, query, case_sensitive))
        .take(limit)
        .map(|block| {
            json!({
                "heading": Value::Null,
                "match": "content",
                "text": block,
            })
        })
        .collect()
}

fn parse_markdown_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 {
        return None;
    }
    let title = trimmed[hashes..].trim();
    (!title.is_empty()).then_some(title)
}

fn normalize_json_pointer(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('/') {
        return Some(trimmed.to_string());
    }

    let mut segments = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut idx = 0usize;
    while idx < chars.len() {
        match chars[idx] {
            '.' => {
                if buf.is_empty() {
                    idx += 1;
                    continue;
                }
                segments.push(std::mem::take(&mut buf));
                idx += 1;
            }
            '[' => {
                if !buf.is_empty() {
                    segments.push(std::mem::take(&mut buf));
                }
                idx += 1;
                let start = idx;
                while idx < chars.len() && chars[idx] != ']' {
                    idx += 1;
                }
                if idx == chars.len() || start == idx {
                    return None;
                }
                segments.push(chars[start..idx].iter().collect::<String>());
                idx += 1;
            }
            ']' => return None,
            ch => {
                buf.push(ch);
                idx += 1;
            }
        }
    }
    if !buf.is_empty() {
        segments.push(buf);
    }
    if segments.is_empty() {
        return None;
    }

    Some(
        segments
            .into_iter()
            .fold(String::new(), |mut acc, segment| {
                acc.push('/');
                acc.push_str(&segment.replace('~', "~0").replace('/', "~1"));
                acc
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::DocumentExtractTool;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
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
    fn extracts_matching_lines_with_context() {
        let fs = Arc::new(MockStateFs::default());
        fs.write(
            "logs/app.log",
            b"line1\nerror start\nstack trace\nline4\nline5\nanother error\nline7",
        )
        .unwrap();
        let tool = DocumentExtractTool::new(fs);
        let result = tool
            .execute(
                r#"{"source":"logs/app.log","mode":"lines","query":"error","context_before":1,"context_after":1}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let items = parsed["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items[0]["text"].as_str().unwrap().contains("line1"));
        assert!(items[0]["text"].as_str().unwrap().contains("stack trace"));
    }

    #[test]
    fn extracts_markdown_section() {
        let fs = Arc::new(MockStateFs::default());
        fs.write(
            "docs/guide.md",
            b"# Intro\nhello\n\n## Setup\ninstall beetle here\nmore setup\n\n## Usage\nrun beetle",
        )
        .unwrap();
        let tool = DocumentExtractTool::new(fs);
        let result = tool
            .execute(
                r#"{"source":"docs/guide.md","mode":"section","query":"setup"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let items = parsed["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["heading"], "Setup");
        assert!(items[0]["text"]
            .as_str()
            .unwrap()
            .contains("install beetle"));
    }

    #[test]
    fn extracts_json_field_from_dot_path() {
        let fs = Arc::new(MockStateFs::default());
        fs.write(
            "config/app.json",
            br#"{"llm":{"model":"gpt-5","routing":[{"name":"fast"}]}}"#,
        )
        .unwrap();
        let tool = DocumentExtractTool::new(fs);
        let result = tool
            .execute(
                r#"{"source":"config/app.json","mode":"json_field","json_path":"llm.routing[0].name"}"#,
                &mut MockToolContext,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["value"], "fast");
        assert_eq!(parsed["resolved_pointer"], "/llm/routing/0/name");
    }
}
