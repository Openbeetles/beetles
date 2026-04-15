use crate::documents::{DocumentsReadResult, DocumentsSummaryHandoff, DocumentsSummaryResult};
use crate::error::{Error, Result};
use std::collections::HashSet;

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
                    "PDF contains no extractable text (may be image-only or encrypted)".to_string(),
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

pub fn summarize_document_read_result(
    document: &DocumentsReadResult,
    focus: Option<&str>,
) -> DocumentsSummaryResult {
    let focus = focus.map(str::trim).unwrap_or_default().to_string();
    let content = document.content.trim();
    if content.is_empty() {
        return DocumentsSummaryResult {
            entry: document.entry.clone(),
            focus,
            summary: document
                .warning
                .clone()
                .unwrap_or_else(|| EMPTY_DOCUMENT_WARNING.to_string()),
            key_points: Vec::new(),
            action_items: Vec::new(),
            handoff: DocumentsSummaryHandoff::default(),
            truncated_source: document.truncated,
            raw_bytes: document.raw_bytes,
            warning: document.warning.clone(),
        };
    }

    let focus_ref = (!focus.is_empty()).then_some(focus.as_str());
    let mut key_points = extract_key_points(content, focus_ref);
    let action_items = extract_action_items(content);
    if key_points.is_empty() {
        key_points.push(truncate_line(content, 180));
    }
    let summary = build_summary_line(content, &key_points, focus_ref);
    let handoff = build_handoff(
        &document.entry.name,
        detect_document_title(content),
        &summary,
        &key_points,
        &action_items,
    );

    DocumentsSummaryResult {
        entry: document.entry.clone(),
        focus,
        summary,
        key_points,
        action_items,
        handoff,
        truncated_source: document.truncated,
        raw_bytes: document.raw_bytes,
        warning: document.warning.clone(),
    }
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

fn char_window(
    text: &str,
    match_start: usize,
    match_end: usize,
    radius: usize,
) -> (&str, bool, bool) {
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

fn extract_key_points(text: &str, focus: Option<&str>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut focused = Vec::new();
    let mut bullets = Vec::new();
    let mut sentences = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "... [truncated] ..." {
            continue;
        }
        if let Some(cleaned) = normalize_action_candidate(line) {
            if push_unique(&mut seen, &cleaned) {
                if matches_focus(&cleaned, focus) {
                    focused.push(cleaned);
                } else {
                    bullets.push(cleaned);
                }
            }
            continue;
        }
        if let Some(cleaned) = normalize_bullet_candidate(line) {
            if push_unique(&mut seen, &cleaned) {
                if matches_focus(&cleaned, focus) {
                    focused.push(cleaned);
                } else {
                    bullets.push(cleaned);
                }
            }
            continue;
        }
        for sentence in split_sentences(line) {
            if push_unique(&mut seen, &sentence) {
                if matches_focus(&sentence, focus) {
                    focused.push(sentence);
                } else {
                    sentences.push(sentence);
                }
            }
        }
    }
    focused
        .into_iter()
        .chain(bullets)
        .chain(sentences)
        .take(5)
        .collect()
}

fn extract_action_items(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(cleaned) = normalize_action_candidate(line) {
            if push_unique(&mut seen, &cleaned) {
                items.push(cleaned);
            }
        }
    }
    items.truncate(5);
    items
}

fn build_summary_line(text: &str, key_points: &[String], focus: Option<&str>) -> String {
    if let Some(point) = key_points
        .iter()
        .find(|point| matches_focus(point.as_str(), focus))
    {
        return truncate_line(point, 240);
    }
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "... [truncated] ..." {
            continue;
        }
        if matches_focus(line, focus) {
            return truncate_line(line, 240);
        }
    }
    key_points
        .first()
        .map(|point| truncate_line(point, 240))
        .unwrap_or_else(|| truncate_line(text, 240))
}

fn build_handoff(
    document_name: &str,
    document_title: Option<String>,
    summary: &str,
    key_points: &[String],
    action_items: &[String],
) -> DocumentsSummaryHandoff {
    let task_candidates = action_items
        .iter()
        .map(|item| truncate_line(item, 140))
        .collect::<Vec<_>>();
    let mut mail_lines = vec![format!("Document: {}", document_name), summary.to_string()];
    if let Some(title) = document_title.filter(|title| !title.eq_ignore_ascii_case(document_name)) {
        mail_lines.insert(1, format!("Topic: {}", title));
    }
    if !key_points.is_empty() {
        mail_lines.push("Key points:".to_string());
        mail_lines.extend(
            key_points
                .iter()
                .take(3)
                .map(|point| format!("- {}", point)),
        );
    }
    if !action_items.is_empty() {
        mail_lines.push("Action items:".to_string());
        mail_lines.extend(
            action_items
                .iter()
                .take(3)
                .map(|item| format!("- {}", item)),
        );
    }
    DocumentsSummaryHandoff {
        task_candidates,
        mail_brief: mail_lines.join("\n"),
    }
}

fn normalize_action_candidate(line: &str) -> Option<String> {
    let prefixes = [
        "Action:",
        "Actions:",
        "Next:",
        "Next step:",
        "TODO:",
        "Follow-up:",
        "Follow up:",
        "待办：",
        "待办:",
        "行动项：",
        "行动项:",
        "下一步：",
        "下一步:",
    ];
    for prefix in prefixes {
        if let Some(rest) = line.strip_prefix(prefix) {
            let cleaned = rest.trim();
            return (!cleaned.is_empty()).then(|| truncate_line(cleaned, 180));
        }
    }
    strip_checkbox(line).map(|value| truncate_line(value, 180))
}

fn detect_document_title(text: &str) -> Option<String> {
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "... [truncated] ..." {
            continue;
        }
        if normalize_action_candidate(line).is_some() || normalize_bullet_candidate(line).is_some()
        {
            continue;
        }
        let title = truncate_line(line, 120);
        if title.chars().count() <= 80 {
            return Some(title);
        }
    }
    None
}

fn normalize_bullet_candidate(line: &str) -> Option<String> {
    let stripped = if let Some(rest) = line.strip_prefix("- ") {
        rest
    } else if let Some(rest) = line.strip_prefix("* ") {
        rest
    } else if let Some(rest) = line.strip_prefix("• ") {
        rest
    } else if let Some(rest) = strip_ordered_prefix(line) {
        rest
    } else {
        return None;
    };
    let cleaned = stripped.trim();
    (!cleaned.is_empty()).then(|| truncate_line(cleaned, 180))
}

fn strip_checkbox(line: &str) -> Option<&str> {
    ["- [ ] ", "* [ ] ", "[ ] ", "- [x] ", "* [x] ", "[x] "]
        .into_iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn strip_ordered_prefix(line: &str) -> Option<&str> {
    let (head, tail) = line.split_once(". ")?;
    head.chars()
        .all(|ch| ch.is_ascii_digit())
        .then_some(tail.trim())
}

fn split_sentences(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；') {
            push_sentence(&mut out, &mut current);
        }
    }
    push_sentence(&mut out, &mut current);
    out.into_iter().take(5).collect()
}

fn push_sentence(out: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if trimmed.is_empty() {
        current.clear();
        return;
    }
    if trimmed.chars().count() >= 12 {
        out.push(truncate_line(trimmed, 180));
    }
    current.clear();
}

fn matches_focus(line: &str, focus: Option<&str>) -> bool {
    let Some(focus) = focus.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    contains_query_text(line, focus, false)
}

fn push_unique(seen: &mut HashSet<String>, value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    if seen.contains(&normalized) {
        return false;
    }
    seen.insert(normalized);
    true
}

fn truncate_line(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(max_chars).collect::<String>();
    out.push_str("...");
    out
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
        "p" | "br"
            | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "li"
            | "ul"
            | "ol"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
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
        let decoded =
            decode_searchable_document_text("docs/test.json", br#"{"name":"beetle","ok":true}"#)
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

    #[test]
    fn summarize_document_read_result_extracts_key_points_actions_and_handoff() {
        let summary = summarize_document_read_result(
            &DocumentsReadResult {
                entry: crate::documents::DocumentsEntry {
                    path: "Reports/q1.txt".to_string(),
                    name: "Q1 Review".to_string(),
                    kind: "text".to_string(),
                    is_dir: false,
                    content_type: Some("text/plain".to_string()),
                    size_bytes: Some(128),
                },
                content: "Q1 Review\n- Calendar bridge shipped to remote office calendar\n- Documents summary should feed weekly updates\nAction: send summary to finance\nTODO: create follow-up task for customer review".to_string(),
                truncated: false,
                raw_bytes: 128,
                warning: None,
            },
            Some("summary"),
        );

        assert_eq!(summary.focus, "summary");
        assert_eq!(
            summary.summary,
            "Documents summary should feed weekly updates"
        );
        assert_eq!(
            summary.key_points[0],
            "Documents summary should feed weekly updates"
        );
        assert!(summary
            .action_items
            .contains(&"send summary to finance".to_string()));
        assert!(summary
            .action_items
            .contains(&"create follow-up task for customer review".to_string()));
        assert!(summary
            .handoff
            .task_candidates
            .contains(&"send summary to finance".to_string()));
        assert!(summary.handoff.mail_brief.contains("Q1 Review"));
    }
}
