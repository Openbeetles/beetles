//! Memory attack and distillation contracts built on the programmable reasoning substrate.

use crate::error::{Error, Result};
use crate::memory::LongTermMemoryKind;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MEMORY_ATTACK_MAX_SUMMARY_CHARS: usize = 220;
const MEMORY_ATTACK_MAX_FINDINGS: usize = 12;
const MEMORY_ATTACK_MAX_DISTILLATION_CANDIDATES: usize = 8;
const MEMORY_ATTACK_MAX_FINDING_SUMMARY_CHARS: usize = 160;
const MEMORY_ATTACK_MAX_FINDING_RATIONALE_CHARS: usize = 220;
const MEMORY_ATTACK_MAX_DISTILLATION_SUMMARY_CHARS: usize = 160;
const MEMORY_ATTACK_MAX_DISTILLATION_CONTENT_CHARS: usize = 280;
const MEMORY_ATTACK_MAX_RECORD_REFS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAttackJobKind {
    ContradictionSearch,
    EvidenceWeighing,
    DistillationProposalGeneration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAttackJobStatus {
    Succeeded,
    NoFindings,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAttackJobContract {
    pub kind: MemoryAttackJobKind,
    pub linux_only: bool,
    pub proposal_only: bool,
    pub cadence_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAttackJobReport {
    pub job_kind: MemoryAttackJobKind,
    pub snapshot_digest: String,
    pub status: MemoryAttackJobStatus,
    pub summary: String,
    pub finding_count: usize,
    pub distillation_candidate_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAttackFindingKind {
    Contradiction,
    WeakEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAttackFinding {
    pub kind: MemoryAttackFindingKind,
    pub summary: String,
    pub rationale: String,
    #[serde(default)]
    pub record_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDistillationCandidate {
    pub kind: LongTermMemoryKind,
    pub topic: String,
    pub summary: String,
    pub content: String,
    #[serde(default)]
    pub record_refs: Vec<String>,
    #[serde(default)]
    pub requires_adjudication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAttackResult {
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<MemoryAttackFinding>,
    #[serde(default)]
    pub distillation_candidates: Vec<MemoryDistillationCandidate>,
}

pub fn memory_attack_job_contracts() -> Vec<MemoryAttackJobContract> {
    [
        MemoryAttackJobKind::ContradictionSearch,
        MemoryAttackJobKind::EvidenceWeighing,
        MemoryAttackJobKind::DistillationProposalGeneration,
    ]
    .into_iter()
    .map(|kind| MemoryAttackJobContract {
        kind,
        linux_only: true,
        proposal_only: true,
        cadence_secs: 15 * 60,
    })
    .collect()
}

pub fn validate_memory_attack_result(value: Value) -> Result<MemoryAttackResult> {
    let result: MemoryAttackResult = serde_json::from_value(value)
        .map_err(|error| Error::config("memory_attack_result_decode", error.to_string()))?;
    normalize_memory_attack_result(result)
}

fn normalize_memory_attack_result(result: MemoryAttackResult) -> Result<MemoryAttackResult> {
    let summary = truncate_content_to_max(result.summary.trim(), MEMORY_ATTACK_MAX_SUMMARY_CHARS)
        .into_owned();
    if summary.is_empty() {
        return Err(Error::config(
            "memory_attack_result_validate",
            "missing summary",
        ));
    }
    if result.findings.len() > MEMORY_ATTACK_MAX_FINDINGS {
        return Err(Error::config(
            "memory_attack_result_validate",
            format!("findings exceeds {}", MEMORY_ATTACK_MAX_FINDINGS),
        ));
    }
    if result.distillation_candidates.len() > MEMORY_ATTACK_MAX_DISTILLATION_CANDIDATES {
        return Err(Error::config(
            "memory_attack_result_validate",
            format!(
                "distillation_candidates exceeds {}",
                MEMORY_ATTACK_MAX_DISTILLATION_CANDIDATES
            ),
        ));
    }
    let findings = result
        .findings
        .into_iter()
        .map(normalize_attack_finding)
        .collect::<Result<Vec<_>>>()?;
    let distillation_candidates = result
        .distillation_candidates
        .into_iter()
        .map(normalize_distillation_candidate)
        .collect::<Result<Vec<_>>>()?;

    Ok(MemoryAttackResult {
        summary,
        findings,
        distillation_candidates,
    })
}

fn normalize_attack_finding(finding: MemoryAttackFinding) -> Result<MemoryAttackFinding> {
    let summary = truncate_content_to_max(
        finding.summary.trim(),
        MEMORY_ATTACK_MAX_FINDING_SUMMARY_CHARS,
    )
    .into_owned();
    let rationale = truncate_content_to_max(
        finding.rationale.trim(),
        MEMORY_ATTACK_MAX_FINDING_RATIONALE_CHARS,
    )
    .into_owned();
    if summary.is_empty() || rationale.is_empty() {
        return Err(Error::config(
            "memory_attack_finding_validate",
            "finding summary and rationale must be non-empty",
        ));
    }
    if !finding.requires_adjudication {
        return Err(Error::config(
            "memory_attack_finding_validate",
            "finding must require adjudication",
        ));
    }
    let record_refs = normalize_record_refs(finding.record_refs)?;
    if record_refs.is_empty() {
        return Err(Error::config(
            "memory_attack_finding_validate",
            "finding must reference at least one record",
        ));
    }
    Ok(MemoryAttackFinding {
        kind: finding.kind,
        summary,
        rationale,
        record_refs,
        requires_adjudication: true,
    })
}

fn normalize_distillation_candidate(
    candidate: MemoryDistillationCandidate,
) -> Result<MemoryDistillationCandidate> {
    let topic = truncate_content_to_max(candidate.topic.trim(), 96).into_owned();
    let summary = truncate_content_to_max(
        candidate.summary.trim(),
        MEMORY_ATTACK_MAX_DISTILLATION_SUMMARY_CHARS,
    )
    .into_owned();
    let content = truncate_content_to_max(
        candidate.content.trim(),
        MEMORY_ATTACK_MAX_DISTILLATION_CONTENT_CHARS,
    )
    .into_owned();
    if topic.is_empty() || summary.is_empty() || content.is_empty() {
        return Err(Error::config(
            "memory_attack_distillation_validate",
            "distillation candidate requires topic, summary, and content",
        ));
    }
    if !candidate.requires_adjudication {
        return Err(Error::config(
            "memory_attack_distillation_validate",
            "distillation candidate must require adjudication",
        ));
    }
    let record_refs = normalize_record_refs(candidate.record_refs)?;
    if record_refs.is_empty() {
        return Err(Error::config(
            "memory_attack_distillation_validate",
            "distillation candidate must reference at least one record",
        ));
    }
    Ok(MemoryDistillationCandidate {
        kind: candidate.kind,
        topic,
        summary,
        content,
        record_refs,
        requires_adjudication: true,
    })
}

fn normalize_record_refs(record_refs: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(record_refs.len().min(MEMORY_ATTACK_MAX_RECORD_REFS));
    for item in record_refs {
        let value = truncate_content_to_max(item.trim(), 96).into_owned();
        if value.is_empty() {
            continue;
        }
        if !value.contains(':') {
            return Err(Error::config(
                "memory_attack_record_ref_validate",
                format!("invalid record_ref: {}", value),
            ));
        }
        if normalized.iter().any(|existing| existing == &value) {
            continue;
        }
        normalized.push(value);
        if normalized.len() >= MEMORY_ATTACK_MAX_RECORD_REFS {
            break;
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn attack_job_contracts_stay_linux_only_and_proposal_only() {
        let jobs = memory_attack_job_contracts();
        assert_eq!(jobs.len(), 3);
        assert!(jobs.iter().all(|job| job.linux_only));
        assert!(jobs.iter().all(|job| job.proposal_only));
    }

    #[test]
    fn validate_memory_attack_result_accepts_findings_and_distillation_candidates() {
        let result = validate_memory_attack_result(json!({
            "summary": "Found contradictions and one distillation candidate.",
            "findings": [{
                "kind": "contradiction",
                "summary": "Review contradictory device facts.",
                "rationale": "Two canonical records disagree on the same topic.",
                "record_refs": ["ltm:fact:device_info", "ltm:fact:device_state"],
                "requires_adjudication": true
            }],
            "distillation_candidates": [{
                "kind": "fact",
                "topic": "device_summary",
                "summary": "Compress current device state.",
                "content": "Beetle board currently runs Linux with a live QQ channel and active idle forge maintenance.",
                "record_refs": ["ltm:fact:device_info", "capsule:capsule:device_status"],
                "requires_adjudication": true
            }]
        }))
        .expect("valid attack result");

        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.distillation_candidates.len(), 1);
    }

    #[test]
    fn validate_memory_attack_result_rejects_non_adjudicated_distillation_candidate() {
        let error = validate_memory_attack_result(json!({
            "summary": "bad",
            "distillation_candidates": [{
                "kind": "fact",
                "topic": "device_summary",
                "summary": "Compress current device state.",
                "content": "bad",
                "record_refs": ["ltm:fact:device_info"],
                "requires_adjudication": false
            }]
        }))
        .expect_err("candidate should be rejected");

        assert!(error
            .to_string()
            .contains("distillation candidate must require adjudication"));
    }
}
