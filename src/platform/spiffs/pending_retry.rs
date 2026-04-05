//! SPIFFS 实现的 PendingRetryStore。单文件 memory/pending_retry.json，存 { msg, replay_count }；
//! replay_count 达上限后不再注入，避免重复饥饿。

use crate::bus::{PcMsg, MAX_CONTENT_LEN};
use crate::constants::PENDING_RETRY_MAX_REPLAY;
use crate::error::{Error, Result};
use crate::memory::{PendingRetryStore, REL_PATH_PENDING_RETRY};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_PENDING_RETRY)
}

#[derive(Clone, Serialize, Deserialize)]
struct PendingRetryEntry {
    msg: PcMsg,
    #[serde(default)]
    replay_count: u32,
}

#[derive(Default)]
struct PendingRetryCache {
    loaded: bool,
    entry: Option<PendingRetryEntry>,
}

/// 单槽 pending_retry，带进程内镜像，避免每次 save 先读盘。
pub struct SpiffsPendingRetryStore {
    cache: Mutex<PendingRetryCache>,
}

impl SpiffsPendingRetryStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(PendingRetryCache::default()),
        }
    }

    fn load_cache_locked(cache: &mut PendingRetryCache) {
        if cache.loaded {
            return;
        }
        cache.loaded = true;
        let path = full_path();
        let buf = match read_file(&path) {
            Ok(buf) => buf,
            Err(_) => return,
        };
        if buf.len() <= 2 {
            return;
        }
        cache.entry = match serde_json::from_slice::<PendingRetryEntry>(&buf) {
            Ok(entry) => Some(entry),
            Err(_) => match serde_json::from_slice::<PcMsg>(&buf) {
                Ok(msg) => Some(PendingRetryEntry {
                    msg,
                    replay_count: 1,
                }),
                Err(_) => {
                    log::warn!("[spiffs_pending_retry] load parse failed");
                    None
                }
            },
        };
    }
}

impl Default for SpiffsPendingRetryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingRetryStore for SpiffsPendingRetryStore {
    fn save_pending_retry(&self, msg: &PcMsg) -> Result<()> {
        if msg.content.len() > MAX_CONTENT_LEN {
            return Err(Error::config(
                "pending_retry_save",
                format!(
                    "content len {} exceeds {}",
                    msg.content.len(),
                    MAX_CONTENT_LEN
                ),
            ));
        }
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Self::load_cache_locked(&mut cache);
        let replay_count = cache
            .entry
            .as_ref()
            .map(|entry| {
                entry
                    .replay_count
                    .saturating_add(1)
                    .min(PENDING_RETRY_MAX_REPLAY)
            })
            .unwrap_or(1);
        let entry = PendingRetryEntry {
            msg: msg.clone(),
            replay_count,
        };
        let json = serde_json::to_vec(&entry)
            .map_err(|e| Error::config("pending_retry_save", e.to_string()))?;
        write_file(full_path(), &json)?;
        cache.entry = Some(entry);
        Ok(())
    }

    fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        Self::load_cache_locked(&mut cache);
        let Some((replay_count, msg)) = cache
            .entry
            .as_ref()
            .map(|entry| (entry.replay_count, entry.msg.clone()))
        else {
            return Ok(None);
        };
        if replay_count >= PENDING_RETRY_MAX_REPLAY {
            let _ = write_file(&full_path(), b"{}");
            cache.entry = None;
            log::info!(
                "[spiffs_pending_retry] replay_count {} >= {}, cleared",
                replay_count,
                PENDING_RETRY_MAX_REPLAY
            );
            return Ok(None);
        }
        Ok(Some(msg))
    }

    fn clear_pending_retry(&self) -> Result<()> {
        write_file(full_path(), b"{}")?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.loaded = true;
        cache.entry = None;
        Ok(())
    }
}
