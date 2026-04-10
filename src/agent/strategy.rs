//! Agent 运行策略：按平台选择轻量或增强链路。
//! Internal agent strategy helpers for platform-specific behavior.

use super::tool_outcome::{ToolBlockerKind, ToolBlockerSummary, ToolFailureSummary};

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

pub(crate) fn build_tool_round_guidance(
    strategy: AgentRunStrategy,
    round_had_success: bool,
    consecutive_stalled_rounds: u8,
    total_calls: usize,
    repeated_calls: usize,
    failure_summary: ToolFailureSummary,
    ping_pong_detected: bool,
) -> Option<String> {
    if strategy != AgentRunStrategy::LinuxEnhanced
        || round_had_success
        || total_calls == 0
        || failure_summary.failed_calls == 0
    {
        return None;
    }
    if ping_pong_detected {
        return Some(
            "\n\n[SYSTEM] Recent tool rounds are bouncing between two failed call patterns without progress. Stop alternating between the same approaches. Choose a clearly different tool path or explain the blocker and remaining uncertainty to the user."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.capability_failures == failure_summary.failed_calls
    {
        return Some(
            "\n\n[SYSTEM] All tool calls in this round were blocked by permissions, unavailable tools, or runtime policy. Stop retrying the same requests. Explain the limitation clearly to the user or switch to a tool path that is actually allowed."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.permanent_failures == failure_summary.failed_calls
    {
        return Some(
            "\n\n[SYSTEM] All tool calls in this round failed due to invalid input, missing resources, or unsupported actions. Do not retry unchanged parameters. Fix the inputs or explain the blocker clearly."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls
        && failure_summary.retryable_failures == failure_summary.failed_calls
        && consecutive_stalled_rounds >= 2
    {
        return Some(
            "\n\n[SYSTEM] Recent tool calls are failing for transient reasons such as timeouts, network issues, or server pressure. Retry at most with a meaningfully different path; otherwise explain the temporary blocker instead of brute-forcing."
                .to_string(),
        );
    }
    if consecutive_stalled_rounds >= 3 {
        return Some(
            "\n\n[SYSTEM] You have spent multiple tool rounds without useful progress. Stop retrying the same path. Either choose a clearly different tool strategy or explain the blocker and remaining uncertainty to the user."
                .to_string(),
        );
    }
    if failure_summary.failed_calls == total_calls && repeated_calls > 0 {
        return Some(format!(
            "\n\n[SYSTEM] All {} tool call(s) in this round failed, and {} repeated a previous call pattern. Do not retry the same call again unless new evidence changes the inputs. Either switch to a different tool path or explain the blocker clearly.",
            total_calls, repeated_calls
        ));
    }
    if failure_summary.failed_calls == total_calls && total_calls > 1 {
        return Some(format!(
            "\n\n[SYSTEM] All {} tool call(s) in this round failed. Do not brute-force similar retries. Try a different tool family or explain the blocker clearly if no better action exists.",
            total_calls
        ));
    }
    if repeated_calls > 0 {
        return Some(format!(
            "\n\n[SYSTEM] {} tool call(s) repeated a previous call pattern without progress. Do not keep retrying the same action. Change strategy or explain the blocker.",
            repeated_calls
        ));
    }
    Some(
        "\n\n[SYSTEM] The last tool round did not produce a useful result. Reassess the plan, avoid repeating the same call, and either switch strategy or explain the blocker clearly."
            .to_string(),
    )
}

pub(crate) fn build_success_tool_round_guidance(
    strategy: AgentRunStrategy,
    total_calls: usize,
    failure_summary: ToolFailureSummary,
) -> Option<String> {
    if strategy != AgentRunStrategy::LinuxEnhanced || total_calls == 0 {
        return None;
    }
    if failure_summary.failed_calls == 0 {
        return Some(
            "\n\n[SYSTEM] You now have concrete tool results. If they answer the user's request, respond directly from those results. If more work is still required, call the next tool immediately instead of narrating a future plan. If you end the turn, deliver only completed results and current conclusions. Do not output execution transcripts, numbered step logs, or pending next-step sections for the user."
                .to_string(),
        );
    }
    if failure_summary.failed_calls < total_calls {
        return Some(
            "\n\n[SYSTEM] Some tool calls succeeded and already produced useful evidence. Prioritize those successful results in your answer. Only call more tools if a specific missing fact still matters. If you end the turn, deliver only completed results and current conclusions. Do not include execution transcripts, numbered step logs, or future-plan sections."
                .to_string(),
        );
    }
    None
}

pub(crate) fn detect_ping_pong_tool_rounds(recent_round_signatures: &[Option<u64>; 4]) -> bool {
    match *recent_round_signatures {
        [Some(a), Some(b), Some(c), Some(d)] => a != b && a == c && b == d,
        _ => false,
    }
}

pub(crate) fn stalled_end_turn_followup(
    strategy: AgentRunStrategy,
    consecutive_stalled_rounds: u8,
    content: &str,
) -> Option<&'static str> {
    if strategy != AgentRunStrategy::LinuxEnhanced || consecutive_stalled_rounds < 2 {
        return None;
    }
    if content_signals_blocker(content) {
        return None;
    }
    if content.chars().count() >= 320 {
        return None;
    }
    Some(
        "[SYSTEM] Previous tool attempts failed or made no useful progress. Do not keep retrying silently. Either switch to a clearly different approach now or explain the blocker, what remains unknown, and what the user can do next.",
    )
}

pub(crate) fn blocker_end_turn_followup(
    strategy: AgentRunStrategy,
    blocker: Option<ToolBlockerSummary>,
    content: &str,
) -> Option<&'static str> {
    if strategy != AgentRunStrategy::LinuxEnhanced {
        return None;
    }
    let blocker = blocker?;
    if content_signals_blocker(content) || content.chars().count() >= 360 {
        return None;
    }
    Some(match blocker.kind {
        ToolBlockerKind::Capability => {
            "[SYSTEM] Recent tool attempts were blocked by unavailable tools, permissions, or runtime policy. Do not end with a vague answer. Clearly explain that limitation, what capability is missing, and what the user can do next."
        }
        ToolBlockerKind::Permanent => {
            "[SYSTEM] Recent tool attempts failed because the requested inputs, resource, or action were invalid or unavailable. Do not suggest blind retries. Explain exactly what is wrong and what the user needs to change."
        }
        ToolBlockerKind::Retryable => {
            "[SYSTEM] Recent tool attempts failed for temporary reasons such as timeout, network issues, or upstream pressure. If you cannot recover with a clearly different path, explain that the blocker looks temporary, what was tried, and when retrying could help."
        }
        ToolBlockerKind::Mixed => {
            "[SYSTEM] Recent tool attempts hit multiple blockers. Do not end vaguely. Summarize the concrete blockers, separate what is temporary from what requires changed input or capability, and tell the user the next useful step."
        }
    })
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
    if content_signals_blocker(content) {
        return None;
    }
    if content_looks_like_process_transcript(content) {
        return Some(
            "[SYSTEM] Your draft reads like an execution transcript instead of a final user-facing answer. Do not show numbered step logs, progress headings, or future-plan sections. Either call the next tool now, or rewrite the answer to contain only completed results and the current conclusion."
                .to_string(),
        );
    }
    if !content_looks_generic_after_tool_success(content) {
        return None;
    }
    Some(
        if recent_successful_round.successful_calls == recent_successful_round.total_calls {
            "[SYSTEM] Recent tool calls already produced concrete results. Answer the latest user request directly from those results now. Do not give a vague summary or meta wrap-up."
            .to_string()
        } else {
            format!(
                "[SYSTEM] Recent tool calls already produced {} useful result(s). Answer directly from those successful results now. Mention only the specific remaining gap if it still matters, and do not end with generic wrap-up text.",
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
    if content.chars().count() < 24 || content_signals_blocker(content) {
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

fn content_signals_blocker(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let markers = [
        "阻塞",
        "卡住",
        "受限",
        "限制",
        "无法",
        "不能",
        "没法",
        "没有权限",
        "没有工具",
        "不支持",
        "做不到",
        "失败",
        "blocker",
        "blocked",
        "limitation",
        "limited",
        "cannot",
        "can't",
        "unable",
        "failed",
        "do not have access",
        "don't have access",
        "not available",
        "not supported",
    ];
    markers
        .iter()
        .any(|marker| content.contains(marker) || lower.contains(marker))
}

fn content_looks_generic_after_tool_success(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.chars().count() >= 140 || content_has_concrete_anchor(trimmed) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let generic_markers = [
        "简短总结",
        "当前情况",
        "目前情况",
        "先给你",
        "目前来看",
        "供你参考",
        "希望有帮助",
        "如果你需要",
        "如需我可以继续",
        "summary",
        "currently",
        "at the moment",
        "for reference",
        "hope this helps",
        "if you want",
        "if you'd like",
        "let me know",
    ];
    generic_markers
        .iter()
        .any(|marker| trimmed.contains(marker) || lower.contains(marker))
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
    use crate::bus::{IngressKind, PcMsg};
    use crate::orchestrator::PressureLevel;
    use crate::util::truncate_content_to_max;

    const PLAN_HINT_PREFIX: &str = "\n\n## Execution plan\n";
    const PLAN_HINT_SUFFIX: &str =
        "\nFollow this plan, but adapt immediately when tool results contradict it.";
    const PLAN_MAX_CHARS: usize = 512;

    fn should_generate_execution_plan(
        msg: &PcMsg,
        has_tools: bool,
        pressure: PressureLevel,
    ) -> bool {
        if msg.ingress != IngressKind::User
            || msg.is_group
            || !has_tools
            || pressure != PressureLevel::Normal
        {
            return false;
        }
        let content = msg.content.trim();
        let char_count = content.chars().count();
        let separators = ['\n', ',', '，', '.', '。', '?', '？', ';', '；'];
        let separator_count = content.chars().filter(|ch| separators.contains(ch)).count();
        let task_markers = [
            "然后",
            "并且",
            "同时",
            "先",
            "再",
            "分别",
            "步骤",
            "排查",
            "分析",
            "修复",
            "设计",
            "implement",
            "debug",
            "analyze",
            "review",
            "plan",
        ];
        let marker_hits = task_markers
            .iter()
            .filter(|marker| content.contains(**marker))
            .count();
        if char_count < 48 && separator_count < 2 && marker_hits < 2 {
            return false;
        }
        separator_count >= 2 || marker_hits >= 2 || char_count >= 120
    }

    fn append_execution_plan(system: &mut String, max_len: usize, plan: &str) {
        let plan = truncate_content_to_max(plan.trim(), PLAN_MAX_CHARS);
        if plan.is_empty() {
            return;
        }
        let addition = format!("{}{}{}", PLAN_HINT_PREFIX, plan, PLAN_HINT_SUFFIX);
        if system.len().saturating_add(addition.len()) <= max_len {
            system.push_str(&addition);
        }
    }

    #[test]
    fn complex_requests_trigger_internal_planning() {
        let msg = PcMsg::new_inbound(
            "telegram",
            "chat",
            "先分析问题，再给出修复步骤，并说明验证方式。",
            false,
        )
        .expect("pcmsg");
        assert!(should_generate_execution_plan(
            &msg,
            true,
            PressureLevel::Normal
        ));
    }

    #[test]
    fn simple_requests_skip_internal_planning() {
        let msg = PcMsg::new_inbound("telegram", "chat", "现在几点", false).expect("pcmsg");
        assert!(!should_generate_execution_plan(
            &msg,
            true,
            PressureLevel::Normal
        ));
    }

    #[test]
    fn execution_plan_hint_respects_system_budget() {
        let mut system = "base".to_string();
        append_execution_plan(&mut system, 12, "1. step");
        assert_eq!(system, "base");
    }

    #[test]
    fn tool_round_guidance_escalates_after_repeated_stalls() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            3,
            2,
            1,
            ToolFailureSummary {
                failed_calls: 2,
                retryable_failures: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("multiple tool rounds without useful progress"));
    }

    #[test]
    fn tool_round_guidance_prioritizes_ping_pong_loops() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            4,
            1,
            0,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            true,
        )
        .expect("guidance");
        assert!(guidance.contains("bouncing between two failed call patterns"));
    }

    #[test]
    fn tool_round_guidance_surfaces_capability_limits() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            1,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                capability_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("blocked by permissions"));
    }

    #[test]
    fn tool_round_guidance_surfaces_permanent_input_failures() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            1,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                permanent_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("invalid input"));
    }

    #[test]
    fn tool_round_guidance_surfaces_retryable_failures_after_stalls() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            false,
            2,
            2,
            0,
            ToolFailureSummary {
                failed_calls: 2,
                retryable_failures: 2,
                ..ToolFailureSummary::default()
            },
            false,
        )
        .expect("guidance");
        assert!(guidance.contains("transient reasons"));
    }

    #[test]
    fn tool_round_guidance_skips_partial_success_rounds() {
        let guidance = build_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            true,
            1,
            2,
            1,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
            false,
        );
        assert!(guidance.is_none());
    }

    #[test]
    fn blocker_end_turn_followup_targets_capability_limits() {
        let followup = blocker_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(ToolBlockerSummary {
                kind: ToolBlockerKind::Capability,
                failed_calls: 2,
                total_calls: 2,
            }),
            "我先给你一个简短总结。",
        );
        assert!(followup.is_some_and(|text| text.contains("capability is missing")));
    }

    #[test]
    fn blocker_end_turn_followup_skips_explicit_blockers() {
        let followup = blocker_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(ToolBlockerSummary {
                kind: ToolBlockerKind::Permanent,
                failed_calls: 1,
                total_calls: 1,
            }),
            "我无法继续，因为路径不存在。",
        );
        assert!(followup.is_none());
    }

    #[test]
    fn success_tool_round_guidance_prefers_direct_answer_after_full_success() {
        let guidance = build_success_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            2,
            ToolFailureSummary::default(),
        )
        .expect("guidance");
        assert!(guidance.contains("respond directly"));
    }

    #[test]
    fn success_tool_round_guidance_handles_mixed_success_rounds() {
        let guidance = build_success_tool_round_guidance(
            AgentRunStrategy::LinuxEnhanced,
            3,
            ToolFailureSummary {
                failed_calls: 1,
                permanent_failures: 1,
                ..ToolFailureSummary::default()
            },
        )
        .expect("guidance");
        assert!(guidance.contains("Some tool calls succeeded"));
    }

    #[test]
    fn final_answer_followup_targets_generic_wrapups_after_tool_success() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 2,
                successful_calls: 2,
            }),
            "我先给你一个简短总结，供你参考。",
        )
        .expect("followup");
        assert!(followup.contains("Answer the latest user request directly"));
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
    fn final_answer_followup_skips_non_linux_strategy() {
        let followup = final_answer_followup(
            AgentRunStrategy::Embedded,
            Some(SuccessfulToolRoundSummary {
                total_calls: 1,
                successful_calls: 1,
            }),
            "我先给你一个简短总结。",
        );
        assert!(followup.is_none());
    }

    #[test]
    fn final_answer_followup_mentions_successful_subset_for_mixed_rounds() {
        let followup = final_answer_followup(
            AgentRunStrategy::LinuxEnhanced,
            Some(SuccessfulToolRoundSummary {
                total_calls: 3,
                successful_calls: 2,
            }),
            "目前来看，我先给你一个简短总结。",
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

    #[test]
    fn ping_pong_detection_matches_alternating_rounds() {
        assert!(detect_ping_pong_tool_rounds(&[
            Some(11),
            Some(29),
            Some(11),
            Some(29),
        ]));
    }

    #[test]
    fn ping_pong_detection_rejects_single_pattern_repeats() {
        assert!(!detect_ping_pong_tool_rounds(&[
            Some(11),
            Some(11),
            Some(11),
            Some(11),
        ]));
    }

    #[test]
    fn stalled_end_turn_followup_requires_blocker_or_new_strategy() {
        let followup = stalled_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            2,
            "我先给你一个很简短的结论。",
        );
        assert!(followup.is_some());
    }

    #[test]
    fn stalled_end_turn_followup_skips_explicit_blockers() {
        let followup = stalled_end_turn_followup(
            AgentRunStrategy::LinuxEnhanced,
            2,
            "我无法继续，因为当前工具没有权限读取这个资源。",
        );
        assert!(followup.is_none());
    }
}
