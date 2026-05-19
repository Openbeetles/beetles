//! Shared task domain: durable task records, query helpers, and store abstraction.

use crate::error::{Error, Result};
use crate::util::truncate_content_to_max;
use serde::{Deserialize, Serialize};

pub const REL_PATH_TASKS: &str = "memory/tasks.json";
pub const MAX_TASK_TITLE_CHARS: usize = 120;
pub const MAX_TASK_DETAIL_CHARS: usize = 512;
pub const MAX_TASK_PROJECT_CHARS: usize = 80;
pub const MAX_TASK_ID_CHARS: usize = 128;
pub const MAX_TASK_CHANNEL_CHARS: usize = 32;
pub const MAX_TASK_CHAT_ID_CHARS: usize = 160;
pub const MAX_TASK_CALENDAR_EVENT_ID_CHARS: usize = 128;
pub const MAX_TASK_CALENDAR_PROVIDER_CHARS: usize = 32;
pub const MAX_TASK_CALENDAR_ACCOUNT_KEY_CHARS: usize = 128;
pub const MAX_TASK_CALENDAR_TARGET_ID_CHARS: usize = 64;
pub const MAX_TASK_CALENDAR_REMOTE_ID_CHARS: usize = 128;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Open,
    InProgress,
    Completed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskItem {
    pub id: String,
    pub channel: String,
    pub chat_id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub due_at_unix_secs: u64,
    #[serde(default)]
    pub due_notified_at_unix_secs: u64,
    #[serde(default)]
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
    pub completed_at_unix_secs: u64,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskQuery {
    pub status: Option<TaskStatus>,
    pub project: String,
    pub include_completed: bool,
    pub limit: usize,
}

impl Default for TaskQuery {
    fn default() -> Self {
        Self {
            status: None,
            project: String::new(),
            include_completed: false,
            limit: 20,
        }
    }
}

pub trait TaskStore: Send + Sync {
    fn list(&self, channel: &str, chat_id: &str, query: TaskQuery) -> Result<Vec<TaskItem>>;
    fn get(&self, channel: &str, chat_id: &str, id: &str) -> Result<Option<TaskItem>>;
    fn upsert(&self, task: &TaskItem) -> Result<()>;
    fn delete(&self, channel: &str, chat_id: &str, id: &str) -> Result<bool>;
    fn list_due_unnotified(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>>;
    fn mark_due_notified(&self, task: &TaskItem, notified_at_unix_secs: u64) -> Result<bool>;
    /// 返回最早待提醒任务的 due_at；无待提醒任务则返回 Ok(None)。
    fn next_due_at(&self) -> Result<Option<u64>> {
        Ok(None)
    }
}

pub fn normalize_task_item(mut task: TaskItem) -> Result<TaskItem> {
    task.id = normalize_field(&task.id, MAX_TASK_ID_CHARS);
    if task.id.is_empty() {
        return Err(Error::config("task_item", "id must not be empty"));
    }
    task.channel = normalize_field(&task.channel, MAX_TASK_CHANNEL_CHARS);
    if task.channel.is_empty() {
        return Err(Error::config("task_item", "channel must not be empty"));
    }
    task.chat_id = normalize_field(&task.chat_id, MAX_TASK_CHAT_ID_CHARS);
    if task.chat_id.is_empty() {
        return Err(Error::config("task_item", "chat_id must not be empty"));
    }
    task.title = normalize_field(&task.title, MAX_TASK_TITLE_CHARS);
    if task.title.is_empty() {
        return Err(Error::config("task_item", "title must not be empty"));
    }
    task.detail = normalize_field(&task.detail, MAX_TASK_DETAIL_CHARS);
    task.project = normalize_field(&task.project, MAX_TASK_PROJECT_CHARS);
    task.calendar_event_id =
        normalize_field(&task.calendar_event_id, MAX_TASK_CALENDAR_EVENT_ID_CHARS);
    task.calendar_provider =
        normalize_field(&task.calendar_provider, MAX_TASK_CALENDAR_PROVIDER_CHARS);
    task.calendar_account_key = normalize_field(
        &task.calendar_account_key,
        MAX_TASK_CALENDAR_ACCOUNT_KEY_CHARS,
    );
    task.calendar_calendar_id = normalize_field(
        &task.calendar_calendar_id,
        MAX_TASK_CALENDAR_TARGET_ID_CHARS,
    );
    task.calendar_remote_id =
        normalize_field(&task.calendar_remote_id, MAX_TASK_CALENDAR_REMOTE_ID_CHARS);
    task.calendar_timezone = normalize_field(&task.calendar_timezone, MAX_TASK_DETAIL_CHARS);
    task.calendar_location = normalize_field(&task.calendar_location, MAX_TASK_DETAIL_CHARS);
    task.calendar_notes = normalize_field(&task.calendar_notes, MAX_TASK_DETAIL_CHARS);

    if task.calendar_start_at_unix_secs == 0 && task.calendar_end_at_unix_secs != 0 {
        return Err(Error::config(
            "task_item",
            "calendar_start_at_unix_secs must be set when calendar_end_at_unix_secs is set",
        ));
    }
    if task.calendar_start_at_unix_secs != 0 && task.calendar_end_at_unix_secs == 0 {
        return Err(Error::config(
            "task_item",
            "calendar_end_at_unix_secs must be set when calendar_start_at_unix_secs is set",
        ));
    }
    if task.calendar_start_at_unix_secs != 0
        && task.calendar_end_at_unix_secs <= task.calendar_start_at_unix_secs
    {
        return Err(Error::config(
            "task_item",
            "calendar_end_at_unix_secs must be greater than calendar_start_at_unix_secs",
        ));
    }
    if task.due_at_unix_secs == 0 {
        task.due_notified_at_unix_secs = 0;
    }
    if task.status != TaskStatus::Completed {
        task.completed_at_unix_secs = 0;
    }
    Ok(task)
}

pub fn filter_tasks(mut tasks: Vec<TaskItem>, query: TaskQuery) -> Vec<TaskItem> {
    let project = query.project.trim();
    let limit = query.limit.clamp(1, 50);
    tasks.retain(|task| {
        if let Some(status) = query.status {
            if task.status != status {
                return false;
            }
        } else if !query.include_completed && task.status.is_terminal() {
            return false;
        }
        if !project.is_empty() && task.project != project {
            return false;
        }
        true
    });
    tasks.sort_by(|left, right| {
        task_status_rank(left.status)
            .cmp(&task_status_rank(right.status))
            .then_with(|| task_due_sort_key(left).cmp(&task_due_sort_key(right)))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.id.cmp(&right.id))
    });
    if tasks.len() > limit {
        tasks.truncate(limit);
    }
    tasks
}

fn task_status_rank(status: TaskStatus) -> u8 {
    match status {
        TaskStatus::InProgress => 0,
        TaskStatus::Open => 1,
        TaskStatus::Completed => 2,
        TaskStatus::Cancelled => 3,
    }
}

fn task_due_sort_key(task: &TaskItem) -> (u8, u64) {
    if task.due_at_unix_secs == 0 {
        (1, u64::MAX)
    } else {
        (0, task.due_at_unix_secs)
    }
}

fn normalize_field(value: &str, max_chars: usize) -> String {
    truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

/// Single bg-timer tick for due tasks. Claims due tasks and injects one system inbound message per task.
pub(crate) fn task_due_tick(
    task_store: &dyn TaskStore,
    inbound_tx: &crate::bus::SystemInboundTx,
    resolve_locale: &std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let limit = crate::constants::DUE_TASK_SWEEP_BATCH_MAX.min(inbound_tx.remaining_capacity());
    if limit == 0 {
        return;
    }
    let tasks = match task_store.list_due_unnotified(now, limit) {
        Ok(tasks) => tasks,
        Err(error) => {
            log::warn!("[task::task_due_tick] failed to list due tasks: {error}");
            return;
        }
    };
    for task in tasks {
        let loc = resolve_locale();
        let mut content = crate::i18n::tr(
            crate::i18n::Message::TaskDue {
                title: task.title.clone(),
            },
            loc,
        );
        if !task.project.is_empty() {
            content.push_str(&format!("\nProject: {}", task.project));
        }
        if !task.detail.is_empty() {
            content.push_str(&format!("\n{}", task.detail));
        }
        let Ok(msg) = crate::bus::PcMsg::new_inbound_with_ingress(
            &task.channel,
            &task.chat_id,
            content,
            false,
            crate::bus::IngressKind::System,
        ) else {
            log::warn!(
                "[task::task_due_tick] failed to build system inbound for task {}",
                task.id
            );
            continue;
        };
        match inbound_tx.try_send(msg) {
            Ok(()) => {
                if let Err(error) = task_store.mark_due_notified(&task, now) {
                    log::warn!(
                        "[task::task_due_tick] failed to mark due task {} notified: {}",
                        task.id,
                        error
                    );
                }
            }
            Err(std::sync::mpsc::TrySendError::Full(_))
            | Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_system_inbound_channel;
    use std::sync::Mutex;

    #[test]
    fn normalize_task_item_rejects_half_calendar_range() {
        let err = normalize_task_item(TaskItem {
            id: "task-1".to_string(),
            channel: "qq".to_string(),
            chat_id: "chat".to_string(),
            title: "task".to_string(),
            calendar_start_at_unix_secs: 100,
            ..TaskItem::default()
        })
        .unwrap_err();
        assert_eq!(err.stage(), "task_item");
    }

    #[test]
    fn filter_tasks_prefers_active_due_tasks() {
        let tasks = vec![
            TaskItem {
                id: "b".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat".to_string(),
                title: "later".to_string(),
                due_at_unix_secs: 200,
                updated_at: 2,
                ..TaskItem::default()
            },
            TaskItem {
                id: "c".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat".to_string(),
                title: "done".to_string(),
                status: TaskStatus::Completed,
                due_at_unix_secs: 100,
                updated_at: 3,
                completed_at_unix_secs: 3,
                ..TaskItem::default()
            },
            TaskItem {
                id: "a".to_string(),
                channel: "qq".to_string(),
                chat_id: "chat".to_string(),
                title: "now".to_string(),
                status: TaskStatus::InProgress,
                due_at_unix_secs: 100,
                updated_at: 1,
                ..TaskItem::default()
            },
        ];
        let filtered = filter_tasks(tasks, TaskQuery::default());
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].id, "a");
        assert_eq!(filtered[1].id, "b");
    }

    #[test]
    fn filter_tasks_clamps_limit_to_fifty() {
        let tasks = (0..60)
            .map(|idx| TaskItem {
                id: format!("task-{}", idx),
                channel: "qq".to_string(),
                chat_id: "chat".to_string(),
                title: format!("task {}", idx),
                updated_at: idx as u64,
                ..TaskItem::default()
            })
            .collect::<Vec<_>>();
        let filtered = filter_tasks(
            tasks,
            TaskQuery {
                limit: 100,
                ..TaskQuery::default()
            },
        );
        assert_eq!(filtered.len(), 50);
    }

    #[derive(Default)]
    struct StubDueTaskStore {
        items: Mutex<Vec<TaskItem>>,
    }

    impl StubDueTaskStore {
        fn new(items: Vec<TaskItem>) -> Self {
            Self {
                items: Mutex::new(items),
            }
        }

        fn notified_ids(&self) -> Vec<String> {
            self.items
                .lock()
                .expect("items lock")
                .iter()
                .filter(|item| item.due_notified_at_unix_secs != 0)
                .map(|item| item.id.clone())
                .collect()
        }

        fn unnotified_ids(&self) -> Vec<String> {
            self.items
                .lock()
                .expect("items lock")
                .iter()
                .filter(|item| item.due_notified_at_unix_secs == 0)
                .map(|item| item.id.clone())
                .collect()
        }
    }

    impl TaskStore for StubDueTaskStore {
        fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<TaskItem>> {
            Ok(None)
        }

        fn upsert(&self, _task: &TaskItem) -> Result<()> {
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn list_due_unnotified(&self, now_unix_secs: u64, limit: usize) -> Result<Vec<TaskItem>> {
            let items = self.items.lock().expect("items lock");
            let mut due = items
                .iter()
                .filter(|item| {
                    item.due_at_unix_secs != 0
                        && item.due_at_unix_secs <= now_unix_secs
                        && item.due_notified_at_unix_secs == 0
                        && !item.status.is_terminal()
                })
                .cloned()
                .collect::<Vec<_>>();
            due.sort_by(|left, right| left.id.cmp(&right.id));
            if due.len() > limit {
                due.truncate(limit);
            }
            Ok(due)
        }

        fn mark_due_notified(&self, task: &TaskItem, notified_at_unix_secs: u64) -> Result<bool> {
            let mut items = self.items.lock().expect("items lock");
            let Some(item) = items.iter_mut().find(|item| {
                item.channel == task.channel && item.chat_id == task.chat_id && item.id == task.id
            }) else {
                return Ok(false);
            };
            if item != task {
                return Ok(false);
            }
            item.due_notified_at_unix_secs = notified_at_unix_secs;
            Ok(true)
        }
    }

    fn due_task(id: &str) -> TaskItem {
        TaskItem {
            id: id.to_string(),
            channel: "qq_channel".to_string(),
            chat_id: "chat-1".to_string(),
            title: format!("task-{id}"),
            due_at_unix_secs: 1,
            ..TaskItem::default()
        }
    }

    #[test]
    fn task_due_tick_processes_at_most_four_due_items_per_tick() {
        let store =
            StubDueTaskStore::new((0..5).map(|idx| due_task(&format!("task-{idx}"))).collect());
        let (tx, rx, _) = new_system_inbound_channel(16);
        let resolve_locale: std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            std::sync::Arc::new(|| crate::i18n::Locale::Zh);

        task_due_tick(&store, &tx, &resolve_locale);

        let delivered = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(delivered.len(), 4);
        assert_eq!(
            store.notified_ids(),
            vec![
                "task-0".to_string(),
                "task-1".to_string(),
                "task-2".to_string(),
                "task-3".to_string()
            ]
        );
        assert_eq!(store.unnotified_ids(), vec!["task-4".to_string()]);
    }

    #[test]
    fn task_due_tick_does_not_mark_notified_when_system_inbound_is_disconnected() {
        let store = StubDueTaskStore::new(vec![due_task("task-1")]);
        let (tx, rx, _) = new_system_inbound_channel(1);
        drop(rx);
        let resolve_locale: std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            std::sync::Arc::new(|| crate::i18n::Locale::Zh);

        task_due_tick(&store, &tx, &resolve_locale);

        assert!(store.notified_ids().is_empty());
        assert_eq!(store.unnotified_ids(), vec!["task-1".to_string()]);
    }
}
