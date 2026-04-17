#![cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]

use crate::error::{Error, Result};
use crate::office::OfficeHttpClient;
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const WECOM_DEFAULT_BASE_URL: &str = "https://qyapi.weixin.qq.com";

pub struct WecomAuthCredential<'a> {
    pub corp_id: &'a str,
    pub corp_secret: &'a str,
    pub base_url: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct WecomApiEnvelope<T> {
    #[serde(default)]
    pub errcode: i64,
    #[serde(default)]
    pub errmsg: String,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Debug, Default, Deserialize)]
pub struct WecomTokenPayload {
    #[serde(default)]
    pub access_token: String,
}

impl<T> WecomApiEnvelope<T> {
    pub fn require_ok(self, stage: &'static str) -> Result<T> {
        if self.errcode != 0 {
            return Err(Error::config(
                stage,
                format!("wecom returned errcode {}: {}", self.errcode, self.errmsg),
            ));
        }
        Ok(self.data)
    }
}

pub fn fetch_wecom_access_token_ureq(
    stage: &'static str,
    credential: WecomAuthCredential<'_>,
) -> Result<String> {
    let url = format!(
        "{}/cgi-bin/gettoken?corpid={}&corpsecret={}",
        credential.base_url.trim_end_matches('/'),
        urlencoding::encode(credential.corp_id),
        urlencoding::encode(credential.corp_secret)
    );
    let payload: WecomApiEnvelope<WecomTokenPayload> =
        request_wecom_json_ureq(stage, ureq::get(&url).call())?;
    let data = payload.require_ok(stage)?;
    if data.access_token.trim().is_empty() {
        return Err(Error::config(stage, "missing access_token"));
    }
    Ok(data.access_token)
}

pub fn fetch_wecom_access_token(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    credential: WecomAuthCredential<'_>,
) -> Result<String> {
    let url = format!(
        "{}/cgi-bin/gettoken?corpid={}&corpsecret={}",
        credential.base_url.trim_end_matches('/'),
        urlencoding::encode(credential.corp_id),
        urlencoding::encode(credential.corp_secret)
    );
    let payload: WecomApiEnvelope<WecomTokenPayload> =
        request_wecom_json(http, stage, "GET", &url, &[], None)?;
    let data = payload.require_ok(stage)?;
    if data.access_token.trim().is_empty() {
        return Err(Error::config(stage, "missing access_token"));
    }
    Ok(data.access_token)
}

pub fn request_wecom_json_ureq<T: DeserializeOwned>(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<T> {
    match response {
        Ok(response) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            serde_json::from_str::<T>(&body)
                .map_err(|error| Error::config(stage, error.to_string()))
        }
        Err(ureq::Error::Status(status, _)) => Err(Error::http(stage, status)),
        Err(ureq::Error::Transport(error)) => Err(Error::config(stage, error.to_string())),
    }
}

pub fn request_wecom_json<T: DeserializeOwned>(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<T> {
    let (status, body) = http.request_with_headers(method, url, headers, body)?;
    if !(200..=299).contains(&status) {
        return Err(Error::http(stage, status));
    }
    serde_json::from_slice::<T>(body.as_slice())
        .map_err(|error| Error::config(stage, error.to_string()))
}
