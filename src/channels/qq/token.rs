//! QQ access_token 获取与解析的共享实现。
//! 供 sender / WSS / connectivity 共用，避免三处各自维护同一条 HTTP 链路。

use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};

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
            crate::metrics::record_channel_http_result(false);
            return Err(Error::Other {
                source: Box::new(e),
                stage,
            });
        }
    };
    if status >= 400 {
        crate::metrics::record_channel_http_result(false);
        return Err(Error::Http {
            status_code: status,
            stage,
        });
    }
    crate::metrics::record_channel_http_result(true);
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
