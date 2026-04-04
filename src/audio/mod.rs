//! 音频业务逻辑（纯 Rust）：能量断句、百度 STT/TTS HTTP、语音采集、语音会话。
//! Pure-Rust audio logic: energy endpointing, Baidu STT/TTS HTTP, voice capture, voice session.

pub mod baidu_token;
pub mod capture;
pub mod energy;
pub mod pipeline;
pub mod realtime;
pub mod stt_baidu;
pub mod tts_baidu;
pub mod voice_session;
