use crate::bus::{
    CanonicalMessageBody, CardBody, CardFormat, MessageBodyKind, PcMsg, TextBody, TextFormat,
};
use crate::channel_capability::{
    ChannelCapabilityEntry, CHANNEL_FEISHU, CHANNEL_QQ_CHANNEL, CHANNEL_TELEGRAM,
};
use serde_json::json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedOutboundMessage {
    pub(crate) msg: PcMsg,
    pub(crate) content: String,
}

pub(crate) fn prepare_outbound_message_for_channel(
    msg: &PcMsg,
    capability: Option<ChannelCapabilityEntry>,
) -> PreparedOutboundMessage {
    let Some(capability) = capability.filter(|entry| entry.enabled) else {
        return PreparedOutboundMessage {
            msg: msg.clone(),
            content: msg.content.clone(),
        };
    };

    let mut adapted = msg.clone();
    let normalized = normalize_body_for_channel(capability, &adapted.body, &msg.content);
    adapted.body = normalized.body;
    adapted.content = normalized.content.clone();
    PreparedOutboundMessage {
        msg: adapted,
        content: normalized.content,
    }
}

struct NormalizedOutboundBody {
    body: CanonicalMessageBody,
    content: String,
}

fn normalize_body_for_channel(
    capability: ChannelCapabilityEntry,
    body: &CanonicalMessageBody,
    content: &str,
) -> NormalizedOutboundBody {
    if !supports_body_kind(capability, body.kind()) {
        let plain = fallback_plain_text_for_body_with_content(body, content);
        return NormalizedOutboundBody {
            body: CanonicalMessageBody::Text(TextBody::plain(plain.clone())),
            content: plain,
        };
    }

    match body {
        CanonicalMessageBody::Text(text) => {
            normalize_text_body_for_channel(capability, text, content)
        }
        CanonicalMessageBody::Card(card) => normalize_card_body_for_channel(capability, card),
        _ => NormalizedOutboundBody {
            body: body.clone(),
            content: content.to_string(),
        },
    }
}

fn normalize_text_body_for_channel(
    capability: ChannelCapabilityEntry,
    text: &TextBody,
    content: &str,
) -> NormalizedOutboundBody {
    let source_text = normalize_text_source(text, content);
    if source_text.is_empty() {
        return NormalizedOutboundBody {
            body: CanonicalMessageBody::Text(TextBody::plain(String::new())),
            content: String::new(),
        };
    }

    match text.format {
        TextFormat::Plain => NormalizedOutboundBody {
            body: CanonicalMessageBody::Text(TextBody::plain(source_text.to_string())),
            content: source_text.to_string(),
        },
        TextFormat::Markdown if supports_text_format(capability, TextFormat::Markdown) => {
            NormalizedOutboundBody {
                body: CanonicalMessageBody::Text(TextBody {
                    text: source_text.to_string(),
                    format: TextFormat::Markdown,
                }),
                content: source_text.to_string(),
            }
        }
        TextFormat::Html if supports_text_format(capability, TextFormat::Html) => {
            NormalizedOutboundBody {
                body: CanonicalMessageBody::Text(TextBody {
                    text: source_text.to_string(),
                    format: TextFormat::Html,
                }),
                content: content.to_string(),
            }
        }
        TextFormat::RichText => normalize_rich_text_for_channel(capability, source_text),
        TextFormat::Markdown
            if capability.id == CHANNEL_TELEGRAM
                && supports_text_format(capability, TextFormat::Html) =>
        {
            NormalizedOutboundBody {
                body: CanonicalMessageBody::Text(TextBody {
                    text: render_markdownish_to_telegram_html(source_text),
                    format: TextFormat::Html,
                }),
                content: source_text.to_string(),
            }
        }
        TextFormat::Markdown
            if capability.id == CHANNEL_FEISHU
                && supports_text_format(capability, TextFormat::RichText) =>
        {
            let body = build_feishu_rich_post_body(source_text);
            let projection = body.text_projection();
            NormalizedOutboundBody {
                body,
                content: projection,
            }
        }
        TextFormat::Html => {
            let plain = render_html_to_plain_text(source_text);
            NormalizedOutboundBody {
                body: CanonicalMessageBody::Text(TextBody::plain(plain.clone())),
                content: plain,
            }
        }
        TextFormat::Markdown => {
            let plain = render_markdownish_to_plain_text(source_text);
            NormalizedOutboundBody {
                body: CanonicalMessageBody::Text(TextBody::plain(plain.clone())),
                content: plain,
            }
        }
    }
}

fn normalize_rich_text_for_channel(
    capability: ChannelCapabilityEntry,
    source_text: &str,
) -> NormalizedOutboundBody {
    if capability.id == CHANNEL_FEISHU && supports_text_format(capability, TextFormat::RichText) {
        let body = build_feishu_rich_post_body(source_text);
        let projection = body.text_projection();
        return NormalizedOutboundBody {
            body,
            content: projection,
        };
    }
    if supports_text_format(capability, TextFormat::Markdown) && looks_like_markdownish(source_text)
    {
        return NormalizedOutboundBody {
            body: CanonicalMessageBody::Text(TextBody {
                text: source_text.to_string(),
                format: TextFormat::Markdown,
            }),
            content: source_text.to_string(),
        };
    }
    let plain = render_markdownish_to_plain_text(source_text);
    NormalizedOutboundBody {
        body: CanonicalMessageBody::Text(TextBody::plain(plain.clone())),
        content: plain,
    }
}

fn normalize_card_body_for_channel(
    capability: ChannelCapabilityEntry,
    card: &CardBody,
) -> NormalizedOutboundBody {
    let supported = match capability.id {
        CHANNEL_FEISHU => matches!(card.format, CardFormat::Interactive | CardFormat::RichPost),
        CHANNEL_QQ_CHANNEL => matches!(card.format, CardFormat::Ark | CardFormat::Embed),
        _ => true,
    };
    if supported {
        return NormalizedOutboundBody {
            body: CanonicalMessageBody::Card(card.clone()),
            content: card.fallback_text.clone(),
        };
    }
    let plain = fallback_plain_text_for_body(&CanonicalMessageBody::Card(card.clone()));
    NormalizedOutboundBody {
        body: CanonicalMessageBody::Text(TextBody::plain(plain.clone())),
        content: plain,
    }
}

fn build_feishu_rich_post_body(source_text: &str) -> CanonicalMessageBody {
    let fallback_text = render_markdownish_to_plain_text(source_text);
    let mut paragraphs = Vec::new();
    for line in fallback_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        paragraphs.push(vec![json!({
            "tag": "text",
            "text": trimmed,
        })]);
    }
    if paragraphs.is_empty() {
        paragraphs.push(vec![json!({
            "tag": "text",
            "text": fallback_text.trim(),
        })]);
    }

    let title = markdownish_title(&fallback_text);
    let locale_payload = json!({
        "title": title,
        "content": paragraphs,
    });
    CanonicalMessageBody::Card(CardBody {
        format: CardFormat::RichPost,
        payload_json: json!({
            "zh_cn": locale_payload.clone(),
            "en_us": locale_payload,
        }),
        fallback_text,
    })
}

fn markdownish_title(text: &str) -> String {
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return trimmed.chars().take(64).collect();
    }
    "beetle".to_string()
}

fn fallback_plain_text_for_body(body: &CanonicalMessageBody) -> String {
    let projection = body.text_projection();
    if looks_like_markdownish(&projection) {
        render_markdownish_to_plain_text(&projection)
    } else {
        projection.trim().to_string()
    }
}

fn fallback_plain_text_for_body_with_content(body: &CanonicalMessageBody, content: &str) -> String {
    let body_fallback = fallback_plain_text_for_body(body);
    if !body_fallback.is_empty() && !is_generic_body_fallback(&body_fallback) {
        return body_fallback;
    }
    let content = content.trim();
    if !content.is_empty() {
        if looks_like_markdownish(content) {
            return render_markdownish_to_plain_text(content);
        }
        return content.to_string();
    }
    body_fallback
}

fn is_generic_body_fallback(text: &str) -> bool {
    matches!(
        text.trim(),
        "[card]" | "[platform_native]" | "[image]" | "[audio]" | "[video]" | "[file]"
    )
}

fn supports_body_kind(capability: ChannelCapabilityEntry, kind: MessageBodyKind) -> bool {
    capability.contract.supported_body_kinds.contains(&kind)
}

fn supports_text_format(capability: ChannelCapabilityEntry, format: TextFormat) -> bool {
    capability.contract.supported_text_formats.contains(&format)
}

fn normalize_text_source<'a>(text: &'a TextBody, content: &'a str) -> &'a str {
    let trimmed = text.text.trim();
    if trimmed.is_empty() {
        content.trim()
    } else {
        trimmed
    }
}

fn looks_like_markdownish(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains("```")
        || trimmed.contains("**")
        || trimmed.contains("__")
        || trimmed.contains("~~")
        || trimmed.contains('`')
        || trimmed.contains("](")
    {
        return true;
    }
    trimmed.lines().any(|line| {
        let trimmed = line.trim_start();
        is_heading_line(trimmed)
            || is_unordered_list_line(trimmed)
            || is_ordered_list_line(trimmed)
            || trimmed.starts_with("> ")
    })
}

pub(crate) fn render_markdownish_to_plain_text(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            lines.push(line.to_string());
            continue;
        }
        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }
        let normalized = if let Some(rest) = heading_text(trimmed) {
            render_inline_markdownish_to_plain(rest)
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            render_inline_markdownish_to_plain(rest)
        } else if let Some(rest) = unordered_list_text(trimmed) {
            format!("• {}", render_inline_markdownish_to_plain(rest))
        } else if let Some((index, rest)) = ordered_list_text(trimmed) {
            format!("{index}. {}", render_inline_markdownish_to_plain(rest))
        } else {
            render_inline_markdownish_to_plain(trimmed)
        };
        lines.push(normalized);
    }
    collapse_blank_lines(lines).join("\n")
}

fn render_markdownish_to_telegram_html(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lines = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_code_block {
                lines.push(format!(
                    "<pre>{}</pre>",
                    escape_html_text(&code_lines.join("\n"))
                ));
                code_lines.clear();
                in_code_block = false;
            } else {
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            code_lines.push(line.to_string());
            continue;
        }
        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }
        let rendered = if let Some(rest) = heading_text(trimmed) {
            format!("<b>{}</b>", render_inline_markdownish_to_html(rest))
        } else if let Some(rest) = trimmed.strip_prefix("> ") {
            format!("&gt; {}", render_inline_markdownish_to_html(rest))
        } else if let Some(rest) = unordered_list_text(trimmed) {
            format!("• {}", render_inline_markdownish_to_html(rest))
        } else if let Some((index, rest)) = ordered_list_text(trimmed) {
            format!("{index}. {}", render_inline_markdownish_to_html(rest))
        } else {
            render_inline_markdownish_to_html(trimmed)
        };
        lines.push(rendered);
    }

    if !code_lines.is_empty() {
        lines.push(format!(
            "<pre>{}</pre>",
            escape_html_text(&code_lines.join("\n"))
        ));
    }
    collapse_blank_lines(lines).join("\n")
}

fn render_inline_markdownish_to_plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < text.len() {
        let rest = &text[idx..];
        if let Some((consumed, rendered)) = consume_inline_marker_plain(rest) {
            out.push_str(&rendered);
            idx += consumed;
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().unwrap_or_default();
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn render_inline_markdownish_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < text.len() {
        let rest = &text[idx..];
        if let Some((consumed, rendered)) = consume_inline_marker_html(rest) {
            out.push_str(&rendered);
            idx += consumed;
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().unwrap_or_default();
        push_escaped_html_char(&mut out, ch);
        idx += ch.len_utf8();
    }
    out
}

fn consume_inline_marker_plain(rest: &str) -> Option<(usize, String)> {
    if let Some((consumed, label, url)) = consume_image_or_link(rest, true) {
        return Some((consumed, plain_link_text(&label, &url)));
    }
    if let Some((consumed, label, url)) = consume_image_or_link(rest, false) {
        return Some((consumed, plain_link_text(&label, &url)));
    }
    if let Some((consumed, inner)) = consume_delimited(rest, "`", "`") {
        return Some((consumed, inner.to_string()));
    }
    if let Some((consumed, inner)) = consume_delimited(rest, "**", "**")
        .or_else(|| consume_delimited(rest, "__", "__"))
        .or_else(|| consume_delimited(rest, "~~", "~~"))
    {
        return Some((consumed, render_inline_markdownish_to_plain(inner)));
    }
    if let Some((consumed, inner)) = consume_emphasis(rest) {
        return Some((consumed, render_inline_markdownish_to_plain(inner)));
    }
    None
}

fn consume_inline_marker_html(rest: &str) -> Option<(usize, String)> {
    if let Some((consumed, label, url)) = consume_image_or_link(rest, true) {
        return Some((
            consumed,
            format!("{} ({})", escape_html_text(&label), escape_html_text(&url)),
        ));
    }
    if let Some((consumed, label, url)) = consume_image_or_link(rest, false) {
        return Some((
            consumed,
            format!(
                "<a href=\"{}\">{}</a>",
                escape_html_attr(&url),
                render_inline_markdownish_to_html(&label)
            ),
        ));
    }
    if let Some((consumed, inner)) = consume_delimited(rest, "`", "`") {
        return Some((
            consumed,
            format!("<code>{}</code>", escape_html_text(inner)),
        ));
    }
    if let Some((consumed, inner)) =
        consume_delimited(rest, "**", "**").or_else(|| consume_delimited(rest, "__", "__"))
    {
        return Some((
            consumed,
            format!("<b>{}</b>", render_inline_markdownish_to_html(inner)),
        ));
    }
    if let Some((consumed, inner)) = consume_delimited(rest, "~~", "~~") {
        return Some((
            consumed,
            format!("<s>{}</s>", render_inline_markdownish_to_html(inner)),
        ));
    }
    if let Some((consumed, inner)) = consume_emphasis(rest) {
        return Some((
            consumed,
            format!("<i>{}</i>", render_inline_markdownish_to_html(inner)),
        ));
    }
    None
}

fn consume_image_or_link(rest: &str, image: bool) -> Option<(usize, String, String)> {
    let prefix = if image { "![" } else { "[" };
    if !rest.starts_with(prefix) {
        return None;
    }
    let label_start = prefix.len();
    let label_end = rest[label_start..].find(']')? + label_start;
    let after_label = &rest[label_end + 1..];
    if !after_label.starts_with('(') {
        return None;
    }
    let url_end = after_label[1..].find(')')? + 1;
    let consumed = label_end + 1 + url_end + 1;
    let label = rest[label_start..label_end].to_string();
    let url = after_label[1..url_end].trim().to_string();
    Some((consumed, label, url))
}

fn consume_delimited<'a>(rest: &'a str, open: &str, close: &str) -> Option<(usize, &'a str)> {
    if !rest.starts_with(open) {
        return None;
    }
    let tail = &rest[open.len()..];
    let close_offset = tail.find(close)?;
    let inner = &tail[..close_offset];
    if inner.is_empty() || inner.contains('\n') {
        return None;
    }
    Some((open.len() + close_offset + close.len(), inner))
}

fn consume_emphasis(rest: &str) -> Option<(usize, &str)> {
    if rest.starts_with("**") || rest.starts_with("__") {
        return None;
    }
    let delimiter = if rest.starts_with('*') {
        "*"
    } else if rest.starts_with('_') {
        "_"
    } else {
        return None;
    };
    let tail = &rest[1..];
    let close_offset = tail.find(delimiter)?;
    let inner = &tail[..close_offset];
    if inner.is_empty()
        || inner.contains('\n')
        || inner.starts_with(char::is_whitespace)
        || inner.ends_with(char::is_whitespace)
    {
        return None;
    }
    Some((1 + close_offset + 1, inner))
}

fn plain_link_text(label: &str, url: &str) -> String {
    let label = label.trim();
    let url = url.trim();
    if label.is_empty() {
        return url.to_string();
    }
    if url.is_empty() || label == url {
        label.to_string()
    } else {
        format!("{label} ({url})")
    }
}

fn render_html_to_plain_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    let mut pending_newline = false;
    let bytes = text.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        if bytes[idx] == b'<' {
            in_tag = true;
            if text[idx..].starts_with("<br") || text[idx..].starts_with("</p") {
                pending_newline = true;
            }
            idx += 1;
            continue;
        }
        if bytes[idx] == b'>' {
            in_tag = false;
            if pending_newline && !out.ends_with('\n') {
                out.push('\n');
            }
            pending_newline = false;
            idx += 1;
            continue;
        }
        if in_tag {
            idx += 1;
            continue;
        }
        let rest = &text[idx..];
        if let Some(entity) = rest.strip_prefix("&lt;") {
            out.push('<');
            idx = text.len() - entity.len();
            continue;
        }
        if let Some(entity) = rest.strip_prefix("&gt;") {
            out.push('>');
            idx = text.len() - entity.len();
            continue;
        }
        if let Some(entity) = rest.strip_prefix("&amp;") {
            out.push('&');
            idx = text.len() - entity.len();
            continue;
        }
        if let Some(entity) = rest.strip_prefix("&quot;") {
            out.push('"');
            idx = text.len() - entity.len();
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().unwrap_or_default();
        out.push(ch);
        idx += ch.len_utf8();
    }
    collapse_blank_lines(
        out.lines()
            .map(|line| line.trim_end().to_string())
            .collect(),
    )
    .join("\n")
}

fn escape_html_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        push_escaped_html_char(&mut out, ch);
    }
    out
}

fn escape_html_attr(text: &str) -> String {
    escape_html_text(text)
}

fn push_escaped_html_char(out: &mut String, ch: char) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        _ => out.push(ch),
    }
}

fn collapse_blank_lines(lines: Vec<String>) -> Vec<String> {
    let mut collapsed = Vec::with_capacity(lines.len());
    let mut previous_blank = false;
    for line in lines {
        let is_blank = line.trim().is_empty();
        if is_blank && previous_blank {
            continue;
        }
        previous_blank = is_blank;
        collapsed.push(line);
    }
    while collapsed.first().is_some_and(|line| line.trim().is_empty()) {
        collapsed.remove(0);
    }
    while collapsed.last().is_some_and(|line| line.trim().is_empty()) {
        collapsed.pop();
    }
    collapsed
}

fn is_heading_line(line: &str) -> bool {
    heading_text(line).is_some()
}

fn heading_text(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = line.get(hashes..)?.trim_start();
    if rest.is_empty() || rest.len() == line.len().saturating_sub(hashes) {
        return None;
    }
    Some(rest)
}

fn is_unordered_list_line(line: &str) -> bool {
    unordered_list_text(line).is_some()
}

fn unordered_list_text(line: &str) -> Option<&str> {
    ["- ", "* ", "+ "]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix))
        .map(str::trim_start)
}

fn is_ordered_list_line(line: &str) -> bool {
    ordered_list_text(line).is_some()
}

fn ordered_list_text(line: &str) -> Option<(String, &str)> {
    let digit_count = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let rest = line.get(digit_count..)?;
    let rest = rest.strip_prefix(". ")?;
    Some((line[..digit_count].to_string(), rest.trim_start()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::{MessageBodyKind, MessageTransport, OutboundKind};
    use crate::channel_capability::{ChannelCapabilityContract, ChannelDeliveryOrderingModel};
    use std::sync::Arc;

    const TEXT_ONLY_KIND: &[MessageBodyKind] = &[MessageBodyKind::Text];
    const TEXT_AND_CARD_KIND: &[MessageBodyKind] = &[MessageBodyKind::Text, MessageBodyKind::Card];
    const PLAIN_ONLY: &[TextFormat] = &[TextFormat::Plain];
    const MARKDOWN_ONLY: &[TextFormat] = &[TextFormat::Plain, TextFormat::Markdown];
    const HTML_ONLY: &[TextFormat] = &[TextFormat::Plain, TextFormat::Html];
    const RICH_TEXT_ONLY: &[TextFormat] = &[TextFormat::Plain, TextFormat::RichText];

    fn capability_entry(
        id: &'static str,
        body_kinds: &'static [MessageBodyKind],
        text_formats: &'static [TextFormat],
    ) -> ChannelCapabilityEntry {
        ChannelCapabilityEntry {
            id,
            configured: true,
            enabled: true,
            contract: ChannelCapabilityContract {
                supports_primary_reply: true,
                supports_supplemental_reply: true,
                supports_edit: false,
                supports_stream_edit: false,
                supports_explicit_target: true,
                supports_attachment: body_kinds.iter().any(|kind| *kind != MessageBodyKind::Text),
                supports_typing_or_chat_action: false,
                supported_body_kinds: body_kinds,
                supported_text_formats: text_formats,
                requires_pre_upload_for_media: false,
                supports_platform_handle_reuse: false,
                supports_http_url_media: false,
                requires_passive_reply_anchor: false,
                max_text_bytes: 4096,
                max_caption_bytes: 0,
                delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
            },
        }
    }

    fn outbound_text_msg(content: &str) -> PcMsg {
        PcMsg {
            channel: Arc::from("telegram"),
            chat_id: Arc::from("chat-1"),
            content: content.to_string(),
            body: CanonicalMessageBody::Text(TextBody::plain(content)),
            platform_thread_id: String::new(),
            req_id: Some("req-1".to_string()),
            outbound_kind: OutboundKind::Primary,
            ingress: crate::bus::IngressKind::User,
            enqueue_ts_ms: 0,
            source_transport: MessageTransport::Internal,
            platform_message_id: String::new(),
            platform_event_id: String::new(),
            inbound_dedup_key: String::new(),
            is_group: false,
        }
    }

    #[test]
    fn primary_markdownish_reply_stays_plain_for_telegram() {
        let msg = outbound_text_msg("## Build Status\n**Green**");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry(
                CHANNEL_TELEGRAM,
                TEXT_ONLY_KIND,
                HTML_ONLY,
            )),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "## Build Status\n**Green**"
        ));
        assert_eq!(prepared.content, "## Build Status\n**Green**");
    }

    #[test]
    fn primary_markdownish_reply_on_qq_stays_plain_text() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "# Title\n- item"
        ));
        assert_eq!(prepared.content, "# Title\n- item");
    }

    #[test]
    fn primary_multiline_plain_reply_on_qq_stays_plain_text() {
        let mut msg = outbound_text_msg("第一段。\n\n第二段。");
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "第一段。\n\n第二段。"
        ));
        assert_eq!(prepared.content, "第一段。\n\n第二段。");
    }

    #[test]
    fn primary_inline_label_chain_on_qq_stays_plain_text() {
        let mut msg =
            outbound_text_msg("状态--芯片: ESP32-S3 - 运行时间: 5 分钟- WiFi: 已连接 - 内存: 正常");
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "状态--芯片: ESP32-S3 - 运行时间: 5 分钟- WiFi: 已连接 - 内存: 正常"
        ));
        assert_eq!(
            prepared.content,
            "状态--芯片: ESP32-S3 - 运行时间: 5 分钟- WiFi: 已连接 - 内存: 正常"
        );
    }

    #[test]
    fn primary_natural_dash_pairs_on_qq_stays_plain_text() {
        let mut msg = outbound_text_msg("我看 A - B: C 只是一个例子，不需要重排。");
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "我看 A - B: C 只是一个例子，不需要重排。"
        ));
        assert_eq!(prepared.content, "我看 A - B: C 只是一个例子，不需要重排。");
    }

    #[test]
    fn oversized_primary_multiline_plain_reply_on_qq_stays_plain_for_sender_chunking() {
        let oversized = format!("{}\n第二段。", "甲".repeat(4096));
        let mut msg = outbound_text_msg(&oversized);
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == &oversized
        ));
        assert_eq!(prepared.content, oversized);
    }

    #[test]
    fn oversized_primary_markdownish_reply_on_qq_stays_plain_for_sender_chunking() {
        let oversized = format!("# 标题\n{}", "甲".repeat(4096));
        let mut msg = outbound_text_msg(&oversized);
        msg.channel = Arc::from("qq_channel");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == &oversized
        ));
        assert_eq!(prepared.content, oversized);
    }

    #[test]
    fn primary_markdownish_reply_stays_plain_for_wecom() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from("wecom");
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("wecom", TEXT_ONLY_KIND, MARKDOWN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "# Title\n- item"
        ));
        assert_eq!(prepared.content, "# Title\n- item");
    }

    #[test]
    fn explicit_qq_markdown_body_downgrades_to_plain_text() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from("qq_channel");
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "# Title\n- item".to_string(),
            format: TextFormat::Markdown,
        });
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("qq_channel", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "Title\n• item"
        ));
        assert_eq!(prepared.content, "Title\n• item");
    }

    #[test]
    fn explicit_markdown_body_stays_markdown_for_markdown_capable_channel() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from("wecom");
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "# Title\n- item".to_string(),
            format: TextFormat::Markdown,
        });

        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("wecom", TEXT_ONLY_KIND, MARKDOWN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Markdown,
                ref text,
            }) if text == "# Title\n- item"
        ));
        assert_eq!(prepared.content, "# Title\n- item");
    }

    #[test]
    fn explicit_markdown_body_converts_to_telegram_html_when_markdown_is_unsupported() {
        let mut msg = outbound_text_msg("## Build Status\n**Green**");
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "## Build Status\n**Green**".to_string(),
            format: TextFormat::Markdown,
        });

        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry(
                CHANNEL_TELEGRAM,
                TEXT_ONLY_KIND,
                HTML_ONLY,
            )),
        );

        match prepared.msg.body {
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Html,
                text,
            }) => {
                assert!(text.contains("<b>Build Status</b>"));
                assert!(text.contains("<b>Green</b>"));
            }
            other => panic!("expected telegram html body, got {other:?}"),
        }
        assert_eq!(prepared.content, "## Build Status\n**Green**");
    }

    #[test]
    fn explicit_markdown_body_converts_to_feishu_rich_post_when_markdown_is_unsupported() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from(CHANNEL_FEISHU);
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "# Title\n- item".to_string(),
            format: TextFormat::Markdown,
        });

        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry(
                CHANNEL_FEISHU,
                TEXT_AND_CARD_KIND,
                RICH_TEXT_ONLY,
            )),
        );

        match prepared.msg.body {
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::RichPost,
                payload_json,
                fallback_text,
            }) => {
                assert_eq!(fallback_text, "Title\n• item");
                assert!(payload_json.get("zh_cn").is_some());
            }
            other => panic!("expected feishu rich post, got {other:?}"),
        }
        assert_eq!(prepared.content, "Title\n• item");
    }

    #[test]
    fn primary_markdownish_reply_stays_plain_for_feishu() {
        let mut msg = outbound_text_msg("# Title\n- item");
        msg.channel = Arc::from(CHANNEL_FEISHU);
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry(
                CHANNEL_FEISHU,
                TEXT_AND_CARD_KIND,
                RICH_TEXT_ONLY,
            )),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ref text,
            }) if text == "# Title\n- item"
        ));
        assert_eq!(prepared.content, "# Title\n- item");
    }

    #[test]
    fn unsupported_media_body_downgrades_to_plain_text() {
        let mut msg = outbound_text_msg("![chart](https://example.com/chart.png)");
        msg.body = CanonicalMessageBody::Image(crate::bus::ImageBody {
            asset: crate::bus::MediaAssetRef::external_url("https://example.com/chart.png"),
            caption: Some(TextBody {
                text: "**chart**".to_string(),
                format: TextFormat::Markdown,
            }),
        });
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("websocket", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        match prepared.msg.body {
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                text,
            }) => assert_eq!(text, "chart"),
            other => panic!("expected plain text downgrade, got {other:?}"),
        }
        assert_eq!(prepared.content, "chart");
    }

    #[test]
    fn unsupported_card_body_prefers_canonical_content_projection() {
        let mut msg = outbound_text_msg("构建已通过");
        msg.body = CanonicalMessageBody::Card(CardBody {
            format: CardFormat::Interactive,
            payload_json: json!({"header":{"title":"Build passed"}}),
            fallback_text: String::new(),
        });
        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("websocket", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        match prepared.msg.body {
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                text,
            }) => assert_eq!(text, "构建已通过"),
            other => panic!("expected plain text downgrade, got {other:?}"),
        }
        assert_eq!(prepared.content, "构建已通过");
    }

    #[test]
    fn rich_text_text_body_on_feishu_becomes_rich_post() {
        let mut msg = outbound_text_msg("**hello**");
        msg.channel = Arc::from(CHANNEL_FEISHU);
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "**hello**".to_string(),
            format: TextFormat::RichText,
        });

        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry(
                CHANNEL_FEISHU,
                TEXT_AND_CARD_KIND,
                RICH_TEXT_ONLY,
            )),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Card(CardBody {
                format: CardFormat::RichPost,
                ..
            })
        ));
        assert_eq!(prepared.content, "hello");
    }

    #[test]
    fn markdown_body_without_markdown_support_downgrades_to_plain_text() {
        let mut msg = outbound_text_msg("**ping**");
        msg.channel = Arc::from("websocket");
        msg.body = CanonicalMessageBody::Text(TextBody {
            text: "**ping**".to_string(),
            format: TextFormat::Markdown,
        });

        let prepared = prepare_outbound_message_for_channel(
            &msg,
            Some(capability_entry("websocket", TEXT_ONLY_KIND, PLAIN_ONLY)),
        );

        assert!(matches!(
            prepared.msg.body,
            CanonicalMessageBody::Text(TextBody {
                format: TextFormat::Plain,
                ..
            })
        ));
        assert_eq!(prepared.content, "ping");
    }
}
