//! 麦克风采集 + VAD 端点检测，提供 `capture_speech` 供 `VoiceInputTool` 与 `voice_session` 共用。
//! Mic capture with VAD endpointing, shared by `VoiceInputTool` and `voice_session`.

use crate::Platform;
use crate::audio::energy::{EndpointConfig, EndpointEvent, EndpointState};
use crate::config::AudioSegment;
use crate::constants::{AUDIO_CAPTURE_FRAME_SAMPLES, AUDIO_STT_MAX_PCM_BYTES};
use crate::error::{Error, Result};
use std::time::Instant;

/// RAII guard：创建时设 orchestrator 录音标志，Drop 时清除。
/// Ensures wake-word detection is suppressed while mic capture is active.
pub struct AudioRecordingGuard;

impl AudioRecordingGuard {
    pub fn new() -> Self {
        crate::orchestrator::set_audio_recording(true);
        Self
    }
}

impl Default for AudioRecordingGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioRecordingGuard {
    fn drop(&mut self) {
        crate::orchestrator::set_audio_recording(false);
    }
}

/// Capture speech from the microphone using VAD endpointing.
///
/// Returns a PSRAM-backed buffer of PCM i16 samples. The caller is responsible
/// for setting `AudioRecordingGuard` before calling this function to prevent
/// wake-word re-triggers during capture.
///
/// Returns `Err` if no speech is detected within `max_ms`.
pub fn capture_speech(
    platform: &dyn Platform,
    audio_cfg: &AudioSegment,
    max_ms: u32,
    log_tag: &'static str,
) -> Result<crate::platform::psram_vec::PsramVec<i16>> {
    let mic_sr = audio_cfg.microphone.sample_rate.max(8_000);
    let frame_ms =
        ((AUDIO_CAPTURE_FRAME_SAMPLES as u64) * 1000 / (mic_sr as u64)).clamp(1, 40) as u32;
    let endpoint_cfg = EndpointConfig {
        threshold: audio_cfg.vad.threshold,
        silence_duration_ms: audio_cfg.vad.silence_duration_ms,
    };
    let mut endpoint = EndpointState::new();
    let mut frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
    let mut started = false;
    let mut elapsed = 0u32;
    let mut dbg_next_log_ms = 0u32;
    let debug_enabled = log::log_enabled!(log::Level::Debug);
    let max_pcm_samples = max_capture_samples(max_ms, mic_sr);
    let mut captured =
        crate::platform::psram_vec::PsramVec::<i16>::with_max_capacity(max_pcm_samples);
    let capture_start = Instant::now();

    while elapsed < max_ms {
        let n = platform.read_mic_pcm_i16(&mut frame)?;
        if n == 0 {
            elapsed = elapsed.saturating_add(frame_ms);
            continue;
        }
        let chunk = &frame[..n.min(frame.len())];
        if debug_enabled && elapsed >= dbg_next_log_ms {
            let rms = crate::audio::energy::normalized_rms(chunk);
            let (mn, mx) = chunk
                .iter()
                .fold((i16::MAX, i16::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            log::debug!(
                "[{}] t={}ms samples={} rms={:.5} min={} max={} thr={:.3}",
                log_tag,
                elapsed,
                n,
                rms,
                mn,
                mx,
                endpoint_cfg.threshold
            );
            dbg_next_log_ms = elapsed.saturating_add(1000);
        }
        match endpoint.update(chunk, frame_ms, &endpoint_cfg) {
            EndpointEvent::SpeechStart => {
                started = true;
                captured.extend_from_slice(chunk);
            }
            EndpointEvent::SpeechEnd => break,
            EndpointEvent::None => {
                if started {
                    captured.extend_from_slice(chunk);
                }
            }
        }
        if captured.len() * 2 >= AUDIO_STT_MAX_PCM_BYTES {
            break;
        }
        elapsed = elapsed.saturating_add(frame_ms);
    }
    crate::metrics::record_voice_input_capture_ms(capture_start.elapsed().as_millis());

    if captured.is_empty() {
        return Err(Error::config(
            log_tag,
            "no speech captured within time window",
        ));
    }
    Ok(captured)
}

fn max_capture_samples(max_ms: u32, sample_rate: u32) -> usize {
    let requested_samples = (max_ms as usize).saturating_mul(sample_rate as usize) / 1000;
    let max_samples_by_bytes = AUDIO_STT_MAX_PCM_BYTES / std::mem::size_of::<i16>();
    requested_samples.min(max_samples_by_bytes)
}

#[cfg(test)]
mod tests {
    use super::max_capture_samples;

    #[test]
    fn keeps_requested_duration_when_under_byte_limit() {
        assert_eq!(max_capture_samples(12_000, 16_000), 192_000);
    }

    #[test]
    fn clamps_by_pcm_byte_limit_in_sample_units() {
        assert_eq!(max_capture_samples(60_000, 16_000), 480_000);
    }
}
