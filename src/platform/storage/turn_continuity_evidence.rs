//! storage-backed compact turn continuity evidence.

use crate::error::{Error, Result};
use crate::memory::{
    TurnContinuityEvidence, TurnContinuityEvidenceStore, REL_PATH_TURN_CONTINUITY_EVIDENCE,
    TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::platform::state_root::state_mount_path;

use super::{read_file, remove_file, write_json_file};

const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const MAX_FLAT_CHAT_ID_FILENAME_LEN: usize = 15;
const EVIDENCE_FILE_EXT: &str = ".json";
const REL_PATH_TURN_CONTINUITY_EVIDENCE_FLAT_STORAGE: &str = "memory/tce";

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnContinuityEvidenceHistory {
    #[serde(default)]
    items: Vec<TurnContinuityEvidence>,
}

fn fnv1a_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn evidence_rel_path(chat_id: &str, flat_storage_namespace: bool) -> Result<PathBuf> {
    if chat_id.is_empty() {
        return Err(Error::config(
            "turn_continuity_evidence_path",
            "chat_id empty",
        ));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(Error::config(
            "turn_continuity_evidence_path",
            "chat_id contains invalid chars",
        ));
    }
    let max_plain_chat_id_len = if flat_storage_namespace {
        MAX_FLAT_CHAT_ID_FILENAME_LEN
    } else {
        MAX_CHAT_ID_FILENAME_LEN
    };
    let filename = if chat_id.len() <= max_plain_chat_id_len {
        format!("{}{}", chat_id, EVIDENCE_FILE_EXT)
    } else {
        format!("{:08x}{}", fnv1a_hash(chat_id), EVIDENCE_FILE_EXT)
    };
    let mut path = PathBuf::new();
    path.push(if flat_storage_namespace {
        REL_PATH_TURN_CONTINUITY_EVIDENCE_FLAT_STORAGE
    } else {
        REL_PATH_TURN_CONTINUITY_EVIDENCE
    });
    path.push(filename);
    if flat_storage_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "turn_continuity_evidence_path",
            format!("storage object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_storage_namespace && path.as_os_str().len() > 64 {
        return Err(Error::config(
            "turn_continuity_evidence_path",
            format!("path too long ({})", path.as_os_str().len()),
        ));
    }
    Ok(path)
}

fn evidence_path(chat_id: &str) -> Result<PathBuf> {
    let mut path = state_mount_path();
    path.push(evidence_rel_path(
        chat_id,
        cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
    )?);
    Ok(path)
}

fn load_history_from_path(path: &Path) -> Result<Vec<TurnContinuityEvidence>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.with_stage("turn_continuity_evidence_read")),
    };
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let stored: StoredTurnContinuityEvidenceHistory = serde_json::from_slice(&buf)
        .map_err(|e| Error::config("turn_continuity_evidence_read", e.to_string()))?;
    Ok(stored.items)
}

/// Storage-backed compact terminal turn evidence store.
pub struct StorageTurnContinuityEvidenceStore;

impl StorageTurnContinuityEvidenceStore {
    /// Create a storage-backed compact evidence store.
    pub fn new() -> Self {
        Self
    }
}

impl Default for StorageTurnContinuityEvidenceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnContinuityEvidenceStore for StorageTurnContinuityEvidenceStore {
    fn append(&self, chat_id: &str, evidence: &TurnContinuityEvidence) -> Result<()> {
        let path = evidence_path(chat_id)?;
        let mut items = load_history_from_path(&path)?;
        items.push(evidence.clone());
        if items.len() > TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS {
            let drain = items.len() - TURN_CONTINUITY_EVIDENCE_HISTORY_MAX_ITEMS;
            items.drain(..drain);
        }
        let json = serde_json::to_vec(&StoredTurnContinuityEvidenceHistory { items })
            .map_err(|e| Error::config("turn_continuity_evidence_write", e.to_string()))?;
        write_json_file(path, &json)
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let path = evidence_path(chat_id)?;
        match remove_file(&path) {
            Ok(()) => Ok(()),
            Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(error) => Err(error.with_stage("turn_continuity_evidence_clear")),
        }
    }

    fn list_recent(&self, chat_id: &str, limit: usize) -> Result<Vec<TurnContinuityEvidence>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut items = load_history_from_path(&evidence_path(chat_id)?)?;
        items.reverse();
        items.truncate(limit);
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::evidence_rel_path;

    #[test]
    fn esp_storage_turn_continuity_evidence_path_stays_within_flat_namespace_limit() {
        let path = evidence_rel_path("chat-id-that-is-longer-than-limit", true).unwrap();
        assert!(path.as_os_str().len() <= 31);
        assert!(path.to_string_lossy().starts_with("memory/tce/"));
    }

    #[test]
    fn esp_storage_turn_continuity_evidence_hashes_medium_relationship_ids() {
        let path = evidence_rel_path("qq_channel:chat-1", true).unwrap();
        assert!(path.as_os_str().len() <= 31);
        assert!(path.to_string_lossy().starts_with("memory/tce/"));
    }
}
