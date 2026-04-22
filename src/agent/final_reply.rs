use super::strategy::AgentRunStrategy;
use crate::bus::CanonicalMessageBody;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalReply {
    pub(crate) visible_text: String,
}

impl CanonicalReply {
    pub(crate) fn new(visible_text: String) -> Self {
        Self { visible_text }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.visible_text.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplyArtifactBundle {
    pub(crate) current_chat_primary_body: CanonicalMessageBody,
}

impl ReplyArtifactBundle {
    pub(crate) fn current_chat_primary(body: CanonicalMessageBody) -> Self {
        Self {
            current_chat_primary_body: body,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplyContractBreachKind {
    ProducerEmpty,
    ArtifactOnlyReply,
    InternalArtifactReply,
    MetaInstructionReply,
}

impl ReplyContractBreachKind {
    pub(crate) fn stage(self) -> &'static str {
        match self {
            Self::ProducerEmpty => "producer_empty",
            Self::ArtifactOnlyReply => "artifact_only_reply",
            Self::InternalArtifactReply => "internal_artifact_reply",
            Self::MetaInstructionReply => "meta_instruction_reply",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReplyArtifactState {
    None,
    ArtifactOnly,
    InternalArtifactLeak,
}

pub(crate) fn is_reply_contract_breach_stage(stage: &str) -> bool {
    matches!(
        stage,
        "producer_empty"
            | "artifact_only_reply"
            | "internal_artifact_reply"
            | "meta_instruction_reply"
    )
}

pub(crate) fn build_canonical_reply(
    strategy: AgentRunStrategy,
    content: &str,
) -> std::result::Result<CanonicalReply, ReplyContractBreachKind> {
    let normalized = normalize_line_endings(content);
    if normalized.trim().is_empty() {
        return Err(ReplyContractBreachKind::ProducerEmpty);
    }

    match classify_reply_artifacts(&normalized) {
        ReplyArtifactState::None => {}
        ReplyArtifactState::ArtifactOnly => {
            return Err(ReplyContractBreachKind::ArtifactOnlyReply);
        }
        ReplyArtifactState::InternalArtifactLeak => {
            return Err(ReplyContractBreachKind::InternalArtifactReply);
        }
    }

    let visible_text = finalize_user_visible_reply(strategy, &normalized);
    if visible_text.trim().is_empty() {
        return Err(ReplyContractBreachKind::ProducerEmpty);
    }
    if looks_like_meta_instruction_reply(&visible_text) {
        return Err(ReplyContractBreachKind::MetaInstructionReply);
    }

    Ok(CanonicalReply::new(visible_text))
}

pub(crate) fn classify_reply_artifacts(content: &str) -> ReplyArtifactState {
    let normalized = normalize_line_endings(content);
    let artifact_scan = scan_internal_reply_artifacts(&normalized);
    if !artifact_scan.has_artifact {
        return ReplyArtifactState::None;
    }
    if artifact_scan.visible_without_artifacts.trim().is_empty() {
        ReplyArtifactState::ArtifactOnly
    } else {
        ReplyArtifactState::InternalArtifactLeak
    }
}

pub(crate) fn finalize_user_visible_reply(strategy: AgentRunStrategy, content: &str) -> String {
    let normalized = normalize_line_endings(content);
    let normalized = collapse_blank_lines(
        normalized
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect(),
    );
    if normalized.is_empty() {
        return String::new();
    }
    if strategy != AgentRunStrategy::LinuxEnhanced {
        return normalized;
    }

    let mut paragraphs = split_paragraphs(&normalized);
    if paragraphs.len() >= 2 && should_strip_heading_prefix(&paragraphs[0], &paragraphs[1]) {
        paragraphs.remove(0);
    }
    if paragraphs.is_empty() {
        normalized
    } else {
        paragraphs.join("\n\n")
    }
}

pub(crate) fn strip_legacy_internal_reply_blocks(content: &str) -> String {
    let normalized = normalize_line_endings(content);
    strip_internal_reply_block(
        &normalized,
        "<foreground_work_packet>",
        "</foreground_work_packet>",
    )
}

struct ArtifactScan {
    has_artifact: bool,
    visible_without_artifacts: String,
}

fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn scan_internal_reply_artifacts(content: &str) -> ArtifactScan {
    let mut lines = Vec::new();
    let mut has_artifact = false;
    let mut skip_until_tag: Option<&'static str> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        if let Some(end_tag) = skip_until_tag {
            has_artifact = true;
            if line == end_tag {
                skip_until_tag = None;
            }
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            lines.push(String::new());
            continue;
        }
        if let Some(end_tag) = internal_block_end_tag(trimmed) {
            has_artifact = true;
            skip_until_tag = Some(end_tag);
            continue;
        }
        if trimmed == "[tool_use]" || trimmed == "[compressed]" || trimmed.starts_with("[SYSTEM]") {
            has_artifact = true;
            continue;
        }
        lines.push(trimmed.to_string());
    }

    ArtifactScan {
        has_artifact,
        visible_without_artifacts: collapse_blank_lines(lines),
    }
}

fn internal_block_end_tag(line: &str) -> Option<&'static str> {
    if line.starts_with("<tool_result ") {
        Some("</tool_result>")
    } else if line.starts_with("<surface_evidence ") {
        Some("</surface_evidence>")
    } else if line == "<tool_evidence_summary>" {
        Some("</tool_evidence_summary>")
    } else if line == "<memory_grounding>" {
        Some("</memory_grounding>")
    } else if line == "<foreground_work_packet>" {
        Some("</foreground_work_packet>")
    } else {
        None
    }
}

fn strip_internal_reply_block(content: &str, start_tag: &str, end_tag: &str) -> String {
    let Some(open_start) = content.find(start_tag) else {
        return content.trim().to_string();
    };
    let close_start = content[open_start..]
        .find(end_tag)
        .map(|value| open_start + value);
    let visible_len = close_start
        .map(|value| content.len().saturating_sub(value))
        .unwrap_or_else(|| content.len().saturating_sub(open_start));
    let mut visible = String::with_capacity(visible_len);
    visible.push_str(content[..open_start].trim_end());
    if let Some(close_start) = close_start {
        let trailing = content[close_start + end_tag.len()..].trim_start();
        if !trailing.is_empty() {
            if !visible.trim().is_empty() {
                visible.push_str("\n\n");
            }
            visible.push_str(trailing);
        }
    }
    visible.trim().to_string()
}

fn looks_like_meta_instruction_reply(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    lower.contains("please rewrite your answer")
        || lower.contains("return json only")
        || lower.contains("do not call tools")
        || lower.contains("using only the completed tool results")
        || lower.contains("tool-execution budget for this turn is exhausted")
        || lower.contains("do not output execution transcripts")
        || content.contains("只返回 JSON")
        || content.contains("不要调用工具")
}

fn collapse_blank_lines(lines: Vec<String>) -> String {
    let mut out = String::new();
    let mut prev_blank = true;
    for line in lines {
        let blank = line.trim().is_empty();
        if blank {
            if prev_blank {
                continue;
            }
            out.push_str("\n\n");
            prev_blank = true;
            continue;
        }
        if !out.is_empty() && !prev_blank {
            out.push('\n');
        }
        out.push_str(line.trim());
        prev_blank = false;
    }
    out.trim().to_string()
}

fn split_paragraphs(content: &str) -> Vec<String> {
    content
        .split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_string)
        .collect()
}

fn should_strip_heading_prefix(first: &str, second: &str) -> bool {
    let trimmed = first.trim();
    trimmed.chars().count() <= 20
        && !reply_has_concrete_anchor(trimmed)
        && (trimmed.ends_with('：') || trimmed.ends_with(':'))
        && content_has_substantive_payload(second)
}

fn content_has_substantive_payload(content: &str) -> bool {
    content.chars().count() >= 24
        && (reply_has_concrete_anchor(content)
            || content.contains('\n')
            || content.starts_with("- ")
            || content.starts_with("1. ")
            || content.starts_with("2. "))
}

pub(crate) fn reply_has_concrete_anchor(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let file_markers = [
        ".rs", ".md", ".json", ".toml", ".yaml", ".yml", ".log", ".txt", ".py", ".sh",
    ];
    content.chars().any(|ch| ch.is_ascii_digit())
        || content.contains("://")
        || content.contains('`')
        || content.contains('/')
        || content.contains('\\')
        || file_markers.iter().any(|marker| lower.contains(marker))
}

pub(crate) fn reply_looks_like_future_action_narration(content: &str) -> bool {
    let trimmed = content.trim();
    let lower = trimmed.to_ascii_lowercase();
    [
        "我先整理",
        "我先检查",
        "我先看看",
        "我先处理",
        "我需要",
        "让我",
        "现在我需要",
        "我需要调整方法",
        "我明白了问题所在",
        "我了解了正确的配置结构",
        "先整理",
        "先检查",
        "先看看",
        "先处理",
        "继续配置",
        "继续处理",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
        || [
            "let me ",
            "i need to ",
            "now i need to ",
            "i will ",
            "i'll ",
            "first, let me ",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

pub(crate) fn reply_looks_like_transition_colon_draft(content: &str) -> bool {
    let trimmed = content.trim();
    !trimmed.is_empty() && (trimmed.ends_with('：') || trimmed.ends_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_canonical_reply_rejects_artifact_only_reply() {
        let raw = "[SYSTEM] do not show\n<tool_result id=\"1\" tool=\"x\" status=\"ok\">\nhello\n</tool_result>\n";
        let err = build_canonical_reply(AgentRunStrategy::LinuxEnhanced, raw)
            .expect_err("artifact-only reply must fail");
        assert_eq!(err, ReplyContractBreachKind::ArtifactOnlyReply);
    }

    #[test]
    fn build_canonical_reply_rejects_internal_artifact_leak() {
        let raw = "这是答复。\n[SYSTEM] hidden";
        let err = build_canonical_reply(AgentRunStrategy::LinuxEnhanced, raw)
            .expect_err("mixed reply must fail");
        assert_eq!(err, ReplyContractBreachKind::InternalArtifactReply);
    }

    #[test]
    fn build_canonical_reply_rejects_meta_instruction_text() {
        let raw = "Please rewrite your answer to be specific to the actual failure.";
        let err =
            build_canonical_reply(AgentRunStrategy::LinuxEnhanced, raw).expect_err("meta leak");
        assert_eq!(err, ReplyContractBreachKind::MetaInstructionReply);
    }

    #[test]
    fn future_action_detection_accepts_subjectless_progress_narration() {
        assert!(reply_looks_like_future_action_narration(
            "先整理一下当前状态。"
        ));
        assert!(reply_looks_like_future_action_narration(
            "继续处理邮箱配置。"
        ));
        assert!(!reply_looks_like_future_action_narration(
            "当前主机 beetle 在线，可继续配置 QQ 邮箱。"
        ));
    }

    #[test]
    fn final_reply_strips_structural_heading_prefix() {
        let raw = "结论如下：\n\n当前版本是 1.2.3，配置文件在 /tmp/a.json。";
        let formatted = finalize_user_visible_reply(AgentRunStrategy::LinuxEnhanced, raw);
        assert_eq!(formatted, "当前版本是 1.2.3，配置文件在 /tmp/a.json。");
    }

    #[test]
    fn final_reply_keeps_non_structural_suffix() {
        let raw = "当前版本是 1.2.3，配置文件在 /tmp/a.json。\n\n如果你需要我可以继续帮你展开。";
        let formatted = finalize_user_visible_reply(AgentRunStrategy::LinuxEnhanced, raw);
        assert_eq!(formatted, raw);
    }

    #[test]
    fn final_reply_keeps_embedded_text_unchanged() {
        let raw = "我先给你一个简短总结。\n\n当前版本是 1.2.3。";
        let formatted = finalize_user_visible_reply(AgentRunStrategy::Embedded, raw);
        assert_eq!(formatted, raw);
    }

    #[test]
    fn final_reply_no_longer_strips_internal_protocol_blocks() {
        let raw = concat!(
            "最终答复如下：\n\n",
            "<tool_result id=\"call_1\" tool=\"x\" status=\"ok\">\n",
            "hidden\n",
            "</tool_result>\n"
        );
        let formatted = finalize_user_visible_reply(AgentRunStrategy::LinuxEnhanced, raw);
        assert!(formatted.contains("<tool_result id=\"call_1\" tool=\"x\" status=\"ok\">"));
    }

    #[test]
    fn strip_legacy_internal_reply_blocks_removes_foreground_packet_wrapper() {
        let raw = "已切到 Work 邮箱。\n<foreground_work_packet>\n{\"legacy\":true}\n</foreground_work_packet>";
        assert_eq!(
            strip_legacy_internal_reply_blocks(raw),
            "已切到 Work 邮箱。"
        );
    }
}
