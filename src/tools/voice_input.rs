//! voice_input：采集麦克风 PCM，能量断句后调用百度 STT。

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::capture::{capture_speech, AudioRecordingGuard};
use crate::audio::stt_baidu;
use crate::config::AudioSegment;
use crate::constants::AUDIO_CAPTURE_MAX_MS;
use crate::error::{Error, Result};
use crate::tools::http_bridge::ToolContextHttpClient;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use std::sync::Arc;
use std::time::Instant;

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

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"max_ms":{"type":"integer","description":"Max capture milliseconds, default 12000"}}}"#
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

            let _recording_guard = AudioRecordingGuard::new();
            let captured = capture_speech(
                self.platform.as_ref(),
                &self.audio_cfg,
                max_ms,
                "voice_input",
            )?;

            let mic_sr = self.audio_cfg.microphone.sample_rate.max(8_000);
            let mut http = ToolContextHttpClient::new(ctx);
            let stt_start = Instant::now();
            let text = stt_baidu::transcribe_pcm16_samples(
                &mut http,
                self.baidu_token.as_ref(),
                &self.audio_cfg.stt,
                captured.as_slice(),
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

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_system_ingress(false)
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
