#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
use super::acoustic::AcousticWakeBackend;
#[cfg(beetle_esp32s3)]
use super::esp_sr::EspSrWakeBackend;
use crate::audio::wake_handoff::WakeAcousticSnapshot;
use crate::config::AudioSegment;

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
    #[cfg(beetle_esp32s3)]
    EspSrWakeNet(EspSrWakeBackend),
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    LinuxAcoustic(AcousticWakeBackend),
}

impl WakeBackend {
    /// Build the platform-default wake backend from the existing audio config.
    pub fn from_audio_config(audio: &AudioSegment) -> Self {
        if !audio.wake_word.enabled {
            return Self::Disabled;
        }

        #[cfg(beetle_esp32s3)]
        {
            return match EspSrWakeBackend::from_audio_config(audio) {
                Ok(backend) => Self::EspSrWakeNet(backend),
                Err(error) => {
                    log::error!(
                        "[wake] ESP-SR WakeNet init failed; wake disabled: {}",
                        error
                    );
                    Self::Disabled
                }
            };
        }

        #[cfg(all(
            any(target_arch = "xtensa", target_arch = "riscv32"),
            not(beetle_esp32s3)
        ))]
        {
            log::warn!(
                "[wake] ESP-SR WakeNet is enabled only on the ESP32-S3 build target; wake disabled on this ESP target"
            );
            return Self::Disabled;
        }

        #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
        {
            Self::LinuxAcoustic(AcousticWakeBackend::from_audio_config(audio))
        }
    }

    /// Whether this backend needs continuous PCM feed from the mic hot path.
    pub fn requires_pcm_feed(&self) -> bool {
        match self {
            Self::Disabled => false,
            #[cfg(beetle_esp32s3)]
            Self::EspSrWakeNet(_) => true,
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::LinuxAcoustic(_) => true,
        }
    }

    /// Consume one mic frame and optional playback reference frame.
    pub fn feed_pcm_i16(
        &mut self,
        mic: &[i16],
        reference: &[i16],
        audio_playing: bool,
    ) -> Option<WakeEvent> {
        #[cfg(beetle_esp32s3)]
        let _ = reference;
        #[cfg(target_arch = "riscv32")]
        let _ = (mic, reference, audio_playing);
        match self {
            Self::Disabled => None,
            #[cfg(beetle_esp32s3)]
            Self::EspSrWakeNet(backend) => backend.feed_pcm_i16(mic, audio_playing),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::LinuxAcoustic(backend) => backend.feed_pcm_i16(mic, reference, audio_playing),
        }
    }

    /// Reset any per-session state without rebuilding the backend.
    pub fn reset_after_session(&mut self) {
        match self {
            Self::Disabled => {}
            #[cfg(beetle_esp32s3)]
            Self::EspSrWakeNet(backend) => backend.reset_after_session(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::LinuxAcoustic(backend) => backend.reset_after_session(),
        }
    }

    pub fn acoustic_snapshot(&self) -> WakeAcousticSnapshot {
        match self {
            Self::Disabled => WakeAcousticSnapshot::default(),
            #[cfg(beetle_esp32s3)]
            Self::EspSrWakeNet(backend) => backend.snapshot(),
            #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
            Self::LinuxAcoustic(backend) => backend.snapshot(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wake::acoustic::AcousticWakeConfig;

    #[test]
    fn wake_backend_requires_pcm_feed_only_for_active_backends() {
        assert!(!WakeBackend::Disabled.requires_pcm_feed());
        assert!(WakeBackend::LinuxAcoustic(AcousticWakeBackend::new(
            AcousticWakeConfig::for_tests(),
        ))
        .requires_pcm_feed());
    }

    #[test]
    fn wake_backend_disabled_emits_no_event() {
        let mut backend = WakeBackend::Disabled;
        assert_eq!(backend.feed_pcm_i16(&[1, 2, 3], &[0, 0, 0], false), None);
    }

    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    #[test]
    fn host_audio_config_uses_linux_acoustic_backend_when_wake_enabled() {
        let mut audio = crate::config::default_disabled_audio_segment();
        audio.wake_word.enabled = true;

        let backend = WakeBackend::from_audio_config(&audio);

        assert!(matches!(backend, WakeBackend::LinuxAcoustic(_)));
    }

    #[test]
    fn disabled_audio_config_disables_wake_backend() {
        let audio = crate::config::default_disabled_audio_segment();

        let backend = WakeBackend::from_audio_config(&audio);

        assert!(matches!(backend, WakeBackend::Disabled));
    }
}
