//! ESP32 平台的 Platform 实现。仅在此目标编译。
//! ESP32 implementation of Platform trait.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::abstraction::{MemorySnapshot, Platform, StateFs};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::{
    display_driver::{install_display_state, DisplayState},
    heartbeat_file::read_heartbeat_file,
    spiffs::{
        spiffs_usage, SpiffsCalendarProviderCredentialStore, SpiffsCalendarStore,
        SpiffsExecutionStateStore, SpiffsImportantMessageStore,
        SpiffsLongTermMemoryExtractionStateStore, SpiffsLongTermMemoryStore, SpiffsMemoryStore,
        SpiffsPendingRetryStore, SpiffsRemindAtStore, SpiffsSessionStore,
        SpiffsSessionSummaryStore, SpiffsSkillMetaStore, SpiffsSkillStorage,
    },
    NvsConfigStore,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::{
    calendar::{CalendarProviderCredentialStore, CalendarStore},
    config::{AppConfig, AudioSegment},
    display::{DisplayCommand, DisplayConfig},
    memory::{
        ExecutionStateStore, ImportantMessageStore, LongTermMemoryExtractionStateStore,
        LongTermMemoryStore, MemoryStore, PendingRetryStore, RemindAtStore, SessionStore,
        SessionSummaryStore,
    },
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
/// ESP32 平台实现。
pub struct Esp32Platform {
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
    execution_state_store: Arc<SpiffsExecutionStateStore>,
    important_message_store: Arc<SpiffsImportantMessageStore>,
    remind_at_store: Arc<SpiffsRemindAtStore>,
    session_summary_store: Arc<SpiffsSessionSummaryStore>,
    wifi_scan_handle: Mutex<Option<Arc<dyn crate::platform::WifiScan + Send + Sync>>>,
    display_state: Mutex<Option<DisplayState>>,
    i2c_state: Mutex<Option<crate::platform::hardware_drivers::I2cBusState>>,
    audio_state: RwLock<Option<Arc<crate::platform::audio_drivers::AudioPipelineState>>>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Esp32Platform {
    pub fn new() -> Self {
        let state_fs: Arc<dyn StateFs + Send + Sync> =
            Arc::new(crate::platform::state_fs::Esp32StateFs);
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
            execution_state_store: Arc::new(SpiffsExecutionStateStore::new()),
            important_message_store: Arc::new(SpiffsImportantMessageStore::new()),
            remind_at_store: Arc::new(SpiffsRemindAtStore::new()),
            session_summary_store: Arc::new(SpiffsSessionSummaryStore::new()),
            wifi_scan_handle: Mutex::new(None),
            display_state: Mutex::new(None),
            i2c_state: Mutex::new(None),
            audio_state: RwLock::new(None),
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Default for Esp32Platform {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Platform for Esp32Platform {
    fn state_fs(&self) -> Arc<dyn StateFs + Send + Sync> {
        Arc::clone(&self.state_fs)
    }

    fn memory_snapshot(&self) -> MemorySnapshot {
        use crate::platform::heap::{
            heap_free_internal, heap_free_spiram, heap_largest_free_block_internal,
        };
        MemorySnapshot {
            heap_free_internal: heap_free_internal() as u32,
            heap_free_spiram: heap_free_spiram() as u32,
            heap_largest_block: heap_largest_free_block_internal() as u32,
        }
    }

    fn memory_profile(&self) -> crate::memory::MemoryProfile {
        crate::memory::MemoryProfile::Embedded
    }

    fn init(&self) -> crate::error::Result<()> {
        esp_idf_svc::sys::link_patches();
        esp_idf_svc::log::EspLogger::initialize_default();
        // 屏蔽 HTTP 服务器每个 URI 注册的 Info 日志，减少刷屏（0.52+ 使用 EspIdfLogFilter）
        let _ = esp_idf_svc::log::EspIdfLogFilter::new()
            .set_target_level("esp_idf_svc::http::server", log::LevelFilter::Warn);
        self.init_nvs()?;
        self.init_spiffs()?;
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
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn wifi_scan(&self) -> Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> {
        let opt: Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> = self
            .wifi_scan_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        opt
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
        unsafe { esp_idf_svc::sys::esp_restart() };
    }

    fn init_sntp(&self) {
        crate::platform::sntp::init_sntp();
    }

    #[cfg(feature = "ota")]
    fn ota_from_url(&self, url: &str) -> crate::error::Result<()> {
        crate::ota::ota_update_from_url(url)
    }

    fn init_display(&self, config: &DisplayConfig) -> crate::error::Result<()> {
        let mut guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        install_display_state(&mut guard, config)
    }

    fn init_audio(&self, config: &AudioSegment) -> crate::error::Result<()> {
        if !config.enabled {
            *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) = None;
            return Ok(());
        }
        match crate::platform::audio_drivers::AudioPipelineState::from_config(config) {
            Ok(state) => {
                *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) =
                    Some(Arc::new(state));
                Ok(())
            }
            Err(e) => {
                *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) = None;
                Err(e)
            }
        }
    }

    fn audio_mic_ready(&self) -> bool {
        self.audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.mic_ready())
            .unwrap_or(false)
    }

    fn audio_speaker_ready(&self) -> bool {
        self.audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.speaker_ready())
            .unwrap_or(false)
    }

    fn read_mic_pcm_i16(&self, out: &mut [i16]) -> crate::error::Result<usize> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_mic", "audio pipeline not initialized")
            })?;
        state.read_mic_pcm_i16(out)
    }

    fn write_speaker_pcm_i16(&self, buf: &[i16]) -> crate::error::Result<()> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_speaker", "audio pipeline not initialized")
            })?;
        state.write_speaker_pcm_i16(buf)
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
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        use crate::platform::hardware_drivers::{parse_aht20, parse_sht3x};
        use std::time::Duration;

        match model {
            "sht3x" => {
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, 0x2C, &[0x06])?;
                }
                std::thread::sleep(Duration::from_millis(15));
                let data = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, 6)?
                };
                let (temperature, humidity) = parse_sht3x(&data)?;
                Ok(format!(
                    r#"{{"temperature":{},"humidity":{},"model":"sht3x"}}"#,
                    temperature, humidity
                ))
            }
            "aht20" => {
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, 0xAC, &[0x33, 0x00])?;
                }
                std::thread::sleep(Duration::from_millis(80));
                let data = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, 6)?
                };
                let (temperature, humidity) = parse_aht20(&data)?;
                Ok(format!(
                    r#"{{"temperature":{},"humidity":{},"model":"aht20"}}"#,
                    temperature, humidity
                ))
            }
            "raw" => {
                let arr = options
                    .get("init_cmd")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        crate::error::Error::config(
                            "drive_i2c_sensor",
                            "raw model requires options.init_cmd",
                        )
                    })?;
                let mut cmd: Vec<u8> = Vec::with_capacity(arr.len());
                for el in arr {
                    let b = el.as_u64().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "init_cmd must be u8")
                    })?;
                    if b > 255 {
                        return Err(crate::error::Error::config(
                            "drive_i2c_sensor",
                            "init_cmd byte must be 0-255",
                        ));
                    }
                    cmd.push(b as u8);
                }
                if cmd.is_empty() {
                    return Err(crate::error::Error::config(
                        "drive_i2c_sensor",
                        "init_cmd must not be empty",
                    ));
                }
                let read_len = options
                    .get("read_len")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "raw requires read_len")
                    })? as usize;
                let wait_ms = options
                    .get("conversion_wait_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(15)
                    .min(2000);
                let reg = cmd[0];
                let tail = &cmd[1..];
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, reg, tail)?;
                }
                std::thread::sleep(Duration::from_millis(wait_ms));
                let buf = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, read_len)?
                };
                let hex: String = buf.iter().map(|b| format!("{:02X}", b)).collect();
                Ok(format!(r#"{{"raw":"{}","model":"raw"}}"#, hex))
            }
            other => Err(crate::error::Error::config(
                "drive_i2c_sensor",
                format!("unknown model '{}'", other),
            )),
        }
    }

    fn init_i2c(&self, config: &crate::config::I2cBusConfig) -> crate::error::Result<()> {
        let state = crate::platform::hardware_drivers::I2cBusState::new(
            config.sda_pin,
            config.scl_pin,
            config.freq_hz,
        )?;
        *self.i2c_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
        Ok(())
    }

    fn i2c_read(&self, addr: u8, register: u8, len: usize) -> crate::error::Result<Vec<u8>> {
        let mut guard = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.read(addr, register, len),
            None => Err(crate::error::Error::config(
                "i2c_read",
                "I2C bus not initialized",
            )),
        }
    }

    fn i2c_write(&self, addr: u8, register: u8, data: &[u8]) -> crate::error::Result<()> {
        let mut guard = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.write(addr, register, data),
            None => Err(crate::error::Error::config(
                "i2c_write",
                "I2C bus not initialized",
            )),
        }
    }
}
