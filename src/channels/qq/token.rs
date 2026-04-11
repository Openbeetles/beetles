//! QQ access_token 获取与解析的共享实现。
//! 供 sender / WSS / connectivity 共用，避免三处各自维护同一条 HTTP 链路。

use crate::channels::send::{record_outbound_http_failure, record_outbound_http_success};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const QQ_GET_APP_ACCESS_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";

#[derive(serde::Serialize)]
pub struct QqTokenRequest {
    #[serde(rename = "appId")]
    pub app_id: String,
    #[serde(rename = "clientSecret")]
    pub client_secret: String,
}

#[derive(serde::Deserialize)]
pub struct QqTokenResponse {
    pub access_token: Option<String>,
    #[serde(default, deserialize_with = "deserialize_u64_or_string")]
    pub expires_in: u64,
}

#[derive(Clone)]
pub(crate) struct CachedQqToken {
    value: String,
    refresh_after: Instant,
}

#[derive(Clone)]
pub struct SharedQqTokenCache {
    inner: Arc<Mutex<Option<CachedQqToken>>>,
}

pub fn new_shared_qq_token_cache() -> SharedQqTokenCache {
    SharedQqTokenCache {
        inner: Arc::new(Mutex::new(None)),
    }
}

/// QQ API 的 expires_in 可能返回数字或字符串，兼容两种格式。
fn deserialize_u64_or_string<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum U64OrString {
        U64(u64),
        Str(String),
    }
    match U64OrString::deserialize(deserializer)? {
        U64OrString::U64(v) => Ok(v),
        U64OrString::Str(s) => s.parse::<u64>().map_err(serde::de::Error::custom),
    }
}

pub(crate) fn fetch_qq_token_response<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    client_secret: &str,
    stage: &'static str,
) -> Result<QqTokenResponse> {
    let body = QqTokenRequest {
        app_id: app_id.to_string(),
        client_secret: client_secret.to_string(),
    };
    let body_bytes = serde_json::to_vec(&body).map_err(|e| Error::Other {
        source: Box::new(e),
        stage,
    })?;
    let (status, resp_body) = match http.http_post(QQ_GET_APP_ACCESS_TOKEN_URL, &body_bytes) {
        Ok(resp) => resp,
        Err(e) => {
            let error = Error::Other {
                source: Box::new(e),
                stage,
            };
            record_outbound_http_failure(&error);
            return Err(error);
        }
    };
    if status >= 400 {
        let error = Error::Http {
            status_code: status,
            stage,
        };
        record_outbound_http_failure(&error);
        return Err(error);
    }
    record_outbound_http_success();
    serde_json::from_slice(resp_body.as_ref()).map_err(|e| Error::Other {
        source: Box::new(e),
        stage,
    })
}

pub(crate) fn fetch_qq_access_token_with_expiry<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    client_secret: &str,
    stage: &'static str,
) -> Result<(String, u64)> {
    let token_resp = fetch_qq_token_response(http, app_id, client_secret, stage)?;
    match token_resp.access_token {
        Some(token) if !token.is_empty() => Ok((token, token_resp.expires_in.max(60))),
        _ => Err(Error::config(stage, "qq access_token missing")),
    }
}

pub(crate) fn fetch_qq_access_token<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    app_id: &str,
    client_secret: &str,
    stage: &'static str,
) -> Result<String> {
    fetch_qq_access_token_with_expiry(http, app_id, client_secret, stage).map(|(token, _)| token)
}

pub(crate) fn cached_qq_token_value(cached_token: &Option<CachedQqToken>) -> Option<&str> {
    cached_qq_token_value_at(cached_token, Instant::now())
}

fn cached_qq_token_value_at(
    cached_token: &Option<CachedQqToken>,
    now: Instant,
) -> Option<&str> {
    cached_token
        .as_ref()
        .filter(|token| now < token.refresh_after)
        .map(|token| token.value.as_str())
}

pub(crate) fn invalidate_cached_qq_token(cached_token: &mut Option<CachedQqToken>) {
    *cached_token = None;
}

pub(crate) fn load_shared_cached_qq_token(
    shared_cache: &SharedQqTokenCache,
) -> Option<CachedQqToken> {
    shared_cache
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub(crate) fn sync_shared_cached_qq_token(
    shared_cache: &SharedQqTokenCache,
    cached_token: &Option<CachedQqToken>,
) {
    *shared_cache
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = cached_token.clone();
}

pub(crate) fn clear_shared_cached_qq_token(shared_cache: &SharedQqTokenCache) {
    *shared_cache
        .inner
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

pub(crate) fn fetch_and_cache_qq_token<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    cached_token: &mut Option<CachedQqToken>,
    app_id: &str,
    client_secret: &str,
    stage: &'static str,
    refresh_skew_secs: u64,
) -> Result<String> {
    let (token, expires_in_secs) =
        fetch_qq_access_token_with_expiry(http, app_id, client_secret, stage)?;
    let usable_for_secs = expires_in_secs.saturating_sub(refresh_skew_secs).max(1);
    *cached_token = Some(CachedQqToken {
        value: token.clone(),
        refresh_after: Instant::now() + Duration::from_secs(usable_for_secs),
    });
    Ok(token)
}

pub(crate) fn ensure_cached_qq_token<H: ChannelHttpClient + ?Sized>(
    http: &mut H,
    cached_token: &mut Option<CachedQqToken>,
    app_id: &str,
    client_secret: &str,
    stage: &'static str,
    refresh_skew_secs: u64,
) -> Result<String> {
    if let Some(token) = cached_qq_token_value(cached_token) {
        return Ok(token.to_string());
    }
    fetch_and_cache_qq_token(
        http,
        cached_token,
        app_id,
        client_secret,
        stage,
        refresh_skew_secs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_token_uses_monotonic_deadline() {
        let now = Instant::now();
        let cached = Some(CachedQqToken {
            value: "qq-token".to_string(),
            refresh_after: now + Duration::from_secs(90),
        });

        assert_eq!(
            cached_qq_token_value_at(&cached, now + Duration::from_secs(30)),
            Some("qq-token")
        );
        assert_eq!(
            cached_qq_token_value_at(&cached, now + Duration::from_secs(89)),
            Some("qq-token")
        );
        assert_eq!(
            cached_qq_token_value_at(&cached, now + Duration::from_secs(90)),
            None
        );
    }
}
