//! Agent 运行策略：按平台选择轻量或增强链路。
//! Internal agent strategy helpers for platform-specific behavior.

/// 内部运行策略：ESP 保持轻量，Linux 启用额外 planning / reflection 增强。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentRunStrategy {
    Embedded,
    LinuxEnhanced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SuccessfulToolRoundSummary {
    pub(crate) total_calls: usize,
    pub(crate) successful_calls: usize,
}

pub(crate) fn final_answer_followup(
    strategy: AgentRunStrategy,
    recent_successful_round: Option<SuccessfulToolRoundSummary>,
    content: &str,
) -> Option<String> {
    if strategy != AgentRunStrategy::LinuxEnhanced {
        return None;
    }
    let recent_successful_round = recent_successful_round?;
    if !content_looks_like_process_transcript(content) {
        return None;
    }
    Some(
        if recent_successful_round.successful_calls == recent_successful_round.total_calls {
            "[SYSTEM] Your draft reads like an execution transcript instead of a final user-facing answer. Do not show step logs or future-plan sections. Rewrite the answer using only the completed results already present in this conversation."
                .to_string()
        } else {
            format!(
                "[SYSTEM] Your draft reads like an execution transcript instead of a final user-facing answer. You already have {} useful result(s) in context. Rewrite the answer around those completed results only, and remove step logs or future-plan sections.",
                recent_successful_round.successful_calls
            )
        },
    )
}

pub(crate) fn empty_final_answer_followup(
    _strategy: AgentRunStrategy,
    any_tool_used: bool,
    content: &str,
) -> Option<&'static str> {
    if !any_tool_used || !content.trim().is_empty() {
        return None;
    }
    Some(
        "[SYSTEM] Your current draft is empty. Provide a user-facing final answer from the completed tool results now. Do not emit an empty reply. Do not output progress logs, numbered execution steps, or future-plan sections.",
    )
}

pub(crate) fn repeated_answer_followup(
    strategy: AgentRunStrategy,
    recent_assistant_messages: &[&str],
    content: &str,
) -> Option<&'static str> {
    if strategy != AgentRunStrategy::LinuxEnhanced || recent_assistant_messages.is_empty() {
        return None;
    }
    let content = content.trim();
    if content.chars().count() < 24 {
        return None;
    }
    let current = normalize_repetition_text(content);
    if current.len() < 24 {
        return None;
    }
    for prior in recent_assistant_messages {
        let prior = prior.trim();
        if prior.is_empty() || prior == "[tool_use]" {
            continue;
        }
        let prior_norm = normalize_repetition_text(prior);
        if prior_norm.len() < 24 {
            continue;
        }
        let min_len = current.len().min(prior_norm.len());
        let near_same = current == prior_norm
            || (min_len >= 32
                && (prior_norm.contains(&current)
                    || current.contains(&prior_norm)
                    || normalized_prefix_overlap(&current, &prior_norm) >= 0.85));
        if near_same {
            return Some(
                "[SYSTEM] Your draft mostly repeats assistant text that is already in the current context. Do not restate the same analysis. Only provide the new delta or the final conclusion that still matters.",
            );
        }
    }
    None
}

fn content_looks_like_process_transcript(content: &str) -> bool {
    let mut heading_count = 0usize;
    let mut rule_count = 0usize;
    let mut enumerated_count = 0usize;
    let mut emphasized_heading_count = 0usize;

    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if line.starts_with('#') {
            heading_count += 1;
            continue;
        }
        if line.len() >= 3 && line.chars().all(|ch| ch == '-') {
            rule_count += 1;
            continue;
        }
        if is_enumerated_visual_line(line) {
            enumerated_count += 1;
            continue;
        }
        if line.starts_with("**") && line.ends_with("**") && line.chars().count() <= 48 {
            emphasized_heading_count += 1;
        }
    }

    heading_count >= 2
        || emphasized_heading_count >= 2
        || (heading_count >= 1 && rule_count >= 1)
        || (heading_count >= 1 && enumerated_count >= 2)
        || enumerated_count >= 3
}

fn is_enumerated_visual_line(line: &str) -> bool {
    let Some(first) = line.chars().next() else {
        return false;
    };
    if first.is_ascii_digit() {
        let rest = &line[first.len_utf8()..];
        return rest.starts_with(". ") || rest.starts_with(") ");
    }
    let Some(close) = line.find(']') else {
        return false;
    };
    if !line.starts_with('[') || close > 6 {
        return false;
    }
    let prefix = &line[1..close];
    !prefix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())
}

fn normalize_repetition_text(content: &str) -> String {
    let lower = content
        .replace("[compressed]", "")
        .replace("...", " ")
        .to_ascii_lowercase();
    let mut out = String::with_capacity(lower.len());
    for ch in lower.chars() {
        if ch.is_alphanumeric()
            || matches!(
                ch,
                '\u{4e00}'..='\u{9fff}'
                    | '\u{3400}'..='\u{4dbf}'
                    | '/'
                    | '\\'
                    | '.'
                    | '_'
                    | '-'
            )
        {
            out.push(ch);
        }
    }
    out
}

fn normalized_prefix_overlap(a: &str, b: &str) -> f32 {
    let max = a.len().max(b.len());
    if max == 0 {
        return 0.0;
    }
    let mut matched = 0usize;
    for (left, right) in a.bytes().zip(b.bytes()) {
        if left != right {
            break;
        }
        matched += 1;
    }
    matched as f32 / max as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_answer_followup_skips_generic_wrapups_without_structural_artifacts() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 2,
                successful_calls: 2,
            }),
            "我先给你一个简短总结，供你参考。",
        );
        assert!(followup.is_none());
    }

    #[test]
    fn final_answer_followup_rewrites_process_transcript_after_tool_success() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 1,
                successful_calls: 1,
            }),
            "## Multi-step run\n\n### Result\nDone.\n\n---\n\n### Next\nContinue scanning.",
        )
        .expect("followup");
        assert!(followup.contains("execution transcript"));
    }

    #[test]
    fn final_answer_followup_keeps_concrete_short_answers() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 1,
                successful_calls: 1,
            }),
            "当前版本是 1.2.3。",
        );
        assert!(followup.is_none());
    }

    #[test]
    fn process_transcript_detection_ignores_plain_direct_answer() {
        assert!(!content_looks_like_process_transcript(
            "当前版本是 1.2.3，配置目录在 /var/lib/beetle/config。"
        ));
    }

    #[test]
    fn final_answer_followup_mentions_successful_subset_for_mixed_rounds() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 3,
                successful_calls: 2,
            }),
            "## Multi-step run\n\n### Result\nDone.\n\n### Next\nContinue scanning.",
        )
        .expect("followup");
        assert!(followup.contains("2 useful result(s)"));
    }

    #[test]
    fn empty_final_answer_followup_requires_non_empty_user_facing_result() {
        let followup =
            empty_final_answer_followup(AgentRunStrategy::LinuxEnhanced, true, "").expect("text");
        assert!(followup.contains("Do not emit an empty reply"));
    }

    #[test]
    fn empty_final_answer_followup_applies_to_embedded_after_tool_progress() {
        let followup =
            empty_final_answer_followup(AgentRunStrategy::Embedded, true, "").expect("text");
        assert!(followup.contains("Provide a user-facing final answer"));
    }

    #[test]
    fn empty_final_answer_followup_skips_without_tool_progress() {
        assert!(empty_final_answer_followup(AgentRunStrategy::LinuxEnhanced, false, "").is_none());
    }

    #[test]
    fn repeated_answer_followup_flags_repeated_assistant_text() {
        let followup = repeated_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            &["目前结论是查看 /tmp/result.json，然后按 phase_b 继续执行。"],
            "目前结论是查看 /tmp/result.json，然后按 phase_b 继续执行。",
        );
        assert!(followup.is_some());
    }

    #[test]
    fn repeated_answer_followup_skips_new_content() {
        let followup = repeated_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            &["先检查日志，再确认模型分区。"],
            "新的问题在于 Linux 侧 agent loop 的最终收口还不够硬。",
        );
        assert!(followup.is_none());
    }

    #[test]
    fn repeated_answer_followup_skips_non_linux_strategy() {
        let followup = repeated_answer_followup(
            AgentRunStrategy::Embedded,
            &["repeat me"],
            "repeat me with extra words to exceed the threshold for testing",
        );
        assert!(followup.is_none());
    }
}
