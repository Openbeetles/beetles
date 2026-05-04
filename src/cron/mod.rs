//! 定时检查持久化 cron 任务与 sensor watch；错峰退避，失败打日志不 panic。
//! Cron: check persisted cron tasks and sensor watches on interval; backoff on failure, log only.
//! 到期任务 / 告警才注入系统消息，避免空闲 tick 驱动 agent 空转。

use crate::bus::{PcMsg, SystemInboundTx};
use crate::config::{DeviceEntry, I2cSensorEntry};
use crate::i18n::Locale;
use crate::memory::MemoryStore;
use crate::tools::cron_manage::CronTask;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 持久化 cron 任务需在下次匹配前从 storage 重载；`cron_manage` 写入后置位。启动时为 `true` 保证首轮加载。
pub static CRON_PERSISTED_TASKS_DIRTY: AtomicBool = AtomicBool::new(true);

/// `cron_manage` 保存任务后调用，使 `run_cron_loop` 内存缓存失效。
pub fn mark_cron_persisted_tasks_dirty() {
    CRON_PERSISTED_TASKS_DIRTY.store(true, Ordering::Release);
}

const TAG: &str = "cron";
/// 默认轮询间隔（秒）。
pub const DEFAULT_CRON_INTERVAL_SECS: u64 = 60;

/// `sensor_watch` 检查所需：平台 + GPIO 类设备 + I2C 传感器列表。
pub struct SensorWatchContext {
    pub platform: Arc<dyn crate::Platform>,
    pub devices: Vec<DeviceEntry>,
    pub i2c_sensors: Vec<I2cSensorEntry>,
}

/// Cron tick 的可变状态，供 bg_timer 跨轮次复用。
#[derive(Default)]
pub struct CronTickState {
    pub(crate) persisted_cron_cache: Vec<CronTask>,
}

impl CronTickState {
    pub fn new() -> Self {
        Self {
            persisted_cron_cache: Vec::new(),
        }
    }
}

/// 单次 cron tick：检查持久化任务与 sensor watch，并在有真实事件时注入消息。
/// 由 bg_timer 每 60s 调用一次。
pub(crate) fn cron_tick(
    inbound_tx: &SystemInboundTx,
    memory_store: Option<&Arc<dyn MemoryStore + Send + Sync>>,
    sensor_watch: Option<&SensorWatchContext>,
    resolve_locale: &Arc<dyn Fn() -> Locale + Send + Sync>,
    state: &mut CronTickState,
) {
    // Check persisted cron tasks and sensor watches. Do not emit a synthetic "tick"
    // message; idle cron rounds must not wake the agent on either ESP or Linux.
    if let Some(store) = memory_store {
        fire_persisted_tasks(
            store.as_ref(),
            inbound_tx,
            &mut state.persisted_cron_cache,
            resolve_locale,
        );
        if let Some(ctx) = sensor_watch {
            let loc = resolve_locale();
            crate::tools::sensor_watch::check_sensor_watches(
                store.as_ref(),
                inbound_tx,
                ctx.platform.as_ref(),
                ctx.devices.as_slice(),
                ctx.i2c_sensors.as_slice(),
                loc,
            );
        }
    }
}

/// Check persisted cron tasks and fire any that match the current minute.
fn fire_persisted_tasks(
    store: &dyn MemoryStore,
    inbound_tx: &SystemInboundTx,
    cache: &mut Vec<CronTask>,
    resolve_locale: &Arc<dyn Fn() -> Locale + Send + Sync>,
) {
    let loc = resolve_locale();
    if CRON_PERSISTED_TASKS_DIRTY.swap(false, Ordering::AcqRel) {
        *cache = crate::tools::cron_manage::load_persisted_cron_tasks(store);
    }
    let tasks = cache.as_slice();
    if tasks.is_empty() {
        return;
    }

    let now_secs = crate::util::current_unix_secs();
    let (_y, mo, d, h, min, _s) = crate::util::epoch_to_ymdhms(now_secs);
    let dow_actual = ((now_secs / 86400) as u32 + 4) % 7; // 0=Sunday

    for task in tasks {
        if !task.enabled {
            continue;
        }
        if let Ok(matches) = cron_matches(&task.expr, min, h, d, mo, dow_actual) {
            if matches {
                let content = crate::i18n::tr(
                    crate::i18n::Message::CronTaskFired {
                        id: task.id.clone(),
                        action: task.action.clone(),
                    },
                    loc,
                );
                match PcMsg::new_inbound_with_ingress(
                    &task.channel,
                    &task.chat_id,
                    content,
                    false,
                    crate::bus::IngressKind::System,
                ) {
                    Ok(msg) => {
                        if let Err(e) = inbound_tx.send(msg) {
                            log::warn!("[{}] failed to fire cron task {}: {}", TAG, task.id, e);
                        } else {
                            log::info!("[{}] fired cron task {}", TAG, task.id);
                        }
                    }
                    Err(e) => {
                        log::warn!("[{}] PcMsg::new for task {} failed: {}", TAG, task.id, e);
                    }
                }
            }
        }
    }
}

/// Check if a cron expression matches the given time components.
fn cron_matches(
    expr: &str,
    minute: u32,
    hour: u32,
    dom: u32,
    month: u32,
    dow: u32,
) -> crate::error::Result<bool> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return Ok(false);
    }
    let minutes = crate::tools::cron::parse_cron_field(parts[0], 0, 59)?;
    let hours = crate::tools::cron::parse_cron_field(parts[1], 0, 23)?;
    let doms = crate::tools::cron::parse_cron_field(parts[2], 1, 31)?;
    let months = crate::tools::cron::parse_cron_field(parts[3], 1, 12)?;
    let dows = crate::tools::cron::parse_cron_field(parts[4], 0, 6)?;

    Ok(minutes.contains(&minute)
        && hours.contains(&hour)
        && (doms.contains(&dom) || dows.contains(&dow))
        && months.contains(&month))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::new_inbound_channel;
    use crate::memory::MemoryStore;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn cron_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("cron test lock poisoned")
    }

    struct TestMemoryStore {
        daily_notes: Mutex<HashMap<String, String>>,
    }

    impl TestMemoryStore {
        fn new() -> Self {
            Self {
                daily_notes: Mutex::new(HashMap::new()),
            }
        }

        fn with_daily_note(path: &str, content: &str) -> Self {
            let mut daily_notes = HashMap::new();
            daily_notes.insert(path.to_string(), content.to_string());
            Self {
                daily_notes: Mutex::new(daily_notes),
            }
        }
    }

    impl MemoryStore for TestMemoryStore {
        fn get_memory(&self) -> crate::error::Result<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> crate::error::Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, _recent_n: usize) -> crate::error::Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn get_daily_note(&self, name: &str) -> crate::error::Result<String> {
            Ok(self
                .daily_notes
                .lock()
                .expect("daily_notes poisoned")
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write_daily_note(&self, name: &str, content: &str) -> crate::error::Result<()> {
            self.daily_notes
                .lock()
                .expect("daily_notes poisoned")
                .insert(name.to_string(), content.to_string());
            Ok(())
        }
    }

    #[test]
    fn cron_tick_without_tasks_does_not_enqueue_synthetic_tick() {
        let _guard = cron_test_guard();
        let (tx, rx, _depth) = new_inbound_channel(4);
        let resolve_locale: Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            Arc::new(|| crate::i18n::Locale::Zh);
        let mut state = CronTickState::new();
        let store: Arc<dyn MemoryStore + Send + Sync> = Arc::new(TestMemoryStore::new());

        cron_tick(&tx, Some(&store), None, &resolve_locale, &mut state);

        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cron_tick_enqueues_due_persisted_task() {
        let _guard = cron_test_guard();
        let (tx, rx, _depth) = new_inbound_channel(4);
        let resolve_locale: Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync> =
            Arc::new(|| crate::i18n::Locale::Zh);
        let mut state = CronTickState::new();
        let tasks = serde_json::json!([{
            "id": "ct_test",
            "expr": "* * * * *",
            "action": "ping",
            "channel": "qq_channel",
            "chat_id": "c2c:test",
            "enabled": true
        }]);
        let store: Arc<dyn MemoryStore + Send + Sync> = Arc::new(TestMemoryStore::with_daily_note(
            "memory/cron_tasks.json",
            &tasks.to_string(),
        ));

        CRON_PERSISTED_TASKS_DIRTY.store(true, Ordering::Release);
        cron_tick(&tx, Some(&store), None, &resolve_locale, &mut state);

        let msg = rx.try_recv().expect("expected due cron task");
        assert_eq!(msg.channel.as_ref(), "qq_channel");
        assert_eq!(msg.chat_id.as_ref(), "c2c:test");
        assert_eq!(msg.ingress, crate::bus::IngressKind::System);
        assert!(msg.content.contains("ct_test"));
        assert!(msg.content.contains("ping"));
    }
}
