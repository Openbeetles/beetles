//! Wake backend runtime glue.
//! 收口声学唤醒后端与语音会话事件队列的接线层。

mod acoustic;
mod backend;

pub use acoustic::{AcousticWakeBackend, AcousticWakeConfig};
pub use backend::{WakeBackend, WakeEvent};

use crate::audio::voice_session::VoiceEvent;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct WakeRuntimeState {
    backend: WakeBackend,
    voice_tx: Option<SyncSender<VoiceEvent>>,
}

fn state() -> &'static Mutex<WakeRuntimeState> {
    static STATE: OnceLock<Mutex<WakeRuntimeState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(WakeRuntimeState::default()))
}

/// Install the current wake backend and the voice-session event sink.
pub fn configure(backend: WakeBackend, voice_tx: SyncSender<VoiceEvent>) {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.backend = backend;
    guard.voice_tx = Some(voice_tx);
}

/// Return whether the active backend needs continuous PCM feed from the mic hot path.
pub fn requires_pcm_feed() -> bool {
    state()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .backend
        .requires_pcm_feed()
}

/// Feed one PCM frame plus optional playback reference into the active wake backend.
pub fn feed_pcm_i16(mic: &[i16], reference: &[i16], audio_playing: bool) {
    let (event, voice_tx) = {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let event = guard.backend.feed_pcm_i16(mic, reference, audio_playing);
        (event, guard.voice_tx.clone())
    };

    match event {
        Some(WakeEvent::TriggerStart) => {
            crate::metrics::record_wake_word_trigger();
            if let Some(tx) = voice_tx {
                match tx.try_send(VoiceEvent::WakeTriggered) {
                    Ok(()) => {}
                    Err(TrySendError::Full(_)) => {
                        log::debug!("[wake] dropping wake trigger because voice queue is full");
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        log::warn!("[wake] dropping wake trigger because voice queue is closed");
                    }
                }
            }
        }
        Some(WakeEvent::InterruptRequest) => {
            crate::metrics::record_voice_interrupt_requested();
            crate::orchestrator::request_audio_interrupt();
        }
        None => {}
    }
}

/// Reset session-scoped wake state after a voice interaction finishes.
pub fn reset_after_session() {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.backend.reset_after_session();
}

/// Disable wake processing and drop the voice-session event sink.
pub fn shutdown() {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.backend = WakeBackend::Disabled;
    guard.voice_tx = None;
}
