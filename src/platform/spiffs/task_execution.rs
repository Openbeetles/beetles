//! SPIFFS / state-root backed stores for the P3-A task workspace plane.

use crate::error::{Error, Result};
use crate::task_execution::{
    TaskArtifactRecord, TaskArtifactStore, TaskExecutionLedgerEntry, TaskExecutionLedgerStore,
    TaskLearningKind, TaskLearningRecord, TaskLearningRoute, TaskLearningStore, TaskRunRecord,
    TaskRunStatus, TaskRunStore,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::{list_dir, read_file, remove_file, state_path_join, write_file};

const RUN_INDEX_STAGE_LOCK: &str = "task_run_index_lock";
const RUN_INDEX_STAGE_CACHE: &str = "task_run_index_cache";
const RUN_INDEX_STAGE_PERSIST: &str = "task_run_index_persist";
const LEARNING_INDEX_STAGE_LOCK: &str = "task_learning_index_lock";
const LEARNING_INDEX_STAGE_CACHE: &str = "task_learning_index_cache";
const LEARNING_INDEX_STAGE_PERSIST: &str = "task_learning_index_persist";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_TASK_RUN_INDEX: &str = "x/ri.json";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_TASK_RUN_INDEX: &str = "memory/task_runs/index.json";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_DIR_TASK_RUN_FILES: &str = "x/r";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_DIR_TASK_RUN_FILES: &str = "memory/task_runs";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_DIR_TASK_ARTIFACT_FILES: &str = "x/a";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_DIR_TASK_ARTIFACT_FILES: &str = "memory/task_artifacts";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_DIR_TASK_LEDGER_FILES: &str = "x/l";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_DIR_TASK_LEDGER_FILES: &str = "memory/task_execution_ledger";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_TASK_LEARNING_INDEX: &str = "x/tli.json";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_TASK_LEARNING_INDEX: &str = "memory/task_learning/index.json";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const REL_DIR_TASK_LEARNING_FILES: &str = "x/t";
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
const REL_DIR_TASK_LEARNING_FILES: &str = "memory/task_learning";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct RunIndexEntry {
    run_id: String,
    source_channel: String,
    source_chat_id: String,
    title: String,
    status: TaskRunStatus,
    updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct LearningIndexEntry {
    learning_id: String,
    source_channel: String,
    source_chat_id: String,
    run_id: String,
    topic: String,
    kind: TaskLearningKind,
    route: TaskLearningRoute,
    observed_at: u64,
}

fn run_index_path() -> PathBuf {
    state_path_join(REL_TASK_RUN_INDEX)
}

fn learning_index_path() -> PathBuf {
    state_path_join(REL_TASK_LEARNING_INDEX)
}

fn run_file_path(run_id: &str) -> PathBuf {
    let ext = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        ".j"
    } else {
        ".json"
    };
    state_path_join(format!("{REL_DIR_TASK_RUN_FILES}/{run_id}{ext}"))
}

fn artifact_file_path(run_id: &str, artifact_id: &str) -> PathBuf {
    let ext = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        ".j"
    } else {
        ".json"
    };
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        state_path_join(format!(
            "{REL_DIR_TASK_ARTIFACT_FILES}/{}_{}{}",
            run_id, artifact_id, ext
        ))
    } else {
        state_path_join(format!(
            "{REL_DIR_TASK_ARTIFACT_FILES}/{run_id}/{artifact_id}{ext}"
        ))
    }
}

fn ledger_file_path(run_id: &str) -> PathBuf {
    let ext = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        ".jl"
    } else {
        ".jsonl"
    };
    state_path_join(format!("{REL_DIR_TASK_LEDGER_FILES}/{run_id}{ext}"))
}

fn learning_file_path(learning_id: &str) -> PathBuf {
    let ext = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        ".j"
    } else {
        ".json"
    };
    state_path_join(format!("{REL_DIR_TASK_LEARNING_FILES}/{learning_id}{ext}"))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn ensure_parent_dir(_path: &Path, _stage: &'static str) -> Result<()> {
    Ok(())
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn ensure_parent_dir(path: &Path, stage: &'static str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| Error::io(stage, error))?;
    }
    Ok(())
}

fn read_optional_json<T>(path: &Path, stage: &'static str) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { .. }) | Err(Error::Other { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    if buf.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&buf)
        .map(Some)
        .map_err(|error| Error::config(stage, error.to_string()))
}

fn list_dir_optional(path: &Path) -> Vec<String> {
    list_dir(path).unwrap_or_default()
}

pub struct SpiffsTaskRunStore {
    index: CachedJsonFileStore<HashMap<String, RunIndexEntry>>,
}

impl SpiffsTaskRunStore {
    pub fn new() -> Self {
        Self {
            index: CachedJsonFileStore::new(
                run_index_path,
                load_json_or_default,
                RUN_INDEX_STAGE_LOCK,
                RUN_INDEX_STAGE_CACHE,
                RUN_INDEX_STAGE_PERSIST,
            ),
        }
    }

    fn upsert_index(&self, record: &TaskRunRecord) -> Result<()> {
        self.index.with_cached_mut(|index| {
            let next = RunIndexEntry {
                run_id: record.run.run_id.clone(),
                source_channel: record.run.source_channel.clone(),
                source_chat_id: record.run.source_chat_id.clone(),
                title: record.run.title.clone(),
                status: record.run.status,
                updated_at: record.run.updated_at,
            };
            if index.get(&next.run_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            index.insert(next.run_id.clone(), next);
            Ok(StoreOp::dirty(()))
        })
    }
}

impl Default for SpiffsTaskRunStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRunStore for SpiffsTaskRunStore {
    fn get(&self, run_id: &str) -> Result<Option<TaskRunRecord>> {
        read_optional_json(&run_file_path(run_id), "task_run_read")
    }

    fn upsert(&self, record: &TaskRunRecord) -> Result<()> {
        let path = run_file_path(&record.run.run_id);
        ensure_parent_dir(&path, "task_run_dir")?;
        let encoded = serde_json::to_vec(record)
            .map_err(|error| Error::config("task_run_write", error.to_string()))?;
        write_file(&path, &encoded)?;
        self.upsert_index(record)
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskRunRecord>> {
        self.index.with_cached_mut(|index| {
            let mut entries = index.values().cloned().collect::<Vec<_>>();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
            let mut out = Vec::new();
            for entry in entries.into_iter().take(limit.max(1)) {
                if let Some(record) = self.get(&entry.run_id)? {
                    out.push(record);
                }
            }
            Ok(StoreOp::clean(out))
        })
    }

    fn list_active_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskRunRecord>> {
        self.index.with_cached_mut(|index| {
            let mut matches = index
                .values()
                .filter(|entry| {
                    entry.source_channel == channel
                        && entry.source_chat_id == chat_id
                        && entry.status.is_active()
                })
                .cloned()
                .collect::<Vec<_>>();
            matches.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
            let mut out = Vec::new();
            for entry in matches.into_iter().take(limit.max(1)) {
                if let Some(record) = self.get(&entry.run_id)? {
                    out.push(record);
                }
            }
            Ok(StoreOp::clean(out))
        })
    }
}

pub struct SpiffsTaskArtifactStore;

impl SpiffsTaskArtifactStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpiffsTaskArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskArtifactStore for SpiffsTaskArtifactStore {
    fn put(&self, record: &TaskArtifactRecord) -> Result<()> {
        let path = artifact_file_path(&record.artifact.run_id, &record.artifact.artifact_id);
        ensure_parent_dir(&path, "task_artifact_dir")?;
        let encoded = serde_json::to_vec(record)
            .map_err(|error| Error::config("task_artifact_write", error.to_string()))?;
        write_file(path, &encoded)
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskArtifactRecord>> {
        let names = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            list_dir_optional(&state_path_join(REL_DIR_TASK_ARTIFACT_FILES))
                .into_iter()
                .filter(|name| name.starts_with(run_id))
                .collect::<Vec<_>>()
        } else {
            list_dir_optional(&state_path_join(format!(
                "{REL_DIR_TASK_ARTIFACT_FILES}/{run_id}"
            )))
        };
        let mut out = Vec::new();
        for name in names.into_iter().take(limit.max(1)) {
            let path = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
                state_path_join(format!("{REL_DIR_TASK_ARTIFACT_FILES}/{name}"))
            } else {
                state_path_join(format!("{REL_DIR_TASK_ARTIFACT_FILES}/{run_id}/{name}"))
            };
            if let Some(record) =
                read_optional_json::<TaskArtifactRecord>(&path, "task_artifact_read")?
            {
                out.push(record);
            }
        }
        out.sort_by_key(|record| std::cmp::Reverse(record.artifact.created_at));
        if out.len() > limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    fn delete(&self, run_id: &str, artifact_id: &str) -> Result<bool> {
        let path = artifact_file_path(run_id, artifact_id);
        match remove_file(&path) {
            Ok(()) => Ok(true),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}

pub struct SpiffsTaskExecutionLedgerStore;

impl SpiffsTaskExecutionLedgerStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpiffsTaskExecutionLedgerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskExecutionLedgerStore for SpiffsTaskExecutionLedgerStore {
    fn append(&self, run_id: &str, entry: &TaskExecutionLedgerEntry) -> Result<()> {
        let path = ledger_file_path(run_id);
        ensure_parent_dir(&path, "task_execution_ledger_dir")?;
        let mut existing = String::new();
        if let Ok(buf) = super::read_file_to_vec(&path) {
            existing = String::from_utf8(buf)
                .map_err(|error| Error::config("task_execution_ledger_utf8", error.to_string()))?;
        }
        let line = serde_json::to_string(entry)
            .map_err(|error| Error::config("task_execution_ledger_write", error.to_string()))?;
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push_str(&line);
        existing.push('\n');
        write_file(path, existing.as_bytes())
    }

    fn list(&self, run_id: &str, limit: usize) -> Result<Vec<TaskExecutionLedgerEntry>> {
        let path = ledger_file_path(run_id);
        let buf = match super::read_file_to_vec(&path) {
            Ok(buf) => buf,
            Err(Error::Io { .. }) | Err(Error::Other { .. }) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        if buf.is_empty() {
            return Ok(Vec::new());
        }
        let content = String::from_utf8(buf)
            .map_err(|error| Error::config("task_execution_ledger_utf8", error.to_string()))?;
        let mut out = Vec::new();
        for line in content.lines().rev().take(limit.max(1)) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry = serde_json::from_str::<TaskExecutionLedgerEntry>(trimmed)
                .map_err(|error| Error::config("task_execution_ledger_read", error.to_string()))?;
            out.push(entry);
        }
        out.reverse();
        Ok(out)
    }
}

pub struct SpiffsTaskLearningStore {
    index: CachedJsonFileStore<HashMap<String, LearningIndexEntry>>,
}

impl SpiffsTaskLearningStore {
    pub fn new() -> Self {
        Self {
            index: CachedJsonFileStore::new(
                learning_index_path,
                load_json_or_default,
                LEARNING_INDEX_STAGE_LOCK,
                LEARNING_INDEX_STAGE_CACHE,
                LEARNING_INDEX_STAGE_PERSIST,
            ),
        }
    }

    fn upsert_index(&self, record: &TaskLearningRecord) -> Result<()> {
        self.index.with_cached_mut(|index| {
            let next = LearningIndexEntry {
                learning_id: record.learning_id.clone(),
                source_channel: record.source_channel.clone(),
                source_chat_id: record.source_chat_id.clone(),
                run_id: record.run_id.clone(),
                topic: record.topic.clone(),
                kind: record.kind,
                route: record.route,
                observed_at: record.observed_at,
            };
            if index.get(&next.learning_id) == Some(&next) {
                return Ok(StoreOp::clean(()));
            }
            index.insert(next.learning_id.clone(), next);
            Ok(StoreOp::dirty(()))
        })
    }
}

impl Default for SpiffsTaskLearningStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskLearningStore for SpiffsTaskLearningStore {
    fn get(&self, learning_id: &str) -> Result<Option<TaskLearningRecord>> {
        read_optional_json(&learning_file_path(learning_id), "task_learning_read")
    }

    fn upsert(&self, record: &TaskLearningRecord) -> Result<()> {
        let path = learning_file_path(&record.learning_id);
        ensure_parent_dir(&path, "task_learning_dir")?;
        let encoded = serde_json::to_vec(record)
            .map_err(|error| Error::config("task_learning_write", error.to_string()))?;
        write_file(&path, &encoded)?;
        self.upsert_index(record)
    }

    fn list_recent(&self, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        self.index.with_cached_mut(|index| {
            let mut entries = index.values().cloned().collect::<Vec<_>>();
            entries.sort_by_key(|entry| std::cmp::Reverse(entry.observed_at));
            let mut out = Vec::new();
            for entry in entries.into_iter().take(limit.max(1)) {
                if let Some(record) = self.get(&entry.learning_id)? {
                    out.push(record);
                }
            }
            Ok(StoreOp::clean(out))
        })
    }

    fn list_for_chat(
        &self,
        channel: &str,
        chat_id: &str,
        limit: usize,
    ) -> Result<Vec<TaskLearningRecord>> {
        self.index.with_cached_mut(|index| {
            let mut matches = index
                .values()
                .filter(|entry| entry.source_channel == channel && entry.source_chat_id == chat_id)
                .cloned()
                .collect::<Vec<_>>();
            matches.sort_by_key(|entry| std::cmp::Reverse(entry.observed_at));
            let mut out = Vec::new();
            for entry in matches.into_iter().take(limit.max(1)) {
                if let Some(record) = self.get(&entry.learning_id)? {
                    out.push(record);
                }
            }
            Ok(StoreOp::clean(out))
        })
    }

    fn list_for_run(&self, run_id: &str, limit: usize) -> Result<Vec<TaskLearningRecord>> {
        self.index.with_cached_mut(|index| {
            let mut matches = index
                .values()
                .filter(|entry| entry.run_id == run_id)
                .cloned()
                .collect::<Vec<_>>();
            matches.sort_by_key(|entry| std::cmp::Reverse(entry.observed_at));
            let mut out = Vec::new();
            for entry in matches.into_iter().take(limit.max(1)) {
                if let Some(record) = self.get(&entry.learning_id)? {
                    out.push(record);
                }
            }
            Ok(StoreOp::clean(out))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esp_paths_stay_short_enough() {
        if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            assert!(run_file_path("tr00112233445566").to_string_lossy().len() < 64);
            assert!(
                artifact_file_path("tr00112233445566", "a01")
                    .to_string_lossy()
                    .len()
                    < 64
            );
            assert!(
                learning_file_path("tr00112233445566_s01_l01")
                    .to_string_lossy()
                    .len()
                    < 64
            );
        }
    }

    #[test]
    fn task_ledger_round_trip_text_lines() {
        let entry = TaskExecutionLedgerEntry {
            sequence: 1,
            run_id: "tr00112233445566".to_string(),
            step_id: "s01".to_string(),
            kind: crate::task_execution::TaskLedgerKind::StepStarted,
            run_status: TaskRunStatus::Running,
            message: "started".to_string(),
            recorded_at: 1,
        };
        let line = serde_json::to_string(&entry).unwrap();
        let parsed: TaskExecutionLedgerEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed.sequence, 1);
    }
}
