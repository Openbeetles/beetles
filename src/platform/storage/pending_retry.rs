//! storage 实现的 PendingRetryStore。单文件 memory/pending_retry.json，存 { msg, replay_count }；
//! replay_count 达上限后不再注入，避免重复饥饿。

use crate::bus::{PcMsg, MAX_CONTENT_LEN};
use crate::constants::PENDING_RETRY_MAX_REPLAY;
use crate::error::{Error, Result};
use crate::memory::{PendingRetryStore, REL_PATH_PENDING_RETRY};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_json_file};

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
pub struct StoragePendingRetryStore {
    cache: Mutex<PendingRetryCache>,
    path: PathBuf,
}

impl StoragePendingRetryStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(PendingRetryCache::default()),
            path: full_path(),
        }
    }

    #[cfg(test)]
    fn new_for_test_path(path: PathBuf) -> Self {
        Self {
            cache: Mutex::new(PendingRetryCache::default()),
            path,
        }
    }

    fn load_cache_locked(&self, cache: &mut PendingRetryCache) -> Result<()> {
        if cache.loaded {
            return Ok(());
        }
        cache.loaded = true;
        let buf = match read_file(&self.path) {
            Ok(buf) => buf,
            Err(Error::Io { source, stage })
                if stage == "storage_read" && source.kind() == ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => {
                cache.loaded = false;
                log::warn!("[storage_pending_retry] load read failed: {}", error);
                return Err(error);
            }
        };
        if buf.len() <= 2 {
            return Ok(());
        }
        cache.entry = match serde_json::from_slice::<PendingRetryEntry>(&buf) {
            Ok(entry) => Some(entry),
            Err(_) => match serde_json::from_slice::<PcMsg>(&buf) {
                Ok(msg) => Some(PendingRetryEntry {
                    msg,
                    replay_count: 1,
                }),
                Err(_) => {
                    log::warn!("[storage_pending_retry] load parse failed");
                    None
                }
            },
        };
        Ok(())
    }
}

impl Default for StoragePendingRetryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingRetryStore for StoragePendingRetryStore {
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
        self.load_cache_locked(&mut cache)?;
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
        write_json_file(&self.path, &json)?;
        cache.entry = Some(entry);
        Ok(())
    }

    fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        self.load_cache_locked(&mut cache)?;
        let Some((replay_count, msg)) = cache
            .entry
            .as_ref()
            .map(|entry| (entry.replay_count, entry.msg.clone()))
        else {
            return Ok(None);
        };
        if replay_count >= PENDING_RETRY_MAX_REPLAY {
            let _ = write_json_file(&self.path, b"{}");
            cache.entry = None;
            log::info!(
                "[storage_pending_retry] replay_count {} >= {}, cleared",
                replay_count,
                PENDING_RETRY_MAX_REPLAY
            );
            return Ok(None);
        }
        Ok(Some(msg))
    }

    fn clear_pending_retry(&self) -> Result<()> {
        write_json_file(&self.path, b"{}")?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.loaded = true;
        cache.entry = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::PendingRetryStore;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_pending_retry_path(name: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "beetle-pending-retry-{}-{}-{id}.json",
            std::process::id(),
            name
        ))
    }

    fn cleanup_path(path: &Path) {
        if path.is_dir() {
            std::fs::remove_dir_all(path).expect("remove pending_retry test dir");
        } else {
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn load_pending_retry_surfaces_storage_read_errors() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let path = test_pending_retry_path("read-error");
        cleanup_path(&path);
        std::fs::create_dir_all(&path).expect("create blocking pending_retry dir");
        let store = StoragePendingRetryStore::new_for_test_path(path.clone());

        let error = store
            .load_pending_retry()
            .expect_err("directory read must not be treated as empty pending retry");

        assert_eq!(error.stage(), "storage_read");
        cleanup_path(&path);
    }
}
