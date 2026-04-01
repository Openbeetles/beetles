//! 唤醒词检测运行时（仅 ESP32）。
//! Wake-word detection runtime (ESP32-only).
//!
//! Architecture
//! ───────────
//! `OnceLock<WakeWordRuntime>` installs immutable Rust-side config once, while
//! `ARMED` + `last_trigger_micros` provide lock-free fast-path state checks.
//! `feed_pcm_i16()` is called from `audio_io_worker` (hot path, every ~20 ms).
//!
//! On detection, a `VoiceEvent::WakeDetected` is sent to the voice session thread
//! which handles capture, STT, and agent injection.
//!
//! Safety invariant: the WakeNet C engine is still serialized by a dedicated
//! feed mutex because `beetle_wakenet_feed/reset` are not re-entrant.

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
        fn beetle_wakenet_init(model_name: *const std::os::raw::c_char) -> i32;
        fn beetle_wakenet_feed(pcm: *const i16, samples: i32) -> i32;
        fn beetle_wakenet_reset();
    }

    const BEETLE_WN_OK: i32 = 0;
    const BEETLE_WN_DETECTED: i32 = 1;

    // ── state ─────────────────────────────────────────────────────────────────

    struct WakeWordRuntime {
        /// WakeNet model name as passed to beetle_wakenet_init.
        model_name: String,
        /// Sender to the voice session thread.
        voice_tx: SyncSender<VoiceEvent>,
        /// Monotonic timestamp of the last successful trigger.
        last_trigger_millis: AtomicU32,
        /// The C engine is not re-entrant; keep feed/reset serialized.
        feed_lock: Mutex<()>,
    }

    /// Fast-path guard: set to `false` when wake word is disabled or
    /// engine init failed, so `feed_pcm_i16` short-circuits without locking.
    static ARMED: AtomicBool = AtomicBool::new(false);

    static RUNTIME: OnceLock<WakeWordRuntime> = OnceLock::new();

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

    /// Initialise the wake-word engine and register the voice event sender.
    ///
    /// Must be called from `run_app`, **after** `MessageBus` is created and
    /// `init_audio` has been called. Subsequent calls log a warning and return.
    pub fn configure(model_name: &str, voice_tx: SyncSender<VoiceEvent>) {
        if RUNTIME.get().is_some() {
            log::warn!("[wake_word] configure() called more than once – ignored");
            return;
        }

        let c_model = match CString::new(model_name) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[wake_word] model name contains null byte: {}", e);
                return;
            }
        };

        let engine_ready = unsafe {
            let rc = beetle_wakenet_init(c_model.as_ptr());
            if rc == BEETLE_WN_OK {
                log::info!("[wake_word] WakeNet init ok (model={})", model_name);
                true
            } else {
                log::error!(
                    "[wake_word] WakeNet init failed (rc={}, model={}); wake word disabled",
                    rc,
                    model_name
                );
                false
            }
        };

        let runtime = WakeWordRuntime {
            model_name: model_name.to_string(),
            voice_tx,
            last_trigger_millis: AtomicU32::new(0),
            feed_lock: Mutex::new(()),
        };
        if RUNTIME.set(runtime).is_err() {
            log::warn!("[wake_word] runtime already configured – ignored");
            return;
        }
        ARMED.store(engine_ready, Ordering::Release);
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
        // Skip while voice capture or speaker playback is active.
        if crate::orchestrator::is_audio_recording() || crate::orchestrator::is_audio_playing() {
            crate::metrics::record_wake_word_feed_skip_busy();
            return;
        }
        let runtime = match RUNTIME.get() {
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
        let _feed_guard = match runtime.feed_lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
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
pub use imp::{configure, feed_pcm_i16, is_armed};
