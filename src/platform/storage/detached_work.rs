//! storage-backed detached background work store.
//! 基于 storage 的 detached 后台工作存储。

use crate::agent::{
    DetachedWorkKey, DetachedWorkRecord, DetachedWorkState, DetachedWorkStore,
    DetachedWorkUpsertOutcome, REL_PATH_DETACHED_WORKS,
};
use crate::bus::PcMsg;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_DETACHED_WORK_ITEMS: usize = 96;

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct StoredDetachedWorkMap(HashMap<String, DetachedWorkRecord>);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_DETACHED_WORKS)
}

pub struct StorageDetachedWorkStore {
    store: CachedJsonFileStore<StoredDetachedWorkMap>,
}

impl StorageDetachedWorkStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "detached_work_cache_lock",
                "detached_work_cache",
                "detached_work_persist",
            ),
        }
    }
}

impl Default for StorageDetachedWorkStore {
    fn default() -> Self {
        Self::new()
    }
}

impl DetachedWorkStore for StorageDetachedWorkStore {
    fn get(&self, key: &DetachedWorkKey) -> Result<Option<DetachedWorkRecord>> {
        let storage_key = key.storage_key();
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.0.get(&storage_key).cloned())))
    }

    fn list(&self) -> Result<Vec<DetachedWorkRecord>> {
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.0.values().cloned().collect())))
    }

    fn upsert(
        &self,
        key: &DetachedWorkKey,
        job: &PcMsg,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<DetachedWorkUpsertOutcome> {
        let storage_key = key.storage_key();
        let reason = reason.trim();
        self.store.with_cached_mut(|map| {
            if let Some(current) = map.0.get(&storage_key) {
                if current.job == *job
                    && current.wake_at_ms == wake_at_ms
                    && current.last_reason == reason
                    && current.state == DetachedWorkState::Pending
                {
                    return Ok(StoreOp::clean(DetachedWorkUpsertOutcome {
                        changed: false,
                        record: current.clone(),
                    }));
                }
            }
            if !map.0.contains_key(&storage_key) && map.0.len() >= MAX_DETACHED_WORK_ITEMS {
                if let Some(oldest_key) = map
                    .0
                    .iter()
                    .min_by_key(|(_, record)| record.updated_at_ms)
                    .map(|(key, _)| key.clone())
                {
                    map.0.remove(&oldest_key);
                }
            }
            let updated_at_ms = current_unix_ms();
            let record = match map.0.get(&storage_key) {
                Some(current) => DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: current.revision.saturating_add(1),
                    last_reason: reason.to_string(),
                    updated_at_ms,
                },
                None => DetachedWorkRecord {
                    key: key.clone(),
                    job: job.clone(),
                    state: DetachedWorkState::Pending,
                    wake_at_ms,
                    revision: 1,
                    last_reason: reason.to_string(),
                    updated_at_ms,
                },
            };
            map.0.insert(storage_key, record.clone());
            Ok(StoreOp::dirty(DetachedWorkUpsertOutcome {
                changed: true,
                record,
            }))
        })
    }

    fn mark_queued(&self, key: &DetachedWorkKey, revision: u64) -> Result<bool> {
        let storage_key = key.storage_key();
        self.store.with_cached_mut(|map| {
            let Some(record) = map.0.get_mut(&storage_key) else {
                return Ok(StoreOp::clean(false));
            };
            if record.revision != revision || record.state != DetachedWorkState::Pending {
                return Ok(StoreOp::clean(false));
            }
            record.state = DetachedWorkState::Queued;
            record.updated_at_ms = current_unix_ms();
            Ok(StoreOp::dirty(true))
        })
    }

    fn claim_running(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
    ) -> Result<Option<DetachedWorkRecord>> {
        let storage_key = key.storage_key();
        self.store.with_cached_mut(|map| {
            let Some(record) = map.0.get_mut(&storage_key) else {
                return Ok(StoreOp::clean(None));
            };
            if record.revision != revision
                || !matches!(
                    record.state,
                    DetachedWorkState::Pending | DetachedWorkState::Queued
                )
            {
                return Ok(StoreOp::clean(None));
            }
            record.state = DetachedWorkState::Running;
            record.updated_at_ms = current_unix_ms();
            Ok(StoreOp::dirty(Some(record.clone())))
        })
    }

    fn reschedule(
        &self,
        key: &DetachedWorkKey,
        revision: u64,
        wake_at_ms: u64,
        reason: &str,
    ) -> Result<Option<DetachedWorkRecord>> {
        let storage_key = key.storage_key();
        let reason = reason.trim();
        self.store.with_cached_mut(|map| {
            let Some(record) = map.0.get_mut(&storage_key) else {
                return Ok(StoreOp::clean(None));
            };
            if record.revision != revision {
                return Ok(StoreOp::clean(None));
            }
            record.revision = record.revision.saturating_add(1);
            record.state = DetachedWorkState::Pending;
            record.wake_at_ms = wake_at_ms;
            record.last_reason = reason.to_string();
            record.updated_at_ms = current_unix_ms();
            Ok(StoreOp::dirty(Some(record.clone())))
        })
    }

    fn finish(&self, key: &DetachedWorkKey, revision: u64) -> Result<()> {
        let storage_key = key.storage_key();
        self.store.with_cached_mut(|map| {
            if map
                .0
                .get(&storage_key)
                .is_some_and(|record| record.revision == revision)
            {
                map.0.remove(&storage_key);
                return Ok(StoreOp::dirty(()));
            }
            Ok(StoreOp::clean(()))
        })
    }
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}
