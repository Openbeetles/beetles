//! Self-state: extensible inward attributes projected into prompt.
//! 当前承载“自我空间 + 内在层 + 自治状态”。
#![allow(clippy::too_many_arguments)]

use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use super::{
    estimate_autonomy_strategy_chars, estimate_inner_life_chars,
    estimate_private_doc_workspace_chars, estimate_self_continuity_chars,
    estimate_self_model_chars, memory_policy, private_garden_scope_id, AutonomyGovernanceTendency,
    AutonomyStrategy, InnerLife, MemoryProfile, PrivateDocWorkspace, PrivateGardenDocRecord,
    SelfContinuity, SelfModel, AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT, INNER_LIFE_TOTAL_CHAR_LIMIT,
    PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT, PRIVATE_GARDEN_MAX_DOCS_PER_CHAT,
    PRIVATE_GARDEN_TOTAL_BYTE_LIMIT, SELF_CONTINUITY_TOTAL_CHAR_LIMIT, SELF_MODEL_TOTAL_CHAR_LIMIT,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SelfAutonomyStatus {
    Dormant,
    Watching,
    Active,
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
pub struct SelfInnerState {
    pub inner_life_chars_used: usize,
    pub inner_life_chars_limit: usize,
    pub self_continuity_chars_used: usize,
    pub self_continuity_chars_limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfAutonomyState {
    pub last_user_turn_at: u64,
    pub last_autonomy_run_at: u64,
    pub status: SelfAutonomyStatus,
    pub health_score: u8,
    pub strategy_chars_used: usize,
    pub strategy_chars_limit: usize,
    pub strategy_mode: String,
    pub strategy_focus: String,
    pub self_model_tendency: AutonomyGovernanceTendency,
    pub private_docs_tendency: AutonomyGovernanceTendency,
    pub private_garden_tendency: AutonomyGovernanceTendency,
    pub idle_enabled: bool,
    pub idle_interval_secs: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfState {
    pub memory_space: SelfMemorySpaceState,
    pub inner_state: SelfInnerState,
    pub autonomy: SelfAutonomyState,
}

pub fn build_self_state(
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    autonomy_strategy: Option<&AutonomyStrategy>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
    garden_docs: &[PrivateGardenDocRecord],
    now_secs: u64,
    profile: MemoryProfile,
) -> SelfState {
    let policy = memory_policy(profile).self_state;
    let strategy_chars_used = autonomy_strategy.map_or(0, estimate_autonomy_strategy_chars);
    let inner_life_chars_used = inner_life.map_or(0, estimate_inner_life_chars);
    let self_continuity_chars_used = self_continuity.map_or(0, estimate_self_continuity_chars);
    let kernel_chars_used = self_model.map_or(0, estimate_self_model_chars)
        + private_workspace.map_or(0, estimate_private_doc_workspace_chars)
        + strategy_chars_used
        + inner_life_chars_used
        + self_continuity_chars_used;
    let kernel_chars_limit = SELF_MODEL_TOTAL_CHAR_LIMIT
        + PRIVATE_DOC_WORKSPACE_TOTAL_CHAR_LIMIT
        + AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT
        + INNER_LIFE_TOTAL_CHAR_LIMIT
        + SELF_CONTINUITY_TOTAL_CHAR_LIMIT;
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
        .max(autonomy_strategy.map_or(0, |strategy| strategy.updated_at))
        .max(inner_life.map_or(0, |inner_life| inner_life.updated_at))
        .max(self_continuity.map_or(0, |continuity| continuity.updated_at))
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
        autonomy_strategy,
        inner_life,
        self_continuity,
        garden_docs,
        now_secs,
        policy.recent_activity_window_secs,
    );
    let recent_activity = match recent_activity_count {
        0 => SelfMemorySpaceActivity::Quiet,
        1..=2 => SelfMemorySpaceActivity::Active,
        _ => SelfMemorySpaceActivity::Growing,
    };
    let last_user_turn_at = self_continuity.map_or(0, |continuity| continuity.last_user_turn_at);
    let last_autonomy_run_at =
        self_continuity.map_or(0, |continuity| continuity.last_autonomy_run_at);
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
        inner_state: SelfInnerState {
            inner_life_chars_used,
            inner_life_chars_limit: INNER_LIFE_TOTAL_CHAR_LIMIT,
            self_continuity_chars_used,
            self_continuity_chars_limit: SELF_CONTINUITY_TOTAL_CHAR_LIMIT,
        },
        autonomy: SelfAutonomyState {
            last_user_turn_at,
            last_autonomy_run_at,
            status: autonomy_status(
                self_continuity,
                now_secs,
                policy.recent_activity_window_secs,
            ),
            health_score: autonomy_health_score(
                dominant_usage_percent as u8,
                autonomy_strategy.is_some(),
                self_continuity.is_some(),
                inner_life.is_some(),
            ),
            strategy_chars_used,
            strategy_chars_limit: AUTONOMY_STRATEGY_TOTAL_CHAR_LIMIT,
            strategy_mode: autonomy_strategy
                .map(|strategy| strategy.current_mode.trim().to_string())
                .unwrap_or_default(),
            strategy_focus: autonomy_strategy
                .map(|strategy| strategy.next_focus.trim().to_string())
                .unwrap_or_default(),
            self_model_tendency: autonomy_strategy
                .map_or(AutonomyGovernanceTendency::Retain, |strategy| {
                    strategy.self_model_tendency
                }),
            private_docs_tendency: autonomy_strategy
                .map_or(AutonomyGovernanceTendency::Retain, |strategy| {
                    strategy.private_docs_tendency
                }),
            private_garden_tendency: autonomy_strategy
                .map_or(AutonomyGovernanceTendency::Retain, |strategy| {
                    strategy.private_garden_tendency
                }),
            idle_enabled: autonomy_strategy.is_none_or(|strategy| strategy.idle_enabled),
            idle_interval_secs: autonomy_strategy.map_or(0, |strategy| strategy.idle_interval_secs),
        },
    }
}

pub fn render_self_state_block(state: &SelfState, max_len: usize) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let memory = &state.memory_space;
    let inner = &state.inner_state;
    let autonomy = &state.autonomy;
    let mut out = String::with_capacity(max_len.min(640));
    out.push_str("## Self State\n");
    out.push_str("These are your current internal memory-space and autonomy conditions. Use them when deciding whether to add, merge, rewrite, or delete private material.\n");
    let _ = writeln!(out, "Private garden owner: {}", private_garden_scope_id());
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
        "Inner life: {}/{} chars used",
        inner.inner_life_chars_used, inner.inner_life_chars_limit
    );
    let _ = writeln!(
        out,
        "Self continuity: {}/{} chars used",
        inner.self_continuity_chars_used, inner.self_continuity_chars_limit
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
    let _ = writeln!(
        out,
        "Autonomy: {:?} (health score {})",
        autonomy.status, autonomy.health_score
    );
    let _ = writeln!(
        out,
        "Autonomy strategy: {}/{} chars used",
        autonomy.strategy_chars_used, autonomy.strategy_chars_limit
    );
    let _ = writeln!(
        out,
        "Autonomy anchors: last_user_turn_at={} last_autonomy_run_at={}",
        autonomy.last_user_turn_at, autonomy.last_autonomy_run_at
    );
    if !autonomy.strategy_mode.is_empty() {
        let _ = writeln!(out, "Autonomy mode: {}", autonomy.strategy_mode);
    }
    if !autonomy.strategy_focus.is_empty() {
        let _ = writeln!(out, "Autonomy next focus: {}", autonomy.strategy_focus);
    }
    let _ = writeln!(
        out,
        "Autonomy tendencies: self_model={} private_docs={} private_garden={}",
        autonomy.self_model_tendency.as_str(),
        autonomy.private_docs_tendency.as_str(),
        autonomy.private_garden_tendency.as_str()
    );
    let _ = writeln!(
        out,
        "Autonomy idle policy: enabled={} interval_secs={}",
        autonomy.idle_enabled, autonomy.idle_interval_secs
    );
    out.push_str("Kernel role: stable continuity and governed private structure that should keep shaping future behavior.\n");
    out.push_str("Inner-life role: active subjective movement, mood, and live inward drift.\n");
    out.push_str("Self-continuity role: preserve the sense of still being the same self across time and autonomy cycles.\n");
    out.push_str("Garden role: free private drafts, temporary organization, and exploratory self-work. Keep it current by rewriting in place instead of piling up history.\n");
    out.push_str(match (memory.governance_posture, memory.bottleneck) {
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::Balanced,
        ) => {
            "Guidance: space is healthy; only write when the material improves continuity or inward organization. Distill stable insights into kernel space, keep live inward movement in inner-life, and keep exploratory work in the garden."
        }
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::Kernel,
        )
        | (
            SelfMemoryGovernancePosture::Consolidate,
            SelfMemorySpaceBottleneck::Kernel,
        ) => {
            "Guidance: kernel-side space is the tightest layer; compress continuity-bearing material, keep inner-life lively but concise, and route temporary work into the garden."
        }
        (
            SelfMemoryGovernancePosture::Expand,
            SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes,
        )
        | (
            SelfMemoryGovernancePosture::Consolidate,
            SelfMemorySpaceBottleneck::GardenDocs | SelfMemorySpaceBottleneck::GardenBytes,
        ) => {
            "Guidance: garden space is the tightest layer; merge overlapping docs, rewrite notes in place, and delete stale scratch material before creating more."
        }
        (SelfMemoryGovernancePosture::Consolidate, SelfMemorySpaceBottleneck::Balanced) => {
            "Guidance: space is tightening; prefer editing or merging existing private material before creating more."
        }
        (
            SelfMemoryGovernancePosture::Prune,
            SelfMemorySpaceBottleneck::Kernel,
        ) => {
            "Guidance: pressure is tight and kernel-side space is the bottleneck; compress or replace low-value kernel, inner-life, or continuity content before adding anything new."
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
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
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
    autonomy_strategy: Option<&AutonomyStrategy>,
    inner_life: Option<&InnerLife>,
    self_continuity: Option<&SelfContinuity>,
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
    if autonomy_strategy.is_some_and(|strategy| strategy.updated_at >= floor) {
        count = count.saturating_add(1);
    }
    if inner_life.is_some_and(|inner_life| inner_life.updated_at >= floor) {
        count = count.saturating_add(1);
    }
    if self_continuity.is_some_and(|continuity| continuity.updated_at >= floor) {
        count = count.saturating_add(1);
    }
    count.saturating_add(
        garden_docs
            .iter()
            .filter(|doc| doc.updated_at >= floor)
            .count(),
    )
}

fn autonomy_status(
    self_continuity: Option<&SelfContinuity>,
    now_secs: u64,
    recent_window_secs: u64,
) -> SelfAutonomyStatus {
    let Some(self_continuity) = self_continuity else {
        return SelfAutonomyStatus::Dormant;
    };
    let floor = now_secs.saturating_sub(recent_window_secs);
    if self_continuity.last_autonomy_run_at >= floor {
        SelfAutonomyStatus::Active
    } else if self_continuity.last_user_turn_at > 0 {
        SelfAutonomyStatus::Watching
    } else {
        SelfAutonomyStatus::Dormant
    }
}

fn autonomy_health_score(
    usage_percent: u8,
    has_strategy: bool,
    has_continuity: bool,
    has_inner_life: bool,
) -> u8 {
    let memory_health = 100u8.saturating_sub(usage_percent.min(100));
    let strategy_bonus = if has_strategy { 10 } else { 0 };
    let continuity_bonus = if has_continuity { 12 } else { 0 };
    let inner_bonus = if has_inner_life { 8 } else { 0 };
    memory_health
        .saturating_add(strategy_bonus)
        .saturating_add(continuity_bonus)
        .saturating_add(inner_bonus)
        .min(100)
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
                ..SelfModel::default()
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
            Some(&AutonomyStrategy {
                current_mode: "protect continuity while pruning low-value drift".to_string(),
                active_priorities: "condense repeated private fragments".to_string(),
                write_policy: "rewrite before append".to_string(),
                next_focus: "keep only one active scratch thread".to_string(),
                cadence_reason: "space is tight".to_string(),
                self_model_tendency: AutonomyGovernanceTendency::Compress,
                private_docs_tendency: AutonomyGovernanceTendency::Rewrite,
                private_garden_tendency: AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 120,
                updated_at: 10,
            }),
            Some(&InnerLife {
                internal_monologue: "i".repeat(220),
                private_journal: "j".repeat(220),
                emotional_drift: "k".repeat(220),
                attention_drift: "l".repeat(220),
                updated_at: 10,
            }),
            Some(&SelfContinuity {
                wake_anchor: "m".repeat(220),
                current_self_state: "n".repeat(220),
                recent_changes: "o".repeat(220),
                continuity_bridge: "p".repeat(220),
                priority_posture: "q".repeat(220),
                relationship_posture: "r".repeat(220),
                task_posture: "s".repeat(220),
                last_user_turn_at: 10,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 10,
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
        assert_eq!(state.autonomy.status, SelfAutonomyStatus::Active);
    }
}
