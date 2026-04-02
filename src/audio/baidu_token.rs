//! 百度 OAuth access_token 获取与进程内缓存（STT/TTS 共用）。
//! Fetch and in-process cache for Baidu OAuth access_token (shared by STT/TTS).

use crate::error::{Error, Result};
use crate::platform::PlatformHttpClient;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BAIDU_TOKEN_URL: &str = "https://aip.baidubce.com/oauth/2.0/token";
/// 在官方 `expires_in` 之前提前刷新，避免边界时刻 401。
const EXPIRY_SAFETY_SECS: u64 = 120;
/// 响应缺少 `expires_in` 时的默认 TTL（约 30 天，与百度常见返回值一致）。
const DEFAULT_EXPIRES_IN_SECS: u64 = 2_592_000;

/// 按 `api_key` 维度缓存；更换 key 会重新拉取（secret 轮换在 token 未过期前仍用旧 token，与百度侧一致）。
#[derive(Default)]
pub struct BaiduTokenCache {
    state: Mutex<Option<CachedState>>,
}

struct CachedState {
    api_key: String,
    token: String,
    expires_at: Instant,
}

#[derive(serde::Deserialize)]
struct BaiduTokenSuccessResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(serde::Deserialize)]
struct BaiduTokenErrorResponse {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl BaiduTokenCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 返回有效 access_token，必要时向百度换取新 token。
    pub fn get_or_fetch(
        &self,
        http: &mut dyn PlatformHttpClient,
        api_key: &str,
        api_secret: &str,
    ) -> Result<String> {
        let key = trim_credential(api_key);
        let secret = trim_credential(api_secret);
        if key.is_empty() || secret.is_empty() {
            return Err(Error::config(
                "baidu_token",
                "api_key/api_secret is required",
            ));
        }

        let now = Instant::now();
        {
            let guard = self
                .state
                .lock()
                .map_err(|e| Error::config("baidu_token_lock", format!("mutex lock: {}", e)))?;
            if let Some(ref c) = *guard {
                if c.api_key == key && now < c.expires_at {
                    log::debug!("[baidu_token] cache hit");
                    return Ok(c.token.clone());
                }
            }
        }

        log::debug!("[baidu_token] cache miss, fetching oauth token");
        let fetch_start = Instant::now();
        let (token, expires_in_secs) = fetch_access_token(http, key, secret)?;
        log::debug!(
            "[baidu_token] oauth fetch done in {} ms",
            fetch_start.elapsed().as_millis()
        );
        let ttl_secs = expires_in_secs
            .unwrap_or(DEFAULT_EXPIRES_IN_SECS)
            .saturating_sub(EXPIRY_SAFETY_SECS)
            .max(60);

        let mut guard = self
            .state
            .lock()
            .map_err(|e| Error::config("baidu_token_lock", format!("mutex lock: {}", e)))?;
        *guard = Some(CachedState {
            api_key: key.to_string(),
            token: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl_secs),
        });
        Ok(token)
    }
}

/// 去掉首尾空白与 UTF-8 BOM，避免从 JSON/编辑器复制密钥时带入不可见字符导致 OAuth 401。
fn trim_credential(s: &str) -> &str {
    s.trim().trim_start_matches('\u{feff}')
}

fn fetch_access_token(
    http: &mut dyn PlatformHttpClient,
    api_key: &str,
    api_secret: &str,
) -> Result<(String, Option<u64>)> {
    let form = format!(
        "grant_type=client_credentials&client_id={}&client_secret={}",
        urlencoding::encode(api_key),
        urlencoding::encode(api_secret)
    );
    let headers = [("Content-Type", "application/x-www-form-urlencoded")];
    let (status, body_buf) = http
        .post(BAIDU_TOKEN_URL, &headers, form.as_bytes())
        .map_err(|e| Error::config("baidu_token", e.to_string()))?;
    if status != 200 {
        let detail = format_baidu_oauth_failure_detail(status, body_buf.as_slice());
        log::warn!(
            "[baidu_token] oauth failed status={} detail={}",
            status,
            detail
        );
        return Err(Error::config(
            "baidu_token",
            format!("token http status {}: {}", status, detail),
        ));
    }
    let v: BaiduTokenSuccessResponse = serde_json::from_slice(body_buf.as_slice())
        .map_err(|e| Error::config("baidu_token_parse", e.to_string()))?;
    let token = v
        .access_token
        .ok_or_else(|| Error::config("baidu_token", "missing access_token"))?
        .to_string();
    let expires_in = v.expires_in;
    Ok((token, expires_in))
}

/// 非 200 时从响应体提取可读说明（脱敏、截断）；便于区分 invalid_client 与网关页。
fn format_baidu_oauth_failure_detail(status: u16, body: &[u8]) -> String {
    const MAX: usize = 280;
    if let Ok(v) = serde_json::from_slice::<BaiduTokenErrorResponse>(body) {
        let err = v.error.as_deref().unwrap_or("");
        let desc = v
            .error_description
            .as_deref()
            .or(v.message.as_deref())
            .unwrap_or("");
        if !err.is_empty() || !desc.is_empty() {
            let mut s = format!("{}{}{}", err, if err.is_empty() { "" } else { ": " }, desc);
            s.truncate(MAX);
            return s;
        }
    }
    let lossy = String::from_utf8_lossy(body);
    let t = lossy.trim();
    if t.is_empty() {
        if status == 401 {
            return "empty body (401 多为 API Key / Secret 错误或未开通对应百度 AI 应用能力；请在控制台核对语音相关服务)"
                .to_string();
        }
        return "empty response body".to_string();
    }
    let mut it = t.chars();
    let mut out: String = it.by_ref().take(MAX).collect();
    if it.next().is_some() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_baidu_oauth_failure_detail, trim_credential};

    #[test]
    fn trim_credential_strips_bom() {
        assert_eq!(trim_credential("\u{feff}abc "), "abc");
    }

    #[test]
    fn oauth_detail_parses_json_error() {
        let b = br#"{"error":"invalid_client","error_description":"unknown client id"}"#;
        let d = format_baidu_oauth_failure_detail(401, b);
        assert!(d.contains("invalid_client"));
        assert!(d.contains("unknown client id"));
    }
}
