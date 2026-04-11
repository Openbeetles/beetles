//! Bootstrap utilities for beetle application.
//! 应用启动引导工具。

use crate::config::{self, AppConfig};
use crate::Platform;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use crate::{
    constants::SOFTAP_DEFAULT_IPV4, DisplayChannelStatus, DisplayCommand, DisplayPressureLevel,
    DisplaySystemState,
};
use std::sync::Arc;

const TAG: &str = "bootstrap";

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn enforce_heap_checkpoint(stage: &'static str) {
    if let Err(error) = crate::platform::debug_heap_checkpoint(stage) {
        log::error!("[{}] {}", TAG, error);
        panic!("[{}] {}", TAG, error);
    }
}

/// 共享：只加载配置，不触发 WiFi、显示、音频等启动副作用。
pub fn load_config(platform: &Arc<dyn Platform>) -> Arc<AppConfig> {
    let config_store = platform.config_store();
    let config_file_store = config::PlatformConfigFileStore(Arc::clone(platform));
    let config = Arc::new(AppConfig::load(
        config_store.as_ref(),
        Some(&config_file_store),
    ));
    crate::runtime::sync_pairing_state_from_store(config_store.as_ref());
    if let Err(e) = config.validate_proxy() {
        log::warn!("[{}] config validate_proxy: {}", TAG, e);
    }
    if let Err(e) = config.validate_for_channels() {
        log::warn!("[{}] config validate_for_channels: {}", TAG, e);
    }
    log::info!(
        "[{}] config loaded (wifi_ssid set: {}, proxy set: {})",
        TAG,
        !config.wifi_ssid.is_empty(),
        !config.proxy_url.is_empty()
    );
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::orchestrator::log_startup_memory_checkpoint("config_loaded");
    config
}

/// 共享：加载配置、校验、WiFi 连接；ESP 侧含启动进度条与 display 初始化（与 Linux 同路径，无重复 main 逻辑）。
pub fn bootstrap_config_and_wifi(platform: &Arc<dyn Platform>) -> (Arc<AppConfig>, bool) {
    let config = load_config(platform);

    if !config.wifi_ssid.is_empty() {
        if let Err(e) = config.validate_for_wifi() {
            log::warn!("[{}] config validate_for_wifi: {}", TAG, e);
        }
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if platform.display_available() {
        let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 1 });
    }
    let wifi_init_ok = match platform.connect_wifi(config.as_ref()) {
        Ok(()) => {
            #[cfg(target_os = "linux")]
            log::info!(
                "[{}] WiFi stack ready (Linux inherited valid WiFi or provisioning fallback is ready)",
                TAG
            );
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            log::info!(
                "[{}] WiFi stack ready (SoftAP + scan; STA may still be negotiating)",
                TAG
            );
            platform.init_sntp();
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            if platform.display_available() {
                let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 2 });
            }
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            crate::orchestrator::log_startup_memory_checkpoint("wifi_stack_ready");
            true
        }
        Err(e) => {
            log::warn!("[{}] WiFi init failed: {}", TAG, e);
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            crate::orchestrator::log_startup_memory_checkpoint("wifi_stack_failed");
            false
        }
    };

    // HTTP config API (all targets): CSRF must be initialized regardless of WiFi outcome.
    if let Err(e) = crate::platform::csrf::init() {
        log::error!("[{}] csrf init failed: {}", TAG, e);
        std::process::exit(1);
    }
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::orchestrator::log_startup_memory_checkpoint("csrf_initialized");

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    post_wifi_display_bootstrap(platform, &config, wifi_init_ok);
    // Audio init has been moved to run_app, after MessageBus creation, so that
    // the wake-word engine can receive a valid inbound sender on first boot.

    (config, wifi_init_ok)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn post_wifi_display_bootstrap(
    platform: &Arc<dyn Platform>,
    config: &Arc<AppConfig>,
    wifi_init_ok: bool,
) {
    if let Some(display_cfg) = config.display.as_ref() {
        if display_cfg.enabled {
            if let Err(e) = platform.init_display(display_cfg) {
                log::warn!("[{}] display init failed (degraded): {}", TAG, e);
            } else {
                log::info!("[{}] display initialized", TAG);
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                crate::orchestrator::log_startup_memory_checkpoint("display_initialized");
                enforce_heap_checkpoint("heap_after_display_init");
                let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 0 });
                enforce_heap_checkpoint("heap_after_display_boot_stage0");
                let _ = platform.display_command(DisplayCommand::RefreshDashboard {
                    state: DisplaySystemState::Booting,
                    presence_subtitle: Some("restoring runtime shell".to_string()),
                    wifi_connected: false,
                    ip_address: None,
                    channels: [
                        DisplayChannelStatus {
                            name: "telegram",
                            enabled: config.enabled_channel == "telegram",
                            healthy: false,
                            consecutive_failures: 0,
                        },
                        DisplayChannelStatus {
                            name: "feishu",
                            enabled: config.enabled_channel == "feishu",
                            healthy: false,
                            consecutive_failures: 0,
                        },
                        DisplayChannelStatus {
                            name: "dingtalk",
                            enabled: config.enabled_channel == "dingtalk",
                            healthy: false,
                            consecutive_failures: 0,
                        },
                        DisplayChannelStatus {
                            name: "wecom",
                            enabled: config.enabled_channel == "wecom",
                            healthy: false,
                            consecutive_failures: 0,
                        },
                        DisplayChannelStatus {
                            name: "qq_channel",
                            enabled: config.enabled_channel == "qq_channel",
                            healthy: false,
                            consecutive_failures: 0,
                        },
                    ],
                    pressure: DisplayPressureLevel::Normal,
                    heap_percent: 0,
                    messages_in: 0,
                    messages_out: 0,
                    last_active_epoch_secs: 0,
                    uptime_secs: 0,
                    busy_phase: false,
                    llm_last_ms: 0,
                    error_flash: false,
                });
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                crate::orchestrator::log_startup_memory_checkpoint("display_boot_dashboard");
                enforce_heap_checkpoint("heap_after_display_boot_dashboard");
            }
        }
    }
    if wifi_init_ok && platform.display_available() {
        let ip = platform
            .wifi_sta_ip()
            .unwrap_or_else(|| SOFTAP_DEFAULT_IPV4.to_string());
        let uptime_secs = crate::platform::time::uptime_secs();
        let _ = platform.display_command(DisplayCommand::UpdateIp {
            ip,
            presence_subtitle: None,
            uptime_secs,
        });
    }
}

/// 启动后音频初始化（从 bootstrap 移出，由 run_app 在 MessageBus 创建后调用）。
/// Audio init after boot (moved out of bootstrap; called by run_app after MessageBus).
pub fn init_audio_if_enabled(platform: &Arc<dyn Platform>, config: &Arc<AppConfig>) {
    let registry = crate::build_device_capability_registry(config.as_ref(), platform.as_ref());
    if !registry.is_mounted(crate::DEVICE_CAPABILITY_VOICE) {
        return;
    }
    if let Some(audio_cfg) = config.audio.as_ref() {
        if let Err(e) = platform.init_audio(audio_cfg) {
            log::warn!("[{}] audio init failed (degraded): {}", TAG, e);
        } else {
            let caps = platform.audio_duplex_capabilities();
            log::info!(
                "[{}] audio initialized (profile={} mic={} speaker={} duplex={} barge_in={} reference={:?} aec={:?})",
                TAG,
                caps.profile().as_str(),
                caps.microphone_input,
                caps.speaker_output,
                caps.concurrent_capture_playback,
                caps.barge_in,
                caps.reference_capture,
                caps.echo_cancellation
            );
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            crate::orchestrator::log_startup_memory_checkpoint("audio_initialized");
        }
    }
}
