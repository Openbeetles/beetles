//! 实时语音会话：唤醒后建立 WSS，会话内持续上送 PCM，并接收模型返回的语音增量。
//! Realtime voice session over WSS: stream PCM in, play audio deltas out.

use crate::Platform;
use crate::audio::capture::AudioRecordingGuard;
use crate::audio::energy::{EndpointConfig, EndpointEvent, EndpointState, normalized_rms};
use crate::channels::{
    WssCloseInfo, WssConnectProfile, WssConnection, WssEvent, connect_wss_with_headers_and_profile,
};
use crate::config::{
    AUDIO_REALTIME_PROVIDER_BAIDU, AUDIO_REALTIME_PROVIDER_OPENAI_COMPATIBLE,
    AUDIO_REALTIME_PROVIDER_QWEN, AudioSegment, audio_realtime_enabled,
};
use crate::constants::{AUDIO_CAPTURE_FRAME_SAMPLES, AUDIO_TTS_WRITE_CHUNK_SAMPLES};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::constants::{
    TLS_ADMISSION_MIN_INTERNAL_BYTES, TLS_ADMISSION_MIN_LARGEST_BLOCK_BYTES,
    TLS_ADMISSION_NO_PSRAM_MIN_BYTES,
};
use crate::error::{Error, Result};
use crate::platform::AudioDuplexCapabilities;
use base64::Engine;
use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const REALTIME_TAG: &str = "audio::realtime";
const REALTIME_OPENAI_BETA: &str = "realtime=v1";
const REALTIME_BAIDU_AUDIO_CODEC: &str = "raw16k";
const REALTIME_INITIAL_SEND_RETRY_MS: u64 = 100;
const REALTIME_INITIAL_SEND_RETRY_MAX: usize = 50;
const REALTIME_RECV_POLL_MS: u64 = 20;
const REALTIME_SESSION_READY_TIMEOUT_MS: u64 = 1_500;
const REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS: u32 = 8_000;
const REALTIME_SERVER_VAD_PREFIX_PADDING_MS: u32 = 300;
const REALTIME_NO_SPEECH_TIMEOUT_MS: u64 = 4_000;
const REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS: u64 = 2_000;
const REALTIME_RESPONSE_WAIT_TIMEOUT_MS: u64 = 8_000;
// Baidu realtime currently streams raw16k PCM over WSS rather than xiaozhi's
// Opus packet path, so the downlink is more burst-sensitive and needs a thicker
// software buffer on ESP to avoid audible underruns on normal Wi-Fi jitter.
const REALTIME_PLAYBACK_TARGET_BUFFER_MS: u32 = 480;
const REALTIME_PLAYBACK_REFILL_LOW_WATER_MS: u32 = 240;
const REALTIME_INTERRUPT_BASELINE_MS: u64 = 180;
const REALTIME_INTERRUPT_SPEECH_MIN_MS: u32 = 180;
const REALTIME_INTERRUPT_THRESHOLD_MIN: f32 = 0.18;
const REALTIME_INTERRUPT_THRESHOLD_MARGIN: f32 = 0.08;
const REALTIME_INTERRUPT_THRESHOLD_MULTIPLIER: f32 = 2.0;
const REALTIME_INTERRUPT_REFERENCE_ACTIVE_MIN: f32 = 0.06;
const REALTIME_INTERRUPT_REFERENCE_SUBTRACT_SCALE: f32 = 0.65;
const REALTIME_LOCAL_SPEECH_COMMIT_MIN_MS: u32 = 160;
const REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS: u32 = 12_000;
const REALTIME_TLS_ADMISSION_RETRY_MAX: usize = 12;
const REALTIME_TLS_ADMISSION_RETRY_MS: u64 = 150;
static REALTIME_EVENT_COUNTER: AtomicU32 = AtomicU32::new(1);
static BAIDU_LICENSE_SKIP_ACTIVATION_FOR_BOOT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
    baidu_license_activation_attempted: bool,
    baidu_license_already_used: bool,
    baidu_license_failure_message: Option<String>,
    baidu_session_usable: bool,
    turns_completed: u32,
    input_samples: usize,
    output_samples: usize,
    pending_output_pcm: Vec<i16>,
    pending_output_offset: usize,
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
            baidu_license_activation_attempted: false,
            baidu_license_already_used: false,
            baidu_license_failure_message: None,
            baidu_session_usable: false,
            turns_completed: 0,
            input_samples: 0,
            output_samples: 0,
            pending_output_pcm: Vec::with_capacity(AUDIO_TTS_WRITE_CHUNK_SAMPLES * 2),
            pending_output_offset: 0,
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
            last_activity: Instant::now(),
        }
    }

    fn pending_output_len(&self) -> usize {
        self.pending_output_pcm
            .len()
            .saturating_sub(self.pending_output_offset)
    }

    fn pending_output_is_empty(&self) -> bool {
        self.pending_output_len() == 0
    }

    fn pending_output_chunk(&self, max_samples: usize) -> &[i16] {
        let start = self.pending_output_offset;
        let end =
            (start + max_samples.min(self.pending_output_len())).min(self.pending_output_pcm.len());
        &self.pending_output_pcm[start..end]
    }

    fn consume_pending_output(&mut self, samples: usize) {
        self.pending_output_offset =
            (self.pending_output_offset + samples).min(self.pending_output_pcm.len());
        if self.pending_output_offset >= self.pending_output_pcm.len() {
            self.pending_output_pcm.clear();
            self.pending_output_offset = 0;
            return;
        }
        if self.pending_output_offset >= AUDIO_TTS_WRITE_CHUNK_SAMPLES * 4
            && self.pending_output_offset * 2 >= self.pending_output_pcm.len()
        {
            let start = self.pending_output_offset;
            let remaining = self.pending_output_pcm.len() - start;
            self.pending_output_pcm.copy_within(start.., 0);
            self.pending_output_pcm.truncate(remaining);
            self.pending_output_offset = 0;
        }
    }

    fn clear_pending_output(&mut self) {
        self.pending_output_pcm.clear();
        self.pending_output_offset = 0;
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

    fn finish_local_speech_window(&mut self, now: Instant) {
        if self.current_local_turn_committed {
            self.awaiting_response = true;
            self.current_turn_received_server_activity = false;
            self.last_local_speech_end_at = Some(now);
        }
        self.current_local_speech_ms = 0;
        self.current_local_turn_committed = false;
        self.last_activity = now;
    }

    fn force_finish_local_speech_window(&mut self, now: Instant) {
        self.finish_local_speech_window(now);
        self.last_activity = now;
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

    fn drop_server_audio_for_stale_turn(&mut self, now: Instant) {
        self.last_activity = now;
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
    if next == 0 { 1 } else { next }
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
    let _cleanup = RealtimeSessionCleanup::new(platform);
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
    let mut conn = connect_realtime_wss_with_retry(platform, ws_url.as_str(), &header_refs)?;
    let mut state = RealtimeLoopState::new(duplex_caps);
    let mut mic_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut reference_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut upload_encoder = RealtimeUploadEncoder::new();
    let frame_ms = ((AUDIO_CAPTURE_FRAME_SAMPLES as u64) * 1000
        / (audio_cfg.microphone.sample_rate.max(8_000) as u64))
        .clamp(1, 40) as u32;
    let endpoint_cfg = EndpointConfig {
        threshold: audio_cfg.vad.threshold,
        silence_duration_ms: audio_cfg.vad.silence_duration_ms,
    };
    let mut endpoint = EndpointState::new();
    let mut local_speech_active = false;

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
        if playback_capture_suspended {
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
                interrupted_this_frame = true;
            } else {
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
                    state.finish_local_speech_window(now);
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
            state.force_finish_local_speech_window(now);
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

    if let Some(message) = state.baidu_license_failure_message.take() {
        if !state.baidu_session_usable {
            return Err(Error::config(
                "realtime_voice_ws",
                format!(
                    "baidu realtime license activation failed before session became usable: {}",
                    message
                ),
            ));
        }
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
        "{base}{separator}a={}&ak={}&sk={}&ac={}&cfg={}",
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

fn redact_realtime_ws_url(provider: RealtimeProvider, url: &str) -> String {
    if provider != RealtimeProvider::Baidu {
        return crate::util::scrub_credentials(url);
    }

    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let mut redacted = String::with_capacity(url.len());
    redacted.push_str(base);
    redacted.push('?');
    for (index, pair) in query.split('&').enumerate() {
        if index > 0 {
            redacted.push('&');
        }
        let Some((key, value)) = pair.split_once('=') else {
            redacted.push_str(pair);
            continue;
        };
        redacted.push_str(key);
        redacted.push('=');
        match key {
            "a" | "ak" | "sk" => {
                let prefix = value.chars().take(4).collect::<String>();
                if prefix.is_empty() {
                    redacted.push_str("[REDACTED]");
                } else {
                    redacted.push_str(prefix.as_str());
                    redacted.push_str("...[REDACTED]");
                }
            }
            _ => redacted.push_str(value),
        }
    }
    redacted
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
            handle_json_server_message(platform, state, audio_cfg, payload)?;
            Ok(ready)
        }
    }
}

fn handle_json_server_message(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    payload: &[u8],
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| Error::config("realtime_voice_parse", e.to_string()))?;
    let event_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let now = Instant::now();

    match event_type {
        "session.created" | "session.updated" | "input_audio_buffer.committed" => {
            if event_type == "session.updated" && !state.session_ready {
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
            let pcm = decode_pcm16_delta(delta)?;
            if !pcm.is_empty() {
                queue_output_audio(
                    platform,
                    state,
                    audio_cfg.speaker.sample_rate,
                    pcm.as_slice(),
                )?;
                state.awaiting_response = true;
                state.mark_server_response_activity(now);
            }
            Ok(())
        }
        "response.output_audio.done" | "response.audio.done" => {
            flush_pending_output(platform, state, true)?;
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
            flush_pending_output(platform, state, true)?;
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

fn handle_baidu_server_message(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    payload: &[u8],
) -> Result<bool> {
    if let Some(text) = baidu_text_message(payload) {
        return handle_baidu_text_message(conn, platform, state, audio_cfg, text);
    }

    let pcm = decode_pcm16_bytes(strip_baidu_audio_prefix(payload))?;
    if !pcm.is_empty() {
        mark_baidu_session_usable(state);
        if state.suppress_server_audio_until_turn_end {
            state.drop_server_audio_for_stale_turn(Instant::now());
            return Ok(false);
        }
        if !state.has_committed_local_turn() || !state.should_accept_server_audio() {
            state.drop_server_audio_for_stale_turn(Instant::now());
            return Ok(false);
        }
        queue_output_audio(
            platform,
            state,
            audio_cfg.speaker.sample_rate,
            pcm.as_slice(),
        )?;
        state.awaiting_response = true;
        state.mark_server_response_activity(Instant::now());
    }
    Ok(false)
}

fn handle_baidu_text_message(
    conn: &mut dyn WssConnection,
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    text: &str,
) -> Result<bool> {
    let message = text.trim();
    let now = Instant::now();
    if message.is_empty() {
        return Ok(false);
    }

    if message.starts_with("[E]:[LIC]:[MUST]") {
        if BAIDU_LICENSE_SKIP_ACTIVATION_FOR_BOOT.load(Ordering::Relaxed) {
            log::info!(
                "[{}] baidu license activation requested again, but this boot already proved the session is usable after LIC; skipping redundant activation",
                REALTIME_TAG
            );
            state.last_activity = now;
            return Ok(false);
        }
        if state.baidu_license_activation_attempted || state.baidu_license_already_used {
            log::warn!(
                "[{}] baidu license activation requested again in the same session; ignoring duplicate request device_id={} user_id={}",
                REALTIME_TAG,
                redact_identity(audio_cfg.realtime.device_id.as_str()),
                redact_identity(audio_cfg.realtime.user_id.as_str()),
            );
            state.last_activity = now;
            return Ok(false);
        }
        state.baidu_license_activation_attempted = true;
        log::info!(
            "[{}] baidu license activation requested app_id={} device_id={} user_id={}",
            REALTIME_TAG,
            redact_identity(audio_cfg.realtime.app_id.as_str()),
            redact_identity(audio_cfg.realtime.device_id.as_str()),
            redact_identity(audio_cfg.realtime.user_id.as_str()),
        );
        send_text_retry(
            conn,
            build_baidu_license_activation(audio_cfg)?.as_str(),
            REALTIME_INITIAL_SEND_RETRY_MAX,
        )?;
        state.last_activity = now;
        return Ok(false);
    }
    if message.starts_with("[E]:[LIC]:[RES]:[FAILED]") {
        state.baidu_license_failure_message = Some(message.to_string());
        log::warn!(
            "[{}] baidu license activation returned failure; deferring final classification until session usability is known: {}",
            REALTIME_TAG,
            message
        );
        state.last_activity = now;
        return Ok(false);
    }
    if message.starts_with("[E]:[MEDIA]:[READY]:1") {
        state.mark_session_ready(now);
        return Ok(true);
    }
    if message.starts_with("[E]:[TTS_BEGIN_SPEAKING]")
        || message.starts_with("[E]:[VOICE_COMING]")
        || message.starts_with("[A]:")
        || message.starts_with("[Q]:")
    {
        mark_baidu_session_usable(state);
        if state.suppress_server_audio_until_turn_end {
            state.last_activity = now;
            return Ok(false);
        }
        if !state.has_committed_local_turn() {
            state.drop_server_audio_for_stale_turn(now);
            return Ok(false);
        }
        state.mark_server_response_created(now);
        return Ok(false);
    }
    if message.starts_with("[E]:[TTS_END_SPEAKING]") {
        if state.suppress_server_audio_until_turn_end {
            state.suppress_server_audio_until_turn_end = false;
            state.last_activity = now;
            return Ok(false);
        }
        let counted = state.awaiting_response || state.audio_playing;
        flush_pending_output(platform, state, true)?;
        if counted {
            state.awaiting_response = false;
            state.turns_completed = state.turns_completed.saturating_add(1);
        }
        if !state.audio_playing {
            state.playback_finished_at = Some(now);
        }
        state.last_activity = now;
        return Ok(false);
    }
    if message.starts_with("[E]:") && message.contains("[ERROR]") {
        return Err(Error::config(
            "realtime_voice_ws",
            format!("baidu realtime error: {}", message),
        ));
    }

    state.last_activity = now;
    Ok(false)
}

fn mark_baidu_session_usable(state: &mut RealtimeLoopState) {
    state.baidu_session_usable = true;
    if state.baidu_license_failure_message.take().is_some() {
        state.baidu_license_already_used = true;
        BAIDU_LICENSE_SKIP_ACTIVATION_FOR_BOOT.store(true, Ordering::Relaxed);
        log::info!(
            "[{}] baidu session remained usable after LIC failure; suppressing future activation attempts for this boot",
            REALTIME_TAG
        );
    }
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
    let mut payload = serde_json::Map::new();
    payload.insert(
        "devId".to_string(),
        serde_json::Value::String(sn.to_string()),
    );
    payload.insert(
        "licKey".to_string(),
        serde_json::Value::String(key.to_string()),
    );
    payload.insert(
        "uId".to_string(),
        serde_json::Value::String(uid.to_string()),
    );
    Ok(format!(
        "[E]:[LIC]:[ACTIVE]:{}",
        serde_json::Value::Object(payload)
    ))
}

fn redact_identity(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }
    let total = trimmed.chars().count();
    if total <= 8 {
        return format!(
            "{}...[REDACTED]",
            trimmed.chars().take(2).collect::<String>()
        );
    }
    let head = trimmed.chars().take(4).collect::<String>();
    let tail = trimmed
        .chars()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{head}...[REDACTED]...{tail}")
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

fn playback_target_buffer_samples(sample_rate_hz: u32) -> usize {
    ((sample_rate_hz.max(1) as usize) * (REALTIME_PLAYBACK_TARGET_BUFFER_MS as usize) / 1000)
        .max(AUDIO_TTS_WRITE_CHUNK_SAMPLES)
}

fn playback_refill_low_water_samples(sample_rate_hz: u32) -> usize {
    ((sample_rate_hz.max(1) as usize) * (REALTIME_PLAYBACK_REFILL_LOW_WATER_MS as usize) / 1000)
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
    state.pending_output_pcm.extend_from_slice(pcm);
    state.output_samples = state.output_samples.saturating_add(pcm.len());

    let target_buffer_samples = playback_target_buffer_samples(sample_rate_hz);
    let refill_low_water_samples = playback_refill_low_water_samples(sample_rate_hz);
    let speaker_depth_samples = platform.speaker_buffered_samples();
    let combined_buffered_samples =
        speaker_depth_samples.saturating_add(state.pending_output_len());

    if !state.audio_playing && combined_buffered_samples < target_buffer_samples {
        return Ok(());
    }
    let mut just_started = false;
    if !state.audio_playing && !state.pending_output_is_empty() {
        state.start_audio_playback(Instant::now());
        just_started = true;
    }
    flush_output_audio(
        platform,
        state,
        target_buffer_samples,
        refill_low_water_samples,
        false,
        just_started,
    )
}

fn flush_output_audio(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    target_buffer_samples: usize,
    refill_low_water_samples: usize,
    force_flush_all: bool,
    just_started: bool,
) -> Result<()> {
    while !state.pending_output_is_empty() {
        let speaker_depth_samples = platform.speaker_buffered_samples();
        if !force_flush_all {
            let refill_needed = just_started || speaker_depth_samples <= refill_low_water_samples;
            if !refill_needed {
                break;
            }
            if speaker_depth_samples >= target_buffer_samples {
                break;
            }
        }

        let chunk_len = AUDIO_TTS_WRITE_CHUNK_SAMPLES.min(state.pending_output_len());
        let chunk = state.pending_output_chunk(chunk_len);
        let written = platform.try_write_speaker_pcm_i16(chunk)?;
        if written == 0 {
            break;
        }
        state.consume_pending_output(written.min(chunk_len));
    }
    Ok(())
}

fn flush_pending_output(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    force_flush_all: bool,
) -> Result<()> {
    if state.pending_output_is_empty() {
        return Ok(());
    }
    if !state.audio_playing {
        state.start_audio_playback(Instant::now());
    }
    flush_output_audio(
        platform,
        state,
        playback_target_buffer_samples(1),
        playback_refill_low_water_samples(1),
        force_flush_all,
        false,
    )
}

fn update_playback_state(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    sample_rate_hz: u32,
    now: Instant,
) -> Result<()> {
    if !state.pending_output_is_empty() {
        flush_output_audio(
            platform,
            state,
            playback_target_buffer_samples(sample_rate_hz),
            playback_refill_low_water_samples(sample_rate_hz),
            false,
            false,
        )?;
    }
    if state.audio_playing
        && state.pending_output_is_empty()
        && platform.speaker_buffered_samples() == 0
    {
        state.finish_audio_playback(now);
    }
    Ok(())
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
        .max(REALTIME_INTERRUPT_THRESHOLD_MIN)
        .min(0.95);
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
    if !state.audio_playing && state.pending_output_is_empty() && !state.awaiting_response {
        return Ok(());
    }
    crate::metrics::record_voice_interrupt_accepted();

    match provider {
        RealtimeProvider::OpenAiCompatible | RealtimeProvider::Qwen => {
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
        RealtimeProvider::Baidu => {}
    }

    state.local_turn_generation = next_turn_generation(state.local_turn_generation);
    state.server_response_generation = 0;
    state.suppress_server_audio_until_turn_end = true;
    state.awaiting_response = false;
    state.current_turn_received_server_activity = false;
    state.playback_finished_at = Some(now);
    state.clear_pending_output();
    platform.clear_speaker_buffer()?;
    state.finish_audio_playback(now);
    state.last_activity = now;
    log::info!(
        "[{}] local interrupt accepted; playback aborted",
        REALTIME_TAG
    );
    Ok(())
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
        REALTIME_OPENAI_BETA, RealtimeProvider, RealtimeUploadEncoder,
        build_baidu_license_activation, build_realtime_headers, build_realtime_ws_url,
        build_session_update, realtime_ws_url_needs_openai_beta, redact_realtime_ws_url,
        server_message_marks_session_ready, strip_baidu_audio_prefix,
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
        assert!(
            headers
                .iter()
                .any(|(name, value)| *name == "OpenAI-Beta" && *value == REALTIME_OPENAI_BETA)
        );
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
        assert!(url.contains("a=app-1"));
        assert!(url.contains("ak=ak-1"));
        assert!(url.contains("sk=sk-1"));
        assert!(url.contains("ac=raw16k"));
        assert!(url.contains("cfg="));
    }

    #[test]
    fn baidu_license_activation_falls_back_to_device_id_when_user_id_is_empty() {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.license_key = "lic-key".to_string();
        cfg.realtime.device_id = "dev-1".to_string();

        let payload = build_baidu_license_activation(&cfg).unwrap();
        assert!(payload.contains("\"devId\":\"dev-1\""));
        assert!(payload.contains("\"licKey\":\"lic-key\""));
        assert!(payload.contains("\"uId\":\"dev-1\""));
    }

    #[test]
    fn baidu_license_activation_uses_explicit_user_id_only() {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.license_key = "lic-key".to_string();
        cfg.realtime.device_id = "dev-1".to_string();
        cfg.realtime.user_id = "user-1".to_string();

        let payload = build_baidu_license_activation(&cfg).unwrap();
        assert!(payload.contains("\"devId\":\"dev-1\""));
        assert!(payload.contains("\"licKey\":\"lic-key\""));
        assert!(payload.contains("\"uId\":\"user-1\""));
    }

    #[test]
    fn redact_baidu_ws_url_masks_credentials_but_keeps_shape() {
        let url = "wss://rtc-aiotgw.exp.bcelive.com/v1/realtime?a=app-1&ak=abcdefghi&sk=xyz987654&ac=raw16k&cfg=%7B%7D";
        let redacted = redact_realtime_ws_url(RealtimeProvider::Baidu, url);
        assert!(redacted.contains("a=app-...[REDACTED]"));
        assert!(redacted.contains("ak=abcd...[REDACTED]"));
        assert!(redacted.contains("sk=xyz9...[REDACTED]"));
        assert!(redacted.contains("ac=raw16k"));
        assert!(redacted.contains("cfg=%7B%7D"));
    }

    #[test]
    fn strip_baidu_prefixed_audio_payload() {
        let payload = b"[A]:[PCM]:[RAW]:0:1:2:3:4:5:\x01\x00\x02\x00";
        assert_eq!(strip_baidu_audio_prefix(payload), b"\x01\x00\x02\x00");
    }
}
