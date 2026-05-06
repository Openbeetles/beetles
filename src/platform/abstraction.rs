//! 平台抽象 trait：ConfigStore、SkillStorage、PlatformHttpClient、Platform。
//! 核心域与 main 仅依赖这些 trait，便于后续支持多种硬件。

use crate::calendar::CalendarStore;
use crate::config::{AppConfig, AudioSegment, PinConfig};
use crate::display::{DisplayCommand, DisplayConfig};
use crate::error::{Error, Result};
use crate::memory::{
    AutonomyStrategyStore, CoreRevisionLedgerStore, ExecutionStateStore, FeltSignificanceStore,
    ImportantMessageStore, InnerConflictStore, InnerLifeStore, LongTermMemoryStore, MemoryStore,
    MemorySystemKind, MentalPrivacyStore, OuterVoiceStore, PendingRetryStore, PrivateDocStore,
    PrivateGardenStore, RelationshipConstitutionStore, RelationshipPortfolioStore,
    RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SelfModelStore, SessionStore, SessionSummaryStore, TemperamentContinuityStore, TurnLedgerStore,
    WorldSenseStore,
};
#[cfg(feature = "capability_office")]
use crate::office::{OfficeCredentialStore, OfficeRuntimeStatusStore};
use crate::platform::camera::PlatformCamera;
use crate::platform::ResponseBody;
use crate::task::TaskStore;
use crate::task_execution::{
    TaskArtifactStore, TaskExecutionLedgerStore, TaskLearningStore, TaskRunStore,
};
use serde_json::Value;
use std::sync::Arc;

/// Owned bytes read from platform state storage.
/// ESP implementations may keep the raw bytes in PSRAM for large files.
pub type StateBytes = crate::platform::byte_buffer::ByteBuffer;

/// 状态根目录下的受控文件访问（相对路径）。ESP 委托 storage + 互斥；Linux 由 `LinuxPlatform` 实现。
/// Controlled file access under the platform state root (relative paths).
pub trait StateFs: Send + Sync {
    /// 读取文件，不存在返回 `Ok(None)`。
    fn read(&self, rel_path: &str) -> crate::error::Result<Option<Vec<u8>>>;
    /// 读取文件为 owned bytes；大对象调用链应优先用本接口避免强制落入 heap `Vec`。
    fn read_bytes(&self, rel_path: &str) -> crate::error::Result<Option<StateBytes>> {
        Ok(self.read(rel_path)?.map(StateBytes::from_vec))
    }
    /// 写入文件；实现须先创建父目录再写入。单文件大小上界由存储实现保证。
    fn write(&self, rel_path: &str, data: &[u8]) -> crate::error::Result<()>;
    /// 删除文件，不存在时 `Ok(())`。
    fn remove(&self, rel_path: &str) -> crate::error::Result<()>;
    /// 列出一层目录下的文件名（不递归子目录）。
    fn list_dir(&self, rel_path: &str) -> crate::error::Result<Vec<String>>;
    /// 文件是否存在。
    fn exists(&self, rel_path: &str) -> crate::error::Result<bool> {
        Ok(self.read_bytes(rel_path)?.is_some())
    }
}

/// 平台内存快照，语义与 orchestrator 堆原子字段对齐（跨平台可比）。
/// Platform memory snapshot aligned with orchestrator heap atomics (cross-platform comparable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySnapshot {
    /// 内部堆空闲字节（ESP: internal heap；Linux: 主内存可用量，通常为 `MemAvailable`）。
    pub heap_free_internal: u32,
    /// 内部堆历史最小空闲字节（ESP low-water mark；Linux/host 固定 0 表示 N/A）。
    pub heap_min_free_internal: u32,
    /// 外部堆空闲字节（ESP: SPIRAM；Linux: 0 或 swap 等扩展字段保留为 0）。
    pub heap_free_spiram: u32,
    /// 外部堆总字节数（ESP: SPIRAM total；Linux/host 固定 0）。
    pub heap_total_spiram: u32,
    /// 外部堆历史最小空闲字节（ESP: SPIRAM low-water mark；Linux/host 固定 0）。
    pub heap_min_free_spiram: u32,
    /// 外部堆当前最大连续空闲块（ESP: SPIRAM largest free block；Linux/host 固定 0）。
    pub heap_largest_block_spiram: u32,
    /// 最大连续可分配块字节（ESP: largest free block for TLS fragmentation checks）。
    /// Linux / host: **always 0** — means *not applicable*, not “zero-byte largest block”.
    /// 最大连续空闲块（字节）。ESP 为 TLS 碎片门禁所用；Linux 固定 **0** 表示本维度不可用，勿当作真实块大小。
    pub heap_largest_block: u32,
}

/// 语音回放参考能力。当前只暴露真实已实现的能力，避免伪 AEC 口径。
/// Playback reference capability. Exposes only implemented capabilities to avoid fake AEC claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReferenceCapability {
    None,
    PlaybackMonitor,
    InputReference,
}

/// 回声消除能力。`Platform` 表示平台已提供真实 AEC 路径，非“未来可做”占位。
/// Echo-cancellation capability. `Platform` means a real implemented AEC path exists today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioEchoCancellationCapability {
    None,
    Platform,
}

/// 双工能力档位。用于把音频平台能力压缩成闭集，不允许业务层自己猜。
/// Closed-set duplex profile so upper layers consume one stable platform contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDuplexProfile {
    Unavailable,
    SpeakerOnly,
    MicrophoneOnly,
    DuplexNoReference,
    DuplexPlaybackReference,
    DuplexInputReference,
    DuplexPlatformAec,
}

impl AudioDuplexProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::SpeakerOnly => "speaker_only",
            Self::MicrophoneOnly => "microphone_only",
            Self::DuplexNoReference => "duplex_no_reference",
            Self::DuplexPlaybackReference => "duplex_playback_reference",
            Self::DuplexInputReference => "duplex_input_reference",
            Self::DuplexPlatformAec => "duplex_platform_aec",
        }
    }
}

/// 存储介质类型。用于统一表达 ESP flash、Linux mmc/nvme/usb 等底层介质。
/// Storage media kind across targets (flash, SD/eMMC/NVMe/USB, virtual mounts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMediaKind {
    Flash,
    SdCard,
    Emmc,
    UsbMassStorage,
    Nvme,
    Virtual,
    Unknown,
}

/// 平台探测到的一条存储介质记录。
/// One discovered storage medium record exposed through the platform contract.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StorageMediaInfo {
    pub id: String,
    pub kind: StorageMediaKind,
    pub label: String,
    pub present: bool,
    pub mounted: bool,
    pub mount_path: Option<String>,
    pub filesystem: Option<String>,
    pub source: Option<String>,
    pub removable: bool,
    pub is_system_root: bool,
    pub is_state_root: bool,
    pub capacity_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
}

/// 当前平台在已初始化音频拓扑下的双工/打断能力快照。
/// Snapshot of duplex / barge-in capability for the initialized audio topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct AudioDuplexCapabilities {
    pub microphone_input: bool,
    pub speaker_output: bool,
    pub concurrent_capture_playback: bool,
    pub barge_in: bool,
    pub reference_capture: AudioReferenceCapability,
    pub echo_cancellation: AudioEchoCancellationCapability,
}

impl AudioDuplexCapabilities {
    pub const fn unavailable() -> Self {
        Self {
            microphone_input: false,
            speaker_output: false,
            concurrent_capture_playback: false,
            barge_in: false,
            reference_capture: AudioReferenceCapability::None,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn speaker_only() -> Self {
        Self {
            microphone_input: false,
            speaker_output: true,
            concurrent_capture_playback: false,
            barge_in: false,
            reference_capture: AudioReferenceCapability::None,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn microphone_only() -> Self {
        Self {
            microphone_input: true,
            speaker_output: false,
            concurrent_capture_playback: false,
            barge_in: false,
            reference_capture: AudioReferenceCapability::None,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn duplex_without_aec() -> Self {
        Self {
            microphone_input: true,
            speaker_output: true,
            concurrent_capture_playback: true,
            barge_in: true,
            reference_capture: AudioReferenceCapability::None,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn duplex_with_playback_reference() -> Self {
        Self {
            microphone_input: true,
            speaker_output: true,
            concurrent_capture_playback: true,
            barge_in: true,
            reference_capture: AudioReferenceCapability::PlaybackMonitor,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn duplex_with_input_reference() -> Self {
        Self {
            microphone_input: true,
            speaker_output: true,
            concurrent_capture_playback: true,
            barge_in: true,
            reference_capture: AudioReferenceCapability::InputReference,
            echo_cancellation: AudioEchoCancellationCapability::None,
        }
    }

    pub const fn duplex_with_platform_aec() -> Self {
        Self {
            microphone_input: true,
            speaker_output: true,
            concurrent_capture_playback: true,
            barge_in: true,
            reference_capture: AudioReferenceCapability::InputReference,
            echo_cancellation: AudioEchoCancellationCapability::Platform,
        }
    }

    /// 规范化能力快照，确保 profile 与字段组合不会出现自相矛盾的口径。
    pub fn normalized(self) -> Self {
        let mut caps = self;
        if !caps.microphone_input {
            caps.concurrent_capture_playback = false;
            caps.barge_in = false;
            caps.echo_cancellation = AudioEchoCancellationCapability::None;
        }
        if !caps.speaker_output {
            caps.concurrent_capture_playback = false;
            caps.barge_in = false;
            caps.reference_capture = AudioReferenceCapability::None;
            caps.echo_cancellation = AudioEchoCancellationCapability::None;
        }
        if !caps.concurrent_capture_playback {
            caps.barge_in = false;
        }
        if caps.echo_cancellation == AudioEchoCancellationCapability::Platform {
            caps.microphone_input = true;
            caps.speaker_output = true;
            caps.concurrent_capture_playback = true;
            caps.barge_in = true;
            if caps.reference_capture == AudioReferenceCapability::None {
                caps.reference_capture = AudioReferenceCapability::InputReference;
            }
        }
        caps
    }

    pub const fn has_microphone_input(self) -> bool {
        self.microphone_input
    }

    pub const fn has_speaker_output(self) -> bool {
        self.speaker_output
    }

    pub const fn has_reference_capture(self) -> bool {
        !matches!(self.reference_capture, AudioReferenceCapability::None)
    }

    pub const fn supports_concurrent_capture_playback(self) -> bool {
        self.concurrent_capture_playback
    }

    pub const fn supports_barge_in(self) -> bool {
        self.barge_in && self.concurrent_capture_playback
    }

    pub const fn has_platform_aec(self) -> bool {
        matches!(
            self.echo_cancellation,
            AudioEchoCancellationCapability::Platform
        )
    }

    pub const fn requires_capture_upload_suspend_during_playback(self) -> bool {
        self.concurrent_capture_playback && self.has_reference_capture() && !self.has_platform_aec()
    }

    pub const fn can_run_realtime_session(self) -> bool {
        self.microphone_input && self.speaker_output
    }

    pub const fn profile(self) -> AudioDuplexProfile {
        if matches!(
            self.echo_cancellation,
            AudioEchoCancellationCapability::Platform
        ) {
            return AudioDuplexProfile::DuplexPlatformAec;
        }
        match (
            self.microphone_input,
            self.speaker_output,
            self.reference_capture,
        ) {
            (false, false, _) => AudioDuplexProfile::Unavailable,
            (false, true, _) => AudioDuplexProfile::SpeakerOnly,
            (true, false, _) => AudioDuplexProfile::MicrophoneOnly,
            (true, true, AudioReferenceCapability::None) => AudioDuplexProfile::DuplexNoReference,
            (true, true, AudioReferenceCapability::PlaybackMonitor) => {
                AudioDuplexProfile::DuplexPlaybackReference
            }
            (true, true, AudioReferenceCapability::InputReference) => {
                AudioDuplexProfile::DuplexInputReference
            }
        }
    }
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

/// 技能元数据（顺序、禁用列表）存储抽象。用于 storage config/skills_meta.json，避免 NVS 高频单键写。
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

/// Hardware discovery bus filter exposed through the config HTTP API.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareDiscoveryBus {
    Usb,
}

impl HardwareDiscoveryBus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "usb" => Some(Self::Usb),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Usb => "usb",
        }
    }
}

/// Capability filter for platform hardware discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardwareCapability {
    AudioOutput,
    AudioInput,
    Camera,
    Serial,
    Hid,
}

impl HardwareCapability {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "audio_output" => Some(Self::AudioOutput),
            "audio_input" => Some(Self::AudioInput),
            "camera" => Some(Self::Camera),
            "serial" => Some(Self::Serial),
            "hid" => Some(Self::Hid),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AudioOutput => "audio_output",
            Self::AudioInput => "audio_input",
            Self::Camera => "camera",
            Self::Serial => "serial",
            Self::Hid => "hid",
        }
    }
}

/// Query for platform-scoped hardware discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwareDiscoveryQuery {
    pub bus: HardwareDiscoveryBus,
    pub capability: HardwareCapability,
}

/// One discovered hardware item returned to the config UI.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HardwareDiscoveryItem {
    pub device_ref: String,
    pub label: String,
    pub kind: String,
    pub capabilities: Vec<HardwareCapability>,
    pub is_default: bool,
    pub metadata: Value,
}

/// Discovery response envelope, intentionally not a bare array.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HardwareDiscoveryResponse {
    pub bus: HardwareDiscoveryBus,
    pub capability: HardwareCapability,
    pub items: Vec<HardwareDiscoveryItem>,
}

/// Platform-specific hardware discovery provider (USB audio devices, cameras, etc.).
pub trait HardwareDiscovery: Send + Sync {
    fn discover(&self, query: &HardwareDiscoveryQuery) -> Result<HardwareDiscoveryResponse>;
}

/// 统一 HTTP 客户端：仅 get/post/post_streaming/reset 方法，LlmHttpClient、ToolContext、ChannelHttpClient 由 lib 层 blanket 转发。
pub trait PlatformHttpClient {
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        match method {
            "GET" => self.get(url, headers),
            "POST" => self.post(url, headers, body.unwrap_or_default()),
            "PATCH" => self.patch(url, headers, body.unwrap_or_default()),
            "PUT" => self.put(url, headers, body.unwrap_or_default()),
            "DELETE" => self.delete(url, headers),
            other => Err(Error::config(
                "http_request_method",
                format!("unsupported http method: {other}"),
            )),
        }
    }
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
    /// 流式 GET：逐块回调响应体，不将完整响应体读入内存。
    /// `max_response_bytes`: None = 无限制；Some(n) = 限制总字节数。
    /// 默认实现回退到 get()，将完整响应体一次性传给 on_chunk。
    fn get_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        _max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        let (status, resp_body) = self.get(url, headers)?;
        on_chunk(resp_body.as_ref())?;
        Ok(status)
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
    fn request(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Result<(u16, ResponseBody)> {
        (**self).request(method, url, headers, body)
    }
    fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, ResponseBody)> {
        (**self).get(url, headers)
    }
    fn get_streaming(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        max_response_bytes: Option<usize>,
        on_chunk: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<u16> {
        (**self).get_streaming(url, headers, max_response_bytes, on_chunk)
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
pub trait Platform: Send + Sync + PlatformCamera {
    /// 状态文件系统抽象（ESP 平台存储根或 Linux 状态目录）。业务域经此访问，禁止直引平台存储后端。
    fn state_fs(&self) -> Arc<dyn StateFs + Send + Sync>;

    /// 当前内存快照；须来自真实数据源（ESP: `heap`；Linux: `/proc/meminfo`），禁止占位常量。
    fn memory_snapshot(&self) -> MemorySnapshot;
    /// 记忆承载制度；用于区分 Linux 厚治理体系与 ESP 紧凑体系。
    fn memory_system_kind(&self) -> MemorySystemKind;

    /// 平台初始化（link_patches、日志、NVS、storage 等）。main 在构造后首先调用。
    fn init(&self) -> Result<()> {
        Ok(())
    }
    fn init_nvs(&self) -> Result<()>;
    fn init_storage(&self) -> Result<()>;
    fn config_store(&self) -> Arc<dyn ConfigStore + Send + Sync>;
    fn connect_wifi(&self, config: &AppConfig) -> Result<()>;
    /// WiFi 扫描句柄（SoftAP 就绪且底层已注册扫描时为 Some；STA 失败时仍应保留以便配网）；用于 GET /api/wifi/scan。
    fn wifi_scan(&self) -> Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> {
        None
    }
    /// Hardware discovery handle (USB devices, cameras, serial peripherals, etc.).
    fn hardware_discovery(
        &self,
    ) -> Option<Arc<dyn crate::platform::HardwareDiscovery + Send + Sync>> {
        None
    }
    /// 当前对外可达的局域网 IPv4；Linux 应返回当前活跃上行接口地址，ESP 默认复用 STA IPv4。
    fn lan_ipv4(&self) -> Option<String> {
        self.wifi_sta_ip()
    }
    /// 当前 STA IPv4 地址（例如 192.168.1.42）；不可用时返回 None。
    fn wifi_sta_ip(&self) -> Option<String> {
        None
    }
    /// 返回平台可见的存储介质列表。Linux 可返回 mmc/nvme/usb/virtual mount；
    /// ESP 后续可返回 storage/SD/FATFS。默认空列表，表示当前平台未实现探测。
    fn storage_media(&self) -> Result<Vec<StorageMediaInfo>> {
        Ok(Vec::new())
    }
    fn memory_store(&self) -> Arc<dyn MemoryStore + Send + Sync>;
    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore + Send + Sync>;
    fn continuity_capsule_store(
        &self,
    ) -> Arc<dyn crate::memory::ContinuityCapsuleStore + Send + Sync>;
    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn crate::memory::LongTermMemoryExtractionStateStore + Send + Sync>;
    fn session_store(&self) -> Arc<dyn SessionStore + Send + Sync>;
    fn pending_retry_store(&self) -> Arc<dyn PendingRetryStore + Send + Sync>;
    fn calendar_store(&self) -> Arc<dyn CalendarStore + Send + Sync>;
    #[cfg(feature = "capability_office")]
    fn office_credential_store(&self) -> Arc<dyn OfficeCredentialStore + Send + Sync>;
    #[cfg(feature = "capability_office")]
    fn office_runtime_status_store(&self) -> Arc<dyn OfficeRuntimeStatusStore + Send + Sync>;
    fn task_store(&self) -> Arc<dyn TaskStore + Send + Sync>;
    fn task_run_store(&self) -> Arc<dyn TaskRunStore + Send + Sync>;
    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore + Send + Sync>;
    fn task_execution_ledger_store(&self) -> Arc<dyn TaskExecutionLedgerStore + Send + Sync>;
    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore + Send + Sync>;
    fn active_work_store(&self) -> Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>;
    fn detached_work_store(&self) -> Arc<dyn crate::agent::DetachedWorkStore + Send + Sync>;
    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore + Send + Sync>;
    fn self_model_store(&self) -> Arc<dyn SelfModelStore + Send + Sync>;
    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore + Send + Sync>;
    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore + Send + Sync>;
    fn relationship_constitution_store(
        &self,
    ) -> Arc<dyn RelationshipConstitutionStore + Send + Sync>;
    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore + Send + Sync>;
    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore + Send + Sync>;
    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore + Send + Sync>;
    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore + Send + Sync>;
    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore + Send + Sync>;
    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore + Send + Sync>;
    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore + Send + Sync>;
    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore + Send + Sync>;
    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore + Send + Sync>;
    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore + Send + Sync>;
    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore + Send + Sync>;
    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore + Send + Sync>;
    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore + Send + Sync>;
    fn important_message_store(&self) -> Arc<dyn ImportantMessageStore + Send + Sync>;
    fn remind_at_store(&self) -> Arc<dyn RemindAtStore + Send + Sync>;
    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore + Send + Sync>;
    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore + Send + Sync>;
    fn skill_storage(&self) -> Arc<dyn SkillStorage + Send + Sync>;
    fn skill_meta_store(&self) -> Arc<dyn SkillMetaStore + Send + Sync>;
    /// 原始 transport primitive：仅 `crate::network` / platform 实现层可直接调用。
    /// 业务域必须走统一治理面，门禁由 `scripts/check_network_governance.sh` 强制执行。
    fn create_http_client(&self, config: &AppConfig) -> Result<Box<dyn PlatformHttpClient>>;
    /// 创建面向用户可见交付面的 HTTP client（如流式编辑/状态更新）。
    /// 原始 transport primitive；业务域同样禁止直接调用，必须通过 `crate::network`。
    /// 默认沿用普通 client；资源更紧的平台可覆写为更高优先级。
    fn create_interactive_http_client(
        &self,
        config: &AppConfig,
    ) -> Result<Box<dyn PlatformHttpClient>> {
        self.create_http_client(config)
    }
    fn storage_usage(&self) -> Option<(u64, u64)>;
    fn read_heartbeat_file(&self) -> Result<String>;

    /// 板级状态 JSON（芯片、堆、运行时间、压力、WiFi、storage）。默认实现委托 `platform/board_info`；新平台可覆写。
    fn board_info_json(&self) -> Result<String> {
        let mut payload: serde_json::Value =
            serde_json::from_str(&crate::platform::board_info::board_info_json_string())
                .map_err(|e| Error::config("board_info_json", e.to_string()))?;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert(
                "firmware_version".to_string(),
                serde_json::json!(env!("CARGO_PKG_VERSION")),
            );
            obj.insert(
                "display".to_string(),
                serde_json::json!({
                    "available": self.display_available(),
                }),
            );
            let audio_caps = self.audio_duplex_capabilities();
            obj.insert(
                "audio".to_string(),
                serde_json::json!({
                    "duplex_profile": audio_caps.profile(),
                    "duplex_capabilities": audio_caps,
                }),
            );
            match self.storage_media() {
                Ok(media) => {
                    obj.insert("storage_media".to_string(), serde_json::json!(media));
                }
                Err(e) => {
                    log::warn!("[platform] storage media probe failed: {}", e);
                    obj.insert("storage_media".to_string(), serde_json::json!([]));
                    obj.insert(
                        "storage_media_error".to_string(),
                        serde_json::json!(e.to_string()),
                    );
                }
            }
        }
        serde_json::to_string(&payload).map_err(|e| Error::config("board_info_json", e.to_string()))
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

    /// 启动 SNTP 时间同步。调用时机由平台决定：
    /// ESP 在 WiFi 栈 ready 后启动；Linux 可在 WiFi/bootstrap 失败后继续尝试，
    /// 以便通过其它可用上行修复墙钟。
    fn init_sntp(&self) {
        log::info!("init_sntp: no-op on this platform");
    }

    /// 初始化显示器硬件。默认 no-op（非显示平台）。
    fn init_display(&self, _config: &DisplayConfig) -> Result<()> {
        Ok(())
    }

    /// 初始化音频硬件（麦克风/喇叭）。默认 no-op（非音频平台）。
    fn init_audio(&self, _config: &AudioSegment) -> Result<()> {
        Ok(())
    }

    /// 麦克风链路是否可用。默认从 `audio_duplex_capabilities()` 派生。
    fn audio_mic_ready(&self) -> bool {
        self.audio_duplex_capabilities().has_microphone_input()
    }

    /// 喇叭链路是否可用。默认从 `audio_duplex_capabilities()` 派生。
    fn audio_speaker_ready(&self) -> bool {
        self.audio_duplex_capabilities().has_speaker_output()
    }

    /// 回放参考链路是否可用。该链路表示平台能提供“当前实际送往扬声器的数据参考”，
    /// 可用于本地打断判定或未来真实 AEC；默认从 `audio_duplex_capabilities()` 派生。
    fn audio_reference_ready(&self) -> bool {
        self.audio_duplex_capabilities().has_reference_capture()
    }

    /// 当前已初始化音频拓扑的双工/打断能力。平台实现必须返回真实已落地的能力快照，
    /// 不允许业务层再根据 ready 状态自行推导更强口径。
    fn audio_duplex_capabilities(&self) -> AudioDuplexCapabilities {
        AudioDuplexCapabilities::unavailable()
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

    /// 尝试非阻塞写入 PCM i16 单声道采样帧；返回实际接收的采样数。
    /// 默认回退到阻塞写完整段，供未实现软队列的平台保持兼容。
    fn try_write_speaker_pcm_i16(&self, buf: &[i16]) -> Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.write_speaker_pcm_i16(buf)?;
        Ok(buf.len())
    }

    /// 读取与扬声器实际输出对齐的 PCM i16 参考帧。默认返回不支持错误。
    fn read_playback_reference_pcm_i16(&self, _out: &mut [i16]) -> Result<usize> {
        Err(crate::error::Error::config(
            "audio_reference",
            "Playback reference input not supported on this platform",
        ))
    }

    /// 当前喇叭输出队列中已缓冲的采样数。默认 0（平台未暴露队列深度）。
    fn speaker_buffered_samples(&self) -> usize {
        0
    }

    /// 清空当前喇叭软件/平台缓冲，供 realtime 打断时立即丢弃陈旧下行音频。
    /// Default no-op so non-audio platforms do not need a special implementation.
    fn clear_speaker_buffer(&self) -> Result<()> {
        Ok(())
    }

    /// Push PCM samples into the speaker staging ring buffer.
    /// The staging buffer decouples WSS data arrival from I2S consumption:
    /// audio_io_worker transfers staging → speaker autonomously.
    /// Returns the number of samples actually written.
    /// Default returns 0 (platform has no staging buffer).
    fn push_speaker_staging_pcm_i16(&self, _buf: &[i16]) -> Result<usize> {
        Ok(0)
    }

    /// Returns the number of samples currently in the speaker staging buffer.
    /// Default 0 (platform has no staging buffer).
    fn speaker_staging_samples(&self) -> usize {
        0
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

    /// Whether the configured I2C bus has been successfully initialized.
    fn i2c_ready(&self) -> bool {
        false
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
    fn drive_i2c_sensor(&self, _addr: u8, _model: &str, _options: &Value) -> Result<String> {
        Err(crate::error::Error::config(
            "drive_i2c_sensor",
            "I2C sensor not supported on this platform",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioDuplexCapabilities, AudioDuplexProfile, AudioEchoCancellationCapability,
        AudioReferenceCapability,
    };

    #[test]
    fn platform_aec_contract_normalizes_to_input_reference_duplex() {
        let caps = AudioDuplexCapabilities {
            microphone_input: true,
            speaker_output: true,
            concurrent_capture_playback: false,
            barge_in: false,
            reference_capture: AudioReferenceCapability::None,
            echo_cancellation: AudioEchoCancellationCapability::Platform,
        }
        .normalized();
        assert_eq!(caps.profile(), AudioDuplexProfile::DuplexPlatformAec);
        assert!(caps.concurrent_capture_playback);
        assert!(caps.barge_in);
        assert_eq!(
            caps.reference_capture,
            AudioReferenceCapability::InputReference
        );
    }

    #[test]
    fn speaker_only_contract_does_not_claim_reference_or_aec() {
        let caps = AudioDuplexCapabilities::speaker_only().normalized();
        assert_eq!(caps.profile(), AudioDuplexProfile::SpeakerOnly);
        assert!(!caps.has_reference_capture());
        assert!(!caps.has_platform_aec());
    }
}
