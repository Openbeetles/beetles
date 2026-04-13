//! document_read 工具：统一读取公网 URL 或状态根文档路径，返回适合 LLM 阅读的文本。
//! document_read tool: read a public URL or state-root document path and return LLM-friendly text.

use crate::error::{Error, Result};
use crate::documents::{decode_readable_document, DecodedReadableDocument};
use crate::orchestrator::ToolDecision;
use crate::tools::state_file_guard::sanitize_state_file_read;
use crate::tools::web_fetch::{parse_max_chars, WebFetchTool};
use crate::tools::{parse_tool_args, PdfReadTool, Tool, ToolContext};
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
        let obj = parse_tool_args(args, "tool_document_read")?;
        let source = obj
            .get("source")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_document_read", "missing source"))?;
        let max_chars = parse_max_chars(obj.get("max_chars"), DEFAULT_DOCUMENT_MAX_CHARS);
        read_document_source(
            self.state_fs.as_ref(),
            source,
            max_chars,
            self.name(),
            "tool_document_read",
            ctx,
        )
        .map(|doc| doc.to_value().to_string())
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
        match crate::orchestrator::can_execute_tool_pub(tool_name, true) {
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
    let args = json!({
        "url": source,
        "max_chars": max_chars,
    })
    .to_string();
    if url_looks_like_pdf(source) {
        let payload = PdfReadTool
            .execute(&args, ctx)
            .map_err(|err| err.with_stage(stage))?;
        return parse_remote_document_result(source, "pdf", payload, stage);
    }
    match WebFetchTool.execute(&args, ctx) {
        Ok(payload) => parse_remote_document_result(source, "text", payload, stage),
        Err(err) if format!("{err}").contains("use pdf_read") => {
            let payload = PdfReadTool
                .execute(&args, ctx)
                .map_err(|inner| inner.with_stage(stage))?;
            parse_remote_document_result(source, "pdf", payload, stage)
        }
        Err(err) => Err(err.with_stage(stage)),
    }
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
    use crate::tools::PdfReadTool;
    use crate::tools::{Tool, ToolContext, ToolPolicyContext, WebFetchTool};
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
            build_local_document("config/SOUL.md", b"  \n\t", 1_000, "tool_document_read").unwrap();
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
                br#"{"tg_token":"123456:live-secret","wecom_corp_secret":"corp-secret","enabled_channel":"telegram"}"#,
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
        assert!(!content.contains("corp-secret"));
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
        let policy = ToolPolicyContext::new(crate::bus::IngressKind::User, "qq_channel");
        let doc_tool = DocumentReadTool::new(Arc::new(MockStateFs::default()));
        assert!(doc_tool.metadata().is_exposed_to_llm(&policy));
        assert!(!WebFetchTool.metadata().is_exposed_to_llm(&policy));
        assert!(!PdfReadTool.metadata().is_exposed_to_llm(&policy));
    }
}
