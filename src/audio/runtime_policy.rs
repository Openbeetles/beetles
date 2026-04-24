//! Audio runtime policy shared by config/bootstrap and ESP drivers.
//! 音频运行态策略：集中收口“是否值得启动 pipeline”和内存 fallback 边界。

/// Internal heap fallback cap for tiny audio scratch rings only.
/// 大型音频环形缓冲必须走 PSRAM；这里只允许很小的兜底缓冲。
pub(crate) const AUDIO_INTERNAL_HEAP_RING_FALLBACK_MAX_BYTES: usize = 16 * 1024;

pub(crate) fn allow_internal_audio_ring_fallback(byte_size: usize) -> bool {
    byte_size <= AUDIO_INTERNAL_HEAP_RING_FALLBACK_MAX_BYTES
}
