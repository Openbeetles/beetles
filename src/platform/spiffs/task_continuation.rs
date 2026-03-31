//! SPIFFS 实现的多轮延续存储。单文件 memory/task_continuation.json，单设备单任务。
//! TaskContinuationStore implementation; single file for one task state.

use crate::constants::TASK_CONTINUATION_MAX_OUTPUT_LEN;
use crate::error::{Error, Result};
use crate::memory::{TaskContinuationStore, REL_PATH_TASK_CONTINUATION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_TASK_CONTINUATIONS: usize = 8;

#[derive(Serialize, Deserialize)]
struct TaskContinuationEntry {
    round: u32,
    last_output: String,
}

/// 兼容旧格式：单设备单任务。
#[derive(Serialize, Deserialize)]
struct LegacyTaskContinuationState {
    chat_id: String,
    round: u32,
    last_output: String,
}

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TASK_CONTINUATION)
}

fn truncate_output_to_max(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut len = 0usize;
    let mut out = String::new();
    for c in s.chars() {
        let n = c.len_utf8();
        if len + n > max_bytes {
            break;
        }
        len += n;
        out.push(c);
    }
    out
}

/// 单文件缓存；按 chat_id 保存多轮延续状态。
pub struct SpiffsTaskContinuationStore {
    cache: Mutex<Option<HashMap<String, TaskContinuationEntry>>>,
}

impl SpiffsTaskContinuationStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, TaskContinuationEntry> {
        let path = full_path();
        let buf = match read_file(&path) {
            Ok(buf) => buf,
            Err(_) => return HashMap::new(),
        };
        if buf.len() <= 2 {
            return HashMap::new();
        }
        if let Ok(map) = serde_json::from_slice::<HashMap<String, TaskContinuationEntry>>(&buf) {
            return map;
        }
        if let Ok(legacy) = serde_json::from_slice::<LegacyTaskContinuationState>(&buf) {
            let mut map = HashMap::with_capacity(1);
            map.insert(
                legacy.chat_id,
                TaskContinuationEntry {
                    round: legacy.round,
                    last_output: legacy.last_output,
                },
            );
            return map;
        }
        HashMap::new()
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, TaskContinuationEntry>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("task_continuation_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("task_continuation_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, TaskContinuationEntry>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("task_continuation_set", e.to_string()))?;
        write_file(full_path(), &json)
    }
}

impl Default for SpiffsTaskContinuationStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskContinuationStore for SpiffsTaskContinuationStore {
    fn get_task_continuation(&self, chat_id: &str) -> Result<Option<(u32, String)>> {
        self.with_map_mut(|map| {
            Ok(map
                .get(chat_id)
                .map(|entry| (entry.round, entry.last_output.clone())))
        })
    }

    fn set_task_continuation(&self, chat_id: &str, round: u32, last_output: &str) -> Result<()> {
        let truncated = truncate_output_to_max(last_output, TASK_CONTINUATION_MAX_OUTPUT_LEN);
        self.with_map_mut(|map| {
            if !map.contains_key(chat_id) && map.len() >= MAX_TASK_CONTINUATIONS {
                if let Some(key_to_remove) = map.keys().next().cloned() {
                    map.remove(&key_to_remove);
                }
            }
            map.insert(
                chat_id.to_string(),
                TaskContinuationEntry {
                    round,
                    last_output: truncated,
                },
            );
            Self::persist(map)
        })
    }

    fn clear_task_continuation(&self, chat_id: &str) -> Result<()> {
        self.with_map_mut(|map| {
            if map.remove(chat_id).is_some() {
                Self::persist(map)?;
            }
            Ok(())
        })
    }
}
