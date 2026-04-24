//! 语音会话调度：主线程消费事件并串行执行语音任务。
//! Voice session scheduler: event intake stays responsive while voice tasks run one at a time.
//!
//! Architecture:
//! - `wake::feed_pcm_i16` pushes `WakeTriggered`
//! - `VoiceSink` pushes `Speak(text)`
//! - `run_voice_session` coalesces events and dispatches one task at a time
//! - Realtime wake interactions run on a dedicated transient `voice_realtime`
//!   worker so the always-on scheduler thread stays shallow and stable
//! - Non-realtime STT/TTS fallback still uses `voice_session_worker`

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::pipeline::{capture_and_transcribe, speak_text};
use crate::audio::realtime::{
    connect_realtime_session, run_connected_realtime_session, ConnectedRealtimeSession,
};
use crate::bus::{PcMsg, TrackedSender};
use crate::config::{audio_realtime_enabled, AudioSegment};
use crate::constants::{AUDIO_CAPTURE_MAX_MS, VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID};
use crate::network::{HttpClientClass, NetworkGovernor, VoiceExclusiveTransportGuard};
use crate::platform::PlatformHttpClient;
use crate::util::{
    spawn_guarded_with_profile_handle, HttpThreadRole, SpawnCore, TaskHandle, STACK_CHANNEL_WS,
    STACK_VOICE_REALTIME, STACK_VOICE_SESSION,
};
use crate::Platform;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TAG: &str = "voice_session";
const WORKER_IDLE_POLL_MS: u64 = 100;
const WORKER_SPAWN_FAILURE_COOLDOWN_MS: u64 = 5_000;
const MAX_PENDING_SPEAK_CHARS: usize = 512;

/// Events consumed by the voice session thread.
#[derive(Debug)]
pub enum VoiceEvent {
    /// Wake backend triggered — start capture + STT + inject to agent.
    WakeTriggered,
    /// Agent reply to speak aloud via TTS.
    Speak(String),
}

#[derive(Clone)]
enum VoiceWorkerTask {
    WakeInteraction,
    Speak(String),
}

#[derive(Default)]
struct PendingVoiceEvents {
    wake_requested: bool,
    pending_speak: Option<String>,
}

#[derive(Default)]
struct VoiceWorkerRetryGate {
    retry_after: Option<Instant>,
}

impl VoiceWorkerRetryGate {
    fn can_retry(&self, now: Instant) -> bool {
        self.retry_after
            .is_none_or(|retry_after| retry_after <= now)
    }

    fn record_spawn_failure(&mut self, now: Instant) {
        self.retry_after = Some(now + Duration::from_millis(WORKER_SPAWN_FAILURE_COOLDOWN_MS));
    }

    fn clear(&mut self) {
        self.retry_after = None;
    }
}

/// All dependencies for the voice session thread, injected by `main`.
#[derive(Clone)]
pub struct VoiceSessionConfig {
    pub platform: Arc<dyn Platform>,
    pub network: Arc<NetworkGovernor>,
    pub audio_cfg: AudioSegment,
    pub baidu_token: Option<Arc<BaiduTokenCache>>,
    pub inbound_tx: TrackedSender<PcMsg>,
    pub wake_prompt: String,
}

/// Entry point for the voice session scheduler thread. Blocks on `rx` until the channel closes.
pub fn run_voice_session(cfg: VoiceSessionConfig, rx: Receiver<VoiceEvent>) {
    log::info!(
        "[{}] started realtime_enabled={}",
        TAG,
        audio_realtime_enabled(&cfg.audio_cfg)
    );

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut worker_busy = false;
    let mut worker_handle: Option<TaskHandle> = None;
    let mut pending = PendingVoiceEvents::default();
    let mut retry_gate = VoiceWorkerRetryGate::default();

    loop {
        crate::platform::task_wdt::feed_current_task();
        drain_worker_done(&mut worker_handle, &done_rx, &mut worker_busy);
        let now = Instant::now();
        if !worker_busy && retry_gate.can_retry(now) {
            if let Some(task) = take_pending_voice_task(&mut pending) {
                match spawn_voice_session_worker(cfg.clone(), task.clone(), done_tx.clone()) {
                    Ok(handle) => {
                        retry_gate.clear();
                        worker_handle = Some(handle);
                        worker_busy = true;
                        continue;
                    }
                    Err(error) => {
                        retry_gate.record_spawn_failure(now);
                        log::error!(
                            "[{}] failed to start worker: {}; retry suppressed for {}ms",
                            TAG,
                            error,
                            WORKER_SPAWN_FAILURE_COOLDOWN_MS
                        );
                        restore_pending_voice_task(&mut pending, task);
                    }
                }
            }
        }

        let next_event = match rx.recv_timeout(Duration::from_millis(WORKER_IDLE_POLL_MS)) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };

        let Some(event) = next_event else {
            continue;
        };
        handle_voice_event(event, &mut pending);
    }

    if let Some(handle) = worker_handle.take() {
        let _ = handle.join();
    }
    log::info!("[{}] scheduler stopped", TAG);
}

fn spawn_voice_session_worker(
    cfg: VoiceSessionConfig,
    task: VoiceWorkerTask,
    done_tx: mpsc::Sender<()>,
) -> std::io::Result<TaskHandle> {
    let (name, stack_size) = voice_worker_spawn_profile(&cfg, &task);
    spawn_guarded_with_profile_handle(
        name,
        stack_size,
        Some(SpawnCore::Core1),
        HttpThreadRole::Background,
        move || run_voice_session_worker(cfg, task, done_tx),
    )
}

fn voice_worker_spawn_profile(
    cfg: &VoiceSessionConfig,
    task: &VoiceWorkerTask,
) -> (&'static str, usize) {
    if matches!(task, VoiceWorkerTask::WakeInteraction) && audio_realtime_enabled(&cfg.audio_cfg) {
        ("voice_realtime", STACK_VOICE_REALTIME)
    } else {
        ("voice_session_worker", STACK_VOICE_SESSION)
    }
}

fn run_voice_session_worker(
    cfg: VoiceSessionConfig,
    task: VoiceWorkerTask,
    done_tx: mpsc::Sender<()>,
) {
    run_voice_task(&cfg, task);
    let _ = done_tx.send(());
    log::info!("[{}] worker stopped", TAG);
}

fn run_voice_task(cfg: &VoiceSessionConfig, task: VoiceWorkerTask) {
    let mut http: Option<Box<dyn PlatformHttpClient>> = None;

    let ensure_http =
        |h: &mut Option<Box<dyn PlatformHttpClient>>,
         make: &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>| {
            if h.is_none() {
                match make() {
                    Ok(client) => *h = Some(client),
                    Err(error) => log::error!("[{}] create_http_client failed: {}", TAG, error),
                }
            }
            h.is_some()
        };

    match task {
        VoiceWorkerTask::WakeInteraction => {
            handle_wake_interaction(cfg, &mut http, &ensure_http);
        }
        VoiceWorkerTask::Speak(text) => {
            handle_speak(cfg, &mut http, &ensure_http, &text);
        }
    }
}

fn connect_realtime_session_via_worker(
    cfg: &VoiceSessionConfig,
) -> crate::Result<ConnectedRealtimeSession> {
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    let platform = Arc::clone(&cfg.platform);
    let audio_cfg = cfg.audio_cfg.clone();
    let handle = spawn_guarded_with_profile_handle(
        "voice_realtime_connect",
        STACK_CHANNEL_WS,
        Some(SpawnCore::Core1),
        HttpThreadRole::Background,
        move || {
            let result = connect_realtime_session(platform.as_ref(), &audio_cfg, TAG);
            if result_tx.send(result).is_err() {
                log::warn!("[{}] realtime connect result receiver dropped", TAG);
            }
        },
    )
    .map_err(|error| crate::Error::io("voice_realtime_connect_spawn", error))?;

    let result = result_rx.recv().map_err(|error| {
        crate::Error::io(
            "voice_realtime_connect_recv",
            std::io::Error::other(error.to_string()),
        )
    })?;
    let _ = handle.join();
    result
}

struct WakeSessionResetGuard;

impl Drop for WakeSessionResetGuard {
    fn drop(&mut self) {
        crate::wake::reset_after_session();
    }
}

fn handle_wake_interaction<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>,
    ) -> bool,
{
    let _wake_reset = WakeSessionResetGuard;
    log::info!("[{}] wake triggered, starting voice interaction", TAG);
    let duplex_caps = cfg.platform.audio_duplex_capabilities();

    if audio_realtime_enabled(&cfg.audio_cfg) {
        if !duplex_caps.can_run_realtime_session() {
            log::warn!(
                "[{}] realtime session unavailable under audio contract profile={}",
                TAG,
                duplex_caps.profile().as_str()
            );
            return;
        }
        let runtime_mode = crate::runtime::thread_registry::runtime_mode_snapshot();
        if !runtime_mode.action_budget.allow_realtime_voice_connect {
            log::info!(
                "[{}] skip realtime voice connect under runtime_mode={}",
                TAG,
                runtime_mode.current_mode.as_str()
            );
            return;
        }
        let _voice_transport = VoiceExclusiveTransportGuard::enter(cfg.platform.as_ref(), TAG);
        match connect_realtime_session_via_worker(cfg).and_then(|connected| {
            run_connected_realtime_session(cfg.platform.as_ref(), &cfg.audio_cfg, connected)
        }) {
            Ok(session) => {
                log::info!(
                    "[{}] realtime session finished turns={} input_ms={} output_ms={} duration_ms={}",
                    TAG,
                    session.turns_completed,
                    session.input_audio_ms,
                    session.output_audio_ms,
                    session.session_ms
                );
                if session.output_audio_ms > 0 {
                    crate::metrics::record_voice_output_play_ms(session.output_audio_ms);
                }
            }
            Err(error) => {
                log::warn!("[{}] realtime voice session failed: {}", TAG, error);
                crate::metrics::record_voice_tool_failure("voice_session_realtime");
            }
        }
        return;
    }

    let make_http = || cfg.network.open_http_client(HttpClientClass::Background);
    if !ensure_http(http, &make_http) {
        return;
    }
    let client = match http.as_mut() {
        Some(client) => client,
        None => return,
    };

    if should_play_wake_prompt(&cfg.audio_cfg)
        && !cfg.wake_prompt.is_empty()
        && duplex_caps.has_speaker_output()
    {
        let Some(baidu_token) = cfg.baidu_token.as_deref() else {
            log::warn!(
                "[{}] wake prompt requested but baidu token cache unavailable",
                TAG
            );
            crate::metrics::record_voice_tool_failure("voice_session_tts");
            return;
        };
        let tts_result = speak_text(
            cfg.platform.as_ref(),
            &cfg.audio_cfg,
            baidu_token,
            client.as_mut(),
            &cfg.wake_prompt,
        );
        match tts_result {
            Ok(playback) => {
                if playback.interrupted {
                    log::info!("[{}] wake prompt playback interrupted by local speech", TAG);
                }
            }
            Err(error) => {
                log::warn!("[{}] wake prompt TTS failed: {}", TAG, error);
            }
        }
    }

    if !duplex_caps.has_microphone_input() {
        log::warn!(
            "[{}] microphone unavailable under audio contract profile={}, skipping capture",
            TAG,
            duplex_caps.profile().as_str()
        );
        return;
    }

    let Some(baidu_token) = cfg.baidu_token.as_deref() else {
        log::warn!(
            "[{}] baidu speech fallback unavailable, skipping capture/transcribe",
            TAG
        );
        crate::metrics::record_voice_tool_failure("voice_session_stt");
        return;
    };

    let text = match capture_and_transcribe(
        cfg.platform.as_ref(),
        &cfg.audio_cfg,
        baidu_token,
        client.as_mut(),
        AUDIO_CAPTURE_MAX_MS,
        TAG,
    ) {
        Ok(text) => text,
        Err(error) => {
            log::info!("[{}] voice capture/transcribe skipped: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_stt");
            return;
        }
    };
    log::info!("[{}] transcribed: {:?}", TAG, text);

    match PcMsg::new_inbound(VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID, &text, false) {
        Ok(msg) => {
            if let Err(error) = cfg.inbound_tx.try_send(msg) {
                log::warn!("[{}] inbound queue full, voice msg dropped: {}", TAG, error);
            }
        }
        Err(error) => {
            log::warn!("[{}] PcMsg construction failed: {}", TAG, error);
        }
    }
}

fn handle_speak<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
    text: &str,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>,
    ) -> bool,
{
    log::info!("[{}] speaking agent reply ({} chars)", TAG, text.len());
    let duplex_caps = cfg.platform.audio_duplex_capabilities();

    if !duplex_caps.has_speaker_output() {
        log::warn!(
            "[{}] speaker unavailable under audio contract profile={}, dropping TTS",
            TAG,
            duplex_caps.profile().as_str()
        );
        return;
    }
    let make_http = || cfg.network.open_http_client(HttpClientClass::Background);
    if !ensure_http(http, &make_http) {
        return;
    }
    let client = match http.as_mut() {
        Some(client) => client,
        None => return,
    };
    let Some(baidu_token) = cfg.baidu_token.as_deref() else {
        log::warn!("[{}] baidu speech fallback unavailable, dropping TTS", TAG);
        crate::metrics::record_voice_tool_failure("voice_session_tts");
        return;
    };

    let tts_result = speak_text(
        cfg.platform.as_ref(),
        &cfg.audio_cfg,
        baidu_token,
        client.as_mut(),
        text,
    );
    match tts_result {
        Ok(playback) => {
            crate::metrics::record_voice_output_tts_http_ms(playback.tts_http_ms);
            crate::metrics::record_voice_output_play_ms(playback.play_ms);
            if playback.interrupted {
                log::info!("[{}] TTS playback interrupted by local speech", TAG);
            }
        }
        Err(error) => {
            log::warn!("[{}] TTS playback failed: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_tts");
        }
    }
}

fn drain_worker_done(
    worker_handle: &mut Option<TaskHandle>,
    done_rx: &mpsc::Receiver<()>,
    worker_busy: &mut bool,
) {
    while done_rx.try_recv().is_ok() {
        *worker_busy = false;
        if let Some(handle) = worker_handle.take() {
            let _ = handle.join();
        }
    }
}

fn take_pending_voice_task(pending: &mut PendingVoiceEvents) -> Option<VoiceWorkerTask> {
    if pending.wake_requested {
        pending.wake_requested = false;
        return Some(VoiceWorkerTask::WakeInteraction);
    }
    pending.pending_speak.take().map(VoiceWorkerTask::Speak)
}

fn restore_pending_voice_task(pending: &mut PendingVoiceEvents, task: VoiceWorkerTask) {
    match task {
        VoiceWorkerTask::WakeInteraction => {
            pending.wake_requested = true;
            pending.pending_speak = None;
        }
        VoiceWorkerTask::Speak(text) => {
            if pending.pending_speak.is_none() {
                pending.pending_speak = Some(text);
            }
        }
    }
}

fn handle_voice_event(event: VoiceEvent, pending: &mut PendingVoiceEvents) {
    match event {
        VoiceEvent::WakeTriggered => {
            pending.wake_requested = true;
            pending.pending_speak = None;
        }
        VoiceEvent::Speak(text) => {
            let normalized = normalize_speak_text(&text);
            if normalized.is_empty() {
                return;
            }
            if pending.pending_speak.as_deref() == Some(normalized.as_str()) {
                return;
            }
            pending.pending_speak = Some(normalized);
        }
    }
}

fn normalize_speak_text(text: &str) -> String {
    crate::util::truncate_content_to_max(text.trim(), MAX_PENDING_SPEAK_CHARS).into_owned()
}

fn should_play_wake_prompt(audio_cfg: &AudioSegment) -> bool {
    audio_cfg.service_provider == "baidu"
        && !audio_cfg.speech.api_key.trim().is_empty()
        && !audio_cfg.speech.api_secret.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_clears_pending_speak() {
        let mut pending = PendingVoiceEvents {
            wake_requested: false,
            pending_speak: Some("old reply".to_string()),
        };
        handle_voice_event(VoiceEvent::WakeTriggered, &mut pending);
        assert!(pending.wake_requested);
        assert!(pending.pending_speak.is_none());
    }

    #[test]
    fn speak_keeps_latest_pending_text() {
        let mut pending = PendingVoiceEvents::default();
        handle_voice_event(VoiceEvent::Speak("first reply".to_string()), &mut pending);
        handle_voice_event(VoiceEvent::Speak("second reply".to_string()), &mut pending);
        assert_eq!(pending.pending_speak.as_deref(), Some("second reply"));
    }

    #[test]
    fn normalize_speak_text_trims_whitespace() {
        assert_eq!(normalize_speak_text("  hello  "), "hello");
    }

    #[test]
    fn worker_retry_gate_blocks_immediate_retry_after_spawn_failure() {
        let mut gate = VoiceWorkerRetryGate::default();
        let now = Instant::now();
        assert!(gate.can_retry(now));

        gate.record_spawn_failure(now);

        assert!(!gate.can_retry(now + Duration::from_millis(100)));
        assert!(gate.can_retry(now + Duration::from_millis(WORKER_SPAWN_FAILURE_COOLDOWN_MS + 1)));
    }

    #[test]
    fn worker_retry_gate_clears_after_success() {
        let mut gate = VoiceWorkerRetryGate::default();
        gate.record_spawn_failure(Instant::now());
        gate.clear();

        assert!(gate.can_retry(Instant::now()));
    }
}
