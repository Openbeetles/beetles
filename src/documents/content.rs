use crate::error::{Error, Result};

pub const EMPTY_DOCUMENT_WARNING: &str = "document is empty or contains no readable text";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedReadableDocument {
    pub kind: String,
    pub content: String,
    pub truncated: bool,
    pub raw_bytes: usize,
    pub warning: Option<String>,
}

pub fn decode_readable_document(
    source: &str,
    raw: &[u8],
    max_chars: usize,
    stage: &'static str,
) -> Result<DecodedReadableDocument> {
    if looks_like_pdf(raw) {
        let normalized = normalize_text(&extract_pdf_text(raw, stage)?);
        let (content, truncated) = truncate_chars(&normalized, max_chars);
        return Ok(DecodedReadableDocument {
            kind: "pdf".to_string(),
            content,
            truncated,
            raw_bytes: raw.len(),
            warning: if normalized.is_empty() {
                Some(
                    "PDF contains no extractable text (may be image-only or encrypted)"
                        .to_string(),
                )
            } else {
                None
            },
        });
    }

    let decoded =
        std::str::from_utf8(raw).map_err(|_| Error::config(stage, "file is not valid UTF-8"))?;
    let trimmed = decoded.trim();
    let (kind, content) = if looks_like_json(source, trimmed) {
        (
            "json",
            format_json_text(trimmed).unwrap_or_else(|| trimmed.to_string()),
        )
    } else if looks_like_html(source, trimmed) {
        ("html", html_to_text(trimmed))
    } else {
        ("text", trimmed.to_string())
    };
    let normalized = normalize_text(&content);
    let (content, truncated) = truncate_chars(&normalized, max_chars);
    Ok(DecodedReadableDocument {
        kind: kind.to_string(),
        content,
        truncated,
        raw_bytes: raw.len(),
        warning: normalized
            .is_empty()
            .then(|| EMPTY_DOCUMENT_WARNING.to_string()),
    })
}

pub fn decode_searchable_document_text(source: &str, raw: &[u8]) -> Option<String> {
    if looks_like_pdf(raw) {
        let text = extract_pdf_text(raw, "documents_content_search").ok()?;
        let normalized = normalize_text(&text);
        return (!normalized.is_empty()).then_some(normalized);
    }

    let decoded = std::str::from_utf8(raw).ok()?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return None;
    }

    let content = if looks_like_json(source, trimmed) {
        format_json_text(trimmed).unwrap_or_else(|| trimmed.to_string())
    } else if looks_like_html(source, trimmed) {
        html_to_text(trimmed)
    } else {
        trimmed.to_string()
    };
    let normalized = normalize_text(&content);
    (!normalized.is_empty()).then_some(normalized)
}

pub fn detect_document_kind(source: &str, raw: &[u8]) -> &'static str {
    if looks_like_pdf(raw) {
        "pdf"
    } else if std::str::from_utf8(raw)
        .ok()
        .is_some_and(|decoded| looks_like_json(source, decoded.trim()))
    {
        "json"
    } else if std::str::from_utf8(raw)
        .ok()
        .is_some_and(|decoded| looks_like_html(source, decoded.trim()))
    {
        "html"
    } else if std::str::from_utf8(raw).is_ok() {
        "text"
    } else {
        "binary"
    }
}

pub fn contains_query_text(haystack: &str, needle: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        haystack.contains(needle)
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

pub fn build_search_snippet(text: &str, query: &str, case_sensitive: bool) -> String {
    const SNIPPET_RADIUS_CHARS: usize = 80;

    if text.is_empty() {
        return String::new();
    }

    let match_offset = if case_sensitive {
        text.find(query)
    } else {
        text.to_lowercase().find(&query.to_lowercase())
    };
    let Some(start) = match_offset else {
        return source_head(text, SNIPPET_RADIUS_CHARS);
    };
    let end = start.saturating_add(query.len()).min(text.len());
    let (snippet, clipped_left, clipped_right) =
        char_window(text, start, end, SNIPPET_RADIUS_CHARS);
    let mut out = String::new();
    if clipped_left {
        out.push_str("...");
    }
    out.push_str(snippet.trim());
    if clipped_right {
        out.push_str("...");
    }
    out
}

fn source_head(text: &str, radius: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= radius * 2 {
        text.to_string()
    } else {
        let end = byte_index_at_char(text, radius * 2);
        format!("{}...", text[..end].trim())
    }
}

fn char_window(text: &str, match_start: usize, match_end: usize, radius: usize) -> (&str, bool, bool) {
    let start_char = text[..match_start].chars().count();
    let end_char = text[..match_end.min(text.len())].chars().count();
    let snippet_start_char = start_char.saturating_sub(radius);
    let snippet_end_char = (end_char + radius).min(text.chars().count());
    let snippet_start = byte_index_at_char(text, snippet_start_char);
    let snippet_end = byte_index_at_char(text, snippet_end_char);
    (
        &text[snippet_start..snippet_end],
        snippet_start_char > 0,
        snippet_end_char < text.chars().count(),
    )
}

fn byte_index_at_char(text: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), false);
    }
    let mut out = text.chars().take(max_chars).collect::<String>();
    out.push_str("\n\n... [truncated] ...");
    (out, true)
}

fn looks_like_json(source: &str, text: &str) -> bool {
    source
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        || text.starts_with('{')
        || text.starts_with('[')
}

fn looks_like_html(source: &str, text: &str) -> bool {
    let lower = text
        .chars()
        .take(2048)
        .collect::<String>()
        .to_ascii_lowercase();
    source
        .rsplit('.')
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

fn looks_like_pdf(bytes: &[u8]) -> bool {
    bytes.windows(5).take(1024).any(|window| window == b"%PDF-")
}

fn format_json_text(text: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    serde_json::to_string_pretty(&parsed).ok()
}

fn html_to_text(html: &str) -> String {
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

fn maybe_push_tag_break(out: &mut String, tag: &str) {
    let trimmed = tag.trim_start_matches('/').trim();
    let lower = trimmed
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "p" | "br" | "div" | "section" | "article" | "header" | "footer" | "li"
            | "ul" | "ol" | "table" | "tr" | "td" | "th" | "h1" | "h2" | "h3"
            | "h4" | "h5" | "h6"
    ) && !out.ends_with('\n')
    {
        out.push('\n');
    }
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn normalize_text(text: &str) -> String {
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

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn extract_pdf_text(raw: &[u8], stage: &'static str) -> Result<String> {
    let start = raw
        .windows(5)
        .take(1024)
        .position(|window| window == b"%PDF-")
        .unwrap_or(0);
    pdf_extract::extract_text_from_mem(&raw[start..])
        .map_err(|error| Error::config(stage, format!("PDF extraction failed: {error}")))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn extract_pdf_text(_raw: &[u8], stage: &'static str) -> Result<String> {
    Err(Error::config(
        stage,
        "PDF extraction is unavailable on this target",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_document_cleans_html() {
        let decoded = decode_readable_document(
            "docs/index.html",
            br#"<html><body><h1>Hello</h1><script>bad()</script></body></html>"#,
            200,
            "documents_content_test",
        )
        .expect("decode html");
        assert_eq!(decoded.kind, "html");
        assert!(decoded.content.contains("Hello"));
        assert!(!decoded.content.contains("bad()"));
    }

    #[test]
    fn searchable_document_formats_json() {
        let decoded = decode_searchable_document_text(
            "docs/test.json",
            br#"{"name":"beetle","ok":true}"#,
        )
        .expect("searchable json");
        assert!(decoded.contains("\"name\": \"beetle\""));
    }

    #[test]
    fn search_snippet_focuses_on_match() {
        let snippet = build_search_snippet(
            "alpha beta gamma delta epsilon zeta eta theta iota",
            "delta",
            true,
        );
        assert!(snippet.contains("delta"));
    }
}
