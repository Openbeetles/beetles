//! 平台抽象 trait：ConfigStore、SkillStorage、PlatformHttpClient、Platform。
//! 核心域与 main 仅依赖这些 trait，便于后续支持多种硬件。

use crate::config::{AppConfig, AudioSegment, PinConfig};
use crate::display::{DisplayCommand, DisplayConfig};
use crate::error::Result;
use crate::memory::{
    ImportantMessageStore, MemoryStore, PendingRetryStore, RemindAtStore, SessionStore,
    SessionSummaryStore, TaskContinuationStore,
};
use crate::platform::ResponseBody;
use serde_json::Value;
use std::sync::Arc;

/// 状态根目录下的受控文件访问（相对路径）。ESP 委托 SPIFFS + 互斥；Linux 由 `LinuxPlatform` 实现。
/// Controlled file access under the platform state root (relative paths).
pub trait StateFs: Send + Sync {
    /// 读取文件，不存在返回 `Ok(None)`。
    fn read(&self, rel_path: &str) -> crate::error::Result<Option<Vec<u8>>>;
    /// 写入文件；实现须先创建父目录再写入。单文件大小上界由实现保证（与 `spiffs::MAX_WRITE_SIZE` 一致）。
    fn write(&self, rel_path: &str, data: &[u8]) -> crate::error::Result<()>;
    /// 删除文件，不存在时 `Ok(())`。
    fn remove(&self, rel_path: &str) -> crate::error::Result<()>;
    /// 列出一层目录下的文件名（不递归子目录）。
    fn list_dir(&self, rel_path: &str) -> crate::error::Result<Vec<String>>;
    /// 文件是否存在。
    fn exists(&self, rel_path: &str) -> crate::error::Result<bool> {
        Ok(self.read(rel_path)?.is_some())
    }
}

/// 平台内存快照，语义与 orchestrator 堆原子字段对齐（跨平台可比）。
/// Platform memory snapshot aligned with orchestrator heap atomics (cross-platform comparable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySnapshot {
    /// 内部堆空闲字节（ESP: internal heap；Linux: 主内存可用量，通常为 `MemAvailable`）。
    pub heap_free_internal: u32,
    /// 外部堆空闲字节（ESP: SPIRAM；Linux: 0 或 swap 等扩展字段保留为 0）。
    pub heap_free_spiram: u32,
    /// 最大连续可分配块字节（ESP: largest free block for TLS fragmentation checks）。
    /// Linux / host: **always 0** — means *not applicable*, not “zero-byte largest block”.
    /// 最大连续空闲块（字节）。ESP 为 TLS 碎片门禁所用；Linux 固定 **0** 表示本维度不可用，勿当作真实块大小。
    pub heap_largest_block: u32,
}

/// 配置键值存储抽象（如 NVS）。用于 config、pairing、skills 的 NVS 部分。
pub trait ConfigStore: Send + Sync {
    fn read_string(&self, key: &str) -> Result<Option<String>>;
    /// 批量读取；默认逐键 read_string。NVS 实现可覆写为单 handle 多 key 读，减少 open/close 避免 4361。
    fn read_strings(&self, keys: &[&str]) -> Result<Vec<Option<String>>> {
        keys.iter().map(|k| self.read_string(k)).collect()
    }
    fn write_string(&self, key: &str, value: &str) -> Result<()>;
    /// 批量写入；默认逐键 write_string，NVS 实现可覆写为单 handle 批量写以避免 4361。
    fn write_strings(&self, pairs: &[(&str, &str)]) -> Result<()> {
        for (k, v) in pairs {
            self.write_string(k, v)?;
        }
        Ok(())
    }
    /// 擦除指定 keys；命名空间由实现方绑定（如 pc_cfg）。
    fn erase_keys(&self, keys: &[&str]) -> Result<()>;
}

/// 技能元数据（顺序、禁用列表）存储抽象。用于 SPIFFS config/skills_meta.json，避免 NVS 高频单键写。
pub trait SkillMetaStore: Send + Sync {
    /// 返回 (order, disabled)。
    fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)>;
    fn write_meta(&self, order: &[String], disabled: &[String]) -> Result<()>;
}

/// Skills 目录下 .md 文件存储抽象。list_names 返回不含 .md 后缀的名称。
pub trait SkillStorage: Send + Sync {
    fn list_names(&self) -> Result<Vec<String>>;
    fn read(&self, name: &str) -> Result<Vec<u8>>;
    fn write(&self, name: &str, content: &[u8]) -> Result<()>;
    fn remove(&self, name: &str) -> Result<()>;
}

/// 统一 HTTP 客户端：仅 get/post/post_streaming/reset 方法，LlmHttpClient、ToolContext、ChannelHttpClient 由 lib 层 blanket 转发。
pub trait PlatformHttpClient {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)>;
    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)>;
    /// HTTP PATCH; default implementation falls back to POST.
    fn patch(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.post(url, headers, body)
    }
    /// HTTP PUT; default implementation falls back to POST.
    fn put(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        self.post(url, headers, body)
    }
    /// HTTP DELETE; default implementation falls back to GET.
    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        self.get(url, headers)
    }
    /// 流式 POST：发送请求后逐块回调 on_chunk，不将响应体读入内存。
    /// `max_response_bytes`: None = 无限制（适用于边到达边消费的场景如 TTS）；Some(n) = 限制总字节数。
    /// 默认实现回退到 post()，将完整响应体一次性传给 on_chunk。
    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        _max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        let (status, resp_body) = self.post(url, headers, body)?;
        on_chunk(resp_body.as_ref())?;
        Ok(status)
    }
    fn reset_connection_for_retry(&mut self) {}
}

impl PlatformHttpClient for Box<dyn PlatformHttpClient + '_> {
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        (**self).get(url, headers)
    }
    fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        (**self).post(url, headers, body)
    }
    fn post_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        (**self).post_streaming(url, headers, body, max_response_bytes, on_chunk)
    }
    fn reset_connection_for_retry(&mut self) {
        (**self).reset_connection_for_retry()
    }
    fn patch(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        (**self).patch(url, headers, body)
    }
    fn put(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, ResponseBody)> {
        (**self).put(url, headers, body)
    }
    fn delete(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        (**self).delete(url, headers)
    }
}

/// 平台能力聚合。main 只依赖当前平台的 Platform 实现。Send + Sync 以便跨线程传入 run_http_server。
pub trait Platform: Send + Sync {
    /// 状态文件系统抽象（SPIFFS 根或 Linux 状态目录）。业务域经此访问，禁止直引 `platform::spiffs`。
    fn state_fs(&self) -> Arc<dyn StateFs + Send + Sync>;

    /// 当前内存快照；须来自真实数据源（ESP: `heap`；Linux: `/proc/meminfo`），禁止占位常量。
    fn memory_snapshot(&self) -> MemorySnapshot;

    /// 平台初始化（link_patches、日志、NVS、SPIFFS 等）。main 在构造后首先调用。
    fn init(&self) -> Result<()> {
        Ok(())
    }
    fn init_nvs(&self) -> Result<()>;
    fn init_spiffs(&self) -> Result<()>;
    fn config_store(&self) -> Arc<dyn ConfigStore + Send + Sync>;
    fn connect_wifi(&self, config: &AppConfig) -> Result<()>;
    /// WiFi 扫描句柄（SoftAP 就绪且底层已注册扫描时为 Some；STA 失败时仍应保留以便配网）；用于 GET /api/wifi/scan。
    fn wifi_scan(&self) -> Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> {
        None
    }
    /// 当前 STA IPv4 地址（例如 192.168.1.42）；不可用时返回 None。
    fn wifi_sta_ip(&self) -> Option<String> {
        None
    }
    fn memory_store(&self) -> Arc<dyn MemoryStore + Send + Sync>;
    fn session_store(&self) -> Arc<dyn SessionStore + Send + Sync>;
    fn pending_retry_store(&self) -> Arc<dyn PendingRetryStore + Send + Sync>;
    fn task_continuation_store(&self) -> Arc<dyn TaskContinuationStore + Send + Sync>;
    fn important_message_store(&self) -> Arc<dyn ImportantMessageStore + Send + Sync>;
    fn remind_at_store(&self) -> Arc<dyn RemindAtStore + Send + Sync>;
    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore + Send + Sync>;
    fn skill_storage(&self) -> Arc<dyn SkillStorage + Send + Sync>;
    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore + Send + Sync>;
    fn create_http_client(&self, config: &AppConfig) -> Result<Box<dyn PlatformHttpClient>>;
    fn spiffs_usage(&self) -> Option<(usize, usize)>;
    fn read_heartbeat_file(&self) -> Result<String>;

    /// 板级状态 JSON（芯片、堆、运行时间、压力、WiFi、SPIFFS）。默认实现委托 `platform/board_info`；新平台可覆写。
    fn board_info_json(&self) -> Result<String> {
        Ok(crate::platform::board_info::board_info_json_string())
    }

    /// 读状态根配置文件（相对路径如 `config/llm.json`）。不存在返回 `Ok(None)`。经 `state_fs` 唯一路径。
    fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        self.state_fs().read(rel_path)
    }

    /// 写状态根配置文件。经 `state_fs` 唯一路径。
    fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.state_fs().write(rel_path, data)
    }

    /// 删除状态根配置文件。不存在时 `Ok(())`。经 `state_fs`。
    fn remove_config_file(&self, rel_path: &str) -> Result<()> {
        self.state_fs().remove(rel_path)
    }

    /// 请求设备重启。ESP 实现调用 esp_restart()；host 默认 no-op。
    fn request_restart(&self) {
        log::warn!("request_restart: not implemented on this platform");
    }

    /// 启动 SNTP 时间同步。WiFi 连接后调用。
    fn init_sntp(&self) {
        log::info!("init_sntp: no-op on this platform");
    }

    /// OTA 固件升级。ESP 实现调用 ota_update_from_url；非 ESP 返回错误。
    fn ota_from_url(&self, _url: &str) -> Result<()> {
        Err(crate::error::Error::config(
            "ota",
            "OTA not supported on this platform",
        ))
    }

    /// 初始化显示器硬件。默认 no-op（非显示平台）。
    fn init_display(&self, _config: &DisplayConfig) -> Result<()> {
        Ok(())
    }

    /// 初始化音频硬件（麦克风/喇叭）。默认 no-op（非音频平台）。
    fn init_audio(&self, _config: &AudioSegment) -> Result<()> {
        Ok(())
    }

    /// 麦克风链路是否可用。默认 false。
    fn audio_mic_ready(&self) -> bool {
        false
    }

    /// 喇叭链路是否可用。默认 false。
    fn audio_speaker_ready(&self) -> bool {
        false
    }

    /// 读取 PCM i16 单声道采样帧；返回实际样本数。默认返回不支持错误。
    fn read_mic_pcm_i16(&self, _out: &mut [i16]) -> Result<usize> {
        Err(crate::error::Error::config(
            "audio_mic",
            "Microphone input not supported on this platform",
        ))
    }

    /// 写入 PCM i16 单声道采样帧。默认返回不支持错误。
    fn write_speaker_pcm_i16(&self, _buf: &[i16]) -> Result<()> {
        Err(crate::error::Error::config(
            "audio_speaker",
            "Speaker output not supported on this platform",
        ))
    }

    /// 语音前后处理硬件加速是否可用（如 PDM 专用路径/NPU/向量加速）。
    fn speech_accel_available(&self) -> bool {
        false
    }

    /// 当前语音加速后端标识（不可用时返回 None）。
    fn speech_accel_backend(&self) -> Option<&'static str> {
        None
    }

    /// 显示器是否可用。默认 false。
    fn display_available(&self) -> bool {
        false
    }

    /// 执行显示指令。无显示硬件的默认实现为 no-op `Ok(())`；带可选显示栈的实现（如 ESP/Linux）在
    /// 未成功 `init_display`、内部状态为 `None` 时须返回 `Err`（stage `display`），勿静默成功。
    fn display_command(&self, _cmd: DisplayCommand) -> Result<()> {
        Ok(())
    }

    /// 设置显示器背光开关。默认 no-op `Ok`；可选显示栈在未初始化时返回 `Err`（`display`）。
    /// Set display backlight on/off. Default no-op.
    fn set_display_backlight(&self, _on: bool) -> Result<()> {
        Ok(())
    }

    /// 背光控制是否可用（需有 BL 引脚且显示器已初始化）。默认 false。
    /// Whether backlight control is available. Default false.
    fn display_backlight_available(&self) -> bool {
        false
    }

    /// 设置显示器背光亮度（0-100%）。默认 no-op；可选显示栈未初始化时返回 `Err`（`display`）。
    /// Set display backlight brightness (0-100%). Default no-op.
    fn set_display_backlight_brightness(&self, _percent: u8) -> Result<()> {
        Ok(())
    }

    /// 背光渐变（阻塞，在调用线程执行）。默认 no-op；可选显示栈未初始化时返回 `Err`（`display`）。
    /// Fade display backlight from `from`% to `to`% over `duration_ms`. Blocking. Default no-op.
    fn fade_display_backlight(&self, _from: u8, _to: u8, _duration_ms: u32) -> Result<()> {
        Ok(())
    }

    /// 按 `I2cBusConfig` 初始化 I2C master 总线（ESP-IDF `i2c_master.h`）。默认 no-op（Linux / 未用 I2C）。
    /// Initialize I2C master bus from config. Default no-op.
    fn init_i2c(&self, _config: &crate::config::I2cBusConfig) -> Result<()> {
        Ok(())
    }

    /// I2C 读取：从指定地址的寄存器读取数据。默认返回不支持错误。
    /// I2C read: read data from register at given address. Default returns unsupported error.
    fn i2c_read(&self, _addr: u8, _register: u8, _len: usize) -> Result<Vec<u8>> {
        Err(crate::error::Error::config(
            "i2c_read",
            "I2C not supported on this platform",
        ))
    }

    /// I2C 写入：向指定地址的寄存器写入数据。默认返回不支持错误。
    /// I2C write: write data to register at given address. Default returns unsupported error.
    fn i2c_write(&self, _addr: u8, _register: u8, _data: &[u8]) -> Result<()> {
        Err(crate::error::Error::config(
            "i2c_write",
            "I2C not supported on this platform",
        ))
    }

    /// GPIO 输出；语义同 `hardware_drivers::drive_gpio_out`。
    fn drive_gpio_out(&self, _pins: &PinConfig, _params: &Value) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_gpio_out",
            "GPIO output not supported on this platform",
        ))
    }

    /// GPIO 输入读取；语义同 `hardware_drivers::drive_gpio_in`。
    fn drive_gpio_in(
        &self,
        _pins: &PinConfig,
        _params: &Value,
        _options: &Value,
    ) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_gpio_in",
            "GPIO input not supported on this platform",
        ))
    }

    /// PWM 输出；语义同 `hardware_drivers::drive_pwm_out`。
    fn drive_pwm_out(
        &self,
        _pins: &PinConfig,
        _params: &Value,
        _options: &Value,
        _ledc_channel: u8,
        _ledc_timer_index: u8,
    ) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_pwm_out",
            "PWM output not supported on this platform",
        ))
    }

    /// ADC 采样；语义同 `hardware_drivers::drive_adc_in`。
    fn drive_adc_in(&self, _pins: &PinConfig, _params: &Value, _options: &Value) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_adc_in",
            "ADC not supported on this platform",
        ))
    }

    /// 蜂鸣器；语义同 `hardware_drivers::drive_buzzer`。
    fn drive_buzzer(&self, _pins: &PinConfig, _params: &Value) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_buzzer",
            "Buzzer not supported on this platform",
        ))
    }

    /// DHT 系列温湿度传感器读取。`params` 为 unused placeholder，传空 JSON 即可。
    /// Read DHT series sensor. `params` is an unused placeholder; pass empty JSON object.
    fn drive_dht(&self, _pins: &PinConfig, _params: &Value, _options: &Value) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_dht",
            "DHT sensor not supported on this platform",
        ))
    }

    /// 通用 I2C 传感器读取（SHT3x / AHT20 / raw）；内部完成测量命令、等待、读回与解析，返回 JSON。
    /// Generic I2C sensor read; returns JSON with temperature/humidity or raw hex for `raw` model.
    fn drive_i2c_sensor(
        &self,
        _addr: u8,
        _model: &str,
        _watch_field: &str,
        _options: &Value,
    ) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_i2c_sensor",
            "I2C sensor not supported on this platform",
        ))
    }
}
