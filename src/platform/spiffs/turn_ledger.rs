//! SPIFFS 实现的最近一轮执行账本存储。按 chat 分文件，避免热路径重写整张 JSON 表。

use crate::error::{Error, Result};
use crate::memory::{
    TurnLedger, TurnLedgerStore, REL_PATH_TURN_LEDGERS, REL_PATH_TURN_LEDGERS_LEGACY,
    REL_PATH_TURN_LEDGER_HISTORY, TURN_LEDGER_HISTORY_MAX_ITEMS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::platform::state_root::state_mount_path;

use super::{read_file, remove_file, state_path_join, write_file};

const TAG: &str = "platform::spiffs::turn_ledger";
const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const LEDGER_FILE_EXT: &str = ".json";
const REL_PATH_TURN_LEDGERS_FLAT_SPIFFS: &str = "memory/tl";
const REL_PATH_TURN_LEDGER_HISTORY_FLAT_SPIFFS: &str = "memory/trh";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedger(TurnLedger);

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedgerHistory {
    #[serde(default)]
    items: Vec<TurnLedger>,
}

fn fnv1a_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn legacy_full_path() -> PathBuf {
    state_path_join(REL_PATH_TURN_LEDGERS_LEGACY)
}

fn ledger_rel_path(chat_id: &str, flat_spiffs_namespace: bool) -> Result<PathBuf> {
    if chat_id.is_empty() {
        return Err(Error::config("turn_ledger_path", "chat_id empty"));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(Error::config(
            "turn_ledger_path",
            "chat_id contains invalid chars",
        ));
    }
    let filename = if chat_id.len() <= MAX_CHAT_ID_FILENAME_LEN {
        format!("{}{}", chat_id, LEDGER_FILE_EXT)
    } else {
        format!("{:08x}{}", fnv1a_hash(chat_id), LEDGER_FILE_EXT)
    };
    let mut path = PathBuf::new();
    path.push(if flat_spiffs_namespace {
        REL_PATH_TURN_LEDGERS_FLAT_SPIFFS
    } else {
        REL_PATH_TURN_LEDGERS
    });
    path.push(filename);
    if flat_spiffs_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "turn_ledger_path",
            format!("spiffs object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_spiffs_namespace && path.as_os_str().len() > 64 {
        return Err(Error::config(
            "turn_ledger_path",
            format!("path too long ({})", path.as_os_str().len()),
        ));
    }
    Ok(path)
}

fn ledger_path(chat_id: &str) -> Result<PathBuf> {
    let mut path = state_mount_path();
    path.push(ledger_rel_path(
        chat_id,
        cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
    )?);
    Ok(path)
}

fn history_rel_path(chat_id: &str, flat_spiffs_namespace: bool) -> Result<PathBuf> {
    if chat_id.is_empty() {
        return Err(Error::config("turn_ledger_history_path", "chat_id empty"));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(Error::config(
            "turn_ledger_history_path",
            "chat_id contains invalid chars",
        ));
    }
    let filename = if chat_id.len() <= MAX_CHAT_ID_FILENAME_LEN {
        format!("{}{}", chat_id, LEDGER_FILE_EXT)
    } else {
        format!("{:08x}{}", fnv1a_hash(chat_id), LEDGER_FILE_EXT)
    };
    let mut path = PathBuf::new();
    path.push(if flat_spiffs_namespace {
        REL_PATH_TURN_LEDGER_HISTORY_FLAT_SPIFFS
    } else {
        REL_PATH_TURN_LEDGER_HISTORY
    });
    path.push(filename);
    if flat_spiffs_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "turn_ledger_history_path",
            format!("spiffs object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_spiffs_namespace && path.as_os_str().len() > 64 {
        return Err(Error::config(
            "turn_ledger_history_path",
            format!("path too long ({})", path.as_os_str().len()),
        ));
    }
    Ok(path)
}

fn history_path(chat_id: &str) -> Result<PathBuf> {
    let mut path = state_mount_path();
    path.push(history_rel_path(
        chat_id,
        cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
    )?);
    Ok(path)
}

fn load_ledger_from_path(path: &Path) -> Result<Option<TurnLedger>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { .. }) | Err(Error::Other { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    if buf.is_empty() {
        return Ok(None);
    }
    let stored: StoredTurnLedger = serde_json::from_slice(&buf)
        .map_err(|e| Error::config("turn_ledger_read", e.to_string()))?;
    Ok(Some(stored.0))
}

fn load_history_from_path(path: &Path) -> Result<Vec<TurnLedger>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { .. }) | Err(Error::Other { .. }) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let stored: StoredTurnLedgerHistory = serde_json::from_slice(&buf)
        .map_err(|e| Error::config("turn_ledger_history_read", e.to_string()))?;
    Ok(stored.items)
}

fn load_legacy_map(path: &Path) -> Result<HashMap<String, StoredTurnLedger>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { .. }) | Err(Error::Other { .. }) => return Ok(HashMap::new()),
        Err(error) => return Err(error),
    };
    if buf.len() <= 2 {
        return Ok(HashMap::new());
    }
    serde_json::from_slice(&buf).map_err(|e| Error::config("turn_ledger_legacy", e.to_string()))
}

pub struct SpiffsTurnLedgerStore {
    legacy_cache: Mutex<Option<HashMap<String, StoredTurnLedger>>>,
}

impl SpiffsTurnLedgerStore {
    pub fn new() -> Self {
        Self {
            legacy_cache: Mutex::new(None),
        }
    }

    fn load_legacy_cached(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        let mut cache = self.legacy_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.is_none() {
            let path = legacy_full_path();
            let loaded = load_legacy_map(&path)?;
            if !loaded.is_empty() {
                log::info!(
                    "[{}] loaded legacy turn ledger map for compatibility (entries={})",
                    TAG,
                    loaded.len()
                );
            }
            *cache = Some(loaded);
        }
        Ok(cache
            .as_ref()
            .and_then(|map| map.get(chat_id))
            .map(|stored| stored.0.clone()))
    }
}

impl Default for SpiffsTurnLedgerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnLedgerStore for SpiffsTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        let path = ledger_path(chat_id)?;
        if let Some(ledger) = load_ledger_from_path(&path)? {
            return Ok(Some(ledger));
        }
        self.load_legacy_cached(chat_id)
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        let path = ledger_path(chat_id)?;
        let json = serde_json::to_vec(&StoredTurnLedger(ledger.clone()))
            .map_err(|e| Error::config("turn_ledger_persist", e.to_string()))?;
        write_file(path, &json)?;
        if ledger.status.is_terminal() {
            self.append_history(chat_id, ledger)?;
        }
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let path = ledger_path(chat_id)?;
        if path.exists() {
            remove_file(&path)?;
        }
        let history_path = history_path(chat_id)?;
        if history_path.exists() {
            remove_file(&history_path)?;
        }
        if let Some(cache) = self
            .legacy_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            cache.remove(chat_id);
        }
        Ok(())
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnLedger>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut items = load_history_from_path(&history_path(chat_id)?)?;
        if items.is_empty() {
            if let Some(latest) = self.get(chat_id)? {
                if latest.status.is_terminal() {
                    return Ok(vec![latest]);
                }
            }
            return Ok(Vec::new());
        }
        items.reverse();
        items.truncate(limit);
        Ok(items)
    }
}

impl SpiffsTurnLedgerStore {
    fn append_history(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        let path = history_path(chat_id)?;
        let mut items = load_history_from_path(&path)?;
        if let Some(existing) = items.iter_mut().find(|existing| {
            (!ledger.req_id.trim().is_empty() && existing.req_id == ledger.req_id)
                || (ledger.started_at_ms > 0 && existing.started_at_ms == ledger.started_at_ms)
        }) {
            *existing = ledger.clone();
        } else {
            items.push(ledger.clone());
        }
        if items.len() > TURN_LEDGER_HISTORY_MAX_ITEMS {
            let drain = items.len() - TURN_LEDGER_HISTORY_MAX_ITEMS;
            items.drain(..drain);
        }
        let json = serde_json::to_vec(&StoredTurnLedgerHistory { items })
            .map_err(|e| Error::config("turn_ledger_history_write", e.to_string()))?;
        write_file(path, &json)
    }
}

#[cfg(test)]
mod tests {
    use super::{history_rel_path, ledger_rel_path, LEDGER_FILE_EXT};

    #[test]
    fn esp_spiffs_turn_ledger_path_stays_within_flat_namespace_limit() {
        let path = ledger_rel_path("c2c:947B12A11A2E0348FAA2C60499D29345", true).unwrap();
        assert!(path.as_os_str().len() <= 31);
        assert!(path.to_string_lossy().starts_with("memory/tl/"));
        assert!(path.to_string_lossy().ends_with(LEDGER_FILE_EXT));
    }

    #[test]
    fn host_turn_ledger_path_keeps_original_tree_shape() {
        let path = ledger_rel_path("c2c:947B12A11A2E0348FAA2C60499D29345", false).unwrap();
        assert!(path.to_string_lossy().starts_with("memory/turn_ledgers/"));
        assert!(path.to_string_lossy().ends_with(LEDGER_FILE_EXT));
    }

    #[test]
    fn esp_spiffs_turn_ledger_history_path_stays_within_flat_namespace_limit() {
        let path = history_rel_path("c2c:947B12A11A2E0348FAA2C60499D29345", true).unwrap();
        assert!(path.as_os_str().len() <= 31);
        assert!(path.to_string_lossy().starts_with("memory/trh/"));
        assert!(path.to_string_lossy().ends_with(LEDGER_FILE_EXT));
    }
}
