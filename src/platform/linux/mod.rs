//! Linux / host 的 `Platform` 实现：与 ESP 相同存储布局（`state_mount_path`），HTTP 由 `ureq` 客户端提供。
//! Linux/host Platform: same on-disk layout as ESP; HTTP via `ureq` client.

#[cfg(target_os = "linux")]
pub(crate) mod display_fb;

use crate::platform::abstraction::{MemorySnapshot, Platform, StateFs};
use crate::platform::{
    display_driver::{install_display_state, DisplayState},
    heartbeat_file::read_heartbeat_file,
    spiffs::{
        spiffs_usage, SpiffsCalendarProviderCredentialStore, SpiffsCalendarStore,
        SpiffsExecutionStateStore, SpiffsImportantMessageStore,
        SpiffsLongTermMemoryExtractionStateStore, SpiffsLongTermMemoryStore, SpiffsMemoryStore,
        SpiffsPendingRetryStore, SpiffsRemindAtStore, SpiffsSessionStore,
        SpiffsSessionSummaryStore, SpiffsSkillMetaStore, SpiffsSkillStorage, SpiffsTaskStore,
    },
    NvsConfigStore,
};
use crate::{
    calendar::{CalendarProviderCredentialStore, CalendarStore},
    config::AppConfig,
    display::{DisplayCommand, DisplayConfig},
    memory::{
        ExecutionStateStore, ImportantMessageStore, LongTermMemoryExtractionStateStore,
        LongTermMemoryStore, MemoryStore, PendingRetryStore, RemindAtStore, SessionStore,
        SessionSummaryStore,
    },
    task::TaskStore,
};
use std::sync::{Arc, Mutex};

/// Linux / host 平台实现（musl 等 CI 与本地 `cargo build`）。
pub struct LinuxPlatform {
    state_fs: Arc<dyn StateFs + Send + Sync>,
    config_store: Arc<NvsConfigStore>,
    skill_storage: Arc<SpiffsSkillStorage>,
    skill_meta_store: Arc<SpiffsSkillMetaStore>,
    memory_store: Arc<SpiffsMemoryStore>,
    long_term_memory_store: Arc<SpiffsLongTermMemoryStore>,
    long_term_memory_extraction_state_store: Arc<SpiffsLongTermMemoryExtractionStateStore>,
    session_store: Arc<SpiffsSessionStore>,
    pending_retry_store: Arc<SpiffsPendingRetryStore>,
    calendar_store: Arc<SpiffsCalendarStore>,
    calendar_provider_credential_store: Arc<SpiffsCalendarProviderCredentialStore>,
    task_store: Arc<SpiffsTaskStore>,
    execution_state_store: Arc<SpiffsExecutionStateStore>,
    important_message_store: Arc<SpiffsImportantMessageStore>,
    remind_at_store: Arc<SpiffsRemindAtStore>,
    session_summary_store: Arc<SpiffsSessionSummaryStore>,
    wifi_scan_handle: Mutex<Option<Arc<dyn crate::platform::WifiScan + Send + Sync>>>,
    display_state: Mutex<Option<DisplayState>>,
}

impl LinuxPlatform {
    pub fn new() -> Self {
        let state_fs: Arc<dyn StateFs + Send + Sync> =
            Arc::new(crate::platform::state_fs::LinuxStateFs);
        Self {
            state_fs,
            config_store: Arc::new(NvsConfigStore),
            skill_storage: Arc::new(SpiffsSkillStorage),
            skill_meta_store: Arc::new(SpiffsSkillMetaStore),
            memory_store: Arc::new(SpiffsMemoryStore::new()),
            long_term_memory_store: Arc::new(SpiffsLongTermMemoryStore::new()),
            long_term_memory_extraction_state_store: Arc::new(
                SpiffsLongTermMemoryExtractionStateStore::new(),
            ),
            session_store: Arc::new(SpiffsSessionStore::new()),
            pending_retry_store: Arc::new(SpiffsPendingRetryStore::new()),
            calendar_store: Arc::new(SpiffsCalendarStore::new()),
            calendar_provider_credential_store: Arc::new(
                SpiffsCalendarProviderCredentialStore::new(),
            ),
            task_store: Arc::new(SpiffsTaskStore::new()),
            execution_state_store: Arc::new(SpiffsExecutionStateStore::new()),
            important_message_store: Arc::new(SpiffsImportantMessageStore::new()),
            remind_at_store: Arc::new(SpiffsRemindAtStore::new()),
            session_summary_store: Arc::new(SpiffsSessionSummaryStore::new()),
            wifi_scan_handle: Mutex::new(None),
            display_state: Mutex::new(None),
        }
    }
}

impl Default for LinuxPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl Platform for LinuxPlatform {
    fn state_fs(&self) -> Arc<dyn StateFs + Send + Sync> {
        Arc::clone(&self.state_fs)
    }

    fn memory_snapshot(&self) -> MemorySnapshot {
        crate::platform::memory_linux::linux_memory_snapshot()
    }

    fn memory_profile(&self) -> crate::memory::MemoryProfile {
        crate::memory::MemoryProfile::Standard
    }

    fn init(&self) -> crate::error::Result<()> {
        // Host 须先创建状态根，`nvs/pc_cfg.json` 依赖 `state_mount_path`。
        self.init_spiffs()?;
        self.init_nvs()?;
        Ok(())
    }

    fn init_nvs(&self) -> crate::error::Result<()> {
        crate::platform::nvs::init_nvs()
    }

    fn init_spiffs(&self) -> crate::error::Result<()> {
        crate::platform::spiffs::init_spiffs()
    }

    fn config_store(&self) -> Arc<dyn crate::platform::ConfigStore + Send + Sync> {
        Arc::clone(&self.config_store) as Arc<dyn crate::platform::ConfigStore + Send + Sync>
    }

    fn connect_wifi(&self, config: &AppConfig) -> crate::error::Result<()> {
        match crate::platform::connect_wifi(config) {
            Ok(Some(handle)) => {
                let arc_dyn: Arc<dyn crate::platform::WifiScan + Send + Sync> = Arc::new(handle);
                *self
                    .wifi_scan_handle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(arc_dyn);
                Ok(())
            }
            Ok(None) => {
                *self
                    .wifi_scan_handle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                Ok(())
            }
            Err(e) => {
                *self
                    .wifi_scan_handle
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = None;
                Err(e)
            }
        }
    }

    fn wifi_scan(&self) -> Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> {
        self.wifi_scan_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn wifi_sta_ip(&self) -> Option<String> {
        crate::platform::wifi::wifi_sta_ip()
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore + Send + Sync> {
        Arc::clone(&self.memory_store) as Arc<dyn MemoryStore + Send + Sync>
    }

    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore + Send + Sync> {
        Arc::clone(&self.long_term_memory_store) as Arc<dyn LongTermMemoryStore + Send + Sync>
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync> {
        Arc::clone(&self.long_term_memory_extraction_state_store)
            as Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync>
    }

    fn session_store(&self) -> Arc<dyn SessionStore + Send + Sync> {
        Arc::clone(&self.session_store) as Arc<dyn SessionStore + Send + Sync>
    }

    fn pending_retry_store(&self) -> Arc<dyn PendingRetryStore + Send + Sync> {
        Arc::clone(&self.pending_retry_store) as Arc<dyn PendingRetryStore + Send + Sync>
    }

    fn calendar_store(&self) -> Arc<dyn CalendarStore + Send + Sync> {
        Arc::clone(&self.calendar_store) as Arc<dyn CalendarStore + Send + Sync>
    }

    fn calendar_provider_credential_store(
        &self,
    ) -> Arc<dyn CalendarProviderCredentialStore + Send + Sync> {
        Arc::clone(&self.calendar_provider_credential_store)
            as Arc<dyn CalendarProviderCredentialStore + Send + Sync>
    }

    fn task_store(&self) -> Arc<dyn TaskStore + Send + Sync> {
        Arc::clone(&self.task_store) as Arc<dyn TaskStore + Send + Sync>
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore + Send + Sync> {
        Arc::clone(&self.execution_state_store) as Arc<dyn ExecutionStateStore + Send + Sync>
    }

    fn important_message_store(&self) -> Arc<dyn ImportantMessageStore + Send + Sync> {
        Arc::clone(&self.important_message_store) as Arc<dyn ImportantMessageStore + Send + Sync>
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore + Send + Sync> {
        Arc::clone(&self.remind_at_store) as Arc<dyn RemindAtStore + Send + Sync>
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore + Send + Sync> {
        Arc::clone(&self.session_summary_store) as Arc<dyn SessionSummaryStore + Send + Sync>
    }

    fn skill_storage(&self) -> Arc<dyn crate::platform::SkillStorage + Send + Sync> {
        Arc::clone(&self.skill_storage) as Arc<dyn crate::platform::SkillStorage + Send + Sync>
    }

    fn skill_meta_store(&self) -> Arc<dyn crate::platform::SkillMetaStore + Send + Sync> {
        Arc::clone(&self.skill_meta_store) as Arc<dyn crate::platform::SkillMetaStore + Send + Sync>
    }

    fn create_http_client(
        &self,
        config: &AppConfig,
    ) -> crate::error::Result<Box<dyn crate::platform::PlatformHttpClient>> {
        if !config.proxy_url.trim().is_empty() {
            Ok(Box::new(crate::platform::EspHttpClient::new_with_config(
                config,
            )?))
        } else {
            Ok(Box::new(crate::platform::EspHttpClient::new()?))
        }
    }

    fn spiffs_usage(&self) -> Option<(usize, usize)> {
        spiffs_usage()
    }

    fn read_heartbeat_file(&self) -> crate::error::Result<String> {
        read_heartbeat_file()
    }

    fn request_restart(&self) {
        log::warn!("[platform::linux] restart requested, exiting process (systemd will restart)");
        std::process::exit(42);
    }

    fn init_sntp(&self) {
        crate::platform::sntp::init_sntp();
    }

    fn init_display(&self, config: &DisplayConfig) -> crate::error::Result<()> {
        let mut guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        install_display_state(&mut guard, config)
    }

    fn display_available(&self) -> bool {
        self.display_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.available)
            .unwrap_or(false)
    }

    fn display_command(&self, cmd: DisplayCommand) -> crate::error::Result<()> {
        let mut guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.execute(cmd),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn set_display_backlight(&self, on: bool) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.set_backlight(on),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn display_backlight_available(&self) -> bool {
        self.display_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.backlight_available())
            .unwrap_or(false)
    }

    fn set_display_backlight_brightness(&self, percent: u8) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.set_brightness(percent),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn fade_display_backlight(
        &self,
        from: u8,
        to: u8,
        duration_ms: u32,
    ) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.fade_brightness(from, to, duration_ms),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn drive_gpio_out(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_gpio_out(pins, params)
    }

    fn drive_gpio_in(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_gpio_in(pins, params, options)
    }

    fn drive_pwm_out(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
        ledc_channel: u8,
        ledc_timer_index: u8,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_pwm_out(
            pins,
            params,
            options,
            ledc_channel,
            ledc_timer_index,
        )
    }

    fn drive_adc_in(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_adc_in(pins, params, options)
    }

    fn drive_buzzer(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_buzzer(pins, params)
    }

    fn drive_dht(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_dht(pins, params, options)
    }

    fn drive_i2c_sensor(
        &self,
        addr: u8,
        model: &str,
        _watch_field: &str,
        _options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_i2c_sensor_stub(addr, model)
    }

    fn i2c_read(&self, addr: u8, register: u8, len: usize) -> crate::error::Result<Vec<u8>> {
        crate::platform::hardware_drivers::drive_i2c_read(addr, register, len)
    }

    fn i2c_write(&self, addr: u8, register: u8, data: &[u8]) -> crate::error::Result<()> {
        crate::platform::hardware_drivers::drive_i2c_write(addr, register, data)
    }
}
