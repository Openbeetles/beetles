use super::acoustic::AcousticWakeBackend;
use crate::audio::wake_handoff::WakeAcousticSnapshot;

/// Wake backend output consumed by the runtime glue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeEvent {
    TriggerStart,
    InterruptRequest,
}

/// Runtime-selectable wake backend.
#[derive(Default)]
pub enum WakeBackend {
    #[default]
    Disabled,
    Acoustic(AcousticWakeBackend),
}

impl WakeBackend {
    /// Whether this backend needs continuous PCM feed from the mic hot path.
    pub fn requires_pcm_feed(&self) -> bool {
        matches!(self, Self::Acoustic(_))
    }

    /// Consume one mic frame and optional playback reference frame.
    pub fn feed_pcm_i16(
        &mut self,
        mic: &[i16],
        reference: &[i16],
        audio_playing: bool,
    ) -> Option<WakeEvent> {
        match self {
            Self::Disabled => None,
            Self::Acoustic(backend) => backend.feed_pcm_i16(mic, reference, audio_playing),
        }
    }

    /// Reset any per-session state without rebuilding the backend.
    pub fn reset_after_session(&mut self) {
        if let Self::Acoustic(backend) = self {
            backend.reset_after_session();
        }
    }

    pub fn acoustic_snapshot(&self) -> WakeAcousticSnapshot {
        match self {
            Self::Disabled => WakeAcousticSnapshot::default(),
            Self::Acoustic(backend) => backend.snapshot(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wake::acoustic::AcousticWakeConfig;

    #[test]
    fn wake_backend_requires_pcm_feed_only_for_acoustic() {
        assert!(!WakeBackend::Disabled.requires_pcm_feed());
        assert!(
            WakeBackend::Acoustic(AcousticWakeBackend::new(AcousticWakeConfig::for_tests(),))
                .requires_pcm_feed()
        );
    }

    #[test]
    fn wake_backend_disabled_emits_no_event() {
        let mut backend = WakeBackend::Disabled;
        assert_eq!(backend.feed_pcm_i16(&[1, 2, 3], &[0, 0, 0], false), None);
    }
}
