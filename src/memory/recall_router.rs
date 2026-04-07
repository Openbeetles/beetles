//! Prompt-time recall router across governed memory planes.
//! Prompt 阶段的 recall 路由：根据结构信号与各 plane 命中质量决定主提示词投影顺序。

use super::RecallSelectionReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromptRecallIntent {
    Factual,
    Procedural,
    Continuity,
    Evidence,
    Mixed,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PromptRecallRouterDecision {
    pub intent: PromptRecallIntent,
}

pub(crate) struct PromptRecallRouterInput<'a> {
    pub user_query: &'a str,
    pub has_execution_state: bool,
    pub has_active_task: bool,
    pub shared_factual_report: &'a RecallSelectionReport,
    pub continuity_capsule_report: &'a RecallSelectionReport,
    pub archive_report: &'a RecallSelectionReport,
    pub runtime_skill_report: &'a RecallSelectionReport,
    pub task_recall_report: Option<&'a RecallSelectionReport>,
}

impl PromptRecallRouterDecision {
    pub fn active_task_parts<'a>(
        self,
        execution_state_text: Option<&'a str>,
        task_workspace_text: Option<&'a str>,
        task_recall_text: Option<&'a str>,
        continuity_capsule_text: Option<&'a str>,
    ) -> [Option<&'a str>; 4] {
        match self.intent {
            PromptRecallIntent::Continuity => [
                execution_state_text,
                continuity_capsule_text,
                task_workspace_text,
                task_recall_text,
            ],
            _ => [
                execution_state_text,
                task_workspace_text,
                task_recall_text,
                None,
            ],
        }
    }

    pub fn governed_memory_parts<'a>(
        self,
        long_term_memory_text: Option<&'a str>,
        continuity_capsule_text: Option<&'a str>,
        archive_evidence_text: Option<&'a str>,
        runtime_skill_text: Option<&'a str>,
    ) -> [Option<&'a str>; 4] {
        match self.intent {
            PromptRecallIntent::Factual => [
                long_term_memory_text,
                continuity_capsule_text,
                archive_evidence_text,
                runtime_skill_text,
            ],
            PromptRecallIntent::Procedural => [
                runtime_skill_text,
                continuity_capsule_text,
                archive_evidence_text,
                long_term_memory_text,
            ],
            PromptRecallIntent::Continuity => [
                archive_evidence_text,
                long_term_memory_text,
                runtime_skill_text,
                None,
            ],
            PromptRecallIntent::Evidence => [
                archive_evidence_text,
                continuity_capsule_text,
                long_term_memory_text,
                runtime_skill_text,
            ],
            PromptRecallIntent::Mixed => [
                long_term_memory_text,
                continuity_capsule_text,
                archive_evidence_text,
                runtime_skill_text,
            ],
        }
    }
}

pub(crate) fn decide_prompt_recall_route(
    input: PromptRecallRouterInput<'_>,
) -> PromptRecallRouterDecision {
    if input.shared_factual_report.query.exact_lookup.is_some() {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Factual,
        };
    }

    let query_is_weak = structurally_weak_query(input.user_query);
    let factual_signal = top_selected_score(input.shared_factual_report)
        .saturating_add(u32::from(input.shared_factual_report.query.exact_lookup.is_some()) * 48);
    let continuity_signal = top_selected_score(input.continuity_capsule_report)
        .saturating_add(u32::from(input.has_active_task) * 8)
        .saturating_add(u32::from(input.has_execution_state) * 4)
        .saturating_add(u32::from(query_is_weak) * 8);
    let procedural_signal = top_selected_score(input.runtime_skill_report)
        .saturating_add(
            input
                .task_recall_report
                .map(top_selected_score)
                .unwrap_or(0)
                / 2,
        )
        .saturating_add(u32::from(input.has_active_task) * 4);
    let evidence_signal = top_selected_score(input.archive_report);

    if input.has_active_task
        && query_is_weak
        && continuity_signal >= factual_signal
        && continuity_signal >= evidence_signal
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Continuity,
        };
    }

    if procedural_signal > 0
        && procedural_signal >= continuity_signal
        && procedural_signal >= evidence_signal
        && procedural_signal >= factual_signal.saturating_add(4)
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Procedural,
        };
    }

    if evidence_signal > 0
        && evidence_signal >= continuity_signal
        && evidence_signal >= procedural_signal
        && evidence_signal >= factual_signal.saturating_add(6)
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Evidence,
        };
    }

    if continuity_signal > 0
        && continuity_signal >= factual_signal
        && continuity_signal >= procedural_signal
        && continuity_signal >= evidence_signal
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Continuity,
        };
    }

    if factual_signal > 0
        && factual_signal >= continuity_signal
        && factual_signal >= procedural_signal
        && factual_signal >= evidence_signal
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Factual,
        };
    }

    if evidence_signal > 0
        && factual_signal == 0
        && procedural_signal == 0
        && continuity_signal == 0
    {
        return PromptRecallRouterDecision {
            intent: PromptRecallIntent::Evidence,
        };
    }

    PromptRecallRouterDecision {
        intent: PromptRecallIntent::Mixed,
    }
}

fn top_selected_score(report: &RecallSelectionReport) -> u32 {
    report
        .candidates
        .iter()
        .filter(|candidate| candidate.selected)
        .map(|candidate| candidate.score.total_score)
        .max()
        .unwrap_or(0)
}

fn structurally_weak_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    let char_count = trimmed.chars().count();
    let term_count = super::archive_search::collect_archive_match_terms(trimmed).len();
    char_count <= 12 && term_count <= 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{RecallCandidate, RecallPlane, RecallQuery, RecallScoreBreakdown};

    fn report(plane: RecallPlane, score: u32, selected: bool) -> RecallSelectionReport {
        RecallSelectionReport {
            plane,
            query: RecallQuery {
                plane,
                ..RecallQuery::default()
            },
            backend: "test".to_string(),
            candidate_count: usize::from(score > 0),
            selected_count: usize::from(selected && score > 0),
            selected_ids: Vec::new(),
            miss_reason: None,
            selection_note: None,
            candidates: (score > 0)
                .then(|| {
                    vec![RecallCandidate {
                        plane,
                        candidate_id: "c1".to_string(),
                        title: "candidate".to_string(),
                        excerpt: String::new(),
                        citation: None,
                        source: "test".to_string(),
                        observed_at: None,
                        selected,
                        score: RecallScoreBreakdown {
                            total_score: score,
                            ..RecallScoreBreakdown::default()
                        },
                    }]
                })
                .unwrap_or_default(),
        }
    }

    #[test]
    fn weak_active_task_turn_routes_to_continuity() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "继续",
            has_execution_state: true,
            has_active_task: true,
            shared_factual_report: &report(RecallPlane::SharedFactual, 18, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 16, true),
            archive_report: &report(RecallPlane::Archive, 12, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 10, true),
            task_recall_report: Some(&report(RecallPlane::TaskRecall, 14, true)),
        });
        assert_eq!(decision.intent, PromptRecallIntent::Continuity);
    }

    #[test]
    fn runtime_skill_dominance_routes_to_procedural() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "按之前那套流程发布",
            has_execution_state: false,
            has_active_task: false,
            shared_factual_report: &report(RecallPlane::SharedFactual, 12, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 8, true),
            archive_report: &report(RecallPlane::Archive, 10, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 30, true),
            task_recall_report: None,
        });
        assert_eq!(decision.intent, PromptRecallIntent::Procedural);
    }

    #[test]
    fn archive_dominance_routes_to_evidence() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "把之前那次日志原文翻出来",
            has_execution_state: false,
            has_active_task: false,
            shared_factual_report: &report(RecallPlane::SharedFactual, 8, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 0, false),
            archive_report: &report(RecallPlane::Archive, 22, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 0, false),
            task_recall_report: None,
        });
        assert_eq!(decision.intent, PromptRecallIntent::Evidence);
    }
}
