//! Voice endpoint profile.
//! 本地 endpoint、server VAD 与 provider turn owner 的共同真源。

use crate::audio::input_profile::{AudioInputHardwareProfile, AudioInputLevelModel};
use crate::audio::realtime_provider::RealtimeProvider;
use crate::config::AudioSegment;
use crate::platform::AudioDuplexCapabilities;

const DEFAULT_LOCAL_ENDPOINT_THRESHOLD_MAX: f32 = 0.12;
const DEFAULT_LOCAL_LEAVE_RATIO: f32 = 0.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceEndpointOwner {
    ServerVad,
    ClientCommit,
}

#[derive(Clone, Copy, Debug)]
pub struct VoiceEndpointProfile {
    pub owner: VoiceEndpointOwner,
    pub hardware: AudioInputHardwareProfile,
    pub local_enter_threshold: f32,
    pub local_leave_threshold: f32,
    pub server_vad_threshold: f32,
    pub min_active_ms: u32,
    pub silence_duration_ms: u32,
    pub low_snr_codec_profile: bool,
}

impl VoiceEndpointProfile {
    pub fn from_audio_config(
        audio_cfg: &AudioSegment,
        provider: RealtimeProvider,
        duplex_caps: AudioDuplexCapabilities,
    ) -> Self {
        let input = AudioInputHardwareProfile::from_audio_config(audio_cfg);
        Self::from_input_profile(input, audio_cfg, provider, duplex_caps)
    }

    pub fn from_input_profile(
        input: AudioInputHardwareProfile,
        audio_cfg: &AudioSegment,
        provider: RealtimeProvider,
        _duplex_caps: AudioDuplexCapabilities,
    ) -> Self {
        let wake = crate::config::audio_wake_word_config_for_runtime(audio_cfg);
        let low_snr = matches!(input.level_model, AudioInputLevelModel::LowSnrFarField);
        let custom_wake = matches!(input.level_model, AudioInputLevelModel::CustomWakeDerived);
        let local_enter_threshold = if low_snr || custom_wake {
            wake.enter_threshold
        } else {
            audio_cfg
                .vad
                .threshold
                .clamp(0.0, 1.0)
                .min(DEFAULT_LOCAL_ENDPOINT_THRESHOLD_MAX)
        };
        let local_leave_threshold = if low_snr || custom_wake {
            wake.leave_threshold
        } else {
            (local_enter_threshold * DEFAULT_LOCAL_LEAVE_RATIO).clamp(0.0, local_enter_threshold)
        };
        let server_vad_threshold = if low_snr || custom_wake {
            wake.enter_threshold
        } else {
            audio_cfg.vad.threshold.clamp(0.0, 1.0)
        };
        Self {
            owner: provider.turn_contract().endpoint_owner,
            hardware: input,
            local_enter_threshold,
            local_leave_threshold,
            server_vad_threshold,
            min_active_ms: if low_snr || custom_wake {
                wake.min_active_ms
            } else {
                160
            },
            silence_duration_ms: audio_cfg.vad.silence_duration_ms,
            low_snr_codec_profile: low_snr,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn realtime_audio() -> AudioSegment {
        let mut audio = crate::config::default_disabled_audio_segment();
        audio.enabled = true;
        audio.microphone.enabled = true;
        audio.microphone.sample_rate = 24_000;
        audio.speaker.enabled = true;
        audio.speaker.sample_rate = 24_000;
        audio.wake_word.enabled = true;
        audio.realtime.provider = crate::config::AUDIO_REALTIME_PROVIDER_QWEN.to_string();
        audio.realtime.api_key = "key".to_string();
        audio.realtime.model = "qwen3.5-omni-plus-realtime".to_string();
        audio.realtime.voice = "Tina".to_string();
        audio
    }

    #[test]
    fn low_snr_thresholds_are_lower_than_default_speech() {
        let mut low = realtime_audio();
        low.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        low.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        low.codec.input_reference = true;
        low.wake_word.enter_threshold = 0.01;
        low.wake_word.leave_threshold = 0.005;
        low.wake_word.zcr_max = 0.65;
        low.wake_word.min_speech_band_ratio = 0.35;
        low.wake_word.min_active_ms = 120;

        let default = realtime_audio();

        let low_profile = VoiceEndpointProfile::from_audio_config(
            &low,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );
        let default_profile = VoiceEndpointProfile::from_audio_config(
            &default,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );

        assert!(low_profile.low_snr_codec_profile);
        assert!(low_profile.server_vad_threshold < default_profile.server_vad_threshold);
        assert!(low_profile.local_enter_threshold < default_profile.local_enter_threshold);
    }

    #[test]
    fn provider_decides_endpoint_owner() {
        let audio = realtime_audio();
        let qwen = VoiceEndpointProfile::from_audio_config(
            &audio,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );
        let doubao = VoiceEndpointProfile::from_audio_config(
            &audio,
            RealtimeProvider::Doubao,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );

        assert_eq!(qwen.owner, VoiceEndpointOwner::ServerVad);
        assert_eq!(doubao.owner, VoiceEndpointOwner::ClientCommit);
    }

    #[test]
    fn custom_wake_thresholds_are_preserved() {
        let mut audio = realtime_audio();
        audio.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        audio.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        audio.codec.input_reference = true;
        audio.wake_word.enter_threshold = 0.037;
        audio.wake_word.leave_threshold = 0.019;

        let profile = VoiceEndpointProfile::from_audio_config(
            &audio,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_with_input_reference(),
        );

        assert_eq!(profile.local_enter_threshold, 0.037);
        assert_eq!(profile.local_leave_threshold, 0.019);
    }

    #[test]
    fn default_speech_keeps_existing_local_endpoint_cap() {
        let mut audio = realtime_audio();
        audio.vad.threshold = 0.5;

        let profile = VoiceEndpointProfile::from_audio_config(
            &audio,
            RealtimeProvider::Qwen,
            AudioDuplexCapabilities::duplex_without_aec(),
        );

        assert_eq!(profile.local_enter_threshold, 0.12);
        assert_eq!(profile.server_vad_threshold, 0.5);
        assert!(!profile.low_snr_codec_profile);
    }
}
