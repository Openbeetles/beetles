//! 将 [`crate::error::Error`] 映射为 [`super::catalog::Message`]，绝不把 `Display` 原文给用户。
//! Maps `Error` to localized user messages without exposing raw `Display` text.

use crate::error::Error;

use super::catalog::Message;
use super::{tr, Locale};

/// HTTP / API 等对用户的错误文案（已按语言本地化）。
pub fn tr_error(err: &Error, loc: Locale) -> String {
    tr(map_error(err), loc)
}

fn map_error(err: &Error) -> Message {
    match err {
        Error::Config { message, stage } => map_config(message, stage),
        Error::Nvs { .. } => Message::ErrorNvs,
        Error::Spiffs { .. } => Message::ErrorSpiffs,
        Error::Io { .. } => Message::ErrorIo,
        Error::Esp { stage: s, code: _ } => match *s {
            "ota_download" => Message::OtaDownload,
            "ota_validate" => Message::OtaValidate,
            "ota_write" => Message::OtaWrite,
            _ => Message::ErrorEsp,
        },
        Error::Http { status_code: c, .. } => Message::ErrorHttpStatus { code: *c },
        Error::Other {
            stage: s, source, ..
        } => {
            if *s == "proxy_connect" {
                return Message::ErrorProxyUnsupported;
            }
            let es = source.to_string();
            if es.contains("proxy CONNECT tunnel not implemented") {
                return Message::ErrorProxyUnsupported;
            }
            Message::OperationFailed
        }
    }
}

fn map_config(message: &str, stage: &str) -> Message {
    match stage {
        "deserialize" => Message::InvalidJson,
        "serialize" => Message::SaveFailed,
        "locale" if message == "must be zh or en" => Message::LocaleMustBeZhOrEn,
        "write_tg_group_activation" => Message::TgGroupActivationInvalid,
        "wifi" => {
            if message.contains("length must be <=") {
                Message::ConfigFieldTooLong
            } else {
                Message::ConfigRejected
            }
        }
        "hardware" => Message::ConfigHardwareInvalid,
        "display" => Message::ConfigDisplayInvalid,
        "config" => map_config_body(message),
        _ => Message::ConfigRejected,
    }
}

fn map_config_body(m: &str) -> Message {
    if m == "wifi_ssid is required for WiFi" {
        return Message::ConfigRejected;
    }
    if m.contains("wifi_ssid length") || m.contains("wifi_pass length") {
        return Message::ConfigFieldTooLong;
    }
    if m == "proxy_url must be empty or like http://host:port" {
        return Message::InvalidUrl;
    }
    if m == "tg_group_activation must be 'mention' or 'always'" {
        return Message::TgGroupActivationInvalid;
    }
    if m.contains("session_max_messages must") {
        return Message::ConfigSessionRangeInvalid;
    }
    if m == "llm_sources must not be empty" {
        return Message::ConfigLlmSourcesEmpty;
    }
    if m.contains("llm_router_source_index and llm_worker_source_index must be") {
        return Message::ConfigLlmIndicesInvalid;
    }
    if (m.contains("llm_router_source_index") || m.contains("llm_worker_source_index"))
        && m.contains("out of range")
    {
        return Message::ConfigLlmIndicesInvalid;
    }
    if m.contains("llm_sources[") && m.contains("field length over limit") {
        return Message::ConfigLlmSourceFieldLen;
    }
    if m.contains("llm_sources[") && m.contains("api_key is required") {
        return Message::ConfigRejected;
    }
    if m.starts_with("enabled_channel must be one of") {
        return Message::ConfigEnabledChannelInvalid;
    }
    if m.starts_with("channel field length must be <=") {
        return Message::ConfigChannelFieldLen;
    }
    if m.contains("dingtalk_webhook_url length must be <=")
        || m.contains("wecom_default_touser length must be <=")
    {
        return Message::ConfigFieldTooLong;
    }
    if m == "wifi_ssid and wifi_pass length must be <=" {
        return Message::ConfigFieldTooLong;
    }
    if m == "wecom_agent_id must be a valid u32" {
        return Message::ConfigRejected;
    }
    if m.contains("enabled_channel=") && m.contains("requires") {
        return Message::ConfigRejected;
    }
    if m.contains("length must be <=") && m.contains("tg_token") {
        return Message::ConfigFieldTooLong;
    }
    if m.contains("duplicate model+api_url") {
        return Message::ConfigRejected;
    }
    Message::ConfigRejected
}
