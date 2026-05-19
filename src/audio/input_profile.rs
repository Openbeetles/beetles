//! Audio input hardware profile.
//! 将音频输入硬件事实收口成 realtime 语音可消费的稳定 profile。

use crate::config::{self, AudioSegment, AUDIO_CODEC_INPUT_ES7210, AUDIO_TOPOLOGY_I2S_CODEC};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioInputCodecKind {
    GenericI2s,
    Es7210,
    DigitalMic,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioInputLevelModel {
    DefaultSpeech,
    LowSnrFarField,
    CustomWakeDerived,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioInputHardwareProfile {
    pub codec: AudioInputCodecKind,
    pub level_model: AudioInputLevelModel,
    pub input_reference_enabled: bool,
    pub sample_rate_hz: u32,
    pub channels: u8,
}

impl AudioInputHardwareProfile {
    pub fn from_audio_config(audio: &AudioSegment) -> Self {
        let codec = if audio.topology == AUDIO_TOPOLOGY_I2S_CODEC {
            match audio.codec.input_codec.as_deref() {
                Some(AUDIO_CODEC_INPUT_ES7210) => AudioInputCodecKind::Es7210,
                Some(_) => AudioInputCodecKind::Unknown,
                None => AudioInputCodecKind::Unknown,
            }
        } else if config::audio_microphone_uses_pdm(audio) {
            AudioInputCodecKind::DigitalMic
        } else if audio.microphone.enabled {
            AudioInputCodecKind::GenericI2s
        } else {
            AudioInputCodecKind::Unknown
        };
        let input_reference_enabled =
            codec == AudioInputCodecKind::Es7210 && audio.codec.input_reference;
        let level_model = if crate::config::audio_uses_es7210_codec_wake_profile(audio) {
            AudioInputLevelModel::LowSnrFarField
        } else if input_reference_enabled
            && audio.wake_word.enabled
            && audio.wake_word.enter_threshold
                < crate::config::default_wake_enter_threshold_for_profile()
        {
            AudioInputLevelModel::CustomWakeDerived
        } else {
            AudioInputLevelModel::DefaultSpeech
        };
        Self {
            codec,
            level_model,
            input_reference_enabled,
            sample_rate_hz: audio.microphone.sample_rate.max(8_000),
            channels: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn es7210_low_snr_profile_is_derived_from_audio_config() {
        let mut audio = crate::config::default_disabled_audio_segment();
        audio.enabled = true;
        audio.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        audio.microphone.enabled = true;
        audio.microphone.sample_rate = 24_000;
        audio.speaker.enabled = true;
        audio.speaker.sample_rate = 24_000;
        audio.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        audio.codec.input_reference = true;
        audio.wake_word.enabled = true;
        audio.wake_word.enter_threshold = 0.01;
        audio.wake_word.leave_threshold = 0.005;
        audio.wake_word.zcr_max = 0.65;
        audio.wake_word.min_speech_band_ratio = 0.35;
        audio.wake_word.min_active_ms = 120;

        let profile = AudioInputHardwareProfile::from_audio_config(&audio);

        assert_eq!(profile.codec, AudioInputCodecKind::Es7210);
        assert_eq!(profile.level_model, AudioInputLevelModel::LowSnrFarField);
        assert!(profile.input_reference_enabled);
    }

    #[test]
    fn es7210_without_input_reference_does_not_inherit_low_snr_profile() {
        let mut audio = crate::config::default_disabled_audio_segment();
        audio.enabled = true;
        audio.topology = crate::config::AUDIO_TOPOLOGY_I2S_CODEC.to_string();
        audio.microphone.enabled = true;
        audio.microphone.sample_rate = 24_000;
        audio.codec.input_codec = Some(crate::config::AUDIO_CODEC_INPUT_ES7210.to_string());
        audio.codec.input_reference = false;
        audio.wake_word.enabled = true;
        audio.wake_word.enter_threshold = 0.01;

        let profile = AudioInputHardwareProfile::from_audio_config(&audio);

        assert_eq!(profile.codec, AudioInputCodecKind::Es7210);
        assert_eq!(profile.level_model, AudioInputLevelModel::DefaultSpeech);
        assert!(!profile.input_reference_enabled);
    }
}
