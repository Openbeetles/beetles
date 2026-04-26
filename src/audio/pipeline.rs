//! 共享语音编排：收口采集+转写、TTS+播放两条主链，供 voice_session 与语音工具共用。
//! Shared voice pipeline helpers for capture/transcribe and TTS playback.

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::capture::{capture_speech, AudioRecordingGuard};
use crate::audio::{stt_baidu, tts_baidu};
use crate::config::AudioSegment;
use crate::constants::{
    AUDIO_SPEAKER_DRAIN_GRACE_MS, AUDIO_SPEAKER_DRAIN_POLL_MS, AUDIO_TTS_WRITE_CHUNK_SAMPLES,
};
use crate::error::{Error, Result};
use crate::platform::PlatformHttpClient;
use crate::Platform;
use std::time::{Duration, Instant};

const VOICE_LOCAL_INTERRUPT_STAGE: &str = "voice_local_interrupt";

/// Logical owner for audio input/output leases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioLeaseOwner {
    VoiceSession,
    VoiceRealtime,
    VoiceInputTool,
    VoiceOutputTool,
}

impl AudioLeaseOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VoiceSession => "voice_session",
            Self::VoiceRealtime => "voice_realtime",
            Self::VoiceInputTool => "voice_input_tool",
            Self::VoiceOutputTool => "voice_output_tool",
        }
    }

    const fn lease_owner(self) -> crate::runtime::lease::LeaseOwner {
        crate::runtime::lease::LeaseOwner::new("voice", self.as_str())
    }
}

#[derive(Debug)]
pub struct AudioLeaseGuard {
    kind: crate::runtime::lease::LeaseKind,
    owner: AudioLeaseOwner,
    token: u64,
}

impl Drop for AudioLeaseGuard {
    fn drop(&mut self) {
        let _ =
            crate::runtime::lease::release_token(self.kind, self.owner.lease_owner(), self.token);
    }
}

pub fn acquire_audio_lease(
    kind: crate::runtime::lease::LeaseKind,
    owner: AudioLeaseOwner,
) -> Result<AudioLeaseGuard> {
    acquire_audio_lease_inner(kind, owner, None)
}

#[cfg(test)]
pub(crate) fn acquire_audio_lease_at(
    kind: crate::runtime::lease::LeaseKind,
    owner: AudioLeaseOwner,
    now_ms: u64,
) -> Result<AudioLeaseGuard> {
    acquire_audio_lease_inner(kind, owner, Some(now_ms))
}

fn acquire_audio_lease_inner(
    kind: crate::runtime::lease::LeaseKind,
    owner: AudioLeaseOwner,
    now_ms: Option<u64>,
) -> Result<AudioLeaseGuard> {
    if !matches!(
        kind,
        crate::runtime::lease::LeaseKind::AudioInput
            | crate::runtime::lease::LeaseKind::AudioOutput
    ) {
        return Err(Error::config(
            "audio_lease",
            format!("unsupported audio lease kind={}", kind.as_str()),
        ));
    }

    let decision = match now_ms {
        Some(now_ms) => crate::runtime::lease::try_acquire_at(
            kind,
            owner.lease_owner(),
            crate::runtime::lease::LeaseMode::Exclusive,
            None,
            now_ms,
        ),
        None => crate::runtime::lease::try_acquire(
            kind,
            owner.lease_owner(),
            crate::runtime::lease::LeaseMode::Exclusive,
            None,
        ),
    };
    match decision {
        crate::runtime::lease::LeaseDecision::Acquired(record)
        | crate::runtime::lease::LeaseDecision::Reentered(record)
        | crate::runtime::lease::LeaseDecision::ReplacedExpired {
            current: record, ..
        } => Ok(AudioLeaseGuard {
            kind,
            owner,
            token: record.token,
        }),
        crate::runtime::lease::LeaseDecision::Denied(denial) => Err(Error::config(
            "audio_lease",
            format!(
                "{} owner={} denied reason={} held_by={:?}",
                kind.as_str(),
                owner.as_str(),
                denial.reason,
                denial.held_by
            ),
        )),
    }
}

pub struct VoicePlaybackStats {
    pub played_samples: usize,
    pub tts_http_ms: u128,
    pub play_ms: u128,
    pub interrupted: bool,
}

pub fn capture_and_transcribe(
    owner: AudioLeaseOwner,
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    max_ms: u32,
    log_tag: &'static str,
) -> Result<String> {
    let captured = {
        let _audio_input_lease =
            acquire_audio_lease(crate::runtime::lease::LeaseKind::AudioInput, owner)?;
        let _recording_guard = AudioRecordingGuard::new();
        capture_speech(platform, audio_cfg, max_ms, log_tag)?
    };
    let mic_sr = audio_cfg.microphone.sample_rate.max(8_000);
    let stt_start = Instant::now();
    let text = stt_baidu::transcribe_pcm16_samples(
        http,
        baidu_token,
        &audio_cfg.speech,
        captured.as_slice(),
        mic_sr,
    )?;
    crate::metrics::record_voice_input_stt_http_ms(stt_start.elapsed().as_millis());
    Ok(text)
}

pub fn speak_text(
    owner: AudioLeaseOwner,
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    text: &str,
) -> Result<VoicePlaybackStats> {
    let _audio_output_lease =
        acquire_audio_lease(crate::runtime::lease::LeaseKind::AudioOutput, owner)?;
    let tts_start = Instant::now();
    let mut first_pcm_at: Option<Instant> = None;
    let playback_state = PlaybackStateGuard::new(platform);
    let mut played_samples = 0usize;
    let mut interrupted = false;
    let result = (|| match tts_baidu::stream_wav_pcm16le(
        http,
        baidu_token,
        &audio_cfg.speech,
        &audio_cfg.tts,
        text,
        AUDIO_TTS_WRITE_CHUNK_SAMPLES,
        |chunk| {
            write_playback_chunk(
                platform,
                playback_state.interrupt_armed,
                chunk,
                &mut first_pcm_at,
                &mut played_samples,
                &mut interrupted,
            )
        },
    ) {
        Ok(_) => Ok(()),
        Err(error) if error.stage() == VOICE_LOCAL_INTERRUPT_STAGE => Ok(()),
        Err(error) if error.stage() == "tts_baidu_wav" => {
            let wav = tts_baidu::synthesize_wav(
                http,
                baidu_token,
                &audio_cfg.speech,
                &audio_cfg.tts,
                text,
            )?;
            tts_baidu::play_wav_pcm16le_chunks(&wav, AUDIO_TTS_WRITE_CHUNK_SAMPLES, |chunk| {
                write_playback_chunk(
                    platform,
                    playback_state.interrupt_armed,
                    chunk,
                    &mut first_pcm_at,
                    &mut played_samples,
                    &mut interrupted,
                )
            })
            .map(|_| ())
            .or_else(|error| {
                if error.stage() == VOICE_LOCAL_INTERRUPT_STAGE {
                    Ok(())
                } else {
                    Err(error)
                }
            })
        }
        Err(error) => Err(error),
    })();
    abort_playback_on_stream_error(result, || {
        let _ = platform.clear_speaker_buffer();
    })?;
    interrupted |= accept_local_interrupt(platform, playback_state.interrupt_armed)?;
    if !interrupted {
        interrupted |= wait_for_platform_playback_drain(
            platform,
            audio_cfg.speaker.sample_rate,
            played_samples,
            playback_state.interrupt_armed,
        )?;
    }
    let total_ms = tts_start.elapsed().as_millis();
    let play_ms = first_pcm_at.map(|at| at.elapsed().as_millis()).unwrap_or(0);
    Ok(VoicePlaybackStats {
        played_samples,
        tts_http_ms: total_ms,
        play_ms,
        interrupted,
    })
}

fn wait_for_platform_playback_drain(
    platform: &dyn Platform,
    sample_rate: u32,
    played_samples: usize,
    interrupt_armed: bool,
) -> Result<bool> {
    if played_samples == 0 {
        return Ok(false);
    }
    let sample_rate = u128::from(sample_rate.max(8_000));
    let playback_ms = (played_samples as u128)
        .saturating_mul(1_000)
        .saturating_add(sample_rate.saturating_sub(1))
        / sample_rate;
    let timeout_ms = playback_ms
        .saturating_add(u128::from(AUDIO_SPEAKER_DRAIN_GRACE_MS))
        .min(u128::from(u64::MAX)) as u64;
    wait_for_playback_drain(
        || {
            (
                platform.audio_speaker_ready(),
                platform
                    .speaker_buffered_samples()
                    .saturating_add(platform.speaker_staging_samples()),
            )
        },
        || accept_local_interrupt(platform, interrupt_armed),
        Duration::from_millis(timeout_ms),
        Duration::from_millis(AUDIO_SPEAKER_DRAIN_POLL_MS),
    )
}

fn wait_for_playback_drain(
    mut state: impl FnMut() -> (bool, usize),
    mut take_interrupt: impl FnMut() -> Result<bool>,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<bool> {
    let started_at = Instant::now();
    let mut saw_pending_samples = false;
    loop {
        if take_interrupt()? {
            return Ok(true);
        }
        let (ready, pending_samples) = state();
        saw_pending_samples |= pending_samples > 0;
        if pending_samples == 0 {
            if saw_pending_samples && !ready {
                return Err(Error::config(
                    "audio_speaker",
                    "speaker became unavailable during playback",
                ));
            }
            return Ok(false);
        }
        if !ready {
            return Err(Error::config(
                "audio_speaker",
                "speaker became unavailable during playback",
            ));
        }
        if started_at.elapsed() >= timeout {
            return Err(Error::config(
                "audio_speaker",
                format!(
                    "speaker playback drain timeout after {} ms (pending_samples={})",
                    timeout.as_millis(),
                    pending_samples
                ),
            ));
        }
        if !poll_interval.is_zero() {
            std::thread::sleep(poll_interval);
        }
    }
}

fn abort_playback_on_stream_error(
    result: Result<()>,
    mut abort_output: impl FnMut(),
) -> Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            abort_output();
            Err(error)
        }
    }
}

struct PlaybackStateGuard {
    interrupt_armed: bool,
}

impl PlaybackStateGuard {
    fn new(platform: &dyn Platform) -> Self {
        let interrupt_armed = platform.audio_duplex_capabilities().supports_barge_in()
            && crate::wake::requires_pcm_feed();
        crate::orchestrator::clear_audio_interrupt_request();
        crate::orchestrator::set_audio_playing(true);
        crate::orchestrator::set_audio_interrupt_listening(interrupt_armed);
        Self { interrupt_armed }
    }
}

impl Drop for PlaybackStateGuard {
    fn drop(&mut self) {
        crate::orchestrator::set_audio_interrupt_listening(false);
        crate::orchestrator::set_audio_playing(false);
    }
}

fn write_playback_chunk(
    platform: &dyn Platform,
    interrupt_armed: bool,
    chunk: &[i16],
    first_pcm_at: &mut Option<Instant>,
    played_samples: &mut usize,
    interrupted: &mut bool,
) -> Result<()> {
    if accept_local_interrupt(platform, interrupt_armed)? {
        *interrupted = true;
        return Err(Error::config(
            VOICE_LOCAL_INTERRUPT_STAGE,
            "local interrupt accepted during playback",
        ));
    }
    if first_pcm_at.is_none() {
        *first_pcm_at = Some(Instant::now());
    }
    platform.write_speaker_pcm_i16(chunk)?;
    *played_samples = played_samples.saturating_add(chunk.len());
    Ok(())
}

fn accept_local_interrupt(platform: &dyn Platform, interrupt_armed: bool) -> Result<bool> {
    if !interrupt_armed || !crate::orchestrator::take_audio_interrupt_request() {
        return Ok(false);
    }
    crate::metrics::record_voice_interrupt_accepted();
    platform.clear_speaker_buffer()?;
    log::info!("[voice_pipeline] local interrupt accepted; playback aborted");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use crate::error::Result;

    use super::{
        abort_playback_on_stream_error, acquire_audio_lease_at, wait_for_playback_drain,
        AudioLeaseOwner,
    };

    #[test]
    fn audio_lease_allows_distinct_input_and_output_for_same_owner() {
        let _guard = crate::runtime::lease::lease_test_guard();

        let input = acquire_audio_lease_at(
            crate::runtime::lease::LeaseKind::AudioInput,
            AudioLeaseOwner::VoiceSession,
            100,
        )
        .expect("input lease");
        let output = acquire_audio_lease_at(
            crate::runtime::lease::LeaseKind::AudioOutput,
            AudioLeaseOwner::VoiceSession,
            101,
        )
        .expect("output lease");

        let snapshot = crate::runtime::lease::snapshot_at(102);
        assert_eq!(snapshot.active_count, 2);

        drop(output);
        drop(input);
        assert_eq!(crate::runtime::lease::snapshot_at(103).active_count, 0);
    }

    #[test]
    fn audio_lease_denies_second_output_owner() {
        let _guard = crate::runtime::lease::lease_test_guard();

        let _session = acquire_audio_lease_at(
            crate::runtime::lease::LeaseKind::AudioOutput,
            AudioLeaseOwner::VoiceSession,
            100,
        )
        .expect("session output lease");

        let error = acquire_audio_lease_at(
            crate::runtime::lease::LeaseKind::AudioOutput,
            AudioLeaseOwner::VoiceOutputTool,
            101,
        )
        .expect_err("voice output tool should be denied");

        assert_eq!(error.stage(), "audio_lease");
        assert!(error.to_string().contains("exclusive_conflict"));
    }

    #[test]
    fn playback_drain_fails_when_speaker_drops_before_queue_drains() {
        let mut states = VecDeque::from([(true, 2048usize), (true, 1024usize), (false, 512usize)]);
        let error = wait_for_playback_drain(
            || states.pop_front().unwrap_or((false, 512)),
            || Ok(false),
            Duration::from_millis(50),
            Duration::from_millis(0),
        )
        .expect_err("speaker drop should surface as an error");
        assert_eq!(error.stage(), "audio_speaker");
        assert!(error.to_string().contains("became unavailable"));
    }

    #[test]
    fn playback_drain_succeeds_after_pending_samples_reach_zero() -> Result<()> {
        let mut states = VecDeque::from([(true, 1024usize), (true, 256usize), (true, 0usize)]);
        wait_for_playback_drain(
            || states.pop_front().unwrap_or((true, 0)),
            || Ok(false),
            Duration::from_millis(50),
            Duration::from_millis(0),
        )
        .map(|_| ())
    }

    #[test]
    fn playback_drain_returns_interrupted_when_local_interrupt_is_consumed() -> Result<()> {
        let mut states = VecDeque::from([(true, 1024usize), (true, 512usize)]);
        let mut interrupts = VecDeque::from([Ok(false), Ok(true)]);
        let interrupted = wait_for_playback_drain(
            || states.pop_front().unwrap_or((true, 512)),
            || interrupts.pop_front().unwrap_or(Ok(false)),
            Duration::from_millis(50),
            Duration::from_millis(0),
        )?;
        assert!(interrupted);
        Ok(())
    }

    #[test]
    fn playback_stream_error_aborts_output_queue() {
        let mut aborts = 0usize;
        let error = abort_playback_on_stream_error(
            Err(crate::error::Error::config("tts_baidu", "stream failed")),
            || aborts += 1,
        )
        .expect_err("stream error should be preserved");

        assert_eq!(error.stage(), "tts_baidu");
        assert_eq!(aborts, 1);
    }
}
