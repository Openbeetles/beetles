//! SPIFFS 实现的 PendingRetryStore。单文件 memory/pending_retry.json，存 { msg, replay_count }；
//! replay_count 达上限后不再注入，避免重复饥饿。

use crate::bus::{PcMsg, MAX_CONTENT_LEN};
use crate::constants::PENDING_RETRY_MAX_REPLAY;
use crate::error::{Error, Result};
use crate::memory::{PendingRetryStore, REL_PATH_PENDING_RETRY};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::{read_file, state_path_join, write_file};

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_PENDING_RETRY)
}

#[derive(Serialize, Deserialize)]
struct PendingRetryEntry {
    msg: PcMsg,
    #[serde(default)]
    replay_count: u32,
}

/// 无状态；路径固定为 SPIFFS_BASE + REL_PATH_PENDING_RETRY。
pub struct SpiffsPendingRetryStore;

impl SpiffsPendingRetryStore {
    pub fn new() -> Self {
        SpiffsPendingRetryStore
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
        let path = full_path();
        let replay_count = match read_file(&path) {
            Ok(buf) if buf.len() > 2 => serde_json::from_slice::<PendingRetryEntry>(&buf)
                .map(|e| {
                    e.replay_count
                        .saturating_add(1)
                        .min(PENDING_RETRY_MAX_REPLAY)
                })
                .unwrap_or(1),
            _ => 1,
        };
        let entry = PendingRetryEntry {
            msg: msg.clone(),
            replay_count,
        };
        let json = serde_json::to_vec(&entry)
            .map_err(|e| Error::config("pending_retry_save", e.to_string()))?;
        write_file(full_path(), &json)
    }

    fn load_pending_retry(&self) -> Result<Option<PcMsg>> {
        let path = full_path();
        let buf = match read_file(&path) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        if buf.len() <= 2 {
            return Ok(None);
        }
        let entry: PendingRetryEntry = match serde_json::from_slice(&buf) {
            Ok(e) => e,
            Err(_) => {
                if let Ok(m) = serde_json::from_slice::<PcMsg>(&buf) {
                    return Ok(Some(m));
                }
                log::warn!("[spiffs_pending_retry] load parse failed");
                return Ok(None);
            }
        };
        if entry.replay_count >= PENDING_RETRY_MAX_REPLAY {
            let _ = write_file(&path, b"{}");
            log::info!(
                "[spiffs_pending_retry] replay_count {} >= {}, cleared",
                entry.replay_count,
                PENDING_RETRY_MAX_REPLAY
            );
            return Ok(None);
        }
        Ok(Some(entry.msg))
    }

    fn clear_pending_retry(&self) -> Result<()> {
        write_file(full_path(), b"{}")
    }
}
