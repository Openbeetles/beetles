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

/// 共享：加载配置、校验、WiFi 连接；ESP 侧含启动进度条与 display 初始化（与 Linux 同路径，无重复 main 逻辑）。
pub fn bootstrap_config_and_wifi(platform: &Arc<dyn Platform>) -> (Arc<AppConfig>, bool) {
    let config_store = platform.config_store();
    let config_file_store = config::PlatformConfigFileStore(Arc::clone(platform));
    let config = Arc::new(AppConfig::load(
        config_store.as_ref(),
        Some(&config_file_store),
    ));
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
            log::info!(
                "[{}] WiFi stack ready (SoftAP + scan; STA may still be negotiating)",
                TAG
            );
            platform.init_sntp();
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            if platform.display_available() {
                let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 2 });
            }
            true
        }
        Err(e) => {
            log::warn!("[{}] WiFi init failed: {}", TAG, e);
            false
        }
    };

    // HTTP config API (all targets): CSRF must be initialized regardless of WiFi outcome.
    if let Err(e) = crate::platform::csrf::init() {
        log::error!("[{}] csrf init failed: {}", TAG, e);
        std::process::exit(1);
    }

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
                let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 0 });
                let _ = platform.display_command(DisplayCommand::RefreshDashboard {
                    state: DisplaySystemState::Booting,
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
            }
        }
    }
    if wifi_init_ok && platform.display_available() {
        let ip = platform
            .wifi_sta_ip()
            .unwrap_or_else(|| SOFTAP_DEFAULT_IPV4.to_string());
        let uptime_secs = crate::platform::time::uptime_secs();
        let _ = platform.display_command(DisplayCommand::UpdateIp { ip, uptime_secs });
    }
}

/// 启动后音频初始化（从 bootstrap 移出，由 run_app 在 MessageBus 创建后调用）。
/// Audio init after boot (moved out of bootstrap; called by run_app after MessageBus).
pub fn init_audio_if_enabled(platform: &Arc<dyn Platform>, config: &Arc<AppConfig>) {
    if let Some(audio_cfg) = config.audio.as_ref() {
        if audio_cfg.enabled {
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
            }
        }
    }
}
