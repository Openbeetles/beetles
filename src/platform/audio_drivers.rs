//! ESP32 音频驱动（I2S DMA 真实实现）。
//! Audio driver for ESP32 using I2S DMA via esp-idf-sys new channel API.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::config::AudioSegment;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::error::{Error, Result};
#[cfg(target_arch = "xtensa")]
use crate::platform::heap::{alloc_spiram_buffer, free_spiram_buffer};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::psram_vec::PsramVec;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::runtime::thread_plan;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::util::STACK_AUDIO_IO_STD_COMPAT;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// I2S handle wrapper types
// ---------------------------------------------------------------------------

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
struct MicState {
    rx_handle: esp_idf_svc::sys::i2s_chan_handle_t,
    /// Pre-allocated i32 buffer for I2S 32-bit reads, reused across calls.
    read_buf: PsramVec<i32>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
struct SpeakerState {
    tx_handle: esp_idf_svc::sys::i2s_chan_handle_t,
    sd_pin: Option<i32>,
    /// Pre-allocated i32 buffer for I2S 32-bit writes, reused across calls.
    write_buf: PsramVec<i32>,
}

// SAFETY: Handles are accessed exclusively behind Mutex<Option<AudioPipelineState>>
// in Esp32Platform, guaranteeing single-thread access (same pattern as I2cBusState).
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe impl Send for MicState {}
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe impl Send for SpeakerState {}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const MIC_DEVICE_I2S_INMP441: &str = "i2s_inmp441";
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const SPEAKER_DEVICE_I2S_MAX98357A: &str = "i2s_max98357a";

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// I2S read/write timeout in milliseconds.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const I2S_IO_TIMEOUT_MS: u32 = 1000;

/// FreeRTOS tick period (ms). ESP32 default configTICK_RATE_HZ = 100 → 10ms/tick.
/// portTICK_PERIOD_MS is a C macro not exported by bindgen, so we hardcode.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const PORT_TICK_PERIOD_MS: u32 = 10;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_IDLE_SLEEP_MS_DEEP: u64 = 250;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_SPEAKER_WRITE_MIN_SAMPLES: usize = 320;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_SPEAKER_COALESCE_WAIT_MS: u64 = 12;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_REFERENCE_READ_WAIT_MS: u64 = 4;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_MIC_FRAME_SAMPLES: usize = 320;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_SPEAKER_FRAME_SAMPLES: usize = 1024;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_MIC_I2S_STAGING_SAMPLES: usize = AUDIO_MIC_FRAME_SAMPLES;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_SPEAKER_I2S_STAGING_SAMPLES: usize = AUDIO_SPEAKER_FRAME_SAMPLES;

/// Check ESP-IDF return code; wrap non-OK as `Error::Esp`.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn check_esp(stage: &'static str, ret: i32) -> Result<()> {
    if ret != esp_idf_svc::sys::ESP_OK {
        return Err(Error::esp(stage, ret));
    }
    Ok(())
}

/// Convert a raw 32-bit I2S microphone sample into the 16-bit PCM amplitude
/// expected by the rest of the voice pipeline.
///
/// INMP441-style microphones deliver left-justified PCM inside 32-bit I2S
/// frames on ESP32-S3. The xiaozhi reference path shifts by 12 bits before
/// saturating into i16; matching that gain keeps acoustic trigger input from
/// becoming needlessly attenuated.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn mic_i2s_sample_to_pcm16(raw: i32) -> i16 {
    let value = raw >> 12;
    value.clamp(-(i16::MAX as i32), i16::MAX as i32) as i16
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn read_mic_i2s_pcm16(mic: &mut MicState, out: &mut [i16]) -> Result<usize> {
    if out.is_empty() {
        return Ok(0);
    }
    crate::platform::task_wdt::feed_current_task();
    let sample_count = out.len();
    if sample_count > mic.read_buf.len() {
        return Err(Error::config(
            "audio_mic",
            format!(
                "mic frame {} exceeds staging capacity {}",
                sample_count,
                mic.read_buf.len()
            ),
        ));
    }
    let buf32 = &mut mic.read_buf.as_mut_slice()[..sample_count];
    let byte_len = sample_count * 4;
    let mut bytes_read: usize = 0;
    let timeout_ticks: u32 = I2S_IO_TIMEOUT_MS / PORT_TICK_PERIOD_MS;
    check_esp("i2s_mic_read", unsafe {
        esp_idf_svc::sys::i2s_channel_read(
            mic.rx_handle,
            buf32.as_mut_ptr() as *mut core::ffi::c_void,
            byte_len,
            &mut bytes_read,
            timeout_ticks,
        )
    })?;
    let samples_read = bytes_read / 4;
    for i in 0..samples_read {
        out[i] = mic_i2s_sample_to_pcm16(buf32[i]);
    }
    crate::platform::task_wdt::feed_current_task();
    Ok(samples_read)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn write_speaker_i2s_pcm16(speaker: &mut SpeakerState, buf: &[i16]) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    crate::platform::task_wdt::feed_current_task();
    if buf.len() > speaker.write_buf.len() {
        return Err(Error::config(
            "audio_speaker",
            format!(
                "speaker frame {} exceeds staging capacity {}",
                buf.len(),
                speaker.write_buf.len()
            ),
        ));
    }
    let buf32 = &mut speaker.write_buf.as_mut_slice()[..buf.len()];
    for (i, &s) in buf.iter().enumerate() {
        buf32[i] = (s as i32) << 16;
    }
    let byte_len = buf32.len() * 4;
    let mut bytes_written: usize = 0;
    let timeout_ticks: u32 = I2S_IO_TIMEOUT_MS / PORT_TICK_PERIOD_MS;
    check_esp("i2s_spk_write", unsafe {
        esp_idf_svc::sys::i2s_channel_write(
            speaker.tx_handle,
            buf32.as_ptr() as *const core::ffi::c_void,
            byte_len,
            &mut bytes_written,
            timeout_ticks,
        )
    })?;
    if bytes_written < byte_len {
        log::warn!(
            "[audio] speaker partial write: {}/{} bytes",
            bytes_written,
            byte_len,
        );
    }
    crate::platform::task_wdt::feed_current_task();
    Ok(())
}

// ---------------------------------------------------------------------------
// Channel init helpers
// ---------------------------------------------------------------------------

/// 初始化 INMP441 麦克风 I2S RX 通道（I2S0）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn init_mic_channel(seg: &AudioSegment) -> Result<MicState> {
    use esp_idf_svc::sys::*;

    let mic = &seg.microphone;
    let mut rx_handle: i2s_chan_handle_t = core::ptr::null_mut();

    // -- 1. Allocate channel on I2S port 0, RX only --------------------------
    let chan_cfg = i2s_chan_config_t {
        // IDF 6: `i2s_port_t` is plain `c_int`; peripheral 0 == I2S0.
        id: 0,
        role: i2s_role_t_I2S_ROLE_MASTER,
        dma_desc_num: 6,
        dma_frame_num: 240,
        __bindgen_anon_1: Default::default(),
        // IDF 6 bindgen: `auto_clear_before_cb` + union `__bindgen_anon_1` (TX `auto_clear` / `auto_clear_after_cb`).
        auto_clear_before_cb: true,
        allow_pd: false,
        intr_priority: 0,
    };

    check_esp("i2s_mic_new_channel", unsafe {
        i2s_new_channel(&chan_cfg, core::ptr::null_mut(), &mut rx_handle)
    })?;

    // -- 2. Configure STD mode -----------------------------------------------
    // INMP441 outputs 24-bit data in 32-bit I2S frames; must use 32-bit width.
    let clk_cfg = i2s_std_clk_config_t {
        sample_rate_hz: mic.sample_rate,
        clk_src: soc_periph_i2s_clk_src_t_I2S_CLK_SRC_DEFAULT,
        mclk_multiple: i2s_mclk_multiple_t_I2S_MCLK_MULTIPLE_256,
        ..unsafe { core::mem::zeroed() }
    };

    let slot_cfg = i2s_std_slot_config_t {
        data_bit_width: i2s_data_bit_width_t_I2S_DATA_BIT_WIDTH_32BIT,
        slot_bit_width: i2s_slot_bit_width_t_I2S_SLOT_BIT_WIDTH_AUTO,
        slot_mode: i2s_slot_mode_t_I2S_SLOT_MODE_MONO,
        slot_mask: i2s_std_slot_mask_t_I2S_STD_SLOT_LEFT,
        ws_width: i2s_data_bit_width_t_I2S_DATA_BIT_WIDTH_32BIT,
        ws_pol: false,
        bit_shift: true, // Philips standard
        left_align: true,
        big_endian: false,
        bit_order_lsb: false,
    };

    let gpio_cfg = i2s_std_gpio_config_t {
        mclk: gpio_num_t_GPIO_NUM_NC,
        bclk: mic.pins.sck as gpio_num_t,
        ws: mic.pins.ws as gpio_num_t,
        dout: gpio_num_t_GPIO_NUM_NC,
        din: mic.pins.din as gpio_num_t,
        invert_flags: unsafe { core::mem::zeroed() },
    };

    let std_cfg = i2s_std_config_t {
        clk_cfg,
        slot_cfg,
        gpio_cfg,
    };

    let init_ret = unsafe { i2s_channel_init_std_mode(rx_handle, &std_cfg) };
    if init_ret != ESP_OK {
        unsafe {
            i2s_del_channel(rx_handle);
        }
        return Err(Error::esp("i2s_mic_init_std", init_ret));
    }

    // -- 3. Enable channel ---------------------------------------------------
    let en_ret = unsafe { i2s_channel_enable(rx_handle) };
    if en_ret != ESP_OK {
        unsafe {
            i2s_del_channel(rx_handle);
        }
        return Err(Error::esp("i2s_mic_enable", en_ret));
    }

    log::info!(
        "[audio] mic I2S0 RX ready: {}Hz 32bit-i2s (ws={} sck={} din={})",
        mic.sample_rate,
        mic.pins.ws,
        mic.pins.sck,
        mic.pins.din,
    );
    Ok(MicState {
        rx_handle,
        read_buf: PsramVec::new(AUDIO_MIC_I2S_STAGING_SAMPLES),
    })
}

/// 初始化 MAX98357A 喇叭 I2S TX 通道（I2S1）。
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn init_speaker_channel(seg: &AudioSegment) -> Result<SpeakerState> {
    use esp_idf_svc::sys::*;

    let spk = &seg.speaker;
    let pins = spk.pins.as_ref().ok_or_else(|| {
        Error::config(
            "i2s_spk_init_std",
            "speaker.pins required for ESP I2S speaker",
        )
    })?;
    let sd_pin = pins.sd;

    // -- 0. Enable SD pin (shutdown control for MAX98357A) -------------------
    if let Some(pin) = sd_pin {
        unsafe {
            let _ = gpio_reset_pin(pin as gpio_num_t);
            let conf: gpio_config_t = gpio_config_t {
                pin_bit_mask: 1u64 << pin,
                mode: gpio_mode_t_GPIO_MODE_OUTPUT,
                pull_up_en: gpio_pullup_t_GPIO_PULLUP_DISABLE,
                pull_down_en: gpio_pulldown_t_GPIO_PULLDOWN_DISABLE,
                intr_type: gpio_int_type_t_GPIO_INTR_DISABLE,
                ..core::mem::zeroed()
            };
            check_esp("i2s_spk_sd_gpio_config", gpio_config(&conf))?;
            check_esp("i2s_spk_sd_gpio_set", gpio_set_level(pin as gpio_num_t, 1))?;
        }
    }

    let mut tx_handle: i2s_chan_handle_t = core::ptr::null_mut();

    // -- 1. Allocate channel on I2S port 1, TX only --------------------------
    let chan_cfg = i2s_chan_config_t {
        id: 1,
        role: i2s_role_t_I2S_ROLE_MASTER,
        dma_desc_num: 6,
        dma_frame_num: 240,
        __bindgen_anon_1: Default::default(),
        auto_clear_before_cb: true,
        allow_pd: false,
        intr_priority: 0,
    };

    let new_ret = unsafe { i2s_new_channel(&chan_cfg, &mut tx_handle, core::ptr::null_mut()) };
    if new_ret != ESP_OK {
        // Revert SD pin on failure
        if let Some(pin) = sd_pin {
            unsafe {
                let _ = gpio_set_level(pin as gpio_num_t, 0);
            }
        }
        return Err(Error::esp("i2s_spk_new_channel", new_ret));
    }

    // -- 2. Configure STD mode -----------------------------------------------
    // MAX98357A expects 32-bit I2S frames (matching mic config).
    let clk_cfg = i2s_std_clk_config_t {
        sample_rate_hz: spk.sample_rate,
        clk_src: soc_periph_i2s_clk_src_t_I2S_CLK_SRC_DEFAULT,
        mclk_multiple: i2s_mclk_multiple_t_I2S_MCLK_MULTIPLE_256,
        ..unsafe { core::mem::zeroed() }
    };

    let slot_cfg = i2s_std_slot_config_t {
        data_bit_width: i2s_data_bit_width_t_I2S_DATA_BIT_WIDTH_32BIT,
        slot_bit_width: i2s_slot_bit_width_t_I2S_SLOT_BIT_WIDTH_AUTO,
        slot_mode: i2s_slot_mode_t_I2S_SLOT_MODE_MONO,
        slot_mask: i2s_std_slot_mask_t_I2S_STD_SLOT_LEFT,
        ws_width: i2s_data_bit_width_t_I2S_DATA_BIT_WIDTH_32BIT,
        ws_pol: false,
        bit_shift: true,
        left_align: true,
        big_endian: false,
        bit_order_lsb: false,
    };

    let gpio_cfg = i2s_std_gpio_config_t {
        mclk: gpio_num_t_GPIO_NUM_NC,
        bclk: pins.sck as gpio_num_t,
        ws: pins.ws as gpio_num_t,
        dout: pins.dout as gpio_num_t,
        din: gpio_num_t_GPIO_NUM_NC,
        invert_flags: unsafe { core::mem::zeroed() },
    };

    let std_cfg = i2s_std_config_t {
        clk_cfg,
        slot_cfg,
        gpio_cfg,
    };

    let init_ret = unsafe { i2s_channel_init_std_mode(tx_handle, &std_cfg) };
    if init_ret != ESP_OK {
        unsafe {
            i2s_del_channel(tx_handle);
        }
        if let Some(pin) = sd_pin {
            unsafe {
                let _ = gpio_set_level(pin as gpio_num_t, 0);
            }
        }
        return Err(Error::esp("i2s_spk_init_std", init_ret));
    }

    // -- 3. Enable channel ---------------------------------------------------
    let en_ret = unsafe { i2s_channel_enable(tx_handle) };
    if en_ret != ESP_OK {
        unsafe {
            i2s_del_channel(tx_handle);
        }
        if let Some(pin) = sd_pin {
            unsafe {
                let _ = gpio_set_level(pin as gpio_num_t, 0);
            }
        }
        return Err(Error::esp("i2s_spk_enable", en_ret));
    }

    log::info!(
        "[audio] speaker I2S1 TX ready: {}Hz 32bit-i2s (ws={} sck={} dout={} sd={:?})",
        spk.sample_rate,
        pins.ws,
        pins.sck,
        pins.dout,
        pins.sd,
    );
    Ok(SpeakerState {
        tx_handle,
        sd_pin,
        write_buf: PsramVec::new(AUDIO_SPEAKER_I2S_STAGING_SAMPLES),
    })
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
trait AudioBackend: Send {
    fn mic_ready(&self) -> bool;
    fn speaker_ready(&self) -> bool;
    fn read_mic_frame_pcm16(&mut self, out: &mut [i16]) -> Result<usize>;
    fn write_speaker_frame_pcm16(&mut self, buf: &[i16]) -> Result<()>;
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
struct I2sStdBackend {
    mic: Option<MicState>,
    speaker: Option<SpeakerState>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl AudioBackend for I2sStdBackend {
    fn mic_ready(&self) -> bool {
        self.mic.is_some()
    }
    fn speaker_ready(&self) -> bool {
        self.speaker.is_some()
    }
    fn read_mic_frame_pcm16(&mut self, out: &mut [i16]) -> Result<usize> {
        let mic = self
            .mic
            .as_mut()
            .ok_or_else(|| Error::config("audio_mic", "microphone not initialized"))?;
        read_mic_i2s_pcm16(mic, out)
    }
    fn write_speaker_frame_pcm16(&mut self, buf: &[i16]) -> Result<()> {
        let speaker = self
            .speaker
            .as_mut()
            .ok_or_else(|| Error::config("audio_speaker", "speaker not initialized"))?;
        write_speaker_i2s_pcm16(speaker, buf)
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for I2sStdBackend {
    fn drop(&mut self) {
        if let Some(ref mic) = self.mic {
            unsafe {
                let _ = esp_idf_svc::sys::i2s_channel_disable(mic.rx_handle);
                let _ = esp_idf_svc::sys::i2s_del_channel(mic.rx_handle);
            }
            log::debug!("[audio] mic I2S0 RX channel released");
        }
        if let Some(ref spk) = self.speaker {
            unsafe {
                let _ = esp_idf_svc::sys::i2s_channel_disable(spk.tx_handle);
                let _ = esp_idf_svc::sys::i2s_del_channel(spk.tx_handle);
            }
            if let Some(pin) = spk.sd_pin {
                unsafe {
                    let _ =
                        esp_idf_svc::sys::gpio_set_level(pin as esp_idf_svc::sys::gpio_num_t, 0);
                }
            }
            log::debug!("[audio] speaker I2S1 TX channel released");
        }
    }
}

/// PSRAM-backed circular buffer for audio samples.
/// On xtensa (ESP32-S3) the backing memory is allocated from PSRAM via
/// `heap_caps_malloc(MALLOC_CAP_SPIRAM)`, freeing ~128KB of internal SRAM.
/// Only tiny scratch rings may fall back to standard heap; large rings fail
/// initialization instead of consuming internal SRAM needed by TLS/WiFi/agent.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Default)]
struct AudioRingBuffer {
    buf: *mut i16,
    cap: usize,
    head: usize,  // read position
    len: usize,   // valid sample count
    spiram: bool, // true if buf was allocated from PSRAM
}

// SAFETY: The buffer pointer is exclusively owned and only accessed behind Mutex.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
unsafe impl Send for AudioRingBuffer {}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl AudioRingBuffer {
    fn try_with_capacity(cap: usize) -> Result<Self> {
        if cap == 0 {
            return Ok(Self::default());
        }
        let byte_size = cap * core::mem::size_of::<i16>();

        // Try PSRAM first (xtensa only). Large audio rings must not silently
        // fall back to internal heap because TLS/WiFi share that scarce memory.
        #[cfg(target_arch = "xtensa")]
        {
            if let Some(ptr) = alloc_spiram_buffer(byte_size) {
                // Zero-initialize
                unsafe { core::ptr::write_bytes(ptr, 0, byte_size) };
                log::info!(
                    "[audio] ring buffer {}KB allocated in PSRAM",
                    byte_size / 1024
                );
                return Ok(Self {
                    buf: ptr as *mut i16,
                    cap,
                    head: 0,
                    len: 0,
                    spiram: true,
                });
            }
        }

        if !crate::audio::runtime_policy::allow_internal_audio_ring_fallback(byte_size) {
            return Err(Error::config(
                "audio_init",
                format!(
                    "PSRAM unavailable for {}KB audio ring buffer; refusing internal heap fallback",
                    byte_size / 1024
                ),
            ));
        }

        // Tiny fallback: standard heap for small scratch rings only.
        let mut v = Vec::new();
        v.try_reserve_exact(cap).map_err(|e| {
            Error::config(
                "audio_init",
                format!("audio ring buffer internal heap allocation failed: {}", e),
            )
        })?;
        v.resize(cap, 0);
        let ptr = v.as_mut_ptr();
        core::mem::forget(v); // ownership transferred to raw pointer
        log::info!(
            "[audio] ring buffer {}KB allocated in internal heap (PSRAM unavailable)",
            byte_size / 1024
        );
        Ok(Self {
            buf: ptr,
            cap,
            head: 0,
            len: 0,
            spiram: false,
        })
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn capacity(&self) -> usize {
        self.cap
    }

    #[inline]
    fn available(&self) -> usize {
        self.cap.saturating_sub(self.len)
    }

    /// Write position (one past the last valid sample, wrapping).
    #[inline]
    fn tail(&self) -> usize {
        let t = self.head + self.len;
        if t >= self.cap {
            t - self.cap
        } else {
            t
        }
    }

    fn push_slice_drop_oldest(&mut self, input: &[i16]) {
        if self.cap == 0 {
            return;
        }
        for &sample in input {
            let t = self.tail();
            unsafe { *self.buf.add(t) = sample };
            if self.len == self.cap {
                // Buffer full — overwrite oldest, advance head
                self.head += 1;
                if self.head == self.cap {
                    self.head = 0;
                }
            } else {
                self.len += 1;
            }
        }
    }

    fn push_slice_blocking(&mut self, input: &[i16]) -> usize {
        let take = input.len().min(self.available());
        for &sample in &input[..take] {
            let t = self.tail();
            unsafe { *self.buf.add(t) = sample };
            self.len += 1;
        }
        take
    }

    fn pop_into(&mut self, out: &mut [i16]) -> usize {
        let n = out.len().min(self.len);
        for slot in out.iter_mut().take(n) {
            unsafe { *slot = *self.buf.add(self.head) };
            self.head += 1;
            if self.head == self.cap {
                self.head = 0;
            }
        }
        self.len -= n;
        n
    }

    fn copy_recent_into(&self, out: &mut [i16]) -> usize {
        let n = out.len().min(self.len);
        if n == 0 {
            return 0;
        }
        let tail = self.tail();
        let start = if tail >= n {
            tail - n
        } else {
            self.cap + tail - n
        };
        for (idx, slot) in out.iter_mut().take(n).enumerate() {
            let pos = (start + idx) % self.cap;
            unsafe { *slot = *self.buf.add(pos) };
        }
        n
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for AudioRingBuffer {
    fn drop(&mut self) {
        if self.buf.is_null() {
            return;
        }
        if self.spiram {
            #[cfg(target_arch = "xtensa")]
            unsafe {
                free_spiram_buffer(self.buf as *mut u8);
            }
        } else {
            // Reconstruct Vec to free via standard allocator
            unsafe {
                let _ = Vec::from_raw_parts(self.buf, self.cap, self.cap);
            }
        }
        self.buf = core::ptr::null_mut();
    }
}

/// Staging ring buffer capacity in seconds of audio.
/// 4 seconds @ 16kHz = 64000 samples = ~128KB in PSRAM.
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
const AUDIO_STAGING_CAPACITY_SECS: usize = 4;

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
/// Audio owner contract for the ESP software rings.
///
/// - StartCapture / StartPlayback are represented by orchestrator audio flags.
/// - Drain is observed through speaker/staging buffered sample counters.
/// - Stop is `request_stop`, which wakes every blocked reader/writer.
/// - Abort / ClearQueues is `clear_output_queues`.
/// - DoneWrite / UnblockReader are the condition-variable notifications emitted after writes,
///   clears, and stop.
struct SharedAudioBuffers {
    mic: Mutex<AudioRingBuffer>,
    mic_cv: Condvar,
    speaker: Mutex<AudioRingBuffer>,
    speaker_cv: Condvar,
    staging: Mutex<AudioRingBuffer>,
    staging_cv: Condvar,
    reference: Mutex<AudioRingBuffer>,
    reference_cv: Condvar,
    speaker_generation: AtomicU32,
    stop: AtomicBool,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
#[derive(Clone, Copy)]
struct AudioQueueCapacities {
    mic: usize,
    speaker: usize,
    staging: usize,
    reference: usize,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl SharedAudioBuffers {
    fn is_stopping(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }

    fn capacities(&self) -> AudioQueueCapacities {
        AudioQueueCapacities {
            mic: self
                .mic
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capacity(),
            speaker: self
                .speaker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capacity(),
            staging: self
                .staging
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capacity(),
            reference: self
                .reference
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .capacity(),
        }
    }

    fn notify_all_waiters(&self) {
        self.mic_cv.notify_all();
        self.speaker_cv.notify_all();
        self.staging_cv.notify_all();
        self.reference_cv.notify_all();
    }

    fn request_stop(&self) {
        self.stop.store(true, Ordering::Release);
        self.notify_all_waiters();
    }

    fn clear_output_queues(&self) {
        let mut staging_guard = self.staging.lock().unwrap_or_else(|e| e.into_inner());
        staging_guard.clear();
        drop(staging_guard);

        let mut speaker_guard = self.speaker.lock().unwrap_or_else(|e| e.into_inner());
        speaker_guard.clear();
        self.speaker_generation.fetch_add(1, Ordering::Relaxed);
        drop(speaker_guard);

        let mut reference_guard = self.reference.lock().unwrap_or_else(|e| e.into_inner());
        reference_guard.clear();
        drop(reference_guard);

        crate::metrics::record_audio_speaker_queue_depth_last_samples(0);
        crate::metrics::record_audio_reference_queue_depth_last_samples(0);
        self.notify_all_waiters();
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn pop_speaker_frame_for_output(
    shared: &SharedAudioBuffers,
    out: &mut [i16],
    min_samples: usize,
    coalesce_wait: Duration,
) -> Option<(usize, u32)> {
    let mut guard = shared.speaker.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = Instant::now() + coalesce_wait;
    loop {
        let available = guard.len();
        if available == 0 {
            if shared.is_stopping() {
                return None;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let waited = shared
                .speaker_cv
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|e| e.into_inner());
            guard = waited.0;
            if waited.1.timed_out() && guard.len() == 0 {
                return None;
            }
            continue;
        }
        if available < min_samples && !shared.is_stopping() && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                let waited = shared
                    .speaker_cv
                    .wait_timeout(guard, remaining)
                    .unwrap_or_else(|e| e.into_inner());
                guard = waited.0;
                continue;
            }
        }
        let n = guard.pop_into(out);
        if n == 0 {
            return None;
        }
        let generation = shared.speaker_generation.load(Ordering::Relaxed);
        crate::metrics::record_audio_speaker_queue_depth_last_samples(guard.len());
        shared.speaker_cv.notify_one();
        return Some((n, generation));
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn wait_for_speaker_work_or_stop(shared: &SharedAudioBuffers, timeout: Duration) {
    // Also check staging: if staging has data, the worker should wake to transfer it.
    {
        let sg = shared.staging.lock().unwrap_or_else(|e| e.into_inner());
        if sg.len() > 0 {
            return;
        }
    }
    let guard = shared.speaker.lock().unwrap_or_else(|e| e.into_inner());
    if guard.len() > 0 || shared.is_stopping() {
        return;
    }
    let _ = shared
        .speaker_cv
        .wait_timeout(guard, timeout)
        .unwrap_or_else(|e| e.into_inner());
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn current_speaker_buffered_samples(shared: &SharedAudioBuffers) -> usize {
    shared
        .speaker
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len()
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn push_playback_reference_frame(shared: &SharedAudioBuffers, samples: &[i16]) {
    if samples.is_empty() {
        return;
    }
    let mut guard = shared.reference.lock().unwrap_or_else(|e| e.into_inner());
    guard.push_slice_drop_oldest(samples);
    crate::metrics::record_audio_reference_queue_depth_last_samples(guard.len());
    shared.reference_cv.notify_one();
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn update_speaker_playback_metrics(
    shared: &SharedAudioBuffers,
    audio_playing: bool,
    prev_audio_playing: &mut bool,
    saw_buffered_audio: &mut bool,
    underrun_reported: &mut bool,
) {
    let depth = current_speaker_buffered_samples(shared);
    crate::metrics::record_audio_speaker_queue_depth_last_samples(depth);

    if !audio_playing {
        *prev_audio_playing = false;
        *saw_buffered_audio = false;
        *underrun_reported = false;
        return;
    }

    if !*prev_audio_playing {
        crate::metrics::reset_audio_speaker_queue_depth_min_samples();
        *saw_buffered_audio = false;
        *underrun_reported = false;
    }

    if depth > 0 {
        *saw_buffered_audio = true;
        *underrun_reported = false;
        crate::metrics::record_audio_speaker_queue_depth_min_candidate(depth);
    } else if *saw_buffered_audio {
        crate::metrics::record_audio_speaker_queue_depth_min_candidate(0);
        if !*underrun_reported {
            crate::metrics::record_audio_speaker_underrun();
            *underrun_reported = true;
        }
    }

    *prev_audio_playing = true;
}

fn should_read_mic_frame(
    audio_recording: bool,
    audio_playing: bool,
    wake_backend_requires_pcm_feed: bool,
    interrupt_listening: bool,
) -> bool {
    if audio_playing {
        return interrupt_listening && wake_backend_requires_pcm_feed;
    }
    audio_recording || wake_backend_requires_pcm_feed
}

fn should_feed_wake_backend(
    audio_recording: bool,
    interrupt_listening: bool,
    wake_backend_requires_pcm_feed: bool,
) -> bool {
    wake_backend_requires_pcm_feed && (!audio_recording || interrupt_listening)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub(crate) struct AudioPipelineState {
    mic_enabled: bool,
    speaker_enabled: bool,
    shared: Arc<SharedAudioBuffers>,
    worker: Option<crate::util::TaskHandle>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn validate_speaker_for_pipeline(seg: &AudioSegment) -> Result<()> {
    if seg.speaker.enabled && seg.speaker.device_type != SPEAKER_DEVICE_I2S_MAX98357A {
        return Err(Error::config(
            "audio_init",
            format!(
                "unsupported speaker device_type '{}', only {} is supported now",
                seg.speaker.device_type, SPEAKER_DEVICE_I2S_MAX98357A
            ),
        ));
    }
    Ok(())
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl AudioPipelineState {
    /// 仅应在存在真实 microphone/speaker 端点时由 `Esp32Platform::init_audio` 调用。
    /// Call only when a real microphone/speaker endpoint exists.
    pub fn from_config(seg: &AudioSegment) -> Result<Self> {
        if !seg.enabled {
            return Err(Error::config(
                "audio_init",
                "AudioPipelineState::from_config requires audio.enabled == true",
            ));
        }
        if !crate::config::audio_runtime_pipeline_enabled(seg) {
            return Err(Error::config(
                "audio_init",
                "AudioPipelineState::from_config requires microphone or speaker endpoint",
            ));
        }
        if seg.microphone.enabled && seg.microphone.bits_per_sample != 16 {
            return Err(Error::config(
                "audio_init",
                format!(
                    "only 16-bit microphone sampling is supported (got {})",
                    seg.microphone.bits_per_sample
                ),
            ));
        }
        if seg.speaker.enabled && seg.speaker.bits_per_sample != 16 {
            return Err(Error::config(
                "audio_init",
                format!(
                    "only 16-bit speaker output is supported (got {})",
                    seg.speaker.bits_per_sample
                ),
            ));
        }
        validate_speaker_for_pipeline(seg)?;

        const MIC_DEVICE_PDM: &str = "pdm";
        if seg.microphone.enabled && seg.microphone.device_type == MIC_DEVICE_PDM {
            return Err(Error::config(
                "audio_init",
                format!(
                    "microphone device_type '{}' is not supported (PDM not implemented); use {}",
                    MIC_DEVICE_PDM, MIC_DEVICE_I2S_INMP441
                ),
            ));
        }
        if seg.microphone.enabled && seg.microphone.device_type != MIC_DEVICE_I2S_INMP441 {
            return Err(Error::config(
                "audio_init",
                format!(
                    "unsupported microphone device_type '{}', only {} is supported",
                    seg.microphone.device_type, MIC_DEVICE_I2S_INMP441
                ),
            ));
        }

        let mic_cap = if seg.microphone.enabled {
            (seg.microphone.sample_rate.max(8_000) as usize).saturating_mul(2)
        } else {
            0
        };
        let speaker_cap = if seg.speaker.enabled {
            (seg.speaker.sample_rate.max(8_000) as usize).saturating_mul(2)
        } else {
            0
        };
        let staging_cap = if seg.speaker.enabled {
            (seg.speaker.sample_rate.max(8_000) as usize)
                .saturating_mul(AUDIO_STAGING_CAPACITY_SECS)
        } else {
            0
        };
        let reference_cap = if seg.microphone.enabled && seg.speaker.enabled {
            speaker_cap
        } else {
            0
        };
        let shared = Arc::new(SharedAudioBuffers {
            mic: Mutex::new(AudioRingBuffer::try_with_capacity(mic_cap)?),
            mic_cv: Condvar::new(),
            speaker: Mutex::new(AudioRingBuffer::try_with_capacity(speaker_cap)?),
            speaker_cv: Condvar::new(),
            staging: Mutex::new(AudioRingBuffer::try_with_capacity(staging_cap)?),
            staging_cv: Condvar::new(),
            reference: Mutex::new(AudioRingBuffer::try_with_capacity(reference_cap)?),
            reference_cv: Condvar::new(),
            speaker_generation: AtomicU32::new(1),
            stop: AtomicBool::new(false),
        });

        let mic = if seg.microphone.enabled {
            Some(init_mic_channel(seg)?)
        } else {
            None
        };
        let speaker = if seg.speaker.enabled {
            match init_speaker_channel(seg) {
                Ok(s) => Some(s),
                Err(e) => {
                    drop(mic);
                    return Err(e);
                }
            }
        } else {
            None
        };
        let mut backend: Box<dyn AudioBackend> = Box::new(I2sStdBackend { mic, speaker });

        let mic_enabled = backend.mic_ready();
        let speaker_enabled = backend.speaker_ready();
        let worker_shared = Arc::clone(&shared);
        let worker_plan = thread_plan("audio_io_worker");
        let worker_surface =
            crate::platform::task_affinity::planned_spawn_surface("audio_io_worker");
        let capacities = shared.capacities();
        log::info!(
            "[audio] worker starting name=audio_io_worker surface={:?} stack={} mic={} speaker={} reference={} cap_mic={} cap_speaker={} cap_staging={} cap_reference={}",
            worker_surface,
            STACK_AUDIO_IO_STD_COMPAT,
            mic_enabled,
            speaker_enabled,
            mic_enabled && speaker_enabled,
            capacities.mic,
            capacities.speaker,
            capacities.staging,
            capacities.reference,
        );
        let worker = crate::util::spawn_guarded_with_profile_handle(
            "audio_io_worker",
            STACK_AUDIO_IO_STD_COMPAT,
            worker_plan.core,
            worker_plan.role,
            move || {
                let mut mic_frame = vec![0i16; AUDIO_MIC_FRAME_SAMPLES];
                let mut reference_frame = vec![0i16; AUDIO_MIC_FRAME_SAMPLES];
                let mut speaker_frame = vec![0i16; AUDIO_SPEAKER_FRAME_SAMPLES];
                let speaker_min_samples = AUDIO_SPEAKER_WRITE_MIN_SAMPLES.min(speaker_frame.len());
                let mut prev_audio_playing = false;
                let mut speaker_saw_buffered_audio = false;
                let mut speaker_underrun_reported = false;
                loop {
                    crate::platform::task_wdt::feed_current_task();
                    let loop_start = Instant::now();
                    crate::metrics::record_audio_worker_turn();
                    if worker_shared.is_stopping() {
                        break;
                    }
                    let mut progressed = false;
                    let audio_playing = crate::orchestrator::is_audio_playing();
                    let audio_recording = crate::orchestrator::is_audio_recording();
                    let interrupt_listening = crate::orchestrator::is_audio_interrupt_listening();
                    let wake_backend_requires_pcm_feed = crate::wake::requires_pcm_feed();
                    let mic_read_needed = should_read_mic_frame(
                        audio_recording,
                        audio_playing,
                        wake_backend_requires_pcm_feed,
                        interrupt_listening,
                    );

                    if backend.speaker_ready() {
                        // --- staging → speaker transfer ---
                        // Pull data from staging into the speaker ring buffer whenever
                        // staging has data — including the pre-playback buffering phase.
                        // This decouples WSS data arrival from I2S consumption.
                        {
                            let staging_popped = {
                                let mut sg = worker_shared
                                    .staging
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                sg.pop_into(&mut speaker_frame)
                            };
                            if staging_popped > 0 {
                                let mut spk = worker_shared
                                    .speaker
                                    .lock()
                                    .unwrap_or_else(|e| e.into_inner());
                                let pushed =
                                    spk.push_slice_blocking(&speaker_frame[..staging_popped]);
                                if pushed > 0 {
                                    crate::metrics::record_audio_speaker_queue_depth_last_samples(
                                        spk.len(),
                                    );
                                    worker_shared.speaker_cv.notify_one();
                                }
                            }
                        }

                        if let Some((n, generation)) = pop_speaker_frame_for_output(
                            worker_shared.as_ref(),
                            &mut speaker_frame,
                            speaker_min_samples,
                            Duration::from_millis(AUDIO_SPEAKER_COALESCE_WAIT_MS),
                        ) {
                            if generation
                                != worker_shared.speaker_generation.load(Ordering::Relaxed)
                            {
                                crate::metrics::record_audio_speaker_queue_depth_last_samples(
                                    current_speaker_buffered_samples(worker_shared.as_ref()),
                                );
                                continue;
                            }
                            crate::platform::task_wdt::feed_current_task();
                            let speaker_write_start = Instant::now();
                            if let Err(e) = backend.write_speaker_frame_pcm16(&speaker_frame[..n]) {
                                log::warn!("[audio] speaker frame write failed: {}", e);
                            } else {
                                crate::metrics::record_audio_speaker_write_us(
                                    speaker_write_start.elapsed().as_micros(),
                                );
                                push_playback_reference_frame(
                                    worker_shared.as_ref(),
                                    &speaker_frame[..n],
                                );
                                progressed = true;
                            }
                        }
                    }

                    if backend.speaker_ready() {
                        update_speaker_playback_metrics(
                            worker_shared.as_ref(),
                            audio_playing,
                            &mut prev_audio_playing,
                            &mut speaker_saw_buffered_audio,
                            &mut speaker_underrun_reported,
                        );
                    } else {
                        prev_audio_playing = false;
                        speaker_saw_buffered_audio = false;
                        speaker_underrun_reported = false;
                    }

                    if backend.mic_ready() && mic_read_needed {
                        crate::metrics::record_audio_mic_poll_turn();
                        crate::platform::task_wdt::feed_current_task();
                        let mic_read_start = Instant::now();
                        match backend.read_mic_frame_pcm16(&mut mic_frame) {
                            Ok(n) if n > 0 => {
                                crate::metrics::record_audio_mic_read_us(
                                    mic_read_start.elapsed().as_micros(),
                                );
                                crate::metrics::record_audio_mic_frame_read();
                                if should_feed_wake_backend(
                                    audio_recording,
                                    interrupt_listening,
                                    wake_backend_requires_pcm_feed,
                                ) {
                                    let reference_copied = {
                                        let guard = worker_shared
                                            .reference
                                            .lock()
                                            .unwrap_or_else(|e| e.into_inner());
                                        guard.copy_recent_into(&mut reference_frame[..n])
                                    };
                                    if reference_copied > 0 {
                                        crate::metrics::record_audio_reference_frame_read();
                                    } else {
                                        crate::metrics::record_audio_reference_zero_read();
                                    }
                                    if reference_copied < n {
                                        reference_frame[reference_copied..n].fill(0);
                                    }
                                    crate::wake::feed_pcm_i16(
                                        &mic_frame[..n],
                                        &reference_frame[..n],
                                        audio_playing,
                                    );
                                }

                                if audio_recording {
                                    let mut guard =
                                        worker_shared.mic.lock().unwrap_or_else(|e| e.into_inner());
                                    guard.push_slice_drop_oldest(&mic_frame[..n]);
                                    worker_shared.mic_cv.notify_one();
                                }
                                progressed = true;
                            }
                            Ok(_) => {
                                crate::metrics::record_audio_mic_read_us(
                                    mic_read_start.elapsed().as_micros(),
                                );
                                crate::metrics::record_audio_mic_zero_read();
                            }
                            Err(e) => {
                                crate::metrics::record_audio_mic_read_us(
                                    mic_read_start.elapsed().as_micros(),
                                );
                                log::debug!("[audio] mic read frame failed: {}", e);
                            }
                        }
                    }
                    if !progressed {
                        crate::metrics::record_audio_worker_idle_turn();
                        crate::platform::task_wdt::feed_current_task();
                        let idle_sleep_ms = if mic_read_needed {
                            2
                        } else {
                            AUDIO_IDLE_SLEEP_MS_DEEP
                        };
                        if backend.speaker_ready() && (!backend.mic_ready() || !mic_read_needed) {
                            wait_for_speaker_work_or_stop(
                                worker_shared.as_ref(),
                                Duration::from_millis(idle_sleep_ms),
                            );
                        } else {
                            std::thread::sleep(Duration::from_millis(idle_sleep_ms));
                        }
                    }
                    crate::metrics::record_audio_loop_us(loop_start.elapsed().as_micros());
                    crate::platform::task_wdt::feed_current_task();
                }
                log::info!("[audio] worker stopped name=audio_io_worker");
            },
        )
        .map_err(|e| Error::config("audio_init", format!("spawn audio worker failed: {}", e)))?;

        Ok(Self {
            mic_enabled,
            speaker_enabled,
            shared,
            worker: Some(worker),
        })
    }

    #[inline]
    pub fn reference_ready(&self) -> bool {
        self.mic_enabled && self.speaker_enabled
    }

    #[inline]
    pub fn duplex_capabilities(&self) -> crate::platform::AudioDuplexCapabilities {
        match (self.mic_enabled, self.speaker_enabled) {
            (true, true) => {
                crate::platform::AudioDuplexCapabilities::duplex_with_playback_reference()
            }
            (true, false) => crate::platform::AudioDuplexCapabilities::microphone_only(),
            (false, true) => crate::platform::AudioDuplexCapabilities::speaker_only(),
            (false, false) => crate::platform::AudioDuplexCapabilities::unavailable(),
        }
    }

    pub fn speaker_buffered_samples(&self) -> usize {
        current_speaker_buffered_samples(self.shared.as_ref())
    }

    pub fn clear_speaker_buffer(&self) -> Result<()> {
        if !self.speaker_enabled {
            return Err(Error::config("audio_speaker", "speaker not initialized"));
        }
        self.shared.clear_output_queues();
        Ok(())
    }

    pub fn read_mic_pcm_i16(&self, out: &mut [i16]) -> Result<usize> {
        if !self.mic_enabled {
            return Err(Error::config("audio_mic", "microphone not initialized"));
        }
        if out.is_empty() {
            return Ok(0);
        }
        let mut guard = self.shared.mic.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() == 0 {
            let waited = self
                .shared
                .mic_cv
                .wait_timeout(guard, Duration::from_millis(I2S_IO_TIMEOUT_MS as u64))
                .unwrap_or_else(|e| e.into_inner());
            guard = waited.0;
        }
        if self.shared.is_stopping() && guard.len() == 0 {
            return Ok(0);
        }
        let n = guard.pop_into(out);
        Ok(n)
    }

    pub fn read_playback_reference_pcm_i16(&self, out: &mut [i16]) -> Result<usize> {
        if !self.reference_ready() {
            return Err(Error::config(
                "audio_reference",
                "playback reference not initialized",
            ));
        }
        if out.is_empty() {
            return Ok(0);
        }
        let mut guard = self
            .shared
            .reference
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.len() == 0 {
            let waited = self
                .shared
                .reference_cv
                .wait_timeout(guard, Duration::from_millis(AUDIO_REFERENCE_READ_WAIT_MS))
                .unwrap_or_else(|e| e.into_inner());
            guard = waited.0;
        }
        if self.shared.is_stopping() && guard.len() == 0 {
            return Ok(0);
        }
        let n = guard.pop_into(out);
        crate::metrics::record_audio_reference_queue_depth_last_samples(guard.len());
        if n > 0 {
            crate::metrics::record_audio_reference_frame_read();
        } else {
            crate::metrics::record_audio_reference_zero_read();
        }
        Ok(n)
    }

    pub fn write_speaker_pcm_i16(&self, buf: &[i16]) -> Result<()> {
        if !self.speaker_enabled {
            return Err(Error::config("audio_speaker", "speaker not initialized"));
        }
        if buf.is_empty() {
            return Ok(());
        }
        let mut written = 0usize;
        while written < buf.len() {
            if self.shared.is_stopping() {
                return Err(Error::config("audio_speaker", "audio owner stopped"));
            }
            let mut guard = self
                .shared
                .speaker
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            while guard.available() == 0 && !self.shared.is_stopping() {
                let waited = self
                    .shared
                    .speaker_cv
                    .wait_timeout(guard, Duration::from_millis(I2S_IO_TIMEOUT_MS as u64))
                    .unwrap_or_else(|e| e.into_inner());
                guard = waited.0;
                if waited.1.timed_out() && guard.available() == 0 {
                    return Err(Error::config(
                        "audio_speaker",
                        "speaker ring buffer blocked",
                    ));
                }
            }
            if self.shared.is_stopping() {
                return Err(Error::config("audio_speaker", "audio owner stopped"));
            }
            let n = guard.push_slice_blocking(&buf[written..]);
            written += n;
            crate::metrics::record_audio_speaker_queue_depth_last_samples(guard.len());
            self.shared.speaker_cv.notify_one();
        }
        Ok(())
    }

    pub fn try_write_speaker_pcm_i16(&self, buf: &[i16]) -> Result<usize> {
        if !self.speaker_enabled {
            return Err(Error::config("audio_speaker", "speaker not initialized"));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.shared.is_stopping() {
            return Err(Error::config("audio_speaker", "audio owner stopped"));
        }
        let mut guard = self
            .shared
            .speaker
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let written = guard.push_slice_blocking(buf);
        crate::metrics::record_audio_speaker_queue_depth_last_samples(guard.len());
        if written > 0 {
            self.shared.speaker_cv.notify_one();
        }
        Ok(written)
    }

    /// Push PCM samples into the staging ring buffer.
    /// Returns the number of samples actually written (may be < buf.len() if staging is full).
    pub fn push_staging_pcm_i16(&self, buf: &[i16]) -> Result<usize> {
        if !self.speaker_enabled {
            return Err(Error::config("audio_speaker", "speaker not initialized"));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        if self.shared.is_stopping() {
            return Err(Error::config("audio_speaker", "audio owner stopped"));
        }
        let mut guard = self
            .shared
            .staging
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let written = guard.push_slice_blocking(buf);
        if written > 0 {
            // Wake the worker so it can transfer staging → speaker.
            self.shared.staging_cv.notify_one();
            self.shared.speaker_cv.notify_one();
        }
        Ok(written)
    }

    /// Returns the number of samples currently in the staging ring buffer.
    pub fn staging_samples(&self) -> usize {
        let guard = self
            .shared
            .staging
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard.len()
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Drop for AudioPipelineState {
    fn drop(&mut self) {
        self.shared.request_stop();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        mic_i2s_sample_to_pcm16, should_feed_wake_backend, should_read_mic_frame,
        AUDIO_MIC_FRAME_SAMPLES, AUDIO_MIC_I2S_STAGING_SAMPLES, AUDIO_SPEAKER_FRAME_SAMPLES,
        AUDIO_SPEAKER_I2S_STAGING_SAMPLES,
    };

    #[test]
    fn mic_polling_stays_off_when_no_consumer_exists() {
        assert!(!should_read_mic_frame(false, false, false, false));
    }

    #[test]
    fn mic_polling_turns_on_for_recording_or_wake_backend() {
        assert!(should_read_mic_frame(true, false, false, false));
        assert!(should_read_mic_frame(false, false, true, false));
    }

    #[test]
    fn audio_playback_only_reads_when_interrupt_listening_is_enabled() {
        assert!(!should_read_mic_frame(true, true, true, false));
        assert!(should_read_mic_frame(true, true, true, true));
        assert!(!should_read_mic_frame(false, true, false, true));
    }

    #[test]
    fn wake_backend_feed_is_suppressed_while_recording_unless_barge_in_is_armed() {
        assert!(should_feed_wake_backend(false, false, true));
        assert!(!should_feed_wake_backend(true, false, true));
        assert!(should_feed_wake_backend(true, true, true));
        assert!(!should_feed_wake_backend(false, false, false));
    }

    #[test]
    fn i2s_staging_buffers_cover_worker_frame_sizes() {
        assert!(AUDIO_MIC_I2S_STAGING_SAMPLES >= AUDIO_MIC_FRAME_SAMPLES);
        assert!(AUDIO_SPEAKER_I2S_STAGING_SAMPLES >= AUDIO_SPEAKER_FRAME_SAMPLES);
    }

    #[test]
    fn mic_i2s_left_justified_24bit_sample_preserves_expected_gain() {
        assert_eq!(mic_i2s_sample_to_pcm16(1024 << 12), 1024);
        assert_eq!(mic_i2s_sample_to_pcm16(0x07ff_f000), i16::MAX);
    }

    #[test]
    fn mic_i2s_conversion_saturates_negative_peak_like_reference() {
        assert_eq!(mic_i2s_sample_to_pcm16((-50_000i32) << 12), -i16::MAX);
    }
}
