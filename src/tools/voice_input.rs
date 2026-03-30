//! voice_input：采集麦克风 PCM，能量断句后调用百度 STT。

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::energy::{EndpointConfig, EndpointEvent, EndpointState};
use crate::audio::stt_baidu;
use crate::config::AudioSegment;
use crate::constants::{
    AUDIO_CAPTURE_FRAME_SAMPLES, AUDIO_CAPTURE_MAX_MS, AUDIO_STT_MAX_PCM_BYTES,
};
use crate::error::{Error, Result};
use crate::tools::http_bridge::ToolContextHttpClient;
use crate::tools::{parse_tool_args, Tool, ToolContext};
use crate::Platform;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

/// RAII guard：创建时设 orchestrator 录音标志，Drop 时清除。
/// 确保无论正常返回还是 `?` 早退，标志都会被清除。
struct AudioRecordingGuard;

impl AudioRecordingGuard {
    fn new() -> Self {
        crate::orchestrator::set_audio_recording(true);
        Self
    }
}

impl Drop for AudioRecordingGuard {
    fn drop(&mut self) {
        crate::orchestrator::set_audio_recording(false);
    }
}

pub struct VoiceInputTool {
    platform: Arc<dyn Platform>,
    audio_cfg: AudioSegment,
    baidu_token: Arc<BaiduTokenCache>,
}

impl VoiceInputTool {
    pub fn new(
        platform: Arc<dyn Platform>,
        audio_cfg: AudioSegment,
        baidu_token: Arc<BaiduTokenCache>,
    ) -> Self {
        Self {
            platform,
            audio_cfg,
            baidu_token,
        }
    }
}

impl Tool for VoiceInputTool {
    fn name(&self) -> &'static str {
        "voice_input"
    }

    fn description(&self) -> &'static str {
        "Listen through the device microphone and transcribe speech to text. Use when user asks to listen/hear/record voice."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "max_ms": { "type": "integer", "description": "Max capture milliseconds, default 12000" }
            }
        })
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let result = (|| {
            log_audio_resource_snapshot("voice_input_start");
            if !self.platform.audio_mic_ready() {
                return Err(Error::config("tool_voice_input", "microphone not ready"));
            }
            let obj = parse_tool_args(args, "tool_voice_input")?;
            let max_ms = obj
                .get("max_ms")
                .and_then(|x| x.as_u64())
                .map(|v| v.min(AUDIO_CAPTURE_MAX_MS as u64) as u32)
                .unwrap_or(AUDIO_CAPTURE_MAX_MS);

            let mut http = ToolContextHttpClient::new(ctx);
            let mic_sr = self.audio_cfg.microphone.sample_rate.max(8_000);
            let frame_ms =
                ((AUDIO_CAPTURE_FRAME_SAMPLES as u64) * 1000 / (mic_sr as u64)).clamp(1, 40) as u32;
            let endpoint_cfg = EndpointConfig {
                threshold: if self.audio_cfg.vad.enabled {
                    self.audio_cfg.vad.threshold
                } else {
                    0.08
                },
                silence_duration_ms: if self.audio_cfg.vad.enabled {
                    self.audio_cfg.vad.silence_duration_ms
                } else {
                    1200
                },
            };
            let mut endpoint = EndpointState::new();
            let mut frame = [0i16; AUDIO_CAPTURE_FRAME_SAMPLES];
            let mut started = false;
            let mut elapsed = 0u32;
            let mut dbg_next_log_ms = 0u32;
            let debug_enabled = log::log_enabled!(log::Level::Debug);
            let max_pcm_samples = (max_ms as usize)
                .saturating_mul(mic_sr as usize)
                .min(AUDIO_STT_MAX_PCM_BYTES)
                / 1000;
            // Start small (~2s worth), grow on demand; avoids 384KB upfront spike.
            let init_cap = (2 * mic_sr as usize).min(max_pcm_samples);
            let mut captured: Vec<i16> = Vec::with_capacity(init_cap);
            let _recording_guard = AudioRecordingGuard::new();
            let capture_start = Instant::now();
            while elapsed < max_ms {
                let n = self.platform.read_mic_pcm_i16(&mut frame)?;
                if n == 0 {
                    elapsed = elapsed.saturating_add(frame_ms);
                    continue;
                }
                let chunk = &frame[..n.min(frame.len())];
                if debug_enabled && elapsed >= dbg_next_log_ms {
                    let rms = crate::audio::energy::normalized_rms(chunk);
                    let (mn, mx) = chunk
                        .iter()
                        .fold((i16::MAX, i16::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                    log::debug!(
                        "[voice_input] t={}ms samples={} rms={:.5} min={} max={} thr={:.3}",
                        elapsed,
                        n,
                        rms,
                        mn,
                        mx,
                        endpoint_cfg.threshold
                    );
                    dbg_next_log_ms = elapsed.saturating_add(1000);
                }
                match endpoint.update(chunk, frame_ms, &endpoint_cfg) {
                    EndpointEvent::SpeechStart => {
                        started = true;
                        captured.extend_from_slice(chunk);
                    }
                    EndpointEvent::SpeechEnd => break,
                    EndpointEvent::None => {
                        if started {
                            captured.extend_from_slice(chunk);
                        }
                    }
                }
                if captured.len() * 2 >= AUDIO_STT_MAX_PCM_BYTES {
                    break;
                }
                elapsed = elapsed.saturating_add(frame_ms);
            }
            crate::metrics::record_voice_input_capture_ms(capture_start.elapsed().as_millis());
            if captured.is_empty() {
                return Err(Error::config(
                    "tool_voice_input",
                    "no speech captured within time window",
                ));
            }
            let stt_start = Instant::now();
            let text = stt_baidu::transcribe_pcm16_samples(
                &mut http,
                self.baidu_token.as_ref(),
                &self.audio_cfg.stt,
                &captured,
                mic_sr,
            )?;
            crate::metrics::record_voice_input_stt_http_ms(stt_start.elapsed().as_millis());
            log_audio_resource_snapshot("voice_input_done");
            Ok(text)
        })();
        if result.is_err() {
            crate::metrics::record_voice_tool_failure("voice_input");
        }
        result
    }
}

fn log_audio_resource_snapshot(stage: &str) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let snap = crate::orchestrator::snapshot();
    log::debug!(
        "[tool_voice_input] {} heap_internal={} heap_spiram={} heap_largest={} pressure={:?}",
        stage,
        snap.heap_free_internal,
        snap.heap_free_spiram,
        snap.heap_largest_block_internal,
        snap.pressure
    );
}
