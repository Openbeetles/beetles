//! SPIFFS / state-root backed task store.

use crate::error::{Error, Result};
use crate::task::{
    filter_tasks, normalize_task_item, TaskItem, TaskQuery, TaskStore, REL_PATH_TASKS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
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
        Self::new_with_path(full_path)
    }

    fn new_with_path(path_fn: fn() -> PathBuf) -> Self {
        Self {
            store: CachedJsonFileStore::new(
                path_fn,
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
            if !map.contains_key(&next_item.0.id) && map.len() >= MAX_TASK_ITEMS {
                return Err(Error::config(
                    "task_store_capacity",
                    format!("task store full (max {MAX_TASK_ITEMS})"),
                ));
            }
            map.insert(next_item.0.id.clone(), next_item);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskStore;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_store_path() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("beetle-task-store-{unique}"));
            std::fs::create_dir_all(&root).unwrap();
            root.join("tasks.json")
        })
        .clone()
    }

    fn reset_test_store() {
        let path = test_store_path();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::create_dir_all(path.parent().unwrap());
    }

    fn task(id: &str, updated_at: u64) -> TaskItem {
        TaskItem {
            id: id.to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            title: format!("task-{id}"),
            updated_at,
            ..TaskItem::default()
        }
    }

    #[test]
    fn task_store_rejects_new_entry_when_capacity_is_exhausted() {
        reset_test_store();
        let store = SpiffsTaskStore::new_with_path(test_store_path);
        for idx in 0..MAX_TASK_ITEMS {
            store
                .upsert(&task(&format!("task-{idx}"), idx as u64 + 1))
                .unwrap();
        }

        let err = store
            .upsert(&task("overflow", 9_999))
            .expect_err("new task beyond capacity must fail");
        assert_eq!(err.stage(), "task_store_capacity");
        assert!(store
            .get("qq_channel", "chat-1", "overflow")
            .unwrap()
            .is_none());
        let retained = (0..MAX_TASK_ITEMS)
            .filter(|idx| {
                store
                    .get("qq_channel", "chat-1", &format!("task-{idx}"))
                    .unwrap()
                    .is_some()
            })
            .count();
        assert_eq!(retained, MAX_TASK_ITEMS);
    }

    #[test]
    fn task_store_allows_updating_existing_entry_at_capacity() {
        reset_test_store();
        let store = SpiffsTaskStore::new_with_path(test_store_path);
        for idx in 0..MAX_TASK_ITEMS {
            store
                .upsert(&task(&format!("task-{idx}"), idx as u64 + 1))
                .unwrap();
        }

        let mut updated = task("task-0", 9_999);
        updated.detail = "updated".to_string();
        store.upsert(&updated).unwrap();

        let loaded = store
            .get("qq_channel", "chat-1", "task-0")
            .unwrap()
            .expect("updated task");
        assert_eq!(loaded.detail, "updated");
        assert_eq!(loaded.updated_at, 9_999);
    }
}
