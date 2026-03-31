//! 百度 STT（REST）。
//! Baidu STT REST client.

use super::baidu_token::BaiduTokenCache;
use crate::config::AudioSttConfig;
use crate::error::{Error, Result};
use crate::platform::{PlatformHttpClient, ResponseBody};
use base64::Engine;
use serde_json::Value;
use std::io::Write;

const BAIDU_STT_DEFAULT_URL: &str = "https://vop.baidu.com/server_api";

pub fn transcribe_pcm16(
    http: &mut dyn PlatformHttpClient,
    token_cache: &BaiduTokenCache,
    stt: &AudioSttConfig,
    pcm16le: &[u8],
    sample_rate: u32,
) -> Result<String> {
    let token = token_cache.get_or_fetch(http, &stt.api_key, &stt.api_secret)?;
    let speech_b64 = base64::engine::general_purpose::STANDARD.encode(pcm16le);
    let dev_pid = stt.model.trim().parse::<u32>().unwrap_or(1537);
    let body = serde_json::json!({
        "format": "pcm",
        "rate": sample_rate,
        "channel": 1,
        "cuid": "beetle",
        "token": token,
        "speech": speech_b64,
        "len": pcm16le.len(),
        "dev_pid": dev_pid,
    })
    .to_string();
    let api_url = if stt.api_url.trim().is_empty() {
        BAIDU_STT_DEFAULT_URL
    } else {
        stt.api_url.trim()
    };
    let headers = [("Content-Type", "application/json")];
    let (status, body_buf) = http
        .post(api_url, &headers, body.as_bytes())
        .map_err(|e| Error::config("stt_baidu_request", e.to_string()))?;
    parse_asr_response(status, body_buf)
}

pub fn transcribe_pcm16_samples(
    http: &mut dyn PlatformHttpClient,
    token_cache: &BaiduTokenCache,
    stt: &AudioSttConfig,
    pcm16: &[i16],
    sample_rate: u32,
) -> Result<String> {
    let token = token_cache.get_or_fetch(http, &stt.api_key, &stt.api_secret)?;
    let dev_pid = stt.model.trim().parse::<u32>().unwrap_or(1537);
    let pcm_byte_len = pcm16.len() * 2;

    let prefix = format!(
        r#"{{"format":"pcm","rate":{},"channel":1,"cuid":"beetle","token":"{}","speech":""#,
        sample_rate, token
    );
    let suffix = format!(r#"","len":{},"dev_pid":{}}}"#, pcm_byte_len, dev_pid);

    let b64_output_len = (pcm_byte_len * 4).div_ceil(3) + 4;
    let total_cap = prefix.len() + b64_output_len + suffix.len();
    let mut body = crate::platform::psram_vec::PsramVec::<u8>::with_max_capacity(total_cap);

    body.extend_from_slice(prefix.as_bytes());
    {
        let mut encoder = base64::write::EncoderWriter::new(
            &mut body,
            &base64::engine::general_purpose::STANDARD,
        );
        let mut staging = [0u8; 1024];
        let mut cursor = 0usize;
        for sample in pcm16 {
            let bytes = sample.to_le_bytes();
            staging[cursor] = bytes[0];
            staging[cursor + 1] = bytes[1];
            cursor += 2;
            if cursor == staging.len() {
                encoder
                    .write_all(&staging)
                    .map_err(|e| Error::config("stt_baidu_encode", e.to_string()))?;
                cursor = 0;
            }
        }
        if cursor > 0 {
            encoder
                .write_all(&staging[..cursor])
                .map_err(|e| Error::config("stt_baidu_encode", e.to_string()))?;
        }
        encoder
            .finish()
            .map_err(|e| Error::config("stt_baidu_encode", e.to_string()))?;
    }
    body.extend_from_slice(suffix.as_bytes());

    let api_url = if stt.api_url.trim().is_empty() {
        BAIDU_STT_DEFAULT_URL
    } else {
        stt.api_url.trim()
    };
    let headers = [("Content-Type", "application/json")];
    let (status, body_buf) = http
        .post(api_url, &headers, body.as_slice())
        .map_err(|e| Error::config("stt_baidu_request", e.to_string()))?;
    parse_asr_response(status, body_buf)
}

fn parse_asr_response(status: u16, body: ResponseBody) -> Result<String> {
    if status != 200 {
        return Err(Error::config(
            "stt_baidu_request",
            format!("asr http status {}", status),
        ));
    }
    let v: Value = serde_json::from_slice(body.as_slice())
        .map_err(|e| Error::config("stt_baidu_parse", e.to_string()))?;
    let err_no = v.get("err_no").and_then(|x| x.as_i64()).unwrap_or(-1);
    if err_no != 0 {
        let err_msg = v
            .get("err_msg")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown asr error");
        return Err(Error::config(
            "stt_baidu_request",
            format!("baidu asr error {}: {}", err_no, err_msg),
        ));
    }
    let first = v
        .get("result")
        .and_then(|x| x.as_array())
        .and_then(|arr| arr.first())
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::config("stt_baidu_parse", "missing result text"))?;
    Ok(first.trim().to_string())
}
