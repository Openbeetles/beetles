//! 平台抽象：仅此处依赖 esp-idf-svc/硬件。核心域不依赖本模块。
//! Platform: only place that depends on esp-idf-svc/hardware.

pub mod abstraction;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) mod audio_drivers;
pub mod board_info;
pub mod csrf;
pub mod display_driver;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub mod esp32;
pub mod fetch_url;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub(crate) mod fs_atomic;
pub(crate) mod hardware_drivers;
pub(crate) mod heap;
pub mod heartbeat_file;
pub mod http_client;
pub mod http_server;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod linux;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod memory_linux;
pub mod nvs;
pub mod operator_status;
#[cfg(all(feature = "ota", any(target_arch = "xtensa", target_arch = "riscv32")))]
pub mod ota;
pub mod pairing;
pub(crate) mod psram_vec;
pub mod response;
pub mod response_body;
pub mod runtime_board;
pub mod sntp;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub mod softap_ip;
pub(crate) mod spiffs;
pub mod state_fs;
pub mod state_root;
pub(crate) mod task_affinity;
pub mod task_wdt;
pub mod time;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub mod wake_word;
pub mod wifi;

pub use abstraction::{
    AudioDuplexCapabilities, AudioDuplexProfile, AudioEchoCancellationCapability,
    AudioReferenceCapability, ConfigStore, HardwareCapability, HardwareDiscovery,
    HardwareDiscoveryBus, HardwareDiscoveryItem, HardwareDiscoveryQuery, HardwareDiscoveryResponse,
    MemorySnapshot, Platform, PlatformHttpClient, SkillMetaStore, SkillStorage, StateFs,
    StorageMediaInfo, StorageMediaKind,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use esp32::Esp32Platform;
pub use fetch_url::fetch_url_with_client;
pub use heartbeat_file::read_heartbeat_file;
pub use http_client::EspHttpClient;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use linux::LinuxPlatform;
pub use nvs::{
    default_config_store, default_config_store_arc, erase_namespace, init_nvs, read_string,
    write_string, NvsConfigStore,
};
pub use response_body::ResponseBody;
pub use sntp::init_sntp;
pub use spiffs::{
    default_skill_storage_arc, init_spiffs, spiffs_base_string, spiffs_usage,
    SpiffsCalendarProviderCredentialStore, SpiffsCalendarStore, SpiffsContinuityCapsuleStore,
    SpiffsLongTermMemoryExtractionStateStore, SpiffsLongTermMemoryStore, SpiffsMemoryStore,
    SpiffsMentalPrivacyStore, SpiffsSessionStore, SpiffsSkillMetaStore, SpiffsSkillStorage,
    SpiffsTaskArtifactStore, SpiffsTaskExecutionLedgerStore, SpiffsTaskLearningStore,
    SpiffsTaskRunStore, SpiffsTaskStore, SpiffsTurnLedgerStore,
};
pub use state_root::state_mount_path;
pub use wifi::{
    connect as connect_wifi, is_wifi_sta_connected, wait_for_network_ready, WifiApEntry, WifiScan,
    WifiScanHandle,
};
