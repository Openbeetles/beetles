//! pdf_read 工具：抓取公网 PDF 并抽取可读文本，供 LLM 继续分析。
//! pdf_read tool: fetch a public PDF and extract readable text for the LLM.

use crate::error::{Error, Result};
use crate::tools::http_request::is_private_url;
use crate::tools::web_fetch::{parse_max_chars, truncate_chars};
use crate::tools::{Tool, ToolContext, ToolMetadata, parse_tool_args};
use serde_json::{Value, json};

const TAG: &str = "tools::pdf_read";
const USER_AGENT: &str = "Beetle/0.1 pdf_read";
const DEFAULT_MAX_CHARS: usize = 16_000;

pub struct PdfReadTool;

impl Tool for PdfReadTool {
    fn name(&self) -> &'static str {
        "pdf_read"
    }

    fn description(&self) -> &'static str {
        "Fetch a public PDF URL and extract readable text. Use this for PDF links returned by web_search."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"url":{"type":"string","description":"Public PDF URL to fetch"},"max_chars":{"type":"integer","description":"Maximum characters to return (default 16000, max 50000)"}},"required":["url"]}"#
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::debug()
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_pdf_read")?;
        let url = obj
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_pdf_read", "missing url"))?;
        let max_chars = parse_max_chars(obj.get("max_chars"), DEFAULT_MAX_CHARS);

        if is_private_url(url) {
            return Err(Error::config(
                "tool_pdf_read",
                "private/internal URLs are blocked for security",
            ));
        }

        let headers = [
            ("User-Agent", USER_AGENT),
            (
                "Accept",
                "application/pdf, application/octet-stream;q=0.9, */*;q=0.1",
            ),
        ];
        let (status, body) = ctx
            .get_with_headers(url, &headers)
            .map_err(|err| err.with_stage("tool_pdf_read"))?;
        if !(200..300).contains(&status) {
            return Err(Error::http("tool_pdf_read", status));
        }

        let raw = body.as_ref();
        let raw_len = raw.len();
        if !looks_like_pdf(raw) {
            return Err(Error::config(
                "tool_pdf_read",
                "response does not look like a PDF document",
            ));
        }

        let normalized = normalize_pdf_text(&extract_pdf_text(raw).map_err(|err| {
            let budget = crate::orchestrator::current_budget().response_body_max;
            let detail = if raw_len >= budget {
                format!(
                    "PDF extraction failed (document may be truncated by response budget): {err}"
                )
            } else {
                format!("PDF extraction failed: {err}")
            };
            Error::config("tool_pdf_read", detail)
        })?);

        if normalized.is_empty() {
            return Ok(json!({
                "url": url,
                "content": "",
                "truncated": false,
                "raw_bytes": raw_len,
                "warning": "PDF contains no extractable text (may be image-only or encrypted)",
            })
            .to_string());
        }

        let (content, truncated) = truncate_chars(&normalized, max_chars);
        log::info!(
            "[{}] fetched url={} raw_bytes={} content_chars={}",
            TAG,
            url,
            raw_len,
            normalized.chars().count()
        );
        Ok(json!({
            "url": url,
            "content": content,
            "truncated": truncated,
            "raw_bytes": raw_len,
            "warning": Value::Null,
        })
        .to_string())
    }
}

pub(crate) fn looks_like_pdf(bytes: &[u8]) -> bool {
    pdf_start_offset(bytes).is_some()
}

fn pdf_start_offset(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(5)
        .take(1024)
        .position(|window| window == b"%PDF-")
}

pub(crate) fn extract_pdf_text(
    bytes: &[u8],
) -> std::result::Result<String, pdf_extract::OutputError> {
    let start = pdf_start_offset(bytes).unwrap_or(0);
    pdf_extract::extract_text_from_mem(&bytes[start..])
}

pub(crate) fn normalize_pdf_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut pending_newlines = 0usize;
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' | '\u{0c}' => {
                pending_space = false;
                pending_newlines = pending_newlines.saturating_add(1).min(2);
            }
            ch if ch.is_whitespace() => {
                if !out.is_empty() {
                    pending_space = true;
                }
            }
            ch => {
                if pending_newlines > 0 {
                    if !out.is_empty() {
                        for _ in 0..pending_newlines {
                            out.push('\n');
                        }
                    }
                    pending_newlines = 0;
                } else if pending_space && !out.ends_with(' ') && !out.ends_with('\n') {
                    out.push(' ');
                }
                pending_space = false;
                out.push(ch);
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
pub(crate) fn test_pdf_fixture_bytes() -> Vec<u8> {
    use base64::Engine;

    const PDF_FIXTURE_B64: &str = "JVBERi0xLjQKJeLjz9MKMSAwIG9iajw8L1R5cGUvQ2F0YWxvZy9QYWdlcyAyIDAgUj4+ZW5kb2JqCjIgMCBvYmo8PC9UeXBlL1BhZ2VzL0tpZHNbMyAwIFJdL0NvdW50IDE+PmVuZG9iagozIDAgb2JqPDwvVHlwZS9QYWdlL01lZGlhQm94WzAgMCA2MTIgNzkyXS9QYXJlbnQgMiAwIFIvQ29udGVudHMgNSAwIFIvUmVzb3VyY2VzPDwvRm9udDw8L0YxIDQgMCBSPj4+Pj4+ZW5kb2JqCjQgMCBvYmo8PC9UeXBlL0ZvbnQvU3VidHlwZS9UeXBlMS9CYXNlRm9udC9IZWx2ZXRpY2E+PmVuZG9iago1IDAgb2JqPDwvTGVuZ3RoIDQxPj5zdHJlYW0KQlQgL0YxIDI0IFRmIDEwMCA3MDAgVGQgKEhlbGxvIFBERikgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgNgowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMTUgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTA3IDAwMDAwIG4gCjAwMDAwMDAyMTcgMDAwMDAgbiAKMDAwMDAwMDI3OCAwMDAwMCBuIAp0cmFpbGVyPDwvU2l6ZSA2L1Jvb3QgMSAwIFI+PgpzdGFydHhyZWYKMzY1CiUlRU9GCg==";
    base64::engine::general_purpose::STANDARD
        .decode(PDF_FIXTURE_B64)
        .expect("embedded PDF fixture must decode")
}

#[cfg(test)]
mod tests {
    use super::{PdfReadTool, normalize_pdf_text};
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::ResponseBody;
    use crate::tools::{Tool, ToolContext};
    use serde_json::Value;

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

    fn minimal_pdf_bytes() -> Vec<u8> {
        super::test_pdf_fixture_bytes()
    }

    #[test]
    fn normalize_pdf_text_collapses_whitespace() {
        assert_eq!(normalize_pdf_text("A \n\n B \u{0c} C"), "A\n\nB\nC");
    }

    #[test]
    fn execute_extracts_text_from_pdf() {
        let tool = PdfReadTool;
        let mut ctx = MockToolContext {
            status: 200,
            body: minimal_pdf_bytes(),
        };

        let result = tool
            .execute(
                r#"{"url":"https://example.com/doc.pdf","max_chars":200}"#,
                &mut ctx,
            )
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert!(parsed["content"].as_str().unwrap().contains("Hello PDF"));
        assert_eq!(parsed["truncated"], false);
        assert!(parsed["warning"].is_null());
    }

    #[test]
    fn execute_rejects_non_pdf_response() {
        let tool = PdfReadTool;
        let mut ctx = MockToolContext {
            status: 200,
            body: b"not a pdf".to_vec(),
        };
        let err = tool
            .execute(r#"{"url":"https://example.com/doc.pdf"}"#, &mut ctx)
            .unwrap_err();
        assert!(format!("{err}").contains("response does not look like a PDF"));
    }
}
