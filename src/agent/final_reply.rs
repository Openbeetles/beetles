use super::strategy::AgentRunStrategy;

pub(crate) fn finalize_user_visible_reply(strategy: AgentRunStrategy, content: &str) -> String {
    let normalized = strip_internal_reply_artifacts(content);
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

fn strip_internal_reply_artifacts(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    let mut skip_until_tag: Option<&'static str> = None;

    for raw_line in normalized.lines() {
        let line = raw_line.trim_end();
        if let Some(end_tag) = skip_until_tag {
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
            skip_until_tag = Some(end_tag);
            continue;
        }
        if trimmed == "[tool_use]" || trimmed == "[compressed]" {
            continue;
        }
        if trimmed.starts_with("[SYSTEM]") {
            continue;
        }
        lines.push(trimmed.to_string());
    }

    collapse_blank_lines(lines)
}

fn internal_block_end_tag(line: &str) -> Option<&'static str> {
    if line.starts_with("<tool_result ") {
        Some("</tool_result>")
    } else if line == "<tool_round_guidance>" {
        Some("</tool_round_guidance>")
    } else if line == "<tool_evidence_summary>" {
        Some("</tool_evidence_summary>")
    } else if line == "<memory_grounding>" {
        Some("</memory_grounding>")
    } else {
        None
    }
}

fn collapse_blank_lines(lines: Vec<String>) -> String {
    let mut out = String::new();
    let mut prev_blank = true;
    for line in lines {
        let blank = line.is_empty();
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
        out.push_str(&line);
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
        && !content_has_concrete_anchor(trimmed)
        && (trimmed.ends_with('：') || trimmed.ends_with(':'))
        && content_has_substantive_payload(second)
}

fn content_has_substantive_payload(content: &str) -> bool {
    content.chars().count() >= 24
        && (content_has_concrete_anchor(content)
            || content.contains('\n')
            || content.starts_with("- ")
            || content.starts_with("1. ")
            || content.starts_with("2. "))
}

fn content_has_concrete_anchor(content: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_reply_strips_internal_blocks_and_system_lines() {
        let raw = "[SYSTEM] do not show\n<tool_result id=\"1\" tool=\"x\" status=\"ok\">\nhello\n</tool_result>\n\n真正答案";
        let formatted = finalize_user_visible_reply(AgentRunStrategy::LinuxEnhanced, raw);
        assert_eq!(formatted, "真正答案");
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
}
