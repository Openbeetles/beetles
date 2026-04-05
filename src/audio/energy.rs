//! 轻量能量断句（不依赖 ESP-SR）。
//! Lightweight energy endpointing without ESP-SR.

/// 断句状态机参数（阈值取 0.0..=1.0）。
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub threshold: f32,
    pub silence_duration_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointEvent {
    None,
    SpeechStart,
    SpeechEnd,
}

/// 按帧更新断句状态。
#[derive(Debug, Clone)]
pub struct EndpointState {
    in_speech: bool,
    silence_acc_ms: u32,
}

impl EndpointState {
    pub fn new() -> Self {
        Self {
            in_speech: false,
            silence_acc_ms: 0,
        }
    }

    /// 输入 PCM i16 单声道帧与帧时长，输出状态事件。
    pub fn update(&mut self, pcm: &[i16], frame_ms: u32, cfg: &EndpointConfig) -> EndpointEvent {
        let e = normalized_rms(pcm);
        if e >= cfg.threshold {
            self.silence_acc_ms = 0;
            if !self.in_speech {
                self.in_speech = true;
                return EndpointEvent::SpeechStart;
            }
            return EndpointEvent::None;
        }
        if !self.in_speech {
            return EndpointEvent::None;
        }
        self.silence_acc_ms = self.silence_acc_ms.saturating_add(frame_ms);
        if self.silence_acc_ms >= cfg.silence_duration_ms {
            self.in_speech = false;
            self.silence_acc_ms = 0;
            EndpointEvent::SpeechEnd
        } else {
            EndpointEvent::None
        }
    }

    /// 强制清空断句状态，供会话层在长时间卡住时收口当前语音窗。
    pub fn reset(&mut self) {
        self.in_speech = false;
        self.silence_acc_ms = 0;
    }
}

impl Default for EndpointState {
    fn default() -> Self {
        Self::new()
    }
}

/// 计算归一化 RMS（0.0..=1.0）。
pub fn normalized_rms(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let mut sum_sq: f64 = 0.0;
    for &v in pcm {
        let x = (v as f64) / 32768.0;
        sum_sq += x * x;
    }
    let rms = (sum_sq / (pcm.len() as f64)).sqrt();
    rms.clamp(0.0, 1.0) as f32
}
