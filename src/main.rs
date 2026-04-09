//! 甲壳虫 (beetle) - ESP32-S3 firmware entry.
//! Firmware version is embedded for OTA and ops.
//! Startup order: NVS → SPIFFS → config → WiFi → memory/session stores → MessageBus → self-check → cron/heartbeat/sinks/dispatch/CLI → agent_loop.
//! ESP32: no graceful shutdown; process runs until power off.
#![allow(clippy::items_after_test_module)]

use beetle::bus::IngressKind;
use beetle::channels::connect_wss;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use beetle::constants::SOFTAP_DEFAULT_IPV4;
use beetle::memory::{MemoryStore, SessionStore};
#[cfg(feature = "feishu")]
use beetle::run_feishu_ws_loop;
use beetle::runtime::{execute_stream_http_op, spawn_planned, spawn_planned_handle, thread_plan};
use beetle::util::STACK_VOICE_CONTROL;
use beetle::util::{STACK_AGENT_LOOP, STACK_CHANNEL_SENDER, STACK_CHANNEL_WS, STACK_DISPATCH};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use beetle::Esp32Platform;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use beetle::LinuxPlatform;
use beetle::Platform;
use beetle::PlatformHttpClient;
use beetle::{
    parse_allowed_chat_ids, run_agent_loop, run_dispatch, send_chat_action, AppConfig, MessageBus,
    DEFAULT_CAPACITY,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use beetle::{DisplayChannelStatus, DisplayCommand, DisplayPressureLevel, DisplaySystemState};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use clap::Parser;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(all(
    feature = "config_api",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
use std::sync::RwLock;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use std::time::{Duration, Instant};

const TAG: &str = "beetle";
const VERSION: &str = env!("CARGO_PKG_VERSION");

type CapabilityPackageTextProvider = Arc<dyn Fn(&str, usize) -> Option<String> + Send + Sync>;

type HttpFactory = beetle::runtime::stream_http::HttpFactory;
struct VoiceEventChannel {
    wake_model_name: Option<String>,
    speak_capable: bool,
    tx: std::sync::mpsc::SyncSender<beetle::audio::voice_session::VoiceEvent>,
    rx: std::sync::mpsc::Receiver<beetle::audio::voice_session::VoiceEvent>,
}

struct TelegramTypingNotifier {
    token: String,
}

impl beetle::TypingNotifier for TelegramTypingNotifier {
    fn notify(&mut self, channel: &str, chat_id: &str, http: &mut dyn beetle::PlatformHttpClient) {
        if channel == beetle::CHANNEL_TELEGRAM {
            let _ = send_chat_action(http, &self.token, chat_id, "typing");
        }
    }
}

#[cfg(all(
    feature = "config_api",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
struct HttpServerSpawnContext {
    platform: Arc<dyn Platform>,
    tool_registry: Arc<beetle::tools::ToolRegistry>,
    channel_capability_registry: Arc<beetle::ChannelCapabilityRegistry>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    memory_store: Arc<dyn beetle::memory::MemoryStore + Send + Sync>,
    session_store: Arc<dyn beetle::memory::SessionStore + Send + Sync>,
    skill_prompt_cache: Arc<beetle::skills::SkillPromptCache>,
    inbound_tx: beetle::bus::InboundTx,
    shared_config: Arc<RwLock<AppConfig>>,
    llm_stream_enabled: bool,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    msg_id_cache: beetle::channels::QqMsgIdCache,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    qq_webhook_enabled: bool,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    qq_app_id: String,
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    qq_secret: String,
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

/// 启动自检：存储可读（memory 或 soul 至少其一成功）。失败返回 false，调用方应 log 并 return。
fn startup_self_check(memory_store: &dyn MemoryStore) -> bool {
    memory_store.get_memory().is_ok() || memory_store.get_soul().is_ok()
}

/// 首次启动或空存储：当 get_memory 与 get_soul 均失败时写入占位数据，使后续自检可过、业务可进（如引导配置）。
fn ensure_storage_ready(memory_store: &dyn MemoryStore) {
    let need_memory = memory_store.get_memory().is_err();
    let need_soul = memory_store.get_soul().is_err();
    let need_user = memory_store.get_user().is_err();
    if !need_memory && !need_soul && !need_user {
        return;
    }
    log::info!(
        "[{}] preparing default storage files memory_missing={} soul_missing={} user_missing={}",
        TAG,
        need_memory,
        need_soul,
        need_user
    );
    if need_memory {
        if let Err(e) = memory_store.set_memory("") {
            log::warn!("[{}] set_memory default failed: {}", TAG, e);
        }
    }
    if need_soul {
        if let Err(e) = memory_store.set_soul("") {
            log::warn!("[{}] set_soul default failed: {}", TAG, e);
        }
    }
    if need_user {
        if let Err(e) = memory_store.set_user("") {
            log::warn!("[{}] set_user default failed: {}", TAG, e);
        }
    }
}

fn bootstrap_pending_retry_into_inbound(
    pending_retry: &dyn beetle::memory::PendingRetryStore,
    user_inbound_tx: &beetle::bus::UserInboundTx,
    system_inbound_tx: &beetle::bus::SystemInboundTx,
) {
    let Ok(Some(msg)) = pending_retry.load_pending_retry() else {
        return;
    };
    if let Err(error) = pending_retry.clear_pending_retry() {
        log::warn!(
            "[main] pending_retry clear failed during bootstrap: {}",
            error
        );
    }
    let inbound_tx = match msg.ingress {
        IngressKind::User => user_inbound_tx,
        IngressKind::System => system_inbound_tx,
    };
    if let Err(error) = inbound_tx.try_send(msg) {
        log::warn!("[main] pending_retry bootstrap enqueue failed: {}", error);
    }
}

/// Telegram 流式编辑器：复用同一 TLS 连接，避免每次 edit 重新握手。
struct TelegramStreamEditor {
    token: String,
    create_http: Arc<HttpFactory>,
}

impl beetle::StreamEditor for TelegramStreamEditor {
    fn send_initial(&self, chat_id: &str, content: &str) -> beetle::Result<Option<String>> {
        execute_stream_http_op(
            self.create_http.as_ref(),
            "tg_stream_send_initial",
            |http| beetle::tg_send_and_get_id(http, &self.token, chat_id, content),
        )
    }
    fn edit(&self, chat_id: &str, message_id: &str, content: &str) -> beetle::Result<()> {
        execute_stream_http_op(self.create_http.as_ref(), "tg_stream_edit", |http| {
            beetle::tg_edit_message_text(http, &self.token, chat_id, message_id, content)
        })
    }
}

struct FeishuStreamEditor {
    app_id: String,
    app_secret: String,
    create_http: Arc<HttpFactory>,
    state: Mutex<beetle::FeishuTokenCache>,
}

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

#[cfg(all(
    feature = "config_api",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
fn spawn_http_config_server(
    ctx: HttpServerSpawnContext,
) -> std::io::Result<beetle::util::TaskHandle> {
    spawn_planned_handle("config_plane_watch", 6144, move || {
        if let Err(e) = beetle::platform::http_server::run(
            ctx.platform,
            ctx.tool_registry,
            ctx.channel_capability_registry,
            ctx.inbound_depth,
            ctx.outbound_depth,
            ctx.memory_store,
            ctx.session_store,
            ctx.skill_prompt_cache,
            ctx.inbound_tx,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            ctx.msg_id_cache,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            ctx.qq_webhook_enabled,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            ctx.qq_app_id,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            ctx.qq_secret,
            ctx.shared_config,
            ctx.llm_stream_enabled,
        ) {
            log::warn!("[{}] HTTP config API server error: {}", TAG, e);
        }
    })
}

fn spawn_voice_session_if_ready(
    platform: &Arc<dyn Platform>,
    config: &Arc<AppConfig>,
    baidu_token_cache: Option<&Arc<beetle::audio::baidu_token::BaiduTokenCache>>,
    user_inbound_tx: &beetle::bus::InboundTx,
    voice_event_tx_rx: &mut Option<VoiceEventChannel>,
) {
    let Some(VoiceEventChannel {
        wake_model_name,
        tx: voice_tx,
        rx: voice_rx,
        ..
    }) = voice_event_tx_rx.take()
    else {
        return;
    };
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let _ = (&wake_model_name, &voice_tx);
    let Some(audio_cfg) = config.audio.as_ref() else {
        return;
    };
    let vs_platform = Arc::clone(platform);
    let vs_audio = audio_cfg.clone();
    let vs_token = baidu_token_cache.cloned();
    let vs_pf = Arc::clone(platform);
    let vs_cfg = Arc::clone(config);
    let vs_make_http: Arc<
        dyn Fn() -> beetle::error::Result<Box<dyn beetle::PlatformHttpClient>> + Send + Sync,
    > = Arc::new(move || vs_pf.create_http_client(vs_cfg.as_ref()));
    let vs_inbound_tx = user_inbound_tx.clone();
    let vs_prompt = audio_cfg.wake_word.wake_prompt.clone();
    let spawned = match spawn_planned_handle("voice_session", STACK_VOICE_CONTROL, move || {
        beetle::audio::voice_session::run_voice_session(
            beetle::audio::voice_session::VoiceSessionConfig {
                platform: vs_platform,
                audio_cfg: vs_audio,
                baidu_token: vs_token,
                make_http: vs_make_http,
                inbound_tx: vs_inbound_tx,
                wake_prompt: vs_prompt,
            },
            voice_rx,
        );
    }) {
        Ok(_) => true,
        Err(error) => {
            let error = beetle::Error::io("voice_session_spawn", error);
            log::error!("[{}] voice_session spawn failed: {}", TAG, error);
            beetle::state::set_last_error(&error);
            false
        }
    };
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let _ = spawned;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if spawned {
        if let Some(model_name) = wake_model_name.as_deref() {
            platform.configure_wake_word(model_name, audio_cfg.microphone.sample_rate, voice_tx);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VoiceRuntimeCapabilities {
    speak_capable: bool,
    wake_capable: bool,
}

fn compute_voice_runtime_capabilities(
    audio_cfg: &beetle::config::AudioSegment,
    duplex_caps: beetle::AudioDuplexCapabilities,
    wake_model_present: bool,
    wake_supported_platform: bool,
    has_baidu_token: bool,
) -> VoiceRuntimeCapabilities {
    let speak_capable =
        audio_cfg.speaker.enabled && duplex_caps.has_speaker_output() && has_baidu_token;
    let wake_capable = if !wake_supported_platform
        || !audio_cfg.wake_word.enabled
        || !audio_cfg.microphone.enabled
        || !duplex_caps.has_microphone_input()
        || !wake_model_present
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
    use super::compute_voice_runtime_capabilities;
    use beetle::config::default_disabled_audio_segment;

    #[test]
    fn voice_sink_requires_tts_token_even_if_speaker_ready() {
        let mut audio = default_disabled_audio_segment();
        audio.enabled = true;
        audio.speaker.enabled = true;
        let caps = compute_voice_runtime_capabilities(
            &audio,
            beetle::AudioDuplexCapabilities::speaker_only(),
            false,
            false,
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
            true,
            true,
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
            true,
            true,
            false,
        );
        assert!(!caps.speak_capable);
        assert!(caps.wake_capable);
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    #[test]
    fn update_display_loop_cache_syncs_owned_dashboard_fields() {
        use super::{update_display_loop_cache, DisplayLoopState};
        use beetle::{DisplayChannelStatus, DisplayPressureLevel};

        let mut state = DisplayLoopState::default();
        let subtitle = Some("subtitle".to_string());
        let ip = "192.168.4.1".to_string();
        let channels = [
            DisplayChannelStatus {
                name: "qq",
                enabled: true,
                healthy: true,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "tg",
                enabled: false,
                healthy: false,
                consecutive_failures: 2,
            },
            DisplayChannelStatus {
                name: "fs",
                enabled: false,
                healthy: true,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "dt",
                enabled: false,
                healthy: true,
                consecutive_failures: 0,
            },
            DisplayChannelStatus {
                name: "wc",
                enabled: false,
                healthy: true,
                consecutive_failures: 0,
            },
        ];

        update_display_loop_cache(
            &mut state,
            &subtitle,
            &ip,
            &channels,
            Some(DisplayPressureLevel::Cautious),
            Some(42),
            Some(7),
            Some(9),
            Some(88),
        );

        assert_eq!(state.last_presence_subtitle, subtitle);
        assert_eq!(state.last_ip, ip);
        assert_eq!(state.last_channels[0], (true, true, 0));
        assert_eq!(state.last_channels[1], (false, false, 2));
        assert_eq!(state.last_pressure, Some(DisplayPressureLevel::Cautious));
        assert_eq!(state.last_heap, 42);
        assert_eq!(state.last_msg_in, 7);
        assert_eq!(state.last_msg_out, 9);
        assert_eq!(state.last_llm_ms, 88);
    }

    #[test]
    fn display_thread_stack_budget_is_large_enough_for_dashboard_render_path() {
        assert!(
            beetle::util::STACK_DISPLAY >= 12 * 1024,
            "display stack budget regressed below the verified 12KB floor",
        );
    }

    #[test]
    fn steady_state_state_change_uses_header_only_refresh() {
        use beetle::DisplaySystemState;

        assert_eq!(
            super::state_change_display_refresh_mode(Some(DisplaySystemState::Idle)),
            super::StateChangeDisplayRefreshMode::StateHeaderOnly,
        );
        assert_eq!(
            super::state_change_display_refresh_mode(None),
            super::StateChangeDisplayRefreshMode::FullDashboard,
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

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let wake_model_name = if audio_cfg.wake_word.enabled {
        beetle::config::wake_word_resolve_model(&audio_cfg.wake_word.keyword)
    } else {
        None
    };
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    let wake_model_name: Option<String> = None;

    let duplex_caps = platform.audio_duplex_capabilities();
    let capabilities = compute_voice_runtime_capabilities(
        audio_cfg,
        duplex_caps,
        wake_model_name.is_some(),
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        true,
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        false,
        baidu_token_cache.is_some(),
    );

    if !capabilities.speak_capable && !capabilities.wake_capable {
        return None;
    }

    let (vtx, vrx) = std::sync::mpsc::sync_channel(4);
    Some(VoiceEventChannel {
        wake_model_name,
        speak_capable: capabilities.speak_capable,
        tx: vtx,
        rx: vrx,
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
#[derive(Clone)]
struct DisplayLoopState {
    last_state: Option<DisplaySystemState>,
    last_presence_subtitle: Option<String>,
    last_ip: String,
    last_channels: [(bool, bool, u32); 5],
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

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
impl Default for DisplayLoopState {
    fn default() -> Self {
        Self {
            last_state: None,
            last_presence_subtitle: None,
            last_ip: String::new(),
            last_channels: [(false, false, 0); 5],
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

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn update_display_loop_cache(
    loop_state: &mut DisplayLoopState,
    presence_subtitle: &Option<String>,
    ip: &String,
    channels: &[DisplayChannelStatus; 5],
    pressure: Option<DisplayPressureLevel>,
    heap_percent: Option<u8>,
    msg_in: Option<u32>,
    msg_out: Option<u32>,
    llm_ms: Option<u32>,
) {
    loop_state
        .last_presence_subtitle
        .clone_from(presence_subtitle);
    loop_state.last_ip.clone_from(ip);
    for (i, ch) in channels.iter().enumerate() {
        loop_state.last_channels[i] = (ch.enabled, ch.healthy, ch.consecutive_failures);
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

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn build_display_channels(
    enabled: &str,
    snapshot: &beetle::orchestrator::ResourceSnapshot,
) -> [DisplayChannelStatus; 5] {
    [
        DisplayChannelStatus {
            name: "telegram",
            enabled: enabled == "telegram",
            healthy: snapshot.channels.telegram.healthy,
            consecutive_failures: snapshot.channels.telegram.consecutive_failures,
        },
        DisplayChannelStatus {
            name: "feishu",
            enabled: enabled == "feishu",
            healthy: snapshot.channels.feishu.healthy,
            consecutive_failures: snapshot.channels.feishu.consecutive_failures,
        },
        DisplayChannelStatus {
            name: "dingtalk",
            enabled: enabled == "dingtalk",
            healthy: snapshot.channels.dingtalk.healthy,
            consecutive_failures: snapshot.channels.dingtalk.consecutive_failures,
        },
        DisplayChannelStatus {
            name: "wecom",
            enabled: enabled == "wecom",
            healthy: snapshot.channels.wecom.healthy,
            consecutive_failures: snapshot.channels.wecom.consecutive_failures,
        },
        DisplayChannelStatus {
            name: "qq_channel",
            enabled: enabled == "qq_channel",
            healthy: snapshot.channels.qq_channel.healthy,
            consecutive_failures: snapshot.channels.qq_channel.consecutive_failures,
        },
    ]
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn update_display_error_flash(
    loop_state: &mut DisplayLoopState,
    metrics: &beetle::metrics::MetricsSnapshot,
) -> bool {
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
        true
    } else if loop_state.flash_active {
        loop_state.flash_active = false;
        false
    } else {
        false
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
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
        let _ = platform.fade_display_backlight(0, 100, 500);
        loop_state.backlight_off = false;
        loop_state.last_state = None;
        loop_state.last_presence_subtitle = None;
        loop_state.last_heap = 255;
        log::info!("[{}] display backlight woke up", TAG);
        return true;
    }
    if !loop_state.backlight_off
        && !any_change
        && loop_state.last_activity_at.elapsed() >= sleep_duration
    {
        let _ = platform.fade_display_backlight(100, 0, 500);
        loop_state.backlight_off = true;
        log::info!("[{}] display backlight auto-sleep", TAG);
        return true;
    }
    loop_state.backlight_off && !any_change
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
fn run_display_loop(platform: Arc<dyn Platform>, config: Arc<AppConfig>) {
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
        std::thread::sleep(Duration::from_secs(loop_state.refresh_secs));
        let snapshot = beetle::orchestrator::snapshot();
        let presence = beetle::runtime::inspect_platform_presence(
            platform.as_ref(),
            beetle::util::current_unix_secs(),
        );
        let pressure = match snapshot.pressure {
            beetle::orchestrator::PressureLevel::Normal => DisplayPressureLevel::Normal,
            beetle::orchestrator::PressureLevel::Cautious => DisplayPressureLevel::Cautious,
            beetle::orchestrator::PressureLevel::Critical => DisplayPressureLevel::Critical,
        };
        let sta_connected = beetle::platform::is_wifi_sta_connected();
        let ip = platform
            .wifi_sta_ip()
            .unwrap_or_else(|| SOFTAP_DEFAULT_IPV4.to_string());
        let display_projection = presence.display_projection(Some(ip.as_str()));
        let state = display_projection.state;
        let channels = build_display_channels(enabled, &snapshot);
        let heap_percent = heap_used_percent(&snapshot);

        let metrics = beetle::metrics::snapshot();
        let msg_in = metrics.messages_in as u32;
        let msg_out = metrics.messages_out as u32;
        let last_active = metrics.last_active_epoch_secs as u32;
        let llm_ms = metrics.llm_last_ms as u32;
        let uptime_secs = beetle::platform::time::uptime_secs();

        loop_state.busy_toggle = state == DisplaySystemState::Busy && !loop_state.busy_toggle;
        if state != DisplaySystemState::Busy {
            loop_state.busy_toggle = false;
        }

        let show_flash = update_display_error_flash(&mut loop_state, &metrics);
        let state_changed = loop_state.last_state != Some(state);
        let subtitle_changed =
            loop_state.last_presence_subtitle != display_projection.subtitle_override;
        let ip_changed = loop_state.last_ip.as_str() != ip.as_str();
        let channels_changed = channels.iter().enumerate().any(|(i, ch)| {
            loop_state.last_channels[i] != (ch.enabled, ch.healthy, ch.consecutive_failures)
        });
        let pressure_changed = loop_state.last_pressure.as_ref() != Some(&pressure);
        let heap_changed = loop_state.last_heap.abs_diff(heap_percent) >= 2;
        let msg_changed = msg_in != loop_state.last_msg_in || msg_out != loop_state.last_msg_out;
        let llm_changed = llm_ms != loop_state.last_llm_ms;
        let any_change = state_changed
            || ip_changed
            || channels_changed
            || pressure_changed
            || heap_changed
            || msg_changed
            || llm_changed
            || subtitle_changed
            || show_flash;

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

        if state_changed {
            let presence_subtitle = display_projection.subtitle_override.clone();
            let ip_owned = ip.clone();
            let cmd = match state_change_display_refresh_mode(loop_state.last_state) {
                StateChangeDisplayRefreshMode::FullDashboard => DisplayCommand::RefreshDashboard {
                    state,
                    presence_subtitle,
                    wifi_connected: sta_connected,
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
            if let Err(e) = platform.display_command(cmd) {
                log::warn!("[{}] display refresh failed: {}", TAG, e);
            }
            loop_state.last_state = Some(state);
            update_display_loop_cache(
                &mut loop_state,
                &display_projection.subtitle_override,
                &ip_owned,
                &channels,
                Some(pressure),
                Some(heap_percent),
                Some(msg_in),
                Some(msg_out),
                Some(llm_ms),
            );
            loop_state.refresh_secs = compute_refresh_secs(
                state,
                loop_state.backlight_off,
                &loop_state.last_activity_at,
            );
            continue;
        }

        if ip_changed || subtitle_changed {
            let presence_subtitle = display_projection.subtitle_override.clone();
            let ip_owned = ip.clone();
            let _ = platform.display_command(DisplayCommand::UpdateIp {
                ip: ip_owned.clone(),
                presence_subtitle,
                uptime_secs,
            });
            update_display_loop_cache(
                &mut loop_state,
                &display_projection.subtitle_override,
                &ip_owned,
                &channels,
                None,
                None,
                None,
                None,
                None,
            );
        }
        if channels_changed {
            let last_presence_subtitle = loop_state.last_presence_subtitle.clone();
            let last_ip = loop_state.last_ip.clone();
            let _ = platform.display_command(DisplayCommand::UpdateChannels { channels });
            update_display_loop_cache(
                &mut loop_state,
                &last_presence_subtitle,
                &last_ip,
                &channels,
                None,
                None,
                None,
                None,
                None,
            );
        }
        if pressure_changed || heap_changed || msg_changed || llm_changed || show_flash {
            let last_presence_subtitle = loop_state.last_presence_subtitle.clone();
            let last_ip = loop_state.last_ip.clone();
            let _ = platform.display_command(DisplayCommand::UpdatePressure {
                level: pressure,
                heap_percent,
                messages_in: msg_in,
                messages_out: msg_out,
                last_active_epoch_secs: last_active,
                llm_last_ms: llm_ms,
                error_flash: show_flash,
            });
            update_display_loop_cache(
                &mut loop_state,
                &last_presence_subtitle,
                &last_ip,
                &channels,
                Some(pressure),
                Some(heap_percent),
                Some(msg_in),
                Some(msg_out),
                Some(llm_ms),
            );
        }
        loop_state.refresh_secs = compute_refresh_secs(
            state,
            loop_state.backlight_off,
            &loop_state.last_activity_at,
        );
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
    let config = beetle::bootstrap::load_config(platform);
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
    let presence = beetle::runtime::inspect_platform_presence(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    let initiative = beetle::runtime::inspect_platform_initiative(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    let os_closure = beetle::runtime::inspect_beetle_os_closure(&presence, &initiative);
    let runtime_mode = presence.runtime_mode;
    let soul_kernel = presence.soul_kernel.clone();
    let supervisor = presence.supervisor.clone();
    let release = presence.release.clone();

    if json {
        let payload = serde_json::json!({
            "version": VERSION,
            "enabled_channel": config.enabled_channel,
            "chat_id": chat_id,
            "recent_turn": recent_turn,
            "initiative": initiative,
            "os_closure": os_closure,
            "presence": presence,
            "runtime_mode": runtime_mode,
            "soul_kernel": soul_kernel,
            "supervisor": supervisor,
            "release": release,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("beetle v{}", VERSION);
        println!("Enabled channel: {}", config.enabled_channel);
        println!(
            "Presence: {} ({})",
            presence.state.as_str(),
            presence.rationale
        );
        println!(
            "Initiative: action={} ready={} rationale={}",
            initiative.action.as_str(),
            initiative.ready,
            initiative.rationale
        );
        println!(
            "OS closure: ready={} planes={}/{} summary={}",
            os_closure.ready, os_closure.ready_planes, os_closure.plane_count, os_closure.summary
        );
        if !os_closure.outstanding.is_empty() {
            println!(
                "OS closure outstanding: {}",
                os_closure.outstanding.join(", ")
            );
        }
        if let Some(reason) = initiative.suppression_reason {
            println!("Initiative suppressed by: {}", reason.as_str());
        }
        println!("Runtime mode: {}", runtime_mode.current_mode.as_str());
        println!(
            "Soul kernel: ready={} safe_mode_readable={} degraded={} key_memory={}",
            soul_kernel.minimum_viable,
            soul_kernel.safe_mode_minimum_readable,
            soul_kernel.degraded,
            soul_kernel.key_memory_count
        );
        if let Some(release) = release.as_ref() {
            println!(
                "Release: managed={} rollout_state={} rollback_available={} current={} rollback={}",
                release.managed,
                release.rollout_state_label(),
                release.rollback_available,
                release
                    .current
                    .as_ref()
                    .map(|pointer| pointer.name.as_str())
                    .unwrap_or("none"),
                release
                    .rollback
                    .as_ref()
                    .map(|pointer| pointer.name.as_str())
                    .unwrap_or("none")
            );
        } else {
            println!("Release: none");
        }
        if !soul_kernel.degradation_reasons.is_empty() {
            println!(
                "Soul kernel degradation: {}",
                soul_kernel.degradation_reasons.join(", ")
            );
        }
        if let Some(snapshot) = supervisor {
            println!(
                "Supervisor: pid={} alive={} state={} restarts={}",
                snapshot.state.supervisor_pid,
                snapshot.supervisor_alive,
                snapshot.state.current_state,
                snapshot.state.restart_count
            );
            println!(
                "Agent: pid={} alive={} state={}",
                snapshot
                    .state
                    .agent
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                snapshot.agent_alive,
                snapshot.state.agent.state
            );
            if let Some(reason) = snapshot.state.safe_mode_reason.as_deref() {
                println!("Safe mode: true ({})", reason);
            } else {
                println!("Safe mode: false");
            }
            if let Some(code) = snapshot.state.agent.last_exit_code {
                println!("Agent last exit code: {}", code);
            }
            if let Some(signal) = snapshot.state.agent.last_exit_signal {
                println!("Agent last exit signal: {}", signal);
            }
            if !snapshot.state.agent.last_exit_reason.trim().is_empty() {
                println!(
                    "Agent last exit reason: {}",
                    snapshot.state.agent.last_exit_reason
                );
            }
        } else {
            println!("Supervisor: none");
        }
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
                        "Recent turn delivery: waiting_notice_sent={} progress_updates_sent={} partial_updates_sent={} tool_outbound_intents_seen={} tool_visible_updates_sent={} explicit_outbound_sent={} tool_outbound_suppressed={} current_primary_delivered={} finalize_streamed={}",
                        ledger.delivery.waiting_notice_sent,
                        ledger.delivery.progress_updates_sent,
                        ledger.delivery.partial_updates_sent,
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
    match presence.supervisor.as_ref() {
        Some(snapshot) => {
            println!(
                "✓ Supervisor status readable (pid={} alive={} agent_alive={})",
                snapshot.state.supervisor_pid, snapshot.supervisor_alive, snapshot.agent_alive
            );
        }
        None => println!("⚠ Supervisor status not found"),
    }
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
    if memory_store.get_memory().is_ok() || memory_store.get_soul().is_ok() {
        println!("✓ Memory store accessible");
    } else {
        println!("⚠ Memory store not accessible");
    }

    println!("\nDiagnostics complete.");
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_restart_command(_platform: &Arc<dyn Platform>) {
    if std::path::Path::new("/etc/systemd/system/beetle.service").exists() {
        match std::process::Command::new("systemctl")
            .args(["restart", "beetle"])
            .status()
        {
            Ok(status) if status.success() => {
                println!("beetle service restart requested.");
                return;
            }
            Ok(status) => {
                eprintln!(
                    "systemctl restart beetle failed with exit status: {}",
                    status
                );
                std::process::exit(status.code().unwrap_or(1));
            }
            Err(e) => {
                eprintln!("failed to run systemctl restart beetle: {}", e);
                std::process::exit(1);
            }
        }
    }

    match beetle::runtime::linux_supervisor::request_restart() {
        Ok(true) => {
            println!("beetle supervisor restart requested.");
        }
        Ok(false) => {
            eprintln!(
                "restart requires a running beetle supervisor or a systemd-managed beetle service."
            );
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("failed to request beetle supervisor restart: {}", error);
            std::process::exit(1);
        }
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_stop_command(_platform: &Arc<dyn Platform>) {
    if std::path::Path::new("/etc/systemd/system/beetle.service").exists() {
        match std::process::Command::new("systemctl")
            .args(["stop", "beetle"])
            .status()
        {
            Ok(status) if status.success() => {
                println!("beetle service stop requested.");
                return;
            }
            Ok(status) => {
                eprintln!("systemctl stop beetle failed with exit status: {}", status);
                std::process::exit(status.code().unwrap_or(1));
            }
            Err(e) => {
                eprintln!("failed to run systemctl stop beetle: {}", e);
                std::process::exit(1);
            }
        }
    }

    match beetle::runtime::linux_supervisor::request_stop() {
        Ok(true) => {
            println!("beetle supervisor stop requested.");
        }
        Ok(false) => {
            eprintln!(
                "stop requires a running beetle supervisor or a systemd-managed beetle service."
            );
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("failed to request beetle supervisor stop: {}", error);
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

    match beetle::runtime::linux_supervisor::request_rollback() {
        Ok(true) => {
            println!("beetle supervisor rollback requested.");
            return;
        }
        Ok(false) => {}
        Err(error) => {
            eprintln!("failed to request beetle supervisor rollback: {}", error);
            std::process::exit(1);
        }
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
fn log_start_banner(config_path: Option<&str>) {
    log::info!("========================================");
    log::info!("  甲壳虫 beetle v{}", VERSION);
    log::info!("========================================");
    if let Some(path) = config_path {
        log::info!("[{}] using config file: {}", TAG, path);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn run_linux_agent_entry(platform: Arc<dyn Platform>) {
    let (config, wifi_init_ok) = beetle::bootstrap::bootstrap_config_and_wifi(&platform);
    run_app(platform, config, wifi_init_ok);
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

    let platform: Arc<dyn Platform> = Arc::new(LinuxPlatform::new());
    if let Err(e) = platform.init() {
        eprintln!("[{}] platform init failed: {}", TAG, e);
        std::process::exit(1);
    }

    match cli.command {
        Commands::Supervise {
            config: config_path,
        } => {
            log_start_banner(config_path.as_deref());
            if let Err(error) = beetle::runtime::linux_supervisor::run_supervisor(
                Arc::clone(&platform),
                config_path,
            ) {
                eprintln!("[{}] supervisor failed: {}", TAG, error);
                std::process::exit(1);
            }
        }
        Commands::Agent {
            config: config_path,
        } => {
            log_start_banner(config_path.as_deref());
            run_linux_agent_entry(platform);
        }
        Commands::Run {
            config: config_path,
        } => {
            log::warn!(
                "[{}] `beetle run` is deprecated; use `beetle supervise` instead",
                TAG
            );
            log_start_banner(config_path.as_deref());
            if let Err(error) = beetle::runtime::linux_supervisor::run_supervisor(
                Arc::clone(&platform),
                config_path,
            ) {
                eprintln!("[{}] supervisor failed: {}", TAG, error);
                std::process::exit(1);
            }
        }
        Commands::Config { action } => {
            handle_config_command(&platform, action);
        }
        Commands::Status { json, chat_id } => {
            handle_status_command(&platform, json, chat_id.as_deref());
        }
        Commands::Restart => {
            handle_restart_command(&platform);
        }
        Commands::Stop => {
            handle_stop_command(&platform);
        }
        Commands::Doctor => {
            handle_doctor_command(&platform);
        }
        Commands::Release { action } => match action {
            ReleaseAction::Status { json } => handle_release_status_command(&platform, json),
            ReleaseAction::Rollback => handle_release_rollback_command(&platform),
        },
        Commands::Version => {
            println!("beetle v{}", VERSION);
        }
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

    let (config, wifi_init_ok) = beetle::bootstrap::bootstrap_config_and_wifi(&platform);
    run_app(platform, config, wifi_init_ok);
}

/// 启动编排：存储与总线 → 自检 → 后台任务与通道 → agent 循环与 flush。与 main 解耦便于单文件内可读性。
fn run_app(platform: std::sync::Arc<dyn Platform>, config: Arc<AppConfig>, wifi_init_ok: bool) {
    beetle::state::set_boot_phase_active(true);
    beetle::orchestrator::register_memory_snapshot_provider(Arc::new({
        let p = Arc::clone(&platform);
        move || p.memory_snapshot()
    }));
    let config_store = platform.config_store();
    let memory_system_kind = platform.memory_system_kind();
    let resolve_locale_ui: Arc<dyn Fn() -> beetle::i18n::Locale + Send + Sync> = Arc::new({
        let cs = Arc::clone(&config_store);
        move || beetle::i18n::Locale::from_storage(&beetle::config::get_locale(cs.as_ref()))
    });
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    let skill_prompt_cache = Arc::new(beetle::skills::SkillPromptCache::new(
        Arc::clone(&skill_meta_store),
        Arc::clone(&skill_storage),
        8192,
    ));
    let _ = skill_prompt_cache.refresh();
    let memory_store: Arc<dyn MemoryStore + Send + Sync> = platform.memory_store();
    let long_term_memory_store: Arc<dyn beetle::memory::LongTermMemoryStore + Send + Sync> =
        platform.long_term_memory_store();
    let continuity_capsule_store: Arc<dyn beetle::memory::ContinuityCapsuleStore + Send + Sync> =
        platform.continuity_capsule_store();
    let long_term_memory_extraction_state_store: Arc<
        dyn beetle::memory::LongTermMemoryExtractionStateStore + Send + Sync,
    > = platform.long_term_memory_extraction_state_store();
    ensure_storage_ready(memory_store.as_ref());
    if let Ok(s) = memory_store.get_memory() {
        log::info!("[{}] memory len={}", TAG, s.len());
    } else {
        log::warn!("[{}] memory read failed or empty", TAG);
    }
    if let Ok(s) = memory_store.get_soul() {
        log::info!("[{}] soul len={}", TAG, s.len());
    } else {
        log::warn!("[{}] soul read failed", TAG);
    }
    if let Ok(s) = memory_store.get_user() {
        log::info!("[{}] user len={}", TAG, s.len());
    } else {
        log::warn!("[{}] user read failed", TAG);
    }

    let session_store: Arc<dyn SessionStore + Send + Sync> = platform.session_store();
    let pending_retry_store: Arc<dyn beetle::memory::PendingRetryStore + Send + Sync> =
        platform.pending_retry_store();
    let task_store: Arc<dyn beetle::task::TaskStore + Send + Sync> = platform.task_store();
    let task_run_store: Arc<dyn beetle::task_execution::TaskRunStore + Send + Sync> =
        platform.task_run_store();
    let task_artifact_store: Arc<dyn beetle::task_execution::TaskArtifactStore + Send + Sync> =
        platform.task_artifact_store();
    let task_execution_ledger_store: Arc<
        dyn beetle::task_execution::TaskExecutionLedgerStore + Send + Sync,
    > = platform.task_execution_ledger_store();
    let task_learning_store: Arc<dyn beetle::task_execution::TaskLearningStore + Send + Sync> =
        platform.task_learning_store();
    let execution_state_store: Arc<dyn beetle::memory::ExecutionStateStore + Send + Sync> =
        platform.execution_state_store();
    let self_model_store: Arc<dyn beetle::memory::SelfModelStore + Send + Sync> =
        platform.self_model_store();
    let self_authored_core_store: Arc<dyn beetle::memory::SelfAuthoredCoreStore + Send + Sync> =
        platform.self_authored_core_store();
    let core_revision_ledger_store: Arc<dyn beetle::memory::CoreRevisionLedgerStore + Send + Sync> =
        platform.core_revision_ledger_store();
    let relationship_constitution_store: Arc<
        dyn beetle::memory::RelationshipConstitutionStore + Send + Sync,
    > = platform.relationship_constitution_store();
    let relationship_portfolio_store: Arc<
        dyn beetle::memory::RelationshipPortfolioStore + Send + Sync,
    > = platform.relationship_portfolio_store();
    let world_sense_store: Arc<dyn beetle::memory::WorldSenseStore + Send + Sync> =
        platform.world_sense_store();
    let autonomy_strategy_store: Arc<dyn beetle::memory::AutonomyStrategyStore + Send + Sync> =
        platform.autonomy_strategy_store();
    let outer_voice_store: Arc<dyn beetle::memory::OuterVoiceStore + Send + Sync> =
        platform.outer_voice_store();
    let inner_life_store: Arc<dyn beetle::memory::InnerLifeStore + Send + Sync> =
        platform.inner_life_store();
    let self_continuity_store: Arc<dyn beetle::memory::SelfContinuityStore + Send + Sync> =
        platform.self_continuity_store();
    let relationship_topology_store: Arc<
        dyn beetle::memory::RelationshipTopologyStore + Send + Sync,
    > = platform.relationship_topology_store();
    let private_doc_store: Arc<dyn beetle::memory::PrivateDocStore + Send + Sync> =
        platform.private_doc_store();
    let private_garden_store: Arc<dyn beetle::memory::PrivateGardenStore + Send + Sync> =
        platform.private_garden_store();
    let mental_privacy_store: Arc<dyn beetle::memory::MentalPrivacyStore + Send + Sync> =
        platform.mental_privacy_store();
    let important_message_store: Arc<dyn beetle::memory::ImportantMessageStore + Send + Sync> =
        platform.important_message_store();
    let remind_at_store: Arc<dyn beetle::memory::RemindAtStore + Send + Sync> =
        platform.remind_at_store();
    let session_summary_store: Arc<dyn beetle::memory::SessionSummaryStore + Send + Sync> =
        platform.session_summary_store();
    let turn_ledger_store: Arc<dyn beetle::memory::TurnLedgerStore + Send + Sync> =
        platform.turn_ledger_store();
    let emotion_signal_store = Arc::new(beetle::memory::MemoryEmotionSignalStore::new());
    let soul_kernel_report = beetle::runtime::ensure_platform_soul_kernel_recovery(
        platform.as_ref(),
        beetle::util::current_unix_secs(),
    );
    if soul_kernel_report.restore_attempted {
        log::info!(
            "[{}] soul_kernel recovery action={:?} restored_snapshots={} restored_layers={} degraded_after={}",
            TAG,
            soul_kernel_report.action,
            soul_kernel_report.restored_snapshots,
            soul_kernel_report.restored_layers.len(),
            soul_kernel_report.status_after.degraded,
        );
    } else {
        log::info!(
            "[{}] soul_kernel ready={} safe_mode_readable={} degraded={}",
            TAG,
            soul_kernel_report.status_after.minimum_viable,
            soul_kernel_report.status_after.safe_mode_minimum_readable,
            soul_kernel_report.status_after.degraded,
        );
    }

    let (bus, user_inbound_rx, outbound_rx) = MessageBus::new(DEFAULT_CAPACITY);
    let (system_inbound_tx, system_inbound_rx, system_inbound_depth) =
        beetle::bus::new_inbound_channel(DEFAULT_CAPACITY);
    log::info!(
        "[{}] MessageBus created (capacity {})",
        TAG,
        DEFAULT_CAPACITY
    );
    let user_inbound_depth = Arc::clone(&bus.inbound_depth);
    let outbound_depth = Arc::clone(&bus.outbound_depth);
    let user_inbound_tx = bus.inbound_tx;
    let outbound_tx = bus.outbound_tx;
    bootstrap_pending_retry_into_inbound(
        pending_retry_store.as_ref(),
        &user_inbound_tx,
        &system_inbound_tx,
    );
    let qq_msg_id_cache: beetle::channels::QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));
    #[allow(unused_variables)]
    let (mut registry, baidu_token_cache) = beetle::build_default_registry(
        &config,
        beetle::DefaultRegistryDeps {
            platform: Arc::clone(&platform),
            remind_at_store: Arc::clone(&remind_at_store),
            session_store: Arc::clone(&session_store),
            memory_store: Arc::clone(&memory_store),
            long_term_memory_store: Arc::clone(&long_term_memory_store),
            turn_ledger_store: Arc::clone(&turn_ledger_store),
            private_garden_store: Arc::clone(&private_garden_store),
            config_store: platform.config_store(),
        },
    );
    // ── Audio init + voice runtime preparation (after MessageBus) ──────────
    beetle::bootstrap::init_audio_if_enabled(&platform, &config);
    let mut voice_event_tx_rx =
        build_voice_event_channel(&platform, &config, baidu_token_cache.as_ref());
    let voice_channel_enabled = matches!(
        voice_event_tx_rx.as_ref(),
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
    let capability_package_runtime_capabilities =
        Arc::new(beetle::build_capability_package_runtime_capabilities(
            channel_capability_registry.as_ref(),
            config.llm_stream,
        ));
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

    if !startup_self_check(memory_store.as_ref()) {
        log::error!(
            "[{}] startup self-check failed: storage not readable (get_memory and get_soul both failed)",
            TAG
        );
        return;
    }
    let wifi_init_status = if wifi_init_ok { "ok" } else { "failed" };
    let sta_up = beetle::platform::is_wifi_sta_connected();
    let spiffs_info = platform
        .spiffs_usage()
        .map(|(total, used)| format!("{} free", total.saturating_sub(used)))
        .unwrap_or_else(|| "N/A".to_string());
    log::info!(
        "[{}] startup self-check ok (storage readable, wifi_init={}, sta_up={}, spiffs={})",
        TAG,
        wifi_init_status,
        sta_up,
        spiffs_info
    );
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::orchestrator::log_baseline();

    #[cfg(feature = "config_api")]
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        let shared_runtime_config = Arc::new(RwLock::new((*config).clone()));
        match spawn_http_config_server(HttpServerSpawnContext {
            platform: Arc::clone(&platform),
            tool_registry: Arc::clone(&registry),
            channel_capability_registry: Arc::clone(&channel_capability_registry),
            inbound_depth: Arc::clone(&user_inbound_depth),
            outbound_depth: Arc::clone(&outbound_depth),
            memory_store: Arc::clone(&memory_store),
            session_store: Arc::clone(&session_store),
            skill_prompt_cache: Arc::clone(&skill_prompt_cache),
            inbound_tx: user_inbound_tx.clone(),
            shared_config: Arc::clone(&shared_runtime_config),
            llm_stream_enabled: config.llm_stream,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            msg_id_cache: Arc::clone(&qq_msg_id_cache),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            qq_webhook_enabled: !config.qq_channel_app_id.trim().is_empty()
                && !config.qq_channel_secret.trim().is_empty(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            qq_app_id: config.qq_channel_app_id.clone(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            qq_secret: config.qq_channel_secret.clone(),
        }) {
            Ok(_) => {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                log::info!(
                    "[{}] HTTP config API server started (ESP WiFi config API; bootstrap SoftAP at {})",
                    TAG,
                    SOFTAP_DEFAULT_IPV4
                );
                #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
                log::info!(
                    "[{}] HTTP config API server started (config API on LAN; BEETLE_CONFIG_HTTP_LISTEN, default 0.0.0.0:80)",
                    TAG
                );
            }
            Err(error) => {
                let error = beetle::Error::io("config_plane_spawn", error);
                log::error!("[{}] HTTP config API server spawn failed: {}", TAG, error);
                beetle::state::set_last_error(&error);
                beetle::runtime::request_restart_with_continuity_flush(
                    Arc::clone(&platform),
                    None,
                    "config_plane_spawn_failed",
                );
                return;
            }
        }
    }

    beetle::bg_timer::run_bg_timer(beetle::bg_timer::BgTimerContext {
        system_inbound_tx: system_inbound_tx.clone(),
        resolve_locale: Arc::clone(&resolve_locale_ui),
        platform: Arc::clone(&platform),
        version: VERSION,
        read_heartbeat: Box::new(|| beetle::platform::read_heartbeat_file().unwrap_or_default()),
        user_inbound_depth: Arc::clone(&user_inbound_depth),
        system_inbound_depth: Arc::clone(&system_inbound_depth),
        outbound_depth: Arc::clone(&outbound_depth),
        session_store: Arc::clone(&session_store),
        memory_system_kind,
        autonomy_strategy_store: Arc::clone(&autonomy_strategy_store),
        self_authored_core_store: Arc::clone(&self_authored_core_store),
        self_continuity_store: Arc::clone(&self_continuity_store),
        relationship_portfolio_store: Arc::clone(&relationship_portfolio_store),
        relationship_topology_store: Arc::clone(&relationship_topology_store),
        memory_store: Some(Arc::clone(&memory_store)),
        sensor_watch: device_capability_registry
            .is_mounted(beetle::DEVICE_CAPABILITY_SENSOR)
            .then(|| beetle::cron::SensorWatchContext {
                platform: Arc::clone(&platform),
                devices: config.hardware_devices.clone(),
                i2c_sensors: config.i2c_sensors.clone(),
            }),
        remind_store: Arc::clone(&remind_at_store),
        task_store: Arc::clone(&task_store),
    });
    // bg_timer: merged cron + heartbeat + remind into one thread (saves ~20KB SRAM).

    // 出站前等待 STA + 编排器初始化：须在 `create_http_client` 成功判定之前，以便 Linux 在 HTTP 桩返回 Err 时仍能 init orchestrator。
    if wifi_init_ok {
        beetle::platform::wait_for_network_ready();
    }
    beetle::orchestrator::init();

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if platform.display_available() {
        let display_platform = Arc::clone(&platform);
        let display_config = Arc::clone(&config);
        let plan = thread_plan("display");
        let _ = beetle::util::spawn_guarded_with_profile_handle(
            "display",
            beetle::util::STACK_DISPLAY,
            plan.core,
            plan.role,
            move || run_display_loop(display_platform, display_config),
        );
    }

    #[allow(unused_mut)]
    let (mut sinks, mut channel_rx_set) =
        beetle::channels::build_channel_sinks(config.as_ref(), &qq_msg_id_cache);
    // F8: 启动进度条 stage=3（channel sinks 后）
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if platform.display_available() {
        let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 3 });
    }

    // Register VoiceSink so dispatch routes channel="voice" replies to the voice session thread.
    if let Some(VoiceEventChannel {
        speak_capable: true,
        tx: ref vtx,
        ..
    }) = voice_event_tx_rx.as_ref()
    {
        sinks.register(
            beetle::constants::VOICE_CHANNEL_NAME,
            Box::new(beetle::channels::VoiceSink::new(vtx.clone())),
        );
    }

    let sinks = Arc::new(sinks);
    let enabled_channel = config.enabled_channel.as_str();
    log::info!(
        "[{}] enabled_channel='{}'",
        TAG,
        if enabled_channel.is_empty() {
            "(none)"
        } else {
            enabled_channel
        }
    );

    #[cfg(feature = "feishu")]
    if let Some(ref c) = channel_rx_set.feishu {
        let tx = user_inbound_tx.clone();
        let id = c.app_id.clone();
        let sec = c.app_secret.clone();
        let allowed = parse_allowed_chat_ids(&config.feishu_allowed_chat_ids);
        let pending = Arc::clone(&pending_retry_store);
        let pf = Arc::clone(&platform);
        let cfg = Arc::clone(&config);
        // WSS + JSON: 16KB on ESP; Linux uses `STACK_CHANNEL_WS` (64KB embedded-class, rustls).
        spawn_planned("feishu_ws", STACK_CHANNEL_WS, move || {
            run_feishu_ws_loop(
                id,
                sec,
                allowed,
                tx,
                pending.as_ref(),
                move || pf.create_http_client(cfg.as_ref()),
                connect_wss,
            )
        });
        log::info!("[{}] Feishu WS loop started", TAG);
    } else if enabled_channel == "feishu" {
        #[cfg(feature = "feishu")]
        log::warn!(
            "[{}] Feishu WS not started: app_id or app_secret empty (check channels config)",
            TAG
        );
    }

    if enabled_channel == "qq_channel" {
        if let Some(ref c) = channel_rx_set.qq_channel {
            if !c.app_id.trim().is_empty() && !c.app_secret.trim().is_empty() {
                let qq_tx = user_inbound_tx.clone();
                let qq_id = c.app_id.clone();
                let qq_sec = c.app_secret.clone();
                let qq_cache_ws = std::sync::Arc::clone(&qq_msg_id_cache);
                let qq_pending = Arc::clone(&pending_retry_store);
                let pf = Arc::clone(&platform);
                let cfg = Arc::clone(&config);
                // QQ WS: 16KB on ESP; Linux `STACK_CHANNEL_WS` (64KB) — 16KB overflows rustls.
                spawn_planned("qq_ws", STACK_CHANNEL_WS, move || {
                    beetle::run_qq_ws_loop(
                        qq_id,
                        qq_sec,
                        qq_tx,
                        qq_cache_ws,
                        qq_pending.as_ref(),
                        move || pf.create_http_client(cfg.as_ref()),
                        connect_wss,
                    )
                });
                log::info!("[{}] QQ WS loop started", TAG);
            }
        }
    }

    let mut agent_handle: Option<beetle::util::TaskHandle> = None;

    // Agent / flush 与各通道工厂均经 `create_http_client`，与代理配置一致。
    if platform.create_http_client(config.as_ref()).is_ok() {
        let outbound_rx_for_dispatch = outbound_rx;
        let sinks_clone = Arc::clone(&sinks);
        if let Err(error) = spawn_planned_handle("dispatch", STACK_DISPATCH, move || {
            run_dispatch(outbound_rx_for_dispatch, sinks_clone)
        }) {
            let error = beetle::Error::io("dispatch_spawn", error);
            log::error!("[{}] dispatch spawn failed: {}", TAG, error);
            beetle::state::set_last_error(&error);
            beetle::runtime::request_restart_with_continuity_flush(
                Arc::clone(&platform),
                None,
                "dispatch_spawn_failed",
            );
            return;
        }

        if enabled_channel == "telegram" && !config.tg_token.trim().is_empty() {
            let tg_token = config.tg_token.clone();
            let tg_allowed = parse_allowed_chat_ids(&config.tg_allowed_chat_ids);
            let tg_group_activation = config.tg_group_activation.clone();
            let tg_inbound_tx = user_inbound_tx.clone();
            let tg_outbound_tx = outbound_tx.clone();
            let tg_session_store = Arc::clone(&session_store);
            let tg_pending = Arc::clone(&pending_retry_store);
            let tg_inbound_depth = Arc::clone(&user_inbound_depth);
            let tg_outbound_depth = Arc::clone(&outbound_depth);
            let tg_config_store = Arc::clone(&config_store);
            let tg_resolve_locale = Arc::clone(&resolve_locale_ui);
            let pf = Arc::clone(&platform);
            let cfg = Arc::clone(&config);
            // tg_poll calls rustls on Linux; use same budget as other channel HTTPS threads.
            spawn_planned("tg_poll", STACK_CHANNEL_SENDER, move || {
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
                    tg_config_store,
                    tg_resolve_locale,
                    move || pf.create_http_client(cfg.as_ref()),
                )
            });
            log::info!("[{}] Telegram poll loop started", TAG);
        }

        if let Some(ref bus_cfg) = config.i2c_bus {
            if let Err(e) = platform.init_i2c(bus_cfg) {
                log::warn!(
                    "[{}] I2C bus init failed (devices will be unavailable): {}",
                    TAG,
                    e
                );
            }
        }

        let worker_llm: Arc<dyn beetle::LlmClient + Send + Sync> = Arc::from(
            beetle::build_llm_clients(&config, Arc::clone(&resolve_locale_ui)),
        );

        // ── Voice session thread (speaker / wake runtime) ───────────────────
        spawn_voice_session_if_ready(
            &platform,
            &config,
            baidu_token_cache.as_ref(),
            &user_inbound_tx,
            &mut voice_event_tx_rx,
        );

        let get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync> =
            Arc::new(move || skill_prompt_cache.get());
        let get_capability_package_text: CapabilityPackageTextProvider = Arc::new({
            let state_fs = platform.state_fs();
            let runtime_capabilities = Arc::clone(&capability_package_runtime_capabilities);
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
        let session_max = config.session_max_messages.clamp(1, 128) as usize;
        let agent_user_inbound_tx = user_inbound_tx;
        let agent_system_inbound_tx = system_inbound_tx;
        let worker_user_inbound_tx = agent_user_inbound_tx;
        let typing_notifier: Option<Box<dyn beetle::TypingNotifier>> = channel_capability_registry
            .get(config.enabled_channel.as_str())
            .filter(|entry| entry.enabled && entry.contract.supports_typing_or_chat_action)
            .and_then(|entry| match entry.id {
                beetle::CHANNEL_TELEGRAM if !config.tg_token.trim().is_empty() => {
                    Some(Box::new(TelegramTypingNotifier {
                        token: config.tg_token.clone(),
                    }) as Box<dyn beetle::TypingNotifier>)
                }
                _ => None,
            });

        // 流式编辑器：根据 enabled_channel 选择对应通道的 StreamEditor 实现。
        let stream_editor: Option<Arc<dyn beetle::StreamEditor + Send + Sync>> = if config
            .llm_stream
            && channel_capability_registry
                .get(config.enabled_channel.as_str())
                .map(|entry| entry.enabled && entry.contract.supports_stream_edit)
                .unwrap_or(false)
        {
            let pf = Arc::clone(&platform);
            let cfg = Arc::clone(&config);
            let make_http: Arc<
                dyn Fn() -> beetle::Result<Box<dyn beetle::PlatformHttpClient>> + Send + Sync,
            > = Arc::new(move || pf.create_interactive_http_client(cfg.as_ref()));
            match config.enabled_channel.as_str() {
                beetle::CHANNEL_TELEGRAM if !config.tg_token.trim().is_empty() => {
                    Some(Arc::new(TelegramStreamEditor {
                        token: config.tg_token.clone(),
                        create_http: Arc::clone(&make_http),
                    })
                        as Arc<dyn beetle::StreamEditor + Send + Sync>)
                }
                beetle::CHANNEL_FEISHU if !config.feishu_app_id.trim().is_empty() => {
                    Some(Arc::new(FeishuStreamEditor {
                        app_id: config.feishu_app_id.clone(),
                        app_secret: config.feishu_app_secret.clone(),
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
            .map(|_| Arc::<str>::from(config.enabled_channel.as_str()));
        let agent_strategy = if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
            beetle::agent::AgentRunStrategy::Embedded
        } else {
            beetle::agent::AgentRunStrategy::LinuxEnhanced
        };
        let agent_config = Arc::new(beetle::AgentLoopConfig {
            memory_store: Arc::clone(&memory_store),
            long_term_memory_store: Arc::clone(&long_term_memory_store),
            continuity_capsule_store: Arc::clone(&continuity_capsule_store),
            long_term_memory_extraction_state_store: Arc::clone(
                &long_term_memory_extraction_state_store,
            ),
            session_store: Arc::clone(&session_store),
            session_summary_store: Arc::clone(&session_summary_store),
            execution_state_store: Arc::clone(&execution_state_store),
            self_model_store: Arc::clone(&self_model_store),
            self_authored_core_store: Arc::clone(&self_authored_core_store),
            core_revision_ledger_store: Arc::clone(&core_revision_ledger_store),
            relationship_constitution_store: Arc::clone(&relationship_constitution_store),
            relationship_portfolio_store: Arc::clone(&relationship_portfolio_store),
            world_sense_store: Arc::clone(&world_sense_store),
            autonomy_strategy_store: Arc::clone(&autonomy_strategy_store),
            outer_voice_store: Arc::clone(&outer_voice_store),
            inner_life_store: Arc::clone(&inner_life_store),
            self_continuity_store: Arc::clone(&self_continuity_store),
            relationship_topology_store: Arc::clone(&relationship_topology_store),
            private_doc_store: Arc::clone(&private_doc_store),
            private_garden_store: Arc::clone(&private_garden_store),
            mental_privacy_store: Arc::clone(&mental_privacy_store),
            turn_ledger_store: Arc::clone(&turn_ledger_store),
            skill_storage: Arc::clone(&skill_storage),
            memory_system_kind,
            get_skill_descriptions,
            get_capability_package_text,
            session_max_messages: session_max,
            tg_group_activation: Arc::<str>::from(config.tg_group_activation.as_str()),
            important_message_store: Arc::clone(&important_message_store),
            emotion_signal_store: Arc::clone(&emotion_signal_store)
                as Arc<dyn beetle::memory::EmotionSignalStore + Send + Sync>,
            remind_store: Arc::clone(&remind_at_store),
            task_store: Arc::clone(&task_store),
            task_run_store: Arc::clone(&task_run_store),
            task_artifact_store: Arc::clone(&task_artifact_store),
            task_execution_ledger_store: Arc::clone(&task_execution_ledger_store),
            task_learning_store: Arc::clone(&task_learning_store),
            pending_retry: Arc::clone(&pending_retry_store),
            channel_capability_registry: Arc::clone(&channel_capability_registry),
            strategy: agent_strategy,
            llm_stream: config.llm_stream,
            stream_editor,
            stream_editor_channel,
            resolve_locale: std::sync::Arc::clone(&resolve_locale_ui),
        });
        #[cfg(feature = "cli")]
        {
            let cli_ctx = beetle::cli::CliContext::new(
                Arc::clone(&config),
                Arc::clone(&config_store),
                Arc::clone(&memory_store),
                Arc::clone(&session_store),
                Arc::clone(&platform),
                Arc::clone(&registry),
                Arc::clone(&channel_capability_registry),
                Arc::clone(&capability_package_runtime_capabilities),
                config.llm_stream,
                Some(Arc::clone(&user_inbound_depth)),
                Some(Arc::clone(&outbound_depth)),
            );
            spawn_planned("cli_repl", 8192, move || {
                let reader = std::io::BufReader::new(std::io::stdin());
                beetle::cli::run_repl(cli_ctx, reader);
            });
            log::info!("[{}] CLI REPL started (stdin)", TAG);
        }

        let create_http: Arc<
            dyn Fn() -> beetle::Result<Box<dyn PlatformHttpClient>> + Send + Sync,
        > = Arc::new({
            let pf = Arc::clone(&platform);
            let cfg = Arc::clone(&config);
            move || pf.create_http_client(cfg.as_ref())
        });
        beetle::channels::spawn_sender_threads(&mut channel_rx_set, &config.tg_token, create_http);

        // F8: 启动进度条 stage=4（agent 前）
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
        if platform.display_available() {
            let _ = platform.display_command(DisplayCommand::UpdateBootProgress { stage: 4 });
        }

        let agent_plan = thread_plan("agent_loop");
        let tag = TAG;
        let agent_registry = Arc::clone(&registry);
        let agent_worker_llm = Arc::clone(&worker_llm);
        let agent_platform = Arc::clone(&platform);
        let agent_config_for_thread = Arc::clone(&config);
        let agent_loop_config = Arc::clone(&agent_config);
        let worker_system_inbound_tx = agent_system_inbound_tx.clone();
        let worker_outbound_tx = outbound_tx.clone();
        agent_handle = match beetle::util::spawn_guarded_with_profile_handle(
            "agent_loop",
            STACK_AGENT_LOOP,
            agent_plan.core,
            agent_plan.role,
            move || {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                beetle::platform::task_wdt::register_current_task_to_task_wdt();
                let mut agent_http =
                    match agent_platform.create_http_client(agent_config_for_thread.as_ref()) {
                        Ok(c) => c,
                        Err(e) => {
                            log::error!("[{}] agent_loop create_http_client failed: {}", tag, e);
                            beetle::state::set_last_error(&e);
                            beetle::runtime::request_restart_with_continuity_flush(
                                Arc::clone(&agent_platform),
                                None,
                                "agent_loop_http_init_failed",
                            );
                            return;
                        }
                    };
                log::info!("[{}] agent_loop running on Core1 thread", tag);
                if let Err(e) = run_agent_loop(
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
                    log::warn!("[{}] agent_loop error: {}", tag, e);
                    beetle::state::set_last_error(&e);
                }
                beetle::runtime::request_restart_with_continuity_flush(
                    agent_platform,
                    None,
                    "agent_loop_exit",
                );
            },
        ) {
            Ok(handle) => Some(handle),
            Err(error) => {
                let error = beetle::Error::io("agent_loop_spawn", error);
                log::error!("[{}] agent_loop spawn failed: {}", TAG, error);
                beetle::state::set_last_error(&error);
                beetle::runtime::request_restart_with_continuity_flush(
                    Arc::clone(&platform),
                    None,
                    "agent_loop_spawn_failed",
                );
                return;
            }
        };
    } else {
        log::warn!(
                "[{}] HTTP client not available (create_http_client failed): dispatch, agent, Telegram poll, and outbound sender threads were not started. On Linux, ensure ureq/rustls stack and network; see dev-docs/beetle-os-plan.md and dev-docs/architecture-and-code.md.",
                TAG
            );
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::platform::task_wdt::register_current_task_to_task_wdt();
    beetle::state::set_boot_phase_active(false);

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
