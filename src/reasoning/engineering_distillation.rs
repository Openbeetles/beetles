//! Engineering reference distillation contracts for P7 synthesis workflows.

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const ENGINEERING_DISTILLATION_MAX_SUMMARY_CHARS: usize = 220;
const ENGINEERING_DISTILLATION_MAX_CANDIDATES: usize = 6;
const ENGINEERING_DISTILLATION_MAX_TITLE_CHARS: usize = 96;
const ENGINEERING_DISTILLATION_MAX_CANDIDATE_SUMMARY_CHARS: usize = 160;
const ENGINEERING_DISTILLATION_MAX_CONTENT_CHARS: usize = 4 * 1024;
const ENGINEERING_DISTILLATION_MAX_EVIDENCE_REFS: usize = 8;
const ENGINEERING_DISTILLATION_MAX_EVIDENCE_REF_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineeringDistillationAssetKind {
    RegisterTable,
    ProtocolFrame,
    StateMachine,
    DatasheetNote,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineeringDistillationAssetCandidate {
    pub kind: EngineeringDistillationAssetKind,
    pub title: String,
    pub summary: String,
    pub content: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineeringDistillationResult {
    pub summary: String,
    #[serde(default)]
    pub asset_candidates: Vec<EngineeringDistillationAssetCandidate>,
}

pub fn validate_engineering_distillation_result(
    value: Value,
) -> Result<EngineeringDistillationResult> {
    let result: EngineeringDistillationResult = serde_json::from_value(value).map_err(|error| {
        Error::config("engineering_distillation_result_decode", error.to_string())
    })?;
    normalize_engineering_distillation_result(result)
}

fn normalize_engineering_distillation_result(
    result: EngineeringDistillationResult,
) -> Result<EngineeringDistillationResult> {
    let summary = truncate_content_to_max(
        result.summary.trim(),
        ENGINEERING_DISTILLATION_MAX_SUMMARY_CHARS,
    )
    .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "engineering_distillation_result_validate",
            "missing summary",
        ));
    }
    if result.asset_candidates.len() > ENGINEERING_DISTILLATION_MAX_CANDIDATES {
        return Err(Error::config(
            "engineering_distillation_result_validate",
            format!(
                "asset_candidates exceeds {}",
                ENGINEERING_DISTILLATION_MAX_CANDIDATES
            ),
        ));
    }
    let asset_candidates = result
        .asset_candidates
        .into_iter()
        .map(normalize_engineering_distillation_candidate)
        .collect::<Result<Vec<_>>>()?;
    Ok(EngineeringDistillationResult {
        summary,
        asset_candidates,
    })
}

fn normalize_engineering_distillation_candidate(
    candidate: EngineeringDistillationAssetCandidate,
) -> Result<EngineeringDistillationAssetCandidate> {
    if !candidate.requires_adjudication {
        return Err(Error::config(
            "engineering_distillation_candidate_validate",
            "engineering distillation candidate must require adjudication",
        ));
    }

    let title = truncate_content_to_max(
        candidate.title.trim(),
        ENGINEERING_DISTILLATION_MAX_TITLE_CHARS,
    )
    .into_owned();
    let summary = truncate_content_to_max(
        candidate.summary.trim(),
        ENGINEERING_DISTILLATION_MAX_CANDIDATE_SUMMARY_CHARS,
    )
    .into_owned();
    let content = truncate_content_to_max(
        candidate.content.trim(),
        ENGINEERING_DISTILLATION_MAX_CONTENT_CHARS,
    )
    .into_owned();
    if title.is_empty() || summary.is_empty() || content.is_empty() {
        return Err(Error::config(
            "engineering_distillation_candidate_validate",
            "engineering distillation candidate requires title, summary, and content",
        ));
    }

    if candidate.evidence_refs.len() > ENGINEERING_DISTILLATION_MAX_EVIDENCE_REFS {
        return Err(Error::config(
            "engineering_distillation_candidate_validate",
            format!(
                "evidence_refs exceeds {}",
                ENGINEERING_DISTILLATION_MAX_EVIDENCE_REFS
            ),
        ));
    }
    let evidence_refs = candidate
        .evidence_refs
        .into_iter()
        .map(|reference| {
            truncate_content_to_max(
                reference.trim(),
                ENGINEERING_DISTILLATION_MAX_EVIDENCE_REF_CHARS,
            )
            .into_owned()
        })
        .filter(|reference| !reference.is_empty())
        .collect::<Vec<_>>();
    if evidence_refs.is_empty() {
        return Err(Error::config(
            "engineering_distillation_candidate_validate",
            "engineering distillation candidate requires evidence_refs",
        ));
    }

    Ok(EngineeringDistillationAssetCandidate {
        kind: candidate.kind,
        title,
        summary,
        content,
        evidence_refs,
        requires_adjudication: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_engineering_distillation_result_accepts_adjudicated_asset_candidates() {
        let result = validate_engineering_distillation_result(json!({
            "summary": "Distilled one candidate.",
            "asset_candidates": [{
                "kind": "state_machine",
                "title": "Boot state sketch",
                "summary": "Summarizes the boot progression for review.",
                "content": "RESET -> INIT -> READY",
                "evidence_refs": ["figure 2", "section 3.1"],
                "requires_adjudication": true
            }]
        }))
        .expect("valid distillation result");

        assert_eq!(result.asset_candidates.len(), 1);
        assert_eq!(
            result.asset_candidates[0].kind,
            EngineeringDistillationAssetKind::StateMachine
        );
    }

    #[test]
    fn validate_engineering_distillation_result_rejects_missing_evidence() {
        let error = validate_engineering_distillation_result(json!({
            "summary": "bad",
            "asset_candidates": [{
                "kind": "datasheet_note",
                "title": "Timing note",
                "summary": "Ungrounded timing note.",
                "content": "Delay 2 ms after reset.",
                "evidence_refs": [],
                "requires_adjudication": true
            }]
        }))
        .expect_err("missing evidence refs should be rejected");

        assert!(error
            .to_string()
            .contains("engineering distillation candidate requires evidence_refs"));
    }
}
