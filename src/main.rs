//! 甲壳虫 (beetle) - ESP32-S3 firmware entry.
//! Firmware version is embedded for operator surfaces and diagnostics.
//! Startup order: ESP app_main initializes platform/logging, then hands off
//! Rust-heavy boot/runtime work to runtime_bootstrap with an explicit stack.
//! ESP32: no graceful shutdown; process runs until power off.
#![allow(clippy::items_after_test_module)]

mod app_runtime_support;

use beetle::bus::IngressKind;
#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(unused_imports))]
use beetle::constants::SOFTAP_DEFAULT_IPV4;
#[cfg(feature = "feishu")]
use beetle::network::{execute_stream_http_op, HttpFactory};
use beetle::network::{HttpClientClass, NetworkGovernor};
#[cfg(any(feature = "telegram", feature = "feishu"))]
use beetle::parse_allowed_chat_ids;
#[cfg(feature = "feishu")]
use beetle::run_feishu_ws_loop;
#[cfg(feature = "cli")]
use beetle::runtime::spawn_planned;
use beetle::runtime::{spawn_planned_handle, thread_plan};
#[cfg(feature = "telegram")]
use beetle::send_chat_action;
#[cfg(any(
    feature = "feishu",
    all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ),
    feature = "wecom",
    feature = "qq_channel"
))]
use beetle::util::STACK_CHANNEL_WS;
use beetle::util::STACK_VOICE_CONTROL;
use beetle::util::{STACK_AGENT_LOOP, STACK_DISPATCH};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use beetle::Esp32Platform;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use beetle::LinuxPlatform;
use beetle::Platform;
#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
use beetle::DISPLAY_CHANNEL_CAPACITY;
use beetle::{run_agent_loop, run_dispatch, AppConfig, DEFAULT_CAPACITY};
#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(unused_imports))]
use beetle::{
    DisplayChannelRuntimeStatus, DisplayChannelStatus, DisplayCommand, DisplayOwner,
    DisplayPressureLevel, DisplaySystemState,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use clap::Parser;

#[cfg(any(feature = "feishu", feature = "dingtalk", feature = "qq_channel"))]
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(any(feature = "feishu", feature = "dingtalk", feature = "qq_channel"))]
use std::sync::Mutex;
#[cfg(any(feature = "config_api", feature = "telegram"))]
use std::sync::RwLock;
#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(unused_imports))]
use std::time::{Duration, Instant};

const TAG: &str = "beetle";
const VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
const DISPLAY_LIFECYCLE_OWNER: &str = "display";

type CapabilityPackageTextProvider = Arc<dyn Fn(&str, usize) -> Option<String> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentHttpOpenFailureAction {
    RetryAfterWifiRecovery,
    RestartRuntime,
}

fn agent_http_open_failure_action(error: &beetle::Error) -> AgentHttpOpenFailureAction {
    if error.stage() == "wifi_not_ready" {
        AgentHttpOpenFailureAction::RetryAfterWifiRecovery
    } else {
        AgentHttpOpenFailureAction::RestartRuntime
    }
}

struct VoiceEventChannel {
    speak_capable: bool,
    tx: std::sync::mpsc::SyncSender<beetle::audio::voice_session::VoiceEvent>,
    rx: std::sync::mpsc::Receiver<beetle::audio::voice_session::VoiceEvent>,
}

struct StartedVoiceSession {
    speak_capable: bool,
    tx: std::sync::mpsc::SyncSender<beetle::audio::voice_session::VoiceEvent>,
}

#[cfg(feature = "telegram")]
struct TelegramTypingNotifier {
    token: String,
}

struct RuntimeBus {
    user_inbound_tx: beetle::bus::UserInboundTx,
    user_inbound_rx: Option<beetle::bus::InboundRx>,
    user_inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    system_inbound_tx: beetle::bus::SystemInboundTx,
    system_inbound_rx: Option<beetle::bus::SystemInboundRx>,
    system_inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_tx: beetle::bus::OutboundTx,
    outbound_rx: Option<beetle::bus::OutboundRx>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
}

impl RuntimeBus {
    fn new(capacity: usize) -> Self {
        let (user_inbound_tx, user_inbound_rx, user_inbound_depth) =
            beetle::bus::new_user_inbound_channel(capacity);
        let (system_inbound_tx, system_inbound_rx, system_inbound_depth) =
            beetle::bus::new_system_inbound_channel(capacity);
        let (outbound_tx, outbound_rx, outbound_depth) = beetle::bus::new_inbound_channel(capacity);
        Self {
            user_inbound_tx,
            user_inbound_rx: Some(user_inbound_rx),
            user_inbound_depth,
            system_inbound_tx,
            system_inbound_rx: Some(system_inbound_rx),
            system_inbound_depth,
            outbound_tx,
            outbound_rx: Some(outbound_rx),
            outbound_depth,
        }
    }
}

struct PreparedRuntimeAssembly {
    runtime: beetle::RuntimeServices,
    config: Arc<AppConfig>,
    resolve_locale_ui: Arc<dyn Fn() -> beetle::i18n::Locale + Send + Sync>,
    skill_prompt_cache: Arc<beetle::skills::SkillPromptCache>,
    bus: RuntimeBus,
    #[cfg(all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    dingtalk_session_store: beetle::channels::DingtalkSessionStore,
    #[cfg(feature = "wecom")]
    wecom_aibot_route_store: beetle::channels::WecomAibotRouteStore,
    #[cfg(feature = "qq_channel")]
    qq_msg_id_cache: beetle::channels::QqMsgIdCache,
    #[cfg(feature = "qq_channel")]
    qq_inbound_dedup_store: beetle::channels::QqInboundDedupStore,
    #[cfg(feature = "qq_channel")]
    qq_token_cache: beetle::channels::SharedQqTokenCache,
    #[cfg(feature = "qq_channel")]
    qq_ws_status: beetle::channels::SharedQqWsStatus,
    registry: Arc<beetle::ToolRegistry>,
    baidu_token_cache: Option<Arc<beetle::audio::baidu_token::BaiduTokenCache>>,
    voice_event_channel: Option<VoiceEventChannel>,
    device_capability_registry: beetle::DeviceCapabilityRegistry,
    channel_capability_registry: Arc<beetle::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<beetle::CapabilityPackageRuntimeCapabilities>,
    network_governor: Arc<NetworkGovernor>,
    chat_streams: Arc<beetle::ChatStreamBroker>,
    #[cfg(feature = "config_api")]
    config_api_shared_config: Option<Arc<RwLock<AppConfig>>>,
}

#[cfg(feature = "telegram")]
impl beetle::TypingNotifier for TelegramTypingNotifier {
    fn notify(&mut self, channel: &str, chat_id: &str, http: &mut dyn beetle::PlatformHttpClient) {
        if channel == beetle::CHANNEL_TELEGRAM {
            let _ = send_chat_action(http, &self.token, chat_id, "typing");
        }
    }
}

#[cfg(feature = "config_api")]
struct HttpServerSpawnContext {
    platform: Arc<dyn Platform>,
    tool_registry: Arc<beetle::tools::ToolRegistry>,
    channel_capability_registry: Arc<beetle::ChannelCapabilityRegistry>,
    capability_package_runtime_capabilities: Arc<beetle::CapabilityPackageRuntimeCapabilities>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn beetle::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn beetle::memory::SessionStore + Send + Sync>,
    system_inbound_tx: beetle::bus::SystemInboundTx,
    skill_prompt_cache: Arc<beetle::skills::SkillPromptCache>,
    inbound_tx: beetle::bus::UserInboundTx,
    chat_streams: Arc<beetle::ChatStreamBroker>,
    shared_config: Arc<RwLock<AppConfig>>,
}

/// 从 orchestrator snapshot 的 internal 堆空闲字节数估算已用百分比。
/// 以运行时首次观测到的空闲值作为动态基线（首次调用时的空闲量，此时大部分业务线程已启动），
/// 反映业务层实际消耗，而非 ESP-IDF 框架本身的固有开销。Linux 永远返回 0（无 PSRAM 堆基线）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn heap_used_percent(snapshot: &beetle::orchestrator::ResourceSnapshot) -> u8 {
    use std::sync::atomic::{AtomicU32, Ordering};
    // 0 means "not yet calibrated"; first call sets the baseline.
    static INTERNAL_BASELINE: AtomicU32 = AtomicU32::new(0);

    let free = snapshot.heap_free_internal;
    let baseline = INTERNAL_BASELINE.load(Ordering::Relaxed);
    if baseline == 0 {
        // First observation — use it as our 100% reference point.
        // This is typically the orchestrator baseline (~219KB), before
        // threads/TLS connections consume their share.
        INTERNAL_BASELINE.store(free, Ordering::Relaxed);
        return 0; // first call: nothing consumed yet relative to baseline
    }
    // If current free exceeds baseline (e.g. after TLS session teardown),
    // update baseline upward so percentage never goes negative.
    if free > baseline {
        INTERNAL_BASELINE.store(free, Ordering::Relaxed);
        return 0;
    }
    let used = baseline - free;
    ((used as u64 * 100) / baseline as u64).min(100) as u8
}

#[cfg(feature = "feishu")]
struct FeishuStreamEditor {
    app_id: String,
    app_secret: String,
    create_http: Arc<HttpFactory>,
    state: Mutex<beetle::FeishuTokenCache>,
}

#[cfg(feature = "feishu")]
impl beetle::StreamEditor for FeishuStreamEditor {
    fn send_initial(&self, chat_id: &str, content: &str) -> beetle::Result<Option<String>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        execute_stream_http_op(
            self.create_http.as_ref(),
            "feishu_stream_send_initial",
            |http| {
                let token =
                    state.ensure_token(http, &self.app_id, &self.app_secret, "feishu_stream")?;
                let r = beetle::feishu_send_and_get_id(http, &token, chat_id, content);
                if r.is_err() {
                    state.invalidate();
                }
                r
            },
        )
    }

    fn edit(&self, _chat_id: &str, message_id: &str, content: &str) -> beetle::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        execute_stream_http_op(self.create_http.as_ref(), "feishu_stream_edit", |http| {
            let token =
                state.ensure_token(http, &self.app_id, &self.app_secret, "feishu_stream")?;
            let r = beetle::feishu_edit_message(http, &token, message_id, content);
            if r.is_err() {
                state.invalidate();
            }
            r
        })
    }
}

/// F2: 根据当前状态计算下一轮显示刷新间隔（秒）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn compute_refresh_secs(
    state: DisplaySystemState,
    backlight_off: bool,
    last_activity_at: &std::time::Instant,
) -> u64 {
    use beetle::constants::*;
    if backlight_off {
        return DISPLAY_REFRESH_SLEEP_SECS;
    }
    match state {
        DisplaySystemState::Busy | DisplaySystemState::Recording => DISPLAY_REFRESH_BUSY_SECS,
        DisplaySystemState::Idle | DisplaySystemState::NoWifi => {
            if last_activity_at.elapsed().as_secs() >= DISPLAY_IDLE_LONG_THRESHOLD_SECS {
                DISPLAY_REFRESH_IDLE_LONG_SECS
            } else {
                DISPLAY_REFRESH_IDLE_SECS
            }
        }
        _ => DISPLAY_REFRESH_IDLE_SECS,
    }
}

#[cfg(feature = "config_api")]
fn spawn_http_config_server(
    ctx: HttpServerSpawnContext,
) -> beetle::Result<beetle::util::TaskHandle> {
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let (linux_config_listen, linux_config_listener) =
        beetle::platform::http_server::bind_linux_config_http_listener()?;
    let restart_platform = Arc::clone(&ctx.platform);
    // Wrapper thread still owns the control-plane lifecycle and route registration surface;
    // keep the historical stack headroom while the direct/worker split is under validation.
    spawn_planned_handle(
        "config_plane_watch",
        beetle::util::STACK_CONFIG_PLANE_WATCH,
        move || {
            #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
            let result = beetle::platform::http_server::run(
                ctx.platform,
                ctx.tool_registry,
                ctx.channel_capability_registry,
                ctx.capability_package_runtime_capabilities,
                ctx.inbound_depth,
                ctx.outbound_depth,
                ctx.memory_store,
                ctx.session_store,
                ctx.system_inbound_tx,
                ctx.skill_prompt_cache,
                ctx.inbound_tx,
                ctx.chat_streams,
                ctx.shared_config,
            );
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            let result = beetle::platform::http_server::run_with_bound_listener(
                linux_config_listener,
                linux_config_listen,
                ctx.platform,
                ctx.tool_registry,
                ctx.channel_capability_registry,
                ctx.capability_package_runtime_capabilities,
                ctx.inbound_depth,
                ctx.outbound_depth,
                ctx.memory_store,
                ctx.session_store,
                ctx.system_inbound_tx,
                ctx.skill_prompt_cache,
                ctx.inbound_tx,
                ctx.chat_streams,
                ctx.shared_config,
            );
            if let Err(e) = result {
                beetle::state::set_last_error(&e);
                log::error!(
                    "[{}] HTTP config API server failed; requesting restart: {}",
                    TAG,
                    e
                );
                restart_platform.request_restart();
            }
        },
    )
    .map_err(|error| beetle::Error::io("config_plane_spawn", error))
}

fn spawn_voice_session_if_ready(
    platform: &Arc<dyn Platform>,
    network: &Arc<NetworkGovernor>,
    config: &Arc<AppConfig>,
    baidu_token_cache: Option<&Arc<beetle::audio::baidu_token::BaiduTokenCache>>,
    user_inbound_tx: &beetle::bus::UserInboundTx,
    voice_event_tx_rx: &mut Option<VoiceEventChannel>,
) -> beetle::Result<Option<StartedVoiceSession>> {
    beetle::wake::shutdown();
    let Some(VoiceEventChannel {
        speak_capable,
        tx: voice_tx,
        rx: voice_rx,
        ..
    }) = voice_event_tx_rx.take()
    else {
        return Ok(None);
    };
    let Some(audio_cfg) = config.audio.as_ref() else {
        return Ok(None);
    };
    let vs_platform = Arc::clone(platform);
    let vs_network = Arc::clone(network);
    let vs_audio = audio_cfg.clone();
    let vs_token = baidu_token_cache.cloned();
    let vs_inbound_tx = user_inbound_tx.clone();
    let vs_prompt = audio_cfg.wake_word.wake_prompt.clone();
    spawn_planned_handle("voice_session", STACK_VOICE_CONTROL, move || {
        beetle::audio::voice_session::run_voice_session(
            beetle::audio::voice_session::VoiceSessionConfig {
                platform: vs_platform,
                network: vs_network,
                audio_cfg: vs_audio,
                baidu_token: vs_token,
                inbound_tx: vs_inbound_tx,
                wake_prompt: vs_prompt,
            },
            voice_rx,
        );
    })
    .map_err(|error| beetle::Error::io("voice_session_spawn", error))?;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("voice_session_spawn");
    let wake_backend = if audio_cfg.wake_word.enabled {
        beetle::wake::WakeBackend::Acoustic(beetle::wake::AcousticWakeBackend::from_audio_config(
            audio_cfg,
        ))
    } else {
        beetle::wake::WakeBackend::Disabled
    };
    beetle::wake::configure(wake_backend, voice_tx.clone());
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("wake_backend_configured");
    Ok(Some(StartedVoiceSession {
        speak_capable,
        tx: voice_tx,
    }))
}

fn voice_sink_sender(
    started_voice_session: Option<&StartedVoiceSession>,
) -> Option<std::sync::mpsc::SyncSender<beetle::audio::voice_session::VoiceEvent>> {
    started_voice_session
        .filter(|session| session.speak_capable)
        .map(|session| session.tx.clone())
}

fn finalize_required_thread_start<F>(
    tag: &str,
    started_label: &str,
    stage: &'static str,
    spawn: F,
) -> beetle::Result<()>
where
    F: FnOnce() -> std::io::Result<beetle::util::TaskHandle>,
{
    spawn().map_err(|error| beetle::Error::io(stage, error))?;
    log::info!("[{}] {}", tag, started_label);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint(stage);
    Ok(())
}

#[cfg(any(
    feature = "telegram",
    all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ),
    test
))]
fn spawn_required_planned_thread<F>(
    tag: &str,
    name: &str,
    stack_size: usize,
    started_label: &str,
    stage: &'static str,
    f: F,
) -> beetle::Result<()>
where
    F: FnOnce() + Send + 'static,
{
    finalize_required_thread_start(tag, started_label, stage, || {
        spawn_planned_handle(name, stack_size, f)
    })
}

#[cfg(any(feature = "feishu", feature = "qq_channel", feature = "wecom", test))]
fn spawn_supervised_channel_wss_thread(
    tag: &str,
    owner: &'static str,
    started_label: &str,
    _stage: &'static str,
    spawner: Arc<dyn Fn() -> beetle::Result<beetle::util::TaskHandle> + Send + Sync>,
) -> beetle::Result<()> {
    let handle = spawner()?;
    beetle::runtime::register_channel_wss_supervisor(owner, handle, spawner);
    log::info!("[{}] {}", tag, started_label);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint(_stage);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VoiceRuntimeCapabilities {
    speak_capable: bool,
    wake_capable: bool,
}

fn compute_voice_runtime_capabilities(
    audio_cfg: &beetle::config::AudioSegment,
    duplex_caps: beetle::AudioDuplexCapabilities,
    has_baidu_token: bool,
) -> VoiceRuntimeCapabilities {
    let speak_capable =
        audio_cfg.speaker.enabled && duplex_caps.has_speaker_output() && has_baidu_token;
    let wake_capable = if !audio_cfg.wake_word.enabled
        || !audio_cfg.microphone.enabled
        || !duplex_caps.has_microphone_input()
    {
        false
    } else if beetle::config::audio_realtime_enabled(audio_cfg) {
        audio_cfg.speaker.enabled && duplex_caps.can_run_realtime_session()
    } else {
        has_baidu_token
    };
    VoiceRuntimeCapabilities {
        speak_capable,
        wake_capable,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compute_voice_runtime_capabilities, finalize_required_thread_start,
        format_soul_kernel_recovery_report_for_log, register_process_memory_snapshot_provider,
        startup_banner_lines, voice_sink_sender, wait_for_startup_recovery_report,
        DisplayChannelRuntimeStatus, DisplayLoopState, StartedVoiceSession,
        StartupRecoveryWaitOutcome, VERSION,
    };
    use beetle::{
        config::default_disabled_audio_segment, DisplaySystemState, LinuxPlatform, Platform,
        DISPLAY_CHANNEL_CAPACITY,
    };
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn soul_kernel_startup_log_includes_degradation_reasons() {
        let status = beetle::runtime::SoulKernelStatus {
            minimum_viable: false,
            safe_mode_minimum_readable: false,
            degraded: true,
            session_chat_count: 1,
            key_memory_count: 0,
            degradation_reasons: vec![
                "missing_identity_anchor".to_string(),
                "missing_continuity_anchor".to_string(),
            ],
            ..Default::default()
        };
        let report = beetle::runtime::SoulKernelRecoveryReport {
            status_after: status,
            ..Default::default()
        };

        let line = format_soul_kernel_recovery_report_for_log(&report);

        assert!(line.contains("degraded=true"));
        assert!(line.contains("session_chats=1"));
        assert!(line.contains("key_memory=0"));
        assert!(line.contains("reasons=missing_identity_anchor|missing_continuity_anchor"));
    }

    #[test]
    fn soul_kernel_recovery_log_redacts_runtime_bundle_errors() {
        let status = beetle::runtime::SoulKernelStatus {
            degraded: true,
            degradation_reasons: vec![
                "runtime_bundle_unreadable:c2c:947B12A11A2E0348FAA2C60499D29345:token=abcdefghi123"
                    .to_string(),
            ],
            ..Default::default()
        };
        let report = beetle::runtime::SoulKernelRecoveryReport {
            restore_attempted: true,
            status_after: status,
            errors: vec![
                "runtime_bundle_import_failed:c2c:947B12A11A2E0348FAA2C60499D29345:token=abcdefghi123"
                    .to_string(),
            ],
            ..Default::default()
        };

        let line = format_soul_kernel_recovery_report_for_log(&report);

        assert!(line.contains("errors=runtime_bundle_import_failed"));
        assert!(line.contains("reasons=runtime_bundle_unreadable"));
        assert!(!line.contains("947B12A11A2E0348FAA2C60499D29345"));
        assert!(!line.contains("abcdefghi123"));
    }

    struct TestMemoryStore {
        has_memory: bool,
    }

    impl beetle::memory::MemoryStore for TestMemoryStore {
        fn get_memory(&self) -> beetle::Result<String> {
            self.has_memory
                .then(String::new)
                .ok_or_else(|| beetle::Error::config("memory", "missing"))
        }

        fn set_memory(&self, _content: &str) -> beetle::Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, _recent_n: usize) -> beetle::Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn get_daily_note(&self, _name: &str) -> beetle::Result<String> {
            Ok(String::new())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> beetle::Result<()> {
            Ok(())
        }
    }

    struct TestPendingRetryStore {
        loaded: Mutex<Option<beetle::PcMsg>>,
        cleared: Mutex<bool>,
    }

    impl beetle::memory::PendingRetryStore for TestPendingRetryStore {
        fn save_pending_retry(&self, _msg: &beetle::PcMsg) -> beetle::Result<()> {
            Ok(())
        }

        fn load_pending_retry(&self) -> beetle::Result<Option<beetle::PcMsg>> {
            Ok(self
                .loaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn clear_pending_retry(&self) -> beetle::Result<()> {
            *self.cleared.lock().unwrap_or_else(|e| e.into_inner()) = true;
            Ok(())
        }
    }

    #[test]
    fn voice_sink_requires_tts_token_even_if_speaker_ready() {
        let mut audio = default_disabled_audio_segment();
        audio.enabled = true;
        audio.speaker.enabled = true;
        let caps = compute_voice_runtime_capabilities(
            &audio,
            beetle::AudioDuplexCapabilities::speaker_only(),
            false,
        );
        assert!(!caps.speak_capable);
        assert!(!caps.wake_capable);
    }

    #[test]
    fn fallback_wake_runtime_requires_baidu_token() {
        let mut audio = default_disabled_audio_segment();
        audio.enabled = true;
        audio.microphone.enabled = true;
        audio.wake_word.enabled = true;
        let caps = compute_voice_runtime_capabilities(
            &audio,
            beetle::AudioDuplexCapabilities::microphone_only(),
            false,
        );
        assert!(!caps.speak_capable);
        assert!(!caps.wake_capable);
    }

    #[test]
    fn realtime_wake_runtime_requires_mic_and_speaker_but_not_baidu_token() {
        let mut audio = default_disabled_audio_segment();
        audio.enabled = true;
        audio.microphone.enabled = true;
        audio.speaker.enabled = true;
        audio.wake_word.enabled = true;
        audio.realtime.provider = "openai_compatible".to_string();
        audio.realtime.ws_url = "wss://example.invalid/realtime".to_string();
        audio.realtime.api_key = "k".to_string();
        audio.realtime.model = "gpt-realtime".to_string();
        audio.realtime.voice = "alloy".to_string();

        let caps = compute_voice_runtime_capabilities(
            &audio,
            beetle::AudioDuplexCapabilities::duplex_with_playback_reference(),
            false,
        );
        assert!(!caps.speak_capable);
        assert!(caps.wake_capable);
    }

    #[test]
    fn voice_sink_sender_requires_started_speaking_session() {
        let (tx, _rx) = std::sync::mpsc::sync_channel(1);
        let wake_only = StartedVoiceSession {
            speak_capable: false,
            tx: tx.clone(),
        };
        let speak_ready = StartedVoiceSession {
            speak_capable: true,
            tx,
        };

        assert!(voice_sink_sender(None).is_none());
        assert!(voice_sink_sender(Some(&wake_only)).is_none());
        assert!(voice_sink_sender(Some(&speak_ready)).is_some());
    }

    #[test]
    fn required_planned_thread_spawn_failure_is_propagated_with_stage() {
        let error = finalize_required_thread_start("beetle", "unused", "synthetic_spawn", || {
            Err(std::io::Error::other("synthetic spawn failure"))
        })
        .expect_err("spawn should fail");

        assert_eq!(error.stage(), "synthetic_spawn");
    }

    #[test]
    fn agent_http_open_wifi_not_ready_retries_without_runtime_restart() {
        let error = beetle::Error::config(
            "wifi_not_ready",
            "STA outbound network is not ready for HTTP client creation",
        );

        assert_eq!(
            super::agent_http_open_failure_action(&error),
            super::AgentHttpOpenFailureAction::RetryAfterWifiRecovery
        );
    }

    #[test]
    fn agent_http_open_non_wifi_error_still_restarts_runtime() {
        let error = beetle::Error::config("llm_config", "missing model");

        assert_eq!(
            super::agent_http_open_failure_action(&error),
            super::AgentHttpOpenFailureAction::RestartRuntime
        );
    }

    #[test]
    fn startup_recovery_wait_returns_report_without_sleeping() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        tx.send("ready").expect("send report");
        let mut feed_count = 0;

        let outcome = wait_for_startup_recovery_report(
            &rx,
            Duration::from_millis(50),
            Duration::from_millis(5),
            || feed_count += 1,
        );

        assert_eq!(outcome, StartupRecoveryWaitOutcome::Report("ready"));
        assert_eq!(feed_count, 0);
    }

    #[test]
    fn startup_recovery_wait_times_out_instead_of_blocking_forever() {
        let (_tx, rx) = std::sync::mpsc::sync_channel::<&'static str>(1);
        let mut feed_count = 0;

        let outcome =
            wait_for_startup_recovery_report(&rx, Duration::ZERO, Duration::from_millis(5), || {
                feed_count += 1
            });

        assert_eq!(outcome, StartupRecoveryWaitOutcome::TimedOut);
        assert_eq!(feed_count, 0);
    }

    #[test]
    fn startup_recovery_wait_reports_worker_exit_without_report() {
        let (tx, rx) = std::sync::mpsc::sync_channel::<&'static str>(1);
        drop(tx);

        let outcome = wait_for_startup_recovery_report(
            &rx,
            Duration::from_millis(50),
            Duration::from_millis(5),
            || {},
        );

        assert_eq!(outcome, StartupRecoveryWaitOutcome::Disconnected);
    }

    #[test]
    fn process_memory_provider_registration_updates_orchestrator_snapshot() {
        register_process_memory_snapshot_provider(Arc::new(|| beetle::platform::MemorySnapshot {
            heap_free_internal: 123,
            heap_min_free_internal: 100,
            heap_free_spiram: 456,
            heap_total_spiram: 1_000,
            heap_min_free_spiram: 400,
            heap_largest_block_spiram: 300,
            heap_largest_block: 78,
        }));
        beetle::orchestrator::update_heap_state();
        let snapshot = beetle::orchestrator::snapshot();

        assert_eq!(snapshot.heap_free_internal, 123);
        assert_eq!(snapshot.heap_min_free_internal, 100);
        assert_eq!(snapshot.heap_free_spiram, 456);
        assert_eq!(snapshot.heap_total_spiram, 1_000);
        assert_eq!(snapshot.heap_min_free_spiram, 400);
        assert_eq!(snapshot.heap_largest_block_spiram, 300);
        assert_eq!(snapshot.heap_used_spiram_est, 544);
        assert_eq!(snapshot.heap_largest_block_internal, 78);
    }

    #[test]
    fn startup_banner_lines_include_entrypoint_role() {
        let lines = startup_banner_lines("supervisor", Some("/tmp/beetle.toml"));

        assert_eq!(
            lines[1],
            format!("  甲壳虫 beetle v{} [supervisor]", VERSION)
        );
        assert!(lines
            .iter()
            .any(|line| line.contains("using config file: /tmp/beetle.toml")));
    }

    #[test]
    fn startup_self_check_requires_shared_memory() {
        let store = TestMemoryStore { has_memory: false };
        assert!(!super::app_runtime_support::startup_self_check(&store));
    }

    #[test]
    fn bootstrap_pending_retry_routes_system_message_to_system_inbound() {
        let pending = TestPendingRetryStore {
            loaded: Mutex::new(Some(
                beetle::PcMsg::new_system("system-maintenance", "chat-1", "retry later")
                    .expect("system msg"),
            )),
            cleared: Mutex::new(false),
        };
        let (user_inbound_tx, user_inbound_rx, _) = beetle::bus::new_user_inbound_channel(2);
        let (system_inbound_tx, system_inbound_rx, _) = beetle::bus::new_system_inbound_channel(2);

        super::app_runtime_support::bootstrap_pending_retry_into_inbound(
            &pending,
            &user_inbound_tx,
            &system_inbound_tx,
        );

        assert!(user_inbound_rx.try_recv().is_err());
        let system_msg = system_inbound_rx.try_recv().expect("system inbound");
        assert_eq!(system_msg.channel.as_ref(), "system-maintenance");
        assert!(*pending.cleared.lock().unwrap_or_else(|e| e.into_inner()));
    }

    #[test]
    fn bootstrap_pending_retry_drops_stale_qq_v2_passive_retry() {
        let mut stale = beetle::PcMsg::new_inbound(
            beetle::CHANNEL_QQ_CHANNEL,
            "c2c:chat-1",
            "retry later",
            false,
        )
        .expect("qq msg");
        stale.enqueue_ts_ms = 1;
        let pending = TestPendingRetryStore {
            loaded: Mutex::new(Some(stale)),
            cleared: Mutex::new(false),
        };
        let (user_inbound_tx, user_inbound_rx, _) = beetle::bus::new_user_inbound_channel(2);
        let (system_inbound_tx, system_inbound_rx, _) = beetle::bus::new_system_inbound_channel(2);

        super::app_runtime_support::bootstrap_pending_retry_into_inbound(
            &pending,
            &user_inbound_tx,
            &system_inbound_tx,
        );

        assert!(user_inbound_rx.try_recv().is_err());
        assert!(system_inbound_rx.try_recv().is_err());
        assert!(*pending.cleared.lock().unwrap_or_else(|e| e.into_inner()));
    }

    #[test]
    fn bootstrap_pending_retry_keeps_c2c_retry_inside_passive_window() {
        let mut recent = beetle::PcMsg::new_inbound(
            beetle::CHANNEL_QQ_CHANNEL,
            "c2c:chat-1",
            "retry soon",
            false,
        )
        .expect("qq msg");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        recent.enqueue_ts_ms = now_ms.saturating_sub(10 * 60 * 1000);
        let pending = TestPendingRetryStore {
            loaded: Mutex::new(Some(recent)),
            cleared: Mutex::new(false),
        };
        let (user_inbound_tx, user_inbound_rx, _) = beetle::bus::new_user_inbound_channel(2);
        let (system_inbound_tx, system_inbound_rx, _) = beetle::bus::new_system_inbound_channel(2);

        super::app_runtime_support::bootstrap_pending_retry_into_inbound(
            &pending,
            &user_inbound_tx,
            &system_inbound_tx,
        );

        let user_msg = user_inbound_rx.try_recv().expect("user retry");
        assert_eq!(user_msg.content, "retry soon");
        assert!(system_inbound_rx.try_recv().is_err());
        assert!(*pending.cleared.lock().unwrap_or_else(|e| e.into_inner()));
    }

    #[test]
    fn update_display_loop_cache_syncs_owned_dashboard_fields() {
        use super::{update_display_loop_cache, DisplayLoopCacheUpdate, DisplayLoopState};
        use beetle::{DisplayChannelStatus, DisplayPressureLevel};

        let mut state = DisplayLoopState::default();
        let subtitle = Some("subtitle".to_string());
        let ip = "192.168.4.1".to_string();
        let channels = [
            DisplayChannelStatus {
                name: "qq",
                display_label: "QQ",
                visible: true,
                enabled: true,
                healthy: true,
                runtime_status: DisplayChannelRuntimeStatus::Configured,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "tg",
                display_label: "TG",
                visible: true,
                enabled: false,
                healthy: false,
                runtime_status: DisplayChannelRuntimeStatus::Disabled,
                consecutive_failures: 2,
            },
            DisplayChannelStatus {
                name: "fs",
                display_label: "FS",
                visible: true,
                enabled: false,
                healthy: true,
                runtime_status: DisplayChannelRuntimeStatus::Disabled,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "dt",
                display_label: "DT",
                visible: true,
                enabled: false,
                healthy: true,
                runtime_status: DisplayChannelRuntimeStatus::Disabled,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "wc",
                display_label: "WC",
                visible: true,
                enabled: false,
                healthy: true,
                runtime_status: DisplayChannelRuntimeStatus::Disabled,
                consecutive_failures: 0,
            },
        ];

        update_display_loop_cache(
            &mut state,
            DisplayLoopCacheUpdate {
                presence_subtitle: Some(&subtitle),
                ip: Some(ip.as_str()),
                channels: &channels,
                pressure: Some(DisplayPressureLevel::Cautious),
                heap_percent: Some(42),
                msg_in: Some(7),
                msg_out: Some(9),
                llm_ms: Some(88),
            },
        );

        assert_eq!(state.last_presence_subtitle, subtitle);
        assert_eq!(state.last_ip, ip);
        assert_eq!(
            state.last_channels[0],
            (true, true, DisplayChannelRuntimeStatus::Configured, 0)
        );
        assert_eq!(
            state.last_channels[1],
            (false, false, DisplayChannelRuntimeStatus::Disabled, 2)
        );
        assert_eq!(state.last_pressure, Some(DisplayPressureLevel::Cautious));
        assert_eq!(state.last_heap, 42);
        assert_eq!(state.last_msg_in, 7);
        assert_eq!(state.last_msg_out, 9);
        assert_eq!(state.last_llm_ms, 88);
    }

    #[test]
    fn update_display_loop_cache_can_leave_header_fields_untouched() {
        use super::{update_display_loop_cache, DisplayLoopCacheUpdate, DisplayLoopState};
        use beetle::{DisplayChannelStatus, DisplayPressureLevel};

        let mut state = DisplayLoopState {
            last_presence_subtitle: Some("old-subtitle".to_string()),
            last_ip: "192.168.4.1".to_string(),
            ..DisplayLoopState::default()
        };
        let channels = [
            DisplayChannelStatus::hidden(),
            DisplayChannelStatus::hidden(),
            DisplayChannelStatus::hidden(),
            DisplayChannelStatus::hidden(),
            DisplayChannelStatus::hidden(),
        ];

        update_display_loop_cache(
            &mut state,
            DisplayLoopCacheUpdate {
                presence_subtitle: None,
                ip: None,
                channels: &channels,
                pressure: Some(DisplayPressureLevel::Normal),
                heap_percent: Some(11),
                msg_in: Some(2),
                msg_out: Some(3),
                llm_ms: Some(4),
            },
        );

        assert_eq!(
            state.last_presence_subtitle.as_deref(),
            Some("old-subtitle")
        );
        assert_eq!(state.last_ip, "192.168.4.1");
        assert_eq!(state.last_pressure, Some(DisplayPressureLevel::Normal));
        assert_eq!(state.last_heap, 11);
        assert_eq!(state.last_msg_in, 2);
        assert_eq!(state.last_msg_out, 3);
        assert_eq!(state.last_llm_ms, 4);
    }

    #[cfg(feature = "qq_channel")]
    #[test]
    fn build_display_channels_marks_enabled_qq_waiting_until_wss_online() {
        let state = beetle::orchestrator::state::OrchestratorState::new();
        let snapshot = beetle::orchestrator::ResourceSnapshot::from_state(&state);
        let channels = super::build_display_channels(beetle::CHANNEL_QQ_CHANNEL, &snapshot);
        let qq = channels
            .iter()
            .find(|channel| channel.name == beetle::CHANNEL_QQ_CHANNEL)
            .expect("qq channel display status");

        assert!(qq.enabled);
        assert!(qq.healthy);
        assert_eq!(
            qq.runtime_status,
            beetle::DisplayChannelRuntimeStatus::Waiting
        );
    }

    #[test]
    fn channel_runtime_status_distinguishes_wss_runtime_modes() {
        use beetle::runtime::plane_lifecycle::PlaneLifecycleRecord;
        use beetle::runtime::{PlaneId, PlaneLifecycleState};

        let record = |state, reason| PlaneLifecycleRecord {
            plane: PlaneId::ChannelWss,
            owner: "qq_ws",
            state,
            updated_at_ms: 0,
            generation: 1,
            transition_count: 1,
            failure_count: 0,
            last_reason: reason,
        };

        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(
                true,
                true,
                0,
                false,
                Some(record(
                    PlaneLifecycleState::Suspended,
                    "wall_clock_untrusted"
                )),
            ),
            beetle::DisplayChannelRuntimeStatus::WaitingWallClock
        );
        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(
                true,
                true,
                0,
                false,
                Some(record(
                    PlaneLifecycleState::Suspended,
                    "voice_exclusive_suspend"
                )),
            ),
            beetle::DisplayChannelRuntimeStatus::Suspended
        );
        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(
                true,
                true,
                0,
                false,
                Some(record(PlaneLifecycleState::Suspended, "wifi_not_ready")),
            ),
            beetle::DisplayChannelRuntimeStatus::Waiting
        );
        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(
                true,
                true,
                0,
                false,
                Some(record(
                    PlaneLifecycleState::Suspended,
                    "wifi_not_configured"
                )),
            ),
            beetle::DisplayChannelRuntimeStatus::Waiting
        );
        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(
                true,
                true,
                0,
                false,
                Some(record(PlaneLifecycleState::Starting, "connect_attempt")),
            ),
            beetle::DisplayChannelRuntimeStatus::Connecting
        );
        assert_eq!(
            super::display_channel_runtime_status_from_lifecycle(true, true, 0, true, None),
            beetle::DisplayChannelRuntimeStatus::Online
        );
    }

    #[test]
    fn invalidate_display_cache_after_backlight_wake_resets_all_dashboard_cache_fields() {
        use super::{invalidate_display_cache_after_backlight_wake, DisplayLoopState};
        use beetle::{DisplayPressureLevel, DisplaySystemState};

        let mut state = DisplayLoopState {
            last_state: Some(DisplaySystemState::Busy),
            last_presence_subtitle: Some("busy".to_string()),
            last_ip: "192.168.4.1".to_string(),
            last_channels: [(true, false, DisplayChannelRuntimeStatus::Failed, 3);
                DISPLAY_CHANNEL_CAPACITY],
            last_pressure: Some(DisplayPressureLevel::Critical),
            last_heap: 77,
            last_msg_in: 11,
            last_msg_out: 12,
            last_llm_ms: 345,
            ..DisplayLoopState::default()
        };

        invalidate_display_cache_after_backlight_wake(&mut state);

        assert_eq!(state.last_state, None);
        assert_eq!(state.last_presence_subtitle, None);
        assert!(state.last_ip.is_empty());
        assert_eq!(
            state.last_channels,
            [(false, false, DisplayChannelRuntimeStatus::Disabled, 0,); DISPLAY_CHANNEL_CAPACITY]
        );
        assert_eq!(state.last_pressure, None);
        assert_eq!(state.last_heap, 255);
        assert_eq!(state.last_msg_in, u32::MAX);
        assert_eq!(state.last_msg_out, u32::MAX);
        assert_eq!(state.last_llm_ms, 0);
    }

    #[test]
    fn display_backlight_wake_keeps_current_refresh_cycle_renderable() {
        let platform: Arc<dyn Platform> = Arc::new(LinuxPlatform::new());
        let mut state = DisplayLoopState {
            backlight_off: true,
            last_state: Some(DisplaySystemState::Busy),
            last_presence_subtitle: Some("old".to_string()),
            last_ip: "192.168.4.1".to_string(),
            ..DisplayLoopState::default()
        };

        let should_skip_render = super::update_display_backlight(
            &platform,
            &mut state,
            true,
            Duration::from_secs(30),
            true,
        );

        assert!(
            !should_skip_render,
            "wake-triggering changes must repaint in the same refresh cycle"
        );
        assert!(!state.backlight_off);
        assert_eq!(state.last_state, None);
        assert_eq!(state.last_presence_subtitle, None);
        assert!(state.last_ip.is_empty());
    }

    #[test]
    fn esp_display_refresh_yields_under_tls_fragmentation_pressure() {
        use beetle::memory::MemorySystemKind;
        use beetle::orchestrator::{PressureLevel, TlsFragmentationRisk};

        assert!(super::should_suppress_display_refresh(
            MemorySystemKind::EspCompact,
            PressureLevel::Normal,
            TlsFragmentationRisk::Critical,
            (beetle::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32).saturating_sub(1),
        ));
        assert!(super::should_suppress_display_refresh(
            MemorySystemKind::EspCompact,
            PressureLevel::Critical,
            TlsFragmentationRisk::Healthy,
            beetle::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32,
        ));
        assert!(!super::should_suppress_display_refresh(
            MemorySystemKind::LinuxFull,
            PressureLevel::Critical,
            TlsFragmentationRisk::Critical,
            0,
        ));
    }

    #[test]
    fn display_refresh_recovery_invalidates_dashboard_cache_once() {
        use beetle::{DisplayPressureLevel, DisplaySystemState};

        let mut state = DisplayLoopState {
            display_refresh_suppressed: true,
            last_state: Some(DisplaySystemState::Busy),
            last_presence_subtitle: Some("busy".to_string()),
            last_ip: "192.168.4.1".to_string(),
            last_channels: [(true, false, DisplayChannelRuntimeStatus::Failed, 2);
                DISPLAY_CHANNEL_CAPACITY],
            last_pressure: Some(DisplayPressureLevel::Critical),
            last_heap: 91,
            last_msg_in: 5,
            last_msg_out: 7,
            last_llm_ms: 1234,
            ..DisplayLoopState::default()
        };

        assert!(super::resume_display_refresh_after_suppression(&mut state));
        assert!(!state.display_refresh_suppressed);
        assert_eq!(state.last_state, None);
        assert_eq!(state.last_presence_subtitle, None);
        assert!(state.last_ip.is_empty());
        assert_eq!(
            state.last_channels,
            [(false, false, DisplayChannelRuntimeStatus::Disabled, 0,); DISPLAY_CHANNEL_CAPACITY]
        );
        assert_eq!(state.last_pressure, None);
        assert_eq!(state.last_heap, 255);
        assert_eq!(state.last_msg_in, u32::MAX);
        assert_eq!(state.last_msg_out, u32::MAX);
        assert_eq!(state.last_llm_ms, 0);

        assert!(!super::resume_display_refresh_after_suppression(&mut state));
    }

    #[test]
    fn display_thread_stack_budget_is_large_enough_for_dashboard_render_path() {
        let stack_budget = std::hint::black_box(beetle::util::STACK_DISPLAY);
        assert!(
            stack_budget >= 8 * 1024,
            "display stack budget regressed below the current 8KB floor",
        );
    }

    #[test]
    fn voice_session_scheduler_stack_budget_stays_shallow_after_realtime_offload() {
        let stack_budget = std::hint::black_box(beetle::util::STACK_VOICE_CONTROL);
        assert!(
            stack_budget >= 8 * 1024,
            "voice_session scheduler stack regressed below the current 8KB floor",
        );
    }

    #[test]
    fn voice_realtime_worker_stack_budget_is_large_enough_for_realtime_wss_path() {
        let min_stack = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            16 * 1024
        } else {
            64 * 1024
        };
        assert!(
            beetle::util::STACK_VOICE_REALTIME >= min_stack,
            "voice_realtime stack budget regressed below the current realtime floor",
        );
    }

    #[test]
    fn channel_ws_stack_budget_matches_platform_contract() {
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        {
            assert!(
                beetle::util::STACK_CHANNEL_WS == 9 * 1024,
                "ESP external WSS stack budget should stay at the validated 9KB steady-channel budget"
            );
            assert!(
                beetle::util::STACK_VOICE_REALTIME_CONNECT == 12 * 1024,
                "ESP realtime voice WSS connect keeps a separate transient 12KB budget"
            );
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            assert_eq!(
                beetle::util::STACK_CHANNEL_WS,
                beetle::util::STACK_AGENT_LOOP,
                "Linux WSS threads should stay on the shared host TLS stack budget",
            );
            assert_eq!(
                beetle::util::STACK_CHANNEL_WS,
                beetle::util::STACK_CHANNEL_SENDER,
                "Linux WSS and sender threads should follow the same host TLS stack budget",
            );
        }
    }

    #[test]
    fn display_refresh_plan_keeps_footer_and_channels_updates_when_state_header_changes() {
        use beetle::DisplaySystemState;

        let deltas = super::DisplayRefreshDeltas {
            state_changed: true,
            subtitle_changed: false,
            ip_changed: false,
            channels_changed: true,
            footer_changed: true,
        };

        assert_eq!(
            super::plan_display_refresh(Some(DisplaySystemState::Idle), deltas),
            super::DisplayRefreshPlan {
                header: Some(super::StateChangeDisplayRefreshMode::StateHeaderOnly),
                ip: false,
                channels: true,
                footer: true,
            }
        );
    }

    #[test]
    fn display_refresh_plan_uses_full_dashboard_only_for_first_render() {
        let deltas = super::DisplayRefreshDeltas {
            state_changed: true,
            subtitle_changed: true,
            ip_changed: true,
            channels_changed: true,
            footer_changed: true,
        };

        assert_eq!(
            super::plan_display_refresh(None, deltas),
            super::DisplayRefreshPlan {
                header: Some(super::StateChangeDisplayRefreshMode::FullDashboard),
                ip: false,
                channels: false,
                footer: false,
            }
        );
    }

    #[test]
    fn display_heavy_refresh_degrade_keeps_status_surface_plan() {
        let deltas = super::DisplayRefreshDeltas {
            state_changed: true,
            subtitle_changed: true,
            ip_changed: true,
            channels_changed: true,
            footer_changed: true,
        };
        let heavy_plan = super::plan_display_refresh(None, deltas);

        assert_eq!(
            super::degrade_display_heavy_refresh_plan(heavy_plan, deltas),
            super::DisplayRefreshPlan {
                header: Some(super::StateChangeDisplayRefreshMode::StateHeaderOnly),
                ip: false,
                channels: false,
                footer: true,
            },
            "heavy refresh degrade must keep status/protect updates without full dashboard or channel redraw"
        );
    }

    #[test]
    fn display_error_flash_emits_flash_on_then_flash_off_transition() {
        let mut state = super::DisplayLoopState::default();
        let mut metrics = beetle::metrics::snapshot();

        metrics.errors_other = 1;
        assert_eq!(
            super::update_display_error_flash(&mut state, &metrics),
            super::DisplayErrorFlashUpdate::FlashOn
        );

        assert_eq!(
            super::update_display_error_flash(&mut state, &metrics),
            super::DisplayErrorFlashUpdate::FlashOff
        );

        assert_eq!(
            super::update_display_error_flash(&mut state, &metrics),
            super::DisplayErrorFlashUpdate::NoChange
        );
    }
}

fn build_voice_event_channel(
    platform: &Arc<dyn Platform>,
    config: &Arc<AppConfig>,
    baidu_token_cache: Option<&Arc<beetle::audio::baidu_token::BaiduTokenCache>>,
) -> Option<VoiceEventChannel> {
    let audio_cfg = config.audio.as_ref()?;
    if !audio_cfg.enabled {
        return None;
    }

    let duplex_caps = platform.audio_duplex_capabilities();
    let capabilities =
        compute_voice_runtime_capabilities(audio_cfg, duplex_caps, baidu_token_cache.is_some());

    if !capabilities.speak_capable && !capabilities.wake_capable {
        return None;
    }

    let (vtx, vrx) = std::sync::mpsc::sync_channel(beetle::constants::VOICE_EVENT_QUEUE_CAPACITY);
    Some(VoiceEventChannel {
        speak_capable: capabilities.speak_capable,
        tx: vtx,
        rx: vrx,
    })
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone)]
struct DisplayLoopState {
    last_state: Option<DisplaySystemState>,
    last_presence_subtitle: Option<String>,
    last_ip: String,
    last_channels: [(bool, bool, DisplayChannelRuntimeStatus, u32); DISPLAY_CHANNEL_CAPACITY],
    last_pressure: Option<DisplayPressureLevel>,
    last_heap: u8,
    last_msg_in: u32,
    last_msg_out: u32,
    last_llm_ms: u32,
    refresh_secs: u64,
    busy_toggle: bool,
    last_error_total: u64,
    flash_active: bool,
    last_activity_at: Instant,
    backlight_off: bool,
    display_refresh_suppressed: bool,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateChangeDisplayRefreshMode {
    FullDashboard,
    StateHeaderOnly,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
impl Default for DisplayLoopState {
    fn default() -> Self {
        Self {
            last_state: None,
            last_presence_subtitle: None,
            last_ip: String::new(),
            last_channels: [(false, false, DisplayChannelRuntimeStatus::Disabled, 0);
                DISPLAY_CHANNEL_CAPACITY],
            last_pressure: None,
            last_heap: 255,
            last_msg_in: u32::MAX,
            last_msg_out: u32::MAX,
            last_llm_ms: 0,
            refresh_secs: beetle::constants::DISPLAY_REFRESH_IDLE_SECS,
            busy_toggle: false,
            last_error_total: 0,
            flash_active: false,
            last_activity_at: Instant::now(),
            backlight_off: false,
            display_refresh_suppressed: false,
        }
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
fn state_change_display_refresh_mode(
    last_state: Option<beetle::DisplaySystemState>,
) -> StateChangeDisplayRefreshMode {
    if last_state.is_none() {
        StateChangeDisplayRefreshMode::FullDashboard
    } else {
        StateChangeDisplayRefreshMode::StateHeaderOnly
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayRefreshDeltas {
    state_changed: bool,
    subtitle_changed: bool,
    ip_changed: bool,
    channels_changed: bool,
    footer_changed: bool,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DisplayRefreshPlan {
    header: Option<StateChangeDisplayRefreshMode>,
    ip: bool,
    channels: bool,
    footer: bool,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn plan_display_refresh(
    last_state: Option<DisplaySystemState>,
    deltas: DisplayRefreshDeltas,
) -> DisplayRefreshPlan {
    let header = deltas
        .state_changed
        .then(|| state_change_display_refresh_mode(last_state));
    if header == Some(StateChangeDisplayRefreshMode::FullDashboard) {
        return DisplayRefreshPlan {
            header,
            ip: false,
            channels: false,
            footer: false,
        };
    }
    DisplayRefreshPlan {
        header,
        ip: header.is_none() && (deltas.ip_changed || deltas.subtitle_changed),
        channels: deltas.channels_changed,
        footer: deltas.footer_changed,
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn degrade_display_heavy_refresh_plan(
    plan: DisplayRefreshPlan,
    deltas: DisplayRefreshDeltas,
) -> DisplayRefreshPlan {
    let header = if plan.header.is_some() || deltas.state_changed || deltas.subtitle_changed {
        Some(StateChangeDisplayRefreshMode::StateHeaderOnly)
    } else {
        None
    };
    DisplayRefreshPlan {
        header,
        ip: header.is_none() && plan.ip,
        channels: false,
        footer: plan.footer || deltas.footer_changed,
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DisplayErrorFlashUpdate {
    NoChange,
    FlashOn,
    FlashOff,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn invalidate_display_cache_after_backlight_wake(loop_state: &mut DisplayLoopState) {
    loop_state.last_state = None;
    loop_state.last_presence_subtitle = None;
    loop_state.last_ip.clear();
    loop_state.last_channels =
        [(false, false, DisplayChannelRuntimeStatus::Disabled, 0); DISPLAY_CHANNEL_CAPACITY];
    loop_state.last_pressure = None;
    loop_state.last_heap = 255;
    loop_state.last_msg_in = u32::MAX;
    loop_state.last_msg_out = u32::MAX;
    loop_state.last_llm_ms = 0;
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn should_suppress_display_refresh(
    memory_system_kind: beetle::memory::MemorySystemKind,
    pressure: beetle::orchestrator::PressureLevel,
    tls_fragmentation_risk: beetle::orchestrator::TlsFragmentationRisk,
    heap_largest_block_internal: u32,
) -> bool {
    if memory_system_kind != beetle::memory::MemorySystemKind::EspCompact {
        return false;
    }
    pressure == beetle::orchestrator::PressureLevel::Critical
        || tls_fragmentation_risk == beetle::orchestrator::TlsFragmentationRisk::Critical
        || (heap_largest_block_internal > 0
            && heap_largest_block_internal
                < beetle::constants::TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32)
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_heavy_refresh_scheduler_degraded(pressure: beetle::orchestrator::PressureLevel) -> bool {
    !matches!(
        beetle::runtime::admit_current_runtime_work(
            beetle::runtime::RuntimeWorkClass::DisplayHeavyRefresh,
            beetle::runtime::RuntimeWorkSource::Background,
            beetle::runtime::default_runtime_scheduler_profile(),
            pressure,
        ),
        beetle::runtime::RuntimeWorkDecision::Proceed
    )
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_status_surface_scheduler_allowed(pressure: beetle::orchestrator::PressureLevel) -> bool {
    matches!(
        beetle::runtime::admit_current_runtime_work(
            beetle::runtime::RuntimeWorkClass::DisplayStatusSurface,
            beetle::runtime::RuntimeWorkSource::System,
            beetle::runtime::default_runtime_scheduler_profile(),
            pressure,
        ),
        beetle::runtime::RuntimeWorkDecision::Proceed
    )
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn resume_display_refresh_after_suppression(loop_state: &mut DisplayLoopState) -> bool {
    if !loop_state.display_refresh_suppressed {
        return false;
    }
    loop_state.display_refresh_suppressed = false;
    invalidate_display_cache_after_backlight_wake(loop_state);
    true
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
struct DisplayLoopCacheUpdate<'a> {
    presence_subtitle: Option<&'a Option<String>>,
    ip: Option<&'a str>,
    channels: &'a [DisplayChannelStatus; DISPLAY_CHANNEL_CAPACITY],
    pressure: Option<DisplayPressureLevel>,
    heap_percent: Option<u8>,
    msg_in: Option<u32>,
    msg_out: Option<u32>,
    llm_ms: Option<u32>,
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn update_display_loop_cache(
    loop_state: &mut DisplayLoopState,
    update: DisplayLoopCacheUpdate<'_>,
) {
    let DisplayLoopCacheUpdate {
        presence_subtitle,
        ip,
        channels,
        pressure,
        heap_percent,
        msg_in,
        msg_out,
        llm_ms,
    } = update;
    if let Some(presence_subtitle) = presence_subtitle {
        loop_state
            .last_presence_subtitle
            .clone_from(presence_subtitle);
    }
    if let Some(ip) = ip {
        loop_state.last_ip.clear();
        loop_state.last_ip.push_str(ip);
    }
    for (i, ch) in channels.iter().enumerate() {
        loop_state.last_channels[i] = (
            ch.enabled,
            ch.healthy,
            ch.runtime_status,
            ch.consecutive_failures,
        );
    }
    if let Some(pressure) = pressure {
        loop_state.last_pressure = Some(pressure);
    }
    if let Some(heap_percent) = heap_percent {
        loop_state.last_heap = heap_percent;
    }
    if let Some(msg_in) = msg_in {
        loop_state.last_msg_in = msg_in;
    }
    if let Some(msg_out) = msg_out {
        loop_state.last_msg_out = msg_out;
    }
    if let Some(llm_ms) = llm_ms {
        loop_state.last_llm_ms = llm_ms;
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn build_display_channels(
    enabled: &str,
    snapshot: &beetle::orchestrator::ResourceSnapshot,
) -> [DisplayChannelStatus; DISPLAY_CHANNEL_CAPACITY] {
    let mut channels = [DisplayChannelStatus::hidden(); DISPLAY_CHANNEL_CAPACITY];
    let normalized_enabled = beetle::normalize_compiled_enabled_channel(enabled);
    for (index, entry) in beetle::display_channel_entries().enumerate() {
        let (healthy, consecutive_failures) = snapshot
            .channels
            .get(entry.id)
            .map(|channel| (channel.healthy, channel.consecutive_failures))
            .unwrap_or((false, 0));
        let enabled = normalized_enabled == entry.id;
        let runtime_status =
            display_channel_runtime_status(entry.id, enabled, healthy, consecutive_failures);
        channels[index] = DisplayChannelStatus {
            name: entry.id,
            display_label: entry.display_label,
            visible: true,
            enabled,
            healthy,
            runtime_status,
            consecutive_failures,
        };
    }
    channels
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_channel_runtime_status(
    channel: &str,
    enabled: bool,
    healthy: bool,
    consecutive_failures: u32,
) -> DisplayChannelRuntimeStatus {
    let wss_lifecycle = display_channel_wss_lifecycle(channel);
    let ws_online = display_channel_ws_online(channel);
    let status = display_channel_runtime_status_from_lifecycle(
        enabled,
        healthy,
        consecutive_failures,
        ws_online,
        wss_lifecycle,
    );
    if status == DisplayChannelRuntimeStatus::Configured
        && enabled
        && healthy
        && consecutive_failures == 0
        && display_channel_wss_lifecycle_owner(channel).is_some()
    {
        DisplayChannelRuntimeStatus::Waiting
    } else {
        status
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_channel_runtime_status_from_lifecycle(
    enabled: bool,
    healthy: bool,
    consecutive_failures: u32,
    ws_online: bool,
    lifecycle: Option<beetle::runtime::plane_lifecycle::PlaneLifecycleRecord>,
) -> DisplayChannelRuntimeStatus {
    if !enabled {
        return DisplayChannelRuntimeStatus::Disabled;
    }
    if consecutive_failures > 0 {
        return DisplayChannelRuntimeStatus::CoolingDown;
    }
    if !healthy {
        return DisplayChannelRuntimeStatus::Failed;
    }
    if ws_online {
        return DisplayChannelRuntimeStatus::Online;
    }
    if let Some(record) = lifecycle {
        return match record.state {
            beetle::runtime::PlaneLifecycleState::Starting => {
                DisplayChannelRuntimeStatus::Connecting
            }
            beetle::runtime::PlaneLifecycleState::Active => DisplayChannelRuntimeStatus::Waiting,
            beetle::runtime::PlaneLifecycleState::Suspended => {
                if record.last_reason == "wall_clock_untrusted" {
                    DisplayChannelRuntimeStatus::WaitingWallClock
                } else if record.last_reason == "wifi_not_ready"
                    || record.last_reason == "wifi_not_configured"
                {
                    DisplayChannelRuntimeStatus::Waiting
                } else {
                    DisplayChannelRuntimeStatus::Suspended
                }
            }
            beetle::runtime::PlaneLifecycleState::Draining
            | beetle::runtime::PlaneLifecycleState::Stopping => {
                DisplayChannelRuntimeStatus::Suspended
            }
            beetle::runtime::PlaneLifecycleState::Failed => DisplayChannelRuntimeStatus::Failed,
            beetle::runtime::PlaneLifecycleState::Registered
            | beetle::runtime::PlaneLifecycleState::Disabled
            | beetle::runtime::PlaneLifecycleState::Unloaded => {
                DisplayChannelRuntimeStatus::Configured
            }
        };
    }
    DisplayChannelRuntimeStatus::Configured
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_channel_ws_online(channel: &str) -> bool {
    #[cfg(feature = "qq_channel")]
    if channel == beetle::CHANNEL_QQ_CHANNEL {
        return beetle::channels::is_ws_online();
    }
    let _ = channel;
    false
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_channel_wss_lifecycle(
    channel: &str,
) -> Option<beetle::runtime::plane_lifecycle::PlaneLifecycleRecord> {
    let owner = display_channel_wss_lifecycle_owner(channel)?;
    beetle::runtime::plane_lifecycle::snapshot()
        .records
        .into_iter()
        .find(|record| {
            record.plane == beetle::runtime::PlaneId::ChannelWss && record.owner == owner
        })
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_channel_wss_lifecycle_owner(channel: &str) -> Option<&'static str> {
    #[cfg(feature = "qq_channel")]
    if channel == beetle::CHANNEL_QQ_CHANNEL {
        return Some("qq_ws");
    }
    #[cfg(feature = "feishu")]
    if channel == beetle::CHANNEL_FEISHU {
        return Some("feishu_ws");
    }
    #[cfg(feature = "dingtalk")]
    if channel == beetle::CHANNEL_DINGTALK {
        return Some("dingtalk_stream");
    }
    #[cfg(feature = "wecom")]
    if channel == beetle::CHANNEL_WECOM {
        return Some("wecom_aibot");
    }
    let _ = channel;
    None
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn update_display_error_flash(
    loop_state: &mut DisplayLoopState,
    metrics: &beetle::metrics::MetricsSnapshot,
) -> DisplayErrorFlashUpdate {
    let current_error_total = metrics.errors_agent_chat
        + metrics.errors_agent_context
        + metrics.errors_tool_execute
        + metrics.errors_llm_request
        + metrics.errors_llm_parse
        + metrics.errors_channel_dispatch
        + metrics.errors_session_append
        + metrics.errors_tls_admission
        + metrics.errors_other;
    let error_flash = if current_error_total > loop_state.last_error_total {
        loop_state.last_error_total = current_error_total;
        true
    } else {
        loop_state.last_error_total = current_error_total;
        false
    };
    if error_flash {
        loop_state.flash_active = true;
        DisplayErrorFlashUpdate::FlashOn
    } else if loop_state.flash_active {
        loop_state.flash_active = false;
        DisplayErrorFlashUpdate::FlashOff
    } else {
        DisplayErrorFlashUpdate::NoChange
    }
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn display_command_with_owner(
    platform: &Arc<dyn Platform>,
    owner: DisplayOwner,
    cmd: DisplayCommand,
) -> Option<beetle::Result<()>> {
    beetle::display::with_display_lease(owner, || platform.display_command(cmd))
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
#[cfg_attr(test, allow(dead_code))]
fn mark_display_lifecycle(state: beetle::runtime::PlaneLifecycleState, reason: &'static str) {
    beetle::runtime::plane_lifecycle::mark(
        beetle::runtime::PlaneId::Display,
        DISPLAY_LIFECYCLE_OWNER,
        state,
        reason,
    );
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
fn fade_display_backlight_with_owner(
    platform: &Arc<dyn Platform>,
    owner: DisplayOwner,
    from: u8,
    to: u8,
    duration_ms: u32,
) -> Option<beetle::Result<()>> {
    beetle::display::with_display_lease(owner, || {
        platform.fade_display_backlight(from, to, duration_ms)
    })
}

#[cfg(any(
    test,
    target_arch = "xtensa",
    target_arch = "riscv32",
    target_os = "linux"
))]
fn update_display_backlight(
    platform: &Arc<dyn Platform>,
    loop_state: &mut DisplayLoopState,
    sleep_enabled: bool,
    sleep_duration: Duration,
    any_change: bool,
) -> bool {
    if !sleep_enabled {
        return false;
    }
    if any_change && loop_state.backlight_off {
        if fade_display_backlight_with_owner(platform, DisplayOwner::DefaultDashboard, 0, 100, 500)
            .is_some()
        {
            loop_state.backlight_off = false;
            invalidate_display_cache_after_backlight_wake(loop_state);
            log::info!("[{}] display backlight woke up", TAG);
            return false;
        }
        return true;
    }
    if !loop_state.backlight_off
        && !any_change
        && loop_state.last_activity_at.elapsed() >= sleep_duration
    {
        if fade_display_backlight_with_owner(platform, DisplayOwner::DefaultDashboard, 100, 0, 500)
            .is_some()
        {
            loop_state.backlight_off = true;
            log::info!("[{}] display backlight auto-sleep", TAG);
        }
        return true;
    }
    loop_state.backlight_off && !any_change
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn run_display_loop(platform: Arc<dyn Platform>, config: Arc<AppConfig>) {
    mark_display_lifecycle(
        beetle::runtime::PlaneLifecycleState::Active,
        "display_loop_started",
    );
    let enabled = config.enabled_channel.as_str();
    let sleep_timeout = config
        .display
        .as_ref()
        .map(|d| d.sleep_timeout_secs)
        .unwrap_or(0);
    let sleep_enabled = sleep_timeout > 0 && platform.display_backlight_available();
    let sleep_duration = Duration::from_secs(sleep_timeout as u64);
    let mut loop_state = DisplayLoopState::default();

    loop {
        beetle::platform::task_wdt::feed_current_task();
        std::thread::sleep(Duration::from_secs(loop_state.refresh_secs));
        beetle::platform::task_wdt::feed_current_task();
        let snapshot = beetle::orchestrator::snapshot();
        if !display_status_surface_scheduler_allowed(snapshot.pressure) {
            log::warn!(
                "[{}] display status surface deferred by runtime scheduler: pressure={:?}",
                TAG,
                snapshot.pressure
            );
            loop_state.refresh_secs = beetle::constants::DISPLAY_REFRESH_IDLE_SECS;
            continue;
        }
        let resource_degrades_heavy_refresh = should_suppress_display_refresh(
            platform.memory_system_kind(),
            snapshot.pressure,
            snapshot.tls_fragmentation_risk,
            snapshot.heap_largest_block_internal,
        );
        let scheduler_degrades_heavy_refresh =
            display_heavy_refresh_scheduler_degraded(snapshot.pressure);
        let heavy_refresh_degraded =
            resource_degrades_heavy_refresh || scheduler_degrades_heavy_refresh;
        if heavy_refresh_degraded {
            if !loop_state.display_refresh_suppressed {
                log::warn!(
                    "[{}] display heavy refresh degraded: resource_degraded={} scheduler_degraded={} pressure={:?} tls_fragmentation={:?} largest_block={}",
                    TAG,
                    resource_degrades_heavy_refresh,
                    scheduler_degrades_heavy_refresh,
                    snapshot.pressure,
                    snapshot.tls_fragmentation_risk,
                    snapshot.heap_largest_block_internal
                );
            }
            loop_state.display_refresh_suppressed = true;
            loop_state.refresh_secs = beetle::constants::DISPLAY_REFRESH_IDLE_SECS;
        }
        if !heavy_refresh_degraded && resume_display_refresh_after_suppression(&mut loop_state) {
            log::info!("[{}] display refresh resumed after resource pressure", TAG);
        }
        let now_secs = beetle::util::current_unix_secs();
        let pressure = match snapshot.pressure {
            beetle::orchestrator::PressureLevel::Normal => DisplayPressureLevel::Normal,
            beetle::orchestrator::PressureLevel::Cautious => DisplayPressureLevel::Cautious,
            beetle::orchestrator::PressureLevel::Critical => DisplayPressureLevel::Critical,
        };
        let ip = platform.wifi_sta_ip();
        let ip_hint = ip.as_deref().unwrap_or(SOFTAP_DEFAULT_IPV4);
        let display_projection = beetle::runtime::inspect_platform_display_projection_with_resource(
            platform.as_ref(),
            &snapshot,
            now_secs,
            Some(ip_hint),
        );
        let state = display_projection.state;
        let channels = build_display_channels(enabled, &snapshot);
        let heap_percent = heap_used_percent(&snapshot);

        let metrics = beetle::metrics::snapshot();
        let msg_in = metrics.messages_in as u32;
        let msg_out = metrics.messages_out as u32;
        let last_active = metrics.last_active_epoch_secs as u32;
        let llm_ms = metrics.llm_last_ms as u32;
        let uptime_secs = beetle::platform::time::app_uptime_secs();

        loop_state.busy_toggle = state == DisplaySystemState::Busy && !loop_state.busy_toggle;
        if state != DisplaySystemState::Busy {
            loop_state.busy_toggle = false;
        }

        let flash_update = update_display_error_flash(&mut loop_state, &metrics);
        let show_flash = flash_update == DisplayErrorFlashUpdate::FlashOn;
        let state_changed = loop_state.last_state != Some(state);
        let subtitle_changed =
            loop_state.last_presence_subtitle != display_projection.subtitle_override;
        let ip_changed = loop_state.last_ip.as_str() != ip_hint;
        let channels_changed = channels.iter().enumerate().any(|(i, ch)| {
            loop_state.last_channels[i]
                != (
                    ch.enabled,
                    ch.healthy,
                    ch.runtime_status,
                    ch.consecutive_failures,
                )
        });
        let pressure_changed = loop_state.last_pressure.as_ref() != Some(&pressure);
        let heap_changed = loop_state.last_heap.abs_diff(heap_percent) >= 2;
        let msg_changed = msg_in != loop_state.last_msg_in || msg_out != loop_state.last_msg_out;
        let llm_changed = llm_ms != loop_state.last_llm_ms;
        let footer_changed = pressure_changed
            || heap_changed
            || msg_changed
            || llm_changed
            || flash_update != DisplayErrorFlashUpdate::NoChange;
        let any_change = state_changed
            || ip_changed
            || channels_changed
            || footer_changed
            || subtitle_changed
            || flash_update != DisplayErrorFlashUpdate::NoChange;

        if any_change {
            loop_state.last_activity_at = Instant::now();
        }

        if update_display_backlight(
            &platform,
            &mut loop_state,
            sleep_enabled,
            sleep_duration,
            any_change,
        ) {
            loop_state.refresh_secs = compute_refresh_secs(
                state,
                loop_state.backlight_off,
                &loop_state.last_activity_at,
            );
            continue;
        }

        let display_deltas = DisplayRefreshDeltas {
            state_changed,
            subtitle_changed,
            ip_changed,
            channels_changed,
            footer_changed,
        };
        let mut refresh_plan = plan_display_refresh(loop_state.last_state, display_deltas);
        if heavy_refresh_degraded {
            refresh_plan = degrade_display_heavy_refresh_plan(refresh_plan, display_deltas);
            log::info!(
                "[{}] display_status_surface retained=true heavy_refresh_degraded=true header={} ip={} footer={}",
                TAG,
                refresh_plan.header.is_some(),
                refresh_plan.ip,
                refresh_plan.footer
            );
        }

        if let Some(header_mode) = refresh_plan.header {
            let presence_subtitle = display_projection.subtitle_override.clone();
            let ip_owned = ip_hint.to_string();
            let cmd = match header_mode {
                StateChangeDisplayRefreshMode::FullDashboard => DisplayCommand::RefreshDashboard {
                    state,
                    presence_subtitle,
                    ip_address: Some(ip_owned.clone()),
                    channels,
                    pressure,
                    heap_percent,
                    messages_in: msg_in,
                    messages_out: msg_out,
                    last_active_epoch_secs: last_active,
                    uptime_secs,
                    busy_phase: loop_state.busy_toggle,
                    llm_last_ms: llm_ms,
                    error_flash: show_flash,
                },
                StateChangeDisplayRefreshMode::StateHeaderOnly => {
                    DisplayCommand::UpdateStateHeader {
                        state,
                        presence_subtitle,
                        ip_address: Some(ip_owned.clone()),
                        uptime_secs,
                        busy_phase: loop_state.busy_toggle,
                    }
                }
            };
            match display_command_with_owner(&platform, DisplayOwner::DefaultDashboard, cmd) {
                Some(Ok(())) => {
                    loop_state.last_state = Some(state);
                    loop_state
                        .last_presence_subtitle
                        .clone_from(&display_projection.subtitle_override);
                    loop_state.last_ip.clone_from(&ip_owned);
                    if header_mode == StateChangeDisplayRefreshMode::FullDashboard {
                        update_display_loop_cache(
                            &mut loop_state,
                            DisplayLoopCacheUpdate {
                                presence_subtitle: Some(&display_projection.subtitle_override),
                                ip: Some(ip_owned.as_str()),
                                channels: &channels,
                                pressure: Some(pressure),
                                heap_percent: Some(heap_percent),
                                msg_in: Some(msg_in),
                                msg_out: Some(msg_out),
                                llm_ms: Some(llm_ms),
                            },
                        );
                        loop_state.refresh_secs = compute_refresh_secs(
                            state,
                            loop_state.backlight_off,
                            &loop_state.last_activity_at,
                        );
                        continue;
                    }
                }
                Some(Err(e)) => {
                    log::warn!("[{}] display refresh failed: {}", TAG, e);
                }
                None => {}
            }
        }

        if refresh_plan.ip {
            let presence_subtitle = display_projection.subtitle_override.clone();
            let ip_owned = ip_hint.to_string();
            match display_command_with_owner(
                &platform,
                DisplayOwner::DefaultDashboard,
                DisplayCommand::UpdateIp {
                    ip: ip_owned.clone(),
                    presence_subtitle,
                    uptime_secs,
                },
            ) {
                Some(Ok(())) => {
                    update_display_loop_cache(
                        &mut loop_state,
                        DisplayLoopCacheUpdate {
                            presence_subtitle: Some(&display_projection.subtitle_override),
                            ip: Some(ip_owned.as_str()),
                            channels: &channels,
                            pressure: None,
                            heap_percent: None,
                            msg_in: None,
                            msg_out: None,
                            llm_ms: None,
                        },
                    );
                }
                Some(Err(e)) => log::warn!("[{}] display ip refresh failed: {}", TAG, e),
                None => {}
            }
        }
        if refresh_plan.channels {
            match display_command_with_owner(
                &platform,
                DisplayOwner::DefaultDashboard,
                DisplayCommand::UpdateChannels { channels },
            ) {
                Some(Ok(())) => {
                    update_display_loop_cache(
                        &mut loop_state,
                        DisplayLoopCacheUpdate {
                            presence_subtitle: None,
                            ip: None,
                            channels: &channels,
                            pressure: None,
                            heap_percent: None,
                            msg_in: None,
                            msg_out: None,
                            llm_ms: None,
                        },
                    );
                }
                Some(Err(e)) => log::warn!("[{}] display channels refresh failed: {}", TAG, e),
                None => {}
            }
        }
        if refresh_plan.footer {
            match display_command_with_owner(
                &platform,
                DisplayOwner::DefaultDashboard,
                DisplayCommand::UpdatePressure {
                    level: pressure,
                    heap_percent,
                    messages_in: msg_in,
                    messages_out: msg_out,
                    last_active_epoch_secs: last_active,
                    llm_last_ms: llm_ms,
                    error_flash: show_flash,
                },
            ) {
                Some(Ok(())) => {
                    update_display_loop_cache(
                        &mut loop_state,
                        DisplayLoopCacheUpdate {
                            presence_subtitle: None,
                            ip: None,
                            channels: &channels,
                            pressure: Some(pressure),
                            heap_percent: Some(heap_percent),
                            msg_in: Some(msg_in),
                            msg_out: Some(msg_out),
                            llm_ms: Some(llm_ms),
                        },
                    );
                }
                Some(Err(e)) => log::warn!("[{}] display footer refresh failed: {}", TAG, e),
                None => {}
            }
        }
        loop_state.refresh_secs = compute_refresh_secs(
            state,
            loop_state.backlight_off,
            &loop_state.last_activity_at,
        );
        beetle::platform::task_wdt::feed_current_task();
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_config_command(platform: &Arc<dyn Platform>, action: beetle::commands::ConfigAction) {
    use beetle::commands::ConfigAction;
    let config_store = platform.config_store();

    match action {
        ConfigAction::Get { key } => match config_store.read_string(&key) {
            Ok(Some(value)) => println!("{}", value),
            Ok(None) => {
                eprintln!("Config key '{}' not found", key);
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Error reading config key '{}': {}", key, e);
                std::process::exit(1);
            }
        },
        ConfigAction::Set { key, value } => match config_store.write_string(&key, &value) {
            Ok(_) => println!("Config '{}' set successfully", key),
            Err(e) => {
                eprintln!("Error writing config key '{}': {}", key, e);
                std::process::exit(1);
            }
        },
        ConfigAction::List => {
            eprintln!("Config list not yet implemented");
            std::process::exit(1);
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_status_command(platform: &Arc<dyn Platform>, json: bool, chat_id: Option<&str>) {
    beetle::platform::refresh_runtime_state();
    let config = beetle::bootstrap::load_config(platform);
    let runtime_services = beetle::RuntimeServices::from_platform(Arc::clone(platform));
    let (tool_registry, _) = beetle::tools::build_default_registry(&config, &runtime_services);
    let operator_status = beetle::platform::operator_status::build_operator_status(
        beetle::platform::operator_status::OperatorStatusInput {
            config: &config,
            platform: platform.as_ref(),
            tool_registry: &tool_registry,
        },
    )
    .unwrap_or_else(|e| {
        eprintln!("Error building operator status: {}", e);
        std::process::exit(1);
    });
    let recent_turn = chat_id.and_then(|id| {
        platform
            .turn_ledger_store()
            .get(id)
            .map_err(|e| {
                eprintln!("Error reading turn ledger for '{}': {}", id, e);
                std::process::exit(1);
            })
            .ok()
            .flatten()
    });

    if json {
        let payload = serde_json::json!({
            "version": VERSION,
            "enabled_channel": config.enabled_channel,
            "chat_id": chat_id,
            "operator_status": operator_status,
            "recent_turn": recent_turn,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("beetle v{}", VERSION);
        println!("Enabled channel: {}", config.enabled_channel);
        print!(
            "{}",
            beetle::platform::operator_status::render_operator_status_text(&operator_status)
        );
        if let Some(id) = chat_id {
            println!("Chat ID: {}", id);
            match recent_turn {
                Some(ledger) => {
                    println!("Recent turn status: {:?}", ledger.status);
                    println!("Recent turn reason: {}", ledger.reason);
                    println!("Recent turn req_id: {}", ledger.req_id);
                    println!("Recent turn ttft_ms: {}", ledger.ttft_ms);
                    println!("Recent turn total_ms: {}", ledger.total_ms);
                    println!("Recent turn tool_calls: {}", ledger.tool_calls);
                    println!(
                        "Recent turn visibility counters: edit_phase_header_updates_sent={} append_only_ack_sent={} append_only_heartbeat_sent={} append_only_first_tool_milestone_sent={} partial_updates_sent={} visible_text_updates_sent={}",
                        ledger.delivery.edit_phase_header_updates_sent,
                        ledger.delivery.append_only_ack_sent,
                        ledger.delivery.append_only_heartbeat_sent,
                        ledger.delivery.append_only_first_tool_milestone_sent,
                        ledger.delivery.partial_updates_sent,
                        ledger.delivery.visible_text_updates_sent
                    );
                    println!(
                        "Recent turn outbound counters: tool_outbound_intents_seen={} tool_visible_updates_sent={} explicit_outbound_sent={} tool_outbound_suppressed={} current_primary_delivered={} finalize_streamed={}",
                        ledger.delivery.tool_outbound_intents_seen,
                        ledger.delivery.tool_visible_updates_sent,
                        ledger.delivery.explicit_outbound_sent,
                        ledger.delivery.tool_outbound_suppressed,
                        ledger.delivery.current_primary_delivered,
                        ledger.delivery.finalize_streamed
                    );
                    if !ledger.user_preview.is_empty() {
                        println!("Recent user preview: {}", ledger.user_preview);
                    }
                    if !ledger.reply_preview.is_empty() {
                        println!("Recent reply preview: {}", ledger.reply_preview);
                    }
                    if let Some(observation) = ledger.observation.as_ref().and_then(|observation| {
                        beetle::memory::render_turn_observation_ledger_block(observation, 320)
                    }) {
                        for line in observation.lines() {
                            println!("{line}");
                        }
                    }
                    if let Some(reasoning_intent) =
                        ledger
                            .reasoning_intent
                            .as_ref()
                            .and_then(|reasoning_intent| {
                                beetle::memory::render_turn_reasoning_intent_ledger_block(
                                    reasoning_intent,
                                    320,
                                )
                            })
                    {
                        for line in reasoning_intent.lines() {
                            println!("{line}");
                        }
                    }
                    if let Some(counterfactual) =
                        ledger.counterfactual.as_ref().and_then(|counterfactual| {
                            beetle::memory::render_turn_counterfactual_ledger_block(
                                counterfactual,
                                320,
                            )
                        })
                    {
                        for line in counterfactual.lines() {
                            println!("{line}");
                        }
                    }
                    if let Some(adversarial_arena) =
                        ledger.adversarial_arena.as_ref().and_then(|arena| {
                            beetle::memory::render_turn_adversarial_arena_ledger_block(arena, 320)
                        })
                    {
                        for line in adversarial_arena.lines() {
                            println!("{line}");
                        }
                    }
                }
                None => println!("Recent turn: none"),
            }
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_doctor_command(platform: &Arc<dyn Platform>) {
    println!("Running beetle diagnostics...\n");

    println!("✓ Platform initialized");

    let config = beetle::bootstrap::load_config(platform);
    if config.wifi_ssid.trim().is_empty() {
        println!("⚠ WiFi configuration not set");
    } else {
        println!("✓ WiFi configuration present");
    }

    println!("✓ Config loaded (channel: {})", config.enabled_channel);

    let presence = beetle::runtime::inspect_platform_presence(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    let initiative = beetle::runtime::inspect_platform_initiative(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    let os_closure = beetle::runtime::inspect_beetle_os_closure(&presence, &initiative);
    println!(
        "✓ Presence resolved (state={} runtime_mode={})",
        presence.state.as_str(),
        presence.runtime_mode.current_mode.as_str()
    );
    println!(
        "✓ Initiative contract action={} ready={} rationale={}",
        initiative.action.as_str(),
        initiative.ready,
        initiative.rationale
    );
    println!(
        "{} Beetle OS closure ready={} planes={}/{} summary={}",
        if os_closure.ready { "✓" } else { "⚠" },
        os_closure.ready,
        os_closure.ready_planes,
        os_closure.plane_count,
        os_closure.summary
    );
    if !os_closure.outstanding.is_empty() {
        println!(
            "⚠ Beetle OS outstanding gates: {}",
            os_closure.outstanding.join(", ")
        );
    }
    if let Some(reason) = initiative.suppression_reason {
        println!("⚠ Initiative suppressed by: {}", reason.as_str());
    }
    println!(
        "✓ Soul kernel ready={} safe_mode_readable={} degraded={} key_memory={}",
        presence.soul_kernel.minimum_viable,
        presence.soul_kernel.safe_mode_minimum_readable,
        presence.soul_kernel.degraded,
        presence.soul_kernel.key_memory_count
    );
    if let Some(release) = presence.release.as_ref() {
        println!(
            "✓ Linux release managed={} rollout_state={} rollback_available={}",
            release.managed,
            release.rollout_state_label(),
            release.rollback_available
        );
    }
    if !presence.soul_kernel.degradation_reasons.is_empty() {
        println!(
            "⚠ Soul kernel degradation: {}",
            presence.soul_kernel.degradation_reasons.join(", ")
        );
    }

    let memory_store = platform.memory_store();
    if memory_store.get_memory().is_ok() {
        println!("✓ Memory store accessible");
    } else {
        println!("⚠ Memory store not accessible");
    }

    println!("\nDiagnostics complete.");
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_restart_command() {
    match beetle::runtime::linux_service::run_beetle_service_action("restart") {
        Ok(Some(status)) if status.success() => {
            println!("beetle service restart requested.");
            return;
        }
        Ok(Some(status)) => {
            eprintln!("beetle service restart failed with exit status: {}", status);
            std::process::exit(status.code().unwrap_or(1));
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("failed to run beetle service restart: {}", error);
            std::process::exit(1);
        }
    }

    eprintln!("restart requires a managed beetle service (systemd or /etc/init.d/beetle).");
    std::process::exit(1);
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_stop_command() {
    match beetle::runtime::linux_stop::request_linux_stop() {
        Ok(beetle::runtime::linux_stop::LinuxStopOutcome::ManagedService) => {
            println!("beetle stop requested via managed service.");
        }
        Ok(beetle::runtime::linux_stop::LinuxStopOutcome::DirectProcess { targets }) => {
            println!(
                "beetle stop requested for active runtime: {}",
                targets.join(", ")
            );
        }
        Err(error) => {
            eprintln!("failed to stop beetle: {}", error);
            std::process::exit(1);
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_release_status_command(platform: &Arc<dyn Platform>, json: bool) {
    let release = beetle::runtime::inspect_platform_linux_release(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&release).unwrap_or_else(|_| "{}".to_string())
        );
        return;
    }

    println!("Linux release status");
    println!("Managed: {}", release.managed);
    println!("Current executable: {}", release.current_exe);
    println!("Rollout state: {}", release.rollout_state_label());
    println!("Rollback available: {}", release.rollback_available);
    println!(
        "Current release: {}",
        release
            .current
            .as_ref()
            .map(|pointer| format!("{} ({})", pointer.name, pointer.path))
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "Rollback release: {}",
        release
            .rollback
            .as_ref()
            .map(|pointer| format!("{} ({})", pointer.name, pointer.path))
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "State schema: version={} current={}",
        release.state_schema_version, release.state_schema_current
    );
    if let Some(consistent) = release.systemd_unit_consistent {
        println!("systemd template consistent: {}", consistent);
    }
    if let Some(consistent) = release.init_script_consistent {
        println!("init script consistent: {}", consistent);
    }
    if !release.last_action.trim().is_empty() {
        println!("Last release action: {}", release.last_action);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_release_rollback_command(platform: &Arc<dyn Platform>) {
    let release = beetle::runtime::inspect_platform_linux_release(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    if !release.managed {
        eprintln!(
            "current Linux runtime is not running from a managed /opt/beetle release layout."
        );
        std::process::exit(1);
    }
    if !release.rollback_available {
        eprintln!("rollback pointer is not available for the current Linux release.");
        std::process::exit(1);
    }

    match beetle::runtime::rollback_current_release(
        platform.as_ref(),
        "manual_release_rollback",
        beetle::util::current_unix_secs(),
    ) {
        Ok(true) => {
            println!("rollback symlink applied. Restart beetle to boot the rolled-back release.");
        }
        Ok(false) => {
            eprintln!("rollback pointer became unavailable before rollback was applied.");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("failed to apply Linux release rollback: {}", error);
            std::process::exit(1);
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn startup_banner_lines(entrypoint: &str, config_path: Option<&str>) -> Vec<String> {
    let mut lines = vec![
        "========================================".to_string(),
        format!("  甲壳虫 beetle v{} [{}]", VERSION, entrypoint),
        "========================================".to_string(),
    ];
    if let Some(path) = config_path {
        lines.push(format!("[{}] using config file: {}", TAG, path));
    }
    lines
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn log_start_banner(entrypoint: &str, config_path: Option<&str>) {
    for line in startup_banner_lines(entrypoint, config_path) {
        log::info!("{}", line);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn log_launch_role(role: &str) {
    log::info!("[{}] entrypoint_role={}", TAG, role);
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn ensure_linux_single_run() -> beetle::Result<()> {
    let others = beetle::platform::other_beetle_run_summaries()
        .map_err(|error| beetle::Error::io("linux_runtime_guard", error))?;
    if others.is_empty() {
        return Ok(());
    }
    Err(beetle::Error::config(
        "linux_runtime_guard",
        format!(
            "another `beetle run` instance is already active: {}",
            others.join(", ")
        ),
    ))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn run_linux_agent_entry(platform: Arc<dyn Platform>) {
    log_launch_role("agent");
    if let Err(error) = ensure_linux_single_run() {
        log::error!("[{}] duplicate runtime start blocked: {}", TAG, error);
        std::process::exit(1);
    }
    if let Err(error) =
        beetle::runtime::linux_signal::install_linux_signal_bridge(Arc::clone(&platform))
    {
        log::error!("[{}] linux signal bridge install failed: {}", TAG, error);
        std::process::exit(1);
    }
    bootstrap_platform_runtime(platform);
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn bootstrap_platform_runtime(platform: Arc<dyn Platform>) {
    if let Some((guard_platform, agent_handle)) = bootstrap_platform_runtime_until_guard(platform) {
        run_runtime_guard_loop(guard_platform, agent_handle);
    }
}

fn bootstrap_platform_runtime_until_guard(
    platform: Arc<dyn Platform>,
) -> Option<(Arc<dyn Platform>, Option<beetle::util::TaskHandle>)> {
    register_platform_memory_snapshot_provider(&platform);
    register_platform_crash_metadata_provider();
    startup_soul_kernel_recovery(Arc::clone(&platform));
    let (config, wifi_init_ok) = beetle::bootstrap::bootstrap_config_and_wifi(&platform);
    let guard_platform = Arc::clone(&platform);
    start_runtime_planes(platform, config, wifi_init_ok).map(|handle| (guard_platform, handle))
}

fn register_process_memory_snapshot_provider(
    provider: Arc<dyn Fn() -> beetle::platform::MemorySnapshot + Send + Sync>,
) {
    beetle::orchestrator::register_memory_snapshot_provider(provider);
    beetle::orchestrator::log_startup_memory_checkpoint("memory_provider_registered");
}

fn register_platform_memory_snapshot_provider(platform: &Arc<dyn Platform>) {
    register_process_memory_snapshot_provider(Arc::new({
        let platform = Arc::clone(platform);
        move || platform.memory_snapshot()
    }));
}

fn register_platform_crash_metadata_provider() {
    beetle::orchestrator::register_crash_metadata_provider(Arc::new(|| {
        beetle::platform::crash_evidence::snapshot()
    }));
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn install_linux_rustls_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn main() {
    use beetle::commands::{Cli, Commands, ReleaseAction};

    let cli = Cli::parse();

    if env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init()
        .is_err()
    {
        eprintln!("[beetle] env_logger init failed (logging may be incomplete)");
    }
    install_linux_rustls_crypto_provider();

    match &cli.command {
        Commands::Restart => {
            handle_restart_command();
            return;
        }
        Commands::Stop => {
            handle_stop_command();
            return;
        }
        Commands::Version => {
            println!("beetle v{}", VERSION);
            return;
        }
        Commands::ReasoningRunner => {
            if let Err(error) = beetle::run_reasoning_runner_stdio() {
                eprintln!("[{}] reasoning runner failed: {}", TAG, error);
                std::process::exit(1);
            }
            return;
        }
        _ => {}
    }

    let platform: Arc<dyn Platform> = Arc::new(LinuxPlatform::new());
    if let Err(e) = platform.init() {
        eprintln!("[{}] platform init failed: {}", TAG, e);
        std::process::exit(1);
    }

    match cli.command {
        Commands::Run {
            config: config_path,
        } => {
            log_start_banner("run", config_path.as_deref());
            run_linux_agent_entry(platform);
        }
        Commands::Config { action } => {
            handle_config_command(&platform, action);
        }
        Commands::Status { json, chat_id } => {
            handle_status_command(&platform, json, chat_id.as_deref());
        }
        Commands::Doctor => {
            handle_doctor_command(&platform);
        }
        Commands::Release { action } => match action {
            ReleaseAction::Status { json } => handle_release_status_command(&platform, json),
            ReleaseAction::Rollback => handle_release_rollback_command(&platform),
        },
        _ => unreachable!("handled before platform initialization"),
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn main() {
    let platform: Arc<dyn Platform> = Arc::new(Esp32Platform::new());
    if let Err(e) = platform.init() {
        // init 失败时日志可能未初始化，尝试 eprintln 兜底
        eprintln!("[{}] platform init failed: {}", TAG, e);
        log::error!("[{}] platform init failed: {}", TAG, e);
        return;
    }
    log::info!("========================================");
    log::info!("  甲壳虫 beetle v{}", VERSION);
    log::info!("========================================");
    for line in beetle::platform::startup_identity_lines() {
        log::info!("{}", line);
    }
    spawn_esp_runtime_bootstrap(platform);
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn spawn_esp_runtime_bootstrap(platform: Arc<dyn Platform>) {
    let runtime_platform = Arc::clone(&platform);
    let spawn_result = beetle::util::spawn_guarded_with_profile_handle(
        "runtime_bootstrap",
        beetle::util::STACK_ESP_RUNTIME_BOOT,
        None,
        beetle::util::HttpThreadRole::Background,
        move || run_esp_runtime_bootstrap(runtime_platform),
    );
    match spawn_result {
        Ok(_handle) => {
            log::info!(
                "[{}] runtime_bootstrap spawned; returning from app_main to release IDF main_task stack",
                TAG
            );
        }
        Err(error) => {
            log::error!("[{}] runtime_bootstrap spawn failed: {}", TAG, error);
            platform.request_restart();
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn run_esp_runtime_bootstrap(platform: Arc<dyn Platform>) {
    let restart_platform = Arc::clone(&platform);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        bootstrap_platform_runtime_until_guard(platform)
    }));
    match result {
        Ok(Some((guard_platform, agent_handle))) => {
            register_esp_runtime_guard(guard_platform, agent_handle);
            return;
        }
        Ok(None) => {
            log::error!(
                "[{}] runtime_bootstrap failed before guard handoff; restart requested",
                TAG
            );
        }
        Err(payload) => {
            log::error!(
                "[{}] runtime_bootstrap panicked: {}; restart requested",
                TAG,
                panic_payload_message(payload.as_ref())
            );
        }
    }
    restart_platform.request_restart();
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn register_esp_runtime_guard(
    platform: Arc<dyn Platform>,
    agent_handle: Option<beetle::util::TaskHandle>,
) {
    beetle::runtime::register_agent_loop_guard(platform, agent_handle);
    log::info!(
        "[{}] runtime_guard registered on bg_timer; runtime_bootstrap can exit and release startup stack",
        TAG
    );
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        (*msg).to_string()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn join_log_values(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join("|")
    }
}

fn sanitize_soul_kernel_error_for_log(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("runtime_bundle_import_failed:") {
        return "runtime_bundle_import_failed".to_string();
    }
    let scrubbed = beetle::util::scrub_credentials(value);
    let sanitized: String = scrubbed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/') {
                ch
            } else {
                '_'
            }
        })
        .take(96)
        .collect();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

fn join_soul_kernel_error_log_values(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values
            .iter()
            .map(|value| sanitize_soul_kernel_error_for_log(value))
            .collect::<Vec<_>>()
            .join("|")
    }
}

fn sanitize_soul_kernel_status_reason_for_log(value: &str) -> String {
    if value.trim().starts_with("runtime_bundle_unreadable:") {
        "runtime_bundle_unreadable".to_string()
    } else {
        sanitize_soul_kernel_error_for_log(value)
    }
}

fn join_soul_kernel_status_reason_log_values(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values
            .iter()
            .map(|value| sanitize_soul_kernel_status_reason_for_log(value))
            .collect::<Vec<_>>()
            .join("|")
    }
}

fn format_soul_kernel_status_for_log(status: &beetle::runtime::SoulKernelStatus) -> String {
    format!(
        "ready={} safe_mode_readable={} degraded={} session_chats={} active_chats={} key_memory={} runtime_bundle_present={} runtime_bundle_loadable={} runtime_bundle_snapshots={} reasons={}",
        status.minimum_viable,
        status.safe_mode_minimum_readable,
        status.degraded,
        status.session_chat_count,
        status.active_chat_ids.len(),
        status.key_memory_count,
        status.runtime_bundle.present,
        status.runtime_bundle.loadable,
        status.runtime_bundle.snapshot_count,
        join_soul_kernel_status_reason_log_values(&status.degradation_reasons),
    )
}

fn format_soul_kernel_recovery_report_for_log(
    report: &beetle::runtime::SoulKernelRecoveryReport,
) -> String {
    if report.restore_attempted {
        format!(
            "recovery action={:?} restored_snapshots={} restored_layers={} errors={} {}",
            report.action,
            report.restored_snapshots,
            join_log_values(&report.restored_layers),
            join_soul_kernel_error_log_values(&report.errors),
            format_soul_kernel_status_for_log(&report.status_after),
        )
    } else {
        format_soul_kernel_status_for_log(&report.status_after)
    }
}

fn log_soul_kernel_recovery_report(report: &beetle::runtime::SoulKernelRecoveryReport) {
    log::info!(
        "[{}] soul_kernel {}",
        TAG,
        format_soul_kernel_recovery_report_for_log(report),
    );
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn startup_soul_kernel_recovery(platform: Arc<dyn Platform>) {
    let report = beetle::runtime::ensure_platform_soul_kernel_recovery(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    log_soul_kernel_recovery_report(&report);
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const STARTUP_RECOVERY_REPORT_WAIT_SECS: u64 = 20;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const STARTUP_RECOVERY_REPORT_POLL_MS: u64 = 250;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
#[derive(Debug, PartialEq, Eq)]
enum StartupRecoveryWaitOutcome<T> {
    Report(T),
    Disconnected,
    TimedOut,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
fn wait_for_startup_recovery_report<T, F>(
    rx: &std::sync::mpsc::Receiver<T>,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
    mut feed: F,
) -> StartupRecoveryWaitOutcome<T>
where
    F: FnMut(),
{
    let started = std::time::Instant::now();
    loop {
        match rx.try_recv() {
            Ok(report) => return StartupRecoveryWaitOutcome::Report(report),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return StartupRecoveryWaitOutcome::Disconnected;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }

        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return StartupRecoveryWaitOutcome::TimedOut;
        }

        let remaining = timeout.saturating_sub(elapsed);
        let sleep_for = if poll_interval.is_zero() || poll_interval > remaining {
            remaining
        } else {
            poll_interval
        };
        if sleep_for.is_zero() {
            std::thread::yield_now();
        } else {
            std::thread::sleep(sleep_for);
        }
        feed();
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn startup_soul_kernel_recovery(platform: Arc<dyn Platform>) {
    let now_secs = beetle::util::current_unix_secs();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let worker_platform = Arc::clone(&platform);
    match beetle::util::spawn_guarded_with_profile_handle(
        "startup_recovery",
        beetle::util::STACK_STARTUP_RECOVERY,
        Some(beetle::util::SpawnCore::Core1),
        beetle::util::HttpThreadRole::Background,
        move || {
            let report = beetle::runtime::ensure_platform_soul_kernel_recovery(
                worker_platform.as_ref(),
                now_secs,
            );
            let _ = tx.send(report);
        },
    ) {
        Ok(handle) => {
            let wait_outcome = wait_for_startup_recovery_report(
                &rx,
                std::time::Duration::from_secs(STARTUP_RECOVERY_REPORT_WAIT_SECS),
                std::time::Duration::from_millis(STARTUP_RECOVERY_REPORT_POLL_MS),
                beetle::platform::task_wdt::feed_current_task,
            );
            match wait_outcome {
                StartupRecoveryWaitOutcome::Report(report) => {
                    log_soul_kernel_recovery_report(&report);
                    if let Err(error) = handle.join() {
                        let message = panic_payload_message(error.as_ref());
                        log::error!("[{}] startup_recovery join failed: {}", TAG, message);
                    }
                }
                StartupRecoveryWaitOutcome::Disconnected => {
                    let error = beetle::Error::config(
                        "startup_recovery_disconnected",
                        "startup recovery worker exited without report",
                    );
                    beetle::state::set_last_error(&error);
                    log::error!("[{}] startup_recovery worker exited without report", TAG);
                    if let Err(error) = handle.join() {
                        let message = panic_payload_message(error.as_ref());
                        log::error!("[{}] startup_recovery join failed: {}", TAG, message);
                    }
                }
                StartupRecoveryWaitOutcome::TimedOut => {
                    let error = beetle::Error::config(
                        "startup_recovery_timeout",
                        format!(
                            "startup recovery did not report within {}s; continuing with degraded startup fact",
                            STARTUP_RECOVERY_REPORT_WAIT_SECS
                        ),
                    );
                    beetle::state::set_last_error(&error);
                    log::error!("[{}] {}", TAG, error);
                }
            }
        }
        Err(error) => {
            let startup_error = beetle::Error::io("startup_recovery_spawn", error);
            beetle::state::set_last_error(&startup_error);
            log::error!(
                "[{}] startup_recovery spawn failed; continuing without pre-WiFi recovery: {}",
                TAG,
                startup_error
            );
        }
    }
}

fn prepare_runtime_assembly(
    platform: Arc<dyn Platform>,
    config: Arc<AppConfig>,
    wifi_init_ok: bool,
) -> Option<PreparedRuntimeAssembly> {
    let runtime = beetle::RuntimeServices::from_platform(Arc::clone(&platform));
    let config_store = Arc::clone(&runtime.config_store);
    let resolve_locale_ui: Arc<dyn Fn() -> beetle::i18n::Locale + Send + Sync> = Arc::new({
        let cs = Arc::clone(&config_store);
        move || beetle::i18n::Locale::from_storage(&beetle::config::get_locale(cs.as_ref()))
    });
    let skill_storage = Arc::clone(&runtime.skill_storage);
    let skill_meta_store = Arc::clone(&runtime.skill_meta_store);
    let skill_prompt_cache = Arc::new(beetle::skills::SkillPromptCache::new(
        Arc::clone(&skill_meta_store),
        Arc::clone(&skill_storage),
        beetle::skills::DEFAULT_PROMPT_SKILL_MAX_CHARS,
    ));
    let _ = skill_prompt_cache.refresh();

    app_runtime_support::ensure_storage_ready(runtime.memory_store.as_ref());
    app_runtime_support::log_runtime_store_lengths(&runtime);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("boot_memory_reads");
    beetle::bootstrap::observe_heap_checkpoint(TAG, "heap_after_boot_memory_reads");

    let bus = RuntimeBus::new(DEFAULT_CAPACITY);
    log::info!(
        "[{}] MessageBus created (capacity {})",
        TAG,
        DEFAULT_CAPACITY
    );
    app_runtime_support::bootstrap_pending_retry_into_inbound(
        runtime.pending_retry_store.as_ref(),
        &bus.user_inbound_tx,
        &bus.system_inbound_tx,
    );

    #[cfg(all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    let dingtalk_session_store: beetle::channels::DingtalkSessionStore =
        Arc::new(Mutex::new(HashMap::new()));
    #[cfg(feature = "wecom")]
    let wecom_aibot_route_store = beetle::channels::new_wecom_aibot_route_store();
    #[cfg(feature = "qq_channel")]
    let qq_msg_id_cache: beetle::channels::QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
    #[cfg(feature = "qq_channel")]
    let qq_inbound_dedup_store: beetle::channels::QqInboundDedupStore =
        Arc::new(Mutex::new(HashMap::new()));
    #[cfg(feature = "qq_channel")]
    let qq_token_cache = beetle::channels::new_shared_qq_token_cache();
    #[cfg(feature = "qq_channel")]
    let qq_ws_status = beetle::channels::new_shared_qq_ws_status();
    #[allow(unused_variables)]
    let (mut registry, baidu_token_cache) = beetle::build_default_registry(&config, &runtime);

    beetle::bootstrap::init_audio_if_enabled(&platform, &config);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("audio_init_phase_done");
    beetle::bootstrap::observe_heap_checkpoint(TAG, "heap_after_audio_init");

    if let Some(ref bus_cfg) = config.i2c_bus {
        if let Err(error) = platform.init_i2c(bus_cfg) {
            log::warn!(
                "[{}] I2C bus init failed (devices will be unavailable): {}",
                TAG,
                error
            );
        }
    }

    let voice_event_channel =
        build_voice_event_channel(&platform, &config, baidu_token_cache.as_ref());
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("voice_event_channel_ready");
    let voice_channel_enabled = matches!(
        voice_event_channel.as_ref(),
        Some(VoiceEventChannel {
            speak_capable: true,
            ..
        })
    );
    let device_capability_registry =
        beetle::build_device_capability_registry(config.as_ref(), platform.as_ref());
    let channel_capability_registry = Arc::new(beetle::build_channel_capability_registry(
        config.as_ref(),
        voice_channel_enabled,
    ));
    let capability_package_runtime_capabilities = Arc::new(
        beetle::build_capability_package_runtime_capabilities(channel_capability_registry.as_ref()),
    );
    registry.set_llm_visibility_overlay_provider(Arc::new({
        let state_fs = platform.state_fs();
        let runtime_capabilities = Arc::clone(&capability_package_runtime_capabilities);
        move |ingress, channel| {
            let policy = beetle::ToolPolicyContext::new(ingress, channel);
            match beetle::build_capability_package_tool_policy_set(
                state_fs.as_ref(),
                runtime_capabilities.as_ref(),
                policy.channel,
            ) {
                Ok(set) => set,
                Err(error) => {
                    log::warn!(
                        "[capability_package] failed to load tool policy overlays for {}:{}: {}",
                        match policy.ingress {
                            IngressKind::User => "user",
                            IngressKind::System => "system",
                        },
                        policy.channel,
                        error
                    );
                    beetle::CapabilityPackageToolPolicySet::default()
                }
            }
        }
    }));
    let registry = Arc::new(registry);
    let network_governor = Arc::new(NetworkGovernor::new(
        Arc::clone(&platform),
        Arc::clone(&config),
    ));
    let chat_streams = Arc::new(beetle::ChatStreamBroker::new());

    if !app_runtime_support::startup_self_check(runtime.memory_store.as_ref()) {
        log::error!(
            "[{}] startup self-check failed: shared memory storage not readable (get_memory failed)",
            TAG
        );
        return None;
    }
    let wifi_init_status = if wifi_init_ok { "ok" } else { "failed" };
    let sta_up = beetle::platform::is_wifi_sta_connected();
    let storage_usage = platform.storage_usage();
    let startup_session_count = runtime
        .session_store
        .list_chat_ids()
        .map(|v| v.len() as u32)
        .unwrap_or(0);
    let state_fs_ready = beetle::orchestrator::update_session_storage_from_bytes(
        startup_session_count,
        storage_usage,
    );
    let wall_clock_valid = beetle::platform::time::wall_clock_is_trustworthy();
    let storage_info = storage_usage
        .map(|(total, used)| format!("{} free", total.saturating_sub(used)))
        .unwrap_or_else(|| "N/A".to_string());
    log::info!(
        "[{}] startup self-check ok (storage readable, wifi_init={}, sta_up={}, wall_clock_valid={}, storage={})",
        TAG,
        wifi_init_status,
        sta_up,
        wall_clock_valid,
        storage_info
    );
    beetle::orchestrator::observe_runtime_capabilities_from_platform_with_config(
        platform.as_ref(),
        Some(config.as_ref()),
        false,
        Some(state_fs_ready),
    );
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        beetle::orchestrator::log_baseline();
        beetle::orchestrator::log_startup_memory_checkpoint("startup_self_check_ok");
    }

    Some(PreparedRuntimeAssembly {
        runtime,
        config,
        resolve_locale_ui,
        skill_prompt_cache,
        bus,
        #[cfg(all(
            feature = "dingtalk",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        dingtalk_session_store,
        #[cfg(feature = "wecom")]
        wecom_aibot_route_store,
        #[cfg(feature = "qq_channel")]
        qq_msg_id_cache,
        #[cfg(feature = "qq_channel")]
        qq_inbound_dedup_store,
        #[cfg(feature = "qq_channel")]
        qq_token_cache,
        #[cfg(feature = "qq_channel")]
        qq_ws_status,
        registry,
        baidu_token_cache,
        voice_event_channel,
        device_capability_registry,
        channel_capability_registry,
        capability_package_runtime_capabilities,
        network_governor,
        chat_streams,
        #[cfg(feature = "config_api")]
        config_api_shared_config: None,
    })
}

fn start_support_planes(assembly: &mut PreparedRuntimeAssembly) -> beetle::Result<()> {
    #[cfg(feature = "config_api")]
    {
        let shared_runtime_config = Arc::new(RwLock::new((*assembly.config).clone()));
        assembly.config_api_shared_config = Some(Arc::clone(&shared_runtime_config));
        spawn_http_config_server(HttpServerSpawnContext {
            platform: Arc::clone(&assembly.runtime.platform),
            tool_registry: Arc::clone(&assembly.registry),
            channel_capability_registry: Arc::clone(&assembly.channel_capability_registry),
            capability_package_runtime_capabilities: Arc::clone(
                &assembly.capability_package_runtime_capabilities,
            ),
            inbound_depth: Arc::clone(&assembly.bus.user_inbound_depth),
            outbound_depth: Arc::clone(&assembly.bus.outbound_depth),
            memory_store: Arc::clone(&assembly.runtime.memory_store),
            session_store: Arc::clone(&assembly.runtime.session_store),
            system_inbound_tx: assembly.bus.system_inbound_tx.clone(),
            skill_prompt_cache: Arc::clone(&assembly.skill_prompt_cache),
            inbound_tx: assembly.bus.user_inbound_tx.clone(),
            chat_streams: Arc::clone(&assembly.chat_streams),
            shared_config: Arc::clone(&shared_runtime_config),
        })?;
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        beetle::orchestrator::log_startup_memory_checkpoint("config_api_spawned");
    }

    finalize_required_thread_start(TAG, "bg_timer spawn confirmed", "bg_timer_spawn", || {
        beetle::bg_timer::run_bg_timer(beetle::bg_timer::BgTimerContext {
            system_inbound_tx: assembly.bus.system_inbound_tx.clone(),
            resolve_locale: Arc::clone(&assembly.resolve_locale_ui),
            platform: Arc::clone(&assembly.runtime.platform),
            config: Arc::clone(&assembly.config),
            version: VERSION,
            read_heartbeat: Box::new(|| {
                beetle::platform::read_heartbeat_file().unwrap_or_default()
            }),
            user_inbound_depth: Arc::clone(&assembly.bus.user_inbound_depth),
            system_inbound_depth: Arc::clone(&assembly.bus.system_inbound_depth),
            outbound_depth: Arc::clone(&assembly.bus.outbound_depth),
            session_store: Arc::clone(&assembly.runtime.session_store),
            memory_system_kind: assembly.runtime.memory_system_kind,
            autonomy_strategy_store: Arc::clone(&assembly.runtime.autonomy_strategy_store),
            self_authored_core_store: Arc::clone(&assembly.runtime.self_authored_core_store),
            self_continuity_store: Arc::clone(&assembly.runtime.self_continuity_store),
            relationship_portfolio_store: Arc::clone(
                &assembly.runtime.relationship_portfolio_store,
            ),
            relationship_topology_store: Arc::clone(&assembly.runtime.relationship_topology_store),
            memory_store: Some(Arc::clone(&assembly.runtime.memory_store)),
            sensor_watch: assembly
                .device_capability_registry
                .is_mounted(beetle::DEVICE_CAPABILITY_SENSOR)
                .then(|| beetle::cron::SensorWatchContext {
                    platform: Arc::clone(&assembly.runtime.platform),
                    devices: assembly.config.hardware_devices.clone(),
                    i2c_sensors: assembly.config.i2c_sensors.clone(),
                }),
            remind_store: Arc::clone(&assembly.runtime.remind_at_store),
            task_store: Arc::clone(&assembly.runtime.task_store),
        })
    })?;
    beetle::bg_timer::log_bg_timer_started();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("bg_timer_started");

    // Refresh runtime capability state from the live Wi-Fi snapshot without
    // blocking startup on STA settle; heartbeat keeps the contract current.
    let outbound_transport_ready = match assembly.runtime.platform.memory_system_kind() {
        beetle::memory::MemorySystemKind::LinuxFull => true,
        beetle::memory::MemorySystemKind::EspCompact => beetle::platform::is_wifi_sta_connected(),
    };
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("communication_plane_ready");
    beetle::orchestrator::observe_runtime_capabilities_from_platform_with_config(
        assembly.runtime.platform.as_ref(),
        Some(assembly.config.as_ref()),
        outbound_transport_ready,
        None,
    );
    beetle::orchestrator::init();
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("orchestrator_initialized");

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if assembly.runtime.platform.display_available() {
        let display_platform = Arc::clone(&assembly.runtime.platform);
        let display_config = Arc::clone(&assembly.config);
        let plan = thread_plan("display");
        mark_display_lifecycle(
            beetle::runtime::PlaneLifecycleState::Starting,
            "display_thread_spawn",
        );
        match beetle::util::spawn_guarded_with_profile_handle(
            "display",
            beetle::util::STACK_DISPLAY,
            plan.core,
            plan.role,
            move || run_display_loop(display_platform, display_config),
        ) {
            Ok(_handle) => {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                beetle::orchestrator::log_startup_memory_checkpoint("display_thread_spawned");
            }
            Err(error) => {
                mark_display_lifecycle(
                    beetle::runtime::PlaneLifecycleState::Failed,
                    "display_thread_spawn_failed",
                );
                log::warn!("[{}] display thread spawn failed: {}", TAG, error);
            }
        }
    } else {
        mark_display_lifecycle(
            beetle::runtime::PlaneLifecycleState::Disabled,
            "display_unavailable",
        );
    }

    Ok(())
}

fn start_communication_planes(assembly: &mut PreparedRuntimeAssembly) -> beetle::Result<()> {
    #[allow(unused_mut)]
    let (mut sinks, mut channel_rx_set) = beetle::channels::build_channel_sinks(
        assembly.config.as_ref(),
        #[cfg(feature = "qq_channel")]
        &assembly.qq_msg_id_cache,
        #[cfg(feature = "qq_channel")]
        &assembly.qq_token_cache,
        #[cfg(all(
            feature = "dingtalk",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        &assembly.dingtalk_session_store,
        #[cfg(feature = "wecom")]
        &assembly.wecom_aibot_route_store,
    );
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if assembly.runtime.platform.display_available() {
        let _ = display_command_with_owner(
            &assembly.runtime.platform,
            DisplayOwner::DefaultDashboard,
            DisplayCommand::UpdateBootProgress { stage: 3 },
        );
    }

    let enabled_channel =
        beetle::normalize_compiled_enabled_channel(assembly.config.enabled_channel.as_str());
    log::info!(
        "[{}] enabled_channel='{}'",
        TAG,
        if enabled_channel.is_empty() {
            "(none)"
        } else {
            enabled_channel
        }
    );

    let started_voice_session = if assembly.voice_event_channel.is_some() {
        spawn_voice_session_if_ready(
            &assembly.runtime.platform,
            &assembly.network_governor,
            &assembly.config,
            assembly.baidu_token_cache.as_ref(),
            &assembly.bus.user_inbound_tx,
            &mut assembly.voice_event_channel,
        )?
    } else {
        None
    };
    if let Some(voice_tx) = voice_sink_sender(started_voice_session.as_ref()) {
        sinks.register(
            beetle::constants::VOICE_CHANNEL_NAME,
            Box::new(beetle::channels::VoiceSink::new(voice_tx)),
        );
    }
    let sinks = Arc::new(sinks);

    #[cfg(feature = "feishu")]
    {
        if let Some(ref c) = channel_rx_set.feishu {
            let tx = assembly.bus.user_inbound_tx.clone();
            let id = c.app_id.clone();
            let sec = c.app_secret.clone();
            let allowed = parse_allowed_chat_ids(&assembly.config.feishu_allowed_chat_ids);
            let pending = Arc::clone(&assembly.runtime.pending_retry_store);
            let http_factory = assembly
                .network_governor
                .http_factory(HttpClientClass::Background);
            let feishu_spawner: Arc<
                dyn Fn() -> beetle::Result<beetle::util::TaskHandle> + Send + Sync,
            > = Arc::new(move || {
                let tx = tx.clone();
                let id = id.clone();
                let sec = sec.clone();
                let allowed = allowed.clone();
                let pending = Arc::clone(&pending);
                let http_factory = Arc::clone(&http_factory);
                spawn_planned_handle("feishu_ws", STACK_CHANNEL_WS, move || {
                    run_feishu_ws_loop(
                        id,
                        sec,
                        allowed,
                        tx,
                        pending.as_ref(),
                        move || http_factory(),
                        |url| beetle::network::connect_external_wss(url, "feishu_ws"),
                    )
                })
                .map_err(|error| beetle::Error::io("feishu_ws_spawn", error))
            });
            spawn_supervised_channel_wss_thread(
                TAG,
                "feishu_ws",
                "Feishu WS loop started",
                "feishu_ws_spawn",
                feishu_spawner,
            )?;
        } else if enabled_channel == "feishu" {
            log::warn!(
                "[{}] Feishu WS not started: app_id or app_secret empty (check channels config)",
                TAG
            );
        }
    }

    #[cfg(feature = "qq_channel")]
    if enabled_channel == "qq_channel" {
        if let Some(ref c) = channel_rx_set.qq_channel {
            if !c.app_id.trim().is_empty() && !c.app_secret.trim().is_empty() {
                let qq_tx = assembly.bus.user_inbound_tx.clone();
                let qq_id = c.app_id.clone();
                let qq_sec = c.app_secret.clone();
                let qq_cache_ws = Arc::clone(&assembly.qq_msg_id_cache);
                let qq_inbound_dedup_ws = Arc::clone(&assembly.qq_inbound_dedup_store);
                let qq_token_cache_ws = assembly.qq_token_cache.clone();
                let qq_ws_status = assembly.qq_ws_status.clone();
                let qq_pending = Arc::clone(&assembly.runtime.pending_retry_store);
                let http_factory = assembly
                    .network_governor
                    .http_factory(HttpClientClass::Background);
                let qq_spawner: Arc<
                    dyn Fn() -> beetle::Result<beetle::util::TaskHandle> + Send + Sync,
                > = Arc::new(move || {
                    let qq_tx = qq_tx.clone();
                    let qq_id = qq_id.clone();
                    let qq_sec = qq_sec.clone();
                    let qq_cache_ws = Arc::clone(&qq_cache_ws);
                    let qq_inbound_dedup_ws = Arc::clone(&qq_inbound_dedup_ws);
                    let qq_token_cache_ws = qq_token_cache_ws.clone();
                    let qq_ws_status = qq_ws_status.clone();
                    let qq_pending = Arc::clone(&qq_pending);
                    let http_factory = Arc::clone(&http_factory);
                    spawn_planned_handle("qq_ws", STACK_CHANNEL_WS, move || {
                        beetle::run_qq_ws_loop(
                            beetle::QqWsLoopConfig {
                                app_id: qq_id,
                                client_secret: qq_sec,
                                msg_id_cache: qq_cache_ws,
                                inbound_dedup_store: qq_inbound_dedup_ws,
                                shared_token_cache: qq_token_cache_ws,
                                shared_ws_status: qq_ws_status,
                            },
                            qq_tx,
                            qq_pending.as_ref(),
                            move || http_factory(),
                            |url| beetle::network::connect_external_wss(url, "qq_ws"),
                        )
                    })
                    .map_err(|error| beetle::Error::io("qq_ws_spawn", error))
                });
                spawn_supervised_channel_wss_thread(
                    TAG,
                    "qq_ws",
                    "QQ WS loop started",
                    "qq_ws_spawn",
                    qq_spawner,
                )?;
            }
        }
    }

    #[cfg(all(
        feature = "dingtalk",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    if enabled_channel == "dingtalk" {
        if assembly.config.dingtalk_client_id.trim().is_empty()
            || assembly.config.dingtalk_client_secret.trim().is_empty()
        {
            log::warn!(
                "[{}] DingTalk Stream not started: client_id or client_secret empty",
                TAG
            );
        } else {
            let dt_client_id = assembly.config.dingtalk_client_id.clone();
            let dt_client_secret = assembly.config.dingtalk_client_secret.clone();
            let dt_inbound_tx = assembly.bus.user_inbound_tx.clone();
            let dt_session_store = Arc::clone(&assembly.dingtalk_session_store);
            let http_factory = assembly
                .network_governor
                .http_factory(HttpClientClass::Background);
            spawn_required_planned_thread(
                TAG,
                "dingtalk_stream",
                STACK_CHANNEL_WS,
                "DingTalk Stream loop started",
                "dingtalk_stream_spawn",
                move || {
                    beetle::run_dingtalk_stream_loop(
                        dt_client_id,
                        dt_client_secret,
                        dt_inbound_tx,
                        dt_session_store,
                        move || http_factory(),
                        |url| beetle::network::connect_external_wss(url, "dingtalk_stream"),
                    )
                },
            )?;
        }
    }

    #[cfg(feature = "wecom")]
    if enabled_channel == "wecom" {
        if let Some(c) = channel_rx_set.wecom.take() {
            let wc_bot_id = c.bot_id;
            let wc_bot_secret = c.bot_secret;
            let wc_websocket_url = c.websocket_url;
            let wc_inbound_tx = assembly.bus.user_inbound_tx.clone();
            let wc_pending = Arc::clone(&assembly.runtime.pending_retry_store);
            let wc_route_store = c.route_store;
            let wc_rx = c.rx;
            let wecom_spawner: Arc<
                dyn Fn() -> beetle::Result<beetle::util::TaskHandle> + Send + Sync,
            > = Arc::new(move || {
                let wc_bot_id = wc_bot_id.clone();
                let wc_bot_secret = wc_bot_secret.clone();
                let wc_websocket_url = wc_websocket_url.clone();
                let wc_inbound_tx = wc_inbound_tx.clone();
                let wc_pending = Arc::clone(&wc_pending);
                let wc_route_store = Arc::clone(&wc_route_store);
                let wc_rx = Arc::clone(&wc_rx);
                spawn_planned_handle("wecom_aibot", STACK_CHANNEL_WS, move || {
                    beetle::run_wecom_aibot_loop(
                        beetle::WecomAibotLoopConfig {
                            bot_id: wc_bot_id,
                            bot_secret: wc_bot_secret,
                            websocket_url: wc_websocket_url,
                            inbound_tx: wc_inbound_tx,
                            pending_retry: wc_pending,
                            outbound_rx: wc_rx,
                            route_store: wc_route_store,
                        },
                        |url| beetle::network::connect_external_wss(url, "wecom_aibot"),
                    )
                })
                .map_err(|error| beetle::Error::io("wecom_aibot_spawn", error))
            });
            spawn_supervised_channel_wss_thread(
                TAG,
                "wecom_aibot",
                "WeCom AI Bot loop started",
                "wecom_aibot_spawn",
                wecom_spawner,
            )?;
        } else {
            log::warn!(
                "[{}] WeCom AI Bot not started: bot_id or bot_secret empty",
                TAG
            );
        }
    }

    let channel_http_factory = assembly
        .network_governor
        .http_factory(HttpClientClass::Background);
    let outbound_rx_for_dispatch = assembly
        .bus
        .outbound_rx
        .take()
        .ok_or_else(|| beetle::Error::config("dispatch_spawn", "outbound_rx already taken"))?;
    let sinks_clone = Arc::clone(&sinks);
    let channel_capability_registry = Arc::clone(&assembly.channel_capability_registry);
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let os_outbound_active = {
        #[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
        let active_outbound = beetle::channels::build_esp_active_outbound_driver(
            assembly.config.as_ref(),
            #[cfg(feature = "qq_channel")]
            &assembly.qq_msg_id_cache,
            #[cfg(feature = "qq_channel")]
            &assembly.qq_token_cache,
            Arc::clone(&channel_http_factory),
        );
        #[cfg(not(any(feature = "telegram", feature = "feishu", feature = "qq_channel")))]
        let active_outbound = None;
        if let Some(active_outbound) = active_outbound {
            spawn_planned_handle("os_outbound_supervisor", STACK_DISPATCH, move || {
                beetle::channels::run_os_outbound_supervisor(
                    outbound_rx_for_dispatch,
                    active_outbound,
                    sinks_clone,
                    channel_capability_registry,
                )
            })
            .map_err(|error| beetle::Error::io("os_outbound_supervisor_spawn", error))?;
            beetle::orchestrator::log_startup_memory_checkpoint("os_outbound_supervisor_spawn");
            true
        } else {
            spawn_planned_handle("dispatch", STACK_DISPATCH, move || {
                run_dispatch(
                    outbound_rx_for_dispatch,
                    sinks_clone,
                    channel_capability_registry,
                )
            })
            .map_err(|error| beetle::Error::io("dispatch_spawn", error))?;
            beetle::orchestrator::log_startup_memory_checkpoint("dispatch_spawn");
            false
        }
    };
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    {
        spawn_planned_handle("dispatch", STACK_DISPATCH, move || {
            run_dispatch(
                outbound_rx_for_dispatch,
                sinks_clone,
                channel_capability_registry,
            )
        })
        .map_err(|error| beetle::Error::io("dispatch_spawn", error))?;
    }

    #[cfg(feature = "telegram")]
    if enabled_channel == "telegram" && !assembly.config.tg_token.trim().is_empty() {
        let tg_token = assembly.config.tg_token.clone();
        let tg_allowed = parse_allowed_chat_ids(&assembly.config.tg_allowed_chat_ids);
        let tg_group_activation =
            Arc::new(RwLock::new(assembly.config.tg_group_activation.clone()));
        let tg_inbound_tx = assembly.bus.user_inbound_tx.clone();
        let tg_outbound_tx = assembly.bus.outbound_tx.clone();
        let tg_session_store = Arc::clone(&assembly.runtime.session_store);
        let tg_pending = Arc::clone(&assembly.runtime.pending_retry_store);
        let tg_inbound_depth = Arc::clone(&assembly.bus.user_inbound_depth);
        let tg_outbound_depth = Arc::clone(&assembly.bus.outbound_depth);
        let tg_config_file_store: Arc<dyn beetle::config::ConfigFileStore + Send + Sync> = Arc::new(
            beetle::config::PlatformConfigFileStore(Arc::clone(&assembly.runtime.platform)),
        );
        let tg_group_activation_for_set = Arc::clone(&tg_group_activation);
        #[cfg(feature = "config_api")]
        let tg_config_api_shared_config =
            assembly.config_api_shared_config.as_ref().map(Arc::clone);
        let tg_resolve_locale = Arc::clone(&assembly.resolve_locale_ui);
        let http_factory = assembly
            .network_governor
            .http_factory(HttpClientClass::Background);
        let set_group_activation = Box::new(move |value: &str| {
            beetle::config::save_tg_group_activation_to_channels(
                tg_config_file_store.as_ref(),
                value,
            )?;
            *tg_group_activation_for_set
                .write()
                .unwrap_or_else(|error| error.into_inner()) = value.to_string();
            #[cfg(feature = "config_api")]
            if let Some(shared_config) = &tg_config_api_shared_config {
                shared_config
                    .write()
                    .unwrap_or_else(|error| error.into_inner())
                    .tg_group_activation = value.to_string();
            }
            Ok(())
        });
        spawn_required_planned_thread(
            TAG,
            "tg_poll",
            beetle::util::STACK_CHANNEL_SENDER,
            "Telegram poll loop started",
            "tg_poll_spawn",
            move || {
                beetle::run_telegram_poll_loop(
                    tg_token,
                    tg_allowed,
                    tg_group_activation,
                    tg_inbound_tx,
                    tg_pending,
                    tg_outbound_tx,
                    tg_session_store,
                    tg_inbound_depth,
                    tg_outbound_depth,
                    set_group_activation,
                    tg_resolve_locale,
                    move || http_factory(),
                )
            },
        )?;
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if !os_outbound_active {
        beetle::channels::spawn_sender_threads(
            &mut channel_rx_set,
            &assembly.config.tg_token,
            Arc::clone(&channel_http_factory),
        )?;
    }
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    beetle::channels::spawn_sender_threads(
        &mut channel_rx_set,
        &assembly.config.tg_token,
        channel_http_factory,
    )?;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if !os_outbound_active {
        beetle::orchestrator::log_startup_memory_checkpoint("sender_threads_spawned");
    }

    Ok(())
}

fn start_agent_plane(
    assembly: &mut PreparedRuntimeAssembly,
) -> beetle::Result<Option<beetle::util::TaskHandle>> {
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if assembly.runtime.platform.display_available() {
        let _ = display_command_with_owner(
            &assembly.runtime.platform,
            DisplayOwner::DefaultDashboard,
            DisplayCommand::UpdateBootProgress { stage: 4 },
        );
    }

    let worker_llm: Arc<dyn beetle::LlmClient + Send + Sync> = Arc::from(
        beetle::build_llm_clients(&assembly.config, Arc::clone(&assembly.resolve_locale_ui)),
    );

    let get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync> = Arc::new({
        let skill_prompt_cache = Arc::clone(&assembly.skill_prompt_cache);
        move || {
            let runtime_mode =
                beetle::runtime::thread_registry::runtime_mode_snapshot().current_mode;
            let pressure = beetle::orchestrator::snapshot().pressure;
            let max_chars = beetle::skills::prompt_skill_budget_for_runtime_mode(
                runtime_mode,
                pressure,
                beetle::skills::DEFAULT_PROMPT_SKILL_MAX_CHARS,
                cfg!(any(
                    target_arch = "xtensa",
                    target_arch = "riscv32",
                    target_os = "linux"
                )),
            );
            if max_chars == 0 {
                return String::new();
            }
            let rendered = skill_prompt_cache.get();
            if rendered.len() <= max_chars {
                rendered
            } else {
                beetle::util::truncate_to_byte_len(&rendered, max_chars)
            }
        }
    });
    let get_capability_package_text: CapabilityPackageTextProvider = Arc::new({
        let state_fs = assembly.runtime.platform.state_fs();
        let runtime_capabilities = Arc::clone(&assembly.capability_package_runtime_capabilities);
        move |channel, max_chars| {
            if max_chars == 0 {
                return None;
            }
            match beetle::build_capability_package_runtime_prompt_bundle(
                state_fs.as_ref(),
                runtime_capabilities.as_ref(),
                channel,
                max_chars,
            ) {
                Ok(bundle) if !bundle.text.trim().is_empty() => Some(bundle.text),
                Ok(_) => None,
                Err(error) => {
                    log::warn!(
                        "[capability_package] failed to load runtime prompt bundle for {}: {}",
                        channel,
                        error
                    );
                    None
                }
            }
        }
    });
    let enabled_channel =
        beetle::normalize_compiled_enabled_channel(assembly.config.enabled_channel.as_str());
    let typing_notifier: Option<Box<dyn beetle::TypingNotifier>> = assembly
        .channel_capability_registry
        .get(enabled_channel)
        .filter(|entry| entry.enabled && entry.contract.supports_typing_or_chat_action)
        .and_then(|entry| match entry.id {
            #[cfg(feature = "telegram")]
            beetle::CHANNEL_TELEGRAM if !assembly.config.tg_token.trim().is_empty() => {
                Some(Box::new(TelegramTypingNotifier {
                    token: assembly.config.tg_token.clone(),
                }) as Box<dyn beetle::TypingNotifier>)
            }
            _ => None,
        });

    let stream_editor: Option<Arc<dyn beetle::StreamEditor + Send + Sync>> = if assembly
        .channel_capability_registry
        .get(enabled_channel)
        .map(|entry| entry.enabled && entry.contract.supports_stream_edit)
        .unwrap_or(false)
    {
        match enabled_channel {
            #[cfg(feature = "feishu")]
            beetle::CHANNEL_FEISHU if !assembly.config.feishu_app_id.trim().is_empty() => {
                let make_http = assembly
                    .network_governor
                    .http_factory(HttpClientClass::Interactive);
                Some(Arc::new(FeishuStreamEditor {
                    app_id: assembly.config.feishu_app_id.clone(),
                    app_secret: assembly.config.feishu_app_secret.clone(),
                    create_http: Arc::clone(&make_http),
                    state: Mutex::new(beetle::FeishuTokenCache::new()),
                })
                    as Arc<dyn beetle::StreamEditor + Send + Sync>)
            }
            _ => None,
        }
    } else {
        None
    };
    let stream_editor_channel = stream_editor
        .as_ref()
        .map(|_| Arc::<str>::from(enabled_channel));
    let agent_strategy = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        beetle::agent::AgentRunStrategy::Embedded
    } else {
        beetle::agent::AgentRunStrategy::LinuxEnhanced
    };
    let agent_config = Arc::new(beetle::AgentLoopConfig {
        runtime: assembly.runtime.clone(),
        get_skill_descriptions,
        get_capability_package_text,
        tg_group_activation: Arc::<str>::from(assembly.config.tg_group_activation.as_str()),
        channel_capability_registry: Arc::clone(&assembly.channel_capability_registry),
        strategy: agent_strategy,
        stream_editor,
        stream_editor_channel,
        chat_streams: Arc::clone(&assembly.chat_streams),
        resolve_locale: Arc::clone(&assembly.resolve_locale_ui),
    });

    #[cfg(feature = "cli")]
    {
        let cli_ctx = beetle::cli::CliContext::new(
            Arc::clone(&assembly.config),
            Arc::clone(&assembly.runtime.config_store),
            Arc::clone(&assembly.runtime.memory_store),
            Arc::clone(&assembly.runtime.session_store),
            Arc::clone(&assembly.runtime.platform),
            Arc::clone(&assembly.registry),
            Arc::clone(&assembly.channel_capability_registry),
            Arc::clone(&assembly.capability_package_runtime_capabilities),
            Some(Arc::clone(&assembly.bus.user_inbound_depth)),
            Some(Arc::clone(&assembly.bus.outbound_depth)),
        );
        spawn_planned("cli_repl", beetle::util::STACK_CLI_REPL, move || {
            let reader = std::io::BufReader::new(std::io::stdin());
            beetle::cli::run_repl(cli_ctx, reader);
        });
        log::info!("[{}] CLI REPL started (stdin)", TAG);
    }

    let agent_plan = thread_plan("agent_loop");
    let tag = TAG;
    let agent_registry = Arc::clone(&assembly.registry);
    let agent_worker_llm = Arc::clone(&worker_llm);
    let agent_platform = Arc::clone(&assembly.runtime.platform);
    let agent_network = Arc::clone(&assembly.network_governor);
    let agent_loop_config = Arc::clone(&agent_config);
    let worker_user_inbound_tx = assembly.bus.user_inbound_tx.clone();
    let user_inbound_rx = assembly.bus.user_inbound_rx.take().ok_or_else(|| {
        beetle::Error::config("agent_loop_spawn", "user_inbound_rx already taken")
    })?;
    let worker_system_inbound_tx = assembly.bus.system_inbound_tx.clone();
    let system_inbound_rx = assembly.bus.system_inbound_rx.take().ok_or_else(|| {
        beetle::Error::config("agent_loop_spawn", "system_inbound_rx already taken")
    })?;
    let worker_outbound_tx = assembly.bus.outbound_tx.clone();
    let handle = beetle::util::spawn_guarded_with_profile_handle(
        "agent_loop",
        STACK_AGENT_LOOP,
        agent_plan.core,
        agent_plan.role,
        move || {
            let mut agent_http = loop {
                match agent_network.open_http_client(HttpClientClass::Interactive) {
                    Ok(client) => break client,
                    Err(error) => match agent_http_open_failure_action(&error) {
                        AgentHttpOpenFailureAction::RetryAfterWifiRecovery => {
                            log::warn!(
                                "[{}] agent_loop waiting for WiFi before opening interactive HTTP client: {}",
                                tag,
                                error
                            );
                            beetle::state::set_last_error(&error);
                            beetle::platform::task_wdt::feed_current_task();
                            std::thread::sleep(std::time::Duration::from_secs(5));
                            beetle::platform::task_wdt::feed_current_task();
                        }
                        AgentHttpOpenFailureAction::RestartRuntime => {
                            log::error!(
                                "[{}] agent_loop open interactive HTTP client failed: {}",
                                tag,
                                error
                            );
                            beetle::state::set_last_error(&error);
                            beetle::runtime::request_restart_with_continuity_flush(
                                Arc::clone(&agent_platform),
                                None,
                                "agent_loop_http_init_failed",
                            );
                            return;
                        }
                    },
                }
            };
            log::info!("[{}] agent_loop running on Core1 thread", tag);
            if let Err(error) = run_agent_loop(
                agent_http.as_mut(),
                agent_worker_llm.as_ref(),
                agent_registry.as_ref(),
                agent_loop_config.as_ref(),
                worker_user_inbound_tx,
                user_inbound_rx,
                worker_system_inbound_tx,
                system_inbound_rx,
                worker_outbound_tx,
                typing_notifier,
            ) {
                log::warn!("[{}] agent_loop error: {}", tag, error);
                beetle::state::set_last_error(&error);
            }
            beetle::runtime::request_restart_with_continuity_flush(
                agent_platform,
                None,
                "agent_loop_exit",
            );
        },
    )
    .map_err(|error| beetle::Error::io("agent_loop_spawn", error))?;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_startup_memory_checkpoint("agent_loop_spawn");
    Ok(Some(handle))
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn run_runtime_guard_loop(
    platform: Arc<dyn Platform>,
    mut agent_handle: Option<beetle::util::TaskHandle>,
) {
    loop {
        beetle::platform::task_wdt::feed_current_task();
        if let Some(handle) = agent_handle.as_ref() {
            if handle.is_finished() {
                if let Some(done) = agent_handle.take() {
                    let _ = done.join();
                    log::error!("[{}] agent_loop exited; restart requested", TAG);
                    beetle::runtime::request_restart_with_continuity_flush(
                        Arc::clone(&platform),
                        None,
                        "agent_loop_join_exit",
                    );
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
        beetle::platform::task_wdt::feed_current_task();
        log::debug!("[{}] running v{}", TAG, VERSION);
    }
}

fn start_runtime_planes(
    platform: std::sync::Arc<dyn Platform>,
    config: Arc<AppConfig>,
    wifi_init_ok: bool,
) -> Option<Option<beetle::util::TaskHandle>> {
    beetle::state::set_boot_phase_active(true);
    let mut assembly = prepare_runtime_assembly(platform, config, wifi_init_ok)?;

    if let Err(error) = start_support_planes(&mut assembly) {
        log::error!("[{}] support plane startup failed: {}", TAG, error);
        app_runtime_support::record_startup_failure_and_request_restart(
            &assembly.runtime.platform,
            &error,
            "support_plane_startup_failed",
        );
        return None;
    }

    let agent_handle = match start_agent_plane(&mut assembly) {
        Ok(handle) => handle,
        Err(error) => {
            log::error!("[{}] agent plane startup failed: {}", TAG, error);
            app_runtime_support::record_startup_failure_and_request_restart(
                &assembly.runtime.platform,
                &error,
                "agent_plane_startup_failed",
            );
            return None;
        }
    };

    if let Err(error) = start_communication_planes(&mut assembly) {
        log::error!("[{}] communication plane startup failed: {}", TAG, error);
        app_runtime_support::record_startup_failure_and_request_restart(
            &assembly.runtime.platform,
            &error,
            "communication_plane_startup_failed",
        );
        return None;
    }
    beetle::state::set_boot_phase_active(false);
    Some(agent_handle)
}
