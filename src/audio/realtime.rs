//! 实时语音会话：唤醒后建立 WSS，会话内持续上送 PCM，并接收模型返回的语音增量。
//! Realtime voice session over WSS: stream PCM in, play audio deltas out.

use crate::audio::capture::AudioRecordingGuard;
use crate::channels::{connect_wss_with_headers, WssConnection, WssEvent};
use crate::config::{audio_realtime_enabled, AudioSegment, AUDIO_REALTIME_PCM16_SAMPLE_RATE};
use crate::constants::AUDIO_CAPTURE_FRAME_SAMPLES;
use crate::error::{Error, Result};
use crate::Platform;
use base64::Engine;
use serde_json::json;
use std::thread;
use std::time::{Duration, Instant};

const REALTIME_TAG: &str = "audio::realtime";
const REALTIME_OPENAI_BETA: &str = "realtime=v1";
const REALTIME_IDLE_TIMEOUT_SECS: u64 = 30;
const REALTIME_INITIAL_SEND_RETRY_MS: u64 = 100;
const REALTIME_INITIAL_SEND_RETRY_MAX: usize = 50;
const REALTIME_RECV_POLL_MS: u64 = 20;
const REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS: u32 = 8_000;
const REALTIME_SERVER_VAD_PREFIX_PADDING_MS: u32 = 300;

pub struct RealtimeSessionResult {
    pub turns_completed: u32,
    pub input_audio_ms: u128,
    pub output_audio_ms: u128,
    pub session_ms: u128,
}

struct RealtimeLoopState {
    awaiting_response: bool,
    audio_playing: bool,
    turns_completed: u32,
    input_samples: usize,
    output_samples: usize,
    last_activity: Instant,
}

impl RealtimeLoopState {
    fn new() -> Self {
        Self {
            awaiting_response: false,
            audio_playing: false,
            turns_completed: 0,
            input_samples: 0,
            output_samples: 0,
            last_activity: Instant::now(),
        }
    }
}

pub fn run_realtime_session(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    log_tag: &'static str,
) -> Result<RealtimeSessionResult> {
    if !audio_realtime_enabled(audio_cfg) {
        return Err(Error::config(
            REALTIME_TAG,
            "run_realtime_session called without realtime config",
        ));
    }

    let _recording_guard = AudioRecordingGuard::new();
    let session_start = Instant::now();
    let ws_url = build_realtime_ws_url(audio_cfg)?;
    let auth = format!("Bearer {}", audio_cfg.realtime.api_key.trim());
    let headers = [
        ("authorization", auth.as_str()),
        ("openai-beta", REALTIME_OPENAI_BETA),
    ];
    let mut conn = connect_realtime_wss(ws_url.as_str(), &headers)?;
    let mut state = RealtimeLoopState::new();
    let mut mic_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];

    send_text_retry(
        conn.as_mut(),
        build_session_update(audio_cfg).as_str(),
        REALTIME_INITIAL_SEND_RETRY_MAX,
    )?;
    log::info!("[{}] realtime session connected", log_tag);

    loop {
        let timed_out = drain_server_events(
            conn.as_mut(),
            platform,
            &mut state,
            Duration::from_millis(REALTIME_RECV_POLL_MS),
        )?;
        if timed_out
            && !state.awaiting_response
            && !state.audio_playing
            && state.last_activity.elapsed() >= Duration::from_secs(REALTIME_IDLE_TIMEOUT_SECS)
        {
            break;
        }

        let n = platform.read_mic_pcm_i16(&mut mic_frame)?;
        if n == 0 {
            thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
            continue;
        }

        append_audio_frame(conn.as_mut(), &mic_frame[..n.min(mic_frame.len())])?;
        state.input_samples = state.input_samples.saturating_add(n);
        state.last_activity = Instant::now();
    }

    if state.audio_playing {
        crate::orchestrator::set_audio_playing(false);
    }

    Ok(RealtimeSessionResult {
        turns_completed: state.turns_completed,
        input_audio_ms: samples_to_ms(state.input_samples),
        output_audio_ms: samples_to_ms(state.output_samples),
        session_ms: session_start.elapsed().as_millis(),
    })
}

fn connect_realtime_wss(url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn WssConnection>> {
    Ok(Box::new(connect_wss_with_headers(url, headers)?))
}

fn build_realtime_ws_url(audio_cfg: &AudioSegment) -> Result<String> {
    let base = audio_cfg.realtime.ws_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::config(REALTIME_TAG, "realtime.ws_url is empty"));
    }
    if base.contains("model=") {
        return Ok(base.to_string());
    }

    let model = urlencoding::encode(audio_cfg.realtime.model.trim());
    let separator = if base.contains('?') { '&' } else { '?' };
    Ok(format!("{base}{separator}model={model}"))
}

fn build_session_update(audio_cfg: &AudioSegment) -> String {
    let mut session = json!({
        "modalities": ["audio", "text"],
        "voice": audio_cfg.realtime.voice.trim(),
        "input_audio_format": "pcm16",
        "output_audio_format": "pcm16",
        "turn_detection": {
            "type": "server_vad",
            "threshold": audio_cfg.vad.threshold,
            "silence_duration_ms": audio_cfg.vad.silence_duration_ms,
            "prefix_padding_ms": REALTIME_SERVER_VAD_PREFIX_PADDING_MS,
            "idle_timeout_ms": REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS,
            "create_response": true,
            "interrupt_response": true
        }
    });

    let instructions = audio_cfg.realtime.instructions.trim();
    if !instructions.is_empty() {
        session["instructions"] = serde_json::Value::String(instructions.to_string());
    }

    json!({
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

fn send_text_retry(conn: &mut dyn WssConnection, text: &str, attempts: usize) -> Result<()> {
    let mut last_err: Option<Error> = None;
    for _ in 0..attempts {
        match conn.send_text(text) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                thread::sleep(Duration::from_millis(REALTIME_INITIAL_SEND_RETRY_MS));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::config(REALTIME_TAG, "wss send failed")))
}

fn drain_server_events(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    timeout: Duration,
) -> Result<bool> {
    let mut wait = timeout;
    let mut saw_event = false;

    loop {
        match conn.recv_timeout(wait)? {
            Some(WssEvent::Binary(data)) => {
                saw_event = true;
                handle_server_message(platform, state, data.as_slice())?;
                wait = Duration::ZERO;
            }
            Some(WssEvent::Disconnected) | Some(WssEvent::Closed) => {
                return Err(Error::config(
                    REALTIME_TAG,
                    "realtime websocket disconnected",
                ));
            }
            None => return Ok(!saw_event),
        }
    }
}

fn handle_server_message(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    payload: &[u8],
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| Error::config("realtime_voice_parse", e.to_string()))?;
    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    match event_type {
        "session.created" | "session.updated" | "input_audio_buffer.committed" => {
            state.last_activity = Instant::now();
            Ok(())
        }
        "response.created" => {
            state.awaiting_response = true;
            state.last_activity = Instant::now();
            Ok(())
        }
        "input_audio_buffer.speech_started" => {
            state.awaiting_response = false;
            state.last_activity = Instant::now();
            Ok(())
        }
        "input_audio_buffer.speech_stopped" => {
            state.awaiting_response = true;
            state.last_activity = Instant::now();
            Ok(())
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            let delta = value
                .get("delta")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::config("realtime_voice_parse", "audio delta missing"))?;
            let pcm = decode_pcm16_delta(delta)?;
            if !pcm.is_empty() {
                if !state.audio_playing {
                    crate::orchestrator::set_audio_playing(true);
                    state.audio_playing = true;
                }
                platform.write_speaker_pcm_i16(&pcm)?;
                state.output_samples = state.output_samples.saturating_add(pcm.len());
                state.last_activity = Instant::now();
            }
            Ok(())
        }
        "response.output_audio.done" | "response.audio.done" => {
            if state.audio_playing {
                crate::orchestrator::set_audio_playing(false);
                state.audio_playing = false;
            }
            state.last_activity = Instant::now();
            Ok(())
        }
        "response.done" => {
            if state.audio_playing {
                crate::orchestrator::set_audio_playing(false);
                state.audio_playing = false;
            }
            if state.awaiting_response {
                state.awaiting_response = false;
                state.turns_completed = state.turns_completed.saturating_add(1);
            }
            state.last_activity = Instant::now();
            Ok(())
        }
        "error" => {
            let msg = value
                .get("error")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("realtime websocket error");
            Err(Error::config("realtime_voice_ws", msg))
        }
        _ => Ok(()),
    }
}

fn append_audio_frame(conn: &mut dyn WssConnection, pcm: &[i16]) -> Result<()> {
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for sample in pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let audio = base64::engine::general_purpose::STANDARD.encode(bytes);
    let event = json!({
        "type": "input_audio_buffer.append",
        "audio": audio,
    })
    .to_string();
    conn.send_text(&event)
}

fn decode_pcm16_delta(delta_b64: &str) -> Result<Vec<i16>> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(delta_b64.as_bytes())
        .map_err(|e| {
            Error::config(
                "realtime_voice_parse",
                format!("audio base64 decode failed: {}", e),
            )
        })?;
    if bytes.len() % 2 != 0 {
        return Err(Error::config(
            "realtime_voice_parse",
            "pcm16 delta length must be even",
        ));
    }
    let mut pcm = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        pcm.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(pcm)
}

fn samples_to_ms(samples: usize) -> u128 {
    (samples as u128).saturating_mul(1000) / (AUDIO_REALTIME_PCM16_SAMPLE_RATE as u128)
}
