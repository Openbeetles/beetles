//! 唤醒词检测运行时（仅 ESP32）。
//! Wake-word detection runtime (ESP32-only).
//!
//! Architecture
//! ───────────
//! A single `OnceLock<Mutex<WakeWordInner>>` holds all mutable state.
//! `configure()` is called once from `run_app` after `MessageBus` creation.
//! `feed_pcm_i16()` is called from `audio_io_worker` (hot path, every ~20 ms).
//!
//! Safety invariant: the Mutex is only held for the duration of a single feed
//! call (≤ 1 ms), so contention with the configure path is negligible.

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
mod imp {
    use crate::bus::{PcMsg, TrackedSender};
    use crate::constants::WAKE_WORD_COOLDOWN_MS;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    // ── C ABI bindings ────────────────────────────────────────────────────────

    extern "C" {
        fn beetle_wakenet_init(model_name: *const std::os::raw::c_char) -> i32;
        fn beetle_wakenet_feed(pcm: *const i16, samples: i32) -> i32;
        fn beetle_wakenet_reset();
        fn beetle_wakenet_destroy();
    }

    const BEETLE_WN_OK: i32 = 0;
    const BEETLE_WN_DETECTED: i32 = 1;

    // ── state ─────────────────────────────────────────────────────────────────

    struct WakeWordInner {
        /// WakeNet model name as passed to beetle_wakenet_init.
        model_name: String,
        /// Sender into the user inbound bus.
        inbound_tx: TrackedSender<PcMsg>,
        /// Channel name for the injected PcMsg.
        channel: String,
        /// Chat ID for the injected PcMsg.
        chat_id: String,
        /// Prompt text injected on each detection.
        prompt: String,
        /// Whether the C engine has been successfully initialised.
        engine_ready: bool,
        /// Monotonic instant of the last successful trigger (for cooldown).
        last_trigger: Option<Instant>,
    }

    /// Fast-path guard: set to `false` when wake word is disabled or
    /// engine init failed, so `feed_pcm_i16` short-circuits without locking.
    static ARMED: AtomicBool = AtomicBool::new(false);

    static STATE: OnceLock<Mutex<WakeWordInner>> = OnceLock::new();

    // ── public API ────────────────────────────────────────────────────────────

    /// Initialise the wake-word engine and register the inbound sender.
    ///
    /// Must be called exactly once from `run_app`, **after** `MessageBus` is
    /// created and `init_audio` has been called.  Subsequent calls are ignored.
    pub fn configure(
        model_name: &str,
        channel: &str,
        chat_id: &str,
        prompt: &str,
        tx: TrackedSender<PcMsg>,
    ) {
        // OnceLock: only the first caller wins; concurrent re-configure is a
        // programming error but harmless (second call is silently dropped).
        if STATE.get().is_some() {
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
                log::error!("[wake_word] WakeNet init failed (rc={}, model={}); wake word disabled", rc, model_name);
                false
            }
        };

        let inner = WakeWordInner {
            model_name: model_name.to_string(),
            inbound_tx: tx,
            channel: channel.to_string(),
            chat_id: chat_id.to_string(),
            prompt: prompt.to_string(),
            engine_ready,
            last_trigger: None,
        };

        if STATE.set(Mutex::new(inner)).is_err() {
            // Another thread beat us; engine is initialised twice – destroy ours.
            unsafe { beetle_wakenet_destroy() };
            return;
        }

        ARMED.store(engine_ready, Ordering::Release);
    }

    /// Feed a frame of mono 16-bit PCM (from `audio_io_worker`).
    ///
    /// Returns immediately (no-op) when:
    /// - `configure()` has not been called yet
    /// - the engine failed to initialise
    /// - `orchestrator::is_audio_recording()` is true (voice_input is active)
    /// - within the post-detection cooldown window
    ///
    /// SAFETY: the C function `beetle_wakenet_feed` is not re-entrant; access
    /// is serialised by the Mutex.
    pub fn feed_pcm_i16(frame: &[i16]) {
        // Hot-path short-circuit (atomic load, no lock).
        if !ARMED.load(Ordering::Relaxed) {
            return;
        }
        // Skip while voice_input tool is actively recording to avoid semantic
        // confusion (overlapping mic ownership is documented in module rustdoc).
        if crate::orchestrator::is_audio_recording() {
            return;
        }

        let state_mutex = match STATE.get() {
            Some(m) => m,
            None => return,
        };

        let mut st = match state_mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
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
            crate::metrics::record_wake_word_trigger();
            log::info!(
                "[wake_word] triggered keyword={} chat={}",
                st.model_name,
                st.chat_id
            );
            st.last_trigger = Some(Instant::now());
            unsafe { beetle_wakenet_reset() };

            match PcMsg::new_inbound(&*st.channel, &*st.chat_id, &*st.prompt, false) {
                Ok(msg) => {
                    if let Err(e) = st.inbound_tx.try_send(msg) {
                        log::warn!("[wake_word] inbound queue full, trigger dropped: {}", e);
                    }
                }
                Err(e) => {
                    log::warn!("[wake_word] PcMsg construction failed: {}", e);
                }
            }
        }
    }
} // mod imp

// ── re-export for ESP targets ─────────────────────────────────────────────────

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub use imp::{configure, feed_pcm_i16};
