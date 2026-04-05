//! 唤醒词检测运行时（仅 ESP32）。
//! Wake-word detection runtime (ESP32-only).
//!
//! Architecture
//! ───────────
//! `OnceLock<Mutex<WakeWordState>>` owns the mutable Rust-side runtime slot,
//! while `ARMED` provides a lock-free fast-path guard for `feed_pcm_i16()`.
//! `feed_pcm_i16()` is called from `audio_io_worker` (hot path, every ~20 ms).
//!
//! On detection, a `VoiceEvent::WakeDetected` is sent to the voice session thread
//! which handles capture, STT, and agent injection.
//!
//! Safety invariant: all WakeNet C ABI calls (`init/feed/reset/destroy`) are
//! serialized by the same state mutex because the engine is not re-entrant.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod imp {
    use crate::audio::voice_session::VoiceEvent;
    use crate::constants::WAKE_WORD_COOLDOWN_MS;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    // ── C ABI bindings ────────────────────────────────────────────────────────

    extern "C" {
        fn beetle_wakenet_init(
            model_name: *const std::os::raw::c_char,
            input_sample_rate_hz: i32,
        ) -> i32;
        fn beetle_wakenet_feed(pcm: *const i16, samples: i32) -> i32;
        fn beetle_wakenet_reset();
        fn beetle_wakenet_destroy();
    }

    const BEETLE_WN_OK: i32 = 0;
    const BEETLE_WN_DETECTED: i32 = 1;

    // ── state ─────────────────────────────────────────────────────────────────

    struct WakeWordRuntime {
        /// WakeNet model name as passed to beetle_wakenet_init.
        model_name: String,
        /// Incoming PCM sample rate from the mic path.
        input_sample_rate_hz: u32,
        /// Sender to the voice session thread.
        voice_tx: SyncSender<VoiceEvent>,
        /// Monotonic timestamp of the last successful trigger.
        last_trigger_millis: AtomicU32,
    }

    /// Fast-path guard: set to `false` when wake word is disabled or
    /// engine init failed, so `feed_pcm_i16` short-circuits without locking.
    static ARMED: AtomicBool = AtomicBool::new(false);

    #[derive(Default)]
    struct WakeWordState {
        runtime: Option<WakeWordRuntime>,
    }

    static STATE: OnceLock<Mutex<WakeWordState>> = OnceLock::new();

    fn state() -> &'static Mutex<WakeWordState> {
        STATE.get_or_init(|| Mutex::new(WakeWordState::default()))
    }

    fn monotonic_millis() -> u32 {
        let micros = unsafe { esp_idf_svc::sys::esp_timer_get_time() };
        (micros / 1_000) as u32
    }

    fn within_cooldown(last_trigger_millis: u32, now_millis: u32) -> bool {
        if last_trigger_millis == 0 {
            return false;
        }
        u64::from(now_millis.wrapping_sub(last_trigger_millis)) < WAKE_WORD_COOLDOWN_MS
    }

    // ── public API ────────────────────────────────────────────────────────────

    pub fn is_armed() -> bool {
        ARMED.load(Ordering::Relaxed)
    }

    /// Initialise or reconfigure the wake-word engine and register the voice event sender.
    ///
    /// Must be called from `run_app`, **after** `MessageBus` is created and
    /// `init_audio` has been called.
    pub fn configure(
        model_name: &str,
        input_sample_rate_hz: u32,
        voice_tx: SyncSender<VoiceEvent>,
    ) {
        let c_model = match CString::new(model_name) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[wake_word] model name contains null byte: {}", e);
                return;
            }
        };
        if input_sample_rate_hz != 16_000 && input_sample_rate_hz != 24_000 {
            log::error!(
                "[wake_word] unsupported input sample rate {}; wake word disabled",
                input_sample_rate_hz
            );
            return;
        }

        ARMED.store(false, Ordering::Release);
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        let previous_model = state
            .runtime
            .as_ref()
            .map(|runtime| (runtime.model_name.clone(), runtime.input_sample_rate_hz));

        let engine_ready = unsafe {
            beetle_wakenet_destroy();
            let rc = beetle_wakenet_init(c_model.as_ptr(), input_sample_rate_hz as i32);
            if rc == BEETLE_WN_OK {
                if let Some((previous_model, previous_rate)) = previous_model.as_ref() {
                    log::info!(
                        "[wake_word] WakeNet reconfigured (from={}@{}Hz, to={}@{}Hz)",
                        previous_model,
                        previous_rate,
                        model_name,
                        input_sample_rate_hz
                    );
                } else {
                    log::info!(
                        "[wake_word] WakeNet init ok (model={}, input={}Hz)",
                        model_name,
                        input_sample_rate_hz
                    );
                }
                true
            } else {
                log::error!(
                    "[wake_word] WakeNet init failed (rc={}, model={}, input={}Hz); wake word disabled",
                    rc,
                    model_name,
                    input_sample_rate_hz
                );
                false
            }
        };

        let runtime = WakeWordRuntime {
            model_name: model_name.to_string(),
            input_sample_rate_hz,
            voice_tx,
            last_trigger_millis: AtomicU32::new(0),
        };
        state.runtime = Some(runtime);
        ARMED.store(engine_ready, Ordering::Release);
    }

    /// Shut down the wake-word engine and release C-side resources.
    pub fn shutdown() {
        ARMED.store(false, Ordering::Release);
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        let previous_model = state
            .runtime
            .take()
            .map(|runtime| (runtime.model_name, runtime.input_sample_rate_hz));
        unsafe { beetle_wakenet_destroy() };
        if let Some((previous_model, previous_rate)) = previous_model {
            log::info!(
                "[wake_word] WakeNet destroyed (model={} input={}Hz)",
                previous_model,
                previous_rate
            );
        }
    }

    /// Feed a frame of mono 16-bit PCM (from `audio_io_worker`).
    ///
    /// Returns immediately (no-op) when:
    /// - `configure()` has not been called yet
    /// - the engine failed to initialise
    /// - `orchestrator::is_audio_recording()` is true (voice capture is active)
    /// - within the post-detection cooldown window
    ///
    /// SAFETY: the C function `beetle_wakenet_feed` is not re-entrant; access
    /// is serialised by the Mutex.
    pub fn feed_pcm_i16(frame: &[i16]) {
        // Hot-path short-circuit (atomic load, no lock).
        if frame.is_empty() || !is_armed() {
            return;
        }
        crate::metrics::record_wake_word_feed_call();
        let interrupt_listening = crate::orchestrator::is_audio_interrupt_listening();
        // Realtime playback interrupt mode keeps mic + wake-word feed alive so the
        // active session can be cut locally; other busy modes still suppress feed.
        if crate::orchestrator::is_audio_recording() && !interrupt_listening {
            crate::metrics::record_wake_word_feed_skip_busy();
            return;
        }
        if crate::orchestrator::is_audio_playing() && !interrupt_listening {
            crate::metrics::record_wake_word_feed_skip_busy();
            return;
        }
        let mut state = state().lock().unwrap_or_else(|e| e.into_inner());
        let runtime = match state.runtime.as_mut() {
            Some(runtime) => runtime,
            None => return,
        };
        let now_millis = monotonic_millis();
        if within_cooldown(
            runtime.last_trigger_millis.load(Ordering::Relaxed),
            now_millis,
        ) {
            crate::metrics::record_wake_word_feed_skip_cooldown();
            return;
        }

        let feed_start = Instant::now();
        let detected = unsafe {
            beetle_wakenet_feed(frame.as_ptr(), frame.len() as i32) == BEETLE_WN_DETECTED
        };
        crate::metrics::record_wake_word_feed_us(feed_start.elapsed().as_micros());

        if detected {
            crate::metrics::record_wake_word_feed_detect();
            if crate::orchestrator::is_audio_playing() && interrupt_listening {
                log::info!(
                    "[wake_word] interrupt requested keyword={}",
                    runtime.model_name
                );
                runtime
                    .last_trigger_millis
                    .store(now_millis, Ordering::Relaxed);
                unsafe { beetle_wakenet_reset() };
                crate::metrics::record_voice_interrupt_requested();
                crate::orchestrator::request_audio_interrupt();
                return;
            }
            log::info!("[wake_word] triggered keyword={}", runtime.model_name);
            runtime
                .last_trigger_millis
                .store(now_millis, Ordering::Relaxed);
            unsafe { beetle_wakenet_reset() };

            if let Err(e) = runtime.voice_tx.try_send(VoiceEvent::WakeDetected) {
                log::warn!("[wake_word] voice event queue full, trigger dropped: {}", e);
            }
        }
    }
} // mod imp

// ── re-export for ESP targets ─────────────────────────────────────────────────

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use imp::{configure, feed_pcm_i16, is_armed, shutdown};
