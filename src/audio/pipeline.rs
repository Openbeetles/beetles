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

pub struct VoicePlaybackStats {
    pub played_samples: usize,
    pub tts_http_ms: u128,
    pub play_ms: u128,
}

pub fn capture_and_transcribe(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    max_ms: u32,
    log_tag: &'static str,
) -> Result<String> {
    let captured = {
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
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    baidu_token: &BaiduTokenCache,
    http: &mut dyn PlatformHttpClient,
    text: &str,
) -> Result<VoicePlaybackStats> {
    let _recording_guard = AudioRecordingGuard::new();
    let tts_start = Instant::now();
    let mut first_pcm_at: Option<Instant> = None;
    crate::orchestrator::set_audio_playing(true);
    let result = (|| match tts_baidu::stream_wav_pcm16le(
        http,
        baidu_token,
        &audio_cfg.speech,
        &audio_cfg.tts,
        text,
        AUDIO_TTS_WRITE_CHUNK_SAMPLES,
        |chunk| {
            if first_pcm_at.is_none() {
                first_pcm_at = Some(Instant::now());
            }
            platform.write_speaker_pcm_i16(chunk)
        },
    ) {
        Ok(samples) => Ok(samples),
        Err(error) if error.stage() == "tts_baidu_wav" => {
            let wav = tts_baidu::synthesize_wav(
                http,
                baidu_token,
                &audio_cfg.speech,
                &audio_cfg.tts,
                text,
            )?;
            tts_baidu::play_wav_pcm16le_chunks(&wav, AUDIO_TTS_WRITE_CHUNK_SAMPLES, |chunk| {
                if first_pcm_at.is_none() {
                    first_pcm_at = Some(Instant::now());
                }
                platform.write_speaker_pcm_i16(chunk)
            })
        }
        Err(error) => Err(error),
    })();
    crate::orchestrator::set_audio_playing(false);
    let played_samples = result?;
    wait_for_platform_playback_drain(platform, audio_cfg.speaker.sample_rate, played_samples)?;
    let total_ms = tts_start.elapsed().as_millis();
    let play_ms = first_pcm_at.map(|at| at.elapsed().as_millis()).unwrap_or(0);
    Ok(VoicePlaybackStats {
        played_samples,
        tts_http_ms: total_ms,
        play_ms,
    })
}

fn wait_for_platform_playback_drain(
    platform: &dyn Platform,
    sample_rate: u32,
    played_samples: usize,
) -> Result<()> {
    if played_samples == 0 {
        return Ok(());
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
        Duration::from_millis(timeout_ms),
        Duration::from_millis(AUDIO_SPEAKER_DRAIN_POLL_MS),
    )
}

fn wait_for_playback_drain(
    mut state: impl FnMut() -> (bool, usize),
    timeout: Duration,
    poll_interval: Duration,
) -> Result<()> {
    let started_at = Instant::now();
    let mut saw_pending_samples = false;
    loop {
        let (ready, pending_samples) = state();
        saw_pending_samples |= pending_samples > 0;
        if pending_samples == 0 {
            if saw_pending_samples && !ready {
                return Err(Error::config(
                    "audio_speaker",
                    "speaker became unavailable during playback",
                ));
            }
            return Ok(());
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use crate::error::Result;

    use super::wait_for_playback_drain;

    #[test]
    fn playback_drain_fails_when_speaker_drops_before_queue_drains() {
        let mut states = VecDeque::from([
            (true, 2048usize),
            (true, 1024usize),
            (false, 512usize),
        ]);
        let error = wait_for_playback_drain(
            || states.pop_front().unwrap_or((false, 512)),
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
            Duration::from_millis(50),
            Duration::from_millis(0),
        )
    }
}
