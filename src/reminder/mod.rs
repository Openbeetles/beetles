//! 共享提醒领域层：持久提醒模型与规范化。
//! Shared reminder domain: durable reminder model and normalization.

use crate::constants::REMIND_AT_MAX_CONTEXT_LEN;
use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub const REL_PATH_REMINDERS: &str = "memory/remind_at.json";
pub const MAX_REMINDER_ID_CHARS: usize = 128;
pub const MAX_REMINDER_CHANNEL_CHARS: usize = 32;
pub const MAX_REMINDER_CHAT_ID_CHARS: usize = 160;
pub const MAX_REMINDER_PROVIDER_CHARS: usize = 32;
pub const MAX_REMINDER_ACCOUNT_KEY_CHARS: usize = 128;
pub const MAX_REMINDER_TARGET_ID_CHARS: usize = 64;
pub const MAX_REMINDER_REMOTE_ID_CHARS: usize = 128;
pub const DEFAULT_REMINDER_CALENDAR_DURATION_SECS: u64 = 15 * 60;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReminderItem {
    pub id: String,
    pub channel: String,
    pub chat_id: String,
    pub at_unix_secs: u64,
    pub context: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calendar_event_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calendar_provider: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calendar_account_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calendar_calendar_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub calendar_remote_id: String,
    #[serde(default)]
    pub calendar_start_at_unix_secs: u64,
    #[serde(default)]
    pub calendar_end_at_unix_secs: u64,
    #[serde(default)]
    pub calendar_timezone: String,
    #[serde(default)]
    pub calendar_location: String,
    #[serde(default)]
    pub calendar_notes: String,
    #[serde(default)]
    pub updated_at: u64,
}

pub fn normalize_reminder_item(mut item: ReminderItem) -> Result<ReminderItem> {
    item.id = normalize_field(&item.id, MAX_REMINDER_ID_CHARS);
    if item.id.is_empty() {
        return Err(Error::config("reminder_item", "id must not be empty"));
    }
    item.channel = normalize_field(&item.channel, MAX_REMINDER_CHANNEL_CHARS);
    if item.channel.is_empty() {
        return Err(Error::config("reminder_item", "channel must not be empty"));
    }
    item.chat_id = normalize_field(&item.chat_id, MAX_REMINDER_CHAT_ID_CHARS);
    if item.chat_id.is_empty() {
        return Err(Error::config("reminder_item", "chat_id must not be empty"));
    }
    if item.at_unix_secs == 0 {
        return Err(Error::config("reminder_item", "at_unix_secs must be > 0"));
    }
    item.context = normalize_field(&item.context, REMIND_AT_MAX_CONTEXT_LEN);
    if item.context.is_empty() {
        return Err(Error::config("reminder_item", "context must not be empty"));
    }
    item.calendar_event_id = normalize_field(&item.calendar_event_id, MAX_REMINDER_ID_CHARS);
    item.calendar_provider = normalize_field(&item.calendar_provider, MAX_REMINDER_PROVIDER_CHARS);
    item.calendar_account_key =
        normalize_field(&item.calendar_account_key, MAX_REMINDER_ACCOUNT_KEY_CHARS);
    item.calendar_calendar_id =
        normalize_field(&item.calendar_calendar_id, MAX_REMINDER_TARGET_ID_CHARS);
    item.calendar_remote_id =
        normalize_field(&item.calendar_remote_id, MAX_REMINDER_REMOTE_ID_CHARS);
    item.calendar_timezone = normalize_field(&item.calendar_timezone, REMIND_AT_MAX_CONTEXT_LEN);
    item.calendar_location = normalize_field(&item.calendar_location, REMIND_AT_MAX_CONTEXT_LEN);
    item.calendar_notes = normalize_field(&item.calendar_notes, REMIND_AT_MAX_CONTEXT_LEN);
    if item.calendar_start_at_unix_secs == 0 && item.calendar_end_at_unix_secs != 0 {
        return Err(Error::config(
            "reminder_item",
            "calendar_start_at_unix_secs must be set when calendar_end_at_unix_secs is set",
        ));
    }
    if item.calendar_start_at_unix_secs != 0 && item.calendar_end_at_unix_secs == 0 {
        return Err(Error::config(
            "reminder_item",
            "calendar_end_at_unix_secs must be set when calendar_start_at_unix_secs is set",
        ));
    }
    if item.calendar_start_at_unix_secs != 0
        && item.calendar_end_at_unix_secs <= item.calendar_start_at_unix_secs
    {
        return Err(Error::config(
            "reminder_item",
            "calendar_end_at_unix_secs must be greater than calendar_start_at_unix_secs",
        ));
    }
    Ok(item)
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reminder_item_rejects_half_calendar_range() {
        let err = normalize_reminder_item(ReminderItem {
            id: "rem-1".to_string(),
            channel: "qq".to_string(),
            chat_id: "chat".to_string(),
            at_unix_secs: 100,
            context: "wake up".to_string(),
            calendar_start_at_unix_secs: 100,
            ..ReminderItem::default()
        })
        .expect_err("half range should fail");
        assert_eq!(err.stage(), "reminder_item");
    }
}
