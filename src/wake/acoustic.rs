use super::backend::WakeEvent;
use crate::audio::energy::normalized_rms;
use crate::config::AudioSegment;
use std::f32::consts::PI;
use std::time::Instant;

const DEFAULT_NOISE_EMA_ALPHA: f32 = 0.08;
const DEFAULT_NOISE_MULTIPLIER: f32 = 1.8;
const NOISE_BOOTSTRAP_ALPHA: f32 = 0.30;
const MIN_NOISE_FLOOR: f32 = 0.004;
const MAX_NOISE_FLOOR: f32 = 0.35;
const NOISE_BOOTSTRAP_FRAMES: u32 = 12;
const LEVEL_ATTACK_ALPHA: f32 = 0.50;
const LEVEL_RELEASE_ALPHA: f32 = 0.18;
const ACTIVATION_LIMIT: f32 = 1.5;
const KEEPALIVE_GAIN: f32 = 0.5;
const MIN_SPEECH_BIN_COVERAGE: f32 = 0.50;
const KEEPALIVE_MIN_SPEECH_BIN_COVERAGE: f32 = 0.25;
const MAX_SPEECH_BIN_DOMINANCE: f32 = 0.78;
const KEEPALIVE_MAX_SPEECH_BIN_DOMINANCE: f32 = 0.90;
const BAND_ACTIVITY_FLOOR_RATIO: f32 = 0.18;
const SPEECH_BAND_HZ: [f32; 4] = [500.0, 1000.0, 1800.0, 2600.0];
const NOISE_BAND_HZ: [f32; 4] = [150.0, 250.0, 4000.0, 5500.0];

/// Acoustic wake backend configuration derived from `AudioSegment::wake_word`.
#[derive(Clone, Debug)]
pub struct AcousticWakeConfig {
    pub sample_rate_hz: u32,
    pub enter_threshold: f32,
    pub leave_threshold: f32,
    pub reference_suppress_ratio: f32,
    pub zcr_min: f32,
    pub zcr_max: f32,
    pub min_speech_band_ratio: f32,
    pub min_active_ms: u32,
    pub hangover_ms: u32,
    pub cooldown_ms: u32,
}

impl AcousticWakeConfig {
    /// Build acoustic wake parameters from the persisted audio config.
    pub fn from_audio_config(audio: &AudioSegment) -> Self {
        Self {
            sample_rate_hz: audio.microphone.sample_rate.max(8_000),
            enter_threshold: audio.wake_word.enter_threshold,
            leave_threshold: audio.wake_word.leave_threshold,
            reference_suppress_ratio: audio.wake_word.reference_suppress_ratio,
            zcr_min: audio.wake_word.zcr_min,
            zcr_max: audio.wake_word.zcr_max,
            min_speech_band_ratio: audio.wake_word.min_speech_band_ratio,
            min_active_ms: audio.wake_word.min_active_ms,
            hangover_ms: audio.wake_word.hangover_ms,
            cooldown_ms: audio.wake_word.cooldown_ms,
        }
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            sample_rate_hz: 16_000,
            enter_threshold: 0.18,
            leave_threshold: 0.10,
            reference_suppress_ratio: 1.35,
            zcr_min: 0.02,
            zcr_max: 0.25,
            min_speech_band_ratio: 0.45,
            min_active_ms: 240,
            hangover_ms: 500,
            cooldown_ms: 1000,
        }
    }
}

/// Lightweight acoustic wake detector used on the ESP mic hot path.
pub struct AcousticWakeBackend {
    config: AcousticWakeConfig,
    activation_score: f32,
    hangover_left_ms: u32,
    cooldown_left_ms: u32,
    noise_floor: f32,
    noise_bootstrap_frames: u32,
    smoothed_mic_rms: f32,
    smoothed_ref_rms: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct SpeechBandSummary {
    speech_ratio: f32,
    speech_bin_coverage: f32,
    dominant_speech_share: f32,
}

impl AcousticWakeBackend {
    /// Create a new acoustic backend with the provided thresholds and timers.
    pub fn new(config: AcousticWakeConfig) -> Self {
        Self {
            config,
            activation_score: 0.0,
            hangover_left_ms: 0,
            cooldown_left_ms: 0,
            noise_floor: MIN_NOISE_FLOOR,
            noise_bootstrap_frames: 0,
            smoothed_mic_rms: 0.0,
            smoothed_ref_rms: 0.0,
        }
    }

    /// Create the backend directly from the persisted audio config.
    pub fn from_audio_config(audio: &AudioSegment) -> Self {
        Self::new(AcousticWakeConfig::from_audio_config(audio))
    }

    /// Reset session-scoped trigger accumulation after one voice interaction finishes.
    pub fn reset_after_session(&mut self) {
        self.activation_score = 0.0;
        self.hangover_left_ms = 0;
        self.cooldown_left_ms = self.config.cooldown_ms;
    }

    /// Feed one mic frame and optional playback reference frame into the detector.
    pub fn feed_pcm_i16(
        &mut self,
        mic: &[i16],
        reference: &[i16],
        audio_playing: bool,
    ) -> Option<WakeEvent> {
        let feed_start = Instant::now();
        crate::metrics::record_wake_word_feed_call();

        if mic.is_empty() {
            crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
            return None;
        }

        let frame_ms = frame_duration_ms(mic.len(), self.config.sample_rate_hz);
        if self.cooldown_left_ms > 0 {
            self.cooldown_left_ms = self.cooldown_left_ms.saturating_sub(frame_ms);
            crate::metrics::record_wake_word_feed_skip_cooldown();
            crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
            return None;
        }

        let mic_rms = normalized_rms(mic);
        let mic_level = smooth_level(&mut self.smoothed_mic_rms, mic_rms);
        let ref_level = if audio_playing {
            let ref_rms = normalized_rms(reference);
            smooth_level(&mut self.smoothed_ref_rms, ref_rms)
        } else {
            0.0
        };
        let zcr = zero_crossing_rate(mic);
        let speech_summary = speech_band_summary(mic, self.config.sample_rate_hz);
        let dynamic_threshold = self
            .config
            .enter_threshold
            .max(self.noise_floor * DEFAULT_NOISE_MULTIPLIER);
        let reference_ok = !audio_playing
            || ref_level <= self.config.leave_threshold
            || mic_level >= ref_level * self.config.reference_suppress_ratio;

        let speech_like = mic_level >= dynamic_threshold
            && zcr >= self.config.zcr_min
            && zcr <= self.config.zcr_max
            && speech_summary.speech_ratio >= self.config.min_speech_band_ratio
            && speech_summary.speech_bin_coverage >= MIN_SPEECH_BIN_COVERAGE
            && speech_summary.dominant_speech_share <= MAX_SPEECH_BIN_DOMINANCE
            && reference_ok;
        let weak_keepalive = mic_level
            >= self
                .config
                .leave_threshold
                .max(self.noise_floor * (DEFAULT_NOISE_MULTIPLIER * 0.75))
            && zcr >= self.config.zcr_min
            && zcr <= self.config.zcr_max
            && speech_summary.speech_ratio >= self.config.min_speech_band_ratio * 0.8
            && speech_summary.speech_bin_coverage >= KEEPALIVE_MIN_SPEECH_BIN_COVERAGE
            && speech_summary.dominant_speech_share <= KEEPALIVE_MAX_SPEECH_BIN_DOMINANCE
            && reference_ok;
        let activation_gain = (frame_ms as f32) / (self.config.min_active_ms.max(frame_ms) as f32);

        if speech_like {
            self.activation_score = (self.activation_score + activation_gain).min(ACTIVATION_LIMIT);
            self.hangover_left_ms = self.config.hangover_ms;
        } else if self.hangover_left_ms > 0 && weak_keepalive {
            self.activation_score =
                (self.activation_score + activation_gain * KEEPALIVE_GAIN).min(ACTIVATION_LIMIT);
            self.hangover_left_ms = self.hangover_left_ms.saturating_sub(frame_ms);
        } else {
            self.activation_score = decay_activation_score(
                self.activation_score,
                frame_ms,
                self.config.hangover_ms.max(frame_ms),
            );
            self.hangover_left_ms = 0;
            if can_update_noise_floor(
                audio_playing,
                mic_level,
                ref_level,
                self.config.leave_threshold,
            ) {
                update_noise_floor(
                    &mut self.noise_floor,
                    &mut self.noise_bootstrap_frames,
                    mic_level,
                );
            }
        }

        if self.activation_score < 1.0 {
            crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
            return None;
        }

        self.activation_score = 0.0;
        self.hangover_left_ms = 0;
        self.cooldown_left_ms = self.config.cooldown_ms;
        crate::metrics::record_wake_word_feed_detect();
        crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
        Some(if audio_playing {
            WakeEvent::InterruptRequest
        } else {
            WakeEvent::TriggerStart
        })
    }
}

fn frame_duration_ms(sample_count: usize, sample_rate_hz: u32) -> u32 {
    if sample_rate_hz == 0 {
        return 0;
    }
    ((sample_count as u64) * 1000 / (sample_rate_hz as u64)).max(1) as u32
}

fn smooth_level(state: &mut f32, sample: f32) -> f32 {
    let alpha = if sample >= *state {
        LEVEL_ATTACK_ALPHA
    } else {
        LEVEL_RELEASE_ALPHA
    };
    *state = if *state <= f32::EPSILON {
        sample
    } else {
        (*state * (1.0 - alpha)) + (sample * alpha)
    };
    state.clamp(0.0, 1.0)
}

fn decay_activation_score(score: f32, frame_ms: u32, decay_window_ms: u32) -> f32 {
    let decay = (frame_ms as f32) / (decay_window_ms.max(frame_ms) as f32);
    (score - decay).max(0.0)
}

fn can_update_noise_floor(
    audio_playing: bool,
    mic_level: f32,
    ref_level: f32,
    leave_threshold: f32,
) -> bool {
    !audio_playing || ref_level <= leave_threshold || mic_level <= ref_level
}

fn update_noise_floor(noise_floor: &mut f32, bootstrap_frames: &mut u32, mic_level: f32) {
    let alpha = if *bootstrap_frames < NOISE_BOOTSTRAP_FRAMES {
        NOISE_BOOTSTRAP_ALPHA
    } else {
        DEFAULT_NOISE_EMA_ALPHA
    };
    let baseline = noise_floor.max(MIN_NOISE_FLOOR);
    *noise_floor =
        ((1.0 - alpha) * baseline + (alpha * mic_level)).clamp(MIN_NOISE_FLOOR, MAX_NOISE_FLOOR);
    *bootstrap_frames = bootstrap_frames.saturating_add(1);
}

fn zero_crossing_rate(pcm: &[i16]) -> f32 {
    if pcm.len() < 2 {
        return 0.0;
    }
    let zero_crossings = pcm
        .windows(2)
        .filter(|pair| (pair[0] >= 0 && pair[1] < 0) || (pair[0] < 0 && pair[1] >= 0))
        .count();
    (zero_crossings as f32) / ((pcm.len() - 1) as f32)
}

fn speech_band_summary(pcm: &[i16], sample_rate_hz: u32) -> SpeechBandSummary {
    let (speech_bins, speech_bin_count) = band_energies(pcm, sample_rate_hz, &SPEECH_BAND_HZ);
    let speech_total: f32 = speech_bins[..speech_bin_count].iter().copied().sum();
    let noise_total = band_energy_total(pcm, sample_rate_hz, &NOISE_BAND_HZ);
    let total = speech_total + noise_total;
    if speech_bin_count == 0 || total <= f32::EPSILON || speech_total <= f32::EPSILON {
        return SpeechBandSummary::default();
    }

    let dominant = speech_bins[..speech_bin_count]
        .iter()
        .copied()
        .fold(0.0f32, f32::max);
    let activity_floor = (dominant * BAND_ACTIVITY_FLOOR_RATIO).max(f32::EPSILON);
    let active_bins = speech_bins[..speech_bin_count]
        .iter()
        .filter(|energy| **energy >= activity_floor)
        .count();

    SpeechBandSummary {
        speech_ratio: (speech_total / total).clamp(0.0, 1.0),
        speech_bin_coverage: (active_bins as f32) / (speech_bin_count as f32),
        dominant_speech_share: (dominant / speech_total).clamp(0.0, 1.0),
    }
}

fn band_energy_total<const N: usize>(pcm: &[i16], sample_rate_hz: u32, bins: &[f32; N]) -> f32 {
    let nyquist = (sample_rate_hz as f32) * 0.5;
    let mut total = 0.0f32;
    for &hz in bins {
        if hz > 0.0 && hz < nyquist {
            total += goertzel_energy(pcm, sample_rate_hz, hz);
        }
    }
    total
}

fn band_energies<const N: usize>(
    pcm: &[i16],
    sample_rate_hz: u32,
    bins: &[f32; N],
) -> ([f32; N], usize) {
    let nyquist = (sample_rate_hz as f32) * 0.5;
    let mut energies = [0.0; N];
    let mut count = 0;
    for &hz in bins {
        if hz > 0.0 && hz < nyquist {
            energies[count] = goertzel_energy(pcm, sample_rate_hz, hz);
            count += 1;
        }
    }
    (energies, count)
}

fn goertzel_energy(pcm: &[i16], sample_rate_hz: u32, target_hz: f32) -> f32 {
    if pcm.is_empty() || sample_rate_hz == 0 {
        return 0.0;
    }
    let omega = 2.0 * PI * target_hz / (sample_rate_hz as f32);
    let coeff = 2.0 * omega.cos();
    let mut s_prev = 0.0f32;
    let mut s_prev2 = 0.0f32;
    for &sample in pcm {
        let x = sample as f32 / 32768.0;
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    let energy = (s_prev2 * s_prev2) + (s_prev * s_prev) - (coeff * s_prev * s_prev2);
    energy.max(0.0)
}

#[cfg(test)]
fn synth_sine(sample_rate_hz: u32, hz: f32, frames: usize, amplitude: f32) -> Vec<i16> {
    (0..frames)
        .map(|idx| {
            let phase = 2.0 * PI * hz * (idx as f32) / (sample_rate_hz as f32);
            (phase.sin() * amplitude * (i16::MAX as f32)) as i16
        })
        .collect()
}

#[cfg(test)]
fn synth_mix(sample_rate_hz: u32, bins: &[(f32, f32)], frames: usize, amplitude: f32) -> Vec<i16> {
    (0..frames)
        .map(|idx| {
            let sample = bins.iter().fold(0.0f32, |acc, (hz, weight)| {
                let phase = 2.0 * PI * *hz * (idx as f32) / (sample_rate_hz as f32);
                acc + phase.sin() * *weight
            });
            let clamped = (sample * amplitude).clamp(-1.0, 1.0);
            (clamped * (i16::MAX as f32)) as i16
        })
        .collect()
}

#[cfg(test)]
fn synth_voice_like(sample_rate_hz: u32, frames: usize, amplitude: f32) -> Vec<i16> {
    synth_mix(
        sample_rate_hz,
        &[
            (500.0, 0.38),
            (1000.0, 0.30),
            (1800.0, 0.20),
            (2600.0, 0.12),
        ],
        frames,
        amplitude,
    )
}

#[cfg(test)]
fn synth_voice_like_alt(sample_rate_hz: u32, frames: usize, amplitude: f32) -> Vec<i16> {
    synth_mix(
        sample_rate_hz,
        &[(540.0, 0.36), (960.0, 0.28), (1680.0, 0.21), (2480.0, 0.15)],
        frames,
        amplitude,
    )
}

#[cfg(test)]
fn scale_pcm(pcm: &[i16], gain: f32) -> Vec<i16> {
    pcm.iter()
        .map(|sample| ((*sample as f32) * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acoustic_backend_emits_trigger_for_near_end_speech() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        config.cooldown_ms = 200;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let speech = synth_voice_like(sample_rate_hz, 320, 0.55);

        assert_eq!(backend.feed_pcm_i16(&speech, &[], false), None);
        assert_eq!(
            backend.feed_pcm_i16(&speech, &[], false),
            Some(WakeEvent::TriggerStart)
        );
    }

    #[test]
    fn acoustic_backend_rejects_reference_dominated_echo() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let echo = synth_voice_like(sample_rate_hz, 320, 0.35);

        assert_eq!(backend.feed_pcm_i16(&echo, &echo, true), None);
        assert_eq!(backend.feed_pcm_i16(&echo, &echo, true), None);
    }

    #[test]
    fn acoustic_backend_enters_cooldown_after_detection() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        config.cooldown_ms = 120;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let speech = synth_voice_like(sample_rate_hz, 320, 0.55);

        assert_eq!(backend.feed_pcm_i16(&speech, &[], false), None);
        assert_eq!(
            backend.feed_pcm_i16(&speech, &[], false),
            Some(WakeEvent::TriggerStart)
        );
        assert_eq!(backend.feed_pcm_i16(&speech, &[], false), None);
    }

    #[test]
    fn acoustic_backend_requests_interrupt_for_near_end_speech_during_playback() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let mic = synth_voice_like(sample_rate_hz, 320, 0.55);
        let reference = synth_voice_like_alt(sample_rate_hz, 320, 0.25);

        assert_eq!(backend.feed_pcm_i16(&mic, &reference, true), None);
        assert_eq!(
            backend.feed_pcm_i16(&mic, &reference, true),
            Some(WakeEvent::InterruptRequest)
        );
    }

    #[test]
    fn acoustic_backend_hangover_keeps_partial_progress_between_frames() {
        let mut config = AcousticWakeConfig::for_tests();
        config.enter_threshold = 0.20;
        config.min_active_ms = 40;
        config.hangover_ms = 80;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let speech = synth_voice_like(sample_rate_hz, 320, 0.55);
        let quiet = synth_voice_like(sample_rate_hz, 320, 0.10);

        assert_eq!(backend.feed_pcm_i16(&speech, &[], false), None);
        assert_eq!(backend.feed_pcm_i16(&quiet, &[], false), None);
        assert!(backend.activation_score > 0.5);
        assert!(backend.activation_score < 1.0);
        assert!(backend.hangover_left_ms < backend.config.hangover_ms);
        assert_eq!(
            backend.feed_pcm_i16(&speech, &[], false),
            Some(WakeEvent::TriggerStart)
        );
    }

    #[test]
    fn acoustic_backend_keepalive_respects_reference_suppression() {
        let mut config = AcousticWakeConfig::for_tests();
        config.enter_threshold = 0.20;
        config.min_active_ms = 40;
        config.hangover_ms = 80;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let speech = synth_voice_like(sample_rate_hz, 320, 0.55);
        let quiet_mic = synth_voice_like(sample_rate_hz, 320, 0.10);
        let loud_ref = synth_voice_like(sample_rate_hz, 320, 0.35);

        assert_eq!(backend.feed_pcm_i16(&speech, &[], false), None);
        let activation_before = backend.activation_score;
        assert_eq!(backend.feed_pcm_i16(&quiet_mic, &loud_ref, true), None);
        assert!(backend.activation_score < activation_before);
    }

    #[test]
    fn acoustic_backend_rejects_pure_tonal_noise() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let tone = synth_sine(sample_rate_hz, 1000.0, 320, 0.55);

        assert_eq!(backend.feed_pcm_i16(&tone, &[], false), None);
        assert_eq!(backend.feed_pcm_i16(&tone, &[], false), None);
    }

    #[test]
    fn acoustic_backend_rejects_amplified_reference_bleed() {
        let mut config = AcousticWakeConfig::for_tests();
        config.min_active_ms = 40;
        let sample_rate_hz = config.sample_rate_hz;
        let mut backend = AcousticWakeBackend::new(config);
        let reference = synth_voice_like(sample_rate_hz, 320, 0.10);
        let mic = scale_pcm(&reference, 1.45);

        assert_eq!(backend.feed_pcm_i16(&mic, &reference, true), None);
        assert_eq!(backend.feed_pcm_i16(&mic, &reference, true), None);
    }
}
