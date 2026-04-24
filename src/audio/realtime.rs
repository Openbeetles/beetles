//! 实时语音会话：唤醒后建立 WSS，会话内持续上送 PCM，并接收模型返回的语音增量。
//! Realtime voice session over WSS: stream PCM in, play audio deltas out.

use crate::audio::capture::AudioRecordingGuard;
use crate::audio::energy::{normalized_rms, EndpointConfig, EndpointEvent, EndpointState};
use crate::channels::{WssCloseInfo, WssConnection, WssEvent};
use crate::config::{
    audio_realtime_enabled, AudioSegment, AUDIO_REALTIME_PROVIDER_DOUBAO,
    AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE, AUDIO_REALTIME_PROVIDER_QWEN,
};
use crate::constants::{AUDIO_CAPTURE_FRAME_SAMPLES, AUDIO_TTS_WRITE_CHUNK_SAMPLES};
use crate::error::{Error, Result};
use crate::platform::AudioDuplexCapabilities;
use crate::Platform;
use base64::Engine;
use serde_json::json;
use std::io::Read as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const REALTIME_TAG: &str = "audio::realtime";
const REALTIME_OPENAI_BETA: &str = "realtime=v1";
const REALTIME_INITIAL_SEND_RETRY_MS: u64 = 100;
const REALTIME_INITIAL_SEND_RETRY_MAX: usize = 50;
const REALTIME_RECV_POLL_MS: u64 = 20;
const REALTIME_SESSION_READY_TIMEOUT_MS: u64 = 1_500;
const REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS: u32 = 8_000;
const REALTIME_SERVER_VAD_PREFIX_PADDING_MS: u32 = 300;
const REALTIME_NO_SPEECH_TIMEOUT_MS: u64 = 4_000;
const REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS: u64 = 5_000;
const REALTIME_RESPONSE_WAIT_TIMEOUT_MS: u64 = 8_000;
const REALTIME_ENDPOINT_THRESHOLD_MAX: f32 = 0.12;
// Realtime downlink is raw PCM over WSS, so ESP keeps a modest software buffer
// to absorb normal Wi-Fi jitter before releasing audio to the speaker.
const REALTIME_PLAYBACK_TARGET_BUFFER_MS: u32 = 800;
const REALTIME_INTERRUPT_BASELINE_MS: u64 = 180;
const REALTIME_INTERRUPT_SPEECH_MIN_MS: u32 = 180;
const REALTIME_INTERRUPT_THRESHOLD_MIN: f32 = 0.18;
const REALTIME_INTERRUPT_THRESHOLD_MARGIN: f32 = 0.08;
const REALTIME_INTERRUPT_THRESHOLD_MULTIPLIER: f32 = 2.0;
const REALTIME_INTERRUPT_REFERENCE_ACTIVE_MIN: f32 = 0.06;
const REALTIME_INTERRUPT_REFERENCE_SUBTRACT_SCALE: f32 = 0.65;
const REALTIME_LOCAL_SPEECH_COMMIT_MIN_MS: u32 = 160;
const REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS: u32 = 12_000;
static REALTIME_EVENT_COUNTER: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RealtimeProvider {
    OpenAiCompatible,
    Qwen,
    Doubao,
}

impl RealtimeProvider {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE => Ok(Self::OpenAiCompatible),
            AUDIO_REALTIME_PROVIDER_QWEN => Ok(Self::Qwen),
            AUDIO_REALTIME_PROVIDER_DOUBAO => Ok(Self::Doubao),
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
            Self::Doubao => "pcm16",
        }
    }

    fn output_audio_format(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "pcm16",
            Self::Qwen => "pcm",
            Self::Doubao => "pcm16",
        }
    }

    fn requires_openai_beta_header(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    fn requires_explicit_turn_submit(self) -> bool {
        matches!(self, Self::Doubao)
    }

    fn session_created_is_ready(self) -> bool {
        matches!(self, Self::Doubao)
    }
}

pub struct RealtimeSessionResult {
    pub turns_completed: u32,
    pub input_audio_ms: u128,
    pub output_audio_ms: u128,
    pub session_ms: u128,
}

pub(crate) struct ConnectedRealtimeSession {
    conn: Box<dyn WssConnection>,
    provider: RealtimeProvider,
    duplex_caps: AudioDuplexCapabilities,
    session_start: Instant,
    session_ready_at: Instant,
}

enum RealtimeExitReason {
    NoSpeech,
    ResponseWait,
    PostPlaybackIdle,
}

struct RealtimeLoopState {
    duplex_caps: AudioDuplexCapabilities,
    awaiting_response: bool,
    audio_playing: bool,
    session_ready: bool,
    suppress_server_audio_until_turn_end: bool,
    turns_completed: u32,
    input_samples: usize,
    output_samples: usize,
    session_ready_at: Option<Instant>,
    first_local_speech_at: Option<Instant>,
    last_local_speech_end_at: Option<Instant>,
    playback_finished_at: Option<Instant>,
    current_turn_received_server_activity: bool,
    interrupt_baseline_deadline: Option<Instant>,
    interrupt_baseline_peak: f32,
    interrupt_speech_ms: u32,
    current_local_speech_ms: u32,
    current_local_turn_committed: bool,
    local_turn_generation: u32,
    server_response_generation: u32,
    output_pcm_decode_buf: Vec<i16>,
    last_activity: Instant,
}

impl RealtimeLoopState {
    fn new(duplex_caps: AudioDuplexCapabilities) -> Self {
        Self {
            duplex_caps,
            awaiting_response: false,
            audio_playing: false,
            session_ready: false,
            suppress_server_audio_until_turn_end: false,
            turns_completed: 0,
            input_samples: 0,
            output_samples: 0,
            session_ready_at: None,
            first_local_speech_at: None,
            last_local_speech_end_at: None,
            playback_finished_at: None,
            current_turn_received_server_activity: false,
            interrupt_baseline_deadline: None,
            interrupt_baseline_peak: 0.0,
            interrupt_speech_ms: 0,
            current_local_speech_ms: 0,
            current_local_turn_committed: false,
            local_turn_generation: 0,
            server_response_generation: 0,
            output_pcm_decode_buf: Vec::new(),
            last_activity: Instant::now(),
        }
    }

    fn mark_session_ready(&mut self, now: Instant) {
        self.session_ready = true;
        self.session_ready_at = Some(now);
        self.last_activity = now;
    }

    fn begin_local_speech_window(&mut self, now: Instant, frame_ms: u32) {
        self.current_local_speech_ms = frame_ms;
        self.current_local_turn_committed = false;
        self.last_activity = now;
    }

    fn extend_local_speech_window(&mut self, now: Instant, frame_ms: u32) {
        self.current_local_speech_ms = self.current_local_speech_ms.saturating_add(frame_ms);
        if !self.current_local_turn_committed
            && self.current_local_speech_ms >= REALTIME_LOCAL_SPEECH_COMMIT_MIN_MS
        {
            self.commit_local_turn(now);
        } else {
            self.last_activity = now;
        }
    }

    fn finish_local_speech_window(&mut self, now: Instant) -> bool {
        let should_submit = self.current_local_turn_committed;
        if self.current_local_turn_committed {
            self.mark_local_turn_submitted(now);
        }
        self.current_local_speech_ms = 0;
        self.current_local_turn_committed = false;
        self.last_activity = now;
        should_submit
    }

    fn reset_local_speech_window(&mut self) {
        self.current_local_speech_ms = 0;
        self.current_local_turn_committed = false;
    }

    fn commit_local_turn(&mut self, now: Instant) {
        if self.current_local_turn_committed {
            self.last_activity = now;
            return;
        }
        self.current_local_turn_committed = true;
        self.local_turn_generation = next_turn_generation(self.local_turn_generation);
        self.server_response_generation = 0;
        if self.first_local_speech_at.is_none() {
            self.first_local_speech_at = Some(now);
        }
        self.awaiting_response = false;
        self.current_turn_received_server_activity = false;
        self.last_local_speech_end_at = None;
        self.playback_finished_at = None;
        self.last_activity = now;
    }

    fn mark_server_response_activity(&mut self, now: Instant) {
        self.current_turn_received_server_activity = true;
        self.last_activity = now;
    }

    fn mark_server_response_created(&mut self, now: Instant) {
        self.awaiting_response = true;
        self.server_response_generation = self.local_turn_generation;
        self.mark_server_response_activity(now);
    }

    fn mark_local_turn_submitted(&mut self, now: Instant) {
        self.awaiting_response = true;
        self.current_turn_received_server_activity = false;
        self.last_local_speech_end_at = Some(now);
        self.last_activity = now;
    }

    fn drop_server_audio_for_stale_turn(&mut self, now: Instant) {
        let _ = now;
        crate::metrics::record_voice_stale_audio_drop();
    }

    fn should_accept_server_audio(&self) -> bool {
        self.server_response_generation == self.local_turn_generation
    }

    fn has_committed_local_turn(&self) -> bool {
        self.local_turn_generation != 0 || self.first_local_speech_at.is_some()
    }

    fn start_audio_playback(&mut self, now: Instant) {
        if self.audio_playing {
            return;
        }
        self.audio_playing = true;
        self.playback_finished_at = None;
        self.interrupt_baseline_deadline =
            Some(now + Duration::from_millis(REALTIME_INTERRUPT_BASELINE_MS));
        self.interrupt_baseline_peak = 0.0;
        self.interrupt_speech_ms = 0;
        crate::orchestrator::set_audio_playing(true);
        crate::orchestrator::set_audio_interrupt_listening(self.duplex_caps.supports_barge_in());
    }

    fn finish_audio_playback(&mut self, now: Instant) {
        if !self.audio_playing {
            return;
        }
        self.audio_playing = false;
        self.playback_finished_at = Some(now);
        self.interrupt_baseline_deadline = None;
        self.interrupt_baseline_peak = 0.0;
        self.interrupt_speech_ms = 0;
        crate::orchestrator::set_audio_interrupt_listening(false);
        crate::orchestrator::set_audio_playing(false);
    }
}

struct RealtimeSessionCleanup<'a> {
    platform: &'a dyn Platform,
}

impl<'a> RealtimeSessionCleanup<'a> {
    fn new(platform: &'a dyn Platform) -> Self {
        crate::orchestrator::clear_audio_interrupt_request();
        crate::orchestrator::set_audio_interrupt_listening(false);
        Self { platform }
    }
}

impl Drop for RealtimeSessionCleanup<'_> {
    fn drop(&mut self) {
        let _ = self.platform.clear_speaker_buffer();
        crate::orchestrator::set_audio_interrupt_listening(false);
        crate::orchestrator::clear_audio_interrupt_request();
        crate::orchestrator::set_audio_playing(false);
    }
}

fn next_turn_generation(current: u32) -> u32 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

fn realtime_endpoint_threshold(cfg_threshold: f32) -> f32 {
    cfg_threshold
        .clamp(0.0, 1.0)
        .min(REALTIME_ENDPOINT_THRESHOLD_MAX)
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
        let b64_capacity = pcm_capacity.div_ceil(3) * 4;
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
            RealtimeProvider::OpenAiCompatible | RealtimeProvider::Doubao => {
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

pub(crate) fn connect_realtime_session(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    log_tag: &'static str,
) -> Result<ConnectedRealtimeSession> {
    if !audio_realtime_enabled(audio_cfg) {
        return Err(Error::config(
            REALTIME_TAG,
            "connect_realtime_session called without realtime config",
        ));
    }

    let session_start = Instant::now();
    let duplex_caps = platform.audio_duplex_capabilities().normalized();
    if !duplex_caps.can_run_realtime_session() {
        return Err(Error::config(
            REALTIME_TAG,
            format!(
                "realtime session requires duplex audio contract, got {}",
                duplex_caps.profile().as_str()
            ),
        ));
    }
    let provider = RealtimeProvider::parse(audio_cfg.realtime.provider.trim())?;
    let ws_url = build_realtime_ws_url(provider, audio_cfg)?;
    log::info!(
        "[{}] realtime ws connect provider={} audio_profile={} reference={:?} aec={:?} url={}",
        log_tag,
        audio_cfg.realtime.provider.trim(),
        duplex_caps.profile().as_str(),
        duplex_caps.reference_capture,
        duplex_caps.echo_cancellation,
        redact_realtime_ws_url(provider, ws_url.as_str())
    );
    let headers = build_realtime_headers(provider, audio_cfg, ws_url.as_str())?;
    let header_refs: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let mut conn =
        crate::network::connect_realtime_wss_with_retry(platform, ws_url.as_str(), &header_refs)?;
    let mut state = RealtimeLoopState::new(duplex_caps);

    send_text_retry(
        conn.as_mut(),
        build_session_update(provider, audio_cfg).as_str(),
        REALTIME_INITIAL_SEND_RETRY_MAX,
    )?;
    await_session_ready(
        conn.as_mut(),
        platform,
        &mut state,
        provider,
        audio_cfg,
        Duration::from_millis(REALTIME_SESSION_READY_TIMEOUT_MS),
    )?;
    let session_ready_at = Instant::now();
    log::info!("[{}] realtime session connected", log_tag);

    Ok(ConnectedRealtimeSession {
        conn,
        provider,
        duplex_caps,
        session_start,
        session_ready_at,
    })
}

pub(crate) fn run_connected_realtime_session(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    connected: ConnectedRealtimeSession,
) -> Result<RealtimeSessionResult> {
    let _recording_guard = AudioRecordingGuard::new();
    let _cleanup = RealtimeSessionCleanup::new(platform);
    let ConnectedRealtimeSession {
        mut conn,
        provider,
        duplex_caps,
        session_start,
        session_ready_at,
    } = connected;
    let mut state = RealtimeLoopState::new(duplex_caps);
    state.mark_session_ready(session_ready_at);
    let mut mic_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut reference_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut upload_encoder = RealtimeUploadEncoder::new();
    let frame_ms = ((AUDIO_CAPTURE_FRAME_SAMPLES as u64) * 1000
        / (audio_cfg.microphone.sample_rate.max(8_000) as u64))
        .clamp(1, 40) as u32;
    let endpoint_cfg = EndpointConfig {
        threshold: realtime_endpoint_threshold(audio_cfg.vad.threshold),
        silence_duration_ms: audio_cfg.vad.silence_duration_ms,
    };
    if endpoint_cfg.threshold < audio_cfg.vad.threshold {
        log::info!(
            "[{}] realtime endpoint threshold capped from {:.3} to {:.3}",
            REALTIME_TAG,
            audio_cfg.vad.threshold,
            endpoint_cfg.threshold
        );
    }
    let mut endpoint = EndpointState::new();
    let mut local_speech_active = false;

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
        let now = Instant::now();
        update_playback_state(platform, &mut state, audio_cfg.speaker.sample_rate, now)?;
        if crate::orchestrator::take_audio_interrupt_request() {
            handle_local_interrupt(conn.as_mut(), platform, &mut state, provider, now)?;
        }
        if timed_out {
            if let Some(reason) = should_exit_realtime_session(&state, now) {
                match reason {
                    RealtimeExitReason::NoSpeech => {
                        crate::metrics::record_voice_no_speech_timeout()
                    }
                    RealtimeExitReason::ResponseWait => {
                        crate::metrics::record_voice_response_wait_timeout()
                    }
                    RealtimeExitReason::PostPlaybackIdle => {
                        crate::metrics::record_voice_post_playback_timeout()
                    }
                }
                break;
            }
        }

        if should_hold_local_capture_for_server_response(provider, &state) {
            crate::platform::task_wdt::feed_current_task();
            thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
            continue;
        }

        if state.audio_playing && !crate::orchestrator::is_audio_interrupt_listening() {
            crate::platform::task_wdt::feed_current_task();
            thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
            continue;
        }

        let n = platform.read_mic_pcm_i16(&mut mic_frame)?;
        if n == 0 {
            crate::platform::task_wdt::feed_current_task();
            thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
            continue;
        }

        let chunk = &mic_frame[..n.min(mic_frame.len())];
        let reference_samples = if state.audio_playing {
            read_reference_frame_if_available(platform, &duplex_caps, &mut reference_frame)
        } else {
            0
        };
        let reference_chunk = &reference_frame[..reference_samples.min(reference_frame.len())];
        let now = Instant::now();
        let playback_capture_suspended = should_suspend_capture_upload(&state);
        let mut interrupted_this_frame = false;
        if state.audio_playing && playback_capture_suspended {
            if should_trigger_playback_interrupt(
                &mut state,
                audio_cfg,
                chunk,
                reference_chunk,
                frame_ms,
                now,
            ) {
                crate::metrics::record_voice_interrupt_requested();
                handle_local_interrupt(conn.as_mut(), platform, &mut state, provider, now)?;
                reset_local_endpoint_window(&mut state, &mut endpoint, &mut local_speech_active);
                interrupted_this_frame = true;
            } else {
                reset_local_endpoint_window(&mut state, &mut endpoint, &mut local_speech_active);
                update_playback_state(
                    platform,
                    &mut state,
                    audio_cfg.speaker.sample_rate,
                    Instant::now(),
                )?;
                continue;
            }
        }

        match endpoint.update(chunk, frame_ms, &endpoint_cfg) {
            EndpointEvent::SpeechStart => {
                local_speech_active = true;
                state.begin_local_speech_window(now, frame_ms);
            }
            EndpointEvent::SpeechEnd => {
                if local_speech_active {
                    let should_submit = state.finish_local_speech_window(now);
                    if should_submit {
                        submit_local_turn(conn.as_mut(), provider, audio_cfg)?;
                    }
                    local_speech_active = false;
                }
            }
            EndpointEvent::None => {
                if local_speech_active {
                    state.extend_local_speech_window(now, frame_ms);
                }
            }
        }

        if local_speech_active
            && state.current_local_speech_ms >= REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS
        {
            log::info!(
                "[{}] force closing long local speech window at {}ms without endpoint release",
                REALTIME_TAG,
                state.current_local_speech_ms
            );
            let should_submit = state.finish_local_speech_window(now);
            if should_submit {
                submit_local_turn(conn.as_mut(), provider, audio_cfg)?;
            }
            endpoint.reset();
            local_speech_active = false;
        }

        if local_speech_active
            && state.audio_playing
            && should_trigger_playback_interrupt(
                &mut state,
                audio_cfg,
                chunk,
                reference_chunk,
                frame_ms,
                now,
            )
        {
            crate::metrics::record_voice_interrupt_requested();
            handle_local_interrupt(conn.as_mut(), platform, &mut state, provider, now)?;
            reset_local_endpoint_window(&mut state, &mut endpoint, &mut local_speech_active);
            interrupted_this_frame = true;
        }

        if !playback_capture_suspended || interrupted_this_frame {
            append_audio_frame(conn.as_mut(), &mut upload_encoder, provider, chunk)?;
            state.input_samples = state.input_samples.saturating_add(n);
        }
        update_playback_state(
            platform,
            &mut state,
            audio_cfg.speaker.sample_rate,
            Instant::now(),
        )?;
    }

    Ok(RealtimeSessionResult {
        turns_completed: state.turns_completed,
        input_audio_ms: samples_to_ms(state.input_samples, audio_cfg.microphone.sample_rate),
        output_audio_ms: samples_to_ms(state.output_samples, audio_cfg.speaker.sample_rate),
        session_ms: session_start.elapsed().as_millis(),
    })
}

fn build_realtime_headers(
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    ws_url: &str,
) -> Result<Vec<(&'static str, String)>> {
    let mut headers = Vec::new();
    headers.push((
        "Authorization",
        format!("Bearer {}", audio_cfg.realtime.api_key.trim()),
    ));
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
    let _ = provider;
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

fn redact_realtime_ws_url(provider: RealtimeProvider, url: &str) -> String {
    let _ = provider;
    crate::util::scrub_credentials(url)
}

fn build_session_update(provider: RealtimeProvider, audio_cfg: &AudioSegment) -> String {
    let mut session = json!({
        "modalities": ["text", "audio"],
        "voice": audio_cfg.realtime.voice.trim(),
        "input_audio_format": provider.input_audio_format(),
        "output_audio_format": provider.output_audio_format(),
    });
    let turn_detection = build_turn_detection(provider, audio_cfg);
    if !turn_detection.is_null() {
        session["turn_detection"] = turn_detection;
    }

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
        RealtimeProvider::Doubao => serde_json::Value::Null,
    }
}

fn build_input_audio_commit_event() -> String {
    json!({
        "event_id": next_realtime_event_id("commit"),
        "type": "input_audio_buffer.commit",
    })
    .to_string()
}

fn build_response_create_event(provider: RealtimeProvider, audio_cfg: &AudioSegment) -> String {
    let mut response = json!({
        "modalities": ["text", "audio"],
        "voice": audio_cfg.realtime.voice.trim(),
        "output_audio_format": provider.output_audio_format(),
    });
    let instructions = audio_cfg.realtime.instructions.trim();
    if !instructions.is_empty() {
        response["instructions"] = serde_json::Value::String(instructions.to_string());
    }
    json!({
        "event_id": next_realtime_event_id("response"),
        "type": "response.create",
        "response": response,
    })
    .to_string()
}

fn submit_local_turn(
    conn: &mut dyn WssConnection,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
) -> Result<()> {
    if !provider.requires_explicit_turn_submit() {
        return Ok(());
    }
    send_text_retry(
        conn,
        build_input_audio_commit_event().as_str(),
        REALTIME_INITIAL_SEND_RETRY_MAX,
    )?;
    send_text_retry(
        conn,
        build_response_create_event(provider, audio_cfg).as_str(),
        REALTIME_INITIAL_SEND_RETRY_MAX,
    )
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
        "timed out waiting for realtime session ready signal",
    ))
}

#[cfg(test)]
fn server_message_marks_session_ready(provider: RealtimeProvider, payload: &[u8]) -> Result<bool> {
    let value = parse_server_message(payload)?;
    Ok(server_message_value_marks_session_ready(provider, &value))
}

fn parse_server_message(payload: &[u8]) -> Result<serde_json::Value> {
    serde_json::from_slice(payload)
        .map_err(|e| Error::config("realtime_voice_parse", e.to_string()))
}

fn server_message_value_marks_session_ready(
    provider: RealtimeProvider,
    value: &serde_json::Value,
) -> bool {
    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    event_type == "session.updated"
        || (provider.session_created_is_ready() && event_type == "session.created")
}

fn process_server_frame(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    payload: &[u8],
) -> Result<bool> {
    let _ = conn;
    let value = parse_server_message(payload)?;
    let ready = server_message_value_marks_session_ready(provider, &value);
    handle_json_server_message(platform, state, provider, audio_cfg, &value)?;
    Ok(ready)
}

fn handle_json_server_message(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    value: &serde_json::Value,
) -> Result<()> {
    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let now = Instant::now();

    match event_type {
        "session.created" | "session.updated" | "input_audio_buffer.committed" => {
            if (event_type == "session.updated"
                || (provider.session_created_is_ready() && event_type == "session.created"))
                && !state.session_ready
            {
                state.mark_session_ready(now);
            } else {
                state.last_activity = now;
            }
            Ok(())
        }
        "response.created" => {
            if state.suppress_server_audio_until_turn_end {
                state.last_activity = now;
                return Ok(());
            }
            if !state.has_committed_local_turn() {
                state.drop_server_audio_for_stale_turn(now);
                return Ok(());
            }
            state.mark_server_response_created(now);
            Ok(())
        }
        "input_audio_buffer.speech_started" => {
            if state.has_committed_local_turn() {
                state.awaiting_response = false;
            }
            state.last_activity = now;
            Ok(())
        }
        "input_audio_buffer.speech_stopped" => {
            if state.has_committed_local_turn() {
                state.awaiting_response = true;
            }
            state.last_activity = now;
            Ok(())
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            if state.suppress_server_audio_until_turn_end {
                state.drop_server_audio_for_stale_turn(now);
                return Ok(());
            }
            if !state.has_committed_local_turn() || !state.should_accept_server_audio() {
                state.drop_server_audio_for_stale_turn(now);
                return Ok(());
            }
            let delta = value
                .get("delta")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::config("realtime_voice_parse", "audio delta missing"))?;
            if queue_pcm16_delta_audio(platform, state, audio_cfg.speaker.sample_rate, delta)? {
                state.awaiting_response = true;
                state.mark_server_response_activity(now);
            }
            Ok(())
        }
        "response.output_audio.done" | "response.audio.done" => {
            // Data already in staging; worker transfers to speaker autonomously.
            state.last_activity = now;
            Ok(())
        }
        "response.done" => {
            if state.suppress_server_audio_until_turn_end {
                state.suppress_server_audio_until_turn_end = false;
                state.last_activity = now;
                return Ok(());
            }
            let counted = state.awaiting_response;
            // Data already in staging; worker handles transfer.
            if counted {
                state.awaiting_response = false;
                state.turns_completed = state.turns_completed.saturating_add(1);
            }
            if !state.audio_playing {
                state.playback_finished_at = Some(now);
            }
            state.last_activity = now;
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

fn append_audio_frame(
    conn: &mut dyn WssConnection,
    encoder: &mut RealtimeUploadEncoder,
    provider: RealtimeProvider,
    pcm: &[i16],
) -> Result<()> {
    crate::platform::task_wdt::feed_current_task();
    conn.send_text(encoder.build_append_event(provider, pcm))
}

fn summarize_close(close: Option<&WssCloseInfo>) -> String {
    close
        .map(WssCloseInfo::summary)
        .unwrap_or_else(|| "peer closed".to_string())
}

fn decode_pcm16_delta_into(delta_b64: &str, out: &mut Vec<i16>) -> Result<()> {
    out.clear();
    let mut reader = base64::read::DecoderReader::new(
        delta_b64.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    let mut buf = [0u8; 512];
    let mut pending_low: Option<u8> = None;
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            Error::config(
                "realtime_voice_parse",
                format!("audio base64 decode failed: {}", e),
            )
        })?;
        if n == 0 {
            break;
        }
        let mut start = 0usize;
        if let Some(low) = pending_low.take() {
            out.push(i16::from_le_bytes([low, buf[0]]));
            start = 1;
        }
        let chunks = buf[start..n].chunks_exact(2);
        for chunk in chunks.clone() {
            out.push(i16::from_le_bytes([chunk[0], chunk[1]]));
        }
        if let Some(&low) = chunks.remainder().first() {
            pending_low = Some(low);
        }
    }
    if pending_low.is_some() {
        out.clear();
        return Err(Error::config(
            "realtime_voice_parse",
            "pcm16 payload length must be even",
        ));
    }
    Ok(())
}

fn queue_pcm16_delta_audio(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    sample_rate_hz: u32,
    delta_b64: &str,
) -> Result<bool> {
    let mut pcm = std::mem::take(&mut state.output_pcm_decode_buf);
    let result = (|| {
        decode_pcm16_delta_into(delta_b64, &mut pcm)?;
        if pcm.is_empty() {
            return Ok(false);
        }
        queue_output_audio(platform, state, sample_rate_hz, pcm.as_slice())?;
        Ok(true)
    })();
    pcm.clear();
    state.output_pcm_decode_buf = pcm;
    result
}

fn playback_target_buffer_samples(sample_rate_hz: u32) -> usize {
    ((sample_rate_hz.max(1) as usize) * (REALTIME_PLAYBACK_TARGET_BUFFER_MS as usize) / 1000)
        .max(AUDIO_TTS_WRITE_CHUNK_SAMPLES)
}

fn queue_output_audio(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    sample_rate_hz: u32,
    pcm: &[i16],
) -> Result<()> {
    if pcm.is_empty() {
        return Ok(());
    }
    state.output_samples = state.output_samples.saturating_add(pcm.len());

    // Push into the staging ring buffer; worker transfers staging → speaker.
    // On Linux, push_speaker_staging_pcm_i16 writes directly to the speaker
    // channel (no separate staging), so the fallback below is a second attempt
    // at the same queue — acceptable since Linux lacks WiFi jitter concerns.
    let written = platform.push_speaker_staging_pcm_i16(pcm)?;
    if written < pcm.len() {
        // Staging full — fallback: write remainder directly to speaker ring buffer.
        let remaining = &pcm[written..];
        let _ = platform.try_write_speaker_pcm_i16(remaining)?;
    }

    // Start playback once enough data is buffered (staging + speaker combined).
    if !state.audio_playing {
        let total = platform
            .speaker_buffered_samples()
            .saturating_add(platform.speaker_staging_samples());
        if total >= playback_target_buffer_samples(sample_rate_hz) {
            state.start_audio_playback(Instant::now());
        }
    }
    Ok(())
}

fn update_playback_state(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    sample_rate_hz: u32,
    now: Instant,
) -> Result<()> {
    if !state.audio_playing {
        // Check if worker has transferred enough staging → speaker to start.
        let total = platform
            .speaker_buffered_samples()
            .saturating_add(platform.speaker_staging_samples());
        if total >= playback_target_buffer_samples(sample_rate_hz) {
            state.start_audio_playback(now);
        }
        return Ok(());
    }
    // Playback finished when both staging and speaker are drained.
    if platform.speaker_staging_samples() == 0 && platform.speaker_buffered_samples() == 0 {
        state.finish_audio_playback(now);
    }
    Ok(())
}

fn reset_local_endpoint_window(
    state: &mut RealtimeLoopState,
    endpoint: &mut EndpointState,
    local_speech_active: &mut bool,
) {
    endpoint.reset();
    *local_speech_active = false;
    state.reset_local_speech_window();
}

fn should_suspend_capture_upload(state: &RealtimeLoopState) -> bool {
    state.audio_playing
        && state
            .duplex_caps
            .requires_capture_upload_suspend_during_playback()
}

fn read_reference_frame_if_available(
    platform: &dyn Platform,
    duplex_caps: &AudioDuplexCapabilities,
    out: &mut [i16],
) -> usize {
    if !duplex_caps.has_reference_capture() {
        return 0;
    }
    match platform.read_playback_reference_pcm_i16(out) {
        Ok(n) => n.min(out.len()),
        Err(error) => {
            log::debug!(
                "[{}] playback reference read unavailable for this frame: {}",
                REALTIME_TAG,
                error
            );
            0
        }
    }
}

fn reference_adjusted_interrupt_rms(mic_pcm: &[i16], reference_pcm: &[i16]) -> (f32, f32) {
    let mic_rms = normalized_rms(mic_pcm);
    if reference_pcm.is_empty() {
        return (mic_rms, mic_rms);
    }
    let reference_rms = normalized_rms(reference_pcm);
    if reference_rms < REALTIME_INTERRUPT_REFERENCE_ACTIVE_MIN {
        return (mic_rms, mic_rms);
    }
    let adjusted = (mic_rms - reference_rms * REALTIME_INTERRUPT_REFERENCE_SUBTRACT_SCALE).max(0.0);
    (adjusted, mic_rms)
}

fn should_trigger_playback_interrupt(
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    pcm: &[i16],
    reference_pcm: &[i16],
    frame_ms: u32,
    now: Instant,
) -> bool {
    let (rms, raw_mic_rms) = reference_adjusted_interrupt_rms(pcm, reference_pcm);
    if let Some(deadline) = state.interrupt_baseline_deadline {
        if now < deadline {
            state.interrupt_baseline_peak = state.interrupt_baseline_peak.max(rms);
            return false;
        }
        state.interrupt_baseline_deadline = None;
    }

    let threshold = (audio_cfg.vad.threshold * REALTIME_INTERRUPT_THRESHOLD_MULTIPLIER)
        .max(state.interrupt_baseline_peak + REALTIME_INTERRUPT_THRESHOLD_MARGIN)
        .clamp(REALTIME_INTERRUPT_THRESHOLD_MIN, 0.95);
    if !reference_pcm.is_empty() && raw_mic_rms >= threshold && rms < threshold {
        crate::metrics::record_voice_interrupt_reference_suppressed();
    }
    if rms >= threshold {
        state.interrupt_speech_ms = state.interrupt_speech_ms.saturating_add(frame_ms);
    } else {
        state.interrupt_speech_ms = 0;
    }
    state.interrupt_speech_ms >= REALTIME_INTERRUPT_SPEECH_MIN_MS
}

fn should_exit_realtime_session(
    state: &RealtimeLoopState,
    now: Instant,
) -> Option<RealtimeExitReason> {
    if let Some(session_ready_at) = state.session_ready_at {
        if state.first_local_speech_at.is_none()
            && !state.awaiting_response
            && !state.audio_playing
            && now.duration_since(session_ready_at)
                >= Duration::from_millis(REALTIME_NO_SPEECH_TIMEOUT_MS)
        {
            return Some(RealtimeExitReason::NoSpeech);
        }
    }

    if let Some(last_speech_end_at) = state.last_local_speech_end_at {
        if state.awaiting_response
            && !state.current_turn_received_server_activity
            && !state.suppress_server_audio_until_turn_end
            && now.duration_since(last_speech_end_at)
                >= Duration::from_millis(REALTIME_RESPONSE_WAIT_TIMEOUT_MS)
        {
            return Some(RealtimeExitReason::ResponseWait);
        }
    }

    if state.awaiting_response
        && state.current_turn_received_server_activity
        && !state.audio_playing
        && now.duration_since(state.last_activity)
            >= Duration::from_millis(REALTIME_RESPONSE_WAIT_TIMEOUT_MS)
    {
        return Some(RealtimeExitReason::ResponseWait);
    }

    if let Some(playback_finished_at) = state.playback_finished_at {
        if !state.awaiting_response
            && !state.audio_playing
            && now.duration_since(playback_finished_at)
                >= Duration::from_millis(REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS)
        {
            return Some(RealtimeExitReason::PostPlaybackIdle);
        }
    }
    None
}

fn handle_local_interrupt(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    now: Instant,
) -> Result<()> {
    if !state.audio_playing
        && platform.speaker_staging_samples() == 0
        && platform.speaker_buffered_samples() == 0
        && !state.awaiting_response
    {
        return Ok(());
    }
    crate::metrics::record_voice_interrupt_accepted();

    match provider {
        RealtimeProvider::OpenAiCompatible | RealtimeProvider::Qwen | RealtimeProvider::Doubao => {
            if let Err(error) = send_text_retry(
                conn,
                build_response_cancel_event().as_str(),
                REALTIME_INITIAL_SEND_RETRY_MAX,
            ) {
                log::warn!("[{}] realtime cancel send failed: {}", REALTIME_TAG, error);
            } else {
                crate::metrics::record_voice_cancel_sent();
            }
        }
    }

    state.local_turn_generation = next_turn_generation(state.local_turn_generation);
    state.server_response_generation = 0;
    state.suppress_server_audio_until_turn_end = true;
    state.awaiting_response = false;
    state.current_turn_received_server_activity = false;
    state.playback_finished_at = Some(now);
    // Staging is cleared inside clear_speaker_buffer; no separate clear needed.
    platform.clear_speaker_buffer()?;
    state.finish_audio_playback(now);
    state.last_activity = now;
    log::info!(
        "[{}] local interrupt accepted; playback aborted",
        REALTIME_TAG
    );
    Ok(())
}

fn should_hold_local_capture_for_server_response(
    _provider: RealtimeProvider,
    state: &RealtimeLoopState,
) -> bool {
    let _ = state;
    false
}

fn build_response_cancel_event() -> String {
    json!({
        "event_id": next_realtime_event_id("cancel"),
        "type": "response.cancel",
    })
    .to_string()
}

fn samples_to_ms(samples: usize, sample_rate_hz: u32) -> u128 {
    let rate = sample_rate_hz.max(1) as u128;
    (samples as u128).saturating_mul(1000) / rate
}

#[cfg(test)]
mod tests {
    use super::{
        build_realtime_headers, build_realtime_ws_url, build_session_update,
        decode_pcm16_delta_into, realtime_ws_url_needs_openai_beta,
        server_message_marks_session_ready, RealtimeProvider, RealtimeUploadEncoder,
        REALTIME_OPENAI_BETA,
    };
    use crate::config::default_disabled_audio_segment;
    use base64::Engine as _;

    fn realtime_cfg(provider: &str) -> crate::config::AudioSegment {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.provider = provider.to_string();
        cfg.realtime.api_key = "token".to_string();
        cfg.realtime.ws_url = "wss://ai-gateway.vei.volces.com/v1/realtime".to_string();
        cfg.realtime.model = "doubao-seed-realtime".to_string();
        cfg.realtime.voice = "zh_female_tianmei".to_string();
        cfg
    }

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
        cfg.realtime.voice = "Tina".to_string();
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
    fn pcm16_delta_decode_reuses_output_buffer() {
        let encoded = base64::engine::general_purpose::STANDARD.encode([1u8, 0, 255, 255]);
        let mut out = Vec::with_capacity(16);
        let original_capacity = out.capacity();

        decode_pcm16_delta_into(encoded.as_str(), &mut out).unwrap();

        assert_eq!(out, vec![1, -1]);
        assert_eq!(out.capacity(), original_capacity);
    }

    #[test]
    fn session_update_contains_event_id() {
        let cfg = default_disabled_audio_segment();
        let payload = build_session_update(RealtimeProvider::OpenAiCompatible, &cfg);
        assert!(payload.contains("\"event_id\":\"session_"));
    }

    #[test]
    fn session_updated_marks_ready() {
        assert!(server_message_marks_session_ready(
            RealtimeProvider::OpenAiCompatible,
            br#"{"type":"session.updated"}"#
        )
        .unwrap());
        assert!(!server_message_marks_session_ready(
            RealtimeProvider::OpenAiCompatible,
            br#"{"type":"session.created"}"#
        )
        .unwrap());
        assert!(!server_message_marks_session_ready(
            RealtimeProvider::OpenAiCompatible,
            br#"{"type":"response.created"}"#
        )
        .unwrap());
    }

    #[test]
    fn doubao_ws_url_and_headers_follow_provider_contract() {
        let cfg = realtime_cfg("doubao");
        let provider = RealtimeProvider::parse("doubao").unwrap();

        let url = build_realtime_ws_url(provider, &cfg).unwrap();
        let headers = build_realtime_headers(provider, &cfg, url.as_str()).unwrap();

        assert_eq!(provider.input_audio_format(), "pcm16");
        assert_eq!(provider.output_audio_format(), "pcm16");
        assert!(url.starts_with("wss://ai-gateway.vei.volces.com/v1/realtime?model="));
        assert!(url.contains("doubao-seed-realtime"));
        assert!(headers
            .iter()
            .any(|(name, value)| { *name == "Authorization" && *value == "Bearer token" }));
    }

    #[test]
    fn doubao_session_update_omits_server_turn_detection() {
        let cfg = realtime_cfg("doubao");
        let provider = RealtimeProvider::parse("doubao").unwrap();
        let payload = build_session_update(provider, &cfg);

        assert!(payload.contains("\"voice\":\"zh_female_tianmei\""));
        assert!(payload.contains("\"input_audio_format\":\"pcm16\""));
        assert!(payload.contains("\"output_audio_format\":\"pcm16\""));
        assert!(!payload.contains("\"turn_detection\""));
    }

    #[test]
    fn doubao_session_created_marks_ready() {
        assert!(server_message_marks_session_ready(
            RealtimeProvider::Doubao,
            br#"{"type":"session.created"}"#
        )
        .unwrap());
    }

    #[test]
    fn doubao_provider_is_not_baidu_alias() {
        let provider = RealtimeProvider::parse("doubao").unwrap();
        assert_ne!(provider.input_audio_format(), "raw16k");
    }
}
