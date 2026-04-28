//! Product control-plane API contract helpers.
//! Product API should expose stable keys and structured metadata instead of
//! Beetle-generated localized prose.

use crate::error::Error;

pub const COMMON_INVALID_JSON: &str = "common.invalid_json";
pub const COMMON_INVALID_UTF8: &str = "common.invalid_utf8";
pub const COMMON_BODY_READ_FAILED: &str = "common.body_read_failed";
pub const COMMON_INVALID_URL: &str = "common.invalid_url";
pub const COMMON_MISSING_QUERY_PARAM: &str = "common.missing_query_param";
pub const COMMON_SAVE_FAILED: &str = "common.save_failed";
pub const COMMON_OPERATION_FAILED: &str = "common.operation_failed";
pub const COMMON_QUEUE_FULL: &str = "common.queue_full";
pub const COMMON_UPSTREAM_HTTP_STATUS: &str = "common.upstream_http_status";
pub const COMMON_STORAGE_ACCESS_FAILED: &str = "common.storage_access_failed";
pub const COMMON_FILE_STORAGE_FAILED: &str = "common.file_storage_failed";
pub const COMMON_IO_FAILED: &str = "common.io_failed";

pub const NETWORK_PROXY_UNSUPPORTED: &str = "network.proxy_unsupported";

pub const SYSTEM_DEVICE_ERROR: &str = "system.device_error";
pub const SYSTEM_LOCALE_INVALID: &str = "system.locale_invalid";

pub const WEBHOOK_DISABLED: &str = "webhook.disabled";
pub const WEBHOOK_INVALID_TOKEN: &str = "webhook.invalid_token";
pub const WEBHOOK_CONTENT_TOO_LONG: &str = "webhook.content_too_long";
pub const WEBHOOK_INGRESS_UNAVAILABLE: &str = "webhook.ingress_unavailable";

pub const CONFIG_REJECTED: &str = "config.rejected";
pub const CONFIG_FIELD_TOO_LONG: &str = "config.field_too_long";

pub const CHANNEL_ENABLED_INVALID: &str = "channel.enabled_invalid";
pub const CHANNEL_FIELD_TOO_LONG: &str = "channel.field_too_long";
pub const CHANNEL_TG_GROUP_ACTIVATION_INVALID: &str = "channel.tg_group_activation_invalid";

pub const LLM_SOURCES_EMPTY: &str = "llm.sources_empty";
pub const LLM_INDICES_INVALID: &str = "llm.indices_invalid";
pub const LLM_SOURCE_FIELD_TOO_LONG: &str = "llm.source_field_too_long";

pub const HARDWARE_CONFIG_INVALID: &str = "hardware.config_invalid";
pub const DISPLAY_CONFIG_INVALID: &str = "display.config_invalid";
pub const OFFICE_CAPABILITY_INVALID: &str = "office.capability_invalid";

pub const PAIRING_ALREADY_SET: &str = "pairing.code_already_set";
pub const PAIRING_CODE_INVALID: &str = "pairing.code_must_be_6_digits";
pub const PAIRING_SAVE_FAILED: &str = "pairing.failed_to_save_code";

pub const SKILL_NOT_FOUND: &str = "skill.not_found";
pub const SKILL_NAME_REQUIRED: &str = "skill.name_required";
pub const SKILL_NAME_REQUIRED_FOR_WRITE: &str = "skill.name_required_for_write";
pub const SKILL_NAME_OR_ENABLED_REQUIRED: &str = "skill.name_or_enabled_required";
pub const SKILL_ORDER_NAME_CONTENT_REQUIRED: &str = "skill.order_name_content_required";
pub const SKILL_URL_REQUIRED: &str = "skill.url_required";
pub const SKILL_URL_BODY_NOT_UTF8: &str = "skill.url_body_not_utf8";
pub const SKILL_IMPORT_FETCH_FAILED: &str = "skill.import_fetch_failed";
pub const SKILL_WRITE_FAILED: &str = "skill.write_failed";

pub const PACKAGE_INVALID_REQUEST: &str = "package.invalid_request";
pub const PACKAGE_INSTALL_PAYLOAD_REQUIRED: &str = "package.install_payload_required";

pub fn error_key(error: &Error) -> &'static str {
    match error {
        Error::Config { message, stage } => config_error_key(message, stage),
        Error::Nvs { .. } => COMMON_STORAGE_ACCESS_FAILED,
        Error::Spiffs { .. } => COMMON_FILE_STORAGE_FAILED,
        Error::Io { .. } => COMMON_IO_FAILED,
        Error::Esp { .. } => SYSTEM_DEVICE_ERROR,
        Error::Http { .. } => COMMON_UPSTREAM_HTTP_STATUS,
        Error::Other { stage, source, .. } => {
            if *stage == "http_body_utf8" {
                return COMMON_INVALID_UTF8;
            }
            if *stage == "proxy_connect" {
                return NETWORK_PROXY_UNSUPPORTED;
            }
            let source_text = source.to_string();
            if source_text.contains("proxy CONNECT tunnel not implemented") {
                return NETWORK_PROXY_UNSUPPORTED;
            }
            COMMON_OPERATION_FAILED
        }
    }
}

fn config_error_key(message: &str, stage: &str) -> &'static str {
    match stage {
        "deserialize" => COMMON_INVALID_JSON,
        "serialize" => COMMON_SAVE_FAILED,
        "locale" if message == "must be zh or en" => SYSTEM_LOCALE_INVALID,
        "tg_group_activation" => CHANNEL_TG_GROUP_ACTIVATION_INVALID,
        "wifi" => {
            if message.contains("length must be <=") {
                CONFIG_FIELD_TOO_LONG
            } else {
                CONFIG_REJECTED
            }
        }
        "office_capability" => OFFICE_CAPABILITY_INVALID,
        "capability_package" => PACKAGE_INVALID_REQUEST,
        "hardware" => HARDWARE_CONFIG_INVALID,
        "display" => DISPLAY_CONFIG_INVALID,
        "config" => config_body_error_key(message),
        _ => CONFIG_REJECTED,
    }
}

fn config_body_error_key(message: &str) -> &'static str {
    if message == "proxy_url must be empty or like http://host:port" {
        return COMMON_INVALID_URL;
    }
    if message == "tg_group_activation must be 'mention' or 'always'" {
        return CHANNEL_TG_GROUP_ACTIVATION_INVALID;
    }
    if message == "llm_sources must not be empty" {
        return LLM_SOURCES_EMPTY;
    }
    if message.contains("llm_router_source_index and llm_worker_source_index must be") {
        return LLM_INDICES_INVALID;
    }
    if (message.contains("llm_router_source_index") || message.contains("llm_worker_source_index"))
        && message.contains("out of range")
    {
        return LLM_INDICES_INVALID;
    }
    if message.contains("llm_sources[") && message.contains("field length over limit") {
        return LLM_SOURCE_FIELD_TOO_LONG;
    }
    if message.starts_with("enabled_channel must be one of") {
        return CHANNEL_ENABLED_INVALID;
    }
    if message.starts_with("channel field length must be <=") {
        return CHANNEL_FIELD_TOO_LONG;
    }
    if message.contains("length must be <=")
        && (message.contains("wifi_")
            || message.contains("dingtalk_")
            || message.contains("wecom_")
            || message.contains("tg_token"))
    {
        return CONFIG_FIELD_TOO_LONG;
    }
    CONFIG_REJECTED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_key_maps_known_validation_messages() {
        assert_eq!(
            error_key(&Error::config("config", "llm_sources must not be empty")),
            LLM_SOURCES_EMPTY
        );
        assert_eq!(
            error_key(&Error::config(
                "config",
                "proxy_url must be empty or like http://host:port"
            )),
            COMMON_INVALID_URL
        );
        assert_eq!(
            error_key(&Error::config(
                "office_capability",
                "unsupported office capability 'invalid'"
            )),
            OFFICE_CAPABILITY_INVALID
        );
    }

    #[test]
    fn error_key_maps_proxy_connect_to_proxy_unsupported() {
        let error = Error::Other {
            source: Box::new(std::io::Error::other(
                "proxy CONNECT tunnel not implemented",
            )),
            stage: "http_request",
        };
        assert_eq!(error_key(&error), NETWORK_PROXY_UNSUPPORTED);
    }

    #[test]
    fn error_key_maps_http_body_utf8_to_invalid_utf8() {
        let error = Error::Other {
            source: Box::new(std::io::Error::other("invalid utf8")),
            stage: "http_body_utf8",
        };
        assert_eq!(error_key(&error), COMMON_INVALID_UTF8);
    }

    #[test]
    fn capability_package_stage_uses_package_domain_key() {
        assert_eq!(
            error_key(&Error::config(
                "capability_package",
                "package is not installed"
            )),
            PACKAGE_INVALID_REQUEST
        );
    }
}
