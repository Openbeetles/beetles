//! Deterministic foreground work continuity projection.
//! 前台工作连续性投影：从现有正式资产汇总“做到哪、为什么停、下一步是什么”。

use crate::agent::ActiveWorkRecord;
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub const MAX_WORK_CONTINUITY_BLOCK_LEN: usize = 560;

const MAX_WORK_CONTINUITY_FOCUS_CHARS: usize = 120;
const MAX_WORK_CONTINUITY_FIELD_CHARS: usize = 200;
const MAX_WORK_CONTINUITY_ARTIFACT_REF_CHARS: usize = 96;
const MAX_WORK_CONTINUITY_ARTIFACT_REFS: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkContinuityRecord {
    #[serde(default)]
    pub focus: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub progress_summary: String,
    #[serde(default)]
    pub blocker: String,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub recent_outcome: String,
    #[serde(default)]
    pub active_artifact_refs: Vec<String>,
    #[serde(default)]
    pub updated_at: u64,
}

impl WorkContinuityRecord {
    pub fn is_meaningful(&self) -> bool {
        !self.focus.trim().is_empty()
            && (!self.progress_summary.trim().is_empty()
                || !self.blocker.trim().is_empty()
                || !self.next_action.trim().is_empty()
                || !self.recent_outcome.trim().is_empty()
                || !self.active_artifact_refs.is_empty())
    }
}

pub fn build_work_continuity_record(
    active_work: Option<&ActiveWorkRecord>,
    summary_text: Option<&str>,
) -> Option<WorkContinuityRecord> {
    if active_work.is_some_and(|record| !record.continuity_open) {
        return None;
    }
    let mut record = active_work
        .map(work_continuity_from_active_work)
        .unwrap_or_default();

    if record.progress_summary.trim().is_empty() {
        record.progress_summary = normalize_field(summary_text, MAX_WORK_CONTINUITY_FIELD_CHARS);
    }

    record.is_meaningful().then_some(record)
}

pub fn render_work_continuity_block(
    record: &WorkContinuityRecord,
    max_len: usize,
) -> Option<String> {
    if !record.is_meaningful() {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(MAX_WORK_CONTINUITY_BLOCK_LEN));
    out.push_str("## Work Continuity\n");
    out.push_str("Focus: ");
    out.push_str(record.focus.trim());
    out.push('\n');
    if !record.status.trim().is_empty() {
        out.push_str("Status: ");
        out.push_str(record.status.trim());
        out.push('\n');
    }
    if !record.progress_summary.trim().is_empty() {
        out.push_str("Progress: ");
        out.push_str(record.progress_summary.trim());
        out.push('\n');
    }
    if !record.blocker.trim().is_empty() {
        out.push_str("Blocker: ");
        out.push_str(record.blocker.trim());
        out.push('\n');
    }
    if !record.next_action.trim().is_empty() {
        out.push_str("Next: ");
        out.push_str(record.next_action.trim());
        out.push('\n');
    }
    if !record.recent_outcome.trim().is_empty() {
        out.push_str("Recent outcome: ");
        out.push_str(record.recent_outcome.trim());
        out.push('\n');
    }
    if !record.active_artifact_refs.is_empty() {
        out.push_str("Artifacts: ");
        out.push_str(&record.active_artifact_refs.join(" | "));
        out.push('\n');
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

fn work_continuity_from_active_work(record: &ActiveWorkRecord) -> WorkContinuityRecord {
    WorkContinuityRecord {
        focus: normalize_field(Some(record.title.as_str()), MAX_WORK_CONTINUITY_FOCUS_CHARS),
        status: record.status.label().to_string(),
        progress_summary: normalize_field(
            Some(record.progress_summary.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        blocker: normalize_field(
            Some(record.blocker.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        next_action: normalize_field(
            Some(record.next_action.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        recent_outcome: normalize_field(
            Some(record.recent_outcome.as_str()),
            MAX_WORK_CONTINUITY_FIELD_CHARS,
        ),
        active_artifact_refs: normalize_artifact_refs(&record.active_artifact_refs),
        updated_at: record.updated_at,
    }
}

fn normalize_artifact_refs(items: &[String]) -> Vec<String> {
    items
        .iter()
        .map(|item| normalize_field(Some(item.as_str()), MAX_WORK_CONTINUITY_ARTIFACT_REF_CHARS))
        .filter(|item| !item.is_empty())
        .take(MAX_WORK_CONTINUITY_ARTIFACT_REFS)
        .collect()
}

fn normalize_field(value: Option<&str>, max_len: usize) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_content_to_max(value, max_len).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ActiveWorkKind, ForegroundWorkStatus};

    fn sample_active_work_record() -> ActiveWorkRecord {
        ActiveWorkRecord {
            kind: ActiveWorkKind::TaskExecution,
            title: "配置 QQ 邮箱".to_string(),
            status: ForegroundWorkStatus::Running,
            continuity_open: true,
            blocks_background_llm: true,
            progress_summary: "已创建账户草案".to_string(),
            blocker: String::new(),
            next_action: "继续写入缺失的邮箱参数".to_string(),
            recent_outcome: "已创建账户草案".to_string(),
            active_artifact_refs: vec!["office.account.qq".to_string()],
            updated_at: 8,
        }
    }

    #[test]
    fn work_continuity_uses_foreground_work_as_single_truth_source() {
        let record = build_work_continuity_record(
            Some(&sample_active_work_record()),
            Some("会话摘要不应覆盖已有工作态"),
        )
        .expect("work continuity");

        assert_eq!(record.focus, "配置 QQ 邮箱");
        assert_eq!(record.status, "active");
        assert_eq!(record.progress_summary, "已创建账户草案");
        assert!(record.blocker.is_empty());
        assert_eq!(record.next_action, "继续写入缺失的邮箱参数");
        assert_eq!(record.recent_outcome, "已创建账户草案");
        assert_eq!(record.active_artifact_refs, vec!["office.account.qq"]);
        assert_eq!(record.updated_at, 8);
    }

    #[test]
    fn summary_only_does_not_create_fake_work_continuity() {
        let record =
            build_work_continuity_record(None, Some("这里只是会话摘要，不能被冒充成当前工作态"));

        assert!(record.is_none());
    }

    #[test]
    fn inactive_foreground_work_does_not_create_work_continuity() {
        let mut record = sample_active_work_record();
        record.continuity_open = false;
        record.status = ForegroundWorkStatus::Completed;
        let record = build_work_continuity_record(Some(&record), None);

        assert!(record.is_none());
    }
}
