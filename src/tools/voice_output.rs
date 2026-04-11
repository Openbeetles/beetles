//! voice_output：调用百度 TTS 并播放到喇叭。

use crate::audio::baidu_token::BaiduTokenCache;
use crate::audio::pipeline::speak_text;
use crate::config::AudioSegment;
use crate::constants::AUDIO_TTS_MAX_TEXT_LEN;
use crate::error::{Error, Result};
use crate::tools::http_bridge::ToolContextHttpClient;
use crate::tools::{parse_tool_args, Tool, ToolCapabilityContract, ToolContext, ToolMetadata};
use crate::Platform;
use serde_json::{json, Map, Value};
use std::sync::Arc;

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

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"text":{"type":"string","description":"Text to speak"}},"required":["text"]}"#
    }

    fn requires_network(&self) -> bool {
        true
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let result = (|| {
            log_audio_resource_snapshot("voice_output_start");
            let duplex_caps = self.platform.audio_duplex_capabilities();
            if !duplex_caps.has_speaker_output() {
                return Err(Error::config(
                    "tool_voice_output",
                    format!(
                        "speaker unavailable under audio contract profile={}",
                        duplex_caps.profile().as_str()
                    ),
                ));
            }
            let text = parse_voice_output_text(args)?;
            let segments = split_voice_output_segments(&text, AUDIO_TTS_MAX_TEXT_LEN);
            if segments.is_empty() {
                return Err(Error::config("tool_voice_output", "text must not be empty"));
            }
            let mut http = ToolContextHttpClient::new(ctx);
            let mut played_samples = 0usize;
            let mut tts_http_ms = 0u128;
            let mut play_ms = 0u128;
            for segment in &segments {
                let playback = speak_text(
                    self.platform.as_ref(),
                    &self.audio_cfg,
                    self.baidu_token.as_ref(),
                    &mut http,
                    segment.as_str(),
                )?;
                played_samples = played_samples.saturating_add(playback.played_samples);
                tts_http_ms = tts_http_ms.saturating_add(playback.tts_http_ms);
                play_ms = play_ms.saturating_add(playback.play_ms);
            }
            crate::metrics::record_voice_output_tts_http_ms(tts_http_ms);
            crate::metrics::record_voice_output_play_ms(play_ms);
            log_audio_resource_snapshot("voice_output_done");
            Ok(json!({
                "ok": true,
                "played_samples": played_samples,
                "segments": segments.len()
            })
            .to_string())
        })();
        if result.is_err() {
            crate::metrics::record_voice_tool_failure("voice_output");
        }
        result
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task().with_system_ingress(false)
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[
            crate::orchestrator::RUNTIME_CAPABILITY_AUDIO_OUTPUT,
            crate::orchestrator::RUNTIME_CAPABILITY_NETWORK_OUTBOUND_HTTP,
        ])
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

fn split_voice_output_segments(text: &str, max_bytes: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || max_bytes == 0 {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut last_soft_break: Option<usize> = None;

    for ch in trimmed.chars() {
        let ch_len = ch.len_utf8();
        if !current.is_empty() && current.len().saturating_add(ch_len) > max_bytes {
            if let Some(idx) = last_soft_break.take() {
                let tail = current[idx..].trim().to_string();
                current.truncate(idx);
                let head = current.trim().to_string();
                if !head.is_empty() {
                    segments.push(head);
                }
                current.clear();
                if !tail.is_empty() {
                    current.push_str(&tail);
                }
            } else {
                let head = current.trim().to_string();
                if !head.is_empty() {
                    segments.push(head);
                }
                current.clear();
            }
        }

        if !current.is_empty() || !ch.is_whitespace() {
            current.push(ch);
            if matches!(
                ch,
                '，' | '。' | '！' | '？' | ',' | '.' | '!' | '?' | ';' | '；' | '\n'
            ) {
                last_soft_break = Some(current.len());
            }
        }
    }

    let tail = current.trim();
    if !tail.is_empty() {
        segments.push(tail.to_string());
    }
    segments
}

#[cfg(test)]
mod parse_tests {
    use super::{parse_voice_output_text, split_voice_output_segments};
    use crate::constants::AUDIO_TTS_MAX_TEXT_LEN;

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

    #[test]
    fn splits_long_voice_output_text_into_tts_sized_segments() {
        let text = "第一句很长，需要分段朗读。第二句也很长，需要继续分段朗读。第三句还是很长，需要继续分段朗读。";
        let segments = split_voice_output_segments(text, 24);

        assert!(segments.len() > 1);
        assert!(segments.iter().all(|segment| !segment.trim().is_empty()));
        assert!(segments.iter().all(|segment| segment.len() <= 24));
        assert_eq!(segments.concat(), text);
    }

    #[test]
    fn keeps_short_voice_output_text_as_single_segment() {
        let text = "你好世界";
        let segments = split_voice_output_segments(text, AUDIO_TTS_MAX_TEXT_LEN);

        assert_eq!(segments, vec!["你好世界".to_string()]);
    }
}
