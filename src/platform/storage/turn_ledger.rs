//! storage 实现的最近一轮执行账本存储。按 chat 分文件，避免热路径重写整张 JSON 表。

use crate::error::{Error, Result};
use crate::memory::{
    derive_recent_persona_evidence, RecentPersonaEvidence, TurnLedger, TurnLedgerStore,
    RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK, RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
    REL_PATH_TURN_LEDGERS, REL_PATH_TURN_LEDGER_HISTORY, TURN_LEDGER_HISTORY_MAX_ITEMS,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::platform::state_root::state_mount_path;

use super::{read_file, remove_file, write_json_file};
const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const LEDGER_FILE_EXT: &str = ".json";
const REL_PATH_TURN_LEDGERS_FLAT_STORAGE: &str = "memory/tl";
const REL_PATH_TURN_LEDGER_HISTORY_FLAT_STORAGE: &str = "memory/trh";
const REL_PATH_RECENT_PERSONA_EVIDENCE: &str = "memory/recent_persona_evidence";
const REL_PATH_RECENT_PERSONA_EVIDENCE_FLAT_STORAGE: &str = "memory/rpe";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedger(TurnLedger);

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct StoredTurnLedgerHistory {
    #[serde(default)]
    items: Vec<TurnLedger>,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
struct StoredRecentPersonaEvidence {
    #[serde(default)]
    evidence: RecentPersonaEvidence,
}

fn fnv1a_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn ledger_rel_path(chat_id: &str, flat_storage_namespace: bool) -> Result<PathBuf> {
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
    path.push(if flat_storage_namespace {
        REL_PATH_TURN_LEDGERS_FLAT_STORAGE
    } else {
        REL_PATH_TURN_LEDGERS
    });
    path.push(filename);
    if flat_storage_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "turn_ledger_path",
            format!("storage object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_storage_namespace && path.as_os_str().len() > 64 {
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

fn history_rel_path(chat_id: &str, flat_storage_namespace: bool) -> Result<PathBuf> {
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
    path.push(if flat_storage_namespace {
        REL_PATH_TURN_LEDGER_HISTORY_FLAT_STORAGE
    } else {
        REL_PATH_TURN_LEDGER_HISTORY
    });
    path.push(filename);
    if flat_storage_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "turn_ledger_history_path",
            format!("storage object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_storage_namespace && path.as_os_str().len() > 64 {
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

fn recent_persona_evidence_rel_path(
    chat_id: &str,
    flat_storage_namespace: bool,
) -> Result<PathBuf> {
    if chat_id.is_empty() {
        return Err(Error::config(
            "recent_persona_evidence_path",
            "chat_id empty",
        ));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(Error::config(
            "recent_persona_evidence_path",
            "chat_id contains invalid chars",
        ));
    }
    let filename = if chat_id.len() <= MAX_CHAT_ID_FILENAME_LEN {
        format!("{}{}", chat_id, LEDGER_FILE_EXT)
    } else {
        format!("{:08x}{}", fnv1a_hash(chat_id), LEDGER_FILE_EXT)
    };
    let mut path = PathBuf::new();
    path.push(if flat_storage_namespace {
        REL_PATH_RECENT_PERSONA_EVIDENCE_FLAT_STORAGE
    } else {
        REL_PATH_RECENT_PERSONA_EVIDENCE
    });
    path.push(filename);
    if flat_storage_namespace && path.as_os_str().len() > 31 {
        return Err(Error::config(
            "recent_persona_evidence_path",
            format!("storage object name too long ({})", path.as_os_str().len()),
        ));
    }
    if !flat_storage_namespace && path.as_os_str().len() > 64 {
        return Err(Error::config(
            "recent_persona_evidence_path",
            format!("path too long ({})", path.as_os_str().len()),
        ));
    }
    Ok(path)
}

fn recent_persona_evidence_path(chat_id: &str) -> Result<PathBuf> {
    let mut path = state_mount_path();
    path.push(recent_persona_evidence_rel_path(
        chat_id,
        cfg!(any(target_arch = "xtensa", target_arch = "riscv32")),
    )?);
    Ok(path)
}

fn load_ledger_from_path(path: &Path) -> Result<Option<TurnLedger>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error.with_stage("turn_ledger_read")),
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
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error.with_stage("turn_ledger_history_read")),
    };
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let stored: StoredTurnLedgerHistory = serde_json::from_slice(&buf)
        .map_err(|e| Error::config("turn_ledger_history_read", e.to_string()))?;
    Ok(stored.items)
}

fn load_recent_persona_evidence_from_path(
    path: &Path,
) -> Result<Option<Option<RecentPersonaEvidence>>> {
    let buf = match read_file(path) {
        Ok(buf) => buf,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error.with_stage("recent_persona_evidence_read")),
    };
    if buf.is_empty() {
        return Ok(Some(None));
    }
    let stored: StoredRecentPersonaEvidence = serde_json::from_slice(&buf)
        .map_err(|e| Error::config("recent_persona_evidence_read", e.to_string()))?;
    if stored.evidence.is_meaningful() {
        Ok(Some(Some(stored.evidence)))
    } else {
        Ok(Some(None))
    }
}

pub struct StorageTurnLedgerStore;

impl StorageTurnLedgerStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StorageTurnLedgerStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnLedgerStore for StorageTurnLedgerStore {
    fn get(&self, chat_id: &str) -> Result<Option<TurnLedger>> {
        let path = ledger_path(chat_id)?;
        load_ledger_from_path(&path)
    }

    fn set(&self, chat_id: &str, ledger: &TurnLedger) -> Result<()> {
        let path = ledger_path(chat_id)?;
        let json = serde_json::to_vec(&StoredTurnLedger(ledger.clone()))
            .map_err(|e| Error::config("turn_ledger_persist", e.to_string()))?;
        write_json_file(path, &json)?;
        if ledger.status.is_terminal() {
            self.append_history(chat_id, ledger)?;
        }
        Ok(())
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let path = ledger_path(chat_id)?;
        ignore_missing_remove(&path)?;
        let history_path = history_path(chat_id)?;
        ignore_missing_remove(&history_path)?;
        let evidence_path = recent_persona_evidence_path(chat_id)?;
        ignore_missing_remove(&evidence_path)?;
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

    fn recent_persona_evidence(&self, chat_id: &str) -> Result<Option<RecentPersonaEvidence>> {
        let path = recent_persona_evidence_path(chat_id)?;
        if let Some(evidence) = load_recent_persona_evidence_from_path(&path)? {
            return Ok(evidence);
        }
        if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            return Ok(None);
        }
        let ledgers = self.list_recent(chat_id, RECENT_PERSONA_EVIDENCE_HISTORY_LOOKBACK)?;
        Ok(derive_recent_persona_evidence(
            &ledgers,
            RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS,
        ))
    }
}

impl StorageTurnLedgerStore {
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
        let evidence =
            derive_recent_persona_evidence(&items, RECENT_PERSONA_EVIDENCE_MEANINGFUL_TURNS);
        let json = serde_json::to_vec(&StoredTurnLedgerHistory { items })
            .map_err(|e| Error::config("turn_ledger_history_write", e.to_string()))?;
        write_json_file(path, &json)?;
        self.write_recent_persona_evidence(chat_id, evidence)
    }

    fn write_recent_persona_evidence(
        &self,
        chat_id: &str,
        evidence: Option<RecentPersonaEvidence>,
    ) -> Result<()> {
        let path = recent_persona_evidence_path(chat_id)?;
        let evidence = evidence.unwrap_or_default();
        let json = serde_json::to_vec(&StoredRecentPersonaEvidence { evidence })
            .map_err(|e| Error::config("recent_persona_evidence_write", e.to_string()))?;
        write_json_file(path, &json)
    }
}

fn ignore_missing_remove(path: &Path) -> Result<()> {
    match remove_file(path) {
        Ok(()) => Ok(()),
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        history_path, history_rel_path, ledger_path, ledger_rel_path, recent_persona_evidence_path,
        LEDGER_FILE_EXT,
    };
    use crate::bus::IngressKind;
    use crate::memory::{
        MentalPrivacyShareAction, TurnLedger, TurnLedgerStatus, TurnLedgerStore,
        TurnPersonaDisclosureLedger, TurnPersonaLedger, TurnPersonaPressureLevel,
        TurnPersonaPriorityLedger,
    };
    use crate::platform::storage::state_path_join;
    fn meaningful_persona_ledger() -> TurnLedger {
        TurnLedger {
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            finished_at_ms: 2_000,
            persona: Some(TurnPersonaLedger {
                disclosure: Some(TurnPersonaDisclosureLedger {
                    request_kind: "boundary_touch".to_string(),
                    share_action: MentalPrivacyShareAction::ExplainWithoutQuote,
                    acknowledge_boundary: true,
                    targets: vec!["self_model".to_string()],
                    response_mode: "relational_explanation".to_string(),
                    response_guidance: "hold boundary".to_string(),
                }),
                priority: Some(TurnPersonaPriorityLedger {
                    stance_summary: "hold self first".to_string(),
                    priority_order: vec![
                        "self_authored_core".to_string(),
                        "boundary".to_string(),
                        "user_contract".to_string(),
                    ],
                    response_mode: "protective_brief".to_string(),
                    task_scope: "brief".to_string(),
                    initiative_posture: "hold".to_string(),
                    relationship_posture: "guarded_warm".to_string(),
                    resource_posture: "steady".to_string(),
                    response_guidance: "stay compact".to_string(),
                }),
                review: Default::default(),
                touched_targets: vec!["self_model".to_string()],
                pressure: TurnPersonaPressureLevel::Normal,
                tool_calls: 0,
                reply_scope: "brief".to_string(),
                reply_delivered: true,
            }),
            ..TurnLedger::default()
        }
    }

    #[test]
    fn esp_storage_turn_ledger_path_stays_within_flat_namespace_limit() {
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
    fn esp_storage_turn_ledger_history_path_stays_within_flat_namespace_limit() {
        let path = history_rel_path("c2c:947B12A11A2E0348FAA2C60499D29345", true).unwrap();
        assert!(path.as_os_str().len() <= 31);
        assert!(path.to_string_lossy().starts_with("memory/trh/"));
        assert!(path.to_string_lossy().ends_with(LEDGER_FILE_EXT));
    }

    #[test]
    fn recent_persona_evidence_reads_sidecar_without_history_scan() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "recent-persona-sidecar-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        store.set(&chat_id, &meaningful_persona_ledger()).unwrap();

        let history = history_path(&chat_id).unwrap();
        let evidence = recent_persona_evidence_path(&chat_id).unwrap();
        assert!(history.exists());
        assert!(evidence.exists());

        super::remove_file(&history).unwrap();

        let loaded = store.recent_persona_evidence(&chat_id).unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().meaningful_turns, 1);

        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn recent_persona_evidence_empty_sidecar_blocks_history_fallback() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "recent-persona-empty-sidecar-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        store.set(&chat_id, &meaningful_persona_ledger()).unwrap();

        let history = history_path(&chat_id).unwrap();
        let evidence = recent_persona_evidence_path(&chat_id).unwrap();
        assert!(history.exists());
        assert!(evidence.exists());
        crate::platform::storage::write_json_file(&evidence, br#"{"evidence":{}}"#).unwrap();

        let loaded = store.recent_persona_evidence(&chat_id).unwrap();
        assert!(loaded.is_none());

        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn terminal_turn_without_persona_materializes_empty_recent_persona_sidecar() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "recent-persona-empty-write-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        let ledger = TurnLedger {
            ingress: IngressKind::User,
            status: TurnLedgerStatus::Answered,
            started_at_ms: 1_000,
            updated_at_ms: 2_000,
            finished_at_ms: 2_000,
            ..TurnLedger::default()
        };

        store.set(&chat_id, &ledger).unwrap();

        let evidence = recent_persona_evidence_path(&chat_id).unwrap();
        assert!(evidence.exists());
        let loaded = store.recent_persona_evidence(&chat_id).unwrap();
        assert!(loaded.is_none());

        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn get_ignores_legacy_aggregate_file() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "legacy-turn-ledger-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();

        let legacy_path = state_path_join("memory/turn_ledgers.json");
        let legacy_bytes = format!(r#"{{"{chat_id}":{{}}}}"#).into_bytes();
        crate::platform::storage::write_json_file(&legacy_path, &legacy_bytes).unwrap();

        let loaded = store.get(&chat_id).unwrap();
        assert!(loaded.is_none());

        let _ = super::remove_file(&legacy_path);
        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn get_reports_unreadable_ledger_instead_of_silent_none() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "turn-ledger-unreadable-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        let path = ledger_path(&chat_id).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();

        let err = store
            .get(&chat_id)
            .expect_err("directory-backed ledger must fail");
        assert_eq!(err.stage(), "turn_ledger_read");

        let _ = std::fs::remove_dir_all(&path);
        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn list_recent_reports_unreadable_history_instead_of_empty() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "turn-history-unreadable-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        let path = history_path(&chat_id).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();

        let err = store
            .list_recent(&chat_id, 4)
            .expect_err("directory-backed history must fail");
        assert_eq!(err.stage(), "turn_ledger_history_read");

        let _ = std::fs::remove_dir_all(&path);
        store.clear(&chat_id).unwrap();
    }

    #[test]
    fn recent_persona_evidence_reports_unreadable_sidecar_instead_of_falling_back() {
        let store = super::StorageTurnLedgerStore::new();
        let chat_id = format!(
            "recent-persona-unreadable-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        store.clear(&chat_id).unwrap();
        let path = recent_persona_evidence_path(&chat_id).unwrap();
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();

        let err = store
            .recent_persona_evidence(&chat_id)
            .expect_err("directory-backed evidence sidecar must fail");
        assert_eq!(err.stage(), "recent_persona_evidence_read");

        let _ = std::fs::remove_dir_all(&path);
        store.clear(&chat_id).unwrap();
    }
}
