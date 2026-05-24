//! 实时语音会话：唤醒后建立 WSS，会话内持续上送 PCM，并接收模型返回的语音增量。
//! Realtime voice session over WSS: stream PCM in, play audio deltas out.

use crate::audio::capture::AudioRecordingGuard;
use crate::audio::endpoint_profile::VoiceEndpointProfile;
use crate::audio::energy::{normalized_rms, EndpointConfig, EndpointEvent, EndpointState};
use crate::audio::input_profile::AudioInputHardwareProfile;
use crate::audio::realtime_provider::RealtimeProvider;
use crate::audio::wake_handoff::WakeAudioHandoff;
use crate::channels::{WssCloseInfo, WssConnection, WssEvent};
use crate::config::AudioSegment;
use crate::config::{audio_realtime_enabled, AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE};
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
const REALTIME_SERVER_VAD_POST_RESPONSE_IDLE_TIMEOUT_MS: u64 = 20_000;
const REALTIME_FOREGROUND_KEEPALIVE_MS: u64 = 5_000;
// Realtime downlink is raw PCM over WSS, so ESP keeps a modest software buffer
// to absorb normal Wi-Fi jitter before releasing audio to the speaker.
const REALTIME_PLAYBACK_TARGET_BUFFER_MS: u32 = 800;
const REALTIME_OUTPUT_STAGING_WAIT_MS: u64 = 30_000;
const REALTIME_OUTPUT_STAGING_RETRY_MS: u64 = 10;
const REALTIME_TRANSPORT_EXIT_DRAIN_MS: u64 = 20_000;
const REALTIME_INTERRUPT_BASELINE_MS: u64 = 180;
const REALTIME_INTERRUPT_SPEECH_MIN_MS: u32 = 180;
const REALTIME_INTERRUPT_THRESHOLD_MIN: f32 = 0.18;
const REALTIME_INTERRUPT_THRESHOLD_MARGIN: f32 = 0.08;
const REALTIME_INTERRUPT_THRESHOLD_MULTIPLIER: f32 = 2.0;
const REALTIME_INTERRUPT_LOW_SNR_SPEECH_MIN_MS: u32 = 120;
const REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MIN: f32 = 0.006;
const REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MARGIN: f32 = 0.003;
const REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MULTIPLIER: f32 = 0.85;
const REALTIME_INTERRUPT_REFERENCE_ACTIVE_MIN: f32 = 0.06;
const REALTIME_INTERRUPT_REFERENCE_SUBTRACT_SCALE: f32 = 0.65;
const REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS: u32 = 12_000;
static REALTIME_EVENT_COUNTER: AtomicU32 = AtomicU32::new(1);

pub struct RealtimeSessionResult {
    pub turns_completed: u32,
    pub input_audio_ms: u128,
    pub output_audio_ms: u128,
    pub session_ms: u128,
    pub server_speech_started: bool,
    pub server_speech_stopped: bool,
    pub response_created: bool,
    pub exit_reason: RealtimeSessionExitReason,
    pub interrupted_active_turn: bool,
    pub partial_output_pending_at_exit: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeSessionExitReason {
    NoLocalSpeechAfterSessionReady,
    ResponseWait,
    PostPlaybackIdle,
    TransportDisconnected,
    PeerClosed,
}

impl RealtimeSessionExitReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoLocalSpeechAfterSessionReady => "no_local_speech_after_session_ready",
            Self::ResponseWait => "response_wait",
            Self::PostPlaybackIdle => "post_playback_idle",
            Self::TransportDisconnected => "transport_disconnected",
            Self::PeerClosed => "peer_closed",
        }
    }

    pub const fn is_normal_dialogue_exit(self) -> bool {
        matches!(self, Self::PostPlaybackIdle)
    }
}

pub(crate) struct ConnectedRealtimeSession {
    conn: Box<dyn WssConnection>,
    provider: RealtimeProvider,
    endpoint_profile: VoiceEndpointProfile,
    duplex_caps: AudioDuplexCapabilities,
    session_start: Instant,
    session_ready_at: Instant,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoSpeechExitReason {
    NoLocalSpeechAfterSessionReady,
    ResponseWait,
    PostPlaybackIdle,
}

impl NoSpeechExitReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::NoLocalSpeechAfterSessionReady => "no_local_speech_after_session_ready",
            Self::ResponseWait => "response_wait",
            Self::PostPlaybackIdle => "post_playback_idle",
        }
    }
}

impl From<NoSpeechExitReason> for RealtimeSessionExitReason {
    fn from(reason: NoSpeechExitReason) -> Self {
        match reason {
            NoSpeechExitReason::NoLocalSpeechAfterSessionReady => {
                Self::NoLocalSpeechAfterSessionReady
            }
            NoSpeechExitReason::ResponseWait => Self::ResponseWait,
            NoSpeechExitReason::PostPlaybackIdle => Self::PostPlaybackIdle,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum RealtimeDrainOutcome {
    Idle,
    Events,
    TransportDisconnected,
    PeerClosed(String),
}

#[derive(Clone, Debug)]
struct RealtimeOutputTurn {
    response_id: Option<String>,
    item_id: Option<String>,
    generation: u32,
    audio_started: bool,
    accepted_samples: usize,
    played_or_queued_samples: usize,
    not_before_drained_at: Option<Instant>,
    audio_done_seen: bool,
    response_done_seen: bool,
    cancelled: bool,
    ended: bool,
}

impl RealtimeOutputTurn {
    fn new(generation: u32, response_id: Option<String>, item_id: Option<String>) -> Self {
        Self {
            response_id,
            item_id,
            generation,
            audio_started: false,
            accepted_samples: 0,
            played_or_queued_samples: 0,
            not_before_drained_at: None,
            audio_done_seen: false,
            response_done_seen: false,
            cancelled: false,
            ended: false,
        }
    }

    fn record_audio_write(&mut self, now: Instant, accepted_samples: usize, sample_rate_hz: u32) {
        if accepted_samples == 0 {
            return;
        }
        self.audio_started = true;
        self.accepted_samples = self.accepted_samples.saturating_add(accepted_samples);
        self.played_or_queued_samples = self
            .played_or_queued_samples
            .saturating_add(accepted_samples);
        let pending_base = self
            .not_before_drained_at
            .filter(|drain_at| *drain_at > now)
            .unwrap_or(now);
        self.not_before_drained_at =
            Some(pending_base + samples_to_duration(accepted_samples, sample_rate_hz));
    }

    fn mark_audio_done(&mut self) {
        self.audio_done_seen = true;
    }

    fn mark_response_done(&mut self) {
        self.response_done_seen = true;
    }

    fn mark_cancelled(&mut self) {
        self.cancelled = true;
        self.ended = true;
    }

    fn audio_duration_pending(&self, now: Instant) -> bool {
        self.not_before_drained_at
            .map(|drain_at| drain_at > now)
            .unwrap_or(false)
    }

    fn mark_drained(&mut self, now: Instant) {
        if (self.response_done_seen || self.cancelled) && !self.audio_duration_pending(now) {
            self.ended = true;
        }
    }

    fn accepts_event(&self, response_id: Option<&str>, item_id: Option<&str>) -> bool {
        if self.cancelled || self.ended {
            return false;
        }
        if let (Some(expected), Some(actual)) = (self.response_id.as_deref(), response_id) {
            if expected != actual {
                return false;
            }
        }
        if let (Some(expected), Some(actual)) = (self.item_id.as_deref(), item_id) {
            if expected != actual {
                return false;
            }
        }
        true
    }
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
    server_speech_started: bool,
    server_speech_stopped: bool,
    response_created: bool,
    interrupt_baseline_deadline: Option<Instant>,
    interrupt_baseline_peak: f32,
    interrupt_speech_ms: u32,
    current_local_speech_ms: u32,
    current_local_turn_committed: bool,
    local_turn_generation: u32,
    server_response_generation: u32,
    output_generation: u32,
    active_output_turn: Option<RealtimeOutputTurn>,
    stale_server_audio_drop_total: u32,
    output_pcm_decode_buf: Vec<i16>,
    downlink_delta_chunks: u32,
    downlink_delta_samples: usize,
    downlink_staging_written_samples: usize,
    downlink_direct_written_samples: usize,
    downlink_dropped_samples: usize,
    downlink_peak_staging_samples: usize,
    downlink_peak_speaker_samples: usize,
    downlink_started_at: Option<Instant>,
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
            server_speech_started: false,
            server_speech_stopped: false,
            response_created: false,
            interrupt_baseline_deadline: None,
            interrupt_baseline_peak: 0.0,
            interrupt_speech_ms: 0,
            current_local_speech_ms: 0,
            current_local_turn_committed: false,
            local_turn_generation: 0,
            server_response_generation: 0,
            output_generation: 0,
            active_output_turn: None,
            stale_server_audio_drop_total: 0,
            output_pcm_decode_buf: Vec::new(),
            downlink_delta_chunks: 0,
            downlink_delta_samples: 0,
            downlink_staging_written_samples: 0,
            downlink_direct_written_samples: 0,
            downlink_dropped_samples: 0,
            downlink_peak_staging_samples: 0,
            downlink_peak_speaker_samples: 0,
            downlink_started_at: None,
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

    fn extend_local_speech_window(&mut self, now: Instant, frame_ms: u32, min_active_ms: u32) {
        self.current_local_speech_ms = self.current_local_speech_ms.saturating_add(frame_ms);
        if !self.current_local_turn_committed && self.current_local_speech_ms >= min_active_ms {
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

    fn begin_local_activity_window(&mut self, now: Instant, frame_ms: u32) {
        self.current_local_speech_ms = frame_ms;
        self.current_local_turn_committed = false;
        self.last_activity = now;
    }

    fn extend_local_activity_window(&mut self, now: Instant, frame_ms: u32) {
        self.current_local_speech_ms = self.current_local_speech_ms.saturating_add(frame_ms);
        self.last_activity = now;
    }

    fn finish_local_activity_window(&mut self, now: Instant) {
        self.current_local_speech_ms = 0;
        self.current_local_turn_committed = false;
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

    fn mark_server_response_created_with_ids(
        &mut self,
        now: Instant,
        response_id: Option<String>,
        item_id: Option<String>,
    ) {
        self.awaiting_response = true;
        self.server_response_generation = self.local_turn_generation;
        self.output_generation = next_turn_generation(self.output_generation);
        self.active_output_turn = Some(RealtimeOutputTurn::new(
            self.output_generation,
            response_id,
            item_id,
        ));
        self.response_created = true;
        self.mark_server_response_activity(now);
    }

    fn begin_server_vad_turn(&mut self, now: Instant) {
        self.awaiting_response = false;
        self.current_turn_received_server_activity = false;
        self.local_turn_generation = next_turn_generation(self.local_turn_generation);
        self.server_response_generation = 0;
        self.server_speech_started = true;
        if self.first_local_speech_at.is_none() {
            self.first_local_speech_at = Some(now);
        }
        self.last_activity = now;
    }

    fn mark_server_vad_speech_stopped(&mut self, now: Instant) {
        self.awaiting_response = true;
        self.last_local_speech_end_at = Some(now);
        self.server_speech_stopped = true;
        self.last_activity = now;
    }

    fn mark_local_turn_submitted(&mut self, now: Instant) {
        self.awaiting_response = true;
        self.current_turn_received_server_activity = false;
        self.last_local_speech_end_at = Some(now);
        self.last_activity = now;
    }

    fn drop_server_audio_for_stale_turn(&mut self, event_type: &str, reason: &str, now: Instant) {
        self.stale_server_audio_drop_total = self.stale_server_audio_drop_total.saturating_add(1);
        self.last_activity = now;
        log::warn!(
            "[{}] stale realtime server audio dropped event={} reason={} local_generation={} server_generation={} output_generation={} active_output_generation={} stale_server_audio_drop_total={}",
            REALTIME_TAG,
            event_type,
            reason,
            self.local_turn_generation,
            self.server_response_generation,
            self.output_generation,
            self.active_output_turn
                .as_ref()
                .map(|turn| turn.generation)
                .unwrap_or(0),
            self.stale_server_audio_drop_total
        );
    }

    fn should_accept_server_audio(&self) -> bool {
        self.server_response_generation == self.local_turn_generation
            && self
                .active_output_turn
                .as_ref()
                .map(|turn| {
                    !turn.cancelled && !turn.ended && turn.generation == self.output_generation
                })
                .unwrap_or(false)
    }

    fn has_committed_local_turn(&self) -> bool {
        self.local_turn_generation != 0 || self.first_local_speech_at.is_some()
    }

    fn should_accept_server_audio_event(&self, value: &serde_json::Value) -> bool {
        let Some(turn) = self.active_output_turn.as_ref() else {
            return false;
        };
        self.should_accept_server_audio()
            && turn.accepts_event(response_event_id(value), item_event_id(value))
    }

    fn can_begin_output_turn(&self, platform: &dyn Platform, now: Instant) -> bool {
        let Some(turn) = self.active_output_turn.as_ref() else {
            return true;
        };
        turn.ended
            || turn.cancelled
            || (turn.response_done_seen
                && !self.audio_playing
                && platform.speaker_staging_samples() == 0
                && platform.speaker_buffered_samples() == 0
                && !turn.audio_duration_pending(now))
    }

    fn active_output_duration_pending(&self, now: Instant) -> bool {
        self.active_output_turn
            .as_ref()
            .map(|turn| !turn.cancelled && turn.audio_duration_pending(now))
            .unwrap_or(false)
    }

    fn has_pending_output_turn(&self, now: Instant) -> bool {
        self.active_output_turn
            .as_ref()
            .map(|turn| !turn.cancelled && (!turn.ended || turn.audio_duration_pending(now)))
            .unwrap_or(false)
    }

    fn has_pending_output_audio(&self, platform: &dyn Platform, now: Instant) -> bool {
        self.audio_playing
            || platform.speaker_staging_samples() != 0
            || platform.speaker_buffered_samples() != 0
            || self.active_output_duration_pending(now)
    }

    fn transport_exit_interrupts_active_turn(&self, platform: &dyn Platform, now: Instant) -> bool {
        self.awaiting_response
            || self.suppress_server_audio_until_turn_end
            || self.has_pending_output_turn(now)
            || self.has_pending_output_audio(platform, now)
    }

    fn mark_active_output_audio_done(&mut self, value: &serde_json::Value) -> bool {
        if !self.should_accept_server_audio_event(value) {
            return false;
        }
        if let Some(turn) = self.active_output_turn.as_mut() {
            turn.mark_audio_done();
        }
        true
    }

    fn mark_active_output_response_done(&mut self, value: &serde_json::Value) -> bool {
        if !self.should_accept_server_audio_event(value) {
            return false;
        }
        if let Some(turn) = self.active_output_turn.as_mut() {
            turn.mark_response_done();
        }
        true
    }

    fn cancel_active_output_turn(&mut self) {
        if let Some(turn) = self.active_output_turn.as_mut() {
            turn.mark_cancelled();
        }
    }

    fn recover_response_wait(&mut self, now: Instant) {
        self.cancel_active_output_turn();
        self.awaiting_response = false;
        self.current_turn_received_server_activity = false;
        self.suppress_server_audio_until_turn_end = false;
        self.current_local_speech_ms = 0;
        self.current_local_turn_committed = false;
        self.last_local_speech_end_at = None;
        self.server_response_generation = 0;
        if !self.audio_playing {
            self.playback_finished_at = Some(now);
        }
        self.last_activity = now;
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
        self.mark_active_output_drained(now);
    }

    fn mark_active_output_drained(&mut self, now: Instant) {
        if let Some(turn) = self.active_output_turn.as_mut() {
            turn.mark_drained(now);
        }
    }

    fn record_downlink_write(
        &mut self,
        now: Instant,
        write: OutputQueueWrite,
        sample_rate_hz: u32,
    ) {
        if self.downlink_started_at.is_none() {
            self.downlink_started_at = Some(now);
        }
        if let Some(turn) = self.active_output_turn.as_mut() {
            turn.record_audio_write(now, write.accepted_samples(), sample_rate_hz);
        }
        self.downlink_delta_chunks = self.downlink_delta_chunks.saturating_add(1);
        self.downlink_delta_samples = self
            .downlink_delta_samples
            .saturating_add(write.input_samples);
        self.downlink_staging_written_samples = self
            .downlink_staging_written_samples
            .saturating_add(write.staging_written);
        self.downlink_direct_written_samples = self
            .downlink_direct_written_samples
            .saturating_add(write.direct_written);
        self.downlink_dropped_samples = self.downlink_dropped_samples.saturating_add(write.dropped);
        self.downlink_peak_staging_samples = self
            .downlink_peak_staging_samples
            .max(write.staging_buffered_after);
        self.downlink_peak_speaker_samples = self
            .downlink_peak_speaker_samples
            .max(write.speaker_buffered_after);
    }

    fn log_downlink_summary(
        &mut self,
        event: &str,
        sample_rate_hz: u32,
        now: Instant,
        force_empty_summary: bool,
    ) {
        if self.downlink_delta_chunks == 0 && !force_empty_summary {
            return;
        }
        let accepted = OutputQueueWrite {
            staging_written: self.downlink_staging_written_samples,
            direct_written: self.downlink_direct_written_samples,
            ..OutputQueueWrite::default()
        }
        .accepted_samples();
        let elapsed_ms = self
            .downlink_started_at
            .map(|started| now.duration_since(started).as_millis())
            .unwrap_or(0);
        let empty_output = self.downlink_delta_chunks == 0;
        let (
            output_generation,
            response_id,
            item_id,
            audio_done_seen,
            response_done_seen,
            cancelled,
            ended,
            turn_accepted_samples,
            played_or_queued_samples,
        ) = self
            .active_output_turn
            .as_ref()
            .map(|turn| {
                (
                    turn.generation,
                    turn.response_id.as_deref().unwrap_or("-").to_string(),
                    turn.item_id.as_deref().unwrap_or("-").to_string(),
                    turn.audio_done_seen,
                    turn.response_done_seen,
                    turn.cancelled,
                    turn.ended,
                    turn.accepted_samples,
                    turn.played_or_queued_samples,
                )
            })
            .unwrap_or_else(|| {
                (
                    0,
                    "-".to_string(),
                    "-".to_string(),
                    false,
                    false,
                    false,
                    false,
                    0,
                    0,
                )
            });
        log::info!(
            "[{}] realtime audio downlink summary event={} chunks={} samples={} accepted={} dropped={} staging_written={} direct_written={} peak_staging={} peak_speaker={} audio_ms={} elapsed_ms={} output_generation={} response_id={} item_id={} empty_output={} audio_done_seen={} response_done_seen={} cancelled={} ended={} turn_accepted_samples={} played_or_queued_samples={} stale_server_audio_drop_total={}",
            REALTIME_TAG,
            event,
            self.downlink_delta_chunks,
            self.downlink_delta_samples,
            accepted,
            self.downlink_dropped_samples,
            self.downlink_staging_written_samples,
            self.downlink_direct_written_samples,
            self.downlink_peak_staging_samples,
            self.downlink_peak_speaker_samples,
            samples_to_ms(accepted, sample_rate_hz),
            elapsed_ms,
            output_generation,
            response_id,
            item_id,
            empty_output,
            audio_done_seen,
            response_done_seen,
            cancelled,
            ended,
            turn_accepted_samples,
            played_or_queued_samples,
            self.stale_server_audio_drop_total
        );
        self.reset_downlink_window();
    }

    fn reset_downlink_window(&mut self) {
        self.downlink_delta_chunks = 0;
        self.downlink_delta_samples = 0;
        self.downlink_staging_written_samples = 0;
        self.downlink_direct_written_samples = 0;
        self.downlink_dropped_samples = 0;
        self.downlink_peak_staging_samples = 0;
        self.downlink_peak_speaker_samples = 0;
        self.downlink_started_at = None;
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

struct RealtimeUploadEncoder {
    input_sample_rate_hz: u32,
    pcm_bytes: Vec<u8>,
    qwen_pcm16: Vec<i16>,
    audio_b64: String,
    event_json: String,
    qwen_resample_tail: [i16; 2],
    qwen_resample_tail_len: usize,
}

impl RealtimeUploadEncoder {
    fn new(input_sample_rate_hz: u32) -> Self {
        let pcm_capacity = AUDIO_CAPTURE_FRAME_SAMPLES * 2;
        let b64_capacity = pcm_capacity.div_ceil(3) * 4;
        Self {
            input_sample_rate_hz: input_sample_rate_hz.max(8_000),
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
                if self.input_sample_rate_hz == AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE {
                    self.pcm_bytes.reserve(pcm.len().saturating_mul(2));
                    for sample in pcm {
                        self.pcm_bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                } else {
                    self.downsample_24k_to_16k(pcm);
                    self.pcm_bytes
                        .reserve(self.qwen_pcm16.len().saturating_mul(2));
                    for sample in &self.qwen_pcm16 {
                        self.pcm_bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                }
            }
        }

        self.finish_append_event()
    }

    fn build_append_event_from_pcm_le_bytes(
        &mut self,
        provider: RealtimeProvider,
        pcm_le_bytes: &[u8],
    ) -> &str {
        self.pcm_bytes.clear();
        match provider {
            RealtimeProvider::OpenAiCompatible | RealtimeProvider::Doubao => {
                self.pcm_bytes.extend_from_slice(pcm_le_bytes);
            }
            RealtimeProvider::Qwen => {
                if self.input_sample_rate_hz == AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE {
                    self.pcm_bytes.extend_from_slice(pcm_le_bytes);
                } else {
                    self.downsample_24k_le_bytes_to_16k(pcm_le_bytes);
                    self.pcm_bytes
                        .reserve(self.qwen_pcm16.len().saturating_mul(2));
                    for sample in &self.qwen_pcm16 {
                        self.pcm_bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                }
            }
        }

        self.finish_append_event()
    }

    fn finish_append_event(&mut self) -> &str {
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

    fn downsample_24k_le_bytes_to_16k(&mut self, pcm_le_bytes: &[u8]) -> &[i16] {
        self.qwen_pcm16.clear();
        let mut triple = [0i16; 3];
        let mut triple_len = self.qwen_resample_tail_len;
        if triple_len > 0 {
            triple[..triple_len].copy_from_slice(&self.qwen_resample_tail[..triple_len]);
        }

        for chunk in pcm_le_bytes.chunks_exact(2) {
            triple[triple_len] = i16::from_le_bytes([chunk[0], chunk[1]]);
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
    let input_profile = AudioInputHardwareProfile::from_audio_config(audio_cfg);
    let endpoint_profile =
        VoiceEndpointProfile::from_input_profile(input_profile, audio_cfg, provider, duplex_caps);
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
        build_session_update(provider, audio_cfg, &endpoint_profile).as_str(),
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
        endpoint_profile,
        duplex_caps,
        session_start,
        session_ready_at,
    })
}

pub(crate) fn run_connected_realtime_session(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    connected: ConnectedRealtimeSession,
    handoff: WakeAudioHandoff,
    mut maintain_foreground: impl FnMut(),
) -> Result<RealtimeSessionResult> {
    let _recording_guard = AudioRecordingGuard::new();
    let _cleanup = RealtimeSessionCleanup::new(platform);
    let ConnectedRealtimeSession {
        mut conn,
        provider,
        endpoint_profile,
        duplex_caps,
        session_start,
        session_ready_at,
    } = connected;
    let mut state = RealtimeLoopState::new(duplex_caps);
    state.mark_session_ready(session_ready_at);
    let mut mic_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut reference_frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut upload_encoder = RealtimeUploadEncoder::new(audio_cfg.microphone.sample_rate);
    let frame_ms = ((AUDIO_CAPTURE_FRAME_SAMPLES as u64) * 1000
        / (audio_cfg.microphone.sample_rate.max(8_000) as u64))
        .clamp(1, 40) as u32;
    let endpoint_cfg = EndpointConfig {
        threshold: endpoint_profile.local_enter_threshold,
        silence_duration_ms: endpoint_profile.silence_duration_ms,
    };
    log::info!(
        "[{}] realtime endpoint profile owner={:?} codec={:?} level={:?} local_enter={:.3} server_vad={:.3}",
        REALTIME_TAG,
        endpoint_profile.owner,
        endpoint_profile.hardware.codec,
        endpoint_profile.hardware.level_model,
        endpoint_profile.local_enter_threshold,
        endpoint_profile.server_vad_threshold
    );
    let handoff_samples = append_handoff_audio(
        conn.as_mut(),
        &mut upload_encoder,
        provider,
        handoff,
        audio_cfg.microphone.sample_rate,
    )?;
    state.input_samples = state.input_samples.saturating_add(handoff_samples);
    let mut endpoint = EndpointState::new();
    let mut local_speech_active = false;
    let mut foreground_keepalive_at = Instant::now();
    maintain_foreground();
    let exit_reason;
    let interrupted_active_turn;
    let partial_output_pending_at_exit;

    loop {
        crate::platform::task_wdt::feed_current_task();
        let drain_outcome = drain_server_events(
            conn.as_mut(),
            platform,
            &mut state,
            provider,
            audio_cfg,
            Duration::from_millis(REALTIME_RECV_POLL_MS),
        )?;
        let now = Instant::now();
        let drain_idle = matches!(drain_outcome, RealtimeDrainOutcome::Idle);
        match drain_outcome {
            RealtimeDrainOutcome::TransportDisconnected => {
                let pending_output = state.has_pending_output_audio(platform, now);
                let interrupted = state.transport_exit_interrupts_active_turn(platform, now);
                state.log_downlink_summary(
                    "transport_disconnected",
                    audio_cfg.speaker.sample_rate,
                    now,
                    true,
                );
                drain_pending_output_before_transport_exit(platform, &mut state, audio_cfg, now)?;
                log_realtime_transport_exit(
                    RealtimeSessionExitReason::TransportDisconnected,
                    None,
                    &state,
                    interrupted,
                    pending_output,
                    session_start,
                    audio_cfg,
                );
                exit_reason = RealtimeSessionExitReason::TransportDisconnected;
                interrupted_active_turn = interrupted;
                partial_output_pending_at_exit = pending_output;
                break;
            }
            RealtimeDrainOutcome::PeerClosed(summary) => {
                let pending_output = state.has_pending_output_audio(platform, now);
                let interrupted = state.transport_exit_interrupts_active_turn(platform, now);
                state.log_downlink_summary("peer_closed", audio_cfg.speaker.sample_rate, now, true);
                drain_pending_output_before_transport_exit(platform, &mut state, audio_cfg, now)?;
                log_realtime_transport_exit(
                    RealtimeSessionExitReason::PeerClosed,
                    Some(summary.as_str()),
                    &state,
                    interrupted,
                    pending_output,
                    session_start,
                    audio_cfg,
                );
                exit_reason = RealtimeSessionExitReason::PeerClosed;
                interrupted_active_turn = interrupted;
                partial_output_pending_at_exit = pending_output;
                break;
            }
            RealtimeDrainOutcome::Idle | RealtimeDrainOutcome::Events => {}
        }
        maintain_realtime_foreground_if_due(
            &mut foreground_keepalive_at,
            now,
            &mut maintain_foreground,
        );
        update_playback_state(platform, &mut state, audio_cfg.speaker.sample_rate, now)?;
        if crate::orchestrator::take_audio_interrupt_request() {
            handle_local_interrupt(conn.as_mut(), platform, &mut state, provider, now)?;
        }
        if drain_idle {
            if let Some(reason) = should_exit_realtime_session(provider, &state, now) {
                if reason == NoSpeechExitReason::ResponseWait
                    && should_recover_realtime_response_wait(provider)
                {
                    recover_realtime_response_wait(
                        conn.as_mut(),
                        &mut state,
                        provider,
                        audio_cfg,
                        now,
                    );
                    reset_local_endpoint_window(
                        &mut state,
                        &mut endpoint,
                        &mut local_speech_active,
                    );
                    continue;
                }
                match reason {
                    NoSpeechExitReason::NoLocalSpeechAfterSessionReady => {
                        crate::metrics::record_voice_no_speech_timeout()
                    }
                    NoSpeechExitReason::ResponseWait => {
                        crate::metrics::record_voice_response_wait_timeout()
                    }
                    NoSpeechExitReason::PostPlaybackIdle => {
                        crate::metrics::record_voice_post_playback_timeout()
                    }
                }
                log::info!(
                    "[{}] realtime session local exit reason={}",
                    REALTIME_TAG,
                    reason.as_str()
                );
                exit_reason = reason.into();
                interrupted_active_turn = false;
                partial_output_pending_at_exit = false;
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
            if state.duplex_caps.supports_barge_in()
                && should_trigger_playback_interrupt(
                    &mut state,
                    &endpoint_profile,
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

        if handle_local_endpoint_event(
            provider,
            &mut state,
            &mut local_speech_active,
            endpoint.update(chunk, frame_ms, &endpoint_cfg),
            frame_ms,
            endpoint_profile.min_active_ms,
            now,
        ) {
            submit_local_turn(conn.as_mut(), provider, audio_cfg)?;
        }

        if should_force_close_local_speech_window(provider, state.current_local_speech_ms) {
            log::info!(
                "[{}] force closing client-commit local speech window at {}ms without endpoint release",
                REALTIME_TAG,
                state.current_local_speech_ms
            );
            if state.finish_local_speech_window(now) {
                submit_local_turn(conn.as_mut(), provider, audio_cfg)?;
            }
            endpoint.reset();
            local_speech_active = false;
        }

        if local_speech_active
            && state.audio_playing
            && state.duplex_caps.supports_barge_in()
            && should_trigger_playback_interrupt(
                &mut state,
                &endpoint_profile,
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
        server_speech_started: state.server_speech_started,
        server_speech_stopped: state.server_speech_stopped,
        response_created: state.response_created,
        exit_reason,
        interrupted_active_turn,
        partial_output_pending_at_exit,
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

fn build_session_update(
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    endpoint_profile: &VoiceEndpointProfile,
) -> String {
    let mut session = json!({
        "modalities": ["text", "audio"],
        "voice": audio_cfg.realtime.voice.trim(),
        "input_audio_format": provider.input_audio_format(),
        "output_audio_format": provider.output_audio_format(),
    });
    let turn_detection = build_turn_detection(provider, endpoint_profile);
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

fn build_turn_detection(
    provider: RealtimeProvider,
    endpoint_profile: &VoiceEndpointProfile,
) -> serde_json::Value {
    match provider.turn_contract().endpoint_owner {
        crate::audio::endpoint_profile::VoiceEndpointOwner::ClientCommit => {
            return serde_json::Value::Null;
        }
        crate::audio::endpoint_profile::VoiceEndpointOwner::ServerVad => {}
    }

    match provider {
        RealtimeProvider::OpenAiCompatible => json!({
            "type": "server_vad",
            "threshold": endpoint_profile.server_vad_threshold,
            "silence_duration_ms": endpoint_profile.silence_duration_ms,
            "prefix_padding_ms": REALTIME_SERVER_VAD_PREFIX_PADDING_MS,
            "idle_timeout_ms": REALTIME_SERVER_VAD_IDLE_TIMEOUT_MS,
            "create_response": true,
            "interrupt_response": true
        }),
        RealtimeProvider::Qwen => json!({
            "type": "server_vad",
            "threshold": endpoint_profile.server_vad_threshold,
            "silence_duration_ms": endpoint_profile.silence_duration_ms,
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
    if !provider
        .turn_contract()
        .sends_response_create_on_client_commit
    {
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
    )?;
    crate::metrics::record_voice_realtime_local_commit();
    Ok(())
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
) -> Result<RealtimeDrainOutcome> {
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
                return Ok(RealtimeDrainOutcome::TransportDisconnected);
            }
            Some(WssEvent::Closed(close)) => {
                return Ok(RealtimeDrainOutcome::PeerClosed(summarize_close(
                    close.as_ref(),
                )));
            }
            None => {
                return Ok(if saw_event {
                    RealtimeDrainOutcome::Events
                } else {
                    RealtimeDrainOutcome::Idle
                })
            }
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

fn response_event_id(value: &serde_json::Value) -> Option<&str> {
    value
        .get("response_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            value
                .get("response")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
        })
}

fn item_event_id(value: &serde_json::Value) -> Option<&str> {
    value.get("item_id").and_then(|v| v.as_str()).or_else(|| {
        value
            .get("item")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str())
    })
}

fn owned_response_event_id(value: &serde_json::Value) -> Option<String> {
    response_event_id(value).map(str::to_string)
}

fn owned_item_event_id(value: &serde_json::Value) -> Option<String> {
    item_event_id(value).map(str::to_string)
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
    let contract = provider.turn_contract();
    log_realtime_server_event_type(event_type);
    refresh_output_drain_state(platform, state, now);

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
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "suppressed_until_prior_turn_end",
                    now,
                );
                return Ok(());
            }
            if !contract.accepts_response_without_local_commit && !state.has_committed_local_turn()
            {
                state.drop_server_audio_for_stale_turn(event_type, "response_without_commit", now);
                return Ok(());
            }
            if !state.can_begin_output_turn(platform, now) {
                state.suppress_server_audio_until_turn_end = true;
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "previous_output_turn_not_drained",
                    now,
                );
                return Ok(());
            }
            state.mark_server_response_created_with_ids(
                now,
                owned_response_event_id(value),
                owned_item_event_id(value),
            );
            Ok(())
        }
        "input_audio_buffer.speech_started" => {
            if state.suppress_server_audio_until_turn_end
                || should_suppress_server_turn_during_half_duplex_output(state, now)
            {
                state.suppress_server_audio_until_turn_end = true;
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "half_duplex_output_suppressed_server_turn",
                    now,
                );
                log::info!(
                    "[{}] suppressing server VAD turn during half-duplex playback",
                    REALTIME_TAG
                );
                return Ok(());
            }
            if contract.accepts_server_speech_events_without_local_commit {
                state.begin_server_vad_turn(now);
                crate::metrics::record_voice_realtime_server_speech();
            } else if state.has_committed_local_turn() {
                state.awaiting_response = false;
                state.last_activity = now;
            } else {
                state.last_activity = now;
            }
            Ok(())
        }
        "input_audio_buffer.speech_stopped" => {
            if state.suppress_server_audio_until_turn_end {
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "suppressed_until_prior_turn_end",
                    now,
                );
                return Ok(());
            }
            if contract.accepts_server_speech_events_without_local_commit {
                state.mark_server_vad_speech_stopped(now);
            } else if state.has_committed_local_turn() {
                state.awaiting_response = true;
                state.last_activity = now;
            } else {
                state.last_activity = now;
            }
            Ok(())
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            if state.suppress_server_audio_until_turn_end
                && !state.should_accept_server_audio_event(value)
            {
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "suppressed_until_prior_turn_end",
                    now,
                );
                return Ok(());
            }
            if !contract.accepts_response_without_local_commit && !state.has_committed_local_turn()
            {
                state.drop_server_audio_for_stale_turn(event_type, "audio_without_commit", now);
                return Ok(());
            }
            if !state.should_accept_server_audio_event(value) {
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "response_generation_mismatch",
                    now,
                );
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
            if state.suppress_server_audio_until_turn_end
                && !state.should_accept_server_audio_event(value)
            {
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "suppressed_until_prior_turn_end",
                    now,
                );
                return Ok(());
            }
            if !state.mark_active_output_audio_done(value) {
                state.drop_server_audio_for_stale_turn(event_type, "audio_done_stale_turn", now);
                return Ok(());
            }
            // Data already in staging; worker transfers to speaker autonomously.
            state.log_downlink_summary(event_type, audio_cfg.speaker.sample_rate, now, true);
            state.last_activity = now;
            Ok(())
        }
        "response.done" => {
            if state.suppress_server_audio_until_turn_end
                && !state.should_accept_server_audio_event(value)
            {
                state.drop_server_audio_for_stale_turn(
                    event_type,
                    "suppressed_until_prior_turn_end",
                    now,
                );
                return Ok(());
            }
            if !state.mark_active_output_response_done(value) {
                state.drop_server_audio_for_stale_turn(event_type, "response_done_stale_turn", now);
                return Ok(());
            }
            let counted = state.awaiting_response;
            // Data already in staging; worker handles transfer.
            if counted {
                state.awaiting_response = false;
                state.turns_completed = state.turns_completed.saturating_add(1);
                crate::metrics::record_voice_realtime_turn_completed();
            }
            if !state.audio_playing {
                state.playback_finished_at = Some(now);
            }
            state.log_downlink_summary("response.done", audio_cfg.speaker.sample_rate, now, false);
            refresh_output_drain_state(platform, state, now);
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
        _ => {
            if !event_type.is_empty() {
                if state.awaiting_response {
                    log::info!(
                        "[{}] realtime server event type={} unhandled awaiting_response=true",
                        REALTIME_TAG,
                        event_type
                    );
                    state.mark_server_response_activity(now);
                } else {
                    log::debug!(
                        "[{}] realtime server event type={} unhandled awaiting_response=false",
                        REALTIME_TAG,
                        event_type
                    );
                    state.last_activity = now;
                }
            }
            Ok(())
        }
    }
}

fn log_realtime_server_event_type(event_type: &str) {
    match event_type {
        "session.created"
        | "session.updated"
        | "input_audio_buffer.committed"
        | "input_audio_buffer.speech_started"
        | "input_audio_buffer.speech_stopped"
        | "response.created"
        | "response.output_audio.done"
        | "response.audio.done"
        | "response.done"
        | "error" => {
            log::info!(
                "[{}] realtime server event type={}",
                REALTIME_TAG,
                event_type
            );
        }
        _ => {}
    }
}

fn refresh_output_drain_state(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    now: Instant,
) {
    if platform.speaker_staging_samples() != 0 || platform.speaker_buffered_samples() != 0 {
        return;
    }
    if state.active_output_duration_pending(now) {
        return;
    }
    if state.audio_playing {
        state.finish_audio_playback(now);
    } else {
        state.mark_active_output_drained(now);
    }
    if state
        .active_output_turn
        .as_ref()
        .map(|turn| turn.ended)
        .unwrap_or(true)
    {
        state.suppress_server_audio_until_turn_end = false;
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

fn append_handoff_audio(
    conn: &mut dyn WssConnection,
    encoder: &mut RealtimeUploadEncoder,
    provider: RealtimeProvider,
    handoff: WakeAudioHandoff,
    configured_sample_rate_hz: u32,
) -> Result<usize> {
    if handoff.is_empty() {
        return Ok(0);
    }
    if handoff.channels != 1 {
        return Err(Error::config(
            "realtime_voice_handoff",
            format!("wake handoff channels={} is not mono", handoff.channels),
        ));
    }
    let expected = configured_sample_rate_hz.max(8_000);
    if handoff.sample_rate_hz != expected {
        return Err(Error::config(
            "realtime_voice_handoff",
            format!(
                "wake handoff sample_rate_hz={} does not match realtime microphone sample_rate_hz={}",
                handoff.sample_rate_hz, expected
            ),
        ));
    }

    crate::platform::task_wdt::feed_current_task();
    conn.send_text(
        encoder.build_append_event_from_pcm_le_bytes(provider, handoff.pcm_le_bytes.as_slice()),
    )?;
    let samples = handoff.pcm_le_bytes.len() / std::mem::size_of::<i16>();
    log::info!(
        "[{}] uploaded wake handoff id={} pre_roll_ms={} bytes={} acoustic_activation_pm={}",
        REALTIME_TAG,
        handoff.id,
        handoff.pre_roll_ms,
        handoff.pcm_le_bytes.len(),
        handoff.acoustic.activation_pm
    );
    Ok(samples)
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
        let write = queue_output_audio(platform, state, sample_rate_hz, pcm.as_slice())?;
        state.record_downlink_write(Instant::now(), write, sample_rate_hz);
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

fn maintain_realtime_foreground_if_due(
    last_keepalive_at: &mut Instant,
    now: Instant,
    maintain_foreground: &mut impl FnMut(),
) {
    if now.duration_since(*last_keepalive_at)
        < Duration::from_millis(REALTIME_FOREGROUND_KEEPALIVE_MS)
    {
        return;
    }
    maintain_foreground();
    *last_keepalive_at = now;
}

fn drain_pending_output_before_transport_exit(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    audio_cfg: &AudioSegment,
    now: Instant,
) -> Result<()> {
    if !state.has_pending_output_audio(platform, now) {
        return Ok(());
    }

    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(REALTIME_TRANSPORT_EXIT_DRAIN_MS) {
        crate::platform::task_wdt::feed_current_task();
        let now = Instant::now();
        update_playback_state(platform, state, audio_cfg.speaker.sample_rate, now)?;
        refresh_output_drain_state(platform, state, now);
        if !state.has_pending_output_audio(platform, now) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(REALTIME_RECV_POLL_MS));
    }

    log::warn!(
        "[{}] realtime transport exit drain timeout staging={} speaker={} audio_playing={} pending_output=true",
        REALTIME_TAG,
        platform.speaker_staging_samples(),
        platform.speaker_buffered_samples(),
        state.audio_playing
    );
    Ok(())
}

fn log_realtime_transport_exit(
    reason: RealtimeSessionExitReason,
    peer_close: Option<&str>,
    state: &RealtimeLoopState,
    interrupted_active_turn: bool,
    partial_output_pending_at_exit: bool,
    session_start: Instant,
    audio_cfg: &AudioSegment,
) {
    log::warn!(
        "[{}] realtime session transport exit reason={} peer_close={} turns_completed={} input_ms={} output_ms={} duration_ms={} awaiting_response={} audio_playing={} response_created={} server_speech_started={} server_speech_stopped={} interrupted_active_turn={} partial_output_pending_at_exit={}",
        REALTIME_TAG,
        reason.as_str(),
        peer_close.unwrap_or("-"),
        state.turns_completed,
        samples_to_ms(state.input_samples, audio_cfg.microphone.sample_rate),
        samples_to_ms(state.output_samples, audio_cfg.speaker.sample_rate),
        session_start.elapsed().as_millis(),
        state.awaiting_response,
        state.audio_playing,
        state.response_created,
        state.server_speech_started,
        state.server_speech_stopped,
        interrupted_active_turn,
        partial_output_pending_at_exit
    );
}

#[derive(Clone, Copy, Debug, Default)]
struct OutputQueueWrite {
    input_samples: usize,
    staging_written: usize,
    direct_written: usize,
    dropped: usize,
    speaker_buffered_after: usize,
    staging_buffered_after: usize,
}

impl OutputQueueWrite {
    fn accepted_samples(self) -> usize {
        self.staging_written.saturating_add(self.direct_written)
    }
}

fn queue_output_audio(
    platform: &dyn Platform,
    state: &mut RealtimeLoopState,
    sample_rate_hz: u32,
    pcm: &[i16],
) -> Result<OutputQueueWrite> {
    if pcm.is_empty() {
        return Ok(OutputQueueWrite::default());
    }

    start_realtime_playback_before_queue_write(state, Instant::now());

    let accepted = push_output_staging_with_backpressure(platform, pcm)?;
    state.output_samples = state.output_samples.saturating_add(accepted);
    let dropped = pcm.len().saturating_sub(accepted);

    // Start playback once enough data is buffered (staging + speaker combined).
    let speaker_buffered_after = platform.speaker_buffered_samples();
    let staging_buffered_after = platform.speaker_staging_samples();
    if !state.audio_playing {
        let total = speaker_buffered_after.saturating_add(staging_buffered_after);
        if total >= playback_target_buffer_samples(sample_rate_hz) {
            state.start_audio_playback(Instant::now());
        }
    }
    Ok(OutputQueueWrite {
        input_samples: pcm.len(),
        staging_written: accepted,
        direct_written: 0,
        dropped,
        speaker_buffered_after,
        staging_buffered_after,
    })
}

fn push_output_staging_with_backpressure(platform: &dyn Platform, pcm: &[i16]) -> Result<usize> {
    let started = Instant::now();
    let mut written = 0usize;
    while written < pcm.len() {
        crate::platform::task_wdt::feed_current_task();
        let n = platform.push_speaker_staging_pcm_i16(&pcm[written..])?;
        if n > 0 {
            written = written.saturating_add(n).min(pcm.len());
            continue;
        }
        if started.elapsed() >= Duration::from_millis(REALTIME_OUTPUT_STAGING_WAIT_MS) {
            return Err(Error::config(
                REALTIME_TAG,
                "realtime output staging ring blocked",
            ));
        }
        thread::sleep(Duration::from_millis(REALTIME_OUTPUT_STAGING_RETRY_MS));
    }
    Ok(written)
}

fn start_realtime_playback_before_queue_write(state: &mut RealtimeLoopState, now: Instant) {
    if !state.audio_playing {
        state.start_audio_playback(now);
    }
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
    // Playback is only finished after both software queues are empty and the
    // accepted PCM duration has elapsed; the hardware DMA tail is not visible
    // in the software queue counters.
    if platform.speaker_staging_samples() == 0
        && platform.speaker_buffered_samples() == 0
        && !state.active_output_duration_pending(now)
    {
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

fn handle_local_endpoint_event(
    provider: RealtimeProvider,
    state: &mut RealtimeLoopState,
    local_speech_active: &mut bool,
    event: EndpointEvent,
    frame_ms: u32,
    min_active_ms: u32,
    now: Instant,
) -> bool {
    let client_commit_owner = provider.turn_contract().requires_client_commit;
    match event {
        EndpointEvent::SpeechStart => {
            *local_speech_active = true;
            if client_commit_owner {
                state.begin_local_speech_window(now, frame_ms);
            } else {
                state.begin_local_activity_window(now, frame_ms);
            }
            false
        }
        EndpointEvent::SpeechEnd => {
            if !*local_speech_active {
                return false;
            }
            *local_speech_active = false;
            if client_commit_owner {
                state.finish_local_speech_window(now)
            } else {
                state.finish_local_activity_window(now);
                false
            }
        }
        EndpointEvent::None => {
            if *local_speech_active {
                if client_commit_owner {
                    state.extend_local_speech_window(now, frame_ms, min_active_ms);
                } else {
                    state.extend_local_activity_window(now, frame_ms);
                }
            }
            false
        }
    }
}

fn should_force_close_local_speech_window(provider: RealtimeProvider, current_ms: u32) -> bool {
    provider.turn_contract().requires_client_commit
        && current_ms >= REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS
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
    endpoint_profile: &VoiceEndpointProfile,
    pcm: &[i16],
    reference_pcm: &[i16],
    frame_ms: u32,
    now: Instant,
) -> bool {
    let (rms, _) = reference_adjusted_interrupt_rms(pcm, reference_pcm);
    if let Some(deadline) = state.interrupt_baseline_deadline {
        if now < deadline {
            state.interrupt_baseline_peak = state.interrupt_baseline_peak.max(rms);
            return false;
        }
        state.interrupt_baseline_deadline = None;
    }

    let threshold = playback_interrupt_threshold(endpoint_profile, state.interrupt_baseline_peak);
    if rms >= threshold {
        state.interrupt_speech_ms = state.interrupt_speech_ms.saturating_add(frame_ms);
    } else {
        state.interrupt_speech_ms = 0;
    }
    state.interrupt_speech_ms >= playback_interrupt_required_ms(endpoint_profile)
}

fn playback_interrupt_threshold(
    endpoint_profile: &VoiceEndpointProfile,
    interrupt_baseline_peak: f32,
) -> f32 {
    if endpoint_profile.low_snr_codec_profile {
        return (endpoint_profile.local_enter_threshold
            * REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MULTIPLIER)
            .max(interrupt_baseline_peak + REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MARGIN)
            .clamp(REALTIME_INTERRUPT_LOW_SNR_THRESHOLD_MIN, 0.12);
    }
    (endpoint_profile.local_enter_threshold * REALTIME_INTERRUPT_THRESHOLD_MULTIPLIER)
        .max(interrupt_baseline_peak + REALTIME_INTERRUPT_THRESHOLD_MARGIN)
        .clamp(REALTIME_INTERRUPT_THRESHOLD_MIN, 0.95)
}

fn playback_interrupt_required_ms(endpoint_profile: &VoiceEndpointProfile) -> u32 {
    if endpoint_profile.low_snr_codec_profile {
        return REALTIME_INTERRUPT_LOW_SNR_SPEECH_MIN_MS;
    }
    REALTIME_INTERRUPT_SPEECH_MIN_MS
}

fn post_response_idle_timeout(provider: RealtimeProvider) -> Duration {
    match provider.turn_contract().endpoint_owner {
        crate::audio::endpoint_profile::VoiceEndpointOwner::ServerVad => {
            Duration::from_millis(REALTIME_SERVER_VAD_POST_RESPONSE_IDLE_TIMEOUT_MS)
        }
        crate::audio::endpoint_profile::VoiceEndpointOwner::ClientCommit => {
            Duration::from_millis(REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS)
        }
    }
}

fn should_exit_realtime_session(
    provider: RealtimeProvider,
    state: &RealtimeLoopState,
    now: Instant,
) -> Option<NoSpeechExitReason> {
    if let Some(session_ready_at) = state.session_ready_at {
        if state.first_local_speech_at.is_none()
            && !state.awaiting_response
            && !state.audio_playing
            && now.duration_since(session_ready_at)
                >= Duration::from_millis(REALTIME_NO_SPEECH_TIMEOUT_MS)
        {
            return Some(NoSpeechExitReason::NoLocalSpeechAfterSessionReady);
        }
    }

    if let Some(last_speech_end_at) = state.last_local_speech_end_at {
        if state.awaiting_response
            && !state.current_turn_received_server_activity
            && !state.suppress_server_audio_until_turn_end
            && now.duration_since(last_speech_end_at)
                >= Duration::from_millis(REALTIME_RESPONSE_WAIT_TIMEOUT_MS)
        {
            return Some(NoSpeechExitReason::ResponseWait);
        }
    }

    if state.awaiting_response
        && state.current_turn_received_server_activity
        && !state.audio_playing
        && now.duration_since(state.last_activity)
            >= Duration::from_millis(REALTIME_RESPONSE_WAIT_TIMEOUT_MS)
    {
        return Some(NoSpeechExitReason::ResponseWait);
    }

    if let Some(playback_finished_at) = state.playback_finished_at {
        let post_response_idle_timeout = post_response_idle_timeout(provider);
        if !state.awaiting_response
            && !state.audio_playing
            && state.current_local_speech_ms == 0
            && now.duration_since(playback_finished_at) >= post_response_idle_timeout
            && now.duration_since(state.last_activity) >= post_response_idle_timeout
        {
            return Some(NoSpeechExitReason::PostPlaybackIdle);
        }
    }
    None
}

fn should_recover_realtime_response_wait(provider: RealtimeProvider) -> bool {
    provider
        .turn_contract()
        .accepts_server_speech_events_without_local_commit
}

fn recover_realtime_response_wait(
    conn: &mut dyn WssConnection,
    state: &mut RealtimeLoopState,
    provider: RealtimeProvider,
    audio_cfg: &AudioSegment,
    now: Instant,
) {
    crate::metrics::record_voice_response_wait_timeout();
    state.log_downlink_summary(
        "response_wait_recovered",
        audio_cfg.speaker.sample_rate,
        now,
        true,
    );
    if let Err(error) = send_text_retry(
        conn,
        build_response_cancel_event().as_str(),
        REALTIME_INITIAL_SEND_RETRY_MAX,
    ) {
        log::warn!(
            "[{}] realtime response wait recovery cancel send failed provider={:?}: {}",
            REALTIME_TAG,
            provider,
            error
        );
    } else {
        crate::metrics::record_voice_cancel_sent();
    }
    log::warn!(
        "[{}] realtime response wait timeout recovered provider={:?} turns_completed={} response_created={} server_speech_started={} server_speech_stopped={}",
        REALTIME_TAG,
        provider,
        state.turns_completed,
        state.response_created,
        state.server_speech_started,
        state.server_speech_stopped
    );
    state.recover_response_wait(now);
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
    state.cancel_active_output_turn();
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
    state
        .duplex_caps
        .requires_capture_upload_suspend_during_playback()
        && !state.duplex_caps.supports_barge_in()
        && state.awaiting_response
        && state.current_turn_received_server_activity
}

fn should_suppress_server_turn_during_half_duplex_output(
    state: &RealtimeLoopState,
    now: Instant,
) -> bool {
    state
        .duplex_caps
        .requires_capture_upload_suspend_during_playback()
        && !state.duplex_caps.supports_barge_in()
        && (state.audio_playing
            || state.has_pending_output_turn(now)
            || (state.awaiting_response && state.current_turn_received_server_activity))
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

fn samples_to_duration(samples: usize, sample_rate_hz: u32) -> Duration {
    let micros = (samples as u128)
        .saturating_mul(1_000_000)
        .saturating_div(sample_rate_hz.max(1) as u128)
        .min(u64::MAX as u128) as u64;
    Duration::from_micros(micros)
}

#[cfg(test)]
mod tests {
    use super::{
        build_realtime_headers, build_realtime_ws_url, build_session_update,
        decode_pcm16_delta_into, realtime_ws_url_needs_openai_beta,
        server_message_marks_session_ready, NoSpeechExitReason, RealtimeLoopState,
        RealtimeProvider, RealtimeUploadEncoder, REALTIME_OPENAI_BETA,
    };
    use crate::audio::endpoint_profile::VoiceEndpointProfile;
    use crate::audio::energy::EndpointEvent;
    use crate::audio::input_profile::AudioInputHardwareProfile;
    use crate::config::default_disabled_audio_segment;
    use crate::platform::AudioDuplexCapabilities;
    use base64::Engine as _;
    use std::time::{Duration, Instant};

    fn realtime_cfg(provider: &str) -> crate::config::AudioSegment {
        let mut cfg = default_disabled_audio_segment();
        cfg.realtime.provider = provider.to_string();
        cfg.realtime.api_key = "token".to_string();
        cfg.realtime.ws_url = "wss://ai-gateway.vei.volces.com/v1/realtime".to_string();
        cfg.realtime.model = "doubao-seed-realtime".to_string();
        cfg.realtime.voice = "zh_female_tianmei".to_string();
        cfg
    }

    fn endpoint_profile(
        provider: RealtimeProvider,
        cfg: &crate::config::AudioSegment,
    ) -> VoiceEndpointProfile {
        let input = AudioInputHardwareProfile::from_audio_config(cfg);
        VoiceEndpointProfile::from_input_profile(
            input,
            cfg,
            provider,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        )
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
        let mut encoder = RealtimeUploadEncoder::new(24_000);
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
        let profile = endpoint_profile(RealtimeProvider::Qwen, &cfg);
        let payload = build_session_update(RealtimeProvider::Qwen, &cfg, &profile);
        assert!(payload.contains("\"input_audio_format\":\"pcm\""));
        assert!(payload.contains("\"output_audio_format\":\"pcm\""));
        assert!(!payload.contains("prefix_padding_ms"));
        assert!(!payload.contains("idle_timeout_ms"));
        assert!(!payload.contains("create_response"));
    }

    #[test]
    fn qwen_session_update_uses_endpoint_profile_server_vad_threshold() {
        let mut cfg = realtime_cfg("qwen");
        cfg.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        cfg.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        cfg.codec.input_reference = true;
        cfg.wake_word.enabled = true;
        cfg.wake_word.enter_threshold = 0.01;
        cfg.wake_word.leave_threshold = 0.005;
        cfg.vad.threshold = 0.5;
        let input = AudioInputHardwareProfile::from_audio_config(&cfg);
        let profile = VoiceEndpointProfile::from_input_profile(
            input,
            &cfg,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );

        let payload = build_session_update(RealtimeProvider::Qwen, &cfg, &profile);

        let value: serde_json::Value = serde_json::from_str(payload.as_str()).unwrap();
        let threshold = value["session"]["turn_detection"]["threshold"]
            .as_f64()
            .unwrap();
        assert!((threshold - 0.01).abs() < 0.000_001);
    }

    #[test]
    fn qwen_audio_append_contains_event_id() {
        let mut encoder =
            RealtimeUploadEncoder::new(crate::config::AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE);
        let payload = encoder.build_append_event(RealtimeProvider::Qwen, &[1, 2, 3]);
        assert!(payload.contains("\"event_id\":\"audio_"));
        assert!(payload.contains("\"type\":\"input_audio_buffer.append\""));
    }

    #[test]
    fn qwen_audio_append_keeps_16k_pcm_without_downsampling() {
        let mut encoder =
            RealtimeUploadEncoder::new(crate::config::AUDIO_REALTIME_QWEN_PCM_SAMPLE_RATE);
        let pcm = [1u8, 0, 2, 0, 3, 0];
        let encoded = base64::engine::general_purpose::STANDARD.encode(pcm);

        let payload = encoder.build_append_event_from_pcm_le_bytes(RealtimeProvider::Qwen, &pcm);

        assert!(payload.contains(encoded.as_str()));
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
        let profile = endpoint_profile(RealtimeProvider::OpenAiCompatible, &cfg);
        let payload = build_session_update(RealtimeProvider::OpenAiCompatible, &cfg, &profile);
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
        let profile = endpoint_profile(provider, &cfg);
        let payload = build_session_update(provider, &cfg, &profile);

        assert!(payload.contains("\"voice\":\"zh_female_tianmei\""));
        assert!(payload.contains("\"input_audio_format\":\"pcm16\""));
        assert!(payload.contains("\"output_audio_format\":\"pcm16\""));
        assert!(!payload.contains("\"turn_detection\""));
    }

    #[test]
    fn server_vad_response_sequence_completes_turn_without_local_commit() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();

        assert!(!state.has_committed_local_turn());
        state.begin_server_vad_turn(now);
        state.mark_server_response_created_with_ids(now + Duration::from_millis(1), None, None);
        state.mark_server_response_activity(now + Duration::from_millis(2));

        assert!(state.awaiting_response);
        assert!(state.should_accept_server_audio());
        let counted = state.awaiting_response;
        if counted {
            state.awaiting_response = false;
            state.turns_completed = state.turns_completed.saturating_add(1);
        }

        assert_eq!(state.turns_completed, 1);
    }

    #[test]
    fn server_vad_response_wait_is_recoverable_but_client_commit_is_not() {
        assert!(super::should_recover_realtime_response_wait(
            RealtimeProvider::Qwen
        ));
        assert!(super::should_recover_realtime_response_wait(
            RealtimeProvider::OpenAiCompatible
        ));
        assert!(!super::should_recover_realtime_response_wait(
            RealtimeProvider::Doubao
        ));
    }

    #[test]
    fn response_wait_recovery_clears_server_vad_turn_state() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now - Duration::from_millis(30_000));
        state.begin_server_vad_turn(now - Duration::from_millis(10_000));
        state.mark_server_vad_speech_stopped(now - Duration::from_millis(9_000));
        state.mark_server_response_created_with_ids(now - Duration::from_millis(8_500), None, None);
        state.mark_server_response_activity(now - Duration::from_millis(8_100));

        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Qwen, &state, now),
            Some(NoSpeechExitReason::ResponseWait)
        );

        state.recover_response_wait(now);

        assert!(!state.awaiting_response);
        assert!(!state.current_turn_received_server_activity);
        assert!(!state.suppress_server_audio_until_turn_end);
        assert_eq!(state.current_local_speech_ms, 0);
        assert!(state.last_local_speech_end_at.is_none());
        assert!(!state.has_pending_output_turn(now));
        assert_eq!(
            super::should_exit_realtime_session(
                RealtimeProvider::Qwen,
                &state,
                now + Duration::from_millis(100),
            ),
            None
        );
    }

    #[test]
    fn should_exit_realtime_session_reports_no_local_speech_reason() {
        let mut state = RealtimeLoopState::new(AudioDuplexCapabilities::duplex_without_aec());
        let ready_at = Instant::now();
        state.mark_session_ready(ready_at);

        let reason = super::should_exit_realtime_session(
            RealtimeProvider::Qwen,
            &state,
            ready_at + Duration::from_millis(super::REALTIME_NO_SPEECH_TIMEOUT_MS),
        );

        assert_eq!(
            reason,
            Some(NoSpeechExitReason::NoLocalSpeechAfterSessionReady)
        );
    }

    #[test]
    fn post_playback_idle_does_not_exit_while_local_speech_is_active() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now - Duration::from_millis(20_000));
        state.first_local_speech_at = Some(now - Duration::from_millis(10_000));
        state.playback_finished_at =
            Some(now - Duration::from_millis(super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS));
        state.current_local_speech_ms = 240;
        state.last_activity = now;

        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Qwen, &state, now),
            None
        );
    }

    #[test]
    fn post_playback_idle_waits_for_last_local_activity() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now - Duration::from_millis(20_000));
        state.first_local_speech_at = Some(now - Duration::from_millis(10_000));
        state.playback_finished_at =
            Some(now - Duration::from_millis(super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS));
        state.last_activity = now - Duration::from_millis(500);

        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Doubao, &state, now),
            None
        );

        state.last_activity =
            now - Duration::from_millis(super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS);
        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Doubao, &state, now),
            Some(NoSpeechExitReason::PostPlaybackIdle)
        );
    }

    #[test]
    fn server_vad_post_playback_idle_uses_extended_grace() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now - Duration::from_millis(30_000));
        state.first_local_speech_at = Some(now - Duration::from_millis(25_000));
        state.playback_finished_at =
            Some(now - Duration::from_millis(super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS));
        state.last_activity =
            now - Duration::from_millis(super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS);

        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Qwen, &state, now),
            None
        );

        let expired = now
            + Duration::from_millis(
                super::REALTIME_SERVER_VAD_POST_RESPONSE_IDLE_TIMEOUT_MS
                    - super::REALTIME_POST_RESPONSE_IDLE_TIMEOUT_MS,
            );
        assert_eq!(
            super::should_exit_realtime_session(RealtimeProvider::Qwen, &state, expired),
            Some(NoSpeechExitReason::PostPlaybackIdle)
        );
    }

    #[test]
    fn server_vad_local_endpoint_activity_does_not_arm_response_wait() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now - Duration::from_millis(30_000));
        state.begin_server_vad_turn(now - Duration::from_millis(10_000));
        state.mark_server_vad_speech_stopped(now - Duration::from_millis(9_000));
        state.mark_server_response_created_with_ids(now - Duration::from_millis(8_500), None, None);
        state.mark_server_response_activity(now - Duration::from_millis(8_000));
        state.awaiting_response = false;
        state.turns_completed = 1;
        state.playback_finished_at = Some(now - Duration::from_millis(1_000));
        state.last_activity = now - Duration::from_millis(1_000);

        let mut local_speech_active = false;
        assert!(!super::handle_local_endpoint_event(
            RealtimeProvider::Qwen,
            &mut state,
            &mut local_speech_active,
            EndpointEvent::SpeechStart,
            40,
            120,
            now,
        ));
        assert!(!super::handle_local_endpoint_event(
            RealtimeProvider::Qwen,
            &mut state,
            &mut local_speech_active,
            EndpointEvent::None,
            40,
            120,
            now + Duration::from_millis(40),
        ));
        assert!(!super::handle_local_endpoint_event(
            RealtimeProvider::Qwen,
            &mut state,
            &mut local_speech_active,
            EndpointEvent::SpeechEnd,
            40,
            120,
            now + Duration::from_millis(80),
        ));

        assert!(!state.awaiting_response);
        assert_eq!(
            super::should_exit_realtime_session(
                RealtimeProvider::Qwen,
                &state,
                now + Duration::from_millis(super::REALTIME_RESPONSE_WAIT_TIMEOUT_MS),
            ),
            None
        );
    }

    #[test]
    fn server_vad_local_activity_is_not_force_closed_by_client_commit_cap() {
        assert!(!super::should_force_close_local_speech_window(
            RealtimeProvider::Qwen,
            super::REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS
        ));
        assert!(!super::should_force_close_local_speech_window(
            RealtimeProvider::OpenAiCompatible,
            super::REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS
        ));
        assert!(!super::should_force_close_local_speech_window(
            RealtimeProvider::Doubao,
            super::REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS - 1
        ));
        assert!(super::should_force_close_local_speech_window(
            RealtimeProvider::Doubao,
            super::REALTIME_LOCAL_SPEECH_WINDOW_MAX_MS
        ));
    }

    #[test]
    fn half_duplex_response_created_holds_capture_before_first_audio_delta() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now);
        state.begin_server_vad_turn(now);
        state.mark_server_vad_speech_stopped(now + Duration::from_millis(40));
        state.mark_server_response_created_with_ids(now + Duration::from_millis(80), None, None);

        assert!(super::should_hold_local_capture_for_server_response(
            RealtimeProvider::Qwen,
            &state
        ));
    }

    #[test]
    fn half_duplex_output_suppresses_stale_server_turn_until_done() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now);
        state.begin_server_vad_turn(now);
        state.mark_server_vad_speech_stopped(now + Duration::from_millis(40));
        state.mark_server_response_created_with_ids(now + Duration::from_millis(80), None, None);

        assert!(super::should_suppress_server_turn_during_half_duplex_output(&state, now));

        state.start_audio_playback(now + Duration::from_millis(120));
        state.awaiting_response = false;
        assert!(super::should_suppress_server_turn_during_half_duplex_output(&state, now));

        state.finish_audio_playback(now + Duration::from_millis(1000));
        assert!(
            super::should_suppress_server_turn_during_half_duplex_output(
                &state,
                now + Duration::from_millis(1000)
            )
        );

        state
            .active_output_turn
            .as_mut()
            .unwrap()
            .mark_response_done();
        state.mark_active_output_drained(now + Duration::from_millis(1000));
        assert!(
            !super::should_suppress_server_turn_during_half_duplex_output(
                &state,
                now + Duration::from_millis(1000)
            )
        );
        crate::orchestrator::set_audio_interrupt_listening(false);
        crate::orchestrator::set_audio_playing(false);
    }

    #[test]
    fn half_duplex_output_keeps_turn_pending_until_audio_duration_drained() {
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        let now = Instant::now();
        state.mark_session_ready(now);
        state.begin_server_vad_turn(now);
        state.mark_server_vad_speech_stopped(now + Duration::from_millis(40));
        state.mark_server_response_created_with_ids(now + Duration::from_millis(80), None, None);
        state.awaiting_response = false;
        let write = super::OutputQueueWrite {
            input_samples: 16_000,
            staging_written: 16_000,
            direct_written: 0,
            dropped: 0,
            speaker_buffered_after: 0,
            staging_buffered_after: 0,
        };
        state.record_downlink_write(now + Duration::from_millis(100), write, 16_000);
        state
            .active_output_turn
            .as_mut()
            .unwrap()
            .mark_response_done();
        state.mark_active_output_drained(now + Duration::from_millis(500));

        assert!(
            super::should_suppress_server_turn_during_half_duplex_output(
                &state,
                now + Duration::from_millis(500)
            )
        );

        state.mark_active_output_drained(now + Duration::from_millis(1200));
        assert!(
            !super::should_suppress_server_turn_during_half_duplex_output(
                &state,
                now + Duration::from_millis(1200)
            )
        );
    }

    #[test]
    fn realtime_downlink_marks_playback_active_before_output_queueing() {
        crate::orchestrator::set_audio_playing(false);
        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());

        super::start_realtime_playback_before_queue_write(&mut state, Instant::now());

        assert!(state.audio_playing);
        assert!(crate::orchestrator::is_audio_playing());
        crate::orchestrator::set_audio_interrupt_listening(false);
        crate::orchestrator::set_audio_playing(false);
    }

    #[test]
    fn realtime_foreground_keepalive_runs_before_ticket_expiry() {
        let start = Instant::now();
        let mut last_keepalive_at = start;
        let mut calls = 0usize;

        super::maintain_realtime_foreground_if_due(
            &mut last_keepalive_at,
            start + Duration::from_millis(super::REALTIME_FOREGROUND_KEEPALIVE_MS - 1),
            &mut || calls += 1,
        );
        assert_eq!(calls, 0);

        super::maintain_realtime_foreground_if_due(
            &mut last_keepalive_at,
            start + Duration::from_millis(super::REALTIME_FOREGROUND_KEEPALIVE_MS),
            &mut || calls += 1,
        );
        assert_eq!(calls, 1);
    }

    #[test]
    fn low_snr_playback_interrupt_uses_endpoint_profile_threshold() {
        let mut cfg = realtime_cfg("qwen");
        cfg.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        cfg.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        cfg.codec.input_reference = true;
        cfg.wake_word.enabled = true;
        cfg.wake_word.enter_threshold = 0.01;
        cfg.wake_word.leave_threshold = 0.005;
        cfg.wake_word.zcr_max = 0.65;
        cfg.wake_word.min_speech_band_ratio = 0.35;
        cfg.wake_word.min_active_ms = 120;
        let profile = endpoint_profile(RealtimeProvider::Qwen, &cfg);
        assert!(profile.low_snr_codec_profile);

        let mut state =
            RealtimeLoopState::new(AudioDuplexCapabilities::duplex_with_input_reference());
        state.interrupt_baseline_deadline = None;
        let pcm = [1000i16; 320];
        let now = Instant::now();

        for i in 0..2 {
            assert!(!super::should_trigger_playback_interrupt(
                &mut state,
                &profile,
                &pcm,
                &[],
                40,
                now + Duration::from_millis(i * 40),
            ));
        }
        assert!(super::should_trigger_playback_interrupt(
            &mut state,
            &profile,
            &pcm,
            &[],
            40,
            now + Duration::from_millis(80),
        ));
        assert_eq!(super::playback_interrupt_required_ms(&profile), 120);
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
