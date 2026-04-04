//! 语音会话调度：主线程只消费事件并派发，重活按任务交给短生命周期 worker。
//! Voice session scheduler: event intake stays responsive while one-shot workers handle STT/TTS.
//!
//! Architecture:
//! - `wake_word::feed_pcm_i16` pushes `WakeDetected`
//! - `VoiceSink` pushes `Speak(text)`
//! - `run_voice_session` coalesces events and dispatches one task at a time to
//!   a short-lived `voice_session_worker`, so long STT/TTS calls no longer
//!   block event intake and the 8KB worker stack is not kept alive while idle.

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::pipeline::{capture_and_transcribe, speak_text};
use crate::audio::realtime::run_realtime_session;
use crate::bus::{PcMsg, TrackedSender};
use crate::config::{audio_realtime_enabled, AudioSegment};
use crate::constants::{AUDIO_CAPTURE_MAX_MS, VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID};
use crate::platform::PlatformHttpClient;
use crate::util::{
    spawn_guarded_with_profile_handle, HttpThreadRole, SpawnCore, STACK_VOICE_SESSION,
};
use crate::Platform;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

const TAG: &str = "voice_session";
const WORKER_IDLE_POLL_MS: u64 = 100;
const MAX_PENDING_SPEAK_CHARS: usize = 512;

/// Events consumed by the voice session thread.
#[derive(Debug)]
pub enum VoiceEvent {
    /// Wake word detected — start capture + STT + inject to agent.
    WakeDetected,
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

/// All dependencies for the voice session thread, injected by `main`.
#[derive(Clone)]
pub struct VoiceSessionConfig {
    pub platform: Arc<dyn Platform>,
    pub audio_cfg: AudioSegment,
    pub baidu_token: Option<Arc<BaiduTokenCache>>,
    pub make_http: Arc<dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
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
    let mut worker_handle: Option<JoinHandle<()>> = None;
    let mut pending = PendingVoiceEvents::default();

    loop {
        drain_worker_done(&mut worker_handle, &done_rx, &mut worker_busy);
        if !worker_busy {
            if let Some(task) = take_pending_voice_task(&mut pending) {
                match spawn_voice_session_worker(cfg.clone(), task.clone(), done_tx.clone()) {
                    Ok(handle) => {
                        worker_handle = Some(handle);
                        worker_busy = true;
                        continue;
                    }
                    Err(error) => {
                        log::error!("[{}] failed to start worker: {}", TAG, error);
                        restore_pending_voice_task(&mut pending, task);
                    }
                }
            }
        }

        let next_event = if worker_busy {
            match rx.recv_timeout(Duration::from_millis(WORKER_IDLE_POLL_MS)) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(event) => Some(event),
                Err(_) => break,
            }
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
) -> std::io::Result<JoinHandle<()>> {
    spawn_guarded_with_profile_handle(
        "voice_session_worker",
        STACK_VOICE_SESSION,
        Some(SpawnCore::Core1),
        HttpThreadRole::Background,
        move || run_voice_session_worker(cfg, task, done_tx),
    )
}

fn run_voice_session_worker(
    cfg: VoiceSessionConfig,
    task: VoiceWorkerTask,
    done_tx: mpsc::Sender<()>,
) {
    let mut http: Option<Box<dyn PlatformHttpClient>> = None;

    let ensure_http = |h: &mut Option<Box<dyn PlatformHttpClient>>,
                       make: &(dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>>
                             + Send
                             + Sync)| {
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
            handle_wake_interaction(&cfg, &mut http, &ensure_http);
        }
        VoiceWorkerTask::Speak(text) => {
            handle_speak(&cfg, &mut http, &ensure_http, &text);
        }
    }
    let _ = done_tx.send(());
    log::info!("[{}] worker stopped", TAG);
}

fn handle_wake_interaction<F>(
    cfg: &VoiceSessionConfig,
    http: &mut Option<Box<dyn PlatformHttpClient>>,
    ensure_http: &F,
) where
    F: Fn(
        &mut Option<Box<dyn PlatformHttpClient>>,
        &(dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync),
    ) -> bool,
{
    log::info!("[{}] wake detected, starting voice interaction", TAG);
    crate::metrics::record_wake_word_trigger();

    if audio_realtime_enabled(&cfg.audio_cfg) {
        if !cfg.platform.audio_mic_ready() {
            log::warn!("[{}] microphone not ready, skipping realtime session", TAG);
            return;
        }
        if !cfg.platform.audio_speaker_ready() {
            log::warn!("[{}] speaker not ready, skipping realtime session", TAG);
            return;
        }
        match run_realtime_session(cfg.platform.as_ref(), &cfg.audio_cfg, TAG) {
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

    if !ensure_http(http, cfg.make_http.as_ref()) {
        return;
    }
    let client = match http.as_mut() {
        Some(client) => client,
        None => return,
    };

    if should_play_wake_prompt(&cfg.audio_cfg)
        && !cfg.wake_prompt.is_empty()
        && cfg.platform.audio_speaker_ready()
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
        if let Err(error) = tts_result {
            log::warn!("[{}] wake prompt TTS failed: {}", TAG, error);
        }
    }

    if !cfg.platform.audio_mic_ready() {
        log::warn!("[{}] microphone not ready, skipping capture", TAG);
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
        &(dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync),
    ) -> bool,
{
    log::info!("[{}] speaking agent reply ({} chars)", TAG, text.len());

    if !cfg.platform.audio_speaker_ready() {
        log::warn!("[{}] speaker not ready, dropping TTS", TAG);
        return;
    }
    if !ensure_http(http, cfg.make_http.as_ref()) {
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
        }
        Err(error) => {
            log::warn!("[{}] TTS playback failed: {}", TAG, error);
            crate::metrics::record_voice_tool_failure("voice_session_tts");
        }
    }
}

fn drain_worker_done(
    worker_handle: &mut Option<JoinHandle<()>>,
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
        VoiceEvent::WakeDetected => {
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
        handle_voice_event(VoiceEvent::WakeDetected, &mut pending);
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
}
