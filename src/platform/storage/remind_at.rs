//! storage 实现的到点提醒存储。单文件 memory/remind_at.json，按 at 排序。

use crate::constants::REMIND_AT_MAX_ENTRIES;
use crate::error::{Error, Result};
use crate::memory::RemindAtStore;
use crate::reminder::{normalize_reminder_item, ReminderItem, REL_PATH_REMINDERS};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::{CachedJsonFileStore, StoreOp};
use super::read_file;
use super::state_path_join;

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_REMINDERS)
}

#[derive(Clone, Serialize, Deserialize)]
struct LegacyRemindEntry {
    channel: String,
    chat_id: String,
    at_unix_secs: u64,
    context: String,
}

fn load_reminders_or_default(path: &PathBuf, stage: &'static str) -> Result<Vec<ReminderItem>> {
    match read_file(path) {
        Ok(buf) if buf.len() > 2 => {
            if let Ok(items) = serde_json::from_slice::<Vec<ReminderItem>>(&buf) {
                return Ok(items);
            }
            if let Ok(items) = serde_json::from_slice::<Vec<LegacyRemindEntry>>(&buf) {
                return Ok(items
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, legacy)| {
                        normalize_reminder_item(ReminderItem {
                            id: format!("legacy_rem_{index}"),
                            channel: legacy.channel,
                            chat_id: legacy.chat_id,
                            at_unix_secs: legacy.at_unix_secs,
                            context: legacy.context,
                            ..ReminderItem::default()
                        })
                        .ok()
                    })
                    .collect());
            }
            Err(crate::error::Error::config(
                stage,
                "invalid reminder cache json",
            ))
        }
        Ok(_) => Ok(Vec::new()),
        Err(crate::error::Error::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error.with_stage(stage)),
    }
}

fn upsert_reminder(list: &mut Vec<ReminderItem>, reminder: ReminderItem) -> Result<bool> {
    if let Some(existing) = list.iter_mut().find(|entry| {
        entry.channel == reminder.channel
            && entry.chat_id == reminder.chat_id
            && entry.id == reminder.id
    }) {
        if *existing == reminder {
            return Ok(false);
        }
        *existing = reminder;
    } else {
        if list.len() >= REMIND_AT_MAX_ENTRIES {
            return Err(Error::config(
                "remind_at_capacity",
                format!("reminder store full (max {REMIND_AT_MAX_ENTRIES})"),
            ));
        }
        list.push(reminder);
    }
    list.sort_by(|left, right| {
        left.at_unix_secs
            .cmp(&right.at_unix_secs)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(true)
}

/// 单文件，JSON 数组；upsert 时按 at/id 排序。
pub struct StorageRemindAtStore {
    store: CachedJsonFileStore<Vec<ReminderItem>>,
}

impl StorageRemindAtStore {
    pub fn new() -> Self {
        Self::new_with_path(full_path)
    }

    fn new_with_path(path_fn: fn() -> PathBuf) -> Self {
        Self {
            store: CachedJsonFileStore::new(
                path_fn,
                load_reminders_or_default,
                "remind_at_cache_lock",
                "remind_at_cache",
                "remind_at_persist",
            ),
        }
    }

    /// 预热进程内缓存；避免 bg_timer 首轮在小栈线程中同步读盘。
    pub fn warm_cache(&self) -> Result<()> {
        self.store
            .with_cached_mut(|_| Ok(StoreOp::clean(())))
            .map(|_| ())
    }
}

impl Default for StorageRemindAtStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RemindAtStore for StorageRemindAtStore {
    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<ReminderItem>> {
        self.store.with_cached_mut(|list| {
            Ok(StoreOp::clean(
                list.iter()
                    .find(|entry| {
                        entry.channel == channel && entry.chat_id == chat_id && entry.id == id
                    })
                    .cloned(),
            ))
        })
    }

    fn upsert(&self, reminder: &ReminderItem) -> Result<()> {
        let reminder = normalize_reminder_item(reminder.clone())?;
        let changed = self.store.with_cached_mut(|list| {
            let changed = upsert_reminder(list, reminder.clone())?;
            Ok(StoreOp::with_dirty(changed, changed))
        })?;
        if changed {
            crate::bg_timer::notify_deadline_changed();
        }
        Ok(())
    }

    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool> {
        let deleted = self.store.with_cached_mut(|list| {
            let pos = list.iter().position(|entry| {
                entry.channel == channel && entry.chat_id == chat_id && entry.id == id
            });
            let Some(idx) = pos else {
                return Ok(StoreOp::clean(false));
            };
            list.remove(idx);
            Ok(StoreOp::dirty(true))
        })?;
        if deleted {
            crate::bg_timer::notify_deadline_changed();
        }
        Ok(deleted)
    }

    fn list_due(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<ReminderItem>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.store.with_cached_mut(|list| {
            Ok(StoreOp::clean(
                list.iter()
                    .filter(|entry| entry.at_unix_secs <= now_unix_secs)
                    .take(limit)
                    .cloned()
                    .collect(),
            ))
        })
    }

    fn delete_due(&self, reminder: &ReminderItem) -> Result<bool> {
        let deleted = self.store.with_cached_mut(|list| {
            let Some(idx) = list.iter().position(|entry| entry == reminder) else {
                return Ok(StoreOp::clean(false));
            };
            list.remove(idx);
            Ok(StoreOp::dirty(true))
        })?;
        if deleted {
            crate::bg_timer::notify_deadline_changed();
        }
        Ok(deleted)
    }

    fn next_due_at(&self) -> Result<Option<u64>> {
        self.store.with_cached_mut(|list| {
            Ok(StoreOp::clean(
                list.iter().map(|entry| entry.at_unix_secs).min(),
            ))
        })
    }

    fn list_upcoming(
        &self,
        channel: &str,
        chat_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<ReminderItem>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.store.with_cached_mut(|list| {
            let mut out = Vec::new();
            for entry in list.iter() {
                if entry.channel == channel
                    && entry.chat_id == chat_id
                    && entry.at_unix_secs >= now_unix_secs
                {
                    out.push(entry.clone());
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            Ok(StoreOp::clean(out))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::RemindAtStore;
    use std::sync::{Mutex, MutexGuard, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_store_path() -> PathBuf {
        static PATH: OnceLock<PathBuf> = OnceLock::new();
        PATH.get_or_init(|| {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("beetle-remind-at-{unique}"));
            std::fs::create_dir_all(&root).unwrap();
            root.join("remind_at.json")
        })
        .clone()
    }

    fn reset_test_store() {
        let path = test_store_path();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::create_dir_all(path.parent().unwrap());
    }

    fn test_store_guard() -> MutexGuard<'static, ()> {
        static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
        GUARD
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn reminder(id: &str, at_unix_secs: u64) -> ReminderItem {
        ReminderItem {
            id: id.to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            at_unix_secs,
            context: format!("reminder-{id}"),
            ..ReminderItem::default()
        }
    }

    #[test]
    fn remind_store_rejects_new_entry_when_capacity_is_exhausted() {
        let _guard = test_store_guard();
        reset_test_store();
        let store = StorageRemindAtStore::new_with_path(test_store_path);
        for idx in 0..REMIND_AT_MAX_ENTRIES {
            store
                .upsert(&reminder(&format!("rem-{idx}"), idx as u64 + 1))
                .unwrap();
        }

        let err = store
            .upsert(&reminder("overflow", REMIND_AT_MAX_ENTRIES as u64 + 10))
            .expect_err("new reminder beyond capacity must fail");
        assert_eq!(err.stage(), "remind_at_capacity");
        assert!(store
            .get("qq_channel", "chat-1", "overflow")
            .unwrap()
            .is_none());
        assert_eq!(
            store
                .list_upcoming("qq_channel", "chat-1", 0, REMIND_AT_MAX_ENTRIES + 10)
                .unwrap()
                .len(),
            REMIND_AT_MAX_ENTRIES
        );
    }

    #[test]
    fn remind_store_allows_updating_existing_entry_at_capacity() {
        let _guard = test_store_guard();
        reset_test_store();
        let store = StorageRemindAtStore::new_with_path(test_store_path);
        for idx in 0..REMIND_AT_MAX_ENTRIES {
            store
                .upsert(&reminder(&format!("rem-{idx}"), idx as u64 + 1))
                .unwrap();
        }

        store.upsert(&reminder("rem-0", 9_999)).unwrap();

        let updated = store
            .get("qq_channel", "chat-1", "rem-0")
            .unwrap()
            .expect("updated reminder");
        assert_eq!(updated.at_unix_secs, 9_999);
    }

    #[test]
    fn delete_due_does_not_remove_updated_same_id_reminder() {
        let _guard = test_store_guard();
        reset_test_store();
        let store = StorageRemindAtStore::new_with_path(test_store_path);
        store.upsert(&reminder("rem-1", 1)).unwrap();
        let due = store.list_due(1, 1).unwrap().pop().expect("due reminder");

        let mut updated = reminder("rem-1", 9_999);
        updated.context = "updated".to_string();
        store.upsert(&updated).unwrap();

        assert!(!store.delete_due(&due).unwrap());
        let loaded = store
            .get("qq_channel", "chat-1", "rem-1")
            .unwrap()
            .expect("updated reminder retained");
        assert_eq!(loaded, updated);
    }
}
