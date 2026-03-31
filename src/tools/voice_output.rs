//! voice_output：调用百度 TTS 并播放到喇叭。

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::tts_baidu;
use crate::config::AudioSegment;
use crate::constants::{AUDIO_TTS_MAX_TEXT_LEN, AUDIO_TTS_WRITE_CHUNK_SAMPLES};
use crate::error::{Error, Result};
use crate::tools::http_bridge::ToolContextHttpClient;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Instant;

pub struct VoiceOutputTool {
    platform: Arc<dyn Platform>,
    audio_cfg: AudioSegment,
    baidu_token: Arc<BaiduTokenCache>,
}

impl VoiceOutputTool {
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

impl Tool for VoiceOutputTool {
    fn name(&self) -> &'static str {
        "voice_output"
    }

    fn description(&self) -> &'static str {
        "Speak text aloud through the device speaker (text-to-speech). Use when user asks to say/speak/read something aloud."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "Text to speak" }
            },
            "required": ["text"]
        })
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let result = (|| {
            log_audio_resource_snapshot("voice_output_start");
            if !self.platform.audio_speaker_ready() {
                return Err(Error::config("tool_voice_output", "speaker not ready"));
            }
            let text = parse_voice_output_text(args)?;
            if text.len() > AUDIO_TTS_MAX_TEXT_LEN {
                return Err(Error::config(
                    "tool_voice_output",
                    format!("text too long (max {})", AUDIO_TTS_MAX_TEXT_LEN),
                ));
            }
            let mut http = ToolContextHttpClient::new(ctx);
            let tts_start = Instant::now();
            let mut first_pcm_at: Option<Instant> = None;
            let played_samples = match tts_baidu::stream_wav_pcm16le(
                &mut http,
                self.baidu_token.as_ref(),
                &self.audio_cfg.stt,
                &self.audio_cfg.tts,
                text.as_str(),
                AUDIO_TTS_WRITE_CHUNK_SAMPLES,
                |chunk| {
                    if first_pcm_at.is_none() {
                        first_pcm_at = Some(Instant::now());
                        crate::orchestrator::set_audio_playing(true);
                        log_audio_resource_snapshot("voice_output_first_pcm");
                    }
                    self.platform.write_speaker_pcm_i16(chunk)
                },
            ) {
                Ok(samples) => samples,
                Err(e) if e.stage() == "tts_baidu_wav" => {
                    log::warn!(
                        "[tool_voice_output] streaming wav parse failed, fallback to buffered path: {}",
                        e
                    );
                    log_audio_resource_snapshot("voice_output_fallback_start");
                    let wav = tts_baidu::synthesize_wav(
                        &mut http,
                        self.baidu_token.as_ref(),
                        &self.audio_cfg.stt,
                        &self.audio_cfg.tts,
                        text.as_str(),
                    )?;
                    tts_baidu::play_wav_pcm16le_chunks(
                        &wav,
                        AUDIO_TTS_WRITE_CHUNK_SAMPLES,
                        |chunk| {
                            if first_pcm_at.is_none() {
                                first_pcm_at = Some(Instant::now());
                                crate::orchestrator::set_audio_playing(true);
                                log_audio_resource_snapshot("voice_output_first_pcm_fallback");
                            }
                            self.platform.write_speaker_pcm_i16(chunk)
                        },
                    )?
                }
                Err(e) => return Err(e),
            };
            crate::metrics::record_voice_output_tts_http_ms(tts_start.elapsed().as_millis());
            let play_ms = first_pcm_at.map(|t| t.elapsed().as_millis()).unwrap_or(0);
            crate::metrics::record_voice_output_play_ms(play_ms);
            log_audio_resource_snapshot("voice_output_done");
            crate::orchestrator::set_audio_playing(false);
            Ok(json!({
                "ok": true,
                "played_samples": played_samples
            })
            .to_string())
        })();
        if result.is_err() {
            crate::orchestrator::set_audio_playing(false);
            crate::metrics::record_voice_tool_failure("voice_output");
        }
        result
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_system_ingress(false)
    }
}

/// 解析 `voice_output` 的 `text` 参数。优先标准 JSON；兼容 LLM 常犯的「键未加引号」、整段纯 JSON 字符串。
fn parse_voice_output_text(args: &str) -> Result<String> {
    const STAGE: &str = "tool_voice_output";
    let trimmed = args.trim();

    let strict = parse_tool_args(trimmed, STAGE);
    if let Ok(ref obj) = strict {
        if let Some(t) = text_from_tool_obj(obj) {
            return Ok(t);
        }
    }

    if let Ok(s) = serde_json::from_str::<String>(trimmed) {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }

    if let Some(text) = extract_bare_value_from_js_object(trimmed) {
        return Ok(text);
    }

    // 终极兜底：任何非空纯文本直接当作要朗读的内容
    if !trimmed.is_empty()
        && !trimmed.starts_with('{')
        && !trimmed.starts_with('[')
        && !trimmed.starts_with('"')
    {
        return Ok(trimmed.to_string());
    }

    match strict {
        Ok(_) => Err(Error::config(
            STAGE,
            "missing non-empty \"text\" field (expected JSON object with string \"text\")",
        )),
        Err(e) => Err(e),
    }
}

fn text_from_tool_obj(obj: &Map<String, Value>) -> Option<String> {
    for k in ["text", "content", "message"] {
        if let Some(t) = obj
            .get(k)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(t.to_string());
        }
    }
    None
}

/// 提取 `{text: ...}` / `{content: ...}` 中冒号后的裸文本值。
/// 处理模型返回键和值均无引号的 JS 风格对象：`{text: 你好}` → `"你好"`。
fn extract_bare_value_from_js_object(s: &str) -> Option<String> {
    let t = s.trim();
    if !t.starts_with('{') || !t.ends_with('}') {
        return None;
    }
    let inner = t[1..t.len() - 1].trim();
    let (key, rest) = split_unquoted_key_and_rest(inner)?;
    if !matches!(key, "text" | "content" | "message") {
        return None;
    }
    // rest 就是裸文本值，去掉可能的首尾引号
    let val = rest.trim().trim_matches('"').trim();
    if val.is_empty() {
        return None;
    }
    Some(val.to_string())
}

fn split_unquoted_key_and_rest(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let key = &s[..i];
    let rest = s[i..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    Some((key, rest))
}

fn log_audio_resource_snapshot(stage: &str) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let snap = crate::orchestrator::snapshot();
    log::debug!(
        "[tool_voice_output] {} heap_internal={} heap_spiram={} heap_largest={} pressure={:?}",
        stage,
        snap.heap_free_internal,
        snap.heap_free_spiram,
        snap.heap_largest_block_internal,
        snap.pressure
    );
}

#[cfg(test)]
mod parse_tests {
    use super::parse_voice_output_text;

    #[test]
    fn accepts_strict_json() {
        let t = parse_voice_output_text(r#"{"text":"你好"}"#).expect("ok");
        assert_eq!(t, "你好");
    }

    #[test]
    fn accepts_unquoted_key_quoted_value() {
        let t = parse_voice_output_text(r#"{text: "hello"}"#).expect("ok");
        assert_eq!(t, "hello");
    }

    #[test]
    fn accepts_unquoted_key_and_value() {
        // 模型实际输出的格式：键和值都没引号
        let t = parse_voice_output_text("{text: 你好，这是测试语音功能}").expect("ok");
        assert_eq!(t, "你好，这是测试语音功能");
    }

    #[test]
    fn accepts_json_string_body() {
        let t = parse_voice_output_text(r#""plain""#).expect("ok");
        assert_eq!(t, "plain");
    }

    #[test]
    fn accepts_bare_text_fallback() {
        let t = parse_voice_output_text("你好世界").expect("ok");
        assert_eq!(t, "你好世界");
    }
}
