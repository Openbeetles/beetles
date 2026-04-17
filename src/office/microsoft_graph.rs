#![cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]

use crate::error::{Error, Result};
use crate::office::OfficeHttpClient;
use crate::platform::ResponseBody;
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub const MICROSOFT_GRAPH_DEFAULT_BASE_URL: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Deserialize)]
pub struct MicrosoftGraphCollection<T> {
    #[serde(default)]
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    pub next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MicrosoftGraphErrorEnvelope {
    #[serde(default)]
    pub error: Option<MicrosoftGraphErrorBody>,
}

#[derive(Debug, Default, Deserialize)]
pub struct MicrosoftGraphErrorBody {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
}

pub fn normalize_microsoft_graph_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        MICROSOFT_GRAPH_DEFAULT_BASE_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn build_microsoft_graph_url(base_url: &str, path: &str, query: &[(&str, String)]) -> String {
    let mut url = format!(
        "{}{}",
        normalize_microsoft_graph_base_url(base_url),
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

pub fn parse_microsoft_graph_json<T: DeserializeOwned>(
    stage: &'static str,
    status: u16,
    body: ResponseBody,
) -> Result<T> {
    if (200..=299).contains(&status) {
        return serde_json::from_slice(body.as_slice())
            .map_err(|error| Error::config(stage, error.to_string()));
    }
    let message = extract_graph_error_message(body.as_slice())
        .unwrap_or_else(|| format!("microsoft graph http status {status}"));
    Err(Error::config(stage, message))
}

pub fn request_microsoft_graph_json_ureq<T: DeserializeOwned>(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<T> {
    match response {
        Ok(response) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            let status = 200u16;
            parse_microsoft_graph_json(stage, status, ResponseBody::Heap(body.into_bytes()))
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            parse_microsoft_graph_json(stage, status, ResponseBody::Heap(body.into_bytes()))
        }
        Err(ureq::Error::Transport(error)) => Err(Error::config(stage, error.to_string())),
    }
}

pub fn request_microsoft_graph_json<T: DeserializeOwned>(
    http: &mut dyn OfficeHttpClient,
    stage: &'static str,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<T> {
    let (status, body) = http.request_with_headers(method, url, headers, body)?;
    parse_microsoft_graph_json(stage, status, body)
}

pub fn request_microsoft_graph_empty_ureq(
    stage: &'static str,
    response: std::result::Result<ureq::Response, ureq::Error>,
) -> Result<u16> {
    match response {
        Ok(response) => Ok(response.status()),
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .map_err(|error| Error::config(stage, error.to_string()))?;
            parse_microsoft_graph_json::<serde_json::Value>(
                stage,
                status,
                ResponseBody::Heap(body.into_bytes()),
            )
            .map(|_| status)
        }
        Err(ureq::Error::Transport(error)) => Err(Error::config(stage, error.to_string())),
    }
}

pub fn request_microsoft_graph_empty(
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
    parse_microsoft_graph_json::<serde_json::Value>(stage, status, body).map(|_| status)
}

fn extract_graph_error_message(body: &[u8]) -> Option<String> {
    let payload = serde_json::from_slice::<MicrosoftGraphErrorEnvelope>(body).ok()?;
    let error = payload.error?;
    if error.message.trim().is_empty() {
        (!error.code.trim().is_empty()).then_some(error.code)
    } else if error.code.trim().is_empty() {
        Some(error.message)
    } else {
        Some(format!("{}: {}", error.code, error.message))
    }
}
