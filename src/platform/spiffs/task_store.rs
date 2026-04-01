//! SPIFFS / state-root backed task store.

use crate::error::{Error, Result};
use crate::task::{
    filter_tasks, normalize_task_item, TaskItem, TaskQuery, TaskStore, REL_PATH_TASKS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_TASK_ITEMS: usize = 256;

#[derive(Clone, Serialize, Deserialize)]
struct StoredTaskItem(TaskItem);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TASKS)
}

pub struct SpiffsTaskStore {
    cache: Mutex<Option<HashMap<String, StoredTaskItem>>>,
}

impl SpiffsTaskStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, StoredTaskItem> {
        match read_file(full_path()) {
            Ok(buf) if buf.len() > 2 => serde_json::from_slice(&buf).unwrap_or_default(),
            _ => HashMap::new(),
        }
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, StoredTaskItem>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("task_store_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("task_store_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, StoredTaskItem>) -> Result<()> {
        let json =
            serde_json::to_vec(map).map_err(|e| Error::config("task_store_set", e.to_string()))?;
        write_file(full_path(), &json)
    }

    fn trim_if_needed(map: &mut HashMap<String, StoredTaskItem>) {
        while map.len() > MAX_TASK_ITEMS {
            let remove_id = map
                .iter()
                .min_by_key(|(_, item)| item.0.updated_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = remove_id {
                map.remove(&id);
            } else {
                break;
            }
        }
    }
}

impl Default for SpiffsTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskStore for SpiffsTaskStore {
    fn list(&self, channel: &str, chat_id: &str, query: TaskQuery) -> Result<Vec<TaskItem>> {
        self.with_map_mut(|map| {
            let tasks = map
                .values()
                .filter(|item| item.0.channel == channel && item.0.chat_id == chat_id)
                .map(|item| item.0.clone())
                .collect::<Vec<_>>();
            Ok(filter_tasks(tasks, query))
        })
    }

    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>> {
        self.with_map_mut(|map| {
            Ok(map.get(id).and_then(|item| {
                (item.0.channel == channel && item.0.chat_id == chat_id).then(|| item.0.clone())
            }))
        })
    }

    fn upsert(&self, task: &TaskItem) -> Result<()> {
        self.with_map_mut(|map| {
            let normalized = normalize_task_item(task.clone())?;
            map.insert(normalized.id.clone(), StoredTaskItem(normalized));
            Self::trim_if_needed(map);
            Self::persist(map)
        })
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        self.with_map_mut(|map| {
            let matched = map
                .get(id)
                .map(|item| item.0.channel == channel && item.0.chat_id == chat_id)
                .unwrap_or(false);
            if !matched {
                return Ok(false);
            }
            let removed = map.remove(id).is_some();
            if removed {
                Self::persist(map)?;
            }
            Ok(removed)
        })
    }

    fn claim_due(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
        self.with_map_mut(|map| {
            if limit == 0 {
                return Ok(Vec::new());
            }
            let mut due = map
                .values()
                .filter_map(|item| {
                    let task = &item.0;
                    (task.due_at_unix_secs != 0
                        && task.due_at_unix_secs <= now_unix_secs
                        && task.due_notified_at_unix_secs == 0
                        && !task.status.is_terminal())
                    .then(|| task.clone())
                })
                .collect::<Vec<_>>();
            due.sort_by(|left, right| {
                left.due_at_unix_secs
                    .cmp(&right.due_at_unix_secs)
                    .then_with(|| left.updated_at.cmp(&right.updated_at))
                    .then_with(|| left.id.cmp(&right.id))
            });
            if due.len() > limit {
                due.truncate(limit);
            }
            if due.is_empty() {
                return Ok(Vec::new());
            }
            for task in &due {
                if let Some(item) = map.get_mut(&task.id) {
                    item.0.due_notified_at_unix_secs = now_unix_secs;
                }
            }
            Self::persist(map)?;
            Ok(due)
        })
    }
}
