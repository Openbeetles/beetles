//! 实时语音会话：唤醒后建立 WSS，会话内持续上送 PCM，并接收模型返回的语音增量。
//! Realtime voice session over WSS: stream PCM in, play audio deltas out.

use crate::audio::capture::AudioRecordingGuard;
use crate::channels::{
    connect_wss_with_headers_and_profile, WssCloseInfo, WssConnectProfile, WssConnection, WssEvent,
};
use crate::config::{
    audio_realtime_enabled, AudioSegment, AUDIO_REALTIME_PROVIDER_BAIDU,
    AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE, AUDIO_REALTIME_PROVIDER_QWEN,
};
use crate::constants::AUDIO_CAPTURE_FRAME_SAMPLES;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::{
    TLS_ADMISSION_MIN_INTERNAL_BYTES, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
    TLS_ADMISSION_NO_PSRAM_MIN_BYTES,
};
use crate::error::{Error, Result};
use crate::Platform;
use base64::Engine;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const REALTIME_TAG: &str = "audio::realtime";
const REALTIME_OPENAI_BETA: &str = "realtime=v1";
const REALTIME_BAIDU_AUDIO_CODEC: &str = "raw16k";
const REALTIME_IDLE_TIMEOUT_SECS: u64 = 30;
const REALTIME_INITIAL_SEND_RETRY_MS: u64 = 100;
const REALTIME_INITIAL_SEND_RETRY_MAX: usize = 50;
const REALTIME_RECV_POLL_MS: u64 = 20;
const REALTIME_SESSION_READY_TIMEOUT_MS: u64 = 1_500;
const REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS: u32 = 8_000;
const REALTIME_SERVER_VAD_PREFIX_PADDING_MS: u32 = 300;
const REALTIME_TLS_ADMISSION_RETRY_MAX: usize = 12;
const REALTIME_TLS_ADMISSION_RETRY_MS: u64 = 150;
static REALTIME_EVENT_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RealtimeProvider {
    OpenAiCompatible,
    Qwen,
    Baidu,
}

impl RealtimeProvider {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE => Ok(Self::OpenAiCompatible),
            AUDIO_REALTIME_PROVIDER_QWEN => Ok(Self::Qwen),
            AUDIO_REALTIME_PROVIDER_BAIDU => Ok(Self::Baidu),
            _ => Err(Error::config(
                REALTIME_TAG,
                format!("unsupported realtime provider: {}", raw),
            )),
        }
    }

    fn input_audio_format(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "pcm16",
            Self::Qwen => "pcm",
            Self::Baidu => REALTIME_BAIDU_AUDIO_CODEC,
        }
    }

    fn output_audio_format(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "pcm16",
            Self::Qwen => "pcm",
            Self::Baidu => REALTIME_BAIDU_AUDIO_CODEC,
        }
    }

    fn requires_openai_beta_header(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    fn uses_json_session_update(self) -> bool {
        !matches!(self, Self::Baidu)
    }
}

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

struct RealtimeUploadEncoder {
    pcm_bytes: Vec<u8>,
    qwen_pcm16: Vec<i16>,
    audio_b64: String,
    event_json: String,
    qwen_resample_tail: [i16; 2],
    qwen_resample_tail_len: usize,
}

impl RealtimeUploadEncoder {
    fn new() -> Self {
        let pcm_capacity = AUDIO_CAPTURE_FRAME_SAMPLES * 2;
        let b64_capacity = ((pcm_capacity + 2) / 3) * 4;
        Self {
            pcm_bytes: Vec::with_capacity(pcm_capacity),
            qwen_pcm16: Vec::with_capacity(AUDIO_CAPTURE_FRAME_SAMPLES),
            audio_b64: String::with_capacity(b64_capacity),
            event_json: String::with_capacity(b64_capacity + 64),
            qwen_resample_tail: [0; 2],
            qwen_resample_tail_len: 0,
        }
    }

    fn build_append_event(&mut self, provider: RealtimeProvider, pcm: &[i16]) -> &str {
        self.pcm_bytes.clear();
        match provider {
            RealtimeProvider::OpenAiCompatible => {
                self.pcm_bytes.reserve(pcm.len().saturating_mul(2));
                for sample in pcm {
                    self.pcm_bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
            RealtimeProvider::Qwen => {
                self.downsample_24k_to_16k(pcm);
                self.pcm_bytes
                    .reserve(self.qwen_pcm16.len().saturating_mul(2));
                for sample in &self.qwen_pcm16 {
                    self.pcm_bytes.extend_from_slice(&sample.to_le_bytes());
                }
            }
            RealtimeProvider::Baidu => unreachable!("baidu upload uses raw binary frames"),
        }

        self.audio_b64.clear();
        base64::engine::general_purpose::STANDARD
            .encode_string(self.pcm_bytes.as_slice(), &mut self.audio_b64);

        self.event_json.clear();
        self.event_json.push_str("{\"event_id\":\"");
        self.event_json
            .push_str(next_realtime_event_id("audio").as_str());
        self.event_json
            .push_str("\",\"type\":\"input_audio_buffer.append\",\"audio\":\"");
        self.event_json.push_str(self.audio_b64.as_str());
        self.event_json.push_str("\"}");
        self.event_json.as_str()
    }

    fn downsample_24k_to_16k(&mut self, pcm: &[i16]) -> &[i16] {
        self.qwen_pcm16.clear();
        let mut triple = [0i16; 3];
        let mut triple_len = self.qwen_resample_tail_len;
        if triple_len > 0 {
            triple[..triple_len].copy_from_slice(&self.qwen_resample_tail[..triple_len]);
        }

        for &sample in pcm {
            triple[triple_len] = sample;
            triple_len += 1;
            if triple_len < 3 {
                continue;
            }

            self.qwen_pcm16.push(triple[0]);
            let blended = ((triple[1] as i32) + (triple[2] as i32)) / 2;
            self.qwen_pcm16.push(blended as i16);
            triple_len = 0;
        }

        self.qwen_resample_tail_len = triple_len;
        if triple_len > 0 {
            self.qwen_resample_tail[..triple_len].copy_from_slice(&triple[..triple_len]);
        }

        self.qwen_pcm16.as_slice()
    }
}

pub fn run_realtime_session(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    log_tag: &'static str,
) -> Result<RealtimeSessionResult> {
    crate::platform::task_wdt::register_current_task_to_task_wdt();
    if !audio_realtime_enabled(audio_cfg) {
        return Err(Error::config(
            REALTIME_TAG,
            "run_realtime_session called without realtime config",
        ));
    }

    let _recording_guard = AudioRecordingGuard::new();
    let session_start = Instant::now();
    let provider = RealtimeProvider::parse(audio_cfg.realtime.provider.trim())?;
    let ws_url = build_realtime_ws_url(provider, audio_cfg)?;
    let headers = build_realtime_headers(provider, audio_cfg, ws_url.as_str())?;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let mut conn = connect_realtime_wss_with_retry(platform, ws_url.as_str(), &header_refs)?;
    let mut state = RealtimeLoopState::new();
    let mut mic_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut upload_encoder = RealtimeUploadEncoder::new();

    if provider.uses_json_session_update() {
        send_text_retry(
            conn.as_mut(),
            build_session_update(provider, audio_cfg).as_str(),
            REALTIME_INITIAL_SEND_RETRY_MAX,
        )?;
    }
    await_session_ready(
        conn.as_mut(),
        platform,
        &mut state,
        provider,
        audio_cfg,
        Duration::from_millis(REALTIME_SESSION_READY_TIMEOUT_MS),
    )?;
    log::info!("[{}] realtime session connected", log_tag);

    loop {
        crate::platform::task_wdt::feed_current_task();
        let timed_out = drain_server_events(
            conn.as_mut(),
            platform,
            &mut state,
            provider,
            audio_cfg,
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
            crate::platform::task_wdt::feed_current_task();
            thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
            continue;
        }

        append_audio_frame(
            conn.as_mut(),
            &mut upload_encoder,
            provider,
            &mic_frame[..n.min(mic_frame.len())],
        )?;
        state.input_samples = state.input_samples.saturating_add(n);
        state.last_activity = Instant::now();
    }

    if state.audio_playing {
        crate::orchestrator::set_audio_playing(false);
    }

    Ok(RealtimeSessionResult {
        turns_completed: state.turns_completed,
        input_audio_ms: samples_to_ms(state.input_samples, audio_cfg.microphone.sample_rate),
        output_audio_ms: samples_to_ms(state.output_samples, audio_cfg.speaker.sample_rate),
        session_ms: session_start.elapsed().as_millis(),
    })
}

fn connect_realtime_wss(url: &str, headers: &[(&str, &str)]) -> Result<Box<dyn WssConnection>> {
    Ok(Box::new(connect_wss_with_headers_and_profile(
        url,
        headers,
        WssConnectProfile::Realtime,
    )?))
}

fn connect_realtime_wss_with_retry(
    platform: &dyn Platform,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<Box<dyn WssConnection>> {
    let mut last_err: Option<Error> = None;
    for attempt in 0..REALTIME_TLS_ADMISSION_RETRY_MAX {
        crate::platform::task_wdt::feed_current_task();
        wait_for_realtime_admission_window(platform);
        match connect_realtime_wss(url, headers) {
            Ok(conn) => return Ok(conn),
            Err(err)
                if err.is_tls_admission() && attempt + 1 < REALTIME_TLS_ADMISSION_RETRY_MAX =>
            {
                let snap = platform.memory_snapshot();
                log::warn!(
                    "[{}] realtime tls admission retry {}/{} internal_free={} largest={} spiram={}",
                    REALTIME_TAG,
                    attempt + 1,
                    REALTIME_TLS_ADMISSION_RETRY_MAX,
                    snap.heap_free_internal,
                    snap.heap_largest_block,
                    snap.heap_free_spiram
                );
                last_err = Some(err);
                crate::platform::task_wdt::feed_current_task();
                thread::sleep(Duration::from_millis(REALTIME_TLS_ADMISSION_RETRY_MS));
            }
            Err(err) => return Err(err),
        }
    }
    Err(last_err.unwrap_or_else(|| Error::config(REALTIME_TAG, "realtime wss connect failed")))
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_realtime_admission_window(platform: &dyn Platform) {
    let deadline = Instant::now()
        + Duration::from_millis(
            (REALTIME_TLS_ADMISSION_RETRY_MAX as u64) * REALTIME_TLS_ADMISSION_RETRY_MS,
        );
    while Instant::now() < deadline {
        crate::platform::task_wdt::feed_current_task();
        let snap = platform.memory_snapshot();
        let min_free = if snap.heap_free_spiram > 0 {
            TLS_ADMISSION_MIN_INTERNAL_BYTES as u32
        } else {
            TLS_ADMISSION_NO_PSRAM_MIN_BYTES as u32
        };
        let enough_free = snap.heap_free_internal >= min_free;
        let enough_largest = snap.heap_free_spiram == 0
            || snap.heap_largest_block >= TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES as u32;
        let no_external_wss = crate::orchestrator::snapshot().active_wss_count == 0;
        if enough_free && enough_largest && no_external_wss {
            return;
        }
        thread::sleep(Duration::from_millis(REALTIME_TLS_ADMISSION_RETRY_MS));
    }
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn wait_for_realtime_admission_window(_platform: &dyn Platform) {}

fn build_realtime_headers(
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    ws_url: &str,
) -> Result<Vec<(&'static str, String)>> {
    let mut headers = Vec::new();
    if provider != RealtimeProvider::Baidu {
        headers.push((
            "Authorization",
            format!("Bearer {}", audio_cfg.realtime.api_key.trim()),
        ));
    }
    if provider.requires_openai_beta_header() && realtime_ws_url_needs_openai_beta(ws_url) {
        headers.push(("OpenAI-Beta", REALTIME_OPENAI_BETA.to_string()));
    }
    Ok(headers)
}

fn realtime_ws_url_needs_openai_beta(ws_url: &str) -> bool {
    let rest = ws_url
        .strip_prefix("wss://")
        .or_else(|| ws_url.strip_prefix("ws://"))
        .unwrap_or(ws_url);
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    authority.eq_ignore_ascii_case("api.openai.com")
}

fn build_realtime_ws_url(provider: RealtimeProvider, audio_cfg: &AudioSegment) -> Result<String> {
    if provider == RealtimeProvider::Baidu {
        return build_baidu_ws_url(audio_cfg);
    }

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

fn build_baidu_ws_url(audio_cfg: &AudioSegment) -> Result<String> {
    let base = audio_cfg.realtime.ws_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::config(REALTIME_TAG, "realtime.ws_url is empty"));
    }

    let cfg_json = build_baidu_cfg(audio_cfg)?;
    let separator = if base.contains('?') { '&' } else { '?' };
    Ok(format!(
        "{base}{separator}app_id={}&ak={}&sk={}&ac={}&cfg={}",
        urlencoding::encode(audio_cfg.realtime.app_id.trim()),
        urlencoding::encode(audio_cfg.realtime.api_key.trim()),
        urlencoding::encode(audio_cfg.realtime.api_secret.trim()),
        REALTIME_BAIDU_AUDIO_CODEC,
        urlencoding::encode(cfg_json.as_str()),
    ))
}

fn build_baidu_cfg(audio_cfg: &AudioSegment) -> Result<String> {
    let mut cfg = json!({
        "audiocodec": REALTIME_BAIDU_AUDIO_CODEC,
        "dfda": true,
    });

    let user_id = audio_cfg.realtime.user_id.trim();
    if !user_id.is_empty() {
        cfg["user_id"] = serde_json::Value::String(user_id.to_string());
    }

    let instructions = audio_cfg.realtime.instructions.trim();
    if !instructions.is_empty() {
        cfg["sceneRoleCfg"] = json!({
            "prompt": instructions,
        });
    }

    serde_json::to_string(&cfg).map_err(|e| Error::config(REALTIME_TAG, e.to_string()))
}

fn build_session_update(provider: RealtimeProvider, audio_cfg: &AudioSegment) -> String {
    let mut session = json!({
        "modalities": ["text", "audio"],
        "voice": audio_cfg.realtime.voice.trim(),
        "input_audio_format": provider.input_audio_format(),
        "output_audio_format": provider.output_audio_format(),
        "turn_detection": build_turn_detection(provider, audio_cfg),
    });

    let instructions = audio_cfg.realtime.instructions.trim();
    if !instructions.is_empty() {
        session["instructions"] = serde_json::Value::String(instructions.to_string());
    }

    json!({
        "event_id": next_realtime_event_id("session"),
        "type": "session.update",
        "session": session,
    })
    .to_string()
}

fn next_realtime_event_id(prefix: &str) -> String {
    let seq = REALTIME_EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{seq}")
}

fn build_turn_detection(provider: RealtimeProvider, audio_cfg: &AudioSegment) -> serde_json::Value {
    match provider {
        RealtimeProvider::OpenAiCompatible => json!({
            "type": "server_vad",
            "threshold": audio_cfg.vad.threshold,
            "silence_duration_ms": audio_cfg.vad.silence_duration_ms,
            "prefix_padding_ms": REALTIME_SERVER_VAD_PREFIX_PADDING_MS,
            "idle_timeout_ms": REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS,
            "create_response": true,
            "interrupt_response": true
        }),
        RealtimeProvider::Qwen => json!({
            "type": "server_vad",
            "threshold": audio_cfg.vad.threshold,
            "silence_duration_ms": audio_cfg.vad.silence_duration_ms,
        }),
        RealtimeProvider::Baidu => serde_json::Value::Null,
    }
}

fn send_text_retry(conn: &mut dyn WssConnection, text: &str, attempts: usize) -> Result<()> {
    let mut last_err: Option<Error> = None;
    for _ in 0..attempts {
        crate::platform::task_wdt::feed_current_task();
        match conn.send_text(text) {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                crate::platform::task_wdt::feed_current_task();
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
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    timeout: Duration,
) -> Result<bool> {
    let mut wait = timeout;
    let mut saw_event = false;

    loop {
        crate::platform::task_wdt::feed_current_task();
        match conn.recv_timeout(wait)? {
            Some(WssEvent::Binary(data)) => {
                saw_event = true;
                let _ = process_server_frame(
                    conn,
                    platform,
                    state,
                    provider,
                    audio_cfg,
                    data.as_slice(),
                )?;
                wait = Duration::ZERO;
            }
            Some(WssEvent::Disconnected) => {
                return Err(Error::config(
                    REALTIME_TAG,
                    "realtime websocket disconnected",
                ));
            }
            Some(WssEvent::Closed(close)) => {
                return Err(Error::config(
                    REALTIME_TAG,
                    format!(
                        "realtime websocket closed by peer: {}",
                        summarize_close(close.as_ref())
                    ),
                ));
            }
            None => return Ok(!saw_event),
        }
    }
}

fn await_session_ready(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        crate::platform::task_wdt::feed_current_task();
        let wait = deadline.saturating_duration_since(Instant::now());
        match conn.recv_timeout(wait.min(Duration::from_millis(REALTIME_RECV_POLL_MS)))? {
            Some(WssEvent::Binary(data)) => {
                let ready = process_server_frame(
                    conn,
                    platform,
                    state,
                    provider,
                    audio_cfg,
                    data.as_slice(),
                )?;
                if ready {
                    return Ok(());
                }
            }
            Some(WssEvent::Disconnected) => {
                return Err(Error::config(
                    REALTIME_TAG,
                    "realtime websocket disconnected before session ready",
                ));
            }
            Some(WssEvent::Closed(close)) => {
                return Err(Error::config(
                    REALTIME_TAG,
                    format!(
                        "realtime websocket closed before session ready: {}",
                        summarize_close(close.as_ref())
                    ),
                ));
            }
            None => {}
        }
    }
    Err(Error::config(
        REALTIME_TAG,
        if provider == RealtimeProvider::Baidu {
            "timed out waiting for baidu realtime ready signal"
        } else {
            "timed out waiting for realtime session.updated"
        },
    ))
}

fn server_message_marks_session_ready(payload: &[u8]) -> Result<bool> {
    let value: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| Error::config("realtime_voice_parse", e.to_string()))?;
    Ok(matches!(
        value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "session.updated"
    ))
}

fn process_server_frame(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    payload: &[u8],
) -> Result<bool> {
    match provider {
        RealtimeProvider::Baidu => {
            handle_baidu_server_message(conn, platform, state, audio_cfg, payload)
        }
        _ => {
            let ready = server_message_marks_session_ready(payload)?;
            handle_json_server_message(platform, state, payload)?;
            Ok(ready)
        }
    }
}

fn handle_json_server_message(
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
            stop_audio_playback(state);
            state.last_activity = Instant::now();
            Ok(())
        }
        "response.done" => {
            let counted = state.awaiting_response;
            stop_audio_playback(state);
            if counted {
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

fn handle_baidu_server_message(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    payload: &[u8],
) -> Result<bool> {
    if let Some(text) = baidu_text_message(payload) {
        return handle_baidu_text_message(conn, state, audio_cfg, text);
    }

    let pcm = decode_pcm16_bytes(strip_baidu_audio_prefix(payload))?;
    if !pcm.is_empty() {
        if !state.audio_playing {
            crate::orchestrator::set_audio_playing(true);
            state.audio_playing = true;
        }
        platform.write_speaker_pcm_i16(&pcm)?;
        state.output_samples = state.output_samples.saturating_add(pcm.len());
        state.awaiting_response = true;
        state.last_activity = Instant::now();
    }
    Ok(false)
}

fn handle_baidu_text_message(
    conn: &mut dyn WssConnection,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    text: &str,
) -> Result<bool> {
    let message = text.trim();
    if message.is_empty() {
        return Ok(false);
    }

    if message.starts_with("[E]:[LIC]:[MUST]") {
        send_text_retry(
            conn,
            build_baidu_license_activation(audio_cfg)?.as_str(),
            REALTIME_INITIAL_SEND_RETRY_MAX,
        )?;
        state.last_activity = Instant::now();
        return Ok(false);
    }
    if message.starts_with("[E]:[LIC]:[RES]:[FAILED]") {
        return Err(Error::config(
            "realtime_voice_ws",
            format!("baidu realtime license activation failed: {}", message),
        ));
    }
    if message.starts_with("[E]:[MEDIA]:[READY]:1") {
        state.last_activity = Instant::now();
        return Ok(true);
    }
    if message.starts_with("[E]:[TTS_BEGIN_SPEAKING]")
        || message.starts_with("[E]:[VOICE_COMING]")
        || message.starts_with("[A]:")
        || message.starts_with("[Q]:")
    {
        state.awaiting_response = true;
        state.last_activity = Instant::now();
        return Ok(false);
    }
    if message.starts_with("[E]:[TTS_END_SPEAKING]") {
        let counted = state.awaiting_response || state.audio_playing;
        stop_audio_playback(state);
        if counted {
            state.awaiting_response = false;
            state.turns_completed = state.turns_completed.saturating_add(1);
        }
        state.last_activity = Instant::now();
        return Ok(false);
    }
    if message.starts_with("[E]:") && message.contains("[ERROR]") {
        return Err(Error::config(
            "realtime_voice_ws",
            format!("baidu realtime error: {}", message),
        ));
    }

    state.last_activity = Instant::now();
    Ok(false)
}

fn baidu_text_message(payload: &[u8]) -> Option<&str> {
    if payload.first().copied() != Some(b'[') {
        return None;
    }
    std::str::from_utf8(payload).ok()
}

fn build_baidu_license_activation(audio_cfg: &AudioSegment) -> Result<String> {
    let sn = audio_cfg.realtime.device_id.trim();
    let key = audio_cfg.realtime.license_key.trim();
    if sn.is_empty() || key.is_empty() {
        return Err(Error::config(
            REALTIME_TAG,
            "baidu realtime license activation requires realtime.device_id and realtime.license_key",
        ));
    }
    let uid = audio_cfg
        .realtime
        .user_id
        .trim()
        .strip_prefix('\u{feff}')
        .unwrap_or(audio_cfg.realtime.user_id.trim());
    let uid = if uid.is_empty() { sn } else { uid };
    Ok(format!(
        "[E]:[LIC]:[ACTIVE]:{}",
        json!({
            "sn": sn,
            "key": key,
            "uId": uid,
        })
    ))
}

fn strip_baidu_audio_prefix(payload: &[u8]) -> &[u8] {
    if !payload.starts_with(b"[A]:[PCM]:[RAW]:") {
        return payload;
    }

    let mut colon_count = 0usize;
    for (idx, byte) in payload.iter().enumerate() {
        if *byte == b':' {
            colon_count += 1;
            if colon_count == 9 {
                return payload.get(idx + 1..).unwrap_or(&[]);
            }
        }
    }
    payload
}

fn append_audio_frame(
    conn: &mut dyn WssConnection,
    encoder: &mut RealtimeUploadEncoder,
    provider: RealtimeProvider,
    pcm: &[i16],
) -> Result<()> {
    crate::platform::task_wdt::feed_current_task();
    match provider {
        RealtimeProvider::Baidu => {
            let mut bytes = Vec::with_capacity(pcm.len().saturating_mul(2));
            for sample in pcm {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            conn.send_binary_owned(bytes)
        }
        _ => conn.send_text(encoder.build_append_event(provider, pcm)),
    }
}

fn summarize_close(close: Option<&WssCloseInfo>) -> String {
    close
        .map(WssCloseInfo::summary)
        .unwrap_or_else(|| "peer closed".to_string())
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
    decode_pcm16_bytes(bytes.as_slice())
}

fn decode_pcm16_bytes(bytes: &[u8]) -> Result<Vec<i16>> {
    if bytes.len() % 2 != 0 {
        return Err(Error::config(
            "realtime_voice_parse",
            "pcm16 payload length must be even",
        ));
    }
    let mut pcm = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        pcm.push(i16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(pcm)
}

fn stop_audio_playback(state: &mut RealtimeLoopState) {
    if state.audio_playing {
        crate::orchestrator::set_audio_playing(false);
        state.audio_playing = false;
    }
}

fn samples_to_ms(samples: usize, sample_rate_hz: u32) -> u128 {
    let rate = sample_rate_hz.max(1) as u128;
    (samples as u128).saturating_mul(1000) / rate
}

#[cfg(test)]
mod tests {
    use super::{
        build_baidu_license_activation, build_realtime_headers, build_realtime_ws_url,
        build_session_update, realtime_ws_url_needs_openai_beta,
        server_message_marks_session_ready, strip_baidu_audio_prefix, RealtimeProvider,
        RealtimeUploadEncoder, REALTIME_OPENAI_BETA,
    };
    use crate::config::default_disabled_audio_segment;

    #[test]
    fn openai_headers_include_beta_flag() {
        let headers = build_realtime_headers(
            RealtimeProvider::OpenAiCompatible,
            &default_disabled_audio_segment(),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime",
        )
        .unwrap();
        assert!(headers
            .iter()
            .any(|(name, value)| *name == "OpenAI-Beta" && *value == REALTIME_OPENAI_BETA));
    }

    #[test]
    fn compatible_non_openai_url_skips_beta_flag() {
        let headers = build_realtime_headers(
            RealtimeProvider::OpenAiCompatible,
            &default_disabled_audio_segment(),
            "wss://example.com/v1/realtime?model=gpt-realtime",
        )
        .unwrap();
        assert!(!headers.iter().any(|(name, _)| *name == "OpenAI-Beta"));
    }

    #[test]
    fn openai_beta_detection_matches_authority_only() {
        assert!(realtime_ws_url_needs_openai_beta(
            "wss://api.openai.com/v1/realtime?model=gpt-realtime"
        ));
        assert!(realtime_ws_url_needs_openai_beta(
            "wss://api.openai.com:443/v1/realtime"
        ));
        assert!(!realtime_ws_url_needs_openai_beta(
            "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime"
        ));
    }

    #[test]
    fn qwen_resampler_converts_24k_to_16k_with_tail() {
        let mut encoder = RealtimeUploadEncoder::new();
        let first = encoder.downsample_24k_to_16k(&[1, 2, 3, 4]);
        assert_eq!(first, &[1, 2]);
        let second = encoder.downsample_24k_to_16k(&[5, 6]);
        assert_eq!(second, &[4, 5]);
    }

    #[test]
    fn qwen_session_update_uses_pcm_audio_format() {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.provider = "qwen".to_string();
        cfg.realtime.model = "qwen3.5-omni-plus-realtime".to_string();
        cfg.realtime.voice = "Cherry".to_string();
        cfg.realtime.api_key = "token".to_string();
        cfg.realtime.ws_url = "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime".to_string();
        let payload = build_session_update(RealtimeProvider::Qwen, &cfg);
        assert!(payload.contains("\"input_audio_format\":\"pcm\""));
        assert!(payload.contains("\"output_audio_format\":\"pcm\""));
        assert!(!payload.contains("prefix_padding_ms"));
        assert!(!payload.contains("idle_timeout_ms"));
        assert!(!payload.contains("create_response"));
    }

    #[test]
    fn qwen_audio_append_contains_event_id() {
        let mut encoder = RealtimeUploadEncoder::new();
        let payload = encoder.build_append_event(RealtimeProvider::Qwen, &[1, 2, 3]);
        assert!(payload.contains("\"event_id\":\"audio_"));
        assert!(payload.contains("\"type\":\"input_audio_buffer.append\""));
    }

    #[test]
    fn session_update_contains_event_id() {
        let cfg = default_disabled_audio_segment();
        let payload = build_session_update(RealtimeProvider::OpenAiCompatible, &cfg);
        assert!(payload.contains("\"event_id\":\"session_"));
    }

    #[test]
    fn session_updated_marks_ready() {
        assert!(server_message_marks_session_ready(br#"{"type":"session.updated"}"#).unwrap());
        assert!(!server_message_marks_session_ready(br#"{"type":"session.created"}"#).unwrap());
        assert!(!server_message_marks_session_ready(br#"{"type":"response.created"}"#).unwrap());
    }

    #[test]
    fn baidu_ws_url_contains_direct_auth_params() {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.provider = "baidu".to_string();
        cfg.realtime.ws_url = "wss://rtc-aiotgw.exp.bcelive.com/v1/realtime".to_string();
        cfg.realtime.app_id = "app-1".to_string();
        cfg.realtime.api_key = "ak-1".to_string();
        cfg.realtime.api_secret = "sk-1".to_string();
        cfg.realtime.instructions = "请默认中文简洁回复".to_string();

        let url = build_realtime_ws_url(RealtimeProvider::Baidu, &cfg).unwrap();
        assert!(url.contains("app_id=app-1"));
        assert!(url.contains("ak=ak-1"));
        assert!(url.contains("sk=sk-1"));
        assert!(url.contains("ac=raw16k"));
        assert!(url.contains("cfg="));
    }

    #[test]
    fn baidu_license_activation_uses_user_or_device_id() {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.license_key = "lic-key".to_string();
        cfg.realtime.device_id = "dev-1".to_string();

        let payload = build_baidu_license_activation(&cfg).unwrap();
        assert!(payload.contains("\"sn\":\"dev-1\""));
        assert!(payload.contains("\"key\":\"lic-key\""));
        assert!(payload.contains("\"uId\":\"dev-1\""));
    }

    #[test]
    fn strip_baidu_prefixed_audio_payload() {
        let payload = b"[A]:[PCM]:[RAW]:0:1:2:3:4:5:\x01\x00\x02\x00";
        assert_eq!(strip_baidu_audio_prefix(payload), b"\x01\x00\x02\x00");
    }
}
