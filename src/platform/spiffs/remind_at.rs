//! SPIFFS 实现的到点提醒存储。单文件 memory/remind_at.json，按 at 排序，条数/context 上界见 constants。

use crate::constants::{REMIND_AT_MAX_CONTEXT_LEN, REMIND_AT_MAX_ENTRIES};
use crate::error::Result;
use crate::memory::RemindAtStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const REL_PATH_REMIND_AT: &str = "memory/remind_at.json";

#[derive(Clone, Serialize, Deserialize)]
struct RemindEntry {
    channel: String,
    chat_id: String,
    at_unix_secs: u64,
    context: String,
}

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_REMIND_AT)
}

fn truncate_context(s: &str) -> String {
    if s.len() <= REMIND_AT_MAX_CONTEXT_LEN {
        s.to_string()
    } else {
        s.chars()
            .take(REMIND_AT_MAX_CONTEXT_LEN)
            .collect::<String>()
    }
}

/// 单文件，JSON 数组；add 时按 at 排序并保留最多 REMIND_AT_MAX_ENTRIES 条。
pub struct SpiffsRemindAtStore {
    store: CachedJsonFileStore<Vec<RemindEntry>>,
}

impl SpiffsRemindAtStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
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
    fn add(&self, channel: &str, chat_id: &str, at_unix_secs: u64, context: &str) -> Result<()> {
        self.store.with_cached_mut(|list| {
            list.push(RemindEntry {
                channel: channel.to_string(),
                chat_id: chat_id.to_string(),
                at_unix_secs,
                context: truncate_context(context),
            });
            list.sort_by_key(|entry| entry.at_unix_secs);
            if list.len() > REMIND_AT_MAX_ENTRIES {
                list.truncate(REMIND_AT_MAX_ENTRIES);
            }
            Ok(StoreOp::dirty(()))
        })?;
        crate::bg_timer::notify_deadline_changed();
        Ok(())
    }

    fn pop_due(&self, now_unix_secs: u64) -> Result<Option<(String, String, String)>> {
        self.store.with_cached_mut(|list| {
            let pos = list
                .iter()
                .position(|entry| entry.at_unix_secs <= now_unix_secs);
            let Some(idx) = pos else {
                return Ok(StoreOp::clean(None));
            };
            let removed = list.remove(idx);
            Ok(StoreOp::dirty(Some((
                removed.channel,
                removed.chat_id,
                removed.context,
            ))))
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
    ) -> Result<Vec<(u64, String)>> {
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
                    out.push((entry.at_unix_secs, entry.context.clone()));
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            Ok(StoreOp::clean(out))
        })
    }
}
