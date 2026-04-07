//! Cross-plane rerank kernel for prompt-time recall decisions.
//! 跨记忆平面的统一重排内核：给 prompt 路由和 recall inspection 提供同构信号。

use super::{RecallPlane, RecallSelectionReport};
use crate::memory::PromptRecallIntent;
use serde::{Deserialize, Serialize};

const CROSS_PLANE_TOP_CANDIDATES: usize = 6;
const PLANE_SIGNAL_TOP_K: usize = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossPlaneRerankCandidate {
    pub plane: RecallPlane,
    #[serde(default)]
    pub candidate_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub original_total_score: u32,
    #[serde(default)]
    pub rerank_score: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossPlanePlaneSignal {
    pub plane: RecallPlane,
    #[serde(default)]
    pub candidate_count: usize,
    #[serde(default)]
    pub selected_count: usize,
    #[serde(default)]
    pub top_rerank_score: u32,
    #[serde(default)]
    pub signal_score: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_candidate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_reason: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossPlaneRerankResult {
    pub intent: PromptRecallIntent,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plane_signals: Vec<CrossPlanePlaneSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_candidates: Vec<CrossPlaneRerankCandidate>,
}

pub(crate) struct CrossPlaneRerankInput<'a> {
    pub intent: PromptRecallIntent,
    pub shared_factual_report: &'a RecallSelectionReport,
    pub continuity_capsule_report: &'a RecallSelectionReport,
    pub archive_report: &'a RecallSelectionReport,
    pub runtime_skill_report: &'a RecallSelectionReport,
    pub task_recall_report: Option<&'a RecallSelectionReport>,
}

pub(crate) fn build_cross_plane_rerank_result(
    input: CrossPlaneRerankInput<'_>,
) -> CrossPlaneRerankResult {
    let reports = [
        Some(input.shared_factual_report),
        Some(input.continuity_capsule_report),
        Some(input.archive_report),
        Some(input.runtime_skill_report),
        input.task_recall_report,
    ];
    let mut plane_signals = Vec::new();
    let mut top_candidates = Vec::new();

    for report in reports.into_iter().flatten() {
        let mut ranked = report
            .candidates
            .iter()
            .map(|candidate| CrossPlaneRerankCandidate {
                plane: candidate.plane,
                candidate_id: candidate.candidate_id.clone(),
                title: candidate.title.clone(),
                source: candidate.source.clone(),
                selected: candidate.selected,
                original_total_score: candidate.score.total_score,
                rerank_score: rerank_candidate_score(input.intent, report, candidate),
                reasons: rerank_reason_fragments(input.intent, report, candidate),
            })
            .filter(|candidate| candidate.rerank_score > 0)
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .rerank_score
                .cmp(&left.rerank_score)
                .then_with(|| right.selected.cmp(&left.selected))
                .then_with(|| right.original_total_score.cmp(&left.original_total_score))
                .then_with(|| {
                    plane_rank(input.intent, right.plane).cmp(&plane_rank(input.intent, left.plane))
                })
                .then_with(|| left.candidate_id.cmp(&right.candidate_id))
        });
        if ranked.is_empty() && report.candidate_count == 0 && report.selected_count == 0 {
            continue;
        }
        let signal_score = ranked.iter().take(PLANE_SIGNAL_TOP_K).enumerate().fold(
            0u32,
            |acc, (index, candidate)| {
                acc.saturating_add(if index == 0 {
                    candidate.rerank_score
                } else {
                    candidate.rerank_score / 2
                })
            },
        );
        let top_candidate = ranked.first();
        plane_signals.push(CrossPlanePlaneSignal {
            plane: report.plane,
            candidate_count: report.candidate_count,
            selected_count: report.selected_count,
            top_rerank_score: top_candidate
                .map(|candidate| candidate.rerank_score)
                .unwrap_or(0),
            signal_score,
            top_candidate_id: top_candidate.map(|candidate| candidate.candidate_id.clone()),
            top_reason: top_candidate.and_then(|candidate| candidate.reasons.first().cloned()),
        });
        top_candidates.extend(ranked.into_iter().take(PLANE_SIGNAL_TOP_K));
    }

    plane_signals.sort_by(|left, right| {
        right
            .signal_score
            .cmp(&left.signal_score)
            .then_with(|| right.top_rerank_score.cmp(&left.top_rerank_score))
            .then_with(|| {
                plane_rank(input.intent, right.plane).cmp(&plane_rank(input.intent, left.plane))
            })
    });
    top_candidates.sort_by(|left, right| {
        right
            .rerank_score
            .cmp(&left.rerank_score)
            .then_with(|| right.selected.cmp(&left.selected))
            .then_with(|| right.original_total_score.cmp(&left.original_total_score))
            .then_with(|| {
                plane_rank(input.intent, right.plane).cmp(&plane_rank(input.intent, left.plane))
            })
            .then_with(|| left.candidate_id.cmp(&right.candidate_id))
    });
    top_candidates.truncate(CROSS_PLANE_TOP_CANDIDATES);

    CrossPlaneRerankResult {
        intent: input.intent,
        plane_signals,
        top_candidates,
    }
}

pub(crate) fn plane_signal_score(result: &CrossPlaneRerankResult, plane: RecallPlane) -> u32 {
    result
        .plane_signals
        .iter()
        .find(|signal| signal.plane == plane)
        .map(|signal| signal.signal_score)
        .unwrap_or(0)
}

fn rerank_candidate_score(
    intent: PromptRecallIntent,
    report: &RecallSelectionReport,
    candidate: &super::RecallCandidate,
) -> u32 {
    let score = &candidate.score;
    let mut rerank = plane_intent_bonus(intent, candidate.plane);
    rerank = rerank
        .saturating_add(score.exact_match_score.min(24).saturating_mul(2))
        .saturating_add(score.lexical_score.min(24))
        .saturating_add(score.semantic_score.min(20))
        .saturating_add(score.scope_affinity_score.min(12).saturating_mul(2))
        .saturating_add(score.recency_score.min(8))
        .saturating_add(score.confidence_score.min(8))
        .saturating_add(score.importance_score.min(8))
        .saturating_add(score.governance_score.min(8).saturating_mul(2))
        .saturating_add(score.source_score.min(8))
        .saturating_add(score.total_score.min(24) / 2);
    if candidate.selected {
        rerank = rerank.saturating_add(4);
    }
    if matches!(intent, PromptRecallIntent::Evidence) && candidate.citation.is_some() {
        rerank = rerank.saturating_add(4);
    }
    if report.query.exact_lookup.is_some() && candidate.plane == RecallPlane::SharedFactual {
        rerank = rerank.saturating_add(16);
    }
    rerank
}

fn rerank_reason_fragments(
    intent: PromptRecallIntent,
    _report: &RecallSelectionReport,
    candidate: &super::RecallCandidate,
) -> Vec<String> {
    let mut reasons = Vec::with_capacity(candidate.score.reason_fragments.len().saturating_add(2));
    reasons.push(format!("intent={}", intent.label()));
    reasons.push(format!("plane={}", candidate.plane.label()));
    reasons.extend(
        candidate
            .score
            .reason_fragments
            .iter()
            .filter_map(|reason| {
                let trimmed = reason.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .take(4),
    );
    reasons
}

fn plane_intent_bonus(intent: PromptRecallIntent, plane: RecallPlane) -> u32 {
    match intent {
        PromptRecallIntent::Factual => match plane {
            RecallPlane::SharedFactual => 18,
            RecallPlane::ContinuityCapsule => 8,
            RecallPlane::Archive => 3,
            RecallPlane::RuntimeSkill => 2,
            RecallPlane::TaskRecall => 4,
        },
        PromptRecallIntent::Procedural => match plane {
            RecallPlane::SharedFactual => 2,
            RecallPlane::ContinuityCapsule => 10,
            RecallPlane::Archive => 3,
            RecallPlane::RuntimeSkill => 18,
            RecallPlane::TaskRecall => 14,
        },
        PromptRecallIntent::Continuity => match plane {
            RecallPlane::SharedFactual => 5,
            RecallPlane::ContinuityCapsule => 20,
            RecallPlane::Archive => 8,
            RecallPlane::RuntimeSkill => 5,
            RecallPlane::TaskRecall => 16,
        },
        PromptRecallIntent::Evidence => match plane {
            RecallPlane::SharedFactual => 2,
            RecallPlane::ContinuityCapsule => 8,
            RecallPlane::Archive => 32,
            RecallPlane::RuntimeSkill => 1,
            RecallPlane::TaskRecall => 2,
        },
        PromptRecallIntent::Mixed => match plane {
            RecallPlane::SharedFactual => 12,
            RecallPlane::ContinuityCapsule => 10,
            RecallPlane::Archive => 8,
            RecallPlane::RuntimeSkill => 7,
            RecallPlane::TaskRecall => 7,
        },
    }
}

fn plane_rank(intent: PromptRecallIntent, plane: RecallPlane) -> u32 {
    plane_intent_bonus(intent, plane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{RecallCandidate, RecallQuery, RecallScoreBreakdown};

    fn report(plane: RecallPlane, scores: &[(u32, bool, &str)]) -> RecallSelectionReport {
        RecallSelectionReport {
            plane,
            query: RecallQuery {
                plane,
                ..RecallQuery::default()
            },
            backend: "test".to_string(),
            candidate_count: scores.len(),
            selected_count: scores.iter().filter(|(_, selected, _)| *selected).count(),
            selected_ids: Vec::new(),
            miss_reason: None,
            selection_note: None,
            candidates: scores
                .iter()
                .enumerate()
                .map(|(idx, (score, selected, reason))| RecallCandidate {
                    plane,
                    candidate_id: format!("{}-{idx}", plane.label()),
                    title: format!("{}-{idx}", plane.label()),
                    excerpt: String::new(),
                    citation: (plane == RecallPlane::Archive).then(|| "archive:1".to_string()),
                    source: plane.label().to_string(),
                    observed_at: None,
                    selected: *selected,
                    score: RecallScoreBreakdown {
                        total_score: *score,
                        exact_match_score: (*score / 3).min(12),
                        lexical_score: (*score / 2).min(18),
                        scope_affinity_score: 4,
                        governance_score: 4,
                        reason_fragments: vec![(*reason).to_string()],
                        ..RecallScoreBreakdown::default()
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn procedural_rerank_prefers_runtime_and_task_planes() {
        let result = build_cross_plane_rerank_result(CrossPlaneRerankInput {
            intent: PromptRecallIntent::Procedural,
            shared_factual_report: &report(RecallPlane::SharedFactual, &[(22, true, "fact")]),
            continuity_capsule_report: &report(
                RecallPlane::ContinuityCapsule,
                &[(24, true, "handoff")],
            ),
            archive_report: &report(RecallPlane::Archive, &[(20, true, "archive")]),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, &[(28, true, "procedure")]),
            task_recall_report: Some(&report(RecallPlane::TaskRecall, &[(26, true, "run")])),
        });
        assert_eq!(result.top_candidates[0].plane, RecallPlane::RuntimeSkill);
        assert_eq!(result.plane_signals[0].plane, RecallPlane::RuntimeSkill);
        assert!(
            plane_signal_score(&result, RecallPlane::RuntimeSkill)
                >= plane_signal_score(&result, RecallPlane::Archive)
        );
    }

    #[test]
    fn evidence_rerank_prefers_archive_plane() {
        let result = build_cross_plane_rerank_result(CrossPlaneRerankInput {
            intent: PromptRecallIntent::Evidence,
            shared_factual_report: &report(RecallPlane::SharedFactual, &[(24, true, "fact")]),
            continuity_capsule_report: &report(
                RecallPlane::ContinuityCapsule,
                &[(24, true, "capsule")],
            ),
            archive_report: &report(RecallPlane::Archive, &[(20, true, "archive trace")]),
            runtime_skill_report: &report(RecallPlane::RuntimeSkill, &[(18, true, "skill")]),
            task_recall_report: None,
        });
        assert_eq!(result.plane_signals[0].plane, RecallPlane::Archive);
        assert_eq!(result.top_candidates[0].plane, RecallPlane::Archive);
    }
}
