//! storage / state-root backed calendar store.

use crate::calendar::{
    filter_calendar_events, normalize_calendar_event, CalendarEvent, CalendarQuery, CalendarStore,
    REL_PATH_CALENDAR_EVENTS,
};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::cached_json::{load_json_or_default, CachedJsonFileStore, StoreOp};
use super::state_path_join;

const MAX_CALENDAR_EVENTS: usize = 256;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StoredCalendarEvent(CalendarEvent);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_CALENDAR_EVENTS)
}

pub struct StorageCalendarStore {
    store: CachedJsonFileStore<HashMap<String, StoredCalendarEvent>>,
}

impl StorageCalendarStore {
    pub fn new() -> Self {
        Self {
            store: CachedJsonFileStore::new(
                full_path,
                load_json_or_default,
                "calendar_store_cache_lock",
                "calendar_store_cache",
                "calendar_store_persist",
            ),
        }
    }

    fn trim_if_needed(map: &mut HashMap<String, StoredCalendarEvent>) {
        while map.len() > MAX_CALENDAR_EVENTS {
            let oldest_id = map
                .iter()
                .min_by_key(|(_, entry)| entry.0.updated_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest_id {
                map.remove(&id);
            } else {
                break;
            }
        }
    }
}

impl Default for StorageCalendarStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CalendarStore for StorageCalendarStore {
    fn list(&self, query: CalendarQuery) -> Result<Vec<CalendarEvent>> {
        self.store.with_cached_mut(|map| {
            let events = map.values().map(|entry| entry.0.clone()).collect();
            Ok(StoreOp::clean(filter_calendar_events(events, query)))
        })
    }

    fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
        self.store
            .with_cached_mut(|map| Ok(StoreOp::clean(map.get(id).map(|entry| entry.0.clone()))))
    }

    fn upsert(&self, event: &CalendarEvent) -> Result<()> {
        self.store.with_cached_mut(|map| {
            let normalized = normalize_calendar_event(event.clone())?;
            let next_event = StoredCalendarEvent(normalized);
            if map.get(&next_event.0.id) == Some(&next_event) {
                return Ok(StoreOp::clean(()));
            }
            map.insert(next_event.0.id.clone(), next_event);
            Self::trim_if_needed(map);
            Ok(StoreOp::dirty(()))
        })
    }

    fn delete(&self, id: &str) -> Result<bool> {
        self.store.with_cached_mut(|map| {
            let removed = map.remove(id).is_some();
            Ok(StoreOp::with_dirty(removed, removed))
        })
    }
}
