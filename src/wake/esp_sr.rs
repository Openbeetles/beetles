use super::backend::WakeEvent;
use crate::audio::wake_handoff::WakeAcousticSnapshot;
use crate::config::AudioSegment;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::time::Instant;

/// Fixed ESP product wake phrase.
pub const ESP_SR_WAKE_PHRASE: &str = "Hi,ESP";
/// Fixed ESP-SR WakeNet model for the product wake phrase.
pub const ESP_SR_WAKENET_MODEL_HIESP: &str = "wn9_hiesp";

const BEETLE_WN_OK: c_int = 0;
const BEETLE_WN_DETECTED: c_int = 1;

extern "C" {
    fn beetle_wakenet_init(
        model_name: *const c_char,
        input_sample_rate_hz: c_int,
        use_reference: c_int,
    ) -> c_int;
    fn beetle_wakenet_feed(mic: *const i16, reference: *const i16, samples: c_int) -> c_int;
    fn beetle_wakenet_take_event() -> c_int;
    fn beetle_wakenet_reset();
    fn beetle_wakenet_destroy();
}

/// ESP-SR AFE WakeNet backend. The model id and wake phrase are fixed product constants.
pub struct EspSrWakeBackend {
    model_name: &'static str,
    input_sample_rate_hz: u32,
    use_reference: bool,
}

impl EspSrWakeBackend {
    pub fn from_audio_config(audio: &AudioSegment) -> Result<Self, &'static str> {
        Self::new(
            audio.microphone.sample_rate.max(8_000),
            audio.speaker.enabled,
        )
    }

    pub fn new(input_sample_rate_hz: u32, use_reference: bool) -> Result<Self, &'static str> {
        if input_sample_rate_hz != 16_000 && input_sample_rate_hz != 24_000 {
            return Err("unsupported_wakenet_input_sample_rate");
        }
        let c_model =
            CString::new(ESP_SR_WAKENET_MODEL_HIESP).map_err(|_| "invalid_wakenet_model")?;
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
                ESP_SR_WAKENET_MODEL_HIESP,
                ESP_SR_WAKE_PHRASE,
                input_sample_rate_hz,
                use_reference
            );
            return Err("wakenet_init_failed");
        }
        log::info!(
            "[wake] ESP-SR AFE WakeNet init ok model={} phrase={} input={}Hz reference={}",
            ESP_SR_WAKENET_MODEL_HIESP,
            ESP_SR_WAKE_PHRASE,
            input_sample_rate_hz,
            use_reference
        );
        Ok(Self {
            model_name: ESP_SR_WAKENET_MODEL_HIESP,
            input_sample_rate_hz,
            use_reference,
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
        let reference_ptr = if self.use_reference && reference.len() >= mic.len() {
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
        WakeAcousticSnapshot::default()
    }
}

impl Drop for EspSrWakeBackend {
    fn drop(&mut self) {
        unsafe { beetle_wakenet_destroy() };
    }
}
