//! document_read 工具：统一读取公网 URL 或状态根文档路径，返回适合 LLM 阅读的文本。
//! document_read tool: read a public URL or state-root document path and return LLM-friendly text.

use crate::documents::{decode_readable_document, DecodedReadableDocument};
use crate::error::{Error, Result};
use crate::orchestrator::ToolDecision;
use crate::tools::state_file_guard::sanitize_state_file_read;
use crate::tools::web_fetch::{fetch_public_page_payload, parse_max_chars, WebFetchPrepared};
use crate::tools::{
    parse_tool_args, PdfReadTool, Tool, ToolClarificationField, ToolContext, ToolExecutionBlocker,
    ToolExecutionOutcome,
};
use crate::util::normalize_state_rel_path;
use serde_json::{json, Value};
use std::sync::Arc;

const TAG: &str = "tools::document_read";
pub(crate) const DEFAULT_DOCUMENT_MAX_CHARS: usize = 16_000;
const MAX_LOCAL_RAW_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadableDocument {
    pub source: String,
    pub kind: String,
    pub title: Option<String>,
    pub content: String,
    pub truncated: bool,
    pub raw_bytes: usize,
    pub warning: Option<String>,
}

impl ReadableDocument {
    pub(crate) fn to_value(&self) -> Value {
        json!({
            "source": self.source,
            "kind": self.kind,
            "title": self.title,
            "content": self.content,
            "truncated": self.truncated,
            "raw_bytes": self.raw_bytes,
            "warning": self.warning,
        })
    }
}

pub struct DocumentReadTool {
    state_fs: Arc<dyn crate::StateFs + Send + Sync>,
}

impl DocumentReadTool {
    pub(crate) fn new(state_fs: Arc<dyn crate::StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for DocumentReadTool {
    fn name(&self) -> &'static str {
        "document_read"
    }

    fn description(&self) -> &'static str {
        "Read a public URL or a document path under storage. Automatically handles HTML/text/JSON/PDF and returns cleaned readable text. If you need to locate the right stored file first, use document_search."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"source":{"type":"string","description":"Public http(s) URL or storage path such as docs/readme.md"},"max_chars":{"type":"integer","description":"Maximum characters to return (default 16000, max 50000)"}},"required":["source"]}"#
    }

    fn requires_network(&self) -> bool {
        false
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
        let obj = parse_tool_args(args, "tool_document_read")?;
        let Some(source) = obj
            .get("source")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return missing_source_outcome();
        };
        let max_chars = parse_max_chars(obj.get("max_chars"), DEFAULT_DOCUMENT_MAX_CHARS);
        if source.starts_with("http://") || source.starts_with("https://") {
            return read_url_document_outcome(source, max_chars, "tool_document_read", ctx);
        }
        read_local_document_outcome(
            self.state_fs.as_ref(),
            source,
            max_chars,
            "tool_document_read",
        )
    }
}

fn url_looks_like_pdf(source: &str) -> bool {
    let head = source.split('?').next().unwrap_or(source);
    head.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

pub(crate) fn read_document_source(
    state_fs: &(dyn crate::StateFs + Send + Sync),
    source: &str,
    max_chars: usize,
    tool_name: &'static str,
    stage: &'static str,
    ctx: &mut dyn ToolContext,
) -> Result<ReadableDocument> {
    if source.starts_with("http://") || source.starts_with("https://") {
        match crate::orchestrator::can_execute_tool_for_channel_pub(
            tool_name,
            true,
            ctx.current_channel().unwrap_or(""),
        ) {
            ToolDecision::Allow => {}
            ToolDecision::Deny { reason } => {
                return Err(Error::config(stage, format!("tool denied: {reason}")));
            }
        }
        return read_url_document(source, max_chars, stage, ctx);
    }

    let rel = normalize_state_rel_path(source).map_err(|_| Error::config(stage, "invalid path"))?;
    let raw = state_fs
        .read(&rel)?
        .ok_or_else(|| Error::config(stage, "file not found"))?;
    if raw.len() > MAX_LOCAL_RAW_BYTES {
        return Err(Error::config(stage, "file too large"));
    }
    let sanitized = sanitize_state_file_read(&rel, &raw, stage)?;

    let document = build_local_document(source, sanitized.as_ref(), max_chars, stage)?;
    log::info!(
        "[{}] read path={} kind={} raw_bytes={} content_chars={}",
        TAG,
        source,
        document.kind,
        raw.len(),
        document.content.chars().count()
    );
    Ok(document)
}

fn read_url_document(
    source: &str,
    max_chars: usize,
    stage: &'static str,
    ctx: &mut dyn ToolContext,
) -> Result<ReadableDocument> {
    match read_url_document_result(source, max_chars, stage, ctx)? {
        UrlDocumentReadResult::Document(document) => Ok(document),
        UrlDocumentReadResult::Unsupported(message) => Err(Error::config(stage, message)),
    }
}

fn read_local_document_outcome(
    state_fs: &(dyn crate::StateFs + Send + Sync),
    source: &str,
    max_chars: usize,
    stage: &'static str,
) -> Result<ToolExecutionOutcome> {
    let rel = match normalize_state_rel_path(source) {
        Ok(value) => value,
        Err(_) => return missing_local_source_outcome(source),
    };
    let Some(raw) = state_fs.read(&rel)? else {
        return missing_local_source_outcome(source);
    };
    if raw.len() > MAX_LOCAL_RAW_BYTES {
        return unsupported_document_outcome(source, "file too large");
    }
    let sanitized = sanitize_state_file_read(&rel, &raw, stage)?;
    let document = build_local_document(source, sanitized.as_ref(), max_chars, stage)?;
    log::info!(
        "[{}] read path={} kind={} raw_bytes={} content_chars={}",
        TAG,
        source,
        document.kind,
        raw.len(),
        document.content.chars().count()
    );
    Ok(ToolExecutionOutcome::text(document.to_value().to_string()))
}

fn read_url_document_outcome(
    source: &str,
    max_chars: usize,
    stage: &'static str,
    ctx: &mut dyn ToolContext,
) -> Result<ToolExecutionOutcome> {
    match crate::orchestrator::can_execute_tool_for_channel_pub(
        "document_read",
        true,
        ctx.current_channel().unwrap_or(""),
    ) {
        ToolDecision::Allow => {}
        ToolDecision::Deny { reason } => {
            return unsupported_document_outcome(source, &format!("tool denied: {reason}"));
        }
    }
    match read_url_document_result(source, max_chars, stage, ctx)? {
        UrlDocumentReadResult::Document(document) => {
            Ok(ToolExecutionOutcome::text(document.to_value().to_string()))
        }
        UrlDocumentReadResult::Unsupported(message) => {
            unsupported_document_outcome(source, message)
        }
    }
}

enum UrlDocumentReadResult {
    Document(ReadableDocument),
    Unsupported(&'static str),
}

fn read_url_document_result(
    source: &str,
    max_chars: usize,
    stage: &'static str,
    ctx: &mut dyn ToolContext,
) -> Result<UrlDocumentReadResult> {
    if url_looks_like_pdf(source) {
        let args = json!({
            "url": source,
            "max_chars": max_chars,
        })
        .to_string();
        let payload = PdfReadTool
            .execute(&args, ctx)
            .map_err(|err| err.with_stage(stage))?;
        return parse_remote_document_result(source, "pdf", payload, stage)
            .map(UrlDocumentReadResult::Document);
    }

    match fetch_public_page_payload(source, max_chars, stage, ctx)? {
        WebFetchPrepared::Page(payload) => Ok(UrlDocumentReadResult::Document(ReadableDocument {
            source: payload.url,
            kind: payload.kind,
            title: payload.title,
            content: payload.content,
            truncated: payload.truncated,
            raw_bytes: payload.raw_bytes,
            warning: None,
        })),
        WebFetchPrepared::PdfDetected => {
            let args = json!({
                "url": source,
                "max_chars": max_chars,
            })
            .to_string();
            let payload = PdfReadTool
                .execute(&args, ctx)
                .map_err(|inner| inner.with_stage(stage))?;
            parse_remote_document_result(source, "pdf", payload, stage)
                .map(UrlDocumentReadResult::Document)
        }
        WebFetchPrepared::Unsupported(message) => Ok(UrlDocumentReadResult::Unsupported(message)),
    }
}

fn missing_source_outcome() -> Result<ToolExecutionOutcome> {
    Ok(
        ToolExecutionOutcome::text(empty_document_payload("", "document_read: missing source"))
            .with_blocker(ToolExecutionBlocker::needs_user_facts(
                "还需要提供要读取的 URL 或存储路径。",
                vec!["source".to_string()],
                vec![ToolClarificationField {
                    key: "source".to_string(),
                    label: "Document source".to_string(),
                    description:
                        "Provide a public http(s) URL or a storage path such as docs/readme.md."
                            .to_string(),
                    required: true,
                    secret: false,
                    multiple: false,
                    options: Vec::new(),
                }],
            )),
    )
}

fn missing_local_source_outcome(source: &str) -> Result<ToolExecutionOutcome> {
    Ok(ToolExecutionOutcome::text(empty_document_payload(
        source,
        "document_read: file not found",
    ))
    .with_blocker(ToolExecutionBlocker::needs_user_facts(
        "指定的文档路径不存在；请提供正确路径。",
        vec!["source".to_string()],
        vec![ToolClarificationField {
            key: "source".to_string(),
            label: "Document source".to_string(),
            description:
                "Provide a valid storage path such as docs/readme.md or switch to a public URL."
                    .to_string(),
            required: true,
            secret: false,
            multiple: false,
            options: Vec::new(),
        }],
    )))
}

fn unsupported_document_outcome(source: &str, warning: &str) -> Result<ToolExecutionOutcome> {
    Ok(
        ToolExecutionOutcome::text(empty_document_payload(source, warning))
            .with_blocker(ToolExecutionBlocker::unsupported(warning)),
    )
}

fn empty_document_payload(source: &str, warning: &str) -> String {
    json!({
        "source": source,
        "kind": Value::Null,
        "title": Value::Null,
        "content": "",
        "truncated": false,
        "raw_bytes": 0,
        "warning": warning,
    })
    .to_string()
}

fn parse_remote_document_result(
    source: &str,
    fallback_kind: &str,
    payload: String,
    stage: &'static str,
) -> Result<ReadableDocument> {
    let parsed: Value = serde_json::from_str(&payload).map_err(|err| Error::Other {
        source: Box::new(err),
        stage,
    })?;
    let content = parsed
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::config(stage, "missing content in document payload"))?;
    Ok(ReadableDocument {
        source: parsed
            .get("source")
            .or_else(|| parsed.get("url"))
            .and_then(Value::as_str)
            .unwrap_or(source)
            .to_string(),
        kind: parsed
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or(fallback_kind)
            .to_string(),
        title: parsed
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_string),
        content: content.to_string(),
        truncated: parsed
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        raw_bytes: parsed.get("raw_bytes").and_then(Value::as_u64).unwrap_or(0) as usize,
        warning: parsed
            .get("warning")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn build_local_document(
    source: &str,
    raw: &[u8],
    max_chars: usize,
    stage: &'static str,
) -> Result<ReadableDocument> {
    let DecodedReadableDocument {
        kind,
        content,
        truncated,
        raw_bytes,
        warning,
    } = decode_readable_document(source, raw, max_chars, stage)?;
    Ok(ReadableDocument {
        source: source.to_string(),
        kind,
        title: None,
        content,
        truncated,
        raw_bytes,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_local_document, DocumentReadTool};
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::pdf_read::test_pdf_fixture_bytes;
    use crate::tools::{Tool, ToolContext, ToolExecutionBlockerKind};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl crate::StateFs for MockStateFs {
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

    struct MockToolContext {
        status: u16,
        body: Vec<u8>,
    }

    impl ToolContext for MockToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            Ok((self.status, ResponseBody::Heap(self.body.clone())))
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
            None
        }

        fn current_channel(&self) -> Option<&str> {
            None
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn local_html_is_cleaned() {
        let payload = build_local_document(
            "docs/index.html",
            br#"<html><body><h1>Hello</h1><script>bad()</script></body></html>"#,
            1_000,
            "tool_document_read",
        )
        .unwrap();
        assert_eq!(payload.kind, "html");
        assert!(payload.content.contains("Hello"));
        assert!(!payload.content.contains("bad()"));
    }

    #[test]
    fn local_pdf_is_extracted() {
        let pdf = test_pdf_fixture_bytes();
        let payload =
            build_local_document("docs/test.pdf", &pdf, 1_000, "tool_document_read").unwrap();
        assert_eq!(payload.kind, "pdf");
        assert!(payload.content.contains("Hello PDF"));
    }

    #[test]
    fn empty_local_text_returns_warning_instead_of_error() {
        let payload =
            build_local_document("docs/empty.txt", b"  \n\t", 1_000, "tool_document_read").unwrap();
        assert_eq!(payload.kind, "text");
        assert!(payload.content.is_empty());
        assert_eq!(
            payload.warning.as_deref(),
            Some(crate::documents::EMPTY_DOCUMENT_WARNING)
        );
        assert!(!payload.truncated);
    }

    #[test]
    fn execute_reads_state_file() {
        let state_fs = Arc::new(MockStateFs::default());
        state_fs
            .write("docs/readme.md", b"# Title\n\nhello beetle")
            .unwrap();
        let tool = DocumentReadTool::new(state_fs);
        let mut ctx = MockToolContext {
            status: 200,
            body: Vec::new(),
        };

        let result = tool
            .execute(r#"{"source":"docs/readme.md","max_chars":200}"#, &mut ctx)
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["kind"], "text");
        assert!(parsed["content"].as_str().unwrap().contains("hello beetle"));
    }

    #[test]
    fn execute_redacts_sensitive_values_in_config_file() {
        let state_fs = Arc::new(MockStateFs::default());
        state_fs
            .write(
                "config/channels.json",
                br#"{"tg_token":"123456:live-secret","wecom_bot_secret":"bot-secret","enabled_channel":"telegram"}"#,
            )
            .unwrap();
        let tool = DocumentReadTool::new(state_fs);
        let mut ctx = MockToolContext {
            status: 200,
            body: Vec::new(),
        };

        let result = tool
            .execute(
                r#"{"source":"config/channels.json","max_chars":400}"#,
                &mut ctx,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        let content = parsed["content"].as_str().unwrap();

        assert!(content.contains("[REDACTED]"));
        assert!(content.contains("\"enabled_channel\": \"telegram\""));
        assert!(!content.contains("123456:live-secret"));
        assert!(!content.contains("bot-secret"));
    }

    #[test]
    fn execute_reads_pdf_url_via_unified_entry() {
        let tool = DocumentReadTool::new(Arc::new(MockStateFs::default()));
        let mut ctx = MockToolContext {
            status: 200,
            body: test_pdf_fixture_bytes(),
        };

        let result = tool
            .execute(
                r#"{"source":"https://example.com/report.pdf","max_chars":200}"#,
                &mut ctx,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["content"].as_str().unwrap().contains("Hello PDF"));
    }

    #[test]
    fn llm_visibility_prefers_unified_entry() {
        let authority = crate::tools::build_default_llm_catalog_authority();
        let document_read = authority
            .get("document_read")
            .expect("document_read catalog");
        let web_fetch = authority.get("web_fetch").expect("web_fetch catalog");
        let pdf_read = authority.get("pdf_read").expect("pdf_read catalog");

        assert!(document_read.user_llm);
        assert!(document_read.system_llm);
        assert!(!document_read.internal_system_llm);
        assert!(!web_fetch.user_llm);
        assert!(!pdf_read.user_llm);
    }

    #[test]
    fn execute_outcome_requires_source_blocker() {
        let tool = DocumentReadTool::new(Arc::new(MockStateFs::default()));
        let mut ctx = MockToolContext {
            status: 200,
            body: Vec::new(),
        };

        let outcome = tool.execute_outcome(r#"{}"#, &mut ctx).unwrap();
        let blocker = outcome.blocker.expect("source blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert_eq!(blocker.missing_fields, vec!["source".to_string()]);
        assert_eq!(blocker.clarification_fields[0].key, "source");
    }

    #[test]
    fn execute_outcome_reports_missing_local_path_as_blocker() {
        let tool = DocumentReadTool::new(Arc::new(MockStateFs::default()));
        let mut ctx = MockToolContext {
            status: 200,
            body: Vec::new(),
        };

        let outcome = tool
            .execute_outcome(r#"{"source":"docs/missing.md"}"#, &mut ctx)
            .unwrap();
        let blocker = outcome.blocker.expect("path blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::NeedsUserFacts);
        assert_eq!(blocker.missing_fields, vec!["source".to_string()]);
        assert_eq!(blocker.clarification_fields[0].key, "source");
    }

    #[test]
    fn execute_outcome_blocks_private_urls() {
        let tool = DocumentReadTool::new(Arc::new(MockStateFs::default()));
        let mut ctx = MockToolContext {
            status: 200,
            body: Vec::new(),
        };

        let outcome = tool
            .execute_outcome(r#"{"source":"http://127.0.0.1/private"}"#, &mut ctx)
            .unwrap();
        let blocker = outcome.blocker.expect("private url blocker");
        assert_eq!(blocker.kind, ToolExecutionBlockerKind::Unsupported);
    }
}
