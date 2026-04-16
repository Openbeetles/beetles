//! SPIFFS 实现的到点提醒存储。单文件 memory/remind_at.json，按 at 排序。

use crate::constants::REMIND_AT_MAX_ENTRIES;
use crate::error::Result;
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

fn load_reminders_or_default(path: &PathBuf) -> Vec<ReminderItem> {
    match read_file(path) {
        Ok(buf) if buf.len() > 2 => {
            if let Ok(items) = serde_json::from_slice::<Vec<ReminderItem>>(&buf) {
                return items;
            }
            if let Ok(items) = serde_json::from_slice::<Vec<LegacyRemindEntry>>(&buf) {
                return items
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
                    .collect();
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// 单文件，JSON 数组；upsert 时按 at/id 排序。
pub struct SpiffsRemindAtStore {
    store: CachedJsonFileStore<Vec<ReminderItem>>,
}

impl SpiffsRemindAtStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
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

impl Default for SpiffsRemindAtStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RemindAtStore for SpiffsRemindAtStore {
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
        self.store.with_cached_mut(|list| {
            if let Some(existing) = list.iter_mut().find(|entry| {
                entry.channel == reminder.channel
                    && entry.chat_id == reminder.chat_id
                    && entry.id == reminder.id
            }) {
                *existing = reminder.clone();
            } else {
                list.push(reminder.clone());
            }
            list.sort_by(|left, right| {
                left.at_unix_secs
                    .cmp(&right.at_unix_secs)
                    .then_with(|| left.id.cmp(&right.id))
            });
            if list.len() > REMIND_AT_MAX_ENTRIES {
                list.truncate(REMIND_AT_MAX_ENTRIES);
            }
            Ok(StoreOp::dirty(()))
        })?;
        crate::bg_timer::notify_deadline_changed();
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

    fn pop_due(&self, now_unix_secs: u64) -> Result<Option<ReminderItem>> {
        self.store.with_cached_mut(|list| {
            let pos = list
                .iter()
                .position(|entry| entry.at_unix_secs <= now_unix_secs);
            let Some(idx) = pos else {
                return Ok(StoreOp::clean(None));
            };
            let removed = list.remove(idx);
            Ok(StoreOp::dirty(Some(removed)))
        })
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
