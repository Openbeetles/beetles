//! 语音会话线程：唤醒后采集+STT+投递 agent，agent 回复走 TTS 播报。
//! Voice session thread: capture + STT on wake, TTS playback for agent replies.
//!
//! Architecture: single-threaded event loop consuming `VoiceEvent` from two producers:
//! - `wake_word::feed_pcm_i16` sends `WakeDetected` when the keyword fires
//! - `VoiceSink` (in dispatch) sends `Speak(text)` for agent replies on channel "voice"

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::capture::{capture_speech, AudioRecordingGuard};
use crate::audio::stt_baidu;
use crate::audio::tts_baidu;
use crate::bus::{PcMsg, TrackedSender};
use crate::config::AudioSegment;
use crate::constants::{
    AUDIO_CAPTURE_MAX_MS, AUDIO_TTS_WRITE_CHUNK_SAMPLES, VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID,
};
use crate::platform::PlatformHttpClient;
use crate::Platform;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Instant;

/// Events consumed by the voice session thread.
#[derive(Debug)]
pub enum VoiceEvent {
    /// Wake word detected — start capture + STT + inject to agent.
    WakeDetected,
    /// Agent reply to speak aloud via TTS.
    Speak(String),
}

/// All dependencies for the voice session thread, injected by `main`.
pub struct VoiceSessionConfig {
    pub platform: Arc<dyn Platform>,
    pub audio_cfg: AudioSegment,
    pub baidu_token: Arc<BaiduTokenCache>,
    pub make_http: Arc<dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync>,
    pub inbound_tx: TrackedSender<PcMsg>,
    pub wake_prompt: String,
}

/// Entry point for the voice session thread. Blocks on `rx` until the channel closes.
pub fn run_voice_session(cfg: VoiceSessionConfig, rx: Receiver<VoiceEvent>) {
    const TAG: &str = "voice_session";
    log::info!("[{}] started", TAG);

    let mut http: Option<Box<dyn PlatformHttpClient>> = None;

    let ensure_http =
        |h: &mut Option<Box<dyn PlatformHttpClient>>,
         make: &(dyn Fn() -> crate::error::Result<Box<dyn PlatformHttpClient>> + Send + Sync)| {
            if h.is_none() {
                match make() {
                    Ok(c) => *h = Some(c),
                    Err(e) => log::error!("[{}] create_http_client failed: {}", TAG, e),
                }
            }
            h.is_some()
        };

    loop {
        let event = match rx.recv() {
            Ok(e) => e,
            Err(_) => {
                log::info!("[{}] event channel closed, exiting", TAG);
                break;
            }
        };

        match event {
            VoiceEvent::WakeDetected => {
                log::info!("[{}] wake detected, starting voice interaction", TAG);
                crate::metrics::record_wake_word_trigger();

                if !ensure_http(&mut http, cfg.make_http.as_ref()) {
                    continue;
                }
                let h = http.as_mut().unwrap();

                // 1. Play wake prompt via TTS (acknowledgment)
                if !cfg.wake_prompt.is_empty() && cfg.platform.audio_speaker_ready() {
                    let _guard = AudioRecordingGuard::new();
                    crate::orchestrator::set_audio_playing(true);
                    let tts_result = tts_baidu::stream_wav_pcm16le(
                        h.as_mut(),
                        cfg.baidu_token.as_ref(),
                        &cfg.audio_cfg.stt,
                        &cfg.audio_cfg.tts,
                        &cfg.wake_prompt,
                        AUDIO_TTS_WRITE_CHUNK_SAMPLES,
                        |chunk| cfg.platform.write_speaker_pcm_i16(chunk),
                    );
                    crate::orchestrator::set_audio_playing(false);
                    if let Err(e) = tts_result {
                        log::warn!("[{}] wake prompt TTS failed: {}", TAG, e);
                    }
                }

                // 2. Capture user speech
                if !cfg.platform.audio_mic_ready() {
                    log::warn!("[{}] microphone not ready, skipping capture", TAG);
                    continue;
                }
                let captured = {
                    let _guard = AudioRecordingGuard::new();
                    match capture_speech(
                        cfg.platform.as_ref(),
                        &cfg.audio_cfg,
                        AUDIO_CAPTURE_MAX_MS,
                        TAG,
                    ) {
                        Ok(pcm) => pcm,
                        Err(e) => {
                            log::info!("[{}] no speech captured: {}", TAG, e);
                            continue;
                        }
                    }
                };

                // 3. STT
                let mic_sr = cfg.audio_cfg.microphone.sample_rate.max(8_000);
                let stt_start = Instant::now();
                let text = match stt_baidu::transcribe_pcm16_samples(
                    h.as_mut(),
                    cfg.baidu_token.as_ref(),
                    &cfg.audio_cfg.stt,
                    captured.as_slice(),
                    mic_sr,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        log::warn!("[{}] STT failed: {}", TAG, e);
                        crate::metrics::record_voice_tool_failure("voice_session_stt");
                        continue;
                    }
                };
                crate::metrics::record_voice_input_stt_http_ms(stt_start.elapsed().as_millis());
                log::info!("[{}] transcribed: {:?}", TAG, text);

                // 4. Inject into agent as PcMsg on the "voice" virtual channel
                match PcMsg::new_inbound(VOICE_CHANNEL_NAME, VOICE_DEVICE_CHAT_ID, &text, false) {
                    Ok(msg) => {
                        if let Err(e) = cfg.inbound_tx.try_send(msg) {
                            log::warn!("[{}] inbound queue full, voice msg dropped: {}", TAG, e);
                        }
                    }
                    Err(e) => {
                        log::warn!("[{}] PcMsg construction failed: {}", TAG, e);
                    }
                }
            }

            VoiceEvent::Speak(text) => {
                log::info!(
                    "[{}] speaking agent reply ({} chars)",
                    TAG,
                    text.len()
                );

                if !cfg.platform.audio_speaker_ready() {
                    log::warn!("[{}] speaker not ready, dropping TTS", TAG);
                    continue;
                }
                if !ensure_http(&mut http, cfg.make_http.as_ref()) {
                    continue;
                }
                let h = http.as_mut().unwrap();

                let _guard = AudioRecordingGuard::new();
                crate::orchestrator::set_audio_playing(true);
                let tts_start = Instant::now();
                let tts_result = tts_baidu::stream_wav_pcm16le(
                    h.as_mut(),
                    cfg.baidu_token.as_ref(),
                    &cfg.audio_cfg.stt,
                    &cfg.audio_cfg.tts,
                    &text,
                    AUDIO_TTS_WRITE_CHUNK_SAMPLES,
                    |chunk| cfg.platform.write_speaker_pcm_i16(chunk),
                );
                crate::orchestrator::set_audio_playing(false);
                let play_ms = tts_start.elapsed().as_millis();
                crate::metrics::record_voice_output_play_ms(play_ms);

                if let Err(e) = tts_result {
                    log::warn!("[{}] TTS playback failed: {}", TAG, e);
                    crate::metrics::record_voice_tool_failure("voice_session_tts");
                }
            }
        }
    }
}
