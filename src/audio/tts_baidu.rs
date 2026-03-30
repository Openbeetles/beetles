//! 百度 TTS（REST）与 WAV(PCM16LE) 解析。
//! Baidu TTS REST and WAV(PCM16LE) parsing.
//!
//! OAuth `access_token` is fetched with **`stt` credentials** (`api_key` / `api_secret`); Baidu uses one app key for both ASR and TTS.
//! 鉴权与 STT 相同：使用配置里 `stt` 的密钥换 token，非独立 TTS 密钥。

use super::baidu_token::BaiduTokenCache;
use crate::config::{AudioSttConfig, AudioTtsConfig};
use crate::error::{Error, Result};
use crate::platform::{PlatformHttpClient, ResponseBody};
use std::fmt::Write as _;
use std::time::Instant;

const BAIDU_TTS_URL: &str = "https://tsn.baidu.com/text2audio";
const TTS_ERROR_PREVIEW_MAX_BYTES: usize = 1024;
/// 单次 TTS 流式下载最长时间（秒），防止无限等待。
const TTS_STREAM_TIMEOUT_SECS: u64 = 90;
/// TTS 最大样本数（5 分钟 @ 16kHz），防止无限长音频。
const MAX_TTS_SAMPLES: usize = 16_000 * 300;

pub fn synthesize_wav(
    http: &mut dyn PlatformHttpClient,
    token_cache: &BaiduTokenCache,
    stt: &AudioSttConfig,
    tts: &AudioTtsConfig,
    text: &str,
) -> Result<ResponseBody> {
    let token = token_cache.get_or_fetch(http, &stt.api_key, &stt.api_secret)?;
    let body = build_tts_form_body(tts, text, &token);
    let headers = [("Content-Type", "application/x-www-form-urlencoded")];
    let (status, resp) = http
        .post(BAIDU_TTS_URL, &headers, body.as_bytes())
        .map_err(|e| Error::config("tts_baidu_request", e.to_string()))?;
    if status != 200 {
        return Err(Error::config(
            "tts_baidu_request",
            format!("tts http status {}", status),
        ));
    }
    let bytes = resp.as_slice();
    if bytes.starts_with(b"{") {
        let msg = extract_tts_error_message(bytes);
        return Err(Error::config("tts_baidu_request", msg));
    }
    Ok(resp)
}

pub fn stream_wav_pcm16le<F>(
    http: &mut dyn PlatformHttpClient,
    token_cache: &BaiduTokenCache,
    stt: &AudioSttConfig,
    tts: &AudioTtsConfig,
    text: &str,
    chunk_samples: usize,
    mut on_pcm_chunk: F,
) -> Result<usize>
where
    F: FnMut(&[i16]) -> Result<()>,
{
    let token = token_cache.get_or_fetch(http, &stt.api_key, &stt.api_secret)?;
    let body = build_tts_form_body(tts, text, &token);
    let headers = [("Content-Type", "application/x-www-form-urlencoded")];
    let mut decoder = WavPcmStreamDecoder::new(chunk_samples.max(1));
    let mut text_preview = Vec::new();
    let mut text_response = false;
    let mut first_byte_seen = false;
    let stream_start = Instant::now();
    let status = http
        .post_streaming(
            BAIDU_TTS_URL,
            &headers,
            body.as_bytes(),
            None, // 无限制：边下载边播放，内存占用恒定
            &mut |chunk: &[u8]| -> Result<()> {
                if stream_start.elapsed().as_secs() > TTS_STREAM_TIMEOUT_SECS {
                    return Err(Error::config("tts_baidu_timeout", "stream timeout"));
                }
                if chunk.is_empty() {
                    return Ok(());
                }
                if !first_byte_seen {
                    first_byte_seen = true;
                    let b = chunk[0];
                    // JSON/HTML 错误页：不按 WAV 解析，先收集错误文本。
                    text_response = b == b'{' || b == b'<';
                }
                if text_response {
                    append_preview(&mut text_preview, chunk, TTS_ERROR_PREVIEW_MAX_BYTES);
                    return Ok(());
                }
                decoder.feed(chunk, &mut on_pcm_chunk)
            },
        )
        .map_err(|e| Error::config("tts_baidu_request", e.to_string()))?;

    if text_response {
        let msg = extract_tts_error_message(&text_preview);
        return Err(Error::config(
            "tts_baidu_request",
            format!("tts http status {}: {}", status, msg),
        ));
    }
    if status != 200 {
        return Err(Error::config(
            "tts_baidu_request",
            format!("tts http status {}", status),
        ));
    }
    decoder.finish()?;
    Ok(decoder.played_samples())
}

pub fn play_wav_pcm16le_chunks<F>(
    wav: &ResponseBody,
    chunk_samples: usize,
    mut on_chunk: F,
) -> Result<usize>
where
    F: FnMut(&[i16]) -> Result<()>,
{
    let bytes = wav.as_slice();
    let (start, data_len) = wav_data_chunk(bytes)?;
    let mut played = 0usize;
    let mut idx = start;
    let end = start + data_len;
    let mut chunk = crate::platform::psram_vec::PsramVec::<i16>::new(chunk_samples.max(1));
    while idx < end {
        let remain_bytes = end - idx;
        let take_samples = (remain_bytes / 2).min(chunk.len());
        if take_samples == 0 {
            break;
        }
        let src = &bytes[idx..idx + take_samples * 2];
        for (sample, pair) in chunk
            .as_mut_slice()
            .iter_mut()
            .take(take_samples)
            .zip(src.chunks_exact(2))
        {
            *sample = i16::from_le_bytes([pair[0], pair[1]]);
        }
        on_chunk(&chunk.as_mut_slice()[..take_samples])?;
        played += take_samples;
        idx += take_samples * 2;
    }
    Ok(played)
}

fn wav_data_chunk(wav: &[u8]) -> Result<(usize, usize)> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(Error::config("tts_baidu_wav", "invalid WAV header"));
    }
    let mut cursor = 12usize;
    let mut data_start = None;
    let mut data_len = 0usize;
    while cursor + 8 <= wav.len() {
        let chunk_id = &wav[cursor..cursor + 4];
        let chunk_len = u32::from_le_bytes([
            wav[cursor + 4],
            wav[cursor + 5],
            wav[cursor + 6],
            wav[cursor + 7],
        ]) as usize;
        cursor += 8;
        if cursor + chunk_len > wav.len() {
            return Err(Error::config("tts_baidu_wav", "corrupted WAV chunks"));
        }
        if chunk_id == b"data" {
            data_start = Some(cursor);
            data_len = chunk_len;
            break;
        }
        cursor += chunk_len;
    }
    let start = data_start.ok_or_else(|| Error::config("tts_baidu_wav", "missing data chunk"))?;
    if !data_len.is_multiple_of(2) {
        return Err(Error::config(
            "tts_baidu_wav",
            "PCM data length must be even",
        ));
    }
    Ok((start, data_len))
}

fn try_parse_wav_data_chunk_prefix(wav: &[u8]) -> Result<Option<(usize, usize)>> {
    if wav.len() < 12 {
        return Ok(None);
    }
    if &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(Error::config("tts_baidu_wav", "invalid WAV header"));
    }
    let mut cursor = 12usize;
    while cursor + 8 <= wav.len() {
        let chunk_id = &wav[cursor..cursor + 4];
        let chunk_len = u32::from_le_bytes([
            wav[cursor + 4],
            wav[cursor + 5],
            wav[cursor + 6],
            wav[cursor + 7],
        ]) as usize;
        cursor += 8;
        if chunk_id == b"data" {
            if !chunk_len.is_multiple_of(2) {
                return Err(Error::config(
                    "tts_baidu_wav",
                    "PCM data length must be even",
                ));
            }
            return Ok(Some((cursor, chunk_len)));
        }
        let padded = chunk_len + (chunk_len % 2);
        if cursor + padded > wav.len() {
            return Ok(None);
        }
        cursor += padded;
    }
    Ok(None)
}

fn build_tts_form_body(tts: &AudioTtsConfig, text: &str, token: &str) -> String {
    let per = tts.voice.trim().parse::<u32>().unwrap_or(0).min(4);
    let spd = map_speed_percent_to_baidu(&tts.rate);
    let pit = map_pitch_percent_to_baidu(&tts.pitch);
    let text_encoded = urlencoding::encode(text);
    let token_encoded = urlencoding::encode(token);
    let mut body = String::with_capacity(text_encoded.len() + token_encoded.len() + 64);
    let _ = write!(
        &mut body,
        "tex={}&tok={}&cuid=beetle&ctp=1&lan=zh&aue=6&per={}&spd={}&pit={}",
        text_encoded, token_encoded, per, spd, pit
    );
    body
}

fn append_preview(dst: &mut Vec<u8>, src: &[u8], max: usize) {
    if dst.len() >= max {
        return;
    }
    let remain = max - dst.len();
    let take = src.len().min(remain);
    dst.extend_from_slice(&src[..take]);
}

struct WavPcmStreamDecoder {
    pending: Vec<u8>,
    chunk: crate::platform::psram_vec::PsramVec<i16>,
    chunk_fill: usize,
    data_bytes_remaining: Option<usize>,
    played_samples: usize,
}

impl WavPcmStreamDecoder {
    fn new(chunk_samples: usize) -> Self {
        Self {
            pending: Vec::with_capacity(256),
            chunk: crate::platform::psram_vec::PsramVec::<i16>::new(chunk_samples),
            chunk_fill: 0,
            data_bytes_remaining: None,
            played_samples: 0,
        }
    }

    fn feed(&mut self, bytes: &[u8], on_chunk: &mut dyn FnMut(&[i16]) -> Result<()>) -> Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.pending.extend_from_slice(bytes);
        loop {
            if self.data_bytes_remaining.is_none() {
                match try_parse_wav_data_chunk_prefix(&self.pending)? {
                    Some((data_start, data_len)) => {
                        self.pending.drain(..data_start);
                        self.data_bytes_remaining = Some(data_len);
                    }
                    None => break,
                }
            }
            let rem = self.data_bytes_remaining.unwrap_or(0);
            if rem == 0 {
                break;
            }
            let available = self.pending.len().min(rem);
            let even = available & !1usize;
            if even == 0 {
                break;
            }
            self.consume_pcm_even_prefix(even, on_chunk)?;
            if self.played_samples > MAX_TTS_SAMPLES {
                return Err(Error::config("tts_baidu_wav", "audio too long (max 5 min)"));
            }
            self.pending.drain(..even);
            self.data_bytes_remaining = Some(rem - even);
            if rem == even {
                self.flush_chunk(on_chunk)?;
                break;
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        match self.data_bytes_remaining {
            None => Err(Error::config("tts_baidu_wav", "missing data chunk")),
            Some(0) => Ok(()),
            Some(_) => Err(Error::config("tts_baidu_wav", "truncated WAV stream")),
        }
    }

    fn played_samples(&self) -> usize {
        self.played_samples
    }

    fn consume_pcm_even_prefix(
        &mut self,
        bytes_len: usize,
        on_chunk: &mut dyn FnMut(&[i16]) -> Result<()>,
    ) -> Result<()> {
        let pcm = &self.pending[..bytes_len];
        for pair in pcm.chunks_exact(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]);
            self.chunk.as_mut_slice()[self.chunk_fill] = sample;
            self.chunk_fill += 1;
            if self.chunk_fill == self.chunk.len() {
                on_chunk(self.chunk.as_mut_slice())?;
                self.played_samples += self.chunk_fill;
                self.chunk_fill = 0;
            }
        }
        Ok(())
    }

    fn flush_chunk(&mut self, on_chunk: &mut dyn FnMut(&[i16]) -> Result<()>) -> Result<()> {
        if self.chunk_fill > 0 {
            on_chunk(&self.chunk.as_mut_slice()[..self.chunk_fill])?;
            self.played_samples += self.chunk_fill;
            self.chunk_fill = 0;
        }
        Ok(())
    }
}

fn map_speed_percent_to_baidu(rate: &str) -> u32 {
    map_percent_like(rate)
}

fn map_pitch_percent_to_baidu(pitch: &str) -> u32 {
    map_percent_like(pitch)
}

/// 将类似 +0% / -20% / +5Hz 映射到百度 0..15，基线 5。
fn map_percent_like(s: &str) -> u32 {
    let trimmed = s.trim();
    let mut num = String::new();
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '+' || ch == '-' {
            num.push(ch);
        }
    }
    let delta = num.parse::<i32>().unwrap_or(0);
    let mapped = 5 + delta / 10;
    mapped.clamp(0, 15) as u32
}

fn extract_tts_error_message(bytes: &[u8]) -> String {
    let parsed = serde_json::from_slice::<serde_json::Value>(bytes).ok();
    if let Some(v) = parsed {
        if let Some(msg) = v.get("err_msg").and_then(|x| x.as_str()) {
            return msg.to_string();
        }
        if let Some(msg) = v.get("message").and_then(|x| x.as_str()) {
            return msg.to_string();
        }
    }
    std::str::from_utf8(bytes)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| "unknown tts error".to_string())
}

#[cfg(test)]
mod tests {
    use super::{extract_tts_error_message, try_parse_wav_data_chunk_prefix, wav_data_chunk};

    #[test]
    fn wav_data_chunk_parses_minimal_pcm_wav() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(44u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(16u32).to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&(16000u32).to_le_bytes());
        wav.extend_from_slice(&(32000u32).to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(4u32).to_le_bytes());
        wav.extend_from_slice(&[0u8, 0, 1, 0]);

        let (start, len) = wav_data_chunk(&wav).expect("valid wav");
        assert_eq!(len, 4);
        assert_eq!(&wav[start..start + len], &[0u8, 0, 1, 0]);
    }

    #[test]
    fn wav_data_chunk_rejects_odd_data_length() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(43u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(16u32).to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&(16000u32).to_le_bytes());
        wav.extend_from_slice(&(32000u32).to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(3u32).to_le_bytes());
        wav.extend_from_slice(&[0u8, 1, 2]);

        assert!(wav_data_chunk(&wav).is_err());
    }

    #[test]
    fn wav_prefix_parser_waits_for_complete_chunk_header() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(100u32).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(16u32).to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(try_parse_wav_data_chunk_prefix(&wav).expect("ok").is_none());
    }

    #[test]
    fn extract_tts_error_message_prefers_json_fields() {
        let err_msg = br#"{"err_msg":"token expired","message":"fallback"}"#;
        assert_eq!(extract_tts_error_message(err_msg), "token expired");

        let only_message = br#"{"message":"bad request"}"#;
        assert_eq!(extract_tts_error_message(only_message), "bad request");
    }
}
