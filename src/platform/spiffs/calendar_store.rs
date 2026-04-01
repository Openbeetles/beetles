//! SPIFFS / state-root backed calendar store.

use crate::calendar::{
    filter_calendar_events, normalize_calendar_event, CalendarEvent, CalendarQuery, CalendarStore,
    REL_PATH_CALENDAR_EVENTS,
};
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use super::{read_file, state_path_join, write_file};

const MAX_CALENDAR_EVENTS: usize = 256;

#[derive(Clone, Serialize, Deserialize)]
struct StoredCalendarEvent(CalendarEvent);

fn full_path() -> PathBuf {
    state_path_join(REL_PATH_CALENDAR_EVENTS)
}

pub struct SpiffsCalendarStore {
    cache: Mutex<Option<HashMap<String, StoredCalendarEvent>>>,
}

impl SpiffsCalendarStore {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(None),
        }
    }

    fn load_map_from_disk() -> HashMap<String, StoredCalendarEvent> {
        match read_file(full_path()) {
            Ok(buf) if buf.len() > 2 => serde_json::from_slice(&buf).unwrap_or_default(),
            _ => HashMap::new(),
        }
    }

    fn with_map_mut<R>(
        &self,
        f: impl FnOnce(&mut HashMap<String, StoredCalendarEvent>) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self
            .cache
            .lock()
            .map_err(|e| Error::config("calendar_store_cache_lock", e.to_string()))?;
        if guard.is_none() {
            *guard = Some(Self::load_map_from_disk());
        }
        let map = guard
            .as_mut()
            .ok_or_else(|| Error::config("calendar_store_cache", "cache not initialized"))?;
        f(map)
    }

    fn persist(map: &HashMap<String, StoredCalendarEvent>) -> Result<()> {
        let json = serde_json::to_vec(map)
            .map_err(|e| Error::config("calendar_store_set", e.to_string()))?;
        write_file(full_path(), &json)
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

impl Default for SpiffsCalendarStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CalendarStore for SpiffsCalendarStore {
    fn list(&self, query: CalendarQuery) -> Result<Vec<CalendarEvent>> {
        self.with_map_mut(|map| {
            let events = map.values().map(|entry| entry.0.clone()).collect();
            Ok(filter_calendar_events(events, query))
        })
    }

    fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
        self.with_map_mut(|map| Ok(map.get(id).map(|entry| entry.0.clone())))
    }

    fn upsert(&self, event: &CalendarEvent) -> Result<()> {
        self.with_map_mut(|map| {
            let normalized = normalize_calendar_event(event.clone())?;
            map.insert(normalized.id.clone(), StoredCalendarEvent(normalized));
            Self::trim_if_needed(map);
            Self::persist(map)
        })
    }

    fn delete(&self, id: &str) -> Result<bool> {
        self.with_map_mut(|map| {
            let removed = map.remove(id).is_some();
            if removed {
                Self::persist(map)?;
            }
            Ok(removed)
        })
    }
}
