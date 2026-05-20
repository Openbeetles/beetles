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
    fn beetle_wakenet_init(model_name: *const c_char, input_sample_rate_hz: c_int) -> c_int;
    fn beetle_wakenet_feed(pcm: *const i16, samples: c_int) -> c_int;
    fn beetle_wakenet_reset();
    fn beetle_wakenet_destroy();
}

/// ESP-SR WakeNet backend. The model id and wake phrase are fixed product constants.
pub struct EspSrWakeBackend {
    model_name: &'static str,
    input_sample_rate_hz: u32,
}

impl EspSrWakeBackend {
    pub fn from_audio_config(audio: &AudioSegment) -> Result<Self, &'static str> {
        Self::new(audio.microphone.sample_rate.max(8_000))
    }

    pub fn new(input_sample_rate_hz: u32) -> Result<Self, &'static str> {
        if input_sample_rate_hz != 16_000 && input_sample_rate_hz != 24_000 {
            return Err("unsupported_wakenet_input_sample_rate");
        }
        let c_model =
            CString::new(ESP_SR_WAKENET_MODEL_HIESP).map_err(|_| "invalid_wakenet_model")?;
        let rc = unsafe { beetle_wakenet_init(c_model.as_ptr(), input_sample_rate_hz as c_int) };
        if rc != BEETLE_WN_OK {
            log::error!(
                "[wake] WakeNet init failed rc={} model={} phrase={} input={}Hz",
                rc,
                ESP_SR_WAKENET_MODEL_HIESP,
                ESP_SR_WAKE_PHRASE,
                input_sample_rate_hz
            );
            return Err("wakenet_init_failed");
        }
        log::info!(
            "[wake] WakeNet init ok model={} phrase={} input={}Hz",
            ESP_SR_WAKENET_MODEL_HIESP,
            ESP_SR_WAKE_PHRASE,
            input_sample_rate_hz
        );
        Ok(Self {
            model_name: ESP_SR_WAKENET_MODEL_HIESP,
            input_sample_rate_hz,
        })
    }

    pub fn feed_pcm_i16(&mut self, mic: &[i16], audio_playing: bool) -> Option<WakeEvent> {
        crate::metrics::record_wake_word_feed_call();
        if mic.is_empty() {
            return None;
        }

        let feed_start = Instant::now();
        let detected =
            unsafe { beetle_wakenet_feed(mic.as_ptr(), mic.len() as c_int) == BEETLE_WN_DETECTED };
        crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());
        if !detected {
            return None;
        }

        crate::metrics::record_wake_word_feed_detect();
        unsafe { beetle_wakenet_reset() };
        log::info!(
            "[wake] WakeNet triggered model={} phrase={} input={}Hz",
            self.model_name,
            ESP_SR_WAKE_PHRASE,
            self.input_sample_rate_hz
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
