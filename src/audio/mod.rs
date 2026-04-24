//! 音频业务逻辑（纯 Rust）：能量断句、百度 STT/TTS HTTP、语音采集、语音会话。
//! Pure-Rust audio logic: energy endpointing, Baidu STT/TTS HTTP, voice capture, voice session.

pub mod baidu_token;
pub mod capture;
pub mod energy;
pub mod pipeline;
pub mod realtime;
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32", test))]
pub(crate) mod runtime_policy;
pub mod stt_baidu;
pub mod tts_baidu;
pub mod voice_session;

#[cfg(test)]
mod tests {
    #[test]
    fn large_audio_ring_fallback_to_internal_heap_is_rejected() {
        assert!(!crate::audio::runtime_policy::allow_internal_audio_ring_fallback(320 * 1024));
        assert!(crate::audio::runtime_policy::allow_internal_audio_ring_fallback(4 * 1024));
    }
}
