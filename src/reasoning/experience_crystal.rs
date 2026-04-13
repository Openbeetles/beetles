//! Experience crystal proposal contracts and runtime-skill integration.

use crate::error::{Error, Result};
use crate::platform::SkillStorage;
use crate::skills::{
    runtime_skill_name_for_topic, write_governed_runtime_skills, RuntimeSkillOperatorSummary,
    RuntimeSkillWrite, RuntimeSkillWriteOutcome, RuntimeSkillWriteSource,
};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;

const EXPERIENCE_CRYSTAL_MAX_SUMMARY_CHARS: usize = 220;
const EXPERIENCE_CRYSTAL_MAX_CANDIDATES: usize = 6;
const EXPERIENCE_CRYSTAL_MAX_TOPIC_CHARS: usize = 72;
const EXPERIENCE_CRYSTAL_MAX_TITLE_CHARS: usize = 96;
const EXPERIENCE_CRYSTAL_MAX_CANDIDATE_SUMMARY_CHARS: usize = 160;
const EXPERIENCE_CRYSTAL_MAX_MACRO_STEPS: usize = 8;
const EXPERIENCE_CRYSTAL_MIN_MACRO_STEPS: usize = 2;
const EXPERIENCE_CRYSTAL_MAX_MACRO_STEP_CHARS: usize = 180;
const EXPERIENCE_CRYSTAL_MAX_EVIDENCE_REFS: usize = 8;
const EXPERIENCE_CRYSTAL_MAX_EVIDENCE_REF_CHARS: usize = 120;
const EXPERIENCE_CRYSTAL_PROMOTION_MIN_SCORE: u8 = 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCrystalCandidate {
    pub topic: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub reusable_macro: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub success_score: u8,
    pub reuse_score: u8,
    pub promotion_readiness: u8,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCrystalResult {
    pub summary: String,
    #[serde(default)]
    pub skill_crystal_candidates: Vec<SkillCrystalCandidate>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ExperienceCrystalOperatorSummary {
    pub runtime_skill_total: usize,
    pub validated_runtime_skills: usize,
    pub revision_pending: usize,
    pub garbage_collectable: usize,
}

pub fn validate_skill_crystal_result(value: Value) -> Result<SkillCrystalResult> {
    let result: SkillCrystalResult = serde_json::from_value(value)
        .map_err(|error| Error::config("skill_crystal_result_decode", error.to_string()))?;
    normalize_skill_crystal_result(result)
}

pub fn skill_crystal_candidate_to_runtime_skill_write(
    candidate: &SkillCrystalCandidate,
    source_chat_id: Option<&str>,
    observed_at: u64,
) -> Result<RuntimeSkillWrite> {
    let candidate = normalize_skill_crystal_candidate(candidate.clone())?;
    if candidate.promotion_readiness < EXPERIENCE_CRYSTAL_PROMOTION_MIN_SCORE {
        return Err(Error::config(
            "skill_crystal_candidate_to_runtime_skill_write",
            format!(
                "promotion_readiness must be >= {}",
                EXPERIENCE_CRYSTAL_PROMOTION_MIN_SCORE
            ),
        ));
    }

    let mut content = String::new();
    for (index, step) in candidate.reusable_macro.iter().enumerate() {
        let _ = writeln!(content, "{}. {}", index + 1, step);
    }
    if content.ends_with('\n') {
        content.pop();
    }
    Ok(RuntimeSkillWrite {
        name: runtime_skill_name_for_topic(&candidate.topic),
        topic: candidate.topic,
        title: candidate.title,
        summary: candidate.summary,
        content,
        citations: candidate.evidence_refs,
        source_chat_id: source_chat_id.map(str::to_string),
        observed_at,
    })
}

pub fn promote_skill_crystal_candidates(
    storage: &dyn SkillStorage,
    candidates: &[SkillCrystalCandidate],
    source_chat_id: Option<&str>,
    observed_at: u64,
) -> Result<RuntimeSkillWriteOutcome> {
    let writes = candidates
        .iter()
        .map(|candidate| {
            skill_crystal_candidate_to_runtime_skill_write(candidate, source_chat_id, observed_at)
        })
        .collect::<Result<Vec<_>>>()?;
    write_governed_runtime_skills(
        storage,
        &writes,
        RuntimeSkillWriteSource::ProgrammableReasoning,
    )
}

pub fn build_experience_crystal_operator_summary(
    runtime_skills: &RuntimeSkillOperatorSummary,
) -> ExperienceCrystalOperatorSummary {
    ExperienceCrystalOperatorSummary {
        runtime_skill_total: runtime_skills.total,
        validated_runtime_skills: runtime_skills.validated,
        revision_pending: runtime_skills.revision_pending,
        garbage_collectable: runtime_skills
            .stale
            .saturating_add(runtime_skills.low_value),
    }
}

fn normalize_skill_crystal_result(result: SkillCrystalResult) -> Result<SkillCrystalResult> {
    let summary =
        truncate_content_to_max(result.summary.trim(), EXPERIENCE_CRYSTAL_MAX_SUMMARY_CHARS)
            .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "skill_crystal_result_validate",
            "missing summary",
        ));
    }
    if result.skill_crystal_candidates.len() > EXPERIENCE_CRYSTAL_MAX_CANDIDATES {
        return Err(Error::config(
            "skill_crystal_result_validate",
            format!(
                "skill_crystal_candidates exceeds {}",
                EXPERIENCE_CRYSTAL_MAX_CANDIDATES
            ),
        ));
    }
    let skill_crystal_candidates = result
        .skill_crystal_candidates
        .into_iter()
        .map(normalize_skill_crystal_candidate)
        .collect::<Result<Vec<_>>>()?;
    Ok(SkillCrystalResult {
        summary,
        skill_crystal_candidates,
    })
}

fn normalize_skill_crystal_candidate(
    candidate: SkillCrystalCandidate,
) -> Result<SkillCrystalCandidate> {
    if !candidate.requires_adjudication {
        return Err(Error::config(
            "skill_crystal_candidate_validate",
            "skill crystal candidate must require adjudication",
        ));
    }

    let topic = truncate_content_to_max(candidate.topic.trim(), EXPERIENCE_CRYSTAL_MAX_TOPIC_CHARS)
        .into_owned();
    let title = truncate_content_to_max(candidate.title.trim(), EXPERIENCE_CRYSTAL_MAX_TITLE_CHARS)
        .into_owned();
    let summary = truncate_content_to_max(
        candidate.summary.trim(),
        EXPERIENCE_CRYSTAL_MAX_CANDIDATE_SUMMARY_CHARS,
    )
    .into_owned();
    if topic.is_empty() || title.is_empty() || summary.is_empty() {
        return Err(Error::config(
            "skill_crystal_candidate_validate",
            "skill crystal candidate requires topic, title, and summary",
        ));
    }

    if candidate.reusable_macro.len() > EXPERIENCE_CRYSTAL_MAX_MACRO_STEPS {
        return Err(Error::config(
            "skill_crystal_candidate_validate",
            format!(
                "reusable_macro exceeds {}",
                EXPERIENCE_CRYSTAL_MAX_MACRO_STEPS
            ),
        ));
    }
    let reusable_macro = candidate
        .reusable_macro
        .into_iter()
        .map(|step| {
            truncate_content_to_max(step.trim(), EXPERIENCE_CRYSTAL_MAX_MACRO_STEP_CHARS)
                .into_owned()
        })
        .filter(|step| !step.is_empty())
        .collect::<Vec<_>>();
    if reusable_macro.len() < EXPERIENCE_CRYSTAL_MIN_MACRO_STEPS {
        return Err(Error::config(
            "skill_crystal_candidate_validate",
            format!(
                "reusable_macro requires at least {} non-empty steps",
                EXPERIENCE_CRYSTAL_MIN_MACRO_STEPS
            ),
        ));
    }

    if candidate.evidence_refs.len() > EXPERIENCE_CRYSTAL_MAX_EVIDENCE_REFS {
        return Err(Error::config(
            "skill_crystal_candidate_validate",
            format!(
                "evidence_refs exceeds {}",
                EXPERIENCE_CRYSTAL_MAX_EVIDENCE_REFS
            ),
        ));
    }
    let evidence_refs = candidate
        .evidence_refs
        .into_iter()
        .map(|reference| {
            truncate_content_to_max(reference.trim(), EXPERIENCE_CRYSTAL_MAX_EVIDENCE_REF_CHARS)
                .into_owned()
        })
        .filter(|reference| !reference.is_empty())
        .collect::<Vec<_>>();

    Ok(SkillCrystalCandidate {
        topic,
        title,
        summary,
        reusable_macro,
        evidence_refs,
        success_score: candidate.success_score.min(100),
        reuse_score: candidate.reuse_score.min(100),
        promotion_readiness: candidate.promotion_readiness.min(100),
        requires_adjudication: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::{
        build_runtime_skill_operator_summary, retrieve_runtime_skill_hits,
        write_governed_runtime_skills, RuntimeSkillWriteSource,
    };
    use crate::{Error, Result, SkillStorage};
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("skill_crystal_test", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(name);
            Ok(())
        }
    }

    #[test]
    fn validate_skill_crystal_result_accepts_scored_candidates() {
        let result = validate_skill_crystal_result(json!({
            "summary": "Prepared one crystal candidate.",
            "skill_crystal_candidates": [{
                "topic": "release_patch_flow",
                "title": "Release patch flow",
                "summary": "Stabilized patch-and-verify method.",
                "reusable_macro": [
                    "Inspect the release diff and identify rollback guards.",
                    "Apply the patch with rollback protection.",
                    "Verify logs and service health before exit."
                ],
                "evidence_refs": ["trace:req-1", "runtime_skill:release_patch_flow"],
                "success_score": 92,
                "reuse_score": 88,
                "promotion_readiness": 90,
                "requires_adjudication": true
            }]
        }))
        .expect("skill crystal candidate should validate");

        assert_eq!(result.skill_crystal_candidates.len(), 1);
        assert_eq!(result.skill_crystal_candidates[0].promotion_readiness, 90);
    }

    #[test]
    fn validate_skill_crystal_result_rejects_non_adjudicated_candidates() {
        let error = validate_skill_crystal_result(json!({
            "summary": "bad",
            "skill_crystal_candidates": [{
                "topic": "release_patch_flow",
                "title": "Release patch flow",
                "summary": "Stabilized patch-and-verify method.",
                "reusable_macro": ["Inspect diff", "Patch", "Verify"],
                "evidence_refs": ["trace:req-1"],
                "success_score": 92,
                "reuse_score": 88,
                "promotion_readiness": 90,
                "requires_adjudication": false
            }]
        }))
        .expect_err("candidate should be rejected");

        assert!(error.to_string().contains("require adjudication"));
    }

    #[test]
    fn promote_skill_crystal_candidates_writes_runtime_skill_with_macro() {
        let storage = StubSkillStorage::default();
        let outcome = promote_skill_crystal_candidates(
            &storage,
            &[SkillCrystalCandidate {
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Stabilized patch-and-verify method.".to_string(),
                reusable_macro: vec![
                    "Inspect the release diff and identify rollback guards.".to_string(),
                    "Apply the patch with rollback protection.".to_string(),
                    "Verify logs and service health before exit.".to_string(),
                ],
                evidence_refs: vec!["trace:req-1".to_string()],
                success_score: 92,
                reuse_score: 88,
                promotion_readiness: 90,
                requires_adjudication: true,
            }],
            Some("chat-1"),
            120,
        )
        .expect("promotion should succeed");

        assert_eq!(outcome.accepted, 1);
        let hits =
            retrieve_runtime_skill_hits(&storage, "继续按 release patch flow 执行", None, 200, 3);
        assert_eq!(hits.len(), 1);
        assert!(hits[0]
            .record
            .procedure
            .contains("1. Inspect the release diff"));
    }

    #[test]
    fn experience_crystal_operator_summary_reuses_runtime_skill_governance_signal() {
        let storage = StubSkillStorage::default();
        write_governed_runtime_skills(
            &storage,
            &[RuntimeSkillWrite {
                name: String::new(),
                topic: "release_patch_flow".to_string(),
                title: "Release patch flow".to_string(),
                summary: "Patch the release and verify the result".to_string(),
                content: "1. inspect release diff\n2. patch rollback guards\n3. verify logs"
                    .to_string(),
                citations: Vec::new(),
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 100,
            }],
            RuntimeSkillWriteSource::TaskLearning,
        )
        .expect("runtime skill write should succeed");

        let runtime_skills = build_runtime_skill_operator_summary(&storage);
        let summary = build_experience_crystal_operator_summary(&runtime_skills);
        assert_eq!(summary.runtime_skill_total, 1);
        assert_eq!(summary.garbage_collectable, 0);
    }
}
