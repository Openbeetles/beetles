//! Wake-trigger audio handoff.
//! 唤醒触发时冻结最近一小段麦克风 PCM，避免 realtime WSS ready 前首句丢失。

use crate::platform::byte_buffer::ByteBuffer;
use std::io::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const PRE_ROLL_MS: u32 = 2_000;
const MONO_CHANNELS: u8 = 1;
static HANDOFF_ID: AtomicU32 = AtomicU32::new(1);

/// Acoustic diagnostic snapshot captured near the wake trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WakeAcousticSnapshot {
    pub mic_level_pm: u32,
    pub zcr_pm: u32,
    pub speech_ratio_pm: u32,
    pub speech_coverage_pm: u32,
    pub speech_dominance_pm: u32,
    pub activation_pm: u32,
    pub speech_like: bool,
    pub reference_ok: bool,
}

/// Frozen wake audio passed from wake backend to realtime voice.
#[derive(Clone, Debug)]
pub struct WakeAudioHandoff {
    pub id: u32,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub pre_roll_ms: u32,
    pub post_wake_ms: u32,
    pub pcm_le_bytes: Arc<ByteBuffer>,
    pub acoustic: WakeAcousticSnapshot,
}

impl WakeAudioHandoff {
    pub fn input_audio_ms(&self) -> u128 {
        pcm_bytes_to_ms(self.pcm_le_bytes.len(), self.sample_rate_hz)
    }

    pub fn is_empty(&self) -> bool {
        self.pcm_le_bytes.is_empty()
    }
}

/// Fixed-capacity PCM pre-roll ring. The backing is a zeroed `ByteBuffer`, so
/// large rings follow the same external-preferred allocation path as other ESP
/// large buffers.
pub struct WakePreRollRing {
    sample_rate_hz: u32,
    capacity_bytes: usize,
    len_bytes: usize,
    write_pos: usize,
    storage: ByteBuffer,
}

impl WakePreRollRing {
    pub fn new(sample_rate_hz: u32) -> Self {
        let sample_rate_hz = sample_rate_hz.max(8_000);
        let capacity_bytes = pre_roll_capacity_bytes(sample_rate_hz);
        Self {
            sample_rate_hz,
            capacity_bytes,
            len_bytes: 0,
            write_pos: 0,
            storage: ByteBuffer::zeroed(capacity_bytes),
        }
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn len_bytes(&self) -> usize {
        self.len_bytes
    }

    pub fn is_external_preferred(&self) -> bool {
        self.storage.is_external_preferred()
    }

    pub fn record_mic_frame(&mut self, pcm: &[i16], sample_rate_hz: u32) {
        let sample_rate_hz = sample_rate_hz.max(8_000);
        if sample_rate_hz != self.sample_rate_hz {
            *self = Self::new(sample_rate_hz);
        }
        for sample in pcm {
            self.write_bytes(&sample.to_le_bytes());
        }
    }

    pub fn freeze(&self, acoustic: WakeAcousticSnapshot) -> WakeAudioHandoff {
        let mut bytes = ByteBuffer::with_capacity(self.len_bytes.max(1));
        if self.len_bytes > 0 {
            if self.len_bytes < self.capacity_bytes {
                bytes
                    .write_all(&self.storage.as_slice()[..self.len_bytes])
                    .expect("write wake pre-roll");
            } else {
                let (tail, head) = self.storage.as_slice().split_at(self.write_pos);
                bytes.write_all(head).expect("write wake pre-roll head");
                bytes.write_all(tail).expect("write wake pre-roll tail");
            }
        }
        WakeAudioHandoff {
            id: next_handoff_id(),
            sample_rate_hz: self.sample_rate_hz,
            channels: MONO_CHANNELS,
            pre_roll_ms: pcm_bytes_to_ms(self.len_bytes, self.sample_rate_hz) as u32,
            post_wake_ms: 0,
            pcm_le_bytes: Arc::new(bytes),
            acoustic,
        }
    }

    fn write_bytes(&mut self, bytes: &[u8; 2]) {
        let storage = self.storage.as_mut_slice();
        for byte in bytes {
            storage[self.write_pos] = *byte;
            self.write_pos = (self.write_pos + 1) % self.capacity_bytes;
            self.len_bytes = self.len_bytes.saturating_add(1).min(self.capacity_bytes);
        }
    }
}

fn pre_roll_capacity_bytes(sample_rate_hz: u32) -> usize {
    let samples = (sample_rate_hz.max(8_000) as usize) * (PRE_ROLL_MS as usize) / 1000;
    samples.saturating_mul(std::mem::size_of::<i16>())
}

fn next_handoff_id() -> u32 {
    let next = HANDOFF_ID.fetch_add(1, Ordering::Relaxed);
    if next == 0 {
        HANDOFF_ID.fetch_add(1, Ordering::Relaxed)
    } else {
        next
    }
}

fn pcm_bytes_to_ms(bytes: usize, sample_rate_hz: u32) -> u128 {
    if sample_rate_hz == 0 {
        return 0;
    }
    let samples = bytes / std::mem::size_of::<i16>();
    ((samples as u128) * 1000) / (sample_rate_hz as u128)
}

#[derive(Default)]
struct WakeHandoffRuntime {
    ring: Option<WakePreRollRing>,
}

fn runtime() -> &'static Mutex<WakeHandoffRuntime> {
    static RUNTIME: OnceLock<Mutex<WakeHandoffRuntime>> = OnceLock::new();
    RUNTIME.get_or_init(|| Mutex::new(WakeHandoffRuntime::default()))
}

pub fn record_mic_frame(pcm: &[i16], sample_rate_hz: u32) {
    if pcm.is_empty() {
        return;
    }
    let mut guard = runtime().lock().unwrap_or_else(|e| e.into_inner());
    let ring = guard
        .ring
        .get_or_insert_with(|| WakePreRollRing::new(sample_rate_hz));
    ring.record_mic_frame(pcm, sample_rate_hz);
}

pub fn freeze_current_handoff(acoustic: WakeAcousticSnapshot) -> Option<WakeAudioHandoff> {
    let guard = runtime().lock().unwrap_or_else(|e| e.into_inner());
    let handoff = guard.ring.as_ref()?.freeze(acoustic);
    if handoff.is_empty() {
        return None;
    }
    Some(handoff)
}

pub fn reset_current_handoff() {
    let mut guard = runtime().lock().unwrap_or_else(|e| e.into_inner());
    guard.ring = None;
}

pub fn reset_after_session() {
    reset_current_handoff();
}

pub fn shutdown_handoff() {
    reset_current_handoff();
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    reset_after_session();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_only_recent_two_seconds() {
        let mut ring = WakePreRollRing::new(8_000);
        let samples = vec![1i16; 8_000 * 3];

        ring.record_mic_frame(&samples, 8_000);

        assert_eq!(ring.len_bytes(), 8_000 * 2 * 2);
        let handoff = ring.freeze(WakeAcousticSnapshot::default());
        assert_eq!(handoff.pre_roll_ms, 2_000);
        assert_eq!(handoff.pcm_le_bytes.len(), 8_000 * 2 * 2);
    }

    #[test]
    fn freeze_contains_pcm_bytes_and_sample_rate() {
        let mut ring = WakePreRollRing::new(16_000);
        ring.record_mic_frame(&[0x1234, -2], 16_000);

        let handoff = ring.freeze(WakeAcousticSnapshot {
            mic_level_pm: 7,
            ..WakeAcousticSnapshot::default()
        });

        assert_eq!(handoff.sample_rate_hz, 16_000);
        assert_eq!(handoff.channels, 1);
        assert_eq!(handoff.acoustic.mic_level_pm, 7);
        assert_eq!(handoff.pcm_le_bytes.as_slice(), &[0x34, 0x12, 0xfe, 0xff]);
    }

    #[test]
    fn large_pre_roll_is_external_preferred() {
        let ring = WakePreRollRing::new(16_000);

        assert!(ring.is_external_preferred());
    }

    #[test]
    fn runtime_reset_drops_old_handoff_ring() {
        reset_for_tests();
        record_mic_frame(&[1, 2, 3, 4], 16_000);
        let before = freeze_current_handoff(WakeAcousticSnapshot::default()).expect("handoff");
        assert!(!before.is_empty());

        reset_after_session();
        assert!(freeze_current_handoff(WakeAcousticSnapshot::default()).is_none());
    }
}
