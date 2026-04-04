//! SPIFFS / state-root backed task store.

use crate::error::Result;
use crate::task::{
    REL_PATH_TASKS, TaskItem, TaskQuery, TaskStore, filter_tasks, normalize_task_item,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{CachedJsonFileStore, StoreOp, load_json_or_default};
use super::state_path_join;

const MAX_TASK_ITEMS: usize = 256;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTaskItem(TaskItem);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_TASKS)
}

pub struct SpiffsTaskStore {
    store: CachedJsonFileStore<HashMap<String, StoredTaskItem>>,
}

impl SpiffsTaskStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "task_store_cache_lock",
                "task_store_cache",
                "task_store_persist",
            ),
        }
    }

    /// 预热进程内缓存；在平台初始化阶段调用，把首次磁盘读取从低栈后台线程前移。
    pub fn warm_cache(&self) -> Result<()> {
        self.store
            .with_cached_mut(|_| Ok(StoreOp::clean(())))
            .map(|_| ())
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
        self.store.with_cached_mut(|map| {
            let tasks = map
                .values()
                .filter(|item| item.0.channel == channel && item.0.chat_id == chat_id)
                .map(|item| item.0.clone())
                .collect::<Vec<_>>();
            Ok(StoreOp::clean(filter_tasks(tasks, query)))
        })
    }

    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(map.get(id).and_then(|item| {
                (item.0.channel == channel && item.0.chat_id == chat_id).then(|| item.0.clone())
            })))
        })
    }

    fn upsert(&self, task: &TaskItem) -> Result<()> {
        let changed = self.store.with_cached_mut(|map| {
            let normalized = normalize_task_item(task.clone())?;
            let next_item = StoredTaskItem(normalized);
            if map.get(&next_item.0.id) == Some(&next_item) {
                return Ok(StoreOp::clean(false));
            }
            map.insert(next_item.0.id.clone(), next_item);
            Self::trim_if_needed(map);
            Ok(StoreOp::dirty(true))
        })?;
        if changed {
            crate::bg_timer::notify_deadline_changed();
        }
        Ok(())
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        let removed = self.store.with_cached_mut(|map| {
            let matched = map
                .get(id)
                .map(|item| item.0.channel == channel && item.0.chat_id == chat_id)
                .unwrap_or(false);
            if !matched {
                return Ok(StoreOp::clean(false));
            }
            let removed = map.remove(id).is_some();
            Ok(StoreOp::with_dirty(removed, removed))
        })?;
        if removed {
            crate::bg_timer::notify_deadline_changed();
        }
        Ok(removed)
    }

    fn claim_due(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
        self.store.with_cached_mut(|map| {
            if limit == 0 {
                return Ok(StoreOp::clean(Vec::new()));
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
                return Ok(StoreOp::clean(Vec::new()));
            }
            for task in &due {
                if let Some(item) = map.get_mut(&task.id) {
                    item.0.due_notified_at_unix_secs = now_unix_secs;
                }
            }
            Ok(StoreOp::dirty(due))
        })
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        self.store.with_cached_mut(|map| {
            Ok(StoreOp::clean(
                map.values()
                    .filter_map(|item| {
                        let task = &item.0;
                        (task.due_at_unix_secs != 0
                            && task.due_notified_at_unix_secs == 0
                            && !task.status.is_terminal())
                        .then_some(task.due_at_unix_secs)
                    })
                    .min(),
            ))
        })
    }
}
