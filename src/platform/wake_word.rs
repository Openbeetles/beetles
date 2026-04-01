//! 唤醒词检测运行时（仅 ESP32）。
//! Wake-word detection runtime (ESP32-only).
//!
//! Architecture
//! ───────────
//! `OnceLock<Mutex<Option<WakeWordInner>>>` holds mutable state: `None` until
//! `configure()` runs; then `Some`. C engine init and Rust state install happen
//! under the same mutex to avoid tearing vs the global `beetle_wakenet_*` context.
//! `feed_pcm_i16()` is called from `audio_io_worker` (hot path, every ~20 ms).
//!
//! On detection, a `VoiceEvent::WakeDetected` is sent to the voice session thread
//! which handles capture, STT, and agent injection.
//!
//! Safety invariant: the Mutex is only held for the duration of a single feed
//! call (≤ 1 ms), so contention with the configure path is negligible.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod imp {
    use crate::audio::voice_session::VoiceEvent;
    use crate::constants::WAKE_WORD_COOLDOWN_MS;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    // ── C ABI bindings ────────────────────────────────────────────────────────

    extern "C" {
        fn beetle_wakenet_init(model_name: *const std::os::raw::c_char) -> i32;
        fn beetle_wakenet_feed(pcm: *const i16, samples: i32) -> i32;
        fn beetle_wakenet_reset();
    }

    const BEETLE_WN_OK: i32 = 0;
    const BEETLE_WN_DETECTED: i32 = 1;

    // ── state ─────────────────────────────────────────────────────────────────

    struct WakeWordInner {
        /// WakeNet model name as passed to beetle_wakenet_init.
        model_name: String,
        /// Sender to the voice session thread.
        voice_tx: SyncSender<VoiceEvent>,
        /// Whether the C engine has been successfully initialised.
        engine_ready: bool,
        /// Monotonic instant of the last successful trigger (for cooldown).
        last_trigger: Option<Instant>,
    }

    /// Fast-path guard: set to `false` when wake word is disabled or
    /// engine init failed, so `feed_pcm_i16` short-circuits without locking.
    static ARMED: AtomicBool = AtomicBool::new(false);

    static STATE: OnceLock<Mutex<Option<WakeWordInner>>> = OnceLock::new();

    fn state_mutex() -> &'static Mutex<Option<WakeWordInner>> {
        STATE.get_or_init(|| Mutex::new(None))
    }

    // ── public API ────────────────────────────────────────────────────────────

    /// Initialise the wake-word engine and register the voice event sender.
    ///
    /// Must be called from `run_app`, **after** `MessageBus` is created and
    /// `init_audio` has been called. Subsequent calls log a warning and return.
    pub fn configure(model_name: &str, voice_tx: SyncSender<VoiceEvent>) {
        let mut guard = state_mutex().lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
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

        let inner = WakeWordInner {
            model_name: model_name.to_string(),
            voice_tx,
            engine_ready,
            last_trigger: None,
        };

        *guard = Some(inner);
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
        if !ARMED.load(Ordering::Relaxed) {
            return;
        }
        // Skip while voice capture or speaker playback is active.
        if crate::orchestrator::is_audio_recording() || crate::orchestrator::is_audio_playing() {
            return;
        }

        let mut st_guard = match state_mutex().lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        let st = match st_guard.as_mut() {
            Some(s) => s,
            None => return,
        };

        if !st.engine_ready {
            return;
        }

        // Cooldown check.
        if let Some(t) = st.last_trigger {
            if t.elapsed() < Duration::from_millis(WAKE_WORD_COOLDOWN_MS) {
                return;
            }
        }

        let detected = unsafe {
            beetle_wakenet_feed(frame.as_ptr(), frame.len() as i32) == BEETLE_WN_DETECTED
        };

        if detected {
            log::info!("[wake_word] triggered keyword={}", st.model_name);
            st.last_trigger = Some(Instant::now());
            unsafe { beetle_wakenet_reset() };

            if let Err(e) = st.voice_tx.try_send(VoiceEvent::WakeDetected) {
                log::warn!("[wake_word] voice event queue full, trigger dropped: {}", e);
            }
        }
    }
} // mod imp

// ── re-export for ESP targets ─────────────────────────────────────────────────

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use imp::{configure, feed_pcm_i16};
