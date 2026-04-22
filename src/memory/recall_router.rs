//! Prompt-time recall router across governed memory planes.
//! Prompt 阶段的 recall 路由：根据结构信号与各 plane 命中质量决定主提示词投影顺序。

use super::{
    build_cross_plane_rerank_result, plane_signal_score, CrossPlaneRerankInput, RecallPlane,
    RecallSelectionReport, SessionMessage,
};
use crate::agent::ActiveWorkRecord;
use crate::task_execution::TaskRunRecord;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptRecallIntent {
    #[default]
    Factual,
    Procedural,
    Continuity,
    Evidence,
    Mixed,
}

impl PromptRecallIntent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Factual => "factual",
            Self::Procedural => "procedural",
            Self::Continuity => "continuity",
            Self::Evidence => "evidence",
            Self::Mixed => "mixed",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PromptRecallRouterDecision {
    pub intent: PromptRecallIntent,
}

pub(crate) struct PromptRecallRouterInput<'a> {
    pub user_query: &'a str,
    pub has_active_continuity: bool,
    pub has_active_task_run: bool,
    pub shared_factual_report: &'a RecallSelectionReport,
    pub continuity_capsule_report: &'a RecallSelectionReport,
    pub archive_report: &'a RecallSelectionReport,
    pub runtime_skill_report: &'a RecallSelectionReport,
    pub task_recall_report: Option<&'a RecallSelectionReport>,
}

impl PromptRecallRouterDecision {
    pub fn active_task_parts<'a>(
        self,
        work_continuity_text: Option<&'a str>,
        recent_turn_observation_text: Option<&'a str>,
        task_workspace_text: Option<&'a str>,
        task_recall_text: Option<&'a str>,
        continuity_capsule_text: Option<&'a str>,
    ) -> [Option<&'a str>; 5] {
        match self.intent {
            PromptRecallIntent::Continuity => [
                work_continuity_text,
                recent_turn_observation_text,
                continuity_capsule_text,
                task_workspace_text,
                task_recall_text,
            ],
            _ => [
                work_continuity_text,
                recent_turn_observation_text,
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
                continuity_capsule_text,
                archive_evidence_text,
                long_term_memory_text,
                runtime_skill_text,
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

pub(crate) fn build_continuity_recall_query(
    user_query: &str,
    summary_text: Option<&str>,
    recent_messages: &[SessionMessage],
    active_work: Option<&ActiveWorkRecord>,
    active_task_run: Option<&TaskRunRecord>,
) -> String {
    let trimmed = user_query.trim();
    if !structurally_weak_query(trimmed) {
        return trimmed.to_string();
    }
    let expanded = [
        Some(trimmed),
        active_task_run.map(|record| record.run.title.trim()),
        active_task_run.map(|record| record.plan.goal.trim()),
        active_work.map(|record| record.title.trim()),
        active_work.map(|record| record.progress_summary.trim()),
        active_work.map(|record| record.next_action.trim()),
        summary_text.map(str::trim),
        recent_messages
            .iter()
            .rev()
            .take(2)
            .map(|message| message.content.trim())
            .find(|value| !value.is_empty()),
    ]
    .into_iter()
    .flatten()
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>()
    .join(" ");
    if expanded.is_empty() {
        trimmed.to_string()
    } else {
        expanded
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
    let factual_rerank = build_cross_plane_rerank_result(CrossPlaneRerankInput {
        intent: PromptRecallIntent::Factual,
        shared_factual_report: input.shared_factual_report,
        continuity_capsule_report: input.continuity_capsule_report,
        archive_report: input.archive_report,
        runtime_skill_report: input.runtime_skill_report,
        task_recall_report: input.task_recall_report,
    });
    let procedural_rerank = build_cross_plane_rerank_result(CrossPlaneRerankInput {
        intent: PromptRecallIntent::Procedural,
        shared_factual_report: input.shared_factual_report,
        continuity_capsule_report: input.continuity_capsule_report,
        archive_report: input.archive_report,
        runtime_skill_report: input.runtime_skill_report,
        task_recall_report: input.task_recall_report,
    });
    let continuity_rerank = build_cross_plane_rerank_result(CrossPlaneRerankInput {
        intent: PromptRecallIntent::Continuity,
        shared_factual_report: input.shared_factual_report,
        continuity_capsule_report: input.continuity_capsule_report,
        archive_report: input.archive_report,
        runtime_skill_report: input.runtime_skill_report,
        task_recall_report: input.task_recall_report,
    });
    let evidence_rerank = build_cross_plane_rerank_result(CrossPlaneRerankInput {
        intent: PromptRecallIntent::Evidence,
        shared_factual_report: input.shared_factual_report,
        continuity_capsule_report: input.continuity_capsule_report,
        archive_report: input.archive_report,
        runtime_skill_report: input.runtime_skill_report,
        task_recall_report: input.task_recall_report,
    });
    let evidence_archive_signal = plane_signal_score(&evidence_rerank, RecallPlane::Archive);
    let procedural_runtime_signal =
        plane_signal_score(&procedural_rerank, RecallPlane::RuntimeSkill)
            .saturating_add(plane_signal_score(&procedural_rerank, RecallPlane::TaskRecall) / 2);

    let factual_signal = factual_support_signal(&factual_rerank)
        .saturating_add(u32::from(input.shared_factual_report.query.exact_lookup.is_some()) * 48);
    let continuity_signal = continuity_support_signal(
        &continuity_rerank,
        input.has_active_continuity,
        input.has_active_task_run,
        query_is_weak,
    )
    .saturating_add(u32::from(input.has_active_task_run) * 8)
    .saturating_add(u32::from(input.has_active_continuity) * 4)
    .saturating_add(u32::from(query_is_weak) * 8);
    let procedural_signal = procedural_support_signal(&procedural_rerank)
        .saturating_add(u32::from(input.has_active_task_run) * 4);
    let evidence_signal = evidence_support_signal(&evidence_rerank);

    if input.has_active_task_run
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
        && evidence_archive_signal
            >= plane_signal_score(&evidence_rerank, RecallPlane::SharedFactual)
        && evidence_archive_signal >= procedural_runtime_signal
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

fn factual_support_signal(result: &super::CrossPlaneRerankResult) -> u32 {
    plane_signal_score(result, RecallPlane::SharedFactual)
        .saturating_add(plane_signal_score(result, RecallPlane::ContinuityCapsule) / 3)
        .saturating_add(plane_signal_score(result, RecallPlane::Archive) / 5)
}

fn procedural_support_signal(result: &super::CrossPlaneRerankResult) -> u32 {
    let runtime = plane_signal_score(result, RecallPlane::RuntimeSkill);
    runtime
        .saturating_add(runtime / 2)
        .saturating_add(plane_signal_score(result, RecallPlane::TaskRecall) / 2)
        .saturating_add(plane_signal_score(result, RecallPlane::ContinuityCapsule) / 3)
}

fn continuity_support_signal(
    result: &super::CrossPlaneRerankResult,
    has_active_continuity: bool,
    has_active_task_run: bool,
    query_is_weak: bool,
) -> u32 {
    let continuity_capsule_signal = plane_signal_score(result, RecallPlane::ContinuityCapsule);
    let continuity_capsule_weighted =
        if has_active_continuity || has_active_task_run || query_is_weak {
            continuity_capsule_signal
        } else {
            continuity_capsule_signal / 2
        };
    continuity_capsule_weighted
        .saturating_add(plane_signal_score(result, RecallPlane::TaskRecall) / 2)
        .saturating_add(plane_signal_score(result, RecallPlane::Archive) / 4)
        .saturating_add(plane_signal_score(result, RecallPlane::SharedFactual) / 5)
}

fn evidence_support_signal(result: &super::CrossPlaneRerankResult) -> u32 {
    plane_signal_score(result, RecallPlane::Archive)
        .saturating_add(plane_signal_score(result, RecallPlane::ContinuityCapsule) / 4)
        .saturating_add(plane_signal_score(result, RecallPlane::SharedFactual) / 5)
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
            candidates: if score > 0 {
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
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn weak_active_task_turn_routes_to_continuity() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "继续",
            has_active_continuity: true,
            has_active_task_run: true,
            shared_factual_report: &report(RecallPlane::SharedFactual, 18, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 16, true),
            archive_report: &report(RecallPlane::Archive, 12, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 10, true),
            task_recall_report: Some(&report(RecallPlane::TaskRecall, 14, true)),
        });
        assert_eq!(decision.intent, PromptRecallIntent::Continuity);
    }

    #[test]
    fn weak_query_without_live_task_still_routes_to_continuity_when_capsule_matches() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "继续",
            has_active_continuity: false,
            has_active_task_run: false,
            shared_factual_report: &report(RecallPlane::SharedFactual, 8, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 28, true),
            archive_report: &report(RecallPlane::Archive, 6, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 0, false),
            task_recall_report: None,
        });
        assert_eq!(decision.intent, PromptRecallIntent::Continuity);
    }

    #[test]
    fn runtime_skill_dominance_routes_to_procedural() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "按之前那套流程发布",
            has_active_continuity: false,
            has_active_task_run: false,
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
            has_active_continuity: false,
            has_active_task_run: false,
            shared_factual_report: &report(RecallPlane::SharedFactual, 8, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 0, false),
            archive_report: &report(RecallPlane::Archive, 22, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 0, false),
            task_recall_report: None,
        });
        assert_eq!(decision.intent, PromptRecallIntent::Evidence);
    }

    #[test]
    fn archive_queries_without_live_task_do_not_let_capsules_override_evidence() {
        let decision = decide_prompt_recall_route(PromptRecallRouterInput {
            user_query: "把那次 network outage 的原始记录翻出来",
            has_active_continuity: false,
            has_active_task_run: false,
            shared_factual_report: &report(RecallPlane::SharedFactual, 12, true),
            continuity_capsule_report: &report(RecallPlane::ContinuityCapsule, 24, true),
            archive_report: &report(RecallPlane::Archive, 22, true),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, 0, false),
            task_recall_report: None,
        });
        assert_eq!(decision.intent, PromptRecallIntent::Evidence);
    }
}
