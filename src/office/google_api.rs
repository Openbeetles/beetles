#![cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]

use crate::error::{Error, Result};
use crate::office::OfficeHttpClient;
use crate::platform::ResponseBody;
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const GOOGLE_GMAIL_DEFAULT_BASE_URL: &str = "https://gmail.googleapis.com/gmail/v1";
pub const GOOGLE_CALENDAR_DEFAULT_BASE_URL: &str = "https://www.googleapis.com/calendar/v3";
pub const GOOGLE_DRIVE_DEFAULT_BASE_URL: &str = "https://www.googleapis.com/drive/v3";
pub const GOOGLE_PEOPLE_DEFAULT_BASE_URL: &str = "https://people.googleapis.com/v1";

#[derive(Debug, Deserialize)]
pub struct GoogleApiListEnvelope<T> {
    #[serde(default)]
    pub messages: Vec<T>,
    #[serde(default)]
    pub files: Vec<T>,
    #[serde(default)]
    pub events: Vec<T>,
    #[serde(default)]
    pub people: Vec<T>,
    #[serde(default, rename = "connections")]
    pub connections: Vec<T>,
    #[serde(default, rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleApiErrorEnvelope {
    #[serde(default)]
    pub error: Option<GoogleApiErrorBody>,
}

#[derive(Debug, Default, Deserialize)]
pub struct GoogleApiErrorBody {
    #[serde(default)]
    pub code: u16,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub status: String,
}

pub fn normalize_google_api_base_url(raw: &str, default_base_url: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        default_base_url.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn build_google_api_url(base_url: &str, path: &str, query: &[(&str, String)]) -> String {
    let mut url = format!(
        "{}{}",
        base_url.trim_end_matches('/'),
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    );
    let query_parts = query
        .iter()
        .filter_map(|(key, value)| {
            let value = value.trim();
            (!value.is_empty()).then(|| format!("{key}={}", urlencoding::encode(value)))
        })
        .collect::<Vec<_>>();
    if !query_parts.is_empty() {
        url.push('?');
        url.push_str(&query_parts.join("&"));
    }
    url
}

pub fn parse_google_api_json<T: DeserializeOwned>(
    stage: &'static str,
    status: u16,
    body: ResponseBody,
) -> Result<T> {
    if (200..=299).contains(&status) {
        return serde_json::from_slice(body.as_slice())
            .map_err(|error| Error::config(stage, error.to_string()));
    }
    let message = extract_google_error_message(body.as_slice())
        .unwrap_or_else(|| format!("google api http status {status}"));
    Err(Error::config(stage, message))
}

pub fn request_google_api_json_ureq<T: DeserializeOwned>(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<T> {
    match response {
        Ok(response) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            parse_google_api_json(stage, 200, ResponseBody::Heap(body.into_bytes()))
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            parse_google_api_json(stage, status, ResponseBody::Heap(body.into_bytes()))
        }
        Err(ureq::Error::Transport(error)) => Err(Error::config(stage, error.to_string())),
    }
}

pub fn request_google_api_json<T: DeserializeOwned>(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<T> {
    let (status, body) = http.request_with_headers(method, url, headers, body)?;
    parse_google_api_json(stage, status, body)
}

pub fn request_google_api_empty_ureq(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<u16> {
    match response {
        Ok(response) => Ok(response.status()),
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            parse_google_api_json::<serde_json::Value>(
                stage,
                status,
                ResponseBody::Heap(body.into_bytes()),
            )
            .map(|_| status)
        }
        Err(ureq::Error::Transport(error)) => Err(Error::config(stage, error.to_string())),
    }
}

pub fn request_google_api_empty(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<u16> {
    let (status, body) = http.request_with_headers(method, url, headers, body)?;
    if (200..=299).contains(&status) {
        return Ok(status);
    }
    parse_google_api_json::<serde_json::Value>(stage, status, body).map(|_| status)
}

fn extract_google_error_message(body: &[u8]) -> Option<String> {
    let payload = serde_json::from_slice::<GoogleApiErrorEnvelope>(body).ok()?;
    let error = payload.error?;
    if error.message.trim().is_empty() {
        if error.status.trim().is_empty() {
            (error.code != 0).then(|| format!("google api http status {}", error.code))
        } else if error.code == 0 {
            Some(error.status)
        } else {
            Some(format!("{} ({})", error.status, error.code))
        }
    } else if error.status.trim().is_empty() {
        Some(error.message)
    } else {
        Some(format!("{}: {}", error.status, error.message))
    }
}
