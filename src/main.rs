//! 甲壳虫 (beetle) - ESP32-S3 firmware entry.
//! Firmware version is embedded for OTA and ops.
//! Startup order: NVS → SPIFFS → config → WiFi → memory/session stores → MessageBus → self-check → cron/heartbeat/sinks/dispatch/CLI → agent_loop.
//! ESP32: no graceful shutdown; process runs until power off.
use beetle::channels::connect_wss;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use beetle::constants::SOFTAP_DEFAULT_IPV4;
use beetle::memory::{MemoryStore, SessionStore};
#[cfg(feature = "feishu")]
use beetle::run_feishu_ws_loop;
use beetle::runtime::{execute_stream_http_op, spawn_planned, thread_plan};
use beetle::util::{STACK_AGENT_LOOP, STACK_CHANNEL_SENDER, STACK_CHANNEL_WS};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use beetle::Esp32Platform;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use beetle::LinuxPlatform;
use beetle::Platform;
use beetle::PlatformHttpClient;
use beetle::{
    parse_allowed_chat_ids, run_dispatch, run_system_agent_loop, run_user_agent_loop,
    send_chat_action, AppConfig, MessageBus, DEFAULT_CAPACITY,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
use beetle::{DisplayChannelStatus, DisplayCommand, DisplayPressureLevel, DisplaySystemState};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use clap::Parser;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TAG: &str = "beetle";
const VERSION: &str = env!("CARGO_PKG_VERSION");

type HttpFactory = beetle::runtime::stream_http::HttpFactory;

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
    let need_defaults = memory_store.get_memory().is_err() && memory_store.get_soul().is_err();
    if !need_defaults {
        return;
    }
    log::info!(
        "[{}] first boot or empty storage: writing default memory/soul",
        TAG
    );
    if let Err(e) = memory_store.set_memory("") {
        log::warn!("[{}] set_memory default failed: {}", TAG, e);
    }
    if let Err(e) = memory_store.set_soul("") {
        log::warn!("[{}] set_soul default failed: {}", TAG, e);
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

/// 飞书流式编辑器：复用 HTTP + tenant_access_token（与 sender 线程相同 TTL 策略）。
struct FeishuStreamState {
    token: Option<(String, Instant)>,
}

/// 飞书 tenant_access_token 缓存 TTL（与 `feishu/send` sender 一致：2h − 300s）。
const FEISHU_STREAM_TOKEN_TTL: Duration = Duration::from_secs(7200 - 300);

struct FeishuStreamEditor {
    app_id: String,
    app_secret: String,
    create_http: Arc<HttpFactory>,
    state: Mutex<FeishuStreamState>,
}

impl FeishuStreamEditor {
    fn ensure_token(
        &self,
        state: &mut FeishuStreamState,
        http: &mut Box<dyn PlatformHttpClient>,
    ) -> beetle::Result<String> {
        let need_refresh = match &state.token {
            Some((_, acquired)) => acquired.elapsed() >= FEISHU_STREAM_TOKEN_TTL,
            None => true,
        };
        if need_refresh {
            let t = beetle::feishu_acquire_token(http, &self.app_id, &self.app_secret).ok_or_else(
                || beetle::Error::config("feishu_stream", "failed to acquire tenant_token"),
            )?;
            state.token = Some((t.clone(), Instant::now()));
            Ok(t)
        } else {
            state.token.as_ref().map(|(t, _)| t.clone()).ok_or_else(|| {
                beetle::Error::config("feishu_stream", "token missing after refresh")
            })
        }
    }
}

impl beetle::StreamEditor for FeishuStreamEditor {
    fn send_initial(&self, chat_id: &str, content: &str) -> beetle::Result<Option<String>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        execute_stream_http_op(
            self.create_http.as_ref(),
            "feishu_stream_send_initial",
            |http| {
                let token = self.ensure_token(&mut state, http)?;
                let r = beetle::feishu_send_and_get_id(http, &token, chat_id, content);
                if r.is_err() {
                    state.token = None;
                }
                r
            },
        )
    }

    fn edit(&self, _chat_id: &str, message_id: &str, content: &str) -> beetle::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        execute_stream_http_op(self.create_http.as_ref(), "feishu_stream_edit", |http| {
            let token = self.ensure_token(&mut state, http)?;
            let r = beetle::feishu_edit_message(http, &token, message_id, content);
            if r.is_err() {
                state.token = None;
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
fn handle_status_command(platform: &Arc<dyn Platform>, json: bool) {
    let (config, _) = beetle::bootstrap::bootstrap_config_and_wifi(platform);

    if json {
        println!("{{");
        println!("  \"version\": \"{}\",", VERSION);
        println!("  \"enabled_channel\": \"{}\"", config.enabled_channel);
        println!("}}");
    } else {
        println!("beetle v{}", VERSION);
        println!("Enabled channel: {}", config.enabled_channel);
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn handle_doctor_command(platform: &Arc<dyn Platform>) {
    println!("Running beetle diagnostics...\n");

    println!("✓ Platform initialized");

    let (config, wifi_ok) = beetle::bootstrap::bootstrap_config_and_wifi(platform);
    if wifi_ok {
        println!("✓ WiFi configuration loaded");
    } else {
        println!("⚠ WiFi configuration not available");
    }

    println!("✓ Config loaded (channel: {})", config.enabled_channel);

    let memory_store = platform.memory_store();
    if memory_store.get_memory().is_ok() || memory_store.get_soul().is_ok() {
        println!("✓ Memory store accessible");
    } else {
        println!("⚠ Memory store not accessible");
    }

    println!("\nDiagnostics complete.");
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn main() {
    use beetle::commands::{Cli, Commands};

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
        Commands::Run {
            config: config_path,
        } => {
            log::info!("========================================");
            log::info!("  甲壳虫 beetle v{}", VERSION);
            log::info!("========================================");
            if let Some(path) = config_path {
                log::info!("[{}] using config file: {}", TAG, path);
            }
            let (config, wifi_init_ok) = beetle::bootstrap::bootstrap_config_and_wifi(&platform);
            run_app(platform, config, wifi_init_ok);
        }
        Commands::Config { action } => {
            handle_config_command(&platform, action);
        }
        Commands::Status { json } => {
            handle_status_command(&platform, json);
        }
        Commands::Doctor => {
            handle_doctor_command(&platform);
        }
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
    beetle::orchestrator::register_memory_snapshot_provider(Arc::new({
        let p = Arc::clone(&platform);
        move || p.memory_snapshot()
    }));
    let config_store = platform.config_store();
    let resolve_locale_ui: Arc<dyn Fn() -> beetle::i18n::Locale + Send + Sync> = Arc::new({
        let cs = Arc::clone(&config_store);
        move || beetle::i18n::Locale::from_storage(&beetle::config::get_locale(cs.as_ref()))
    });
    let skill_storage = platform.skill_storage();
    let skill_meta_store = platform.skill_meta_store();
    let memory_store: Arc<dyn MemoryStore + Send + Sync> = platform.memory_store();
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
    let task_continuation_store: Arc<dyn beetle::memory::TaskContinuationStore + Send + Sync> =
        platform.task_continuation_store();
    let important_message_store: Arc<dyn beetle::memory::ImportantMessageStore + Send + Sync> =
        platform.important_message_store();
    let remind_at_store: Arc<dyn beetle::memory::RemindAtStore + Send + Sync> =
        platform.remind_at_store();
    let session_summary_store: Arc<dyn beetle::memory::SessionSummaryStore + Send + Sync> =
        platform.session_summary_store();
    let emotion_signal_store = Arc::new(beetle::memory::MemoryEmotionSignalStore::new());

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
    let qq_msg_id_cache: beetle::channels::QqMsgIdCache = Arc::new(Mutex::new(HashMap::new()));

    // ── Audio init + wake-word registration (ESP only, after MessageBus) ───
    // voice_event_tx is created here so wake_word::configure can arm the signal;
    // voice_session thread is spawned later (after build_default_registry provides BaiduTokenCache).
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    let mut voice_event_tx_rx: Option<(
        std::sync::mpsc::SyncSender<beetle::audio::voice_session::VoiceEvent>,
        std::sync::mpsc::Receiver<beetle::audio::voice_session::VoiceEvent>,
    )>;
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    {
        beetle::bootstrap::esp_init_audio(&platform, &config);
        voice_event_tx_rx = None;
        if let Some(audio_cfg) = config.audio.as_ref() {
            if audio_cfg.enabled && audio_cfg.wake_word.enabled {
                if let Some(model_name) =
                    beetle::config::wake_word_resolve_model(&audio_cfg.wake_word.keyword)
                {
                    let (vtx, vrx) = std::sync::mpsc::sync_channel(4);
                    beetle::platform::wake_word::configure(model_name.as_str(), vtx.clone());
                    voice_event_tx_rx = Some((vtx, vrx));
                }
            }
        }
    }

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
    {
        let platform_http = Arc::clone(&platform);
        let inc = Arc::clone(&user_inbound_depth);
        let out = Arc::clone(&outbound_depth);
        let memory_http = Arc::clone(&memory_store);
        let session_http = Arc::clone(&session_store);
        let http_inbound_tx = user_inbound_tx.clone();
        let http_qq_cache = Arc::clone(&qq_msg_id_cache);
        let qq_wh_enabled = !config.qq_channel_app_id.trim().is_empty()
            && !config.qq_channel_secret.trim().is_empty();
        let qq_app_id = config.qq_channel_app_id.clone();
        let qq_secret = config.qq_channel_secret.clone();
        let initial_config = (*config).clone();
        spawn_planned("http_server", 6144, move || {
            if let Err(e) = beetle::platform::http_server::run(
                platform_http,
                inc,
                out,
                memory_http,
                session_http,
                http_inbound_tx,
                http_qq_cache,
                qq_wh_enabled,
                qq_app_id,
                qq_secret,
                initial_config,
            ) {
                log::warn!("[{}] HTTP config API server error: {}", TAG, e);
            }
        });
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        log::info!(
            "[{}] HTTP config API server started (SoftAP: {})",
            TAG,
            SOFTAP_DEFAULT_IPV4
        );
        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        log::info!(
            "[{}] HTTP config API server started (config API on LAN; BEETLE_CONFIG_HTTP_LISTEN, default 0.0.0.0:80)",
            TAG
        );
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
        memory_store: Some(Arc::clone(&memory_store)),
        sensor_watch: Some(beetle::cron::SensorWatchContext {
            platform: Arc::clone(&platform),
            devices: config.hardware_devices.clone(),
            i2c_sensors: config.i2c_sensors.clone(),
        }),
        remind_store: Arc::clone(&remind_at_store),
    });
    // bg_timer: merged cron + heartbeat + remind into one thread (saves ~20KB SRAM).

    // 出站前等待 STA + 编排器初始化：须在 `create_http_client` 成功判定之前，以便 Linux 在 HTTP 桩返回 Err 时仍能 init orchestrator。
    beetle::platform::wait_for_network_ready();
    beetle::orchestrator::init();

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32", target_os = "linux"))]
    if platform.display_available() {
        let display_platform = Arc::clone(&platform);
        let display_config = Arc::clone(&config);
        let plan = thread_plan("display");
        let _ = beetle::util::spawn_guarded_with_profile_handle(
            "display",
            6144,
            plan.core,
            plan.role,
            move || {
                let enabled = display_config.enabled_channel.as_str();
                // Dirty-region cache: skip SPI when nothing changed.
                let mut last_state: Option<DisplaySystemState> = None;
                let mut last_ip = String::new();
                let mut last_channels: [(bool, bool, u32); 5] = [(false, false, 0); 5]; // F5: +consecutive_failures
                let mut last_pressure: Option<DisplayPressureLevel> = None;
                let mut last_heap: u8 = 255; // 255 forces first-round refresh
                let mut last_msg_in: u32 = u32::MAX; // force first-round refresh
                let mut last_msg_out: u32 = u32::MAX;
                let mut last_llm_ms: u32 = 0; // F6: LLM 延迟 dirty cache

                // F2: 自适应刷新频率
                let mut refresh_secs: u64 = beetle::constants::DISPLAY_REFRESH_IDLE_SECS;

                // F4: Busy 呼吸动画
                let mut busy_toggle: bool = false;

                // F7: 错误闪烁指示
                let mut last_error_total: u64 = 0;
                let mut flash_active: bool = false;

                // Auto-sleep: backlight off after N seconds of no activity.
                let sleep_timeout = display_config
                    .display
                    .as_ref()
                    .map(|d| d.sleep_timeout_secs)
                    .unwrap_or(0);
                let sleep_enabled =
                    sleep_timeout > 0 && display_platform.display_backlight_available();
                let sleep_duration = std::time::Duration::from_secs(sleep_timeout as u64);
                let mut last_activity_at = std::time::Instant::now();
                let mut backlight_off = false;

                loop {
                    std::thread::sleep(std::time::Duration::from_secs(refresh_secs));
                    let snapshot = beetle::orchestrator::snapshot();
                    let pressure = match snapshot.pressure {
                        beetle::orchestrator::PressureLevel::Normal => DisplayPressureLevel::Normal,
                        beetle::orchestrator::PressureLevel::Cautious => {
                            DisplayPressureLevel::Cautious
                        }
                        beetle::orchestrator::PressureLevel::Critical => {
                            DisplayPressureLevel::Critical
                        }
                    };
                    let sta_connected = beetle::platform::is_wifi_sta_connected();
                    let busy = snapshot.active_agent_tasks > 0
                        || snapshot.active_http_count > 0
                        || snapshot.inbound_depth > 0
                        || snapshot.outbound_depth > 0;
                    let state =
                        if snapshot.pressure == beetle::orchestrator::PressureLevel::Critical {
                            DisplaySystemState::Fault
                        } else if !sta_connected {
                            DisplaySystemState::NoWifi
                        } else if snapshot.audio_recording {
                            DisplaySystemState::Recording
                        } else if snapshot.audio_playing {
                            DisplaySystemState::Playing
                        } else if busy {
                            DisplaySystemState::Busy
                        } else {
                            DisplaySystemState::Idle
                        };
                    let ip = display_platform
                        .wifi_sta_ip()
                        .unwrap_or_else(|| SOFTAP_DEFAULT_IPV4.to_string());

                    // F5: 通道状态含 consecutive_failures
                    let channels = [
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
                    ];
                    let heap_percent = heap_used_percent(&snapshot);

                    // Read metrics for footer display.
                    let m_snap = beetle::metrics::snapshot();
                    let msg_in = m_snap.messages_in as u32;
                    let msg_out = m_snap.messages_out as u32;
                    let last_active = m_snap.last_active_epoch_secs as u32;
                    let llm_ms = m_snap.llm_last_ms as u32; // F6

                    // F3: uptime
                    let uptime_secs = beetle::platform::time::uptime_secs();

                    // F4: Busy 呼吸动画翻转
                    if state == DisplaySystemState::Busy {
                        busy_toggle = !busy_toggle;
                    } else {
                        busy_toggle = false;
                    }

                    // F7: 错误闪烁 — 检测新错误
                    let current_error_total = m_snap.errors_agent_chat
                        + m_snap.errors_agent_context
                        + m_snap.errors_tool_execute
                        + m_snap.errors_llm_request
                        + m_snap.errors_llm_parse
                        + m_snap.errors_channel_dispatch
                        + m_snap.errors_session_append
                        + m_snap.errors_tls_admission
                        + m_snap.errors_other;
                    let error_flash = if current_error_total > last_error_total {
                        last_error_total = current_error_total;
                        true
                    } else {
                        last_error_total = current_error_total;
                        false
                    };
                    // flash_active tracks: this round flash, next round auto-reset
                    let show_flash = if error_flash {
                        flash_active = true;
                        true
                    } else if flash_active {
                        flash_active = false; // auto-reset after one cycle
                        false
                    } else {
                        false
                    };

                    // Detect any dirty region change as "activity".
                    let state_changed = last_state != Some(state);
                    let ip_changed = last_ip.as_str() != ip.as_str();
                    let channels_changed = channels.iter().enumerate().any(|(i, ch)| {
                        last_channels[i] != (ch.enabled, ch.healthy, ch.consecutive_failures)
                    });
                    let pressure_changed = last_pressure.as_ref() != Some(&pressure);
                    let heap_changed = last_heap.abs_diff(heap_percent) >= 2;
                    let msg_changed = msg_in != last_msg_in || msg_out != last_msg_out;
                    let llm_changed = llm_ms != last_llm_ms; // F6
                    let any_change = state_changed
                        || ip_changed
                        || channels_changed
                        || pressure_changed
                        || heap_changed
                        || msg_changed
                        || llm_changed
                        || show_flash;

                    if any_change {
                        last_activity_at = std::time::Instant::now();
                    }

                    // Auto-sleep logic: wake on change, sleep on idle timeout.
                    if sleep_enabled {
                        if any_change && backlight_off {
                            // F1: Wake up with fade
                            let _ = display_platform.fade_display_backlight(0, 100, 500);
                            backlight_off = false;
                            last_state = None; // force full RefreshDashboard
                            last_heap = 255;
                            log::info!("[{}] display backlight woke up", TAG);
                            continue;
                        }
                        if !backlight_off
                            && !any_change
                            && last_activity_at.elapsed() >= sleep_duration
                        {
                            // F1: Go to sleep with fade
                            let _ = display_platform.fade_display_backlight(100, 0, 500);
                            backlight_off = true;
                            log::info!("[{}] display backlight auto-sleep", TAG);
                            continue;
                        }
                        if backlight_off {
                            // Still sleeping, no change — skip all rendering.
                            continue;
                        }
                    }

                    // State changed (icon + title area) → full refresh + update all cache
                    if state_changed {
                        let cmd = DisplayCommand::RefreshDashboard {
                            state,
                            wifi_connected: sta_connected,
                            ip_address: Some(ip.clone()),
                            channels: channels.clone(),
                            pressure: pressure.clone(),
                            heap_percent,
                            messages_in: msg_in,
                            messages_out: msg_out,
                            last_active_epoch_secs: last_active,
                            uptime_secs,
                            busy_phase: busy_toggle,
                            llm_last_ms: llm_ms,
                            error_flash: show_flash,
                        };
                        if let Err(e) = display_platform.display_command(cmd) {
                            log::warn!("[{}] display refresh failed: {}", TAG, e);
                        }
                        last_state = Some(state);
                        last_ip.clear();
                        last_ip.push_str(&ip);
                        for (i, ch) in channels.iter().enumerate() {
                            last_channels[i] = (ch.enabled, ch.healthy, ch.consecutive_failures);
                        }
                        last_pressure = Some(pressure.clone());
                        last_heap = heap_percent;
                        last_msg_in = msg_in;
                        last_msg_out = msg_out;
                        last_llm_ms = llm_ms;

                        // F2: 计算下一轮刷新间隔
                        refresh_secs =
                            compute_refresh_secs(state, backlight_off, &last_activity_at);
                        continue;
                    }

                    // State unchanged → partial updates per dirty region
                    if ip_changed {
                        let _ = display_platform.display_command(DisplayCommand::UpdateIp {
                            ip: ip.clone(),
                            uptime_secs,
                        });
                        last_ip.clear();
                        last_ip.push_str(&ip);
                    }
                    if channels_changed {
                        let _ = display_platform.display_command(DisplayCommand::UpdateChannels {
                            channels: channels.clone(),
                        });
                        for (i, ch) in channels.iter().enumerate() {
                            last_channels[i] = (ch.enabled, ch.healthy, ch.consecutive_failures);
                        }
                    }
                    // 2% hysteresis on heap to avoid progress bar flicker
                    if pressure_changed || heap_changed || msg_changed || llm_changed || show_flash
                    {
                        let _ = display_platform.display_command(DisplayCommand::UpdatePressure {
                            level: pressure.clone(),
                            heap_percent,
                            messages_in: msg_in,
                            messages_out: msg_out,
                            last_active_epoch_secs: last_active,
                            llm_last_ms: llm_ms,
                            error_flash: show_flash,
                        });
                        last_pressure = Some(pressure.clone());
                        last_heap = heap_percent;
                        last_msg_in = msg_in;
                        last_msg_out = msg_out;
                        last_llm_ms = llm_ms;
                    }

                    // F2: 计算下一轮刷新间隔
                    refresh_secs = compute_refresh_secs(state, backlight_off, &last_activity_at);
                }
            },
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
    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    if let Some((ref vtx, _)) = voice_event_tx_rx {
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

    let mut user_agent_handle: Option<std::thread::JoinHandle<()>> = None;
    let mut system_agent_handle: Option<std::thread::JoinHandle<()>> = None;

    // Agent / flush 与各通道工厂均经 `create_http_client`，与代理配置一致。
    if platform.create_http_client(config.as_ref()).is_ok() {
        let outbound_rx_for_dispatch = outbound_rx;
        let sinks_clone = Arc::clone(&sinks);
        spawn_planned("dispatch", 4096, move || {
            run_dispatch(outbound_rx_for_dispatch, sinks_clone)
        });

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
        #[allow(unused_variables)]
        let (registry, baidu_token_cache) = beetle::build_default_registry(
            &config,
            Arc::clone(&platform),
            Arc::clone(&remind_at_store),
            Arc::clone(&session_summary_store),
            Arc::clone(&session_store),
            Arc::clone(&memory_store),
            platform.config_store(),
        );
        let registry = Arc::new(registry);

        // ── Voice session thread (ESP only) ─────────────────────────────────
        #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
        if let Some((_, voice_rx)) = voice_event_tx_rx.take() {
            if let (Some(audio_cfg), Some(ref bt_cache)) =
                (config.audio.as_ref(), &baidu_token_cache)
            {
                let vs_platform = Arc::clone(&platform);
                let vs_audio = audio_cfg.clone();
                let vs_token = Arc::clone(bt_cache);
                let vs_pf = Arc::clone(&platform);
                let vs_cfg = Arc::clone(&config);
                let vs_make_http: Arc<
                    dyn Fn() -> beetle::error::Result<Box<dyn beetle::PlatformHttpClient>>
                        + Send
                        + Sync,
                > = Arc::new(move || vs_pf.create_http_client(vs_cfg.as_ref()));
                let vs_inbound_tx = user_inbound_tx.clone();
                let vs_prompt = audio_cfg.wake_word.wake_prompt.clone();
                spawn_planned(
                    "voice_session",
                    beetle::util::STACK_VOICE_SESSION,
                    move || {
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
                    },
                );
            }
        }

        let tool_specs: Arc<[beetle::llm::ToolSpec]> = registry.tool_specs_for_api(32768).into();
        let skill_meta_store_fn = Arc::clone(&skill_meta_store);
        let skill_storage_fn = Arc::clone(&skill_storage);
        let get_skill_descriptions: Arc<dyn Fn() -> String + Send + Sync> = Arc::new(move || {
            beetle::skills::build_skill_descriptions_for_system_prompt(
                skill_meta_store_fn.as_ref(),
                skill_storage_fn.as_ref(),
                8192,
            )
        });
        let session_max = config.session_max_messages.clamp(1, 128) as usize;
        let agent_user_inbound_tx = user_inbound_tx;
        let agent_system_inbound_tx = system_inbound_tx;
        let user_worker_user_inbound_tx = agent_user_inbound_tx.clone();
        let system_worker_user_inbound_tx = agent_user_inbound_tx;
        let tg_token_for_typing = config.tg_token.clone();
        let typing_notifier: beetle::TypingNotifier = Box::new(move |ch, cid, http| {
            if ch == "telegram" {
                let _ = send_chat_action(http, &tg_token_for_typing, cid, "typing");
            }
        });

        // 流式编辑器：根据 enabled_channel 选择对应通道的 StreamEditor 实现。
        let stream_editor: Option<Arc<dyn beetle::StreamEditor + Send + Sync>> =
            if config.llm_stream {
                let pf = Arc::clone(&platform);
                let cfg = Arc::clone(&config);
                let make_http: Arc<
                    dyn Fn() -> beetle::Result<Box<dyn beetle::PlatformHttpClient>> + Send + Sync,
                > = Arc::new(move || pf.create_http_client(cfg.as_ref()));
                match config.enabled_channel.as_str() {
                    "telegram" if !config.tg_token.trim().is_empty() => {
                        Some(Arc::new(TelegramStreamEditor {
                            token: config.tg_token.clone(),
                            create_http: Arc::clone(&make_http),
                        })
                            as Arc<dyn beetle::StreamEditor + Send + Sync>)
                    }
                    "feishu" if !config.feishu_app_id.trim().is_empty() => {
                        Some(Arc::new(FeishuStreamEditor {
                            app_id: config.feishu_app_id.clone(),
                            app_secret: config.feishu_app_secret.clone(),
                            create_http: Arc::clone(&make_http),
                            state: Mutex::new(FeishuStreamState { token: None }),
                        })
                            as Arc<dyn beetle::StreamEditor + Send + Sync>)
                    }
                    _ => None,
                }
            } else {
                None
            };
        let agent_config = Arc::new(beetle::AgentLoopConfig {
            memory_store: Arc::clone(&memory_store),
            session_store: Arc::clone(&session_store),
            session_summary_store: Arc::clone(&session_summary_store),
            tool_specs,
            get_skill_descriptions,
            session_max_messages: session_max,
            tg_group_activation: Arc::<str>::from(config.tg_group_activation.as_str()),
            task_continuation: Arc::clone(&task_continuation_store),
            task_continuation_max_rounds: 0u32,
            important_message_store: Arc::clone(&important_message_store),
            emotion_signal_store: Arc::clone(&emotion_signal_store)
                as Arc<dyn beetle::memory::EmotionSignalStore + Send + Sync>,
            pending_retry: Arc::clone(&pending_retry_store),
            llm_stream: config.llm_stream,
            stream_editor,
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

        let user_plan = thread_plan("agent_user_loop");
        let tag = TAG;
        let user_agent_registry = Arc::clone(&registry);
        let user_agent_worker_llm = Arc::clone(&worker_llm);
        let user_agent_platform = Arc::clone(&platform);
        let user_agent_config_for_thread = Arc::clone(&config);
        let user_agent_loop_config = Arc::clone(&agent_config);
        let user_worker_system_inbound_tx = agent_system_inbound_tx.clone();
        let user_worker_outbound_tx = outbound_tx.clone();
        user_agent_handle = beetle::util::spawn_guarded_with_profile_handle(
            "agent_user_loop",
            STACK_AGENT_LOOP,
            user_plan.core,
            user_plan.role,
            move || {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                beetle::platform::task_wdt::register_current_task_to_task_wdt();
                let mut agent_http = match user_agent_platform
                    .create_http_client(user_agent_config_for_thread.as_ref())
                {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[{}] agent_user_loop create_http_client failed: {}", tag, e);
                        beetle::state::set_last_error(&e);
                        user_agent_platform.request_restart();
                        return;
                    }
                };
                log::info!("[{}] agent_user_loop running on Core1 thread", tag);
                if let Err(e) = run_user_agent_loop(
                    agent_http.as_mut(),
                    user_agent_worker_llm.as_ref(),
                    user_agent_registry.as_ref(),
                    user_agent_loop_config.as_ref(),
                    user_worker_user_inbound_tx,
                    user_inbound_rx,
                    user_worker_system_inbound_tx,
                    user_worker_outbound_tx,
                    Some(typing_notifier),
                ) {
                    log::warn!("[{}] agent_user_loop error: {}", tag, e);
                    beetle::state::set_last_error(&e);
                }
                user_agent_platform.request_restart();
            },
        )
        .ok();

        let system_plan = thread_plan("agent_system_loop");
        let system_agent_registry = Arc::clone(&registry);
        let system_agent_worker_llm = Arc::clone(&worker_llm);
        let system_agent_platform = Arc::clone(&platform);
        let system_agent_config_for_thread = Arc::clone(&config);
        let system_agent_loop_config = Arc::clone(&agent_config);
        let system_worker_outbound_tx = outbound_tx.clone();
        system_agent_handle = beetle::util::spawn_guarded_with_profile_handle(
            "agent_system_loop",
            STACK_AGENT_LOOP,
            system_plan.core,
            system_plan.role,
            move || {
                #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
                beetle::platform::task_wdt::register_current_task_to_task_wdt();
                let mut agent_http = match system_agent_platform
                    .create_http_client(system_agent_config_for_thread.as_ref())
                {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!(
                            "[{}] agent_system_loop create_http_client failed: {}",
                            tag,
                            e
                        );
                        beetle::state::set_last_error(&e);
                        system_agent_platform.request_restart();
                        return;
                    }
                };
                log::info!("[{}] agent_system_loop running on Core1 thread", tag);
                if let Err(e) = run_system_agent_loop(
                    agent_http.as_mut(),
                    system_agent_worker_llm.as_ref(),
                    system_agent_registry.as_ref(),
                    system_agent_loop_config.as_ref(),
                    system_worker_user_inbound_tx,
                    agent_system_inbound_tx,
                    system_inbound_rx,
                    system_worker_outbound_tx,
                ) {
                    log::warn!("[{}] agent_system_loop error: {}", tag, e);
                    beetle::state::set_last_error(&e);
                }
                system_agent_platform.request_restart();
            },
        )
        .ok();
    } else {
        log::warn!(
            "[{}] HTTP client not available (create_http_client failed): dispatch, agent, Telegram poll, and outbound sender threads were not started. On Linux, ensure ureq/rustls stack and network; see dev-docs/linux-platform-plan.md.",
            TAG
        );
    }

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    beetle::platform::task_wdt::register_current_task_to_task_wdt();

    loop {
        beetle::platform::task_wdt::feed_current_task();
        if let Some(handle) = user_agent_handle.as_ref() {
            if handle.is_finished() {
                if let Some(done) = user_agent_handle.take() {
                    let _ = done.join();
                    log::error!("[{}] agent_user_loop exited; restart requested", TAG);
                    platform.request_restart();
                }
            }
        }
        if let Some(handle) = system_agent_handle.as_ref() {
            if handle.is_finished() {
                if let Some(done) = system_agent_handle.take() {
                    let _ = done.join();
                    log::error!("[{}] agent_system_loop exited; restart requested", TAG);
                    platform.request_restart();
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
        beetle::platform::task_wdt::feed_current_task();
        log::info!("[{}] running v{}", TAG, VERSION);
    }
}
