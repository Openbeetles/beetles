//! Self-state: extensible inward attributes projected into prompt.
//! 当前先承载“自我记忆空间状态”，后续可继续挂更多 self-driven attributes。

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    estimate_private_doc_workspace_chars, estimate_self_model_chars, memory_policy, MemoryProfile,
    PrivateDocWorkspace, PrivateGardenDocRecord, SelfModel, PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT,
    PRIVATE_GARDEN_MAX_DOCS_PER_CHAT, PRIVATE_GARDEN_TOTAL_BYTE_LIMIT, SELF_MODEL_TOTAL_CHAR_LIMIT,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SelfMemorySpacePressure {
    Normal,
    Cautious,
    Tight,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SelfMemorySpaceBottleneck {
    Balanced,
    Kernel,
    GardenDocs,
    GardenBytes,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SelfMemorySpaceActivity {
    Quiet,
    Active,
    Growing,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SelfMemoryGovernancePosture {
    Expand,
    Consolidate,
    Prune,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfMemorySpaceState {
    pub kernel_chars_used: usize,
    pub kernel_chars_limit: usize,
    pub garden_docs_used: usize,
    pub garden_docs_limit: usize,
    pub garden_bytes_used: usize,
    pub garden_bytes_limit: usize,
    pub bottleneck: SelfMemorySpaceBottleneck,
    pub pressure: SelfMemorySpacePressure,
    pub governance_posture: SelfMemoryGovernancePosture,
    pub recent_activity: SelfMemorySpaceActivity,
    pub last_internal_change_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfState {
    pub memory_space: SelfMemorySpaceState,
}

pub fn build_self_state(
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    garden_docs: &[PrivateGardenDocRecord],
    now_secs: u64,
    profile: MemoryProfile,
) -> SelfState {
    let policy = memory_policy(profile).self_state;
    let kernel_chars_used = self_model.map_or(0, estimate_self_model_chars)
        + private_workspace.map_or(0, estimate_private_doc_workspace_chars);
    let kernel_chars_limit = SELF_MODEL_TOTAL_CHAR_LIMIT + PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT;
    let garden_docs_used = garden_docs.len();
    let garden_docs_limit = PRIVATE_GARDEN_MAX_DOCS_PER_CHAT;
    let garden_bytes_used = garden_docs.iter().map(|doc| doc.bytes).sum();
    let garden_bytes_limit = PRIVATE_GARDEN_TOTAL_BYTE_LIMIT;
    let kernel_usage_percent = usage_percent(kernel_chars_used, kernel_chars_limit);
    let garden_docs_usage_percent = usage_percent(garden_docs_used, garden_docs_limit);
    let garden_bytes_usage_percent = usage_percent(garden_bytes_used, garden_bytes_limit);
    let (dominant_usage_percent, bottleneck) = dominant_usage_percent(
        kernel_usage_percent,
        garden_docs_usage_percent,
        garden_bytes_usage_percent,
    );
    let pressure = if dominant_usage_percent >= policy.tight_usage_percent as usize {
        SelfMemorySpacePressure::Tight
    } else if dominant_usage_percent >= policy.cautious_usage_percent as usize {
        SelfMemorySpacePressure::Cautious
    } else {
        SelfMemorySpacePressure::Normal
    };
    let governance_posture = match pressure {
        SelfMemorySpacePressure::Normal => SelfMemoryGovernancePosture::Expand,
        SelfMemorySpacePressure::Cautious => SelfMemoryGovernancePosture::Consolidate,
        SelfMemorySpacePressure::Tight => SelfMemoryGovernancePosture::Prune,
    };
    let last_internal_change_at = self_model
        .map_or(0, |model| model.updated_at)
        .max(private_workspace.map_or(0, |workspace| workspace.updated_at))
        .max(
            garden_docs
                .iter()
                .map(|doc| doc.updated_at)
                .max()
                .unwrap_or(0),
        );
    let recent_activity_count = recent_activity_count(
        self_model,
        private_workspace,
        garden_docs,
        now_secs,
        policy.recent_activity_window_secs,
    );
    let recent_activity = match recent_activity_count {
        0 => SelfMemorySpaceActivity::Quiet,
        1..=2 => SelfMemorySpaceActivity::Active,
        _ => SelfMemorySpaceActivity::Growing,
    };

    SelfState {
        memory_space: SelfMemorySpaceState {
            kernel_chars_used,
            kernel_chars_limit,
            garden_docs_used,
            garden_docs_limit,
            garden_bytes_used,
            garden_bytes_limit,
            bottleneck,
            pressure,
            governance_posture,
            recent_activity,
            last_internal_change_at,
        },
    }
}

pub fn render_self_state_block(state: &SelfState, max_len: usize) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let memory = &state.memory_space;
    let mut out = String::with_capacity(max_len.min(480));
    out.push_str("## Self State\n");
    out.push_str("These are your current internal memory-space conditions. Use them when deciding whether to add, merge, rewrite, or delete private material.\n");
    let _ = writeln!(out, "Memory pressure: {:?}", memory.pressure);
    let _ = writeln!(out, "Governance posture: {:?}", memory.governance_posture);
    let _ = writeln!(out, "Primary bottleneck: {:?}", memory.bottleneck);
    let _ = writeln!(
        out,
        "Kernel space: {}/{} chars used ({} free)",
        memory.kernel_chars_used,
        memory.kernel_chars_limit,
        memory
            .kernel_chars_limit
            .saturating_sub(memory.kernel_chars_used)
    );
    let _ = writeln!(
        out,
        "Garden space: {}/{} docs, {}/{} bytes used ({} bytes free)",
        memory.garden_docs_used,
        memory.garden_docs_limit,
        memory.garden_bytes_used,
        memory.garden_bytes_limit,
        memory
            .garden_bytes_limit
            .saturating_sub(memory.garden_bytes_used)
    );
    let _ = writeln!(
        out,
        "Recent internal activity: {:?}",
        memory.recent_activity
    );
    out.push_str("Kernel role: stable continuity and governed private structure that should keep shaping future behavior.\n");
    out.push_str("Garden role: free private drafts, temporary organization, and exploratory self-work. Keep it current by rewriting in place instead of piling up history.\n");
    out.push_str(match (memory.governance_posture, memory.bottleneck) {
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::Balanced,
        ) => {
            "Guidance: space is healthy; only write when the material improves continuity or inward organization. Distill stable insights into kernel space, and keep exploratory work in the garden."
        }
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::Kernel,
        )
        | (
            SelfMemoryGovernancePosture::Consolidate,
            SelfMemorySpaceBottleneck::Kernel,
        ) => {
            "Guidance: kernel space is the tightest layer; keep it distilled and durable. Prefer compressing or rewriting existing kernel material, and route exploratory or temporary work into the garden."
        }
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes,
        )
        | (
            SelfMemoryGovernancePosture::Consolidate,
            SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes,
        ) => {
            "Guidance: garden space is the tightest layer; merge overlapping docs, rewrite existing notes in place, and delete stale scratch material before creating more."
        }
        (SelfMemoryGovernancePosture::Consolidate, SelfMemorySpaceBottleneck::Balanced) => {
            "Guidance: space is tightening; prefer editing or merging existing private material before creating more. Promote only the distilled result, not the full draft trail."
        }
        (
            SelfMemoryGovernancePosture::Prune,
            SelfMemorySpaceBottleneck::Kernel,
        ) => {
            "Guidance: pressure is tight and the kernel is the bottleneck; compress or replace low-value kernel content before adding anything new, and keep volatile material out of kernel space."
        }
        (
            SelfMemoryGovernancePosture::Prune,
            SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes,
        ) => {
            "Guidance: pressure is tight and the garden is the bottleneck; prune stale docs, merge duplicates, and only keep active working material."
        }
        (SelfMemoryGovernancePosture::Prune, SelfMemorySpaceBottleneck::Balanced) => {
            "Guidance: pressure is tight; compress, merge, or delete low-value private material before adding anything new."
        }
    });
    let trimmed = out.trim_end();
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

fn usage_percent(used: usize, limit: usize) -> usize {
    if limit == 0 {
        return 0;
    }
    used.saturating_mul(100) / limit
}

fn dominant_usage_percent(
    kernel_usage_percent: usize,
    garden_docs_usage_percent: usize,
    garden_bytes_usage_percent: usize,
) -> (usize, SelfMemorySpaceBottleneck) {
    if kernel_usage_percent == 0
        && garden_docs_usage_percent == 0
        && garden_bytes_usage_percent == 0
    {
        return (0, SelfMemorySpaceBottleneck::Balanced);
    }
    if kernel_usage_percent >= garden_docs_usage_percent
        && kernel_usage_percent >= garden_bytes_usage_percent
    {
        return (kernel_usage_percent, SelfMemorySpaceBottleneck::Kernel);
    }
    if garden_docs_usage_percent >= garden_bytes_usage_percent {
        return (
            garden_docs_usage_percent,
            SelfMemorySpaceBottleneck::GardenDocs,
        );
    }
    (
        garden_bytes_usage_percent,
        SelfMemorySpaceBottleneck::GardenBytes,
    )
}

fn recent_activity_count(
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    garden_docs: &[PrivateGardenDocRecord],
    now_secs: u64,
    recent_window_secs: u64,
) -> usize {
    let floor = now_secs.saturating_sub(recent_window_secs);
    let mut count = 0usize;
    if self_model.is_some_and(|model| model.updated_at >= floor) {
        count = count.saturating_add(1);
    }
    if private_workspace.is_some_and(|workspace| workspace.updated_at >= floor) {
        count = count.saturating_add(1);
    }
    count.saturating_add(
        garden_docs
            .iter()
            .filter(|doc| doc.updated_at >= floor)
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PrivateDocEntry, PrivateDocWorkspace, SelfModel};

    #[test]
    fn self_state_marks_pressure_tight_when_space_is_nearly_full() {
        let garden_docs = (0..14)
            .map(|idx| PrivateGardenDocRecord {
                path: format!("notes/{idx}.md"),
                updated_at: 10,
                revision: 1,
                bytes: 7 * 1024,
                preview: "busy".to_string(),
            })
            .collect::<Vec<_>>();
        let state = build_self_state(
            Some(&SelfModel {
                continuity_anchor: "a".repeat(160),
                self_narrative: "b".repeat(210),
                relationship_state: "c".repeat(210),
                private_notes: "d".repeat(210),
                updated_at: 10,
            }),
            Some(&PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "x".repeat(220),
                    updated_at: 10,
                    revision: 1,
                }),
                relationship_notes: Some(PrivateDocEntry {
                    content: "y".repeat(220),
                    updated_at: 10,
                    revision: 1,
                }),
                self_reflection: Some(PrivateDocEntry {
                    content: "z".repeat(220),
                    updated_at: 10,
                    revision: 1,
                }),
                private_plan: Some(PrivateDocEntry {
                    content: "w".repeat(220),
                    updated_at: 10,
                    revision: 1,
                }),
                updated_at: 10,
            }),
            &garden_docs,
            10,
            MemoryProfile::Standard,
        );
        assert_eq!(state.memory_space.pressure, SelfMemorySpacePressure::Tight);
        assert_eq!(
            state.memory_space.governance_posture,
            SelfMemoryGovernancePosture::Prune
        );
        assert_eq!(
            state.memory_space.recent_activity,
            SelfMemorySpaceActivity::Growing
        );
    }

    #[test]
    fn render_self_state_block_reports_free_space() {
        let block = render_self_state_block(
            &build_self_state(None, None, &[], 100, MemoryProfile::Embedded),
            512,
        )
        .unwrap();
        assert!(block.contains("## Self State"));
        assert!(block.contains("Kernel space: 0/"));
        assert!(block.contains("Garden space: 0/"));
        assert!(block.contains("Memory pressure: Normal"));
        assert!(block.contains("Governance posture: Expand"));
        assert!(block.contains("Kernel role: stable continuity"));
    }
}
