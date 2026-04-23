//! Bootstrap utilities for beetle application.
//! 应用启动引导工具。

use crate::config::{self, AppConfig};
use crate::memory::MemorySystemKind;
use crate::Platform;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use crate::{
    constants::SOFTAP_DEFAULT_IPV4, DisplayChannelStatus, DisplayCommand, DisplayPressureLevel,
    DisplaySystemState,
};
use std::sync::Arc;

const TAG: &str = "bootstrap";

fn should_init_sntp_after_wifi_attempt(
    memory_system_kind: MemorySystemKind,
    wifi_init_ok: bool,
) -> bool {
    wifi_init_ok || memory_system_kind == MemorySystemKind::LinuxFull
}

fn handle_heap_checkpoint_result(
    log_tag: &'static str,
    stage: &'static str,
    result: crate::Result<()>,
) -> bool {
    match result {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "[{}] heap checkpoint degraded stage={}: {}",
                log_tag,
                stage,
                error
            );
            false
        }
    }
}

/// Observe a heap debug checkpoint without turning a failed debug probe into a production panic.
/// 观察调试期 heap checkpoint；失败只记录降级，不再升级为生产崩溃。
pub fn observe_heap_checkpoint(log_tag: &'static str, stage: &'static str) -> bool {
    handle_heap_checkpoint_result(
        log_tag,
        stage,
        crate::platform::debug_heap_checkpoint(stage),
    )
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
    if should_init_sntp_after_wifi_attempt(platform.memory_system_kind(), wifi_init_ok) {
        if !wifi_init_ok {
            log::info!(
                "[{}] starting SNTP background sync despite WiFi init failure (Linux may still have a usable uplink)",
                TAG
            );
        }
        platform.init_sntp();
    }

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
                observe_heap_checkpoint(TAG, "heap_after_display_init");
                let mut channels =
                    [DisplayChannelStatus::hidden(); crate::DISPLAY_CHANNEL_CAPACITY];
                let normalized_enabled =
                    crate::normalize_compiled_enabled_channel(&config.enabled_channel);
                for (index, entry) in crate::display_channel_entries().enumerate() {
                    channels[index] = DisplayChannelStatus {
                        name: entry.id,
                        display_label: entry.display_label,
                        visible: true,
                        enabled: normalized_enabled == entry.id,
                        healthy: false,
                        consecutive_failures: 0,
                    };
                }
                let ip_address = if wifi_init_ok {
                    Some(
                        platform
                            .wifi_sta_ip()
                            .unwrap_or_else(|| SOFTAP_DEFAULT_IPV4.to_string()),
                    )
                } else {
                    None
                };
                let _ = platform.display_command(DisplayCommand::RefreshDashboard {
                    state: DisplaySystemState::Booting,
                    presence_subtitle: Some("restoring runtime shell".to_string()),
                    ip_address,
                    channels,
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
                observe_heap_checkpoint(TAG, "heap_after_display_boot_dashboard");
            }
        }
    }
}

/// 启动后音频初始化（从 bootstrap 移出，由 run_app 在 MessageBus 创建后调用）。
/// Audio init after boot (moved out of bootstrap; called by run_app after MessageBus).
pub fn init_audio_if_enabled(platform: &Arc<dyn Platform>, config: &Arc<AppConfig>) {
    if !crate::compiled_voice_capability() {
        return;
    }
    let Some(audio_cfg) = config.audio.as_ref() else {
        return;
    };
    if !audio_cfg.enabled {
        return;
    }
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

#[cfg(test)]
mod tests {
    use super::should_init_sntp_after_wifi_attempt;
    use crate::memory::MemorySystemKind;

    #[test]
    fn linux_full_keeps_sntp_alive_even_when_wifi_bootstrap_failed() {
        assert!(should_init_sntp_after_wifi_attempt(
            MemorySystemKind::LinuxFull,
            false
        ));
    }

    #[test]
    fn esp_compact_still_requires_wifi_stack_ready_before_sntp() {
        assert!(!should_init_sntp_after_wifi_attempt(
            MemorySystemKind::EspCompact,
            false
        ));
        assert!(should_init_sntp_after_wifi_attempt(
            MemorySystemKind::EspCompact,
            true
        ));
    }

    #[test]
    fn heap_checkpoint_error_returns_false_without_panicking() {
        let outcome = std::panic::catch_unwind(|| {
            assert!(!super::handle_heap_checkpoint_result(
                "bootstrap",
                "heap_after_display_init",
                Err(crate::error::Error::config(
                    "heap_after_display_init",
                    "heap integrity check failed",
                )),
            ));
        });
        assert!(outcome.is_ok());
    }

    #[test]
    fn heap_checkpoint_ok_returns_true() {
        assert!(super::handle_heap_checkpoint_result(
            "bootstrap",
            "heap_after_display_init",
            Ok(()),
        ));
    }
}
