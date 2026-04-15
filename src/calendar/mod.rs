//! 共享日历领域层：事件模型、查询与存储抽象。
//! Shared calendar domain: event model, query helpers, and store abstraction.

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod credentials;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod provider;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub mod providers;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod service;

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use credentials::{
    CalendarProviderCredential, CalendarProviderCredentialStatus, CalendarProviderCredentialStore,
    OfficeBackedCalendarProviderCredentialStore, FEISHU_CALENDAR_DEFAULT_BASE_URL,
    OFFICE_METADATA_CALENDAR_APP_ID,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use provider::{
    CalendarHttpClient, CalendarOperation, CalendarProvider, CalendarProviderRegistry,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use service::CalendarService;

pub const REL_PATH_CALENDAR_EVENTS: &str = "memory/calendar_events.json";
pub const CALENDAR_PROVIDER_LOCAL: &str = "local";
pub const MAX_CALENDAR_TITLE_CHARS: usize = 120;
pub const MAX_CALENDAR_TEXT_CHARS: usize = 512;
pub const MAX_CALENDAR_PROVIDER_CHARS: usize = 32;
pub const MAX_CALENDAR_CALENDAR_ID_CHARS: usize = 64;
pub const MAX_CALENDAR_REMOTE_ID_CHARS: usize = 128;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CalendarEventStatus {
    #[default]
    Confirmed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: String,
    pub title: String,
    pub start_at_unix_secs: u64,
    pub end_at_unix_secs: u64,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub location: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "default_calendar_provider")]
    pub provider: String,
    #[serde(default)]
    pub calendar_id: String,
    #[serde(default)]
    pub remote_id: String,
    #[serde(default)]
    pub status: CalendarEventStatus,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarQuery {
    pub start_from_unix_secs: Option<u64>,
    pub start_to_unix_secs: Option<u64>,
    pub limit: usize,
    pub include_cancelled: bool,
}

impl CalendarQuery {
    pub fn upcoming(now_secs: u64, limit: usize) -> Self {
        Self {
            start_from_unix_secs: Some(now_secs),
            start_to_unix_secs: None,
            limit,
            include_cancelled: false,
        }
    }
}

pub trait CalendarStore: Send + Sync {
    fn list(&self, query: CalendarQuery) -> Result<Vec<CalendarEvent>>;
    fn get(&self, id: &str) -> Result<Option<CalendarEvent>>;
    fn upsert(&self, event: &CalendarEvent) -> Result<()>;
    fn delete(&self, id: &str) -> Result<bool>;
}

pub fn normalize_calendar_event(mut event: CalendarEvent) -> Result<CalendarEvent> {
    event.id = normalize_field(&event.id, MAX_CALENDAR_REMOTE_ID_CHARS);
    if event.id.is_empty() {
        return Err(Error::config("calendar_event", "id must not be empty"));
    }
    event.title = normalize_field(&event.title, MAX_CALENDAR_TITLE_CHARS);
    if event.title.is_empty() {
        return Err(Error::config("calendar_event", "title must not be empty"));
    }
    if event.start_at_unix_secs == 0 {
        return Err(Error::config(
            "calendar_event",
            "start_at_unix_secs must be > 0",
        ));
    }
    if event.end_at_unix_secs <= event.start_at_unix_secs {
        return Err(Error::config(
            "calendar_event",
            "end_at_unix_secs must be greater than start_at_unix_secs",
        ));
    }
    event.timezone = normalize_field(&event.timezone, MAX_CALENDAR_TEXT_CHARS);
    event.location = normalize_field(&event.location, MAX_CALENDAR_TEXT_CHARS);
    event.notes = normalize_field(&event.notes, MAX_CALENDAR_TEXT_CHARS);
    event.provider = normalize_field(&event.provider, MAX_CALENDAR_PROVIDER_CHARS);
    if event.provider.is_empty() {
        event.provider = CALENDAR_PROVIDER_LOCAL.to_string();
    }
    event.calendar_id = normalize_field(&event.calendar_id, MAX_CALENDAR_CALENDAR_ID_CHARS);
    event.remote_id = normalize_field(&event.remote_id, MAX_CALENDAR_REMOTE_ID_CHARS);
    Ok(event)
}

pub fn filter_calendar_events(
    mut events: Vec<CalendarEvent>,
    query: CalendarQuery,
) -> Vec<CalendarEvent> {
    let limit = query.limit.clamp(1, 100);
    events.retain(|event| {
        if !query.include_cancelled && event.status == CalendarEventStatus::Cancelled {
            return false;
        }
        if let Some(start_from) = query.start_from_unix_secs {
            if event.start_at_unix_secs < start_from {
                return false;
            }
        }
        if let Some(start_to) = query.start_to_unix_secs {
            if event.start_at_unix_secs > start_to {
                return false;
            }
        }
        true
    });
    events.sort_by(|left, right| {
        left.start_at_unix_secs
            .cmp(&right.start_at_unix_secs)
            .then_with(|| left.updated_at.cmp(&right.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    if events.len() > limit {
        events.truncate(limit);
    }
    events
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn default_calendar_provider() -> String {
    CALENDAR_PROVIDER_LOCAL.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_calendar_event_rejects_invalid_range() {
        let err = normalize_calendar_event(CalendarEvent {
            id: "event-1".to_string(),
            title: "发布".to_string(),
            start_at_unix_secs: 100,
            end_at_unix_secs: 100,
            timezone: String::new(),
            location: String::new(),
            notes: String::new(),
            provider: CALENDAR_PROVIDER_LOCAL.to_string(),
            calendar_id: String::new(),
            remote_id: String::new(),
            status: CalendarEventStatus::Confirmed,
            updated_at: 1,
        })
        .unwrap_err();
        assert_eq!(err.stage(), "calendar_event");
    }

    #[test]
    fn normalize_calendar_event_trims_and_defaults_provider() {
        let event = normalize_calendar_event(CalendarEvent {
            id: "  event-1  ".to_string(),
            title: "  跟进周会  ".to_string(),
            start_at_unix_secs: 100,
            end_at_unix_secs: 160,
            timezone: "  Asia/Shanghai  ".to_string(),
            location: "  会议室 A  ".to_string(),
            notes: "  带上路线图  ".to_string(),
            provider: String::new(),
            calendar_id: "  default ".to_string(),
            remote_id: " remote-1 ".to_string(),
            status: CalendarEventStatus::Confirmed,
            updated_at: 1,
        })
        .unwrap();
        assert_eq!(event.id, "event-1");
        assert_eq!(event.title, "跟进周会");
        assert_eq!(event.provider, CALENDAR_PROVIDER_LOCAL);
        assert_eq!(event.calendar_id, "default");
    }

    #[test]
    fn filter_calendar_events_respects_range_and_cancelled() {
        let items = vec![
            CalendarEvent {
                id: "b".to_string(),
                title: "第二个".to_string(),
                start_at_unix_secs: 200,
                end_at_unix_secs: 260,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: CALENDAR_PROVIDER_LOCAL.to_string(),
                calendar_id: String::new(),
                remote_id: String::new(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 2,
            },
            CalendarEvent {
                id: "a".to_string(),
                title: "第一个".to_string(),
                start_at_unix_secs: 100,
                end_at_unix_secs: 160,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: CALENDAR_PROVIDER_LOCAL.to_string(),
                calendar_id: String::new(),
                remote_id: String::new(),
                status: CalendarEventStatus::Cancelled,
                updated_at: 1,
            },
            CalendarEvent {
                id: "c".to_string(),
                title: "第三个".to_string(),
                start_at_unix_secs: 300,
                end_at_unix_secs: 360,
                timezone: String::new(),
                location: String::new(),
                notes: String::new(),
                provider: CALENDAR_PROVIDER_LOCAL.to_string(),
                calendar_id: String::new(),
                remote_id: String::new(),
                status: CalendarEventStatus::Confirmed,
                updated_at: 3,
            },
        ];
        let filtered = filter_calendar_events(
            items,
            CalendarQuery {
                start_from_unix_secs: Some(150),
                start_to_unix_secs: Some(320),
                limit: 10,
                include_cancelled: false,
            },
        );
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, "b");
        assert_eq!(filtered[1].id, "c");
    }
}
