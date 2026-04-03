//! web_fetch 工具：抓取网页并返回适合 LLM 阅读的正文文本。
//! web_fetch tool: fetch a public page and return cleaned text for the LLM.

use crate::error::{Error, Result};
use crate::tools::http_request::is_private_url;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde_json::{json, Value};

const TAG: &str = "tools::web_fetch";
const USER_AGENT: &str = "Beetle/0.1 web_fetch";
const DEFAULT_MAX_CHARS: usize = 12_000;
const MAX_OUTPUT_CHARS: usize = 50_000;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

pub struct WebFetchTool;

impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch a public web page and return cleaned readable text. Best used after web_search with one of the returned HTML/text URLs. For PDF documents, use pdf_read instead."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"url":{"type":"string","description":"Public http(s) URL to fetch"},"max_chars":{"type":"integer","description":"Maximum characters to return (default 12000, max 50000)"}},"required":["url"]}"#
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::debug()
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_web_fetch")?;
        let url = obj
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_web_fetch", "missing url"))?;
        let max_chars = parse_max_chars(obj.get("max_chars"), DEFAULT_MAX_CHARS);

        if is_private_url(url) {
            return Err(Error::config(
                "tool_web_fetch",
                "private/internal URLs are blocked for security",
            ));
        }

        let headers = [("User-Agent", USER_AGENT)];
        let (status, body) = ctx
            .get_with_headers(url, &headers)
            .map_err(|err| err.with_stage("tool_web_fetch"))?;
        if !(200..300).contains(&status) {
            return Err(Error::http("tool_web_fetch", status));
        }

        let raw = body.as_ref();
        let raw_len = raw.len();
        let truncated_raw = raw_len > MAX_RESPONSE_BYTES;
        let bounded = if truncated_raw {
            &raw[..MAX_RESPONSE_BYTES]
        } else {
            raw
        };

        if looks_like_pdf(bounded) {
            return Err(Error::config(
                "tool_web_fetch",
                "PDF detected; use pdf_read for document extraction",
            ));
        }

        if looks_like_binary(bounded) {
            return Err(Error::config(
                "tool_web_fetch",
                "response looks like binary data; only text-like pages are supported",
            ));
        }

        let decoded = String::from_utf8_lossy(bounded);
        let trimmed = decoded.trim();
        let (kind, title, content) = if looks_like_json(url, trimmed) {
            (
                "json",
                None,
                format_json_text(trimmed).unwrap_or_else(|| trimmed.to_string()),
            )
        } else if looks_like_html(url, trimmed) {
            let html = trimmed.to_string();
            let title = extract_title(&html);
            ("html", title, html_to_text(&html))
        } else {
            ("text", None, trimmed.to_string())
        };

        let cleaned = normalize_text(&content);
        if cleaned.is_empty() {
            return Err(Error::config(
                "tool_web_fetch",
                "page did not contain readable text",
            ));
        }

        let (content, truncated_chars) = truncate_chars(&cleaned, max_chars);
        log::info!(
            "[{}] fetched url={} kind={} raw_bytes={} content_chars={}",
            TAG,
            url,
            kind,
            raw_len,
            cleaned.chars().count()
        );

        Ok(json!({
            "url": url,
            "kind": kind,
            "title": title,
            "content": content,
            "truncated": truncated_raw || truncated_chars,
            "raw_bytes": raw_len,
        })
        .to_string())
    }
}

pub(crate) fn parse_max_chars(value: Option<&Value>, default_max_chars: usize) -> usize {
    value
        .and_then(Value::as_u64)
        .map(|raw| raw.clamp(1, MAX_OUTPUT_CHARS as u64) as usize)
        .unwrap_or(default_max_chars)
}

pub(crate) fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), false);
    }
    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("\n\n... [truncated] ...");
    (out, true)
}

fn looks_like_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut control = 0usize;
    for &b in bytes.iter().take(512) {
        if b == 0 {
            return true;
        }
        if b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t' | 0x0c) {
            control += 1;
        }
    }
    control * 10 > bytes.len().min(512)
}

pub(crate) fn looks_like_pdf(bytes: &[u8]) -> bool {
    bytes.windows(5).take(1024).any(|window| window == b"%PDF-")
}

pub(crate) fn looks_like_json(url: &str, text: &str) -> bool {
    url.rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        || text.starts_with('{')
        || text.starts_with('[')
}

pub(crate) fn looks_like_html(url: &str, text: &str) -> bool {
    let lower = text
        .chars()
        .take(2048)
        .collect::<String>()
        .to_ascii_lowercase();
    url.rsplit('.')
        .next()
        .is_some_and(|ext| matches!(ext, "html" | "htm" | "xhtml"))
        || lower.contains("<!doctype html")
        || lower.contains("<html")
        || lower.contains("<head")
        || lower.contains("<body")
        || lower.contains("<article")
        || lower.contains("<div")
        || lower.contains("<p>")
}

pub(crate) fn format_json_text(text: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&parsed).ok()
}

fn extract_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let after_tag = lower[start..].find('>')? + start + 1;
    let end = lower[after_tag..].find("</title>")? + after_tag;
    let raw = html.get(after_tag..end)?.trim();
    let decoded = decode_html_entities(raw);
    let normalized = normalize_text(&decoded);
    (!normalized.is_empty()).then_some(normalized)
}

pub(crate) fn html_to_text(html: &str) -> String {
    let no_blocks = ["script", "style", "noscript", "svg"]
        .into_iter()
        .fold(html.to_string(), |acc, tag| remove_tag_block(&acc, tag));
    let no_comments = remove_html_comments(&no_blocks);
    let mut out = String::with_capacity(no_comments.len());
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in no_comments.chars() {
        if in_tag {
            if ch == '>' {
                maybe_push_tag_break(&mut out, &tag);
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }
        if ch == '<' {
            in_tag = true;
            continue;
        }
        out.push(ch);
    }
    decode_html_entities(&out)
}

fn remove_tag_block(input: &str, tag: &str) -> String {
    let mut source = input.to_string();
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    loop {
        let lower = source.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else {
            break;
        };
        let Some(open_end_rel) = source[start..].find('>') else {
            source.truncate(start);
            break;
        };
        let open_end = start + open_end_rel + 1;
        let Some(close_rel) = lower[open_end..].find(&close) else {
            source.truncate(start);
            break;
        };
        let close_end = open_end + close_rel + close.len();
        source.replace_range(start..close_end, "\n");
    }
    source
}

fn remove_html_comments(input: &str) -> String {
    let mut source = input.to_string();
    loop {
        let Some(start) = source.find("<!--") else {
            break;
        };
        let Some(end_rel) = source[start + 4..].find("-->") else {
            source.truncate(start);
            break;
        };
        let end = start + 4 + end_rel + 3;
        source.replace_range(start..end, "");
    }
    source
}

fn maybe_push_tag_break(out: &mut String, raw_tag: &str) {
    let tag = raw_tag
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let is_break = matches!(
        tag.as_str(),
        "br" | "p"
            | "div"
            | "section"
            | "article"
            | "main"
            | "header"
            | "footer"
            | "li"
            | "ul"
            | "ol"
            | "tr"
            | "table"
            | "pre"
            | "blockquote"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    );
    if is_break && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn decode_html_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < chars.len() && end.saturating_sub(i) <= 12 && chars[end] != ';' {
            end += 1;
        }
        if end >= chars.len() || chars[end] != ';' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let entity: String = chars[i + 1..end].iter().collect();
        if let Some(decoded) = decode_entity(&entity) {
            out.push(decoded);
            i = end + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "#39" | "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            if let Some(rest) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
            {
                u32::from_str_radix(rest, 16).ok().and_then(char::from_u32)
            } else if let Some(rest) = entity.strip_prefix('#') {
                rest.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

pub(crate) fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut pending_newlines = 0usize;
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
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
mod tests {
    use super::{html_to_text, normalize_text, WebFetchTool};
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

    #[test]
    fn html_to_text_strips_tags_and_scripts() {
        let html = r#"
            <html>
              <head><title>Example &amp; Demo</title><style>.x{}</style></head>
              <body>
                <script>alert(1)</script>
                <h1>Hello</h1>
                <p>World&nbsp;now</p>
              </body>
            </html>
        "#;
        let text = normalize_text(&html_to_text(html));
        assert!(text.contains("Example & Demo"));
        assert!(text.contains("Hello"));
        assert!(text.contains("World now"));
        assert!(!text.contains("alert(1)"));
    }

    #[test]
    fn execute_returns_cleaned_html_content() {
        let tool = WebFetchTool;
        let mut ctx = MockToolContext {
            status: 200,
            body: br#"
                <html>
                  <head><title>Example</title></head>
                  <body><article><p>Hello <b>world</b>.</p></article></body>
                </html>
            "#
            .to_vec(),
        };

        let result = tool
            .execute(r#"{"url":"https://example.com/page"}"#, &mut ctx)
            .unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["kind"], "html");
        assert_eq!(parsed["title"], "Example");
        assert!(parsed["content"].as_str().unwrap().contains("Hello world."));
    }

    #[test]
    fn execute_rejects_pdf_and_redirects_to_pdf_read() {
        let tool = WebFetchTool;
        let mut ctx = MockToolContext {
            status: 200,
            body: b"%PDF-1.4 sample".to_vec(),
        };

        let err = tool
            .execute(r#"{"url":"https://example.com/report.pdf"}"#, &mut ctx)
            .unwrap_err();
        assert!(format!("{err}").contains("use pdf_read"));
    }
}
