//! ESP codec backend for `audio.topology = i2s_codec`.
//! Rust owns only a minimal FFI handle; codec graph stays in the local C shim.

#![cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]

use crate::config::{
    AudioSegment, I2cBusConfig, I2sBusConfig, AUDIO_CODEC_INPUT_ES7210, AUDIO_CODEC_OUTPUT_ES8311,
};
use crate::error::{Error, Result};
use crate::platform::audio_drivers::AudioBackend;
use std::ffi::c_int;

const BEETLE_AUDIO_CODEC_OK: i32 = 0;
const BEETLE_AUDIO_CODEC_ERR_INVALID_ARG: i32 = -1;
const BEETLE_AUDIO_CODEC_ERR_NOMEM: i32 = -2;
const BEETLE_AUDIO_CODEC_ERR_ESP: i32 = -3;
const BEETLE_AUDIO_CODEC_ERR_STATE: i32 = -4;

// Beetle config stores codec I2C addresses in the user-facing 7-bit form.
const DEFAULT_ES7210_ADDR: u8 = 0x40;
const DEFAULT_ES8311_ADDR: u8 = 0x18;

#[repr(C)]
struct BeetleAudioCodec {
    _private: [u8; 0],
}

#[repr(C)]
struct BeetleAudioCodecConfig {
    input_sample_rate_hz: c_int,
    output_sample_rate_hz: c_int,
    i2c_sda_pin: c_int,
    i2c_scl_pin: c_int,
    i2c_freq_hz: u32,
    i2s_mclk_pin: c_int,
    i2s_ws_pin: c_int,
    i2s_bclk_pin: c_int,
    i2s_din_pin: c_int,
    i2s_dout_pin: c_int,
    pa_pin: c_int,
    input_addr: u8,
    output_addr: u8,
    input_reference: bool,
    mic_enabled: bool,
    speaker_enabled: bool,
}

unsafe extern "C" {
    fn beetle_audio_codec_create(
        config: *const BeetleAudioCodecConfig,
        out_codec: *mut *mut BeetleAudioCodec,
    ) -> i32;
    fn beetle_audio_codec_destroy(codec: *mut BeetleAudioCodec);
    fn beetle_audio_codec_read_mic_pcm16(
        codec: *mut BeetleAudioCodec,
        out_samples: *mut i16,
        sample_count: usize,
        out_samples_read: *mut usize,
    ) -> i32;
    fn beetle_audio_codec_read_mic_reference_pcm16(
        codec: *mut BeetleAudioCodec,
        out_samples: *mut i16,
        out_reference: *mut i16,
        sample_count: usize,
        out_samples_read: *mut usize,
    ) -> i32;
    fn beetle_audio_codec_write_speaker_pcm16(
        codec: *mut BeetleAudioCodec,
        samples: *const i16,
        sample_count: usize,
    ) -> i32;
    fn beetle_audio_codec_last_esp_err(codec: *const BeetleAudioCodec) -> c_int;
}

fn codec_status_to_error(stage: &'static str, raw: *mut BeetleAudioCodec, status: i32) -> Error {
    match status {
        BEETLE_AUDIO_CODEC_ERR_INVALID_ARG => {
            Error::config(stage, "invalid beetle_audio_codec argument")
        }
        BEETLE_AUDIO_CODEC_ERR_NOMEM => Error::Other {
            source: Box::new(std::io::Error::other("beetle_audio_codec out of memory")),
            stage,
        },
        BEETLE_AUDIO_CODEC_ERR_ESP => {
            let err = unsafe { beetle_audio_codec_last_esp_err(raw) };
            if err != 0 {
                Error::esp(stage, err)
            } else {
                Error::Other {
                    source: Box::new(std::io::Error::other("beetle_audio_codec esp error")),
                    stage,
                }
            }
        }
        BEETLE_AUDIO_CODEC_ERR_STATE => Error::config(stage, "beetle_audio_codec invalid state"),
        other => Error::Other {
            source: Box::new(std::io::Error::other(format!(
                "beetle_audio_codec unexpected status={other}"
            ))),
            stage,
        },
    }
}

fn require_codec_name(field: &'static str, value: Option<&str>, expected: &str) -> Result<()> {
    let actual = value.ok_or_else(|| {
        Error::config(
            "audio_init",
            format!("{field} is required when audio.topology == i2s_codec"),
        )
    })?;
    if actual != expected {
        return Err(Error::config(
            "audio_init",
            format!("{field} currently supports only {expected}"),
        ));
    }
    Ok(())
}

pub(crate) struct CodecAudioBackend {
    raw: *mut BeetleAudioCodec,
    mic_enabled: bool,
    speaker_enabled: bool,
    input_reference: bool,
}

unsafe impl Send for CodecAudioBackend {}

impl CodecAudioBackend {
    pub(crate) fn new(
        seg: &AudioSegment,
        i2c_bus: Option<&I2cBusConfig>,
        i2s_bus: Option<&I2sBusConfig>,
    ) -> Result<Self> {
        let i2c_bus = i2c_bus.ok_or_else(|| {
            Error::config(
                "audio_init",
                "hardware.i2c_bus is required when audio.topology == i2s_codec",
            )
        })?;
        let i2s_bus = i2s_bus.ok_or_else(|| {
            Error::config(
                "audio_init",
                "hardware.i2s_bus is required when audio.topology == i2s_codec",
            )
        })?;
        require_codec_name(
            "audio.codec.input_codec",
            seg.codec.input_codec.as_deref(),
            AUDIO_CODEC_INPUT_ES7210,
        )?;
        require_codec_name(
            "audio.codec.output_codec",
            seg.codec.output_codec.as_deref(),
            AUDIO_CODEC_OUTPUT_ES8311,
        )?;
        let pa_pin = seg.codec.pa_pin.ok_or_else(|| {
            Error::config(
                "audio_init",
                "audio.codec.pa_pin is required when audio.topology == i2s_codec",
            )
        })?;
        if seg.microphone.enabled
            && seg.speaker.enabled
            && seg.microphone.sample_rate != seg.speaker.sample_rate
        {
            return Err(Error::config(
                "audio_init",
                "speaker.sample_rate must equal microphone.sample_rate when audio.topology == i2s_codec",
            ));
        }

        let cfg = BeetleAudioCodecConfig {
            input_sample_rate_hz: seg.microphone.sample_rate as c_int,
            output_sample_rate_hz: seg.speaker.sample_rate as c_int,
            i2c_sda_pin: i2c_bus.sda_pin as c_int,
            i2c_scl_pin: i2c_bus.scl_pin as c_int,
            i2c_freq_hz: i2c_bus.freq_hz,
            i2s_mclk_pin: i2s_bus.mclk_pin as c_int,
            i2s_ws_pin: i2s_bus.ws_pin as c_int,
            i2s_bclk_pin: i2s_bus.bclk_pin as c_int,
            i2s_din_pin: i2s_bus.din_pin as c_int,
            i2s_dout_pin: i2s_bus.dout_pin as c_int,
            pa_pin: pa_pin as c_int,
            input_addr: seg.codec.input_addr.unwrap_or(DEFAULT_ES7210_ADDR),
            output_addr: seg.codec.output_addr.unwrap_or(DEFAULT_ES8311_ADDR),
            input_reference: seg.codec.input_reference,
            mic_enabled: seg.microphone.enabled,
            speaker_enabled: seg.speaker.enabled,
        };

        let mut raw = std::ptr::null_mut();
        let status = unsafe { beetle_audio_codec_create(&cfg, &mut raw) };
        if status != BEETLE_AUDIO_CODEC_OK {
            return Err(codec_status_to_error("audio_init", raw, status));
        }
        if raw.is_null() {
            return Err(Error::Other {
                source: Box::new(std::io::Error::other(
                    "beetle_audio_codec returned null handle",
                )),
                stage: "audio_init",
            });
        }

        Ok(Self {
            raw,
            mic_enabled: seg.microphone.enabled,
            speaker_enabled: seg.speaker.enabled,
            input_reference: seg.codec.input_reference,
        })
    }
}

impl AudioBackend for CodecAudioBackend {
    fn mic_ready(&self) -> bool {
        self.mic_enabled
    }

    fn speaker_ready(&self) -> bool {
        self.speaker_enabled
    }

    fn read_mic_frame_pcm16(&mut self, out: &mut [i16]) -> Result<usize> {
        if !self.mic_enabled {
            return Err(Error::config("audio_mic", "microphone not initialized"));
        }
        let mut samples_read = 0usize;
        let status = unsafe {
            beetle_audio_codec_read_mic_pcm16(
                self.raw,
                out.as_mut_ptr(),
                out.len(),
                &mut samples_read,
            )
        };
        if status == BEETLE_AUDIO_CODEC_OK {
            return Ok(samples_read);
        }
        Err(codec_status_to_error("audio_mic", self.raw, status))
    }

    fn read_mic_reference_frame_pcm16(
        &mut self,
        mic: &mut [i16],
        reference: &mut [i16],
    ) -> Result<(usize, usize)> {
        if !self.input_reference {
            let n = self.read_mic_frame_pcm16(mic)?;
            return Ok((n, 0));
        }
        if !self.mic_enabled {
            return Err(Error::config("audio_mic", "microphone not initialized"));
        }
        if reference.len() < mic.len() {
            return Err(Error::config(
                "audio_reference",
                "reference frame capacity is smaller than mic frame capacity",
            ));
        }
        let mut samples_read = 0usize;
        let status = unsafe {
            beetle_audio_codec_read_mic_reference_pcm16(
                self.raw,
                mic.as_mut_ptr(),
                reference.as_mut_ptr(),
                mic.len(),
                &mut samples_read,
            )
        };
        if status == BEETLE_AUDIO_CODEC_OK {
            return Ok((samples_read, samples_read));
        }
        Err(codec_status_to_error("audio_mic", self.raw, status))
    }

    fn write_speaker_frame_pcm16(&mut self, buf: &[i16]) -> Result<()> {
        if !self.speaker_enabled {
            return Err(Error::config("audio_speaker", "speaker not initialized"));
        }
        let status =
            unsafe { beetle_audio_codec_write_speaker_pcm16(self.raw, buf.as_ptr(), buf.len()) };
        if status == BEETLE_AUDIO_CODEC_OK {
            return Ok(());
        }
        Err(codec_status_to_error("audio_speaker", self.raw, status))
    }
}

impl Drop for CodecAudioBackend {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        unsafe {
            beetle_audio_codec_destroy(self.raw);
        }
        self.raw = std::ptr::null_mut();
    }
}
