//! 编译时/环境变量配置，加载后校验；密钥与敏感字段永不打印、不写存储空间。
//! NVS 仅存系统小键；LLM/通道存储在 storage，由 ConfigFileStore 读写。
//! Build-time / env config with validation; secrets never logged or written to storage.

use crate::display::{
    default_disabled_display_config, is_framebuffer_config, validate_display_config_core,
    DisplayConfig, DISPLAY_CONFIG_VERSION,
};
use crate::error::{Error, Result};
#[cfg(feature = "capability_office")]
use crate::office::{
    OfficeAccountRegistry, OfficeCredentialStore, OfficeCredentialsSegment, OfficeSelectionPolicy,
};
use crate::platform::ConfigStore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

static CHANNELS_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_channels_config() -> std::sync::MutexGuard<'static, ()> {
    CHANNELS_CONFIG_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// 存储读出的单体 JSON：默认严格解析（与历史行为一致）；仅当整段非法而「第一个顶层值」仍合法时降级
/// （典型：短写未截断导致尾部旧字节 → `trailing characters`），并打 warn，避免静默掩盖其它错误。
fn deserialize_storage_json_loose_tail<T: DeserializeOwned>(
    s: &str,
) -> std::result::Result<T, serde_json::Error> {
    let s = s.trim_start();
    match serde_json::from_str::<T>(s) {
        Ok(v) => Ok(v),
        Err(e_strict) => {
            let mut iter = serde_json::Deserializer::from_str(s).into_iter::<T>();
            match iter.next() {
                Some(Ok(v)) => {
                    log::warn!(
                        "[config] storage JSON strict parse failed ({}); accepted first top-level value only — re-save config or check flash write",
                        e_strict
                    );
                    Ok(v)
                }
                Some(Err(_)) | None => Err(e_strict),
            }
        }
    }
}

fn load_storage_json_merge<F>(
    reader: Option<&dyn ConfigFileStore>,
    rel_path: &'static str,
    read_error_code: &'static str,
    load_errors: &mut Vec<String>,
    merge: F,
) where
    F: FnOnce(&str, &mut Vec<String>),
{
    let Some(reader) = reader else {
        return;
    };
    match reader.read_config_file(rel_path) {
        Ok(Some(b)) => {
            let s = String::from_utf8_lossy(&b);
            if s.trim().is_empty() {
                return;
            }
            merge(&s, load_errors);
        }
        Ok(None) => {}
        Err(_) => {
            load_errors.push(read_error_code.into());
        }
    }
}

/// 存储配置文件读写，用于 config/llm.json、config/channels.json。由 Platform 实现。
pub trait ConfigFileStore: Send + Sync {
    fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>>;
    fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()>;
    fn remove_config_file(&self, rel_path: &str) -> Result<()>;
}

/// NVS 配置命名空间，与 platform::nvs 一致。若 NVS 中存在某 key 则 load 时覆盖 env 值。
pub const NVS_NAMESPACE: &str = "pc_cfg";

const NVS_KEY_WIFI_SSID: &str = "wifi_ssid";
const NVS_KEY_WIFI_PASS: &str = "wifi_pass";
const NVS_KEY_PROXY_URL: &str = "proxy_url";
/// 界面语言，单独 NVS 键；zh / en，默认 zh。
pub const NVS_KEY_LOCALE: &str = "locale";

/// 单键 NVS 最大长度（字节）；llm_sources JSON 超此长度时 save_to_nvs 返回错误。
pub const NVS_MAX_VALUE_LEN: usize = 512;

/// 配置字段长度上界（wifi/通道/LLM 单字段）。校验统一引用，避免魔法数。
pub const CONFIG_FIELD_MAX_LEN: usize = 64;
/// URL 类字段长度上界（如 wecom_ws_url）。
pub const CONFIG_URL_MAX_LEN: usize = 512;
pub const CONFIG_ACCOUNT_KEY_MAX_LEN: usize = 64;
pub const CONFIG_ACCOUNT_LABEL_MAX_LEN: usize = 64;
pub const CONFIG_PROVIDER_KIND_MAX_LEN: usize = 64;
pub const CONFIG_EXTERNAL_ACCOUNT_ID_MAX_LEN: usize = 128;
pub const CONFIG_OFFICE_ACCOUNT_LIMIT: usize = 16;

#[cfg(any(test, feature = "cli", feature = "capability_office"))]
fn validate_field_len(s: &str, max: usize, field_name: &str) -> Result<()> {
    if s.len() > max {
        Err(Error::config(
            "config",
            format!("{} length must be <= {}", field_name, max),
        ))
    } else {
        Ok(())
    }
}

/// LLM 源 api_url 长度上界。
pub const CONFIG_LLM_API_URL_MAX: usize = 256;
pub const CONFIG_LLM_SOURCE_ID_MAX: usize = 64;
pub const CONFIG_LLM_HEADER_NAME_MAX: usize = 64;
pub const CONFIG_LLM_HEADER_VALUE_MAX: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmModelKind {
    Text,
    Multimodal,
    ImageGeneration,
    VideoGeneration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmHeaderEntry {
    pub name: String,
    pub value: String,
}

/// 单个 LLM 源配置；与原有 api_key/model/model_provider/api_url 同语义。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmSource {
    pub id: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub api_url: String,
    /// 单次响应最大 token 数；None 时由各客户端使用内置默认值（1024）。
    #[serde(default)]
    pub max_tokens: Option<u32>,
    pub model_kind: LlmModelKind,
    pub custom_headers: Vec<LlmHeaderEntry>,
}

/// NVS 仅存系统小键；LLM/通道存储在 storage config/llm.json、config/channels.json。
pub(crate) const NVS_ALL_KEYS: &[&str] = &[
    NVS_KEY_WIFI_SSID,
    NVS_KEY_WIFI_PASS,
    NVS_KEY_PROXY_URL,
    NVS_KEY_LOCALE,
];

/// 应用配置。由 main 加载一次并通过参数下传；对外只暴露不可变结构体。
/// App config. Load once in main and pass by reference; immutable.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    // WiFi
    pub wifi_ssid: String,
    pub wifi_pass: String,

    // Telegram
    pub tg_token: String,
    /// 逗号分隔的 chat_id 白名单；空则拒绝所有入站。环境变量 BEETLE_TG_ALLOWED_CHAT_IDS。
    pub tg_allowed_chat_ids: String,

    // Feishu
    pub feishu_app_id: String,
    pub feishu_app_secret: String,
    /// 逗号分隔的 chat_id 白名单；空则拒绝所有入站。环境变量 BEETLE_FEISHU_ALLOWED_CHAT_IDS。
    pub feishu_allowed_chat_ids: String,

    /// 钉钉 Stream Mode Client ID；用于注册官方长连接。
    #[serde(default)]
    pub dingtalk_client_id: String,
    /// 钉钉 Stream Mode Client Secret；用于注册官方长连接。
    #[serde(default)]
    pub dingtalk_client_secret: String,

    /// 企业微信 AI Bot BotID；用于官方长连接订阅。
    #[serde(default)]
    pub wecom_bot_id: String,
    /// 企业微信 AI Bot Secret；用于官方长连接订阅。
    #[serde(default)]
    pub wecom_bot_secret: String,
    /// 企业微信 AI Bot 长连接地址；为空时使用官方默认 `wss://openws.work.weixin.qq.com`。
    #[serde(default)]
    pub wecom_ws_url: String,

    /// QQ 频道机器人 App ID；与 qq_channel_secret 均非空时启用回调与出站。
    #[serde(default)]
    pub qq_channel_app_id: String,
    /// QQ 频道机器人 Bot Secret（用于 Ed25519 验签与 getAppAccessToken）。
    #[serde(default)]
    pub qq_channel_secret: String,

    // LLM
    pub api_key: String,
    pub model: String,
    pub model_provider: String,
    /// OpenAI 兼容端点 base URL，如 https://api.openai.com/v1；仅 model_provider 为 openai/openai_compatible 时使用，空则默认 OpenAI。
    pub api_url: String,

    /// 代理 URL，如 http://proxy.example.com:8080；留空直连。
    pub proxy_url: String,

    // Search
    pub search_key: String,
    pub tavily_key: String,

    /// 群组触发：mention = 仅被 @ 时回复；always = 每条都处理，无需回复时输出 SILENT。默认 mention。
    #[serde(default = "default_tg_group_activation")]
    pub tg_group_activation: String,

    /// Webhook 是否启用；与 webhook_token 配合，空 token 或 false 时拒绝 POST /api/webhook。
    #[serde(default)]
    pub webhook_enabled: bool,
    /// Webhook 校验 token；请求头 X-Webhook-Token 或 query token 需与此一致。
    #[serde(default)]
    pub webhook_token: String,

    /// 当前启用的通道（仅一个）："" | "telegram" | "feishu" | "dingtalk" | "wecom" | "qq_channel"。空表示不启用任何通道。
    #[serde(default)]
    pub enabled_channel: String,

    /// 多 LLM 源（回退顺序）；空时由 load 从 api_key/model/model_provider/api_url 构造单源。
    #[serde(default)]
    pub llm_sources: Vec<LlmSource>,

    /// 界面语言 "zh" | "en"；存 NVS 键 locale，GET /api/config/system 与前端一致。
    #[serde(default)]
    pub locale: Option<String>,

    /// 硬件设备配置（从 storage config/hardware.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub hardware_devices: Vec<DeviceEntry>,

    /// I2C 总线配置（从 storage config/hardware.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub i2c_bus: Option<I2cBusConfig>,
    /// I2S 总线配置（从 storage config/hardware.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub i2s_bus: Option<I2sBusConfig>,
    /// I2C 设备列表（从 storage config/hardware.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub i2c_devices: Vec<I2cDeviceEntry>,
    /// I2C 温湿度等传感器条目（从 storage config/hardware.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub i2c_sensors: Vec<I2cSensorEntry>,

    /// 显示配置（从 storage config/display.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub display: Option<DisplayConfig>,
    /// 音频配置（从 storage config/audio.json 加载），不序列化到 NVS。
    #[serde(skip, default)]
    pub audio: Option<AudioSegment>,
    /// 办公账户配置（从 storage config/accounts.json 加载）。
    #[cfg(feature = "capability_office")]
    #[serde(default)]
    pub office_accounts: OfficeAccountsSegment,

    /// 加载过程中产生的可观测错误（NVS/storage/JSON 解析），仅 load() 内写入，不序列化。
    #[serde(skip, default)]
    pub load_errors: Option<Vec<String>>,
}

fn default_tg_group_activation() -> String {
    "mention".into()
}

impl AppConfig {
    /// 从编译时环境变量加载。构建前可设置 e.g. BEETLE_WIFI_SSID。
    /// Load from compile-time env (option_env!). Set e.g. BEETLE_WIFI_SSID before build.
    pub fn load_from_env() -> Self {
        Self {
            wifi_ssid: option_env!("BEETLE_WIFI_SSID").unwrap_or("").into(),
            wifi_pass: option_env!("BEETLE_WIFI_PASS").unwrap_or("").into(),
            tg_token: option_env!("BEETLE_TG_TOKEN").unwrap_or("").into(),
            tg_allowed_chat_ids: option_env!("BEETLE_TG_ALLOWED_CHAT_IDS")
                .unwrap_or("")
                .into(),
            feishu_app_id: option_env!("BEETLE_FEISHU_APP_ID").unwrap_or("").into(),
            feishu_app_secret: option_env!("BEETLE_FEISHU_APP_SECRET").unwrap_or("").into(),
            feishu_allowed_chat_ids: option_env!("BEETLE_FEISHU_ALLOWED_CHAT_IDS")
                .unwrap_or("")
                .into(),
            dingtalk_client_id: option_env!("BEETLE_DINGTALK_CLIENT_ID")
                .unwrap_or("")
                .into(),
            dingtalk_client_secret: option_env!("BEETLE_DINGTALK_CLIENT_SECRET")
                .unwrap_or("")
                .into(),
            wecom_bot_id: option_env!("BEETLE_WECOM_BOT_ID").unwrap_or("").into(),
            wecom_bot_secret: option_env!("BEETLE_WECOM_BOT_SECRET").unwrap_or("").into(),
            wecom_ws_url: option_env!("BEETLE_WECOM_WS_URL").unwrap_or("").into(),
            api_key: option_env!("BEETLE_API_KEY").unwrap_or("").into(),
            model: option_env!("BEETLE_MODEL")
                .unwrap_or("claude-opus-4-5")
                .into(),
            model_provider: option_env!("BEETLE_MODEL_PROVIDER")
                .unwrap_or("anthropic")
                .into(),
            api_url: option_env!("BEETLE_API_URL").unwrap_or("").into(),
            proxy_url: option_env!("BEETLE_PROXY_URL").unwrap_or("").into(),
            search_key: option_env!("BEETLE_SEARCH_KEY").unwrap_or("").into(),
            tavily_key: option_env!("BEETLE_TAVILY_KEY").unwrap_or("").into(),
            tg_group_activation: match option_env!("BEETLE_TG_GROUP_ACTIVATION") {
                Some("always") => "always".into(),
                _ => "mention".into(),
            },
            webhook_enabled: option_env!("BEETLE_WEBHOOK_ENABLED")
                .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            webhook_token: option_env!("BEETLE_WEBHOOK_TOKEN").unwrap_or("").into(),
            enabled_channel: option_env!("BEETLE_ENABLED_CHANNEL").unwrap_or("").into(),
            qq_channel_app_id: option_env!("BEETLE_QQ_CHANNEL_APP_ID").unwrap_or("").into(),
            qq_channel_secret: option_env!("BEETLE_QQ_CHANNEL_SECRET").unwrap_or("").into(),
            llm_sources: vec![],
            locale: option_env!("BEETLE_LOCALE")
                .filter(|s| *s == "zh" || *s == "en")
                .map(String::from),
            hardware_devices: vec![],
            i2c_bus: None,
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![],
            display: None,
            audio: None,
            #[cfg(feature = "capability_office")]
            office_accounts: OfficeAccountsSegment::default(),
            load_errors: None,
        }
    }

    /// 加载过程中产生的错误码列表（nvs_read_failed / storage_*_unavailable / *_json_invalid），供 health/diagnose 或日志可观测。
    pub fn load_errors(&self) -> &[String] {
        self.load_errors.as_deref().unwrap_or(&[])
    }

    /// 多源加载：先 load_from_env()，再 NVS 系统键覆盖，再可选从 reader 读 storage llm/channels 合并。
    pub fn load(store: &dyn ConfigStore, reader: Option<&dyn ConfigFileStore>) -> Self {
        let mut c = Self::load_from_env();
        let mut load_errors = Vec::new();
        let values = match store.read_strings(NVS_ALL_KEYS) {
            Ok(v) => v,
            Err(_) => {
                log::warn!("[config] NVS read_strings failed");
                load_errors.push("nvs_read_failed".into());
                Vec::new()
            }
        };
        let opt = |i: usize| values.get(i).and_then(|v| v.as_ref());
        // NVS 系统键：wifi_ssid, wifi_pass, proxy_url, locale
        if let Some(s) = opt(0) {
            if !s.is_empty() {
                c.wifi_ssid = s.clone();
            }
        }
        if let Some(s) = opt(1) {
            if !s.is_empty() {
                c.wifi_pass = s.clone();
            }
        }
        if let Some(s) = opt(2) {
            if !s.is_empty() {
                c.proxy_url = s.clone();
            }
        }
        if let Some(s) = opt(3) {
            if s == "zh" || s == "en" {
                c.locale = Some(s.clone());
            }
        }
        load_storage_json_merge(
            reader,
            "config/llm.json",
            "storage_llm_read_error",
            &mut load_errors,
            |json, errors| c.merge_llm_from_json(json, errors),
        );
        load_storage_json_merge(
            reader,
            "config/channels.json",
            "storage_channels_read_error",
            &mut load_errors,
            |json, errors| c.merge_channels_from_json(json, errors),
        );
        load_storage_json_merge(
            reader,
            "config/hardware.json",
            "storage_hardware_read_error",
            &mut load_errors,
            |json, errors| c.merge_hardware_from_json(json, errors),
        );
        load_storage_json_merge(
            reader,
            "config/display.json",
            "storage_display_read_error",
            &mut load_errors,
            |json, errors| c.merge_display_from_json(json, errors),
        );
        load_storage_json_merge(
            reader,
            "config/audio.json",
            "storage_audio_read_error",
            &mut load_errors,
            |json, errors| c.merge_audio_from_json(json, errors),
        );
        #[cfg(feature = "capability_office")]
        load_storage_json_merge(
            reader,
            "config/accounts.json",
            "storage_accounts_read_error",
            &mut load_errors,
            |json, errors| c.merge_office_accounts_from_json(json, errors),
        );
        sanitize_proxy_url_for_target(
            &mut c,
            proxy_supported_on_current_target(),
            &mut load_errors,
        );
        crate::llm::ensure_legacy_llm_sources(&mut c);
        c.load_errors = if load_errors.is_empty() {
            None
        } else {
            Some(load_errors)
        };
        c
    }

    /// 从 storage 读到的 llm.json 字符串合并到当前 config（仅覆盖 LLM 相关字段）。
    pub fn merge_llm_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match deserialize_storage_json_loose_tail::<LlmSegment>(json) {
            Ok(seg) => {
                if let Err(e) = validate_llm_segment(&seg) {
                    log::warn!("[config] merge_llm_from_json validation failed: {}", e);
                    errors.push("llm_json_invalid".into());
                    return;
                }
                if !seg.llm_sources.is_empty() {
                    self.llm_sources = seg.llm_sources.clone();
                    let first = &self.llm_sources[0];
                    self.api_key = first.api_key.clone();
                    self.model = first.model.clone();
                    self.model_provider = first.provider.clone();
                    self.api_url = first.api_url.clone();
                }
            }
            Err(e) => {
                log::warn!("[config] merge_llm_from_json parse failed: {}", e);
                errors.push("llm_json_invalid".into());
            }
        }
    }

    /// 从 storage 读到的 channels.json 字符串合并到当前 config（仅覆盖通道相关字段）。
    pub fn merge_channels_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match deserialize_storage_json_loose_tail::<ChannelsSegment>(json) {
            Ok(seg) => {
                if let Err(e) = validate_channels_segment_storage_merge_fields(&seg) {
                    log::warn!("[config] merge_channels_from_json validation failed: {}", e);
                    errors.push("channels_json_invalid".into());
                    return;
                }
                self.tg_group_activation = seg.tg_group_activation;
                self.tg_token = seg.tg_token;
                self.tg_allowed_chat_ids = seg.tg_allowed_chat_ids;
                self.feishu_app_id = seg.feishu_app_id;
                self.feishu_app_secret = seg.feishu_app_secret;
                self.feishu_allowed_chat_ids = seg.feishu_allowed_chat_ids;
                self.dingtalk_client_id = seg.dingtalk_client_id;
                self.dingtalk_client_secret = seg.dingtalk_client_secret;
                self.wecom_bot_id = seg.wecom_bot_id;
                self.wecom_bot_secret = seg.wecom_bot_secret;
                self.wecom_ws_url = seg.wecom_ws_url;
                self.qq_channel_app_id = seg.qq_channel_app_id;
                self.qq_channel_secret = seg.qq_channel_secret;
                self.webhook_enabled = seg.webhook_enabled;
                self.webhook_token = seg.webhook_token;
                self.enabled_channel = seg.enabled_channel;
            }
            Err(e) => {
                log::warn!("[config] merge_channels_from_json parse failed: {}", e);
                errors.push("channels_json_invalid".into());
            }
        }
    }

    /// 从 storage 读到的 hardware.json 字符串合并到当前 config（仅覆盖硬件设备列表）。
    /// 解析成功后校验；校验失败则不覆盖、保留空列表，并记录 hardware_validation_failed。
    pub fn merge_hardware_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match deserialize_storage_json_loose_tail::<HardwareSegment>(json) {
            Ok(seg) => {
                if let Err(e) = validate_hardware_segment(&seg) {
                    log::warn!("[config] merge_hardware_from_json validation failed: {}", e);
                    errors.push("hardware_validation_failed".into());
                    return;
                }
                self.hardware_devices = seg.hardware_devices;
                self.i2c_bus = seg.i2c_bus;
                self.i2s_bus = seg.i2s_bus;
                self.i2c_devices = seg.i2c_devices;
                self.i2c_sensors = seg.i2c_sensors;
            }
            Err(e) => {
                log::warn!("[config] merge_hardware_from_json parse failed: {}", e);
                errors.push("hardware_json_invalid".into());
            }
        }
    }

    /// 从 storage 读到的 display.json 字符串合并到当前 config。
    pub fn merge_display_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match deserialize_storage_json_loose_tail::<DisplayConfig>(json) {
            Ok(mut cfg) => {
                if cfg.version == 0 {
                    cfg.version = DISPLAY_CONFIG_VERSION;
                }
                if let Err(e) = validate_display_segment(&cfg, &self.hardware_devices) {
                    log::warn!("[config] merge_display_from_json validation failed: {}", e);
                    errors.push("display_validation_failed".into());
                    return;
                }
                self.display = Some(cfg);
            }
            Err(e) => {
                log::warn!("[config] merge_display_from_json parse failed: {}", e);
                errors.push("display_json_invalid".into());
            }
        }
    }

    /// 从 storage 读到的 audio.json 字符串合并到当前 config。
    pub fn merge_audio_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match deserialize_storage_json_loose_tail::<AudioSegment>(json) {
            Ok(mut seg) => {
                if seg.version == 0 {
                    seg.version = AUDIO_CONFIG_VERSION;
                }
                normalize_audio_segment(&mut seg);
                if let Err(e) = validate_audio_segment(&seg) {
                    log::warn!("[config] merge_audio_from_json validation failed: {}", e);
                    errors.push("audio_validation_failed".into());
                    return;
                }
                let hardware = HardwareSegment::from_app_config(self);
                if let Err(e) = validate_audio_hardware_pair("audio", &seg, &hardware) {
                    log::warn!(
                        "[config] merge_audio_from_json pair validation failed: {}",
                        e
                    );
                    errors.push("audio_validation_failed".into());
                    return;
                }
                self.audio = Some(seg);
            }
            Err(e) => {
                log::warn!("[config] merge_audio_from_json parse failed: {}", e);
                errors.push("audio_json_invalid".into());
            }
        }
    }

    #[cfg(feature = "capability_office")]
    pub fn merge_office_accounts_from_json(&mut self, json: &str, errors: &mut Vec<String>) {
        match serde_json::from_str::<OfficeAccountsSegment>(json) {
            Ok(seg) => {
                self.office_accounts = seg;
            }
            Err(e) => {
                log::warn!(
                    "[config] merge_office_accounts_from_json parse failed: {}",
                    e
                );
                errors.push("accounts_json_invalid".into());
            }
        }
    }
}

/// 将 Platform 转为 ConfigFileStore，供 load/save 使用。
pub struct PlatformConfigFileStore(pub std::sync::Arc<dyn crate::Platform>);

impl ConfigFileStore for PlatformConfigFileStore {
    fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
        self.0.read_config_file(rel_path)
    }
    fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
        self.0.write_config_file(rel_path, data)
    }
    fn remove_config_file(&self, rel_path: &str) -> Result<()> {
        self.0.remove_config_file(rel_path)
    }
}

/// 清空配置区（store 内所有已知 key），重启后 load 仅来自 env。
pub fn reset_to_defaults(store: &dyn ConfigStore) -> Result<()> {
    store.erase_keys(NVS_ALL_KEYS)
}

/// 校验 Telegram 群组触发策略；value 仅允许 "mention" 或 "always"。
pub fn validate_tg_group_activation(value: &str) -> Result<()> {
    if value != "mention" && value != "always" {
        return Err(Error::config(
            "tg_group_activation",
            "value must be 'mention' or 'always'",
        ));
    }
    Ok(())
}

impl AppConfig {
    /// 校验：启动 WiFi 前必须提供 SSID（密码可为空用于开放网络）。
    pub fn validate_for_wifi(&self) -> Result<()> {
        if self.wifi_ssid.is_empty() {
            return Err(Error::config("config", "wifi_ssid is required for WiFi"));
        }
        if self.wifi_ssid.len() > CONFIG_FIELD_MAX_LEN {
            return Err(Error::config(
                "config",
                format!("wifi_ssid length must be <= {}", CONFIG_FIELD_MAX_LEN),
            ));
        }
        if self.wifi_pass.len() > CONFIG_FIELD_MAX_LEN {
            return Err(Error::config(
                "config",
                format!("wifi_pass length must be <= {}", CONFIG_FIELD_MAX_LEN),
            ));
        }
        Ok(())
    }

    /// 校验：proxy_url 为空或形如 scheme://host 或 scheme://host:port。
    pub fn validate_proxy(&self) -> Result<()> {
        validate_proxy_url_for_target(self.proxy_url.trim(), proxy_supported_on_current_target())
    }

    /// 启动期通道校验：enabled_channel 对应凭证非空且长度在界内；失败返回 Config 错误，不打印凭证。
    pub fn validate_for_channels(&self) -> Result<()> {
        let ch = crate::normalize_compiled_enabled_channel(&self.enabled_channel);
        match ch {
            "telegram" => {
                if self.tg_token.trim().is_empty() {
                    return Err(Error::config(
                        "config",
                        "enabled_channel=telegram requires tg_token",
                    ));
                }
                if self.tg_token.len() > CONFIG_FIELD_MAX_LEN {
                    return Err(Error::config(
                        "config",
                        format!("tg_token length must be <= {}", CONFIG_FIELD_MAX_LEN),
                    ));
                }
            }
            "feishu" => {
                if self.feishu_app_id.trim().is_empty() || self.feishu_app_secret.trim().is_empty()
                {
                    return Err(Error::config(
                        "config",
                        "enabled_channel=feishu requires feishu_app_id and feishu_app_secret",
                    ));
                }
                if self.feishu_app_id.len() > CONFIG_FIELD_MAX_LEN
                    || self.feishu_app_secret.len() > CONFIG_FIELD_MAX_LEN
                {
                    return Err(Error::config(
                        "config",
                        format!("feishu field length must be <= {}", CONFIG_FIELD_MAX_LEN),
                    ));
                }
            }
            "dingtalk" => {
                if self.dingtalk_client_id.trim().is_empty()
                    || self.dingtalk_client_secret.trim().is_empty()
                {
                    return Err(Error::config(
                        "config",
                        "enabled_channel=dingtalk requires dingtalk_client_id and dingtalk_client_secret",
                    ));
                }
                if self.dingtalk_client_id.len() > CONFIG_FIELD_MAX_LEN
                    || self.dingtalk_client_secret.len() > CONFIG_FIELD_MAX_LEN
                {
                    return Err(Error::config(
                        "config",
                        format!("dingtalk field length must be <= {}", CONFIG_FIELD_MAX_LEN),
                    ));
                }
            }
            "wecom" => {
                if self.wecom_bot_id.trim().is_empty() || self.wecom_bot_secret.trim().is_empty() {
                    return Err(Error::config(
                        "config",
                        "enabled_channel=wecom requires wecom_bot_id and wecom_bot_secret",
                    ));
                }
                if self.wecom_bot_id.len() > CONFIG_FIELD_MAX_LEN
                    || self.wecom_bot_secret.len() > CONFIG_FIELD_MAX_LEN
                {
                    return Err(Error::config(
                        "config",
                        format!("wecom field length must be <= {}", CONFIG_FIELD_MAX_LEN),
                    ));
                }
                if self.wecom_ws_url.len() > CONFIG_URL_MAX_LEN {
                    return Err(Error::config(
                        "config",
                        format!("wecom_ws_url length must be <= {}", CONFIG_URL_MAX_LEN),
                    ));
                }
            }
            "qq_channel" => {
                if self.qq_channel_app_id.trim().is_empty()
                    || self.qq_channel_secret.trim().is_empty()
                {
                    return Err(Error::config(
                        "config",
                        "enabled_channel=qq_channel requires qq_channel_app_id and qq_channel_secret",
                    ));
                }
                if self.qq_channel_app_id.len() > CONFIG_FIELD_MAX_LEN
                    || self.qq_channel_secret.len() > CONFIG_FIELD_MAX_LEN
                {
                    return Err(Error::config(
                        "config",
                        format!(
                            "qq_channel field length must be <= {}",
                            CONFIG_FIELD_MAX_LEN
                        ),
                    ));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// 从 proxy_url（如 http://host:8080）解析出 (host, port)，供 HTTP 客户端使用。
/// 拒绝 after_scheme 以 ':' 开头（如误配 http://:host）以免得到 host=":host" 导致底层 getaddrinfo 报错。
pub fn parse_proxy_url_to_host_port(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let after_scheme = url.find("://").and_then(|i| url.get((i + 3)..))?;
    if after_scheme.is_empty() || after_scheme.starts_with(':') {
        return None;
    }
    let scheme = url.get(..url.find("://")?).unwrap_or("http");
    let default_port = match scheme {
        "https" => "443",
        "socks4" | "socks5" => "1080",
        _ => "80",
    };
    let (host, port) = if let Some(col) = after_scheme.rfind(':') {
        let (h, p) = after_scheme.split_at(col);
        let p = p.trim_start_matches(':');
        if h.is_empty() {
            return None;
        }
        if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            (h, p.to_string())
        } else {
            (after_scheme, default_port.to_string())
        }
    } else {
        (after_scheme, default_port.to_string())
    };
    if host.is_empty() || host.starts_with(':') {
        return None;
    }
    Some((host.to_string(), port))
}

/// Return whether the current compile target has a working HTTP proxy transport.
///
/// ESP targets currently do not implement CONNECT tunneling, so accepting
/// `proxy_url` there would create a client that fails every request.
pub fn proxy_supported_on_current_target() -> bool {
    !cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
}

/// Validate `proxy_url` using an explicit target capability flag.
///
/// The flag keeps tests independent from the host target while production uses
/// [`proxy_supported_on_current_target`].
pub fn validate_proxy_url_for_target(proxy_url: &str, proxy_supported: bool) -> Result<()> {
    let proxy_url = proxy_url.trim();
    if proxy_url.is_empty() {
        return Ok(());
    }
    parse_proxy_url_to_host_port(proxy_url).ok_or_else(|| {
        Error::config("config", "proxy_url must be empty or like http://host:port")
    })?;
    if !proxy_supported {
        return Err(Error::config(
            "config",
            "proxy_url is not supported on this target",
        ));
    }
    Ok(())
}

fn sanitize_proxy_url_for_target(
    config: &mut AppConfig,
    proxy_supported: bool,
    load_errors: &mut Vec<String>,
) {
    let proxy_url = config.proxy_url.trim();
    if proxy_url.is_empty() {
        return;
    }
    if validate_proxy_url_for_target(proxy_url, proxy_supported).is_ok() {
        return;
    }
    let error_code = if parse_proxy_url_to_host_port(proxy_url).is_some() && !proxy_supported {
        "proxy_unsupported_on_target"
    } else {
        "proxy_url_invalid"
    };
    log::warn!("[config] dropping unsupported proxy_url ({})", error_code);
    config.proxy_url.clear();
    load_errors.push(error_code.to_string());
}

// 以下仍属 impl AppConfig（与 parse_proxy_url_to_host_port 并列的 impl 块继续）
impl AppConfig {
    /// 序列化为 JSON，供 CLI 使用。
    /// NOTE: 含明文密钥，仅限本地使用，不得用于日志或公开接口。
    /// For local use only; contains plaintext secrets.
    #[cfg(feature = "cli")]
    pub fn to_full_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| Error::config("serialize", e.to_string()))
    }

    /// 从 JSON 反序列化并校验（validate_for_wifi、validate_proxy、tg_group_activation、llm_sources）。
    #[cfg(any(test, feature = "cli"))]
    pub fn from_json_and_validate(body: &[u8]) -> Result<Self> {
        let mut c: AppConfig = serde_json::from_slice(body)
            .map_err(|e| Error::config("deserialize", e.to_string()))?;
        validate_field_len(&c.wifi_ssid, CONFIG_FIELD_MAX_LEN, "wifi_ssid")?;
        validate_field_len(&c.wifi_pass, CONFIG_FIELD_MAX_LEN, "wifi_pass")?;
        validate_field_len(&c.tg_token, CONFIG_FIELD_MAX_LEN, "tg_token")?;
        validate_field_len(&c.feishu_app_id, CONFIG_FIELD_MAX_LEN, "feishu_app_id")?;
        validate_field_len(
            &c.feishu_app_secret,
            CONFIG_FIELD_MAX_LEN,
            "feishu_app_secret",
        )?;
        validate_field_len(&c.api_key, CONFIG_FIELD_MAX_LEN, "api_key")?;
        validate_field_len(&c.search_key, CONFIG_FIELD_MAX_LEN, "search_key")?;
        validate_field_len(&c.tavily_key, CONFIG_FIELD_MAX_LEN, "tavily_key")?;
        validate_field_len(&c.webhook_token, CONFIG_FIELD_MAX_LEN, "webhook_token")?;
        validate_field_len(
            &c.qq_channel_app_id,
            CONFIG_FIELD_MAX_LEN,
            "qq_channel_app_id",
        )?;
        validate_field_len(
            &c.qq_channel_secret,
            CONFIG_FIELD_MAX_LEN,
            "qq_channel_secret",
        )?;
        validate_field_len(
            &c.dingtalk_client_id,
            CONFIG_FIELD_MAX_LEN,
            "dingtalk_client_id",
        )?;
        validate_field_len(
            &c.dingtalk_client_secret,
            CONFIG_FIELD_MAX_LEN,
            "dingtalk_client_secret",
        )?;
        validate_field_len(&c.wecom_bot_id, CONFIG_FIELD_MAX_LEN, "wecom_bot_id")?;
        validate_field_len(
            &c.wecom_bot_secret,
            CONFIG_FIELD_MAX_LEN,
            "wecom_bot_secret",
        )?;
        validate_field_len(&c.wecom_ws_url, CONFIG_URL_MAX_LEN, "wecom_ws_url")?;
        crate::llm::ensure_legacy_llm_sources(&mut c);
        validate_llm_sources(&c.llm_sources)?;
        if c.tg_group_activation != "mention" && c.tg_group_activation != "always" {
            return Err(Error::config(
                "config",
                "tg_group_activation must be 'mention' or 'always'",
            ));
        }
        if !c.wifi_ssid.is_empty() {
            c.validate_for_wifi()?;
        }
        c.validate_proxy()?;
        Ok(c)
    }
}

/// 将配置按键名逐字段写入 store；单条 value 超 NVS_MAX_VALUE_LEN 返回错误。
/// 仅写入 NVS 保留的系统键；LLM/通道由 save_llm_segment / save_channels_segment 写 storage。
pub fn save_to_nvs(store: &dyn ConfigStore, config: &AppConfig) -> Result<()> {
    let locale = config.locale.as_deref().unwrap_or("zh");
    store.write_strings(&[
        (NVS_KEY_WIFI_SSID, &config.wifi_ssid),
        (NVS_KEY_WIFI_PASS, &config.wifi_pass),
        (NVS_KEY_PROXY_URL, &config.proxy_url),
        (NVS_KEY_LOCALE, locale),
    ])?;
    Ok(())
}

/// 从 store 读取当前 locale；无或非法则返回 "zh"。
pub fn get_locale(store: &dyn ConfigStore) -> String {
    match store.read_string(NVS_KEY_LOCALE) {
        Ok(Some(locale)) => normalize_locale_value(&locale)
            .map(str::to_string)
            .unwrap_or_else(|_| "zh".to_string()),
        _ => "zh".to_string(),
    }
}

/// 写入 locale（仅接受 "zh" 或 "en"）。
pub fn set_locale(store: &dyn ConfigStore, locale: &str) -> Result<()> {
    let locale = normalize_locale_value(locale)?;
    store.write_string(NVS_KEY_LOCALE, locale)?;
    Ok(())
}

fn normalize_locale_value(locale: &str) -> Result<&str> {
    let locale = locale.trim();
    if locale != "zh" && locale != "en" {
        return Err(Error::config("locale", "must be zh or en"));
    }
    Ok(locale)
}

fn normalize_optional_locale(locale: Option<&str>) -> Result<Option<&str>> {
    locale.map(normalize_locale_value).transpose()
}

/// GET/POST /api/config/llm 读写模型。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LlmSegment {
    pub llm_sources: Vec<LlmSource>,
}

impl LlmSegment {
    /// 从运行态 AppConfig 投影出当前 LLM 配置段；保留 legacy 单源字段回退语义。
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            llm_sources: crate::llm::llm_sources_or_legacy(config),
        }
    }
}

fn enabled_channel_validation_message() -> String {
    let mut values = crate::compiled_enabled_channel_ids()
        .iter()
        .copied()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.insert(0, "empty");
    format!("enabled_channel must be one of: {}", values.join(", "))
}

fn is_valid_enabled_channel(s: &str) -> bool {
    crate::compiled_enabled_channel_ids().contains(&s)
}

/// POST /api/config/channels 请求体；仅通道相关字段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelsSegment {
    #[serde(default)]
    pub enabled_channel: String,
    #[serde(default = "default_tg_group_activation")]
    pub tg_group_activation: String,
    #[serde(default)]
    pub tg_token: String,
    #[serde(default)]
    pub tg_allowed_chat_ids: String,
    #[serde(default)]
    pub feishu_app_id: String,
    #[serde(default)]
    pub feishu_app_secret: String,
    #[serde(default)]
    pub feishu_allowed_chat_ids: String,
    #[serde(default)]
    pub dingtalk_client_id: String,
    #[serde(default)]
    pub dingtalk_client_secret: String,
    #[serde(default)]
    pub wecom_bot_id: String,
    #[serde(default)]
    pub wecom_bot_secret: String,
    #[serde(default)]
    pub wecom_ws_url: String,
    #[serde(default)]
    pub qq_channel_app_id: String,
    #[serde(default)]
    pub qq_channel_secret: String,
    #[serde(default)]
    pub webhook_enabled: bool,
    #[serde(default)]
    pub webhook_token: String,
}

impl ChannelsSegment {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            enabled_channel: crate::normalize_compiled_enabled_channel(&config.enabled_channel)
                .to_string(),
            tg_group_activation: config.tg_group_activation.clone(),
            tg_token: config.tg_token.clone(),
            tg_allowed_chat_ids: config.tg_allowed_chat_ids.clone(),
            feishu_app_id: config.feishu_app_id.clone(),
            feishu_app_secret: config.feishu_app_secret.clone(),
            feishu_allowed_chat_ids: config.feishu_allowed_chat_ids.clone(),
            dingtalk_client_id: config.dingtalk_client_id.clone(),
            dingtalk_client_secret: config.dingtalk_client_secret.clone(),
            wecom_bot_id: config.wecom_bot_id.clone(),
            wecom_bot_secret: config.wecom_bot_secret.clone(),
            wecom_ws_url: config.wecom_ws_url.clone(),
            qq_channel_app_id: config.qq_channel_app_id.clone(),
            qq_channel_secret: config.qq_channel_secret.clone(),
            webhook_enabled: config.webhook_enabled,
            webhook_token: config.webhook_token.clone(),
        }
    }
}

/// GET/POST /api/config/system 读写模型。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemSegment {
    #[serde(default)]
    pub wifi_ssid: String,
    #[serde(default)]
    pub wifi_pass: String,
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default)]
    pub locale: Option<String>,
}

impl SystemSegment {
    /// 从运行态 AppConfig 投影出当前系统配置段。
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            wifi_ssid: config.wifi_ssid.clone(),
            wifi_pass: config.wifi_pass.clone(),
            proxy_url: config.proxy_url.clone(),
            locale: config.locale.clone(),
        }
    }
}

/// POST /api/config/accounts 请求体。
#[cfg(feature = "capability_office")]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OfficeAccountsSegment {
    #[serde(default)]
    pub registry: OfficeAccountRegistry,
    #[serde(default)]
    pub policy: OfficeSelectionPolicy,
}

// ── Audio config constants & schema ──
pub const AUDIO_CONFIG_VERSION: u32 = 1;
const AUDIO_SAMPLE_RATE_MIN: u32 = 8_000;
const AUDIO_SAMPLE_RATE_MAX: u32 = 48_000;
const AUDIO_DEVICE_TYPE_MAX_LEN: usize = 32;
const AUDIO_DEVICE_REF_MAX_LEN: usize = 256;
const AUDIO_KEYWORD_MAX_LEN: usize = 64;
const AUDIO_VOICE_MAX_LEN: usize = 64;
const AUDIO_RATE_MAX_LEN: usize = 16;
const AUDIO_PITCH_MAX_LEN: usize = 16;
const AUDIO_SPEECH_API_KEY_MAX_LEN: usize = 256;
const AUDIO_SPEECH_API_SECRET_MAX_LEN: usize = 256;
const AUDIO_SOUND_EVENTS_MAX: usize = 16;
const AUDIO_SOUND_EVENT_MAX_LEN: usize = 32;
const AUDIO_REALTIME_INSTRUCTIONS_MAX_LEN: usize = 1024;
const AUDIO_TOPOLOGY_MAX_LEN: usize = 32;
const AUDIO_MIC_DEVICE_I2S_INMP441: &str = "i2s_inmp441";
/// Maximum length for `wake_word.wake_prompt`.
const AUDIO_WAKE_PROMPT_MAX_LEN: usize = 256;
const AUDIO_MIC_DEVICE_PDM: &str = "pdm";
const AUDIO_SPEAKER_DEVICE_I2S_MAX98357A: &str = "i2s_max98357a";
const AUDIO_SPEAKER_DEVICE_USB: &str = "usb";
pub const AUDIO_TOPOLOGY_DISCRETE_I2S: &str = "discrete_i2s";
pub const AUDIO_TOPOLOGY_I2S_CODEC: &str = "i2s_codec";
pub const AUDIO_CODEC_INPUT_ES7210: &str = "es7210";
pub const AUDIO_CODEC_OUTPUT_ES8311: &str = "es8311";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioMicPins {
    pub ws: i32,
    pub sck: i32,
    pub din: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSpeakerPins {
    pub ws: i32,
    pub sck: i32,
    pub dout: i32,
    #[serde(default)]
    pub sd: Option<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioMicrophoneConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub device_type: String,
    pub pins: AudioMicPins,
    #[serde(default = "default_audio_sample_rate")]
    pub sample_rate: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSpeakerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub device_type: String,
    #[serde(default)]
    pub device_ref: Option<String>,
    #[serde(default)]
    pub pins: Option<AudioSpeakerPins>,
    #[serde(default = "default_audio_sample_rate")]
    pub sample_rate: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioCodecConfig {
    #[serde(default)]
    pub input_codec: Option<String>,
    #[serde(default)]
    pub output_codec: Option<String>,
    #[serde(default)]
    pub input_addr: Option<u8>,
    #[serde(default)]
    pub output_addr: Option<u8>,
    #[serde(default)]
    pub pa_pin: Option<i32>,
    #[serde(default)]
    pub input_reference: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioVadConfig {
    #[serde(default = "default_audio_vad_threshold")]
    pub threshold: f32,
    #[serde(default = "default_audio_vad_silence_ms")]
    pub silence_duration_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioRealtimeConfig {
    #[serde(default = "default_audio_realtime_provider")]
    pub provider: String,
    #[serde(default = "default_audio_realtime_ws_url")]
    pub ws_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_audio_realtime_model")]
    pub model: String,
    #[serde(default = "default_audio_realtime_voice")]
    pub voice: String,
    #[serde(default)]
    pub instructions: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioWakeWordConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Legacy hidden field kept for config compatibility.
    #[serde(default)]
    pub keyword: String,
    #[serde(default = "default_wake_enter_threshold")]
    pub enter_threshold: f32,
    #[serde(default = "default_wake_leave_threshold")]
    pub leave_threshold: f32,
    #[serde(default = "default_wake_reference_suppress_ratio")]
    pub reference_suppress_ratio: f32,
    #[serde(default = "default_wake_zcr_min")]
    pub zcr_min: f32,
    #[serde(default = "default_wake_zcr_max")]
    pub zcr_max: f32,
    #[serde(default = "default_wake_min_speech_band_ratio")]
    pub min_speech_band_ratio: f32,
    #[serde(default = "default_wake_min_active_ms")]
    pub min_active_ms: u32,
    #[serde(default = "default_wake_hangover_ms")]
    pub hangover_ms: u32,
    #[serde(default = "default_wake_cooldown_ms")]
    pub cooldown_ms: u32,
    /// TTS greeting played when wake word fires, before voice capture starts.
    /// 唤醒后 TTS 播报的问候语，播报完毕后开始采集用户语音。
    #[serde(default = "default_wake_prompt")]
    pub wake_prompt: String,
}

fn default_wake_enter_threshold() -> f32 {
    0.18
}

pub(crate) fn default_wake_enter_threshold_for_profile() -> f32 {
    default_wake_enter_threshold()
}

fn default_wake_leave_threshold() -> f32 {
    0.10
}

fn default_wake_reference_suppress_ratio() -> f32 {
    1.35
}

fn default_wake_zcr_min() -> f32 {
    0.02
}

fn default_wake_zcr_max() -> f32 {
    0.25
}

fn default_wake_min_speech_band_ratio() -> f32 {
    0.45
}

fn default_wake_min_active_ms() -> u32 {
    240
}

fn default_wake_hangover_ms() -> u32 {
    500
}

fn default_wake_cooldown_ms() -> u32 {
    1000
}

const ES7210_WAKE_ENTER_THRESHOLD: f32 = 0.01;
const ES7210_WAKE_LEAVE_THRESHOLD: f32 = 0.005;
const ES7210_WAKE_ZCR_MAX: f32 = 0.65;
const ES7210_WAKE_MIN_SPEECH_BAND_RATIO: f32 = 0.35;
const ES7210_WAKE_MIN_ACTIVE_MS: u32 = 120;
const WAKE_FLOAT_EQ_EPSILON: f32 = 0.000_1;

fn default_wake_prompt() -> String {
    "你好，我在听，请说。".to_string()
}

/// 语音服务配置：当前回退链路的识别与合成共用一套服务商、接口和鉴权字段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSpeechConfig {
    #[serde(default)]
    pub api_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_secret: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub language: String,
}

/// 语音合成（TTS）音色与语速等。当前语音服务链路共用 `AudioSpeechConfig` 的提供商与鉴权字段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioTtsConfig {
    #[serde(default)]
    pub voice: String,
    #[serde(default)]
    pub rate: String,
    #[serde(default)]
    pub pitch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioAmbientListeningConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub detect_emotions: bool,
    #[serde(default)]
    pub sound_events: Vec<String>,
    #[serde(default = "default_audio_cooldown_minutes")]
    pub cooldown_minutes: u32,
    #[serde(default = "default_audio_check_interval_seconds")]
    pub check_interval_seconds: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioLedStatesConfig {
    #[serde(default)]
    pub listening: String,
    #[serde(default)]
    pub processing: String,
    #[serde(default)]
    pub speaking: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioLedIndicatorConfig {
    #[serde(default)]
    pub enabled: bool,
    pub pin: i32,
    pub states: AudioLedStatesConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioSegment {
    #[serde(default = "default_audio_config_version")]
    pub version: u32,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_audio_service_provider")]
    pub service_provider: String,
    #[serde(default = "default_audio_topology")]
    pub topology: String,
    pub microphone: AudioMicrophoneConfig,
    pub speaker: AudioSpeakerConfig,
    #[serde(default = "default_audio_codec_config")]
    pub codec: AudioCodecConfig,
    pub vad: AudioVadConfig,
    pub wake_word: AudioWakeWordConfig,
    pub speech: AudioSpeechConfig,
    pub tts: AudioTtsConfig,
    #[serde(default = "default_audio_realtime_config")]
    pub realtime: AudioRealtimeConfig,
    pub ambient_listening: AudioAmbientListeningConfig,
    pub led_indicator: AudioLedIndicatorConfig,
}

fn default_audio_config_version() -> u32 {
    AUDIO_CONFIG_VERSION
}

fn default_audio_sample_rate() -> u32 {
    16_000
}

fn default_audio_topology() -> String {
    AUDIO_TOPOLOGY_DISCRETE_I2S.to_string()
}

fn default_audio_codec_config() -> AudioCodecConfig {
    AudioCodecConfig {
        input_codec: None,
        output_codec: None,
        input_addr: None,
        output_addr: None,
        pa_pin: None,
        input_reference: false,
    }
}

fn default_audio_vad_threshold() -> f32 {
    0.5
}

fn default_audio_vad_silence_ms() -> u32 {
    1000
}

fn default_audio_cooldown_minutes() -> u32 {
    10
}

fn default_audio_check_interval_seconds() -> u32 {
    300
}

/// OpenAI / OpenAI-compatible realtime voice provider.
pub const AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE: &str = "openai_compatible";
/// Alibaba Qwen realtime voice provider.
pub const AUDIO_REALTIME_PROVIDER_QWEN: &str = "qwen";
/// Doubao realtime voice provider.
pub const AUDIO_REALTIME_PROVIDER_DOUBAO: &str = "doubao";

fn audio_realtime_default_ws_url(provider: &str) -> &'static str {
    match provider {
        AUDIO_REALTIME_PROVIDER_QWEN => "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime",
        AUDIO_REALTIME_PROVIDER_DOUBAO => "wss://ai-gateway.vei.volces.com/v1/realtime",
        _ => "wss://api.openai.com/v1/realtime",
    }
}

fn audio_realtime_default_model(provider: &str) -> &'static str {
    match provider {
        AUDIO_REALTIME_PROVIDER_QWEN => "qwen3.5-omni-plus-realtime",
        AUDIO_REALTIME_PROVIDER_DOUBAO => "",
        _ => "gpt-realtime",
    }
}

fn audio_realtime_default_voice(provider: &str) -> &'static str {
    match provider {
        AUDIO_REALTIME_PROVIDER_QWEN => "Tina",
        AUDIO_REALTIME_PROVIDER_DOUBAO => "",
        _ => "alloy",
    }
}

/// Return whether the realtime voice provider string is supported by the current firmware.
pub fn audio_realtime_provider_supported(provider: &str) -> bool {
    matches!(
        provider,
        AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE
            | AUDIO_REALTIME_PROVIDER_QWEN
            | AUDIO_REALTIME_PROVIDER_DOUBAO
    )
}

fn default_audio_realtime_provider() -> String {
    AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE.to_string()
}

fn default_audio_service_provider() -> String {
    "baidu".to_string()
}

fn default_audio_realtime_ws_url() -> String {
    audio_realtime_default_ws_url(AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE).to_string()
}

fn default_audio_realtime_model() -> String {
    audio_realtime_default_model(AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE).to_string()
}

fn default_audio_realtime_voice() -> String {
    audio_realtime_default_voice(AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE).to_string()
}

fn default_audio_realtime_config() -> AudioRealtimeConfig {
    AudioRealtimeConfig {
        provider: default_audio_realtime_provider(),
        ws_url: default_audio_realtime_ws_url(),
        api_key: String::new(),
        model: default_audio_realtime_model(),
        voice: default_audio_realtime_voice(),
        instructions: "你是甲壳虫的语音助手。请直接口语化回应，简洁自然，默认使用中文。"
            .to_string(),
    }
}

pub fn default_disabled_audio_segment() -> AudioSegment {
    AudioSegment {
        version: AUDIO_CONFIG_VERSION,
        enabled: false,
        service_provider: default_audio_service_provider(),
        topology: default_audio_topology(),
        microphone: AudioMicrophoneConfig {
            enabled: false,
            device_type: "i2s_inmp441".to_string(),
            pins: AudioMicPins {
                ws: 25,
                sck: 26,
                din: 27,
            },
            sample_rate: default_audio_sample_rate(),
        },
        speaker: AudioSpeakerConfig {
            enabled: false,
            device_type: "i2s_max98357a".to_string(),
            device_ref: None,
            pins: Some(AudioSpeakerPins {
                ws: 32,
                sck: 33,
                dout: 22,
                sd: None,
            }),
            sample_rate: default_audio_sample_rate(),
        },
        codec: default_audio_codec_config(),
        vad: AudioVadConfig {
            threshold: default_audio_vad_threshold(),
            silence_duration_ms: default_audio_vad_silence_ms(),
        },
        wake_word: AudioWakeWordConfig {
            enabled: false,
            keyword: "hiesp".to_string(),
            enter_threshold: default_wake_enter_threshold(),
            leave_threshold: default_wake_leave_threshold(),
            reference_suppress_ratio: default_wake_reference_suppress_ratio(),
            zcr_min: default_wake_zcr_min(),
            zcr_max: default_wake_zcr_max(),
            min_speech_band_ratio: default_wake_min_speech_band_ratio(),
            min_active_ms: default_wake_min_active_ms(),
            hangover_ms: default_wake_hangover_ms(),
            cooldown_ms: default_wake_cooldown_ms(),
            wake_prompt: default_wake_prompt(),
        },
        speech: AudioSpeechConfig {
            api_url: "https://vop.baidu.com/server_api".to_string(),
            api_key: String::new(),
            api_secret: String::new(),
            model: "1537".to_string(),
            language: "zh".to_string(),
        },
        tts: AudioTtsConfig {
            voice: "0".to_string(),
            rate: "+0%".to_string(),
            pitch: "+0Hz".to_string(),
        },
        realtime: default_audio_realtime_config(),
        ambient_listening: AudioAmbientListeningConfig {
            enabled: false,
            detect_emotions: true,
            sound_events: vec![
                "sigh".to_string(),
                "cough".to_string(),
                "laugh".to_string(),
                "cry".to_string(),
                "door_close".to_string(),
            ],
            cooldown_minutes: default_audio_cooldown_minutes(),
            check_interval_seconds: default_audio_check_interval_seconds(),
        },
        led_indicator: AudioLedIndicatorConfig {
            enabled: false,
            pin: 2,
            states: AudioLedStatesConfig {
                listening: "breathing".to_string(),
                processing: "fast_blink".to_string(),
                speaking: "solid".to_string(),
            },
        },
    }
}

pub const AUDIO_REALTIME_PCM16_SAMPLE_RATE: u32 = 24_000;
pub fn audio_realtime_required_sample_rate(provider: &str) -> u32 {
    let _ = provider;
    AUDIO_REALTIME_PCM16_SAMPLE_RATE
}

pub fn audio_realtime_enabled(seg: &AudioSegment) -> bool {
    let provider = seg.realtime.provider.trim();
    if !audio_realtime_provider_supported(provider) || seg.realtime.ws_url.trim().is_empty() {
        return false;
    }
    !seg.realtime.api_key.trim().is_empty()
        && !seg.realtime.model.trim().is_empty()
        && !seg.realtime.voice.trim().is_empty()
}

/// Return whether the audio runtime has a physical endpoint worth initializing.
///
/// Feature flags such as ambient listening or LED indication do not by
/// themselves justify starting the ESP audio worker.
pub fn audio_runtime_pipeline_enabled(seg: &AudioSegment) -> bool {
    seg.enabled && (seg.microphone.enabled || seg.speaker.enabled)
}

// ── Hardware device config constants ──
const MAX_HARDWARE_DEVICES: usize = 8;
const MAX_PWM_DEVICES: usize = 4;
const HARDWARE_ID_MAX_LEN: usize = 32;
const HARDWARE_WHAT_MAX_LEN: usize = 128;
const HARDWARE_HOW_MAX_LEN: usize = 256;
const HARDWARE_PIN_MIN: i32 = 1;
const HARDWARE_PIN_MAX: i32 = 48;
// ESP32-S3 strap：0/3/46 禁止作通用 GPIO。45 在数据手册中为 strap，部分板卡将外设接至 IO45，校验放行（仍须与原理图一致）。
// ESP32-S3 strapping: forbid 0/3/46 as general GPIO. IO45 is strap per datasheet but allowed when the board wires it.
const HARDWARE_FORBIDDEN_PINS: [i32; 3] = [0, 3, 46];
const HARDWARE_ADC1_PINS: std::ops::RangeInclusive<i32> = 1..=10;
const HARDWARE_PWM_FREQ_MIN: u32 = 1;
const HARDWARE_PWM_FREQ_MAX: u32 = 40_000;
const I2C_BUS_FREQ_MIN: u32 = 10_000;
const I2C_BUS_FREQ_MAX: u32 = 1_000_000;
const KNOWN_DEVICE_TYPES: [&str; 6] = ["gpio_out", "gpio_in", "pwm_out", "adc_in", "buzzer", "dht"];

/// 引脚配置：键为引脚角色（如 "pin"），值为 GPIO 编号。
pub type PinConfig = std::collections::HashMap<String, i32>;

/// 单个硬件设备条目。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub id: String,
    pub device_type: String,
    pub pins: PinConfig,
    pub what: String,
    pub how: String,
    #[serde(default)]
    pub options: serde_json::Value,
}

/// I2C 总线配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct I2cBusConfig {
    pub sda_pin: i32,
    pub scl_pin: i32,
    #[serde(default = "default_i2c_freq")]
    pub freq_hz: u32,
}

fn default_i2c_freq() -> u32 {
    crate::constants::I2C_DEFAULT_FREQ_HZ
}

/// I2S 总线配置。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct I2sBusConfig {
    pub mclk_pin: i32,
    pub ws_pin: i32,
    pub bclk_pin: i32,
    pub din_pin: i32,
    pub dout_pin: i32,
}

/// 单个 I2C 设备条目。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct I2cDeviceEntry {
    pub id: String,
    pub addr: u8,
    pub what: String,
    pub how: String,
    #[serde(default)]
    pub options: serde_json::Value,
}

/// I2C 传感器条目（SHT3x / AHT20 / raw）；与 `I2cDeviceEntry` 分离，供 `drive_i2c_sensor` 使用。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct I2cSensorEntry {
    pub id: String,
    /// 7-bit I2C 地址。
    pub addr: u8,
    /// `sht3x` | `aht20` | `raw`
    pub model: String,
    pub what: String,
    pub how: String,
    #[serde(default)]
    pub options: serde_json::Value,
}

/// POST /api/config/hardware 请求体；硬件设备列表。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HardwareSegment {
    #[serde(default)]
    pub hardware_devices: Vec<DeviceEntry>,
    #[serde(default)]
    pub i2c_bus: Option<I2cBusConfig>,
    #[serde(default)]
    pub i2s_bus: Option<I2sBusConfig>,
    #[serde(default)]
    pub i2c_devices: Vec<I2cDeviceEntry>,
    #[serde(default)]
    pub i2c_sensors: Vec<I2cSensorEntry>,
}

impl HardwareSegment {
    pub fn from_app_config(config: &AppConfig) -> Self {
        Self {
            hardware_devices: config.hardware_devices.clone(),
            i2c_bus: config.i2c_bus.clone(),
            i2s_bus: config.i2s_bus.clone(),
            i2c_devices: config.i2c_devices.clone(),
            i2c_sensors: config.i2c_sensors.clone(),
        }
    }
}

impl AudioSegment {
    pub fn from_app_config(config: &AppConfig) -> Self {
        config
            .audio
            .clone()
            .unwrap_or_else(default_disabled_audio_segment)
    }
}

pub fn display_segment_from_app_config(config: &AppConfig) -> DisplayConfig {
    config
        .display
        .clone()
        .unwrap_or_else(default_disabled_display_config)
}

/// 私有：校验 llm_sources 非空、字段长度。供 from_json_and_validate 与 save_llm_segment 复用。
fn validate_llm_sources(sources: &[LlmSource]) -> Result<()> {
    if sources.is_empty() {
        return Err(Error::config("config", "llm_sources must not be empty"));
    }
    let mut ids = HashSet::new();
    for (i, s) in sources.iter().enumerate() {
        if s.api_key.len() > CONFIG_FIELD_MAX_LEN
            || s.provider.len() > CONFIG_FIELD_MAX_LEN
            || s.model.len() > CONFIG_FIELD_MAX_LEN
            || s.api_url.len() > CONFIG_LLM_API_URL_MAX
            || s.id.len() > CONFIG_LLM_SOURCE_ID_MAX
        {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}] field length over limit", i),
            ));
        }
        if s.id.trim().is_empty() {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}].id is required", i),
            ));
        }
        if !ids.insert(s.id.trim().to_string()) {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}].id duplicate", i),
            ));
        }
        let mut header_names = HashSet::new();
        for (j, header) in s.custom_headers.iter().enumerate() {
            let name = header.name.trim();
            if name.is_empty() {
                return Err(Error::config(
                    "config",
                    format!("llm_sources[{}].custom_headers[{}].name is required", i, j),
                ));
            }
            if name.len() > CONFIG_LLM_HEADER_NAME_MAX
                || header.value.len() > CONFIG_LLM_HEADER_VALUE_MAX
            {
                return Err(Error::config(
                    "config",
                    format!(
                        "llm_sources[{}].custom_headers[{}] field length over limit",
                        i, j
                    ),
                ));
            }
            if !header_names.insert(name.to_ascii_lowercase()) {
                return Err(Error::config(
                    "config",
                    format!("llm_sources[{}].custom_headers duplicate header name", i),
                ));
            }
        }
    }
    Ok(())
}

/// 私有：校验 ChannelsSegment 的 enabled_channel 与各字段长度。供 save_channels_segment 复用。
fn validate_channels_segment_fields(seg: &ChannelsSegment) -> Result<()> {
    validate_channels_segment_fields_with_enabled_channel(seg, false)
}

fn validate_channels_segment_storage_merge_fields(seg: &ChannelsSegment) -> Result<()> {
    validate_channels_segment_fields_with_enabled_channel(seg, true)
}

fn validate_channels_segment_fields_with_enabled_channel(
    seg: &ChannelsSegment,
    allow_unavailable_enabled_channel: bool,
) -> Result<()> {
    if !allow_unavailable_enabled_channel && !is_valid_enabled_channel(seg.enabled_channel.as_str())
    {
        return Err(Error::config(
            "config",
            enabled_channel_validation_message(),
        ));
    }
    validate_tg_group_activation(&seg.tg_group_activation)?;
    if seg.enabled_channel.len() > CONFIG_FIELD_MAX_LEN
        || seg.tg_token.len() > CONFIG_FIELD_MAX_LEN
        || seg.feishu_app_secret.len() > CONFIG_FIELD_MAX_LEN
        || seg.feishu_app_id.len() > CONFIG_FIELD_MAX_LEN
        || seg.dingtalk_client_id.len() > CONFIG_FIELD_MAX_LEN
        || seg.dingtalk_client_secret.len() > CONFIG_FIELD_MAX_LEN
        || seg.wecom_bot_id.len() > CONFIG_FIELD_MAX_LEN
        || seg.wecom_bot_secret.len() > CONFIG_FIELD_MAX_LEN
        || seg.qq_channel_app_id.len() > CONFIG_FIELD_MAX_LEN
        || seg.qq_channel_secret.len() > CONFIG_FIELD_MAX_LEN
    {
        return Err(Error::config(
            "config",
            format!("channel field length must be <= {}", CONFIG_FIELD_MAX_LEN),
        ));
    }
    if seg.wecom_ws_url.len() > CONFIG_URL_MAX_LEN {
        return Err(Error::config(
            "config",
            format!("wecom_ws_url length must be <= {}", CONFIG_URL_MAX_LEN),
        ));
    }
    Ok(())
}

/// 私有：校验 SystemSegment 的 wifi 长度、proxy。供 save_system_segment_to_nvs 复用。
fn validate_system_segment_fields(seg: &SystemSegment) -> Result<()> {
    if seg.wifi_ssid.len() > CONFIG_FIELD_MAX_LEN || seg.wifi_pass.len() > CONFIG_FIELD_MAX_LEN {
        return Err(Error::config(
            "config",
            format!(
                "wifi_ssid and wifi_pass length must be <= {}",
                CONFIG_FIELD_MAX_LEN
            ),
        ));
    }
    validate_proxy_url_for_target(seg.proxy_url.trim(), proxy_supported_on_current_target())?;
    normalize_optional_locale(seg.locale.as_deref())?;
    Ok(())
}

#[cfg(feature = "capability_office")]
fn validate_office_accounts_segment(seg: &OfficeAccountsSegment) -> Result<()> {
    let all_accounts = seg.registry.all_accounts();
    if all_accounts.len() > CONFIG_OFFICE_ACCOUNT_LIMIT {
        return Err(Error::config(
            "office_accounts",
            format!(
                "office account count must be <= {}",
                CONFIG_OFFICE_ACCOUNT_LIMIT
            ),
        ));
    }
    for account in all_accounts {
        validate_field_len(
            &account.account_key,
            CONFIG_ACCOUNT_KEY_MAX_LEN,
            "account_key",
        )?;
        validate_field_len(
            &account.provider_kind,
            CONFIG_PROVIDER_KIND_MAX_LEN,
            "provider_kind",
        )?;
        validate_field_len(
            &account.external_account_id,
            CONFIG_EXTERNAL_ACCOUNT_ID_MAX_LEN,
            "external_account_id",
        )?;
        validate_field_len(
            &account.account_label,
            CONFIG_ACCOUNT_LABEL_MAX_LEN,
            "account_label",
        )?;
        if account.account_key.trim().is_empty() {
            return Err(Error::config(
                "office_accounts",
                "account_key must not be empty",
            ));
        }
        if account.provider_kind.trim().is_empty() {
            return Err(Error::config(
                "office_accounts",
                "provider_kind must not be empty",
            ));
        }
        if account.enabled_capabilities.is_empty() {
            return Err(Error::config(
                "office_accounts",
                format!(
                    "account '{}' must enable at least one capability",
                    account.account_key
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "capability_office")]
fn validate_office_credentials_segment(seg: &OfficeCredentialsSegment) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for credential in &seg.items {
        let account_key = credential.account_key.trim();
        if account_key.is_empty() {
            return Err(Error::config(
                "office_credentials",
                "account_key must not be empty",
            ));
        }
        if !seen.insert(account_key.to_string()) {
            return Err(Error::config(
                "office_credentials",
                format!("duplicate office credential account_key '{}'", account_key),
            ));
        }
    }
    Ok(())
}

fn validate_audio_sample_rate(value: u32, field: &str) -> Result<()> {
    if !(AUDIO_SAMPLE_RATE_MIN..=AUDIO_SAMPLE_RATE_MAX).contains(&value) {
        return Err(Error::config(
            "audio",
            format!(
                "{} must be {}..={}",
                field, AUDIO_SAMPLE_RATE_MIN, AUDIO_SAMPLE_RATE_MAX
            ),
        ));
    }
    Ok(())
}

fn audio_can_use_baidu_speech_fallback(seg: &AudioSegment) -> bool {
    seg.service_provider == "baidu"
        && !seg.speech.api_key.trim().is_empty()
        && !seg.speech.api_secret.trim().is_empty()
}

pub(crate) fn normalize_audio_segment(seg: &mut AudioSegment) {
    if seg
        .speaker
        .device_ref
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        seg.speaker.device_ref = None;
    }
    if seg.speaker.device_type != AUDIO_SPEAKER_DEVICE_USB {
        seg.speaker.device_ref = None;
    }
    seg.topology = seg.topology.trim().to_ascii_lowercase();
    trim_optional_nonempty(&mut seg.codec.input_codec);
    trim_optional_nonempty(&mut seg.codec.output_codec);
    normalize_es7210_codec_wake_defaults(seg);
}

fn trim_optional_nonempty(field: &mut Option<String>) {
    if let Some(value) = field {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            *field = None;
        } else if trimmed.len() != value.len() {
            *value = trimmed.to_string();
        }
    }
}

fn float_nearly_eq(left: f32, right: f32) -> bool {
    (left - right).abs() <= WAKE_FLOAT_EQ_EPSILON
}

pub(crate) fn audio_input_codec_is_es7210(seg: &AudioSegment) -> bool {
    seg.topology == AUDIO_TOPOLOGY_I2S_CODEC
        && seg
            .codec
            .input_codec
            .as_deref()
            .is_some_and(|value| value == AUDIO_CODEC_INPUT_ES7210)
}

pub(crate) fn audio_uses_es7210_codec_input(seg: &AudioSegment) -> bool {
    audio_input_codec_is_es7210(seg) && seg.codec.input_reference
}

pub(crate) fn audio_microphone_uses_pdm(seg: &AudioSegment) -> bool {
    seg.topology == AUDIO_TOPOLOGY_DISCRETE_I2S
        && seg.microphone.device_type == AUDIO_MIC_DEVICE_PDM
}

pub(crate) fn audio_uses_es7210_codec_wake_profile(seg: &AudioSegment) -> bool {
    let mut seg = seg.clone();
    normalize_audio_segment(&mut seg);
    seg.wake_word.enabled
        && audio_uses_es7210_codec_input(&seg)
        && float_nearly_eq(seg.wake_word.enter_threshold, ES7210_WAKE_ENTER_THRESHOLD)
        && float_nearly_eq(seg.wake_word.leave_threshold, ES7210_WAKE_LEAVE_THRESHOLD)
        && float_nearly_eq(seg.wake_word.zcr_max, ES7210_WAKE_ZCR_MAX)
        && float_nearly_eq(
            seg.wake_word.min_speech_band_ratio,
            ES7210_WAKE_MIN_SPEECH_BAND_RATIO,
        )
        && seg.wake_word.min_active_ms == ES7210_WAKE_MIN_ACTIVE_MS
}

fn wake_word_uses_legacy_acoustic_defaults(wake: &AudioWakeWordConfig) -> bool {
    float_nearly_eq(wake.enter_threshold, default_wake_enter_threshold())
        && float_nearly_eq(wake.leave_threshold, default_wake_leave_threshold())
        && float_nearly_eq(
            wake.reference_suppress_ratio,
            default_wake_reference_suppress_ratio(),
        )
        && float_nearly_eq(wake.zcr_min, default_wake_zcr_min())
        && float_nearly_eq(wake.zcr_max, default_wake_zcr_max())
        && float_nearly_eq(
            wake.min_speech_band_ratio,
            default_wake_min_speech_band_ratio(),
        )
        && wake.min_active_ms == default_wake_min_active_ms()
        && wake.hangover_ms == default_wake_hangover_ms()
        && wake.cooldown_ms == default_wake_cooldown_ms()
}

fn normalize_es7210_codec_wake_defaults(seg: &mut AudioSegment) {
    if !seg.wake_word.enabled
        || !audio_uses_es7210_codec_input(seg)
        || !wake_word_uses_legacy_acoustic_defaults(&seg.wake_word)
    {
        return;
    }

    seg.wake_word.enter_threshold = ES7210_WAKE_ENTER_THRESHOLD;
    seg.wake_word.leave_threshold = ES7210_WAKE_LEAVE_THRESHOLD;
    seg.wake_word.zcr_max = ES7210_WAKE_ZCR_MAX;
    seg.wake_word.min_speech_band_ratio = ES7210_WAKE_MIN_SPEECH_BAND_RATIO;
    seg.wake_word.min_active_ms = ES7210_WAKE_MIN_ACTIVE_MS;
}

pub(crate) fn audio_wake_word_config_for_runtime(audio: &AudioSegment) -> AudioWakeWordConfig {
    let mut seg = audio.clone();
    normalize_audio_segment(&mut seg);
    seg.wake_word
}

pub(crate) fn audio_topology_is_codec(seg: &AudioSegment) -> bool {
    seg.topology == AUDIO_TOPOLOGY_I2S_CODEC
}

fn validate_audio_codec_pa_pin(field: &'static str, pin: i32) -> Result<()> {
    if !(HARDWARE_PIN_MIN..=HARDWARE_PIN_MAX).contains(&pin) {
        return Err(Error::config(
            "audio",
            format!(
                "{} = {} out of range {}..={}",
                field, pin, HARDWARE_PIN_MIN, HARDWARE_PIN_MAX
            ),
        ));
    }
    Ok(())
}

fn validate_audio_codec_config(seg: &AudioSegment) -> Result<()> {
    let input_codec = seg.codec.input_codec.as_deref().ok_or_else(|| {
        Error::config(
            "audio",
            "audio.codec.input_codec is required when audio.topology == i2s_codec",
        )
    })?;
    if input_codec != AUDIO_CODEC_INPUT_ES7210 {
        return Err(Error::config(
            "audio",
            format!(
                "audio.codec.input_codec must be one of: {}",
                AUDIO_CODEC_INPUT_ES7210
            ),
        ));
    }
    let output_codec = seg.codec.output_codec.as_deref().ok_or_else(|| {
        Error::config(
            "audio",
            "audio.codec.output_codec is required when audio.topology == i2s_codec",
        )
    })?;
    if output_codec != AUDIO_CODEC_OUTPUT_ES8311 {
        return Err(Error::config(
            "audio",
            format!(
                "audio.codec.output_codec must be one of: {}",
                AUDIO_CODEC_OUTPUT_ES8311
            ),
        ));
    }
    let pa_pin = seg.codec.pa_pin.ok_or_else(|| {
        Error::config(
            "audio",
            "audio.codec.pa_pin is required when audio.topology == i2s_codec",
        )
    })?;
    validate_audio_codec_pa_pin("audio.codec.pa_pin", pa_pin)?;
    if let Some(addr) = seg.codec.input_addr {
        if !(0x08..=0x77).contains(&addr) {
            return Err(Error::config(
                "audio",
                format!(
                    "audio.codec.input_addr 0x{:02X} must be in 0x08..=0x77",
                    addr
                ),
            ));
        }
    }
    if let Some(addr) = seg.codec.output_addr {
        if !(0x08..=0x77).contains(&addr) {
            return Err(Error::config(
                "audio",
                format!(
                    "audio.codec.output_addr 0x{:02X} must be in 0x08..=0x77",
                    addr
                ),
            ));
        }
    }
    Ok(())
}

/// 私有：校验 AudioSegment 字段（引脚、采样率、阈值、字符串长度等）。
fn validate_audio_segment(seg: &AudioSegment) -> Result<()> {
    let wake_voice_pipeline_enabled = seg.enabled && seg.wake_word.enabled;
    let wake_realtime_enabled = wake_voice_pipeline_enabled && audio_realtime_enabled(seg);

    if seg.version != AUDIO_CONFIG_VERSION {
        return Err(Error::config(
            "audio",
            format!(
                "audio version must be {} (got {})",
                AUDIO_CONFIG_VERSION, seg.version
            ),
        ));
    }
    if seg.topology.is_empty() || seg.topology.len() > AUDIO_TOPOLOGY_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "audio.topology length must be 1..={}",
                AUDIO_TOPOLOGY_MAX_LEN
            ),
        ));
    }
    if seg.topology != AUDIO_TOPOLOGY_DISCRETE_I2S && seg.topology != AUDIO_TOPOLOGY_I2S_CODEC {
        return Err(Error::config(
            "audio",
            "audio.topology must be one of: discrete_i2s, i2s_codec",
        ));
    }
    if seg.microphone.device_type.len() > AUDIO_DEVICE_TYPE_MAX_LEN
        || seg.speaker.device_type.len() > AUDIO_DEVICE_TYPE_MAX_LEN
    {
        return Err(Error::config(
            "audio",
            format!(
                "microphone/speaker device_type length must be <= {}",
                AUDIO_DEVICE_TYPE_MAX_LEN
            ),
        ));
    }
    if seg
        .speaker
        .device_ref
        .as_ref()
        .is_some_and(|value| value.len() > AUDIO_DEVICE_REF_MAX_LEN)
    {
        return Err(Error::config(
            "audio",
            format!(
                "speaker.device_ref length must be <= {}",
                AUDIO_DEVICE_REF_MAX_LEN
            ),
        ));
    }
    if seg.wake_word.keyword.len() > AUDIO_KEYWORD_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "wake_word.keyword length must be <= {}",
                AUDIO_KEYWORD_MAX_LEN
            ),
        ));
    }
    if seg.wake_word.wake_prompt.len() > AUDIO_WAKE_PROMPT_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "wake_word.wake_prompt length must be <= {}",
                AUDIO_WAKE_PROMPT_MAX_LEN
            ),
        ));
    }
    if wake_voice_pipeline_enabled {
        if !(0.01..=1.0).contains(&seg.wake_word.enter_threshold) {
            return Err(Error::config(
                "audio",
                "wake_word.enter_threshold must be within 0.01..=1.0",
            ));
        }
        if !(0.0..seg.wake_word.enter_threshold).contains(&seg.wake_word.leave_threshold) {
            return Err(Error::config(
                "audio",
                "wake_word.leave_threshold must be >= 0 and less than wake_word.enter_threshold",
            ));
        }
        if !(0.5..=4.0).contains(&seg.wake_word.reference_suppress_ratio) {
            return Err(Error::config(
                "audio",
                "wake_word.reference_suppress_ratio must be within 0.5..=4.0",
            ));
        }
        if seg.wake_word.zcr_min < 0.0
            || seg.wake_word.zcr_max > 1.0
            || seg.wake_word.zcr_min >= seg.wake_word.zcr_max
        {
            return Err(Error::config(
                "audio",
                "wake_word.zcr_min and wake_word.zcr_max must satisfy 0.0 <= min < max <= 1.0",
            ));
        }
        if !(0.0..=1.0).contains(&seg.wake_word.min_speech_band_ratio) {
            return Err(Error::config(
                "audio",
                "wake_word.min_speech_band_ratio must be within 0.0..=1.0",
            ));
        }
        if !(20..=5_000).contains(&seg.wake_word.min_active_ms) {
            return Err(Error::config(
                "audio",
                "wake_word.min_active_ms must be within 20..=5000",
            ));
        }
        if seg.wake_word.hangover_ms > 10_000 {
            return Err(Error::config(
                "audio",
                "wake_word.hangover_ms must be within 0..=10000",
            ));
        }
        if !(100..=10_000).contains(&seg.wake_word.cooldown_ms) {
            return Err(Error::config(
                "audio",
                "wake_word.cooldown_ms must be within 100..=10000",
            ));
        }
        if !seg.microphone.enabled {
            return Err(Error::config(
                "audio",
                "wake_word.enabled requires microphone.enabled == true",
            ));
        }
        if !seg.speaker.enabled {
            return Err(Error::config(
                "audio",
                "wake_word.enabled requires speaker.enabled == true",
            ));
        }
        if !wake_realtime_enabled && !audio_can_use_baidu_speech_fallback(seg) {
            return Err(Error::config(
                "audio",
                "wake_word.enabled requires realtime voice config or a configured speech fallback for the currently wired provider",
            ));
        }
    }
    if seg.service_provider.len() > CONFIG_FIELD_MAX_LEN
        || seg.speech.model.len() > CONFIG_FIELD_MAX_LEN
        || seg.speech.language.len() > CONFIG_FIELD_MAX_LEN
        || seg.tts.voice.len() > AUDIO_VOICE_MAX_LEN
        || seg.tts.rate.len() > AUDIO_RATE_MAX_LEN
        || seg.tts.pitch.len() > AUDIO_PITCH_MAX_LEN
    {
        return Err(Error::config(
            "audio",
            "audio text fields exceed max length",
        ));
    }
    if seg.realtime.provider.len() > CONFIG_FIELD_MAX_LEN
        || seg.realtime.model.len() > CONFIG_FIELD_MAX_LEN
        || seg.realtime.voice.len() > AUDIO_VOICE_MAX_LEN
    {
        return Err(Error::config(
            "audio",
            "realtime provider/model/voice identifiers exceed max length",
        ));
    }
    if seg.realtime.instructions.len() > AUDIO_REALTIME_INSTRUCTIONS_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "realtime.instructions length must be <= {}",
                AUDIO_REALTIME_INSTRUCTIONS_MAX_LEN
            ),
        ));
    }
    if seg.speech.api_key.len() > AUDIO_SPEECH_API_KEY_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "speech.api_key length must be <= {}",
                AUDIO_SPEECH_API_KEY_MAX_LEN
            ),
        ));
    }
    if seg.speech.api_secret.len() > AUDIO_SPEECH_API_SECRET_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "speech.api_secret length must be <= {}",
                AUDIO_SPEECH_API_SECRET_MAX_LEN
            ),
        ));
    }
    if seg.realtime.api_key.len() > AUDIO_SPEECH_API_KEY_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!(
                "realtime.api_key length must be <= {}",
                AUDIO_SPEECH_API_KEY_MAX_LEN
            ),
        ));
    }
    if seg.speech.api_url.len() > CONFIG_URL_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!("speech.api_url length must be <= {}", CONFIG_URL_MAX_LEN),
        ));
    }
    if seg.realtime.ws_url.len() > CONFIG_URL_MAX_LEN {
        return Err(Error::config(
            "audio",
            format!("realtime.ws_url length must be <= {}", CONFIG_URL_MAX_LEN),
        ));
    }
    if wake_voice_pipeline_enabled
        && !wake_realtime_enabled
        && seg.microphone.enabled
        && seg.service_provider == "baidu"
        && (seg.speech.api_key.trim().is_empty() || seg.speech.api_secret.trim().is_empty())
    {
        return Err(Error::config(
            "audio",
            "speech.api_key and speech.api_secret are required when service_provider == baidu",
        ));
    }
    if wake_voice_pipeline_enabled
        && !wake_realtime_enabled
        && seg.speaker.enabled
        && seg.service_provider == "baidu"
        && (seg.speech.api_key.trim().is_empty() || seg.speech.api_secret.trim().is_empty())
    {
        return Err(Error::config(
            "audio",
            "speaker fallback requires non-empty speech.api_key/speech.api_secret when service_provider == baidu",
        ));
    }
    if wake_realtime_enabled {
        let provider = seg.realtime.provider.trim();
        if !seg.microphone.enabled {
            return Err(Error::config(
                "audio",
                "realtime voice requires microphone.enabled == true",
            ));
        }
        if !seg.speaker.enabled {
            return Err(Error::config(
                "audio",
                "realtime voice requires speaker.enabled == true",
            ));
        }
        if !audio_realtime_provider_supported(provider) {
            return Err(Error::config(
                "audio",
                "realtime.provider must be one of: openai_compatible, qwen, doubao",
            ));
        }
        if !seg.realtime.ws_url.starts_with("wss://") && !seg.realtime.ws_url.starts_with("ws://") {
            return Err(Error::config(
                "audio",
                "realtime.ws_url must start with wss:// or ws://",
            ));
        }
        if seg.realtime.api_key.trim().is_empty() {
            return Err(Error::config(
                "audio",
                "realtime voice requires realtime.api_key",
            ));
        }
        if seg.realtime.model.trim().is_empty() {
            return Err(Error::config(
                "audio",
                "realtime voice requires realtime.model",
            ));
        }
        if seg.realtime.voice.trim().is_empty() {
            return Err(Error::config(
                "audio",
                "realtime voice requires realtime.voice",
            ));
        }
        let required_sample_rate = audio_realtime_required_sample_rate(provider);
        if seg.microphone.sample_rate != required_sample_rate {
            return Err(Error::config(
                "audio",
                format!(
                    "microphone.sample_rate must equal {} for realtime voice",
                    required_sample_rate
                ),
            ));
        }
        if seg.speaker.sample_rate != required_sample_rate {
            return Err(Error::config(
                "audio",
                format!(
                    "speaker.sample_rate must equal {} for realtime voice",
                    required_sample_rate
                ),
            ));
        }
    }

    match seg.topology.as_str() {
        AUDIO_TOPOLOGY_DISCRETE_I2S => {
            if seg.microphone.enabled {
                if seg.microphone.device_type != AUDIO_MIC_DEVICE_I2S_INMP441
                    && seg.microphone.device_type != AUDIO_MIC_DEVICE_PDM
                {
                    return Err(Error::config(
                        "audio",
                        format!(
                            "microphone.device_type must be one of: {}, {}",
                            AUDIO_MIC_DEVICE_I2S_INMP441, AUDIO_MIC_DEVICE_PDM
                        ),
                    ));
                }
                validate_pin_range(seg.microphone.pins.ws, "audio")?;
                validate_pin_range(seg.microphone.pins.sck, "audio")?;
                validate_pin_range(seg.microphone.pins.din, "audio")?;
                validate_audio_sample_rate(seg.microphone.sample_rate, "microphone.sample_rate")?;
            }
            if seg.speaker.enabled {
                match seg.speaker.device_type.as_str() {
                    AUDIO_SPEAKER_DEVICE_I2S_MAX98357A => {
                        let pins = seg.speaker.pins.as_ref().ok_or_else(|| {
                            Error::config(
                                "audio",
                                "speaker.pins are required when speaker.device_type == i2s_max98357a",
                            )
                        })?;
                        validate_pin_range(pins.ws, "audio")?;
                        validate_pin_range(pins.sck, "audio")?;
                        validate_pin_range(pins.dout, "audio")?;
                        if let Some(sd) = pins.sd {
                            validate_pin_range(sd, "audio")?;
                        }
                    }
                    AUDIO_SPEAKER_DEVICE_USB => {}
                    _ => {
                        return Err(Error::config(
                            "audio",
                            format!(
                                "speaker.device_type must be one of: {}, {}",
                                AUDIO_SPEAKER_DEVICE_I2S_MAX98357A, AUDIO_SPEAKER_DEVICE_USB
                            ),
                        ));
                    }
                }
                validate_audio_sample_rate(seg.speaker.sample_rate, "speaker.sample_rate")?;
            }
        }
        AUDIO_TOPOLOGY_I2S_CODEC => {
            if seg.microphone.enabled {
                validate_audio_sample_rate(seg.microphone.sample_rate, "microphone.sample_rate")?;
            }
            if seg.speaker.enabled {
                validate_audio_sample_rate(seg.speaker.sample_rate, "speaker.sample_rate")?;
            }
            if seg.microphone.enabled
                && seg.speaker.enabled
                && seg.microphone.sample_rate != seg.speaker.sample_rate
            {
                return Err(Error::config(
                    "audio",
                    "speaker.sample_rate must equal microphone.sample_rate when audio.topology == i2s_codec",
                ));
            }
            validate_audio_codec_config(seg)?;
        }
        _ => {}
    }
    if !(0.0..=1.0).contains(&seg.vad.threshold) {
        return Err(Error::config(
            "audio",
            "vad.threshold must be in [0.0, 1.0]",
        ));
    }
    if seg.vad.silence_duration_ms == 0 || seg.vad.silence_duration_ms > 60_000 {
        return Err(Error::config(
            "audio",
            "vad.silence_duration_ms must be 1..=60000",
        ));
    }
    if seg.ambient_listening.sound_events.len() > AUDIO_SOUND_EVENTS_MAX {
        return Err(Error::config(
            "audio",
            format!("sound_events count must be <= {}", AUDIO_SOUND_EVENTS_MAX),
        ));
    }
    for (i, evt) in seg.ambient_listening.sound_events.iter().enumerate() {
        if evt.is_empty() || evt.len() > AUDIO_SOUND_EVENT_MAX_LEN {
            return Err(Error::config(
                "audio",
                format!(
                    "sound_events[{}] length must be 1..={}",
                    i, AUDIO_SOUND_EVENT_MAX_LEN
                ),
            ));
        }
    }
    if seg.ambient_listening.cooldown_minutes > 24 * 60 {
        return Err(Error::config(
            "audio",
            "ambient_listening.cooldown_minutes must be <= 1440",
        ));
    }
    if seg.ambient_listening.check_interval_seconds == 0
        || seg.ambient_listening.check_interval_seconds > 24 * 3600
    {
        return Err(Error::config(
            "audio",
            "ambient_listening.check_interval_seconds must be 1..=86400",
        ));
    }
    if seg.led_indicator.enabled {
        validate_pin_range(seg.led_indicator.pin, "audio")?;
    }
    if seg.led_indicator.states.listening.len() > CONFIG_FIELD_MAX_LEN
        || seg.led_indicator.states.processing.len() > CONFIG_FIELD_MAX_LEN
        || seg.led_indicator.states.speaking.len() > CONFIG_FIELD_MAX_LEN
    {
        return Err(Error::config(
            "audio",
            format!(
                "led_indicator.states field length must be <= {}",
                CONFIG_FIELD_MAX_LEN
            ),
        ));
    }

    Ok(())
}

/// 私有：校验 HardwareSegment 全部约束（设备数、ID、类型、引脚范围/冲突、PWM 频率等）。
fn validate_hardware_segment(seg: &HardwareSegment) -> Result<()> {
    if seg.hardware_devices.len() > MAX_HARDWARE_DEVICES {
        return Err(Error::config(
            "hardware",
            format!("hardware_devices count must be <= {}", MAX_HARDWARE_DEVICES),
        ));
    }
    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_pins = std::collections::HashSet::new();
    let mut pwm_count: usize = 0;
    for (i, dev) in seg.hardware_devices.iter().enumerate() {
        // id
        if dev.id.is_empty() || dev.id.len() > HARDWARE_ID_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "hardware_devices[{}].id must be 1..={} chars",
                    i, HARDWARE_ID_MAX_LEN
                ),
            ));
        }
        if !seen_ids.insert(&dev.id) {
            return Err(Error::config(
                "hardware",
                format!("hardware_devices[{}].id '{}' is duplicated", i, dev.id),
            ));
        }
        // device_type
        if !KNOWN_DEVICE_TYPES.contains(&dev.device_type.as_str()) {
            return Err(Error::config(
                "hardware",
                format!(
                    "hardware_devices[{}].device_type '{}' is not one of {:?}",
                    i, dev.device_type, KNOWN_DEVICE_TYPES
                ),
            ));
        }
        // what / how
        if dev.what.len() > HARDWARE_WHAT_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "hardware_devices[{}].what length must be <= {}",
                    i, HARDWARE_WHAT_MAX_LEN
                ),
            ));
        }
        if dev.how.len() > HARDWARE_HOW_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "hardware_devices[{}].how length must be <= {}",
                    i, HARDWARE_HOW_MAX_LEN
                ),
            ));
        }
        // pins: must have "pin" key
        let pin_val = dev.pins.get("pin").ok_or_else(|| {
            Error::config(
                "hardware",
                format!("hardware_devices[{}].pins must have a \"pin\" key", i),
            )
        })?;
        // validate all pin values
        for (role, &pv) in &dev.pins {
            if !(HARDWARE_PIN_MIN..=HARDWARE_PIN_MAX).contains(&pv) {
                return Err(Error::config(
                    "hardware",
                    format!(
                        "hardware_devices[{}].pins.{} = {} out of range {}..={}",
                        i, role, pv, HARDWARE_PIN_MIN, HARDWARE_PIN_MAX
                    ),
                ));
            }
            if HARDWARE_FORBIDDEN_PINS.contains(&pv) {
                return Err(Error::config(
                    "hardware",
                    format!(
                        "hardware_devices[{}].pins.{} = {} is a forbidden strapping pin",
                        i, role, pv
                    ),
                ));
            }
            if !seen_pins.insert(pv) {
                return Err(Error::config(
                    "hardware",
                    format!(
                        "pin {} is used by multiple devices (conflict at devices[{}].pins.{})",
                        pv, i, role
                    ),
                ));
            }
        }
        // adc_in: pin must be in ADC1 range
        if dev.device_type == "adc_in" && !HARDWARE_ADC1_PINS.contains(pin_val) {
            return Err(Error::config(
                "hardware",
                format!(
                    "hardware_devices[{}] adc_in pin {} must be in ADC1 range {:?}",
                    i, pin_val, HARDWARE_ADC1_PINS
                ),
            ));
        }
        // pwm_out count + frequency
        if dev.device_type == "pwm_out" {
            pwm_count += 1;
            if let Some(freq) = dev.options.get("frequency_hz").and_then(|v| v.as_u64()) {
                let freq = freq as u32;
                if !(HARDWARE_PWM_FREQ_MIN..=HARDWARE_PWM_FREQ_MAX).contains(&freq) {
                    return Err(Error::config(
                        "hardware",
                        format!(
                            "hardware_devices[{}] pwm_out frequency_hz {} must be {}..={}",
                            i, freq, HARDWARE_PWM_FREQ_MIN, HARDWARE_PWM_FREQ_MAX
                        ),
                    ));
                }
            }
        }
        if dev.device_type == "dht" {
            if let Some(model) = dev.options.get("model").and_then(|v| v.as_str()) {
                if !["dht11", "dht22", "dht21"].contains(&model) {
                    return Err(Error::config(
                        "hardware",
                        format!(
                            "hardware_devices[{}] dht options.model '{}' must be dht11|dht22|dht21",
                            i, model
                        ),
                    ));
                }
            }
        }
    }
    if pwm_count > MAX_PWM_DEVICES {
        return Err(Error::config(
            "hardware",
            format!(
                "pwm_out device count {} exceeds max {}",
                pwm_count, MAX_PWM_DEVICES
            ),
        ));
    }

    if !seg.i2c_sensors.is_empty() && seg.i2c_bus.is_none() {
        return Err(Error::config(
            "hardware",
            "i2c_bus is required when i2c_sensors are configured",
        ));
    }
    if let Some(bus) = &seg.i2c_bus {
        validate_i2c_bus_pin("i2c_bus.sda_pin", bus.sda_pin)?;
        validate_i2c_bus_pin("i2c_bus.scl_pin", bus.scl_pin)?;
        if bus.sda_pin == bus.scl_pin {
            return Err(Error::config(
                "hardware",
                "i2c_bus.sda_pin and i2c_bus.scl_pin must be different",
            ));
        }
        if !(I2C_BUS_FREQ_MIN..=I2C_BUS_FREQ_MAX).contains(&bus.freq_hz) {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_bus.freq_hz {} must be {}..={}",
                    bus.freq_hz, I2C_BUS_FREQ_MIN, I2C_BUS_FREQ_MAX
                ),
            ));
        }
    }
    if let Some(bus) = &seg.i2s_bus {
        validate_i2s_bus_config(bus)?;
    }

    // i2c_sensors
    use crate::constants::{
        I2C_MAX_READ_LEN, I2C_SENSOR_ID_MAX_LEN, I2C_SENSOR_MAX_CMD_LEN, I2C_SENSOR_MAX_ENTRIES,
    };
    const I2C_SENSOR_MODELS: [&str; 3] = ["sht3x", "aht20", "raw"];
    if seg.i2c_sensors.len() > I2C_SENSOR_MAX_ENTRIES {
        return Err(Error::config(
            "hardware",
            format!("i2c_sensors count must be <= {}", I2C_SENSOR_MAX_ENTRIES),
        ));
    }
    let mut seen_i2c_sensor_ids = std::collections::HashSet::new();
    for (i, s) in seg.i2c_sensors.iter().enumerate() {
        if seg.hardware_devices.iter().any(|d| d.id == s.id) {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].id '{}' conflicts with hardware_devices id",
                    i, s.id
                ),
            ));
        }
        if s.id.is_empty() || s.id.len() > I2C_SENSOR_ID_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].id must be 1..={} chars",
                    i, I2C_SENSOR_ID_MAX_LEN
                ),
            ));
        }
        if !seen_i2c_sensor_ids.insert(&s.id) {
            return Err(Error::config(
                "hardware",
                format!("i2c_sensors[{}].id '{}' is duplicated", i, s.id),
            ));
        }
        if !(0x08..=0x77).contains(&s.addr) {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].addr 0x{:02X} must be in 0x08..=0x77",
                    i, s.addr
                ),
            ));
        }
        if !I2C_SENSOR_MODELS.contains(&s.model.as_str()) {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].model '{}' must be one of {:?}",
                    i, s.model, I2C_SENSOR_MODELS
                ),
            ));
        }
        if s.what.len() > HARDWARE_WHAT_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].what length must be <= {}",
                    i, HARDWARE_WHAT_MAX_LEN
                ),
            ));
        }
        if s.how.len() > HARDWARE_HOW_MAX_LEN {
            return Err(Error::config(
                "hardware",
                format!(
                    "i2c_sensors[{}].how length must be <= {}",
                    i, HARDWARE_HOW_MAX_LEN
                ),
            ));
        }
        if s.model == "raw" {
            let init = s
                .options
                .get("init_cmd")
                .and_then(|v| v.as_array())
                .ok_or_else(|| {
                    Error::config(
                        "hardware",
                        format!(
                            "i2c_sensors[{}] raw model requires options.init_cmd as array",
                            i
                        ),
                    )
                })?;
            if init.is_empty() || init.len() > I2C_SENSOR_MAX_CMD_LEN {
                return Err(Error::config(
                    "hardware",
                    format!(
                        "i2c_sensors[{}] raw init_cmd length must be 1..={}",
                        i, I2C_SENSOR_MAX_CMD_LEN
                    ),
                ));
            }
            for (j, el) in init.iter().enumerate() {
                let b = el.as_u64().ok_or_else(|| {
                    Error::config(
                        "hardware",
                        format!(
                            "i2c_sensors[{}].options.init_cmd[{}] must be integer 0-255",
                            i, j
                        ),
                    )
                })?;
                if b > 255 {
                    return Err(Error::config(
                        "hardware",
                        format!("i2c_sensors[{}].options.init_cmd[{}] must be 0-255", i, j),
                    ));
                }
            }
            let read_len = s
                .options
                .get("read_len")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    Error::config(
                        "hardware",
                        format!(
                            "i2c_sensors[{}] raw model requires options.read_len (1..={})",
                            i, I2C_MAX_READ_LEN
                        ),
                    )
                })? as usize;
            if read_len == 0 || read_len > I2C_MAX_READ_LEN {
                return Err(Error::config(
                    "hardware",
                    format!(
                        "i2c_sensors[{}] raw read_len must be 1..={}",
                        i, I2C_MAX_READ_LEN
                    ),
                ));
            }
            if let Some(ms) = s.options.get("conversion_wait_ms").and_then(|v| v.as_u64()) {
                if ms > 2000 {
                    return Err(Error::config(
                        "hardware",
                        format!("i2c_sensors[{}] conversion_wait_ms must be <= 2000", i),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_i2c_bus_pin(field: &'static str, pin: i32) -> Result<()> {
    if !(HARDWARE_PIN_MIN..=HARDWARE_PIN_MAX).contains(&pin) {
        return Err(Error::config(
            "hardware",
            format!(
                "{} = {} out of range {}..={}",
                field, pin, HARDWARE_PIN_MIN, HARDWARE_PIN_MAX
            ),
        ));
    }
    if HARDWARE_FORBIDDEN_PINS.contains(&pin) {
        return Err(Error::config(
            "hardware",
            format!("{} = {} is a forbidden strapping pin", field, pin),
        ));
    }
    Ok(())
}

fn validate_i2s_bus_pin(field: &'static str, pin: i32) -> Result<()> {
    if !(HARDWARE_PIN_MIN..=HARDWARE_PIN_MAX).contains(&pin) {
        return Err(Error::config(
            "hardware",
            format!(
                "{} = {} out of range {}..={}",
                field, pin, HARDWARE_PIN_MIN, HARDWARE_PIN_MAX
            ),
        ));
    }
    if HARDWARE_FORBIDDEN_PINS.contains(&pin) {
        return Err(Error::config(
            "hardware",
            format!("{} = {} is a forbidden strapping pin", field, pin),
        ));
    }
    Ok(())
}

fn validate_i2s_bus_config(bus: &I2sBusConfig) -> Result<()> {
    validate_i2s_bus_pin("i2s_bus.mclk_pin", bus.mclk_pin)?;
    validate_i2s_bus_pin("i2s_bus.ws_pin", bus.ws_pin)?;
    validate_i2s_bus_pin("i2s_bus.bclk_pin", bus.bclk_pin)?;
    validate_i2s_bus_pin("i2s_bus.din_pin", bus.din_pin)?;
    validate_i2s_bus_pin("i2s_bus.dout_pin", bus.dout_pin)?;

    let pins = [
        ("i2s_bus.mclk_pin", bus.mclk_pin),
        ("i2s_bus.ws_pin", bus.ws_pin),
        ("i2s_bus.bclk_pin", bus.bclk_pin),
        ("i2s_bus.din_pin", bus.din_pin),
        ("i2s_bus.dout_pin", bus.dout_pin),
    ];
    let mut seen = std::collections::HashSet::new();
    for (field, pin) in pins {
        if !seen.insert(pin) {
            return Err(Error::config(
                "hardware",
                format!("{} duplicates another i2s_bus pin ({})", field, pin),
            ));
        }
    }
    Ok(())
}

fn validate_audio_hardware_pair(
    stage: &'static str,
    audio: &AudioSegment,
    hardware: &HardwareSegment,
) -> Result<()> {
    if !audio_topology_is_codec(audio) {
        return Ok(());
    }
    if hardware.i2c_bus.is_none() {
        return Err(Error::config(
            stage,
            "hardware.i2c_bus is required when audio.topology == i2s_codec",
        ));
    }
    if hardware.i2s_bus.is_none() {
        return Err(Error::config(
            stage,
            "hardware.i2s_bus is required when audio.topology == i2s_codec",
        ));
    }
    Ok(())
}

fn default_hardware_segment() -> HardwareSegment {
    HardwareSegment {
        hardware_devices: vec![],
        i2c_bus: None,
        i2s_bus: None,
        i2c_devices: vec![],
        i2c_sensors: vec![],
    }
}

fn load_hardware_segment_value(reader: &dyn ConfigFileStore) -> Result<HardwareSegment> {
    match reader.read_config_file("config/hardware.json")? {
        Some(bytes) if bytes.iter().all(|b| b.is_ascii_whitespace()) => {
            Ok(default_hardware_segment())
        }
        Some(bytes) => {
            let json = std::str::from_utf8(&bytes)
                .map_err(|e| Error::config("hardware", e.to_string()))?;
            let seg = deserialize_storage_json_loose_tail::<HardwareSegment>(json)
                .map_err(|e| Error::config("hardware", e.to_string()))?;
            validate_hardware_segment(&seg)?;
            Ok(seg)
        }
        None => Ok(default_hardware_segment()),
    }
}

fn load_audio_segment_value(reader: &dyn ConfigFileStore) -> Result<AudioSegment> {
    match reader.read_config_file("config/audio.json")? {
        Some(bytes) if bytes.iter().all(|b| b.is_ascii_whitespace()) => {
            Ok(default_disabled_audio_segment())
        }
        Some(bytes) => {
            let json =
                std::str::from_utf8(&bytes).map_err(|e| Error::config("audio", e.to_string()))?;
            let mut seg = deserialize_storage_json_loose_tail::<AudioSegment>(json)
                .map_err(|e| Error::config("audio", e.to_string()))?;
            if seg.version == 0 {
                seg.version = AUDIO_CONFIG_VERSION;
            }
            normalize_audio_segment(&mut seg);
            validate_audio_segment(&seg)?;
            Ok(seg)
        }
        None => Ok(default_disabled_audio_segment()),
    }
}

fn validate_pin_range(pin: i32, stage: &'static str) -> Result<()> {
    if !(HARDWARE_PIN_MIN..=HARDWARE_PIN_MAX).contains(&pin) {
        return Err(Error::config(
            stage,
            format!(
                "pin {} out of range {}..={}",
                pin, HARDWARE_PIN_MIN, HARDWARE_PIN_MAX
            ),
        ));
    }
    if HARDWARE_FORBIDDEN_PINS.contains(&pin) {
        return Err(Error::config(
            stage,
            format!("pin {} is forbidden (strapping pin)", pin),
        ));
    }
    Ok(())
}

fn collect_display_pins(cfg: &DisplayConfig) -> Vec<(String, i32)> {
    #[cfg(target_os = "linux")]
    if !is_framebuffer_config(cfg) {
        let mut out = vec![("dc".to_string(), cfg.spi.dc)];
        if let Some(v) = cfg.spi.rst {
            out.push(("rst".to_string(), v));
        }
        if let Some(v) = cfg.spi.bl {
            out.push(("bl".to_string(), v));
        }
        return out;
    }

    let mut out = vec![
        ("sclk".to_string(), cfg.spi.sclk),
        ("mosi".to_string(), cfg.spi.mosi),
        ("cs".to_string(), cfg.spi.cs),
        ("dc".to_string(), cfg.spi.dc),
    ];
    if let Some(v) = cfg.spi.rst {
        out.push(("rst".to_string(), v));
    }
    if let Some(v) = cfg.spi.bl {
        out.push(("bl".to_string(), v));
    }
    out
}

fn validate_display_segment(cfg: &DisplayConfig, hardware_devices: &[DeviceEntry]) -> Result<()> {
    validate_display_config_core(cfg)?;
    if !cfg.enabled {
        return Ok(());
    }
    // Framebuffer 模式无 SPI 引脚，跳过 pin 冲突检查。
    if is_framebuffer_config(cfg) {
        return Ok(());
    }

    let pins = collect_display_pins(cfg);
    let mut seen = std::collections::HashSet::new();
    for (name, pin) in &pins {
        validate_pin_range(*pin, "display")?;
        if !seen.insert(*pin) {
            return Err(Error::config(
                "display",
                format!(
                    "DISPLAY_CONFIG_PIN_CONFLICT_INTERNAL: duplicate pin {} found at {}",
                    pin, name
                ),
            ));
        }
    }

    let mut external = std::collections::HashSet::new();
    for dev in hardware_devices {
        for pin in dev.pins.values() {
            external.insert(*pin);
        }
    }
    for (name, pin) in pins {
        if external.contains(&pin) {
            return Err(Error::config(
                "display",
                format!(
                    "DISPLAY_CONFIG_PIN_CONFLICT_EXTERNAL: display {} pin {} conflicts with hardware_devices",
                    name, pin
                ),
            ));
        }
    }
    Ok(())
}

fn validate_llm_segment(seg: &LlmSegment) -> Result<()> {
    for (i, src) in seg.llm_sources.iter().enumerate() {
        if src.provider.trim().is_empty() {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}].provider is required (cannot be empty)", i),
            ));
        }
        if src.api_key.trim().is_empty() {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}].api_key is required (cannot be empty)", i),
            ));
        }
        if src.model.trim().is_empty() {
            return Err(Error::config(
                "config",
                format!("llm_sources[{}].model is required (cannot be empty)", i),
            ));
        }
    }
    validate_llm_sources(&seg.llm_sources)?;
    Ok(())
}

/// 校验 LlmSegment 并写入 storage config/llm.json；body 即全量，不做合并。
pub fn save_llm_segment(writer: &dyn ConfigFileStore, body: &str) -> Result<()> {
    let seg: LlmSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    save_llm_segment_value(writer, &seg)
}

/// 校验 LlmSegment 并写入 storage config/llm.json；直接消费已解析的配置对象。
pub fn save_llm_segment_value(writer: &dyn ConfigFileStore, seg: &LlmSegment) -> Result<()> {
    validate_llm_segment(seg)?;
    let json =
        serde_json::to_string(&seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/llm.json", json.as_bytes())?;
    Ok(())
}

/// 校验 ChannelsSegment 并写入 storage config/channels.json；直接消费已解析的配置对象。
pub fn save_channels_segment_value(
    writer: &dyn ConfigFileStore,
    seg: &ChannelsSegment,
) -> Result<()> {
    let _guard = lock_channels_config();
    save_channels_segment_value_unlocked(writer, seg)
}

fn save_channels_segment_value_unlocked(
    writer: &dyn ConfigFileStore,
    seg: &ChannelsSegment,
) -> Result<()> {
    validate_channels_segment_fields(seg)?;
    let json = serde_json::to_string(seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/channels.json", json.as_bytes())?;
    Ok(())
}

/// 只更新 channels.json 内的 Telegram 群组触发策略；不触碰 NVS。
pub fn save_tg_group_activation_to_channels(
    writer: &dyn ConfigFileStore,
    value: &str,
) -> Result<()> {
    validate_tg_group_activation(value)?;
    let _guard = lock_channels_config();
    let mut segment = match writer.read_config_file("config/channels.json")? {
        Some(bytes) if bytes.iter().all(|b| b.is_ascii_whitespace()) => {
            ChannelsSegment::from_app_config(&AppConfig::load_from_env())
        }
        Some(bytes) => {
            let json = std::str::from_utf8(&bytes)
                .map_err(|e| Error::config("channels", e.to_string()))?;
            deserialize_storage_json_loose_tail::<ChannelsSegment>(json)
                .map_err(|e| Error::config("channels", e.to_string()))?
        }
        None => ChannelsSegment::from_app_config(&AppConfig::load_from_env()),
    };
    segment.tg_group_activation = value.to_string();
    save_channels_segment_value_unlocked(writer, &segment)
}

/// 将已通过保存校验的 LlmSegment 投影回运行时缓存。
pub(crate) fn apply_llm_segment_to_config(config: &mut AppConfig, seg: &LlmSegment) {
    config.llm_sources = seg.llm_sources.clone();
    if let Some(first) = config.llm_sources.first() {
        config.api_key = first.api_key.clone();
        config.model = first.model.clone();
        config.model_provider = first.provider.clone();
        config.api_url = first.api_url.clone();
    }
}

/// 将已通过保存校验的 ChannelsSegment 投影回运行时缓存。
pub(crate) fn apply_channels_segment_to_config(config: &mut AppConfig, seg: &ChannelsSegment) {
    config.tg_group_activation = seg.tg_group_activation.clone();
    config.tg_token = seg.tg_token.clone();
    config.tg_allowed_chat_ids = seg.tg_allowed_chat_ids.clone();
    config.feishu_app_id = seg.feishu_app_id.clone();
    config.feishu_app_secret = seg.feishu_app_secret.clone();
    config.feishu_allowed_chat_ids = seg.feishu_allowed_chat_ids.clone();
    config.dingtalk_client_id = seg.dingtalk_client_id.clone();
    config.dingtalk_client_secret = seg.dingtalk_client_secret.clone();
    config.wecom_bot_id = seg.wecom_bot_id.clone();
    config.wecom_bot_secret = seg.wecom_bot_secret.clone();
    config.wecom_ws_url = seg.wecom_ws_url.clone();
    config.qq_channel_app_id = seg.qq_channel_app_id.clone();
    config.qq_channel_secret = seg.qq_channel_secret.clone();
    config.webhook_enabled = seg.webhook_enabled;
    config.webhook_token = seg.webhook_token.clone();
    config.enabled_channel = seg.enabled_channel.clone();
}

/// 校验 SystemSegment 并写入对应 NVS 键；body 即全量，不做合并。
#[cfg(test)]
pub(crate) fn save_system_segment_to_nvs(store: &dyn ConfigStore, body: &str) -> Result<()> {
    let seg: SystemSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    save_system_segment_value_to_nvs(store, &seg)
}

/// 校验 SystemSegment 并写入对应 NVS 键；直接消费已解析的系统配置对象。
pub(crate) fn save_system_segment_value_to_nvs(
    store: &dyn ConfigStore,
    seg: &SystemSegment,
) -> Result<()> {
    validate_system_segment_fields(seg)?;
    let mut pairs: Vec<(&str, &str)> = vec![
        (NVS_KEY_WIFI_SSID, &seg.wifi_ssid),
        (NVS_KEY_WIFI_PASS, &seg.wifi_pass),
        (NVS_KEY_PROXY_URL, &seg.proxy_url),
    ];
    if let Some(locale) = normalize_optional_locale(seg.locale.as_deref())? {
        pairs.push((NVS_KEY_LOCALE, locale));
    }
    store.write_strings(&pairs)?;
    Ok(())
}

/// 将 WiFi 配置投影回运行时缓存；仅修改当前 handler 已持久化的字段。
pub(crate) fn apply_wifi_to_config(config: &mut AppConfig, wifi_ssid: &str, wifi_pass: &str) {
    config.wifi_ssid = wifi_ssid.to_string();
    config.wifi_pass = wifi_pass.to_string();
}

/// 将已通过保存校验的 SystemSegment 投影回运行时缓存。
pub(crate) fn apply_system_segment_to_config(config: &mut AppConfig, seg: &SystemSegment) {
    apply_wifi_to_config(config, &seg.wifi_ssid, &seg.wifi_pass);
    config.proxy_url = seg.proxy_url.clone();
    if let Some(locale) = seg.locale.as_deref().map(str::trim) {
        config.locale = Some(locale.to_string());
    }
}

/// 校验 HardwareSegment 并写入 storage config/hardware.json；body 即全量，不做合并。
pub fn save_hardware_segment(writer: &dyn ConfigFileStore, body: &str) -> Result<()> {
    let seg: HardwareSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    save_hardware_segment_value(writer, &seg)
}

/// 校验 HardwareSegment 并写入 storage config/hardware.json；直接消费已解析的配置对象。
pub fn save_hardware_segment_value(
    writer: &dyn ConfigFileStore,
    seg: &HardwareSegment,
) -> Result<()> {
    validate_hardware_segment(seg)?;
    let audio = load_audio_segment_value(writer)?;
    validate_audio_hardware_pair("hardware", &audio, seg)?;
    let json =
        serde_json::to_string(&seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/hardware.json", json.as_bytes())?;
    Ok(())
}

/// GET /api/config/audio：返回 audio.json 内容（不存在时返回 disabled 默认配置）。
pub fn get_audio_segment(reader: &dyn ConfigFileStore) -> Result<String> {
    match reader.read_config_file("config/audio.json")? {
        Some(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        None => serde_json::to_string(&default_disabled_audio_segment())
            .map_err(|e| Error::config("audio", e.to_string())),
    }
}

/// POST /api/config/audio：校验并写入 storage config/audio.json；body 即全量，不做合并。
pub fn save_audio_segment(writer: &dyn ConfigFileStore, body: &str) -> Result<()> {
    let seg: AudioSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    save_audio_segment_value(writer, seg).map(|_| ())
}

/// POST /api/config/audio：校验并写入 storage config/audio.json；返回规范化后的配置对象。
pub fn save_audio_segment_value(
    writer: &dyn ConfigFileStore,
    mut seg: AudioSegment,
) -> Result<AudioSegment> {
    if seg.version == 0 {
        seg.version = AUDIO_CONFIG_VERSION;
    }
    normalize_audio_segment(&mut seg);
    validate_audio_segment(&seg)?;
    let hardware = load_hardware_segment_value(writer)?;
    validate_audio_hardware_pair("audio", &seg, &hardware)?;
    let json =
        serde_json::to_string(&seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/audio.json", json.as_bytes())?;
    Ok(seg)
}

/// GET /api/config/display：返回 display.json 内容（不存在时返回 disabled 默认配置）。
pub fn get_display_segment(reader: &dyn ConfigFileStore) -> Result<String> {
    match reader.read_config_file("config/display.json")? {
        Some(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        None => serde_json::to_string(&default_disabled_display_config())
            .map_err(|e| Error::config("display", e.to_string())),
    }
}

/// POST /api/config/display：校验并写入 storage config/display.json；body 即全量，不做合并。
pub fn save_display_segment(
    writer: &dyn ConfigFileStore,
    hardware_devices: &[DeviceEntry],
    body: &str,
) -> Result<()> {
    let seg: DisplayConfig =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    save_display_segment_value(writer, hardware_devices, seg).map(|_| ())
}

/// POST /api/config/display：校验并写入 storage config/display.json；返回规范化后的配置对象。
pub fn save_display_segment_value(
    writer: &dyn ConfigFileStore,
    hardware_devices: &[DeviceEntry],
    mut seg: DisplayConfig,
) -> Result<DisplayConfig> {
    if seg.version == 0 {
        seg.version = DISPLAY_CONFIG_VERSION;
    }
    validate_display_segment(&seg, hardware_devices)?;
    let json =
        serde_json::to_string(&seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/display.json", json.as_bytes())?;
    Ok(seg)
}

/// 将已通过保存校验的 HardwareSegment 投影回运行时缓存。
pub(crate) fn apply_hardware_segment_to_config(config: &mut AppConfig, seg: &HardwareSegment) {
    config.hardware_devices = seg.hardware_devices.clone();
    config.i2c_bus = seg.i2c_bus.clone();
    config.i2s_bus = seg.i2s_bus.clone();
    config.i2c_devices = seg.i2c_devices.clone();
    config.i2c_sensors = seg.i2c_sensors.clone();
    drop_invalid_display_after_hardware_update(config);
}

fn drop_invalid_display_after_hardware_update(config: &mut AppConfig) {
    let invalid = config
        .display
        .as_ref()
        .and_then(|display| validate_display_segment(display, &config.hardware_devices).err());
    if let Some(error) = invalid {
        log::warn!(
            "[config] cached display invalid after hardware update; dropping display cache: {}",
            error
        );
        config.display = None;
    }
}

/// 将已通过保存校验的 AudioSegment 投影回运行时缓存。
pub(crate) fn apply_audio_segment_to_config(config: &mut AppConfig, seg: AudioSegment) {
    config.audio = Some(seg);
}

/// 将已通过保存校验的 DisplayConfig 投影回运行时缓存。
pub(crate) fn apply_display_segment_to_config(config: &mut AppConfig, seg: DisplayConfig) {
    config.display = Some(seg);
}

/// 读取 `config/accounts.json` authority 段；不存在时返回空账户注册表默认值。
#[cfg(feature = "capability_office")]
pub fn get_office_accounts_segment(reader: &dyn ConfigFileStore) -> Result<String> {
    match reader.read_config_file("config/accounts.json")? {
        Some(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
        None => serde_json::to_string(&OfficeAccountsSegment::default())
            .map_err(|e| Error::config("office_accounts", e.to_string())),
    }
}

/// 校验并整体写入 `config/accounts.json` authority 段；body 为完整注册表快照，不做合并。
#[cfg(feature = "capability_office")]
pub fn save_office_accounts_segment(writer: &dyn ConfigFileStore, body: &str) -> Result<()> {
    let seg: OfficeAccountsSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    validate_office_accounts_segment(&seg)?;
    let json =
        serde_json::to_string(&seg).map_err(|e| Error::config("serialize", e.to_string()))?;
    writer.write_config_file("config/accounts.json", json.as_bytes())?;
    Ok(())
}

#[cfg(feature = "capability_office")]
pub fn validate_office_accounts_candidate(seg: &OfficeAccountsSegment) -> Result<()> {
    validate_office_accounts_segment(seg)
}

/// 读取 office credential authority，序列化为 `OfficeCredentialsSegment` JSON。
#[cfg(feature = "capability_office")]
pub fn get_office_credentials_segment(store: &dyn OfficeCredentialStore) -> Result<String> {
    let mut items = store.list()?;
    items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
    serde_json::to_string(&OfficeCredentialsSegment { items })
        .map_err(|e| Error::config("office_credentials", e.to_string()))
}

/// 严格解析并整体替换 office credential authority。
#[cfg(feature = "capability_office")]
pub fn save_office_credentials_segment(
    store: &dyn OfficeCredentialStore,
    body: &str,
) -> Result<()> {
    let seg: OfficeCredentialsSegment =
        serde_json::from_str(body).map_err(|e| Error::config("deserialize", e.to_string()))?;
    validate_office_credentials_segment(&seg)?;
    let current_keys = store
        .list()?
        .into_iter()
        .map(|item| item.account_key)
        .collect::<std::collections::BTreeSet<_>>();
    let next_keys = seg
        .items
        .iter()
        .map(|item| item.account_key.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for account_key in current_keys.difference(&next_keys) {
        store.clear(account_key)?;
    }
    for credential in &seg.items {
        store.set(credential)?;
    }
    Ok(())
}

#[cfg(feature = "capability_office")]
pub fn validate_office_credentials_candidate(seg: &OfficeCredentialsSegment) -> Result<()> {
    validate_office_credentials_segment(seg)
}

/// 单条 ID 最大长度、白名单最大条数（避免滥用）。
/// 单条 chat_id 最大长度（飞书 oc_xxx 等可超过 32）。
const MAX_ALLOWED_ID_LEN: usize = 64;
const MAX_ALLOWED_COUNT: usize = 64;

/// 解析逗号分隔的 chat_id 白名单；空字符串返回空 vec；超长或超条数截断。
/// 约定：空列表 = 拒绝所有；非空 = 仅允许列表中的 chat_id。
pub fn parse_allowed_chat_ids(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return vec![];
    }
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .take(MAX_ALLOWED_COUNT)
        .map(|x| {
            if x.len() > MAX_ALLOWED_ID_LEN {
                x.chars().take(MAX_ALLOWED_ID_LEN).collect()
            } else {
                x
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MultiFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MultiFileStore {
        fn with_file(path: &str, data: impl Into<Vec<u8>>) -> Self {
            let mut files = HashMap::new();
            files.insert(path.to_string(), data.into());
            Self {
                files: Mutex::new(files),
            }
        }
    }

    impl ConfigFileStore for MultiFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    fn aht20_sensor_for_tests() -> I2cSensorEntry {
        I2cSensorEntry {
            id: "box_aht20".into(),
            addr: 0x38,
            model: "aht20".into(),
            what: "AHT20 temperature and humidity sensor".into(),
            how: "AHT20 on I2C expansion bus".into(),
            options: serde_json::json!({}),
        }
    }

    #[test]
    fn hardware_validation_requires_i2c_bus_for_i2c_sensors() {
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: None,
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![aht20_sensor_for_tests()],
        };

        assert!(validate_hardware_segment(&segment)
            .unwrap_err()
            .to_string()
            .contains("i2c_bus is required"));
    }

    #[test]
    fn hardware_validation_rejects_invalid_i2c_bus_pins() {
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: Some(I2cBusConfig {
                sda_pin: 49,
                scl_pin: 40,
                freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
            }),
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![],
        };

        assert!(validate_hardware_segment(&segment)
            .unwrap_err()
            .to_string()
            .contains("i2c_bus.sda_pin"));
    }

    #[test]
    fn hardware_validation_rejects_same_i2c_bus_pins() {
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: Some(I2cBusConfig {
                sda_pin: 41,
                scl_pin: 41,
                freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
            }),
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![],
        };

        assert!(validate_hardware_segment(&segment)
            .unwrap_err()
            .to_string()
            .contains("must be different"));
    }

    #[test]
    fn hardware_validation_accepts_aht20_i2c_segment() {
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: Some(I2cBusConfig {
                sda_pin: 21,
                scl_pin: 22,
                freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
            }),
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![aht20_sensor_for_tests()],
        };

        validate_hardware_segment(&segment).unwrap();
    }

    #[test]
    fn hardware_validation_rejects_duplicate_i2s_bus_pins() {
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: None,
            i2s_bus: Some(I2sBusConfig {
                mclk_pin: 2,
                ws_pin: 45,
                bclk_pin: 17,
                din_pin: 16,
                dout_pin: 16,
            }),
            i2c_devices: vec![],
            i2c_sensors: vec![],
        };

        assert!(validate_hardware_segment(&segment)
            .unwrap_err()
            .to_string()
            .contains("i2s_bus.dout_pin duplicates"));
    }

    #[test]
    fn hardware_segment_round_trips_i2s_bus() {
        let store = MultiFileStore::default();
        let segment = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: Some(I2cBusConfig {
                sda_pin: 8,
                scl_pin: 18,
                freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
            }),
            i2s_bus: Some(I2sBusConfig {
                mclk_pin: 2,
                ws_pin: 45,
                bclk_pin: 17,
                din_pin: 16,
                dout_pin: 15,
            }),
            i2c_devices: vec![],
            i2c_sensors: vec![],
        };

        save_hardware_segment_value(&store, &segment).expect("save hardware");
        let written = store
            .read_config_file("config/hardware.json")
            .expect("read")
            .expect("written");
        let saved: HardwareSegment =
            serde_json::from_slice(&written).expect("parse saved hardware");
        let bus = saved.i2s_bus.expect("saved i2s bus");
        assert_eq!(bus.mclk_pin, 2);
        assert_eq!(bus.ws_pin, 45);
        assert_eq!(bus.bclk_pin, 17);
        assert_eq!(bus.din_pin, 16);
        assert_eq!(bus.dout_pin, 15);
    }

    #[test]
    fn llm_segment_from_app_config_prefers_runtime_llm_sources() {
        let mut config = AppConfig::load_from_env();
        config.llm_sources = vec![LlmSource {
            id: "primary".to_string(),
            provider: "openai".to_string(),
            api_key: "source-key".to_string(),
            model: "gpt-4o-mini".to_string(),
            api_url: "https://api.openai.com/v1".to_string(),
            max_tokens: Some(2048),
            model_kind: LlmModelKind::Text,
            custom_headers: vec![LlmHeaderEntry {
                name: "X-Title".to_string(),
                value: "Beetle".to_string(),
            }],
        }];

        let segment = LlmSegment::from_app_config(&config);
        assert_eq!(segment.llm_sources.len(), 1);
        assert_eq!(segment.llm_sources[0].id, "primary");
        assert_eq!(segment.llm_sources[0].api_key, "source-key");
        assert_eq!(segment.llm_sources[0].model, "gpt-4o-mini");
        assert_eq!(segment.llm_sources[0].max_tokens, Some(2048));
        assert_eq!(segment.llm_sources[0].model_kind, LlmModelKind::Text);
        assert_eq!(segment.llm_sources[0].custom_headers[0].name, "X-Title");
    }

    #[test]
    fn llm_segment_rejects_unknown_model_kind() {
        struct MemoryFileStore;

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }

            fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                Ok(())
            }
        }

        let body = r#"{"llm_sources":[{
            "id":"src-text",
            "provider":"openai",
            "api_key":"source-key",
            "model":"gpt-4o-mini",
            "api_url":"https://api.openai.com/v1",
            "model_kind":"audio",
            "custom_headers":[]
        }]}"#;

        let err = save_llm_segment(&MemoryFileStore, body)
            .expect_err("unsupported model_kind must be rejected");
        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn llm_segment_rejects_blank_model() {
        struct MemoryFileStore;

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }

            fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                Ok(())
            }
        }

        let body = r#"{"llm_sources":[{
            "id":"src-text",
            "provider":"openai",
            "api_key":"source-key",
            "model":"   ",
            "api_url":"https://api.openai.com/v1",
            "model_kind":"text",
            "custom_headers":[]
        }]}"#;

        let err = save_llm_segment(&MemoryFileStore, body)
            .expect_err("blank model must be rejected before runtime filtering");
        assert!(err.to_string().contains("model is required"));
    }

    #[test]
    fn llm_segment_rejects_duplicate_custom_headers_case_insensitive() {
        struct MemoryFileStore;

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }

            fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                Ok(())
            }
        }

        let body = r#"{"llm_sources":[{
            "id":"src-text",
            "provider":"openai",
            "api_key":"source-key",
            "model":"gpt-4o-mini",
            "api_url":"https://api.openai.com/v1",
            "model_kind":"text",
            "custom_headers":[
                {"name":"X-Provider-Trace","value":"a"},
                {"name":"x-provider-trace","value":"b"}
            ]
        }]}"#;

        let err = save_llm_segment(&MemoryFileStore, body)
            .expect_err("duplicate custom header names must be rejected");
        assert!(err.to_string().contains("custom_headers"));
    }

    #[test]
    fn merge_llm_from_json_rejects_semantically_invalid_storage_segment() {
        let mut config = AppConfig::load_from_env();
        config.api_key = "env-key".to_string();
        config.model = "env-model".to_string();
        config.model_provider = "openai".to_string();
        config.api_url = "https://env.example.test/v1".to_string();
        let mut errors = Vec::new();

        config.merge_llm_from_json(
            r#"{"llm_sources":[
                {
                    "id":"dup",
                    "provider":"openai",
                    "api_key":"source-key-1",
                    "model":"gpt-4o-mini",
                    "api_url":"https://api.openai.com/v1",
                    "model_kind":"text",
                    "custom_headers":[]
                },
                {
                    "id":"dup",
                    "provider":"openai",
                    "api_key":"source-key-2",
                    "model":"gpt-4o-mini",
                    "api_url":"https://api.openai.com/v1",
                    "model_kind":"text",
                    "custom_headers":[]
                }
            ]}"#,
            &mut errors,
        );

        assert_eq!(errors, vec!["llm_json_invalid"]);
        assert!(config.llm_sources.is_empty());
        assert_eq!(config.api_key, "env-key");
        assert_eq!(config.model, "env-model");
    }

    #[test]
    fn llm_segment_from_app_config_falls_back_to_legacy_single_source_fields() {
        let mut config = AppConfig::load_from_env();
        config.llm_sources.clear();
        config.model_provider = "deepseek".to_string();
        config.api_key = "legacy-key".to_string();
        config.model = "deepseek-chat".to_string();
        config.api_url = "https://api.deepseek.com".to_string();

        let segment = LlmSegment::from_app_config(&config);
        assert_eq!(segment.llm_sources.len(), 1);
        assert_eq!(segment.llm_sources[0].provider, "deepseek");
        assert_eq!(segment.llm_sources[0].api_key, "legacy-key");
        assert_eq!(segment.llm_sources[0].model, "deepseek-chat");
        assert_eq!(segment.llm_sources[0].api_url, "https://api.deepseek.com");
        assert_eq!(segment.llm_sources[0].max_tokens, None);
        assert_eq!(segment.llm_sources[0].id, "env_default");
        assert_eq!(segment.llm_sources[0].model_kind, LlmModelKind::Text);
        assert!(segment.llm_sources[0].custom_headers.is_empty());
    }

    #[test]
    fn audio_validation_allows_empty_speech_fallback_when_wake_word_disabled() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn audio_runtime_pipeline_requires_physical_audio_endpoint() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.ambient_listening.enabled = true;
        seg.led_indicator.enabled = true;

        assert!(!audio_runtime_pipeline_enabled(&seg));

        seg.speaker.enabled = true;
        assert!(audio_runtime_pipeline_enabled(&seg));
    }

    #[test]
    fn proxy_validation_rejects_valid_proxy_on_unsupported_target() {
        let error = validate_proxy_url_for_target("http://proxy.local:8080", false)
            .expect_err("proxy must be rejected when target does not support CONNECT");

        assert!(error.to_string().contains("proxy_url is not supported"));
    }

    #[test]
    fn proxy_sanitize_drops_loaded_proxy_on_unsupported_target() {
        let mut config = AppConfig::load_from_env();
        let mut load_errors = Vec::new();
        config.proxy_url = "http://proxy.local:8080".to_string();

        sanitize_proxy_url_for_target(&mut config, false, &mut load_errors);

        assert!(config.proxy_url.is_empty());
        assert_eq!(load_errors, vec!["proxy_unsupported_on_target"]);
    }

    #[cfg(feature = "dingtalk")]
    #[test]
    fn dingtalk_channel_requires_stream_credentials() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = "dingtalk".to_string();
        config.dingtalk_client_id.clear();
        config.dingtalk_client_secret.clear();

        let error = config
            .validate_for_channels()
            .expect_err("dingtalk requires stream credentials");
        assert!(error.to_string().contains(
            "enabled_channel=dingtalk requires dingtalk_client_id and dingtalk_client_secret"
        ));

        config.dingtalk_client_id = "ding-client".to_string();
        config.dingtalk_client_secret = "ding-secret".to_string();
        config
            .validate_for_channels()
            .expect("dingtalk stream credentials are sufficient");
    }

    #[cfg(feature = "wecom")]
    #[test]
    fn wecom_channel_requires_aibot_credentials() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = "wecom".to_string();
        config.wecom_bot_id.clear();
        config.wecom_bot_secret.clear();

        let error = config
            .validate_for_channels()
            .expect_err("wecom requires ai bot credentials");
        assert!(error
            .to_string()
            .contains("enabled_channel=wecom requires wecom_bot_id and wecom_bot_secret"));

        config.wecom_bot_id = "bot-id".to_string();
        config.wecom_bot_secret = "bot-secret".to_string();
        config
            .validate_for_channels()
            .expect("wecom ai bot credentials are sufficient");
    }

    #[test]
    fn channels_segment_ignores_unknown_removed_fields() {
        let json = r#"{
            "enabled_channel": "telegram",
            "tg_token": "123456:token",
            "removed_social_webhook_field": "ignored"
        }"#;
        let seg: ChannelsSegment = serde_json::from_str(json).expect("unknown fields ignored");
        assert_eq!(seg.enabled_channel, "telegram");
        assert_eq!(seg.tg_token, "123456:token");

        let serialized = serde_json::to_value(&seg).expect("serialize segment");
        assert!(!serialized
            .as_object()
            .unwrap()
            .contains_key("removed_social_webhook_field"));
    }

    #[test]
    fn merge_channels_preserves_unavailable_enabled_channel_for_api_warning() {
        let mut config = AppConfig::load_from_env();
        let mut errors = Vec::new();

        config.merge_channels_from_json(
            r#"{"enabled_channel":"future_channel","tg_token":"saved-token"}"#,
            &mut errors,
        );

        assert!(errors.is_empty());
        assert_eq!(config.enabled_channel, "future_channel");
        assert_eq!(config.tg_token, "saved-token");
        assert_eq!(
            ChannelsSegment::from_app_config(&config).enabled_channel,
            ""
        );
    }

    #[test]
    fn merge_channels_from_json_rejects_oversized_field_without_partial_merge() {
        let mut config = AppConfig::load_from_env();
        let original_channel = config.enabled_channel.clone();
        let original_token = config.tg_token.clone();
        let oversized_token = "x".repeat(CONFIG_FIELD_MAX_LEN + 1);
        let mut errors = Vec::new();

        config.merge_channels_from_json(
            &format!(
                r#"{{"enabled_channel":"future_channel","tg_token":"{}"}}"#,
                oversized_token
            ),
            &mut errors,
        );

        assert_eq!(errors, ["channels_json_invalid"]);
        assert_eq!(config.enabled_channel, original_channel);
        assert_eq!(config.tg_token, original_token);
    }

    #[test]
    fn audio_validation_ignores_hidden_realtime_config_when_wake_word_disabled() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.realtime.api_key = "test-key".to_string();
        seg.realtime.model = "gpt-realtime".to_string();
        seg.realtime.voice = "alloy".to_string();
        seg.realtime.ws_url = "wss://api.openai.com/v1/realtime".to_string();

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn audio_validation_i2s_codec_accepts_pa_pin_46_and_ignores_legacy_pins() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = 24_000;
        seg.speaker.sample_rate = 24_000;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.pa_pin = Some(46);
        seg.codec.input_reference = true;
        seg.microphone.pins.ws = 0;
        seg.microphone.pins.sck = 0;
        seg.microphone.pins.din = 0;
        seg.speaker.pins = None;

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn audio_validation_i2s_codec_requires_matching_sample_rates() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = 16_000;
        seg.speaker.sample_rate = 24_000;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.pa_pin = Some(46);

        let error = validate_audio_segment(&seg)
            .expect_err("codec topology should enforce equal sample rates");
        assert!(error
            .to_string()
            .contains("speaker.sample_rate must equal microphone.sample_rate"));
    }

    #[test]
    fn audio_validation_requires_realtime_sample_rate_when_wake_word_enabled() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.wake_word.enabled = true;
        seg.realtime.api_key = "test-key".to_string();
        seg.realtime.model = "gpt-realtime".to_string();
        seg.realtime.voice = "alloy".to_string();
        seg.realtime.ws_url = "wss://api.openai.com/v1/realtime".to_string();

        let error = validate_audio_segment(&seg).expect_err("wake realtime should enforce 24kHz");
        assert!(error
            .to_string()
            .contains("microphone.sample_rate must equal 24000 for realtime voice"));
    }

    #[test]
    fn audio_validation_allows_acoustic_wake_without_keyword_model() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.wake_word.enabled = true;
        seg.wake_word.keyword = "legacy-hidden-value".to_string();
        seg.speech.api_key = "test-key".to_string();
        seg.speech.api_secret = "test-secret".to_string();

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn audio_validation_rejects_acoustic_threshold_inversion() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.wake_word.enabled = true;
        seg.wake_word.leave_threshold = seg.wake_word.enter_threshold;
        seg.speech.api_key = "test-key".to_string();
        seg.speech.api_secret = "test-secret".to_string();

        let error = validate_audio_segment(&seg).expect_err("leave threshold must be below enter");
        assert!(error.to_string().contains(
            "wake_word.leave_threshold must be >= 0 and less than wake_word.enter_threshold"
        ));
    }

    #[test]
    fn doubao_realtime_validation_requires_model_voice_and_api_key() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.wake_word.enabled = true;
        seg.realtime.provider = AUDIO_REALTIME_PROVIDER_DOUBAO.to_string();
        seg.realtime.ws_url =
            audio_realtime_default_ws_url(AUDIO_REALTIME_PROVIDER_DOUBAO).to_string();
        seg.realtime.api_key = "key".to_string();
        seg.realtime.model = "doubao-realtime".to_string();
        seg.realtime.voice = "zh_female".to_string();
        seg.microphone.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
        seg.speaker.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn qwen_default_voice_matches_qwen35_default_model() {
        assert_eq!(
            audio_realtime_default_model(AUDIO_REALTIME_PROVIDER_QWEN),
            "qwen3.5-omni-plus-realtime"
        );
        assert_eq!(
            audio_realtime_default_voice(AUDIO_REALTIME_PROVIDER_QWEN),
            "Tina"
        );
    }

    #[test]
    fn realtime_provider_support_removes_baidu_and_keeps_doubao() {
        assert!(audio_realtime_provider_supported(
            AUDIO_REALTIME_PROVIDER_DOUBAO
        ));
        assert!(!audio_realtime_provider_supported("baidu"));
    }

    #[test]
    fn realtime_schema_omits_removed_baidu_fields() {
        let value = serde_json::to_value(default_disabled_audio_segment()).unwrap();
        let realtime = value.get("realtime").and_then(|v| v.as_object()).unwrap();
        assert!(!realtime.contains_key("api_secret"));
        assert!(!realtime.contains_key("app_id"));
        assert!(!realtime.contains_key("user_id"));
        assert!(!realtime.contains_key("license_key"));
        assert!(!realtime.contains_key("device_id"));
    }

    #[test]
    fn audio_validation_allows_usb_speaker_without_pins() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.speaker.enabled = true;
        seg.speaker.device_type = AUDIO_SPEAKER_DEVICE_USB.to_string();
        seg.speaker.device_ref = Some("usb:vid=1234:pid=5678:serial=test".to_string());
        seg.speaker.pins = None;

        assert!(validate_audio_segment(&seg).is_ok());
    }

    #[test]
    fn audio_validation_requires_pins_for_i2s_speaker() {
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.speaker.enabled = true;
        seg.speaker.pins = None;

        let error = validate_audio_segment(&seg).expect_err("i2s speaker should require pins");
        assert!(error.to_string().contains("speaker.pins are required"));
    }

    #[test]
    fn audio_segment_round_trips_topology_and_codec() {
        let store = MultiFileStore::with_file(
            "config/hardware.json",
            serde_json::to_vec(&HardwareSegment {
                hardware_devices: vec![],
                i2c_bus: Some(I2cBusConfig {
                    sda_pin: 8,
                    scl_pin: 18,
                    freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
                }),
                i2s_bus: Some(I2sBusConfig {
                    mclk_pin: 2,
                    ws_pin: 45,
                    bclk_pin: 17,
                    din_pin: 16,
                    dout_pin: 15,
                }),
                i2c_devices: vec![],
                i2c_sensors: vec![],
            })
            .expect("serialize hardware"),
        );
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = 24_000;
        seg.speaker.sample_rate = 24_000;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.input_addr = Some(0x40);
        seg.codec.output_addr = Some(0x18);
        seg.codec.pa_pin = Some(46);
        seg.codec.input_reference = true;

        save_audio_segment_value(&store, seg).expect("save audio");
        let written = store
            .read_config_file("config/audio.json")
            .expect("read")
            .expect("written");
        let saved: AudioSegment = serde_json::from_slice(&written).expect("parse saved audio");
        assert_eq!(saved.topology, AUDIO_TOPOLOGY_I2S_CODEC);
        assert_eq!(
            saved.codec.input_codec.as_deref(),
            Some(AUDIO_CODEC_INPUT_ES7210)
        );
        assert_eq!(
            saved.codec.output_codec.as_deref(),
            Some(AUDIO_CODEC_OUTPUT_ES8311)
        );
        assert_eq!(saved.codec.input_addr, Some(0x40));
        assert_eq!(saved.codec.output_addr, Some(0x18));
        assert_eq!(saved.codec.pa_pin, Some(46));
        assert!(saved.codec.input_reference);
    }

    #[test]
    fn es7210_codec_wake_defaults_are_normalized_for_esp_box3_levels() {
        let store = MultiFileStore::with_file(
            "config/hardware.json",
            serde_json::to_vec(&HardwareSegment {
                hardware_devices: vec![],
                i2c_bus: Some(I2cBusConfig {
                    sda_pin: 8,
                    scl_pin: 18,
                    freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
                }),
                i2s_bus: Some(I2sBusConfig {
                    mclk_pin: 2,
                    ws_pin: 45,
                    bclk_pin: 17,
                    din_pin: 16,
                    dout_pin: 15,
                }),
                i2c_devices: vec![],
                i2c_sensors: vec![],
            })
            .expect("serialize hardware"),
        );
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
        seg.speaker.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.pa_pin = Some(46);
        seg.codec.input_reference = true;
        seg.wake_word.enabled = true;
        seg.realtime.provider = AUDIO_REALTIME_PROVIDER_QWEN.to_string();
        seg.realtime.ws_url =
            audio_realtime_default_ws_url(AUDIO_REALTIME_PROVIDER_QWEN).to_string();
        seg.realtime.api_key = "test-key".to_string();
        seg.realtime.model = audio_realtime_default_model(AUDIO_REALTIME_PROVIDER_QWEN).to_string();
        seg.realtime.voice = audio_realtime_default_voice(AUDIO_REALTIME_PROVIDER_QWEN).to_string();

        save_audio_segment_value(&store, seg).expect("save audio");
        let written = store
            .read_config_file("config/audio.json")
            .expect("read")
            .expect("written");
        let saved: AudioSegment = serde_json::from_slice(&written).expect("parse saved audio");
        assert_eq!(saved.wake_word.enter_threshold, 0.01);
        assert_eq!(saved.wake_word.leave_threshold, 0.005);
        assert_eq!(saved.wake_word.zcr_max, 0.65);
        assert_eq!(saved.wake_word.min_speech_band_ratio, 0.35);
        assert_eq!(saved.wake_word.min_active_ms, 120);
    }

    #[test]
    fn es7210_codec_wake_normalization_preserves_custom_acoustic_profile() {
        let store = MultiFileStore::with_file(
            "config/hardware.json",
            serde_json::to_vec(&HardwareSegment {
                hardware_devices: vec![],
                i2c_bus: Some(I2cBusConfig {
                    sda_pin: 8,
                    scl_pin: 18,
                    freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
                }),
                i2s_bus: Some(I2sBusConfig {
                    mclk_pin: 2,
                    ws_pin: 45,
                    bclk_pin: 17,
                    din_pin: 16,
                    dout_pin: 15,
                }),
                i2c_devices: vec![],
                i2c_sensors: vec![],
            })
            .expect("serialize hardware"),
        );
        let mut seg = default_disabled_audio_segment();
        seg.enabled = true;
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
        seg.speaker.sample_rate = AUDIO_REALTIME_PCM16_SAMPLE_RATE;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.pa_pin = Some(46);
        seg.codec.input_reference = true;
        seg.wake_word.enabled = true;
        seg.wake_word.zcr_min = 0.09;
        seg.realtime.provider = AUDIO_REALTIME_PROVIDER_QWEN.to_string();
        seg.realtime.ws_url =
            audio_realtime_default_ws_url(AUDIO_REALTIME_PROVIDER_QWEN).to_string();
        seg.realtime.api_key = "test-key".to_string();
        seg.realtime.model = audio_realtime_default_model(AUDIO_REALTIME_PROVIDER_QWEN).to_string();
        seg.realtime.voice = audio_realtime_default_voice(AUDIO_REALTIME_PROVIDER_QWEN).to_string();

        save_audio_segment_value(&store, seg).expect("save audio");
        let written = store
            .read_config_file("config/audio.json")
            .expect("read")
            .expect("written");
        let saved: AudioSegment = serde_json::from_slice(&written).expect("parse saved audio");
        assert_eq!(
            saved.wake_word.enter_threshold,
            default_wake_enter_threshold()
        );
        assert_eq!(saved.wake_word.zcr_min, 0.09);
    }

    #[test]
    fn merge_audio_from_json_defaults_missing_topology_to_discrete_i2s() {
        let mut config = AppConfig::load_from_env();
        let mut errors = Vec::new();
        config.merge_audio_from_json(
            r#"{
                "version": 1,
                "enabled": false,
                "service_provider": "baidu",
                "microphone": {
                  "enabled": false,
                  "device_type": "i2s_inmp441",
                  "pins": { "ws": 25, "sck": 26, "din": 27 },
                  "sample_rate": 16000
                },
                "speaker": {
                  "enabled": false,
                  "device_type": "i2s_max98357a",
                  "pins": { "ws": 32, "sck": 33, "dout": 22, "sd": null },
                  "sample_rate": 16000
                },
                "vad": { "threshold": 0.5, "silence_duration_ms": 1000 },
                "wake_word": { "enabled": false, "keyword": "hiesp", "wake_prompt": "你好，我在听，请说。" },
                "speech": { "api_url": "https://vop.baidu.com/server_api", "api_key": "", "api_secret": "", "model": "1537", "language": "zh" },
                "tts": { "voice": "0", "rate": "+0%", "pitch": "+0Hz" },
                "realtime": {
                  "provider": "openai_compatible",
                  "ws_url": "wss://api.openai.com/v1/realtime",
                  "api_key": "",
                  "model": "gpt-realtime",
                  "voice": "alloy",
                  "instructions": ""
                },
                "ambient_listening": {
                  "enabled": false,
                  "detect_emotions": true,
                  "sound_events": ["sigh"],
                  "cooldown_minutes": 10,
                  "check_interval_seconds": 300
                },
                "led_indicator": {
                  "enabled": false,
                  "pin": 2,
                  "states": { "listening": "breathing", "processing": "fast_blink", "speaking": "solid" }
                }
            }"#,
            &mut errors,
        );

        assert!(errors.is_empty());
        assert_eq!(
            config.audio.as_ref().expect("merged audio").topology,
            AUDIO_TOPOLOGY_DISCRETE_I2S
        );
    }

    #[test]
    fn save_audio_segment_rejects_i2s_codec_without_codec_fields() {
        let store = MultiFileStore::with_file(
            "config/hardware.json",
            serde_json::to_vec(&HardwareSegment {
                hardware_devices: vec![],
                i2c_bus: Some(I2cBusConfig {
                    sda_pin: 8,
                    scl_pin: 18,
                    freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
                }),
                i2s_bus: Some(I2sBusConfig {
                    mclk_pin: 2,
                    ws_pin: 45,
                    bclk_pin: 17,
                    din_pin: 16,
                    dout_pin: 15,
                }),
                i2c_devices: vec![],
                i2c_sensors: vec![],
            })
            .expect("serialize hardware"),
        );
        let mut seg = default_disabled_audio_segment();
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = 24_000;
        seg.speaker.sample_rate = 24_000;

        let error = save_audio_segment_value(&store, seg).expect_err("codec fields are required");
        assert!(error
            .to_string()
            .contains("audio.codec.input_codec is required"));
    }

    #[test]
    fn save_audio_segment_rejects_codec_addr_out_of_range() {
        let store = MultiFileStore::with_file(
            "config/hardware.json",
            serde_json::to_vec(&HardwareSegment {
                hardware_devices: vec![],
                i2c_bus: Some(I2cBusConfig {
                    sda_pin: 8,
                    scl_pin: 18,
                    freq_hz: crate::constants::I2C_DEFAULT_FREQ_HZ,
                }),
                i2s_bus: Some(I2sBusConfig {
                    mclk_pin: 2,
                    ws_pin: 45,
                    bclk_pin: 17,
                    din_pin: 16,
                    dout_pin: 15,
                }),
                i2c_devices: vec![],
                i2c_sensors: vec![],
            })
            .expect("serialize hardware"),
        );
        let mut seg = default_disabled_audio_segment();
        seg.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        seg.enabled = true;
        seg.microphone.enabled = true;
        seg.speaker.enabled = true;
        seg.microphone.sample_rate = 24_000;
        seg.speaker.sample_rate = 24_000;
        seg.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        seg.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        seg.codec.input_addr = Some(0x01);
        seg.codec.pa_pin = Some(46);

        let error = save_audio_segment_value(&store, seg).expect_err("codec addr range");
        assert!(error.to_string().contains("audio.codec.input_addr"));
    }

    #[test]
    fn save_hardware_segment_rejects_i2s_codec_audio_without_required_buses() {
        let mut audio = default_disabled_audio_segment();
        audio.topology = AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        audio.enabled = true;
        audio.microphone.enabled = true;
        audio.speaker.enabled = true;
        audio.microphone.sample_rate = 24_000;
        audio.speaker.sample_rate = 24_000;
        audio.codec.input_codec = Some(AUDIO_CODEC_INPUT_ES7210.to_string());
        audio.codec.output_codec = Some(AUDIO_CODEC_OUTPUT_ES8311.to_string());
        audio.codec.pa_pin = Some(46);
        let store = MultiFileStore::with_file(
            "config/audio.json",
            serde_json::to_vec(&audio).expect("serialize audio"),
        );
        let hardware = HardwareSegment {
            hardware_devices: vec![],
            i2c_bus: None,
            i2s_bus: None,
            i2c_devices: vec![],
            i2c_sensors: vec![],
        };

        let error = save_hardware_segment_value(&store, &hardware)
            .expect_err("codec topology requires buses");
        assert!(error
            .to_string()
            .contains("hardware.i2c_bus is required when audio.topology == i2s_codec"));
    }

    #[test]
    fn merge_audio_from_json_rejects_pair_invalid_loaded_hardware() {
        let mut config = AppConfig::load_from_env();
        let mut errors = Vec::new();
        config.merge_audio_from_json(
            r#"{
                "version": 1,
                "enabled": true,
                "service_provider": "baidu",
                "topology": "i2s_codec",
                "microphone": {
                  "enabled": true,
                  "device_type": "i2s_inmp441",
                  "pins": { "ws": 25, "sck": 26, "din": 27 },
                  "sample_rate": 24000
                },
                "speaker": {
                  "enabled": true,
                  "device_type": "i2s_max98357a",
                  "pins": { "ws": 32, "sck": 33, "dout": 22, "sd": null },
                  "sample_rate": 24000
                },
                "codec": {
                  "input_codec": "es7210",
                  "output_codec": "es8311",
                  "input_addr": null,
                  "output_addr": null,
                  "pa_pin": 46,
                  "input_reference": true
                },
                "vad": { "threshold": 0.5, "silence_duration_ms": 1000 },
                "wake_word": { "enabled": false, "keyword": "hiesp", "wake_prompt": "你好，我在听，请说。" },
                "speech": { "api_url": "https://vop.baidu.com/server_api", "api_key": "", "api_secret": "", "model": "1537", "language": "zh" },
                "tts": { "voice": "0", "rate": "+0%", "pitch": "+0Hz" },
                "realtime": {
                  "provider": "openai_compatible",
                  "ws_url": "wss://api.openai.com/v1/realtime",
                  "api_key": "",
                  "model": "gpt-realtime",
                  "voice": "alloy",
                  "instructions": ""
                },
                "ambient_listening": {
                  "enabled": false,
                  "detect_emotions": true,
                  "sound_events": ["sigh"],
                  "cooldown_minutes": 10,
                  "check_interval_seconds": 300
                },
                "led_indicator": {
                  "enabled": false,
                  "pin": 2,
                  "states": { "listening": "breathing", "processing": "fast_blink", "speaking": "solid" }
                }
            }"#,
            &mut errors,
        );

        assert_eq!(errors, vec!["audio_validation_failed"]);
        assert!(config.audio.is_none());
    }

    #[test]
    fn save_audio_segment_normalizes_empty_usb_device_ref_to_none() {
        struct MemoryFileStore(std::sync::Mutex<Option<Vec<u8>>>);

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
            }

            fn write_config_file(&self, _rel_path: &str, data: &[u8]) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(data.to_vec());
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(())
            }
        }

        let store = MemoryFileStore(std::sync::Mutex::new(None));
        let body = r#"{
            "version": 1,
            "enabled": true,
            "service_provider": "baidu",
            "microphone": {
              "enabled": false,
              "device_type": "i2s_inmp441",
              "pins": { "ws": 25, "sck": 26, "din": 27 },
              "sample_rate": 16000
            },
            "speaker": {
              "enabled": true,
              "device_type": "usb",
              "device_ref": "   ",
              "sample_rate": 16000
            },
            "vad": { "threshold": 0.5, "silence_duration_ms": 1000 },
            "wake_word": { "enabled": false, "keyword": "hiesp", "wake_prompt": "你好，我在听，请说。" },
            "speech": { "api_url": "https://vop.baidu.com/server_api", "api_key": "", "api_secret": "", "model": "1537", "language": "zh" },
            "tts": { "voice": "0", "rate": "+0%", "pitch": "+0Hz" },
            "realtime": {
              "provider": "openai_compatible",
              "ws_url": "wss://api.openai.com/v1/realtime",
              "api_key": "",
              "model": "gpt-realtime",
              "voice": "alloy",
              "instructions": ""
            },
            "ambient_listening": {
              "enabled": false,
              "detect_emotions": true,
              "sound_events": ["sigh"],
              "cooldown_minutes": 10,
              "check_interval_seconds": 300
            },
            "led_indicator": {
              "enabled": false,
              "pin": 2,
              "states": { "listening": "breathing", "processing": "fast_blink", "speaking": "solid" }
            }
        }"#;

        save_audio_segment(&store, body).expect("save audio");
        let written = store
            .read_config_file("config/audio.json")
            .expect("read")
            .expect("written");
        let saved: AudioSegment = serde_json::from_slice(&written).expect("parse saved audio");
        assert_eq!(saved.speaker.device_ref, None);
    }

    #[test]
    fn save_tg_group_activation_to_channels_updates_only_channels_file() {
        struct MemoryFileStore(std::sync::Mutex<Option<Vec<u8>>>);

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
            }

            fn write_config_file(&self, _rel_path: &str, data: &[u8]) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(data.to_vec());
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(())
            }
        }

        let previous =
            br#"{"enabled_channel":"telegram","tg_group_activation":"mention","tg_token":"token"}"#
                .to_vec();
        let store = MemoryFileStore(std::sync::Mutex::new(Some(previous.clone())));

        save_tg_group_activation_to_channels(&store, "always").expect("save activation");

        let written = store
            .read_config_file("config/channels.json")
            .expect("read saved file")
            .expect("saved file should exist");
        let saved: ChannelsSegment = serde_json::from_slice(&written).expect("parse saved file");
        assert_eq!(saved.enabled_channel, "telegram");
        assert_eq!(saved.tg_token, "token");
        assert_eq!(saved.tg_group_activation, "always");
    }

    #[test]
    fn save_tg_group_activation_to_channels_recovers_empty_channels_file() {
        struct MemoryFileStore(std::sync::Mutex<Option<Vec<u8>>>);

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
            }

            fn write_config_file(&self, _rel_path: &str, data: &[u8]) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(data.to_vec());
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(())
            }
        }

        let store = MemoryFileStore(std::sync::Mutex::new(Some(Vec::new())));

        save_tg_group_activation_to_channels(&store, "always").expect("save activation");

        let written = store
            .read_config_file("config/channels.json")
            .expect("read saved file")
            .expect("saved file should exist");
        let saved: ChannelsSegment = serde_json::from_slice(&written).expect("parse saved file");
        assert_eq!(saved.tg_group_activation, "always");
    }

    #[test]
    fn save_system_segment_to_nvs_rejects_invalid_locale() {
        #[derive(Default)]
        struct MemoryConfigStore(std::sync::Mutex<Vec<(String, String)>>);

        impl crate::platform::ConfigStore for MemoryConfigStore {
            fn read_string(&self, _key: &str) -> Result<Option<String>> {
                Ok(None)
            }

            fn write_string(&self, key: &str, value: &str) -> Result<()> {
                self.0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((key.to_string(), value.to_string()));
                Ok(())
            }

            fn erase_keys(&self, _keys: &[&str]) -> Result<()> {
                Ok(())
            }
        }

        let store = MemoryConfigStore::default();
        let error = save_system_segment_to_nvs(
            &store,
            r#"{
                "wifi_ssid":"BeetleNet",
                "wifi_pass":"secret-pass",
                "proxy_url":"",
                "locale":"ja"
            }"#,
        )
        .expect_err("invalid locale should be rejected");

        assert_eq!(error.stage(), "locale");
        assert!(
            store.0.lock().unwrap_or_else(|e| e.into_inner()).is_empty(),
            "invalid locale must fail before any NVS write"
        );
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn save_office_accounts_segment_roundtrips_multi_account_registry() {
        struct MemoryFileStore(std::sync::Mutex<Option<Vec<u8>>>);

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
            }

            fn write_config_file(&self, _rel_path: &str, data: &[u8]) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(data.to_vec());
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(())
            }
        }

        let store = MemoryFileStore(std::sync::Mutex::new(None));
        let body = r#"{
            "registry": {
                "accounts": {
                    "mail-work": {
                        "account_key": "mail-work",
                        "provider_kind": "imap_smtp",
                        "external_account_id": "work@example.com",
                        "account_label": "工作邮箱",
                        "identity_class": "work",
                        "enabled_capabilities": ["mail"]
                    },
                    "calendar-personal": {
                        "account_key": "calendar-personal",
                        "provider_kind": "caldav",
                        "external_account_id": "personal@example.com",
                        "account_label": "私人日历",
                        "identity_class": "personal",
                        "enabled_capabilities": ["calendar"]
                    }
                }
            },
            "policy": {
                "ask_when_ambiguous": true,
                "preferred_identity_class": "work"
            }
        }"#;

        save_office_accounts_segment(&store, body).expect("save office accounts");
        let saved = get_office_accounts_segment(&store).expect("get office accounts");
        let parsed: OfficeAccountsSegment =
            serde_json::from_str(&saved).expect("parse saved office accounts");
        assert!(parsed.registry.get("mail-work").is_some());
        assert!(parsed.registry.get("calendar-personal").is_some());
        assert!(parsed.policy.ask_when_ambiguous);
        assert_eq!(
            parsed.policy.preferred_identity_class,
            Some(crate::office::OfficeAccountIdentityClass::Work)
        );
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn save_office_accounts_segment_rejects_legacy_binding_key() {
        struct MemoryFileStore;

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }

            fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                Ok(())
            }
        }

        let error = save_office_accounts_segment(
            &MemoryFileStore,
            r#"{
                "registry": { "accounts": {} },
                "binding": { "capability_defaults": { "mail": "ghost" } },
                "policy": {}
            }"#,
        )
        .expect_err("legacy binding should be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn save_office_accounts_segment_rejects_legacy_global_default_policy_key() {
        struct MemoryFileStore;

        impl ConfigFileStore for MemoryFileStore {
            fn read_config_file(&self, _rel_path: &str) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }

            fn write_config_file(&self, _rel_path: &str, _data: &[u8]) -> Result<()> {
                Ok(())
            }

            fn remove_config_file(&self, _rel_path: &str) -> Result<()> {
                Ok(())
            }
        }

        let error = save_office_accounts_segment(
            &MemoryFileStore,
            r#"{
                "registry": {
                    "accounts": {
                        "mail-work": {
                            "account_key": "mail-work",
                            "provider_kind": "imap_smtp",
                            "external_account_id": "work@example.com",
                            "account_label": "Work",
                            "identity_class": "work",
                            "enabled_capabilities": ["mail"]
                        }
                    }
                },
                "policy": { "global_default_account_key": "mail-work" }
            }"#,
        )
        .expect_err("legacy global default should be rejected");
        assert!(error.to_string().contains("unknown field"));
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn merge_office_accounts_from_json_rejects_trailing_garbage() {
        let mut config = AppConfig::load_from_env();
        let mut errors = Vec::new();
        config.merge_office_accounts_from_json(
            r#"{
                "registry": {
                    "accounts": {
                        "calendar-work": {
                            "account_key": "calendar-work",
                            "provider_kind": "caldav",
                            "external_account_id": "work@example.com",
                            "account_label": "Work",
                            "identity_class": "work",
                            "enabled_capabilities": ["calendar"]
                        }
                    }
                },
                "policy": {}
            } trailing"#,
            &mut errors,
        );
        assert_eq!(errors, vec!["accounts_json_invalid".to_string()]);
        assert!(config
            .office_accounts
            .registry
            .get("calendar-work")
            .is_none());
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn save_office_credentials_segment_roundtrips() {
        struct MemoryOfficeCredentialStore(std::sync::Mutex<Vec<crate::office::OfficeCredential>>);

        impl crate::office::OfficeCredentialStore for MemoryOfficeCredentialStore {
            fn get(&self, account_key: &str) -> Result<Option<crate::office::OfficeCredential>> {
                Ok(self
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .find(|item| item.account_key == account_key)
                    .cloned())
            }

            fn list(&self) -> Result<Vec<crate::office::OfficeCredential>> {
                Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
            }

            fn set(&self, credential: &crate::office::OfficeCredential) -> Result<()> {
                let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
                guard.retain(|item| item.account_key != credential.account_key);
                guard.push(credential.clone());
                Ok(())
            }

            fn clear(&self, account_key: &str) -> Result<()> {
                self.0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .retain(|item| item.account_key != account_key);
                Ok(())
            }
        }

        let store = MemoryOfficeCredentialStore(std::sync::Mutex::new(vec![]));
        save_office_credentials_segment(
            &store,
            r#"{
                "items": [
                    {
                        "account_key": "calendar-work",
                        "access_token": "token",
                        "refresh_token": "refresh",
                        "token_endpoint": "https://example.com/token",
                        "expires_at_unix_secs": 12,
                        "updated_at": 34,
                        "metadata": { "calendar_id": "primary" }
                    }
                ]
            }"#,
        )
        .expect("save office credentials");

        let saved = get_office_credentials_segment(&store).expect("get office credentials");
        let parsed: OfficeCredentialsSegment =
            serde_json::from_str(&saved).expect("parse office credentials");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].account_key, "calendar-work");
        assert_eq!(
            parsed.items[0]
                .metadata
                .get("calendar_id")
                .map(String::as_str),
            Some("primary")
        );
    }

    #[cfg(feature = "capability_office")]
    #[test]
    fn save_office_credentials_segment_rejects_trailing_garbage() {
        struct MemoryOfficeCredentialStore;

        impl crate::office::OfficeCredentialStore for MemoryOfficeCredentialStore {
            fn get(&self, _account_key: &str) -> Result<Option<crate::office::OfficeCredential>> {
                Ok(None)
            }

            fn list(&self) -> Result<Vec<crate::office::OfficeCredential>> {
                Ok(Vec::new())
            }

            fn set(&self, _credential: &crate::office::OfficeCredential) -> Result<()> {
                Ok(())
            }

            fn clear(&self, _account_key: &str) -> Result<()> {
                Ok(())
            }
        }

        let error = save_office_credentials_segment(
            &MemoryOfficeCredentialStore,
            r#"{"items":[{"account_key":"calendar-work"}]} trailing"#,
        )
        .expect_err("office credentials should reject trailing garbage");
        assert!(error.to_string().contains("trailing"));
    }
}
