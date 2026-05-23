use super::backend::WakeEvent;
use crate::audio::wake_handoff::WakeAcousticSnapshot;
use crate::config::AudioSegment;
use crate::metrics::WakeWordAcousticFrameMetrics;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::time::Instant;

/// Fixed ESP product wake phrase for the current WakeNet A/B firmware.
pub const ESP_SR_WAKE_PHRASE: &str = "Hi 乐鑫";
/// Fixed ESP-SR WakeNet model for the current WakeNet A/B firmware.
pub const ESP_SR_WAKENET_MODEL: &str = "wn9_hilexin";
/// Fixed ESP-SR AFE WakeNet detection mode used by the current wake profile.
pub const ESP_SR_WAKENET_DETECTION_MODE: &str = "DET_MODE_95";
/// WakeNet model index passed to ESP-SR AFE threshold APIs.
pub const ESP_SR_WAKENET_THRESHOLD_INDEX: i32 = 1;

/// ESP-SR WakeNet threshold profile.
#[derive(Clone, Copy, Debug)]
pub struct EspSrWakeThresholdProfile {
    pub index: i32,
    pub threshold: Option<f32>,
}

/// Current ESP-SR threshold profile.
///
/// This is an explicit ESP-SR WakeNet threshold, not the host acoustic fallback
/// threshold. Keep changes to this value as single-variable live-test profiles.
pub const ESP_SR_WAKENET_THRESHOLD_PROFILE: EspSrWakeThresholdProfile = EspSrWakeThresholdProfile {
    index: ESP_SR_WAKENET_THRESHOLD_INDEX,
    threshold: Some(0.40),
};

const BEETLE_WN_OK: c_int = 0;
const BEETLE_WN_DETECTED: c_int = 1;
const ESP_SR_WAKENET_THRESHOLD_MIN: f32 = 0.4;
const ESP_SR_WAKENET_THRESHOLD_MAX: f32 = 0.9999;

extern "C" {
    fn beetle_wakenet_init(
        model_name: *const c_char,
        input_sample_rate_hz: c_int,
        use_reference: c_int,
    ) -> c_int;
    fn beetle_wakenet_feed(mic: *const i16, reference: *const i16, samples: c_int) -> c_int;
    fn beetle_wakenet_take_event() -> c_int;
    fn beetle_wakenet_set_threshold(index: c_int, threshold: f32) -> c_int;
    fn beetle_wakenet_reset_threshold(index: c_int) -> c_int;
    fn beetle_wakenet_reset();
    fn beetle_wakenet_destroy();
}

/// ESP-SR AFE WakeNet backend. The model id and wake phrase are fixed product constants.
pub struct EspSrWakeBackend {
    model_name: &'static str,
    input_sample_rate_hz: u32,
    use_reference: bool,
    last_snapshot: WakeAcousticSnapshot,
}

impl EspSrWakeBackend {
    pub fn from_audio_config(audio: &AudioSegment) -> Result<Self, &'static str> {
        Self::new(
            audio.microphone.sample_rate.max(8_000),
            crate::config::audio_uses_es7210_codec_input(audio),
        )
    }

    pub fn new(input_sample_rate_hz: u32, use_reference: bool) -> Result<Self, &'static str> {
        if input_sample_rate_hz != 16_000 && input_sample_rate_hz != 24_000 {
            return Err("unsupported_wakenet_input_sample_rate");
        }
        let c_model = CString::new(ESP_SR_WAKENET_MODEL).map_err(|_| "invalid_wakenet_model")?;
        let rc = unsafe {
            beetle_wakenet_init(
                c_model.as_ptr(),
                input_sample_rate_hz as c_int,
                i32::from(use_reference) as c_int,
            )
        };
        if rc != BEETLE_WN_OK {
            log::error!(
                "[wake] ESP-SR AFE WakeNet init failed rc={} model={} phrase={} input={}Hz reference={}",
                rc,
                ESP_SR_WAKENET_MODEL,
                ESP_SR_WAKE_PHRASE,
                input_sample_rate_hz,
                use_reference
            );
            return Err("wakenet_init_failed");
        }
        if let Err(error) = apply_threshold_profile(ESP_SR_WAKENET_THRESHOLD_PROFILE) {
            unsafe { beetle_wakenet_destroy() };
            return Err(error);
        }
        log::info!(
            "[wake] ESP-SR AFE WakeNet init ok model={} phrase={} input={}Hz reference={} mode={} threshold_index={} threshold={}",
            ESP_SR_WAKENET_MODEL,
            ESP_SR_WAKE_PHRASE,
            input_sample_rate_hz,
            use_reference,
            ESP_SR_WAKENET_DETECTION_MODE,
            ESP_SR_WAKENET_THRESHOLD_PROFILE.index,
            threshold_label(ESP_SR_WAKENET_THRESHOLD_PROFILE.threshold)
        );
        Ok(Self {
            model_name: ESP_SR_WAKENET_MODEL,
            input_sample_rate_hz,
            use_reference,
            last_snapshot: WakeAcousticSnapshot::default(),
        })
    }

    pub fn feed_pcm_i16(
        &mut self,
        mic: &[i16],
        reference: &[i16],
        audio_playing: bool,
    ) -> Option<WakeEvent> {
        crate::metrics::record_wake_word_feed_call();
        if mic.is_empty() {
            return None;
        }

        let feed_start = Instant::now();
        let reference_available = self.use_reference && reference.len() >= mic.len();
        let mic_level = pcm_abs_level(mic);
        let zcr = zero_crossing_rate(mic);
        let reference_ok = !self.use_reference || reference_available;
        crate::metrics::record_wake_word_acoustic_frame(WakeWordAcousticFrameMetrics {
            mic_level,
            zcr,
            speech_ratio: 0.0,
            speech_coverage: 0.0,
            speech_dominance: 0.0,
            activation_score: 0.0,
            speech_like: mic_level > 0.0,
            reference_ok,
        });
        self.last_snapshot = WakeAcousticSnapshot {
            mic_level_pm: unit_per_mille(mic_level),
            zcr_pm: unit_per_mille(zcr),
            speech_ratio_pm: 0,
            speech_coverage_pm: 0,
            speech_dominance_pm: 0,
            activation_pm: 0,
            speech_like: mic_level > 0.0,
            reference_ok,
        };

        let reference_ptr = if reference_available {
            reference.as_ptr()
        } else {
            std::ptr::null()
        };
        unsafe {
            beetle_wakenet_feed(mic.as_ptr(), reference_ptr, mic.len() as c_int);
        }
        let detected = unsafe { beetle_wakenet_take_event() == BEETLE_WN_DETECTED };
        crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
        if !detected {
            return None;
        }

        crate::metrics::record_wake_word_feed_detect();
        unsafe { beetle_wakenet_reset() };
        log::info!(
            "[wake] ESP-SR AFE WakeNet triggered model={} phrase={} input={}Hz reference={}",
            self.model_name,
            ESP_SR_WAKE_PHRASE,
            self.input_sample_rate_hz,
            self.use_reference
        );
        Some(if audio_playing {
            WakeEvent::InterruptRequest
        } else {
            WakeEvent::TriggerStart
        })
    }

    pub fn reset_after_session(&mut self) {
        unsafe { beetle_wakenet_reset() };
    }

    pub fn snapshot(&self) -> WakeAcousticSnapshot {
        self.last_snapshot
    }
}

fn apply_threshold_profile(profile: EspSrWakeThresholdProfile) -> Result<(), &'static str> {
    match profile.threshold {
        Some(threshold) => {
            if !threshold.is_finite()
                || !(ESP_SR_WAKENET_THRESHOLD_MIN..=ESP_SR_WAKENET_THRESHOLD_MAX)
                    .contains(&threshold)
            {
                log::error!(
                    "[wake] ESP-SR AFE WakeNet threshold invalid model={} phrase={} mode={} threshold_index={} threshold={:.4} allowed=0.4..0.9999",
                    ESP_SR_WAKENET_MODEL,
                    ESP_SR_WAKE_PHRASE,
                    ESP_SR_WAKENET_DETECTION_MODE,
                    profile.index,
                    threshold
                );
                return Err("wakenet_threshold_invalid");
            }
            let rc = unsafe { beetle_wakenet_set_threshold(profile.index as c_int, threshold) };
            if rc != BEETLE_WN_OK {
                log::error!(
                    "[wake] ESP-SR AFE WakeNet threshold apply failed rc={} model={} phrase={} mode={} threshold_index={} threshold={:.4}",
                    rc,
                    ESP_SR_WAKENET_MODEL,
                    ESP_SR_WAKE_PHRASE,
                    ESP_SR_WAKENET_DETECTION_MODE,
                    profile.index,
                    threshold
                );
                return Err("wakenet_threshold_apply_failed");
            }
            log::info!(
                "[wake] ESP-SR AFE WakeNet threshold apply ok model={} phrase={} mode={} threshold_index={} threshold={:.4}",
                ESP_SR_WAKENET_MODEL,
                ESP_SR_WAKE_PHRASE,
                ESP_SR_WAKENET_DETECTION_MODE,
                profile.index,
                threshold
            );
        }
        None => {
            log::info!(
                "[wake] ESP-SR AFE WakeNet threshold profile model={} phrase={} mode={} threshold_index={} threshold=default source=esp_sr_default",
                ESP_SR_WAKENET_MODEL,
                ESP_SR_WAKE_PHRASE,
                ESP_SR_WAKENET_DETECTION_MODE,
                profile.index
            );
        }
    }
    Ok(())
}

/// Reset the active ESP-SR WakeNet threshold to the model default.
///
/// This is a diagnostic/profile-control hook for controlled live tests. It
/// requires an initialized ESP-SR AFE instance.
pub fn reset_threshold_to_model_default() -> Result<(), &'static str> {
    let rc = unsafe { beetle_wakenet_reset_threshold(ESP_SR_WAKENET_THRESHOLD_INDEX as c_int) };
    if rc != BEETLE_WN_OK {
        log::error!(
            "[wake] ESP-SR AFE WakeNet threshold reset failed rc={} model={} phrase={} mode={} threshold_index={} threshold=default",
            rc,
            ESP_SR_WAKENET_MODEL,
            ESP_SR_WAKE_PHRASE,
            ESP_SR_WAKENET_DETECTION_MODE,
            ESP_SR_WAKENET_THRESHOLD_INDEX
        );
        return Err("wakenet_threshold_reset_failed");
    }
    log::info!(
        "[wake] ESP-SR AFE WakeNet threshold reset ok model={} phrase={} mode={} threshold_index={} threshold=default",
        ESP_SR_WAKENET_MODEL,
        ESP_SR_WAKE_PHRASE,
        ESP_SR_WAKENET_DETECTION_MODE,
        ESP_SR_WAKENET_THRESHOLD_INDEX
    );
    Ok(())
}

fn threshold_label(threshold: Option<f32>) -> String {
    threshold
        .map(|value| format!("{value:.4}"))
        .unwrap_or_else(|| "default".to_string())
}

impl Drop for EspSrWakeBackend {
    fn drop(&mut self) {
        unsafe { beetle_wakenet_destroy() };
    }
}

fn pcm_abs_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let abs_sum: u64 = samples
        .iter()
        .map(|sample| {
            let sample = i32::from(*sample);
            if sample < 0 {
                (-sample) as u64
            } else {
                sample as u64
            }
        })
        .sum();
    (abs_sum as f32 / samples.len() as f32 / 32768.0).clamp(0.0, 1.0)
}

fn zero_crossing_rate(samples: &[i16]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let crossings = samples
        .windows(2)
        .filter(|pair| (pair[0] < 0 && pair[1] >= 0) || (pair[0] >= 0 && pair[1] < 0))
        .count();
    (crossings as f32 / (samples.len() - 1) as f32).clamp(0.0, 1.0)
}

fn unit_per_mille(value: f32) -> u32 {
    (value.clamp(0.0, 1.0) * 1000.0).round() as u32
}
