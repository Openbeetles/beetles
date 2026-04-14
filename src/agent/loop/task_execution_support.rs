use super::*;

pub(super) fn should_consider_task_execution(
    msg: &crate::bus::PcMsg,
    has_tools: bool,
    pressure: crate::orchestrator::PressureLevel,
    has_active_run: bool,
) -> bool {
    if msg.ingress != IngressKind::User || msg.is_group {
        return false;
    }
    if matches!(pressure, crate::orchestrator::PressureLevel::Critical) {
        return false;
    }
    if has_active_run {
        return true;
    }
    let content = msg.content.trim();
    if content.is_empty() {
        return false;
    }
    let char_count = content.chars().count();
    let line_count = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let separator_count = content
        .chars()
        .filter(|ch| matches!(ch, '\n' | ',' | '，' | '.' | '。' | ';' | '；'))
        .count();
    if has_tools {
        char_count >= TASK_EXECUTION_MIN_CHARS
            || line_count >= TASK_EXECUTION_MIN_LINES
            || separator_count >= TASK_EXECUTION_MIN_SEPARATORS
    } else {
        char_count >= TASK_EXECUTION_MIN_CHARS.saturating_mul(2)
            || line_count >= TASK_EXECUTION_MIN_LINES
    }
}

pub(super) fn parse_task_execution_json<T>(raw: &str, stage: &'static str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(crate::error::Error::config(stage, "empty llm response"));
    }
    serde_json::from_str(trimmed)
        .or_else(|_| {
            let body = trimmed
                .split_once("```")
                .and_then(|(_, rest)| rest.split_once('\n'))
                .and_then(|(_, rest)| rest.split_once("```"))
                .map(|(json, _)| json.trim())
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("no fence body")))?;
            serde_json::from_str(body)
        })
        .or_else(|_| {
            let start = trimmed.find('{').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("no json object start"))
            })?;
            let end = trimmed.rfind('}').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("no json object end"))
            })?;
            serde_json::from_str(&trimmed[start..=end])
        })
        .map_err(|error| crate::error::Error::config(stage, error.to_string()))
}

pub(super) fn persist_task_run_record(
    store: &dyn TaskRunStore,
    record: &TaskRunRecord,
    stage: &str,
) {
    if let Err(error) = store.upsert(record) {
        log::warn!(
            "[task_execution] failed to persist run stage={} run_id={}: {}",
            stage,
            record.run.run_id,
            error
        );
    }
}

pub(super) fn persist_task_artifact_record(
    store: &dyn TaskArtifactStore,
    record: &TaskArtifactRecord,
    stage: &str,
) {
    if let Err(error) = store.put(record) {
        log::warn!(
            "[task_execution] failed to persist artifact stage={} run_id={} artifact_id={}: {}",
            stage,
            record.artifact.run_id,
            record.artifact.artifact_id,
            error
        );
    }
}

pub(super) fn append_task_execution_ledger_entry(
    store: &dyn TaskExecutionLedgerStore,
    entry: &TaskExecutionLedgerEntry,
    stage: &str,
) {
    if let Err(error) = store.append(&entry.run_id, entry) {
        log::warn!(
            "[task_execution] failed to append ledger stage={} run_id={} seq={}: {}",
            stage,
            entry.run_id,
            entry.sequence,
            error
        );
    }
}
