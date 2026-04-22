//! 通道连通性检查：按当前启用通道做一次现场 HTTP 探测，供 GET /api/channel_connectivity 使用。
//! 不依赖 Platform，仅依赖 ChannelHttpClient 与 AppConfig。
//!
//! 配置面一次只启用一个 outbound channel，因此这里只对 `enabled_channel`
//! 做真实探测；其余通道返回“当前未启用”的占位结果，避免无意义的串行外网请求。

use crate::config::AppConfig;
use crate::i18n::Locale;
use serde::Serialize;

pub const CONNECTIVITY_NOT_CONFIGURED_KEY: &str = "network.connectivity_not_configured";
pub const CONNECTIVITY_CHECK_FAILED_KEY: &str = "network.connectivity_check_failed";
pub const CONNECTIVITY_TOKEN_INVALID_KEY: &str = "network.connectivity_token_invalid";
pub const CONNECTIVITY_SESSION_REPLY_ONLY_KEY: &str = "network.connectivity_session_reply_only";
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub const CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY: &str = "network.channel_connectivity_unavailable";

/// 单通道连通性结果；与前端约定字段名。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelConnectivityItem {
    pub id: String,
    pub configured: bool,
    pub ok: bool,
    pub message_key: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelConnectivitySnapshot {
    pub channels: Vec<ChannelConnectivityItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at_unix_secs: Option<u64>,
    pub stale: bool,
}

/// 供各通道 check_connectivity 构建结果用。
pub(crate) fn item(
    id: &'static str,
    configured: bool,
    ok: bool,
    message_key: Option<&'static str>,
) -> ChannelConnectivityItem {
    ChannelConnectivityItem {
        id: id.to_string(),
        configured,
        ok,
        message_key,
    }
}

/// Canonical probe outcome for channel connectivity checks.
/// 通道连通性探测的统一结果口径。
pub(crate) enum ProbeStatus {
    Ok,
    InvalidToken,
    CheckFailed,
}

/// Builds a connectivity item from a common "configured -> probe -> normalized message" flow.
/// 统一处理“已配置 -> 探测 -> 归一化消息”的通道连通性骨架。
pub(crate) fn probe_item<F>(id: &'static str, configured: bool, probe: F) -> ChannelConnectivityItem
where
    F: FnOnce() -> ProbeStatus,
{
    if !configured {
        return item(id, false, false, Some(CONNECTIVITY_NOT_CONFIGURED_KEY));
    }
    match probe() {
        ProbeStatus::Ok => item(id, true, true, None),
        ProbeStatus::InvalidToken => item(id, true, false, Some(CONNECTIVITY_TOKEN_INVALID_KEY)),
        ProbeStatus::CheckFailed => item(id, true, false, Some(CONNECTIVITY_CHECK_FAILED_KEY)),
    }
}

fn webhook_configured(c: &AppConfig) -> bool {
    c.webhook_enabled && !c.webhook_token.trim().is_empty()
}

fn disabled_item(id: &'static str) -> ChannelConnectivityItem {
    item(id, false, false, Some(CONNECTIVITY_NOT_CONFIGURED_KEY))
}

fn active_channel_item<H: crate::channels::ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    _loc: Locale,
) -> Option<ChannelConnectivityItem> {
    match config.enabled_channel.as_str() {
        "telegram" => Some(crate::channels::telegram::check_connectivity(config, http)),
        "feishu" => Some(crate::channels::feishu::check_connectivity(config, http)),
        "dingtalk" => Some(crate::channels::dingtalk::check_connectivity(config, http)),
        "wecom" => Some(crate::channels::wecom::check_connectivity(config, http)),
        "qq_channel" => Some(crate::channels::qq::check_connectivity(config, http)),
        _ => None,
    }
}

fn webhook_item(config: &AppConfig) -> ChannelConnectivityItem {
    let configured = webhook_configured(config);
    let message_key = if configured {
        None
    } else {
        Some(CONNECTIVITY_NOT_CONFIGURED_KEY)
    };
    item("webhook", configured, configured, message_key)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn active_channel_configured(config: &AppConfig) -> bool {
    match config.enabled_channel.as_str() {
        "telegram" => !config.tg_token.trim().is_empty(),
        "feishu" => {
            !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty()
        }
        "dingtalk" => {
            !config.dingtalk_webhook_url.trim().is_empty() || config.enabled_channel == "dingtalk"
        }
        "wecom" => {
            !config.wecom_corp_id.trim().is_empty()
                && !config.wecom_agent_id.trim().is_empty()
                && !config.wecom_corp_secret.trim().is_empty()
        }
        "qq_channel" => {
            !config.qq_channel_app_id.trim().is_empty()
                && !config.qq_channel_secret.trim().is_empty()
        }
        _ => false,
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
pub fn build_unavailable_snapshot(config: &AppConfig, _loc: Locale) -> ChannelConnectivitySnapshot {
    let active_id = config.enabled_channel.as_str();
    let active_configured = active_channel_configured(config);
    let unavailable = Some(CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY);
    ChannelConnectivitySnapshot {
        channels: vec![
            if active_id == "telegram" {
                item("telegram", active_configured, false, unavailable.clone())
            } else {
                disabled_item("telegram")
            },
            if active_id == "feishu" {
                item("feishu", active_configured, false, unavailable.clone())
            } else {
                disabled_item("feishu")
            },
            if active_id == "dingtalk" {
                item("dingtalk", active_configured, false, unavailable.clone())
            } else {
                disabled_item("dingtalk")
            },
            if active_id == "wecom" {
                item("wecom", active_configured, false, unavailable.clone())
            } else {
                disabled_item("wecom")
            },
            if active_id == "qq_channel" {
                item("qq_channel", active_configured, false, unavailable)
            } else {
                disabled_item("qq_channel")
            },
            webhook_item(config),
        ],
        checked_at_unix_secs: Some(crate::util::current_unix_secs()),
        stale: true,
    }
}

/// 按固定顺序返回通道连通性结果。
/// 仅当前 `enabled_channel` 执行真实探测；其他通道返回当前未启用。
pub fn build_snapshot<H: crate::channels::ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: Locale,
) -> ChannelConnectivitySnapshot {
    let active = active_channel_item(config, http, loc);
    ChannelConnectivitySnapshot {
        channels: vec![
            active
                .clone()
                .filter(|item| item.id == "telegram")
                .unwrap_or_else(|| disabled_item("telegram")),
            active
                .clone()
                .filter(|item| item.id == "feishu")
                .unwrap_or_else(|| disabled_item("feishu")),
            active
                .clone()
                .filter(|item| item.id == "dingtalk")
                .unwrap_or_else(|| disabled_item("dingtalk")),
            active
                .clone()
                .filter(|item| item.id == "wecom")
                .unwrap_or_else(|| disabled_item("wecom")),
            active
                .filter(|item| item.id == "qq_channel")
                .unwrap_or_else(|| disabled_item("qq_channel")),
            webhook_item(config),
        ],
        checked_at_unix_secs: Some(crate::util::current_unix_secs()),
        stale: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::ChannelHttpClient;
    use crate::error::Result;

    #[derive(Default)]
    struct StubHttp;

    impl ChannelHttpClient for StubHttp {
        fn http_get(&mut self, _url: &str) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post(
            &mut self,
            _url: &str,
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(b"{}".to_vec())))
        }

        fn http_post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(b"{}".to_vec())))
        }
    }

    fn configured_config() -> AppConfig {
        let mut config = AppConfig::load_from_env();
        config.tg_token = "tg-token".to_string();
        config.enabled_channel = "telegram".to_string();
        config
    }

    #[test]
    fn build_snapshot_checks_only_enabled_channel() {
        let mut http = StubHttp;
        let snapshot = build_snapshot(&configured_config(), &mut http, Locale::Zh);
        assert!(!snapshot.stale);
        assert_eq!(snapshot.channels[0].id, "telegram");
        assert!(snapshot.channels[0].configured);
        assert!(snapshot
            .channels
            .iter()
            .skip(1)
            .take(4)
            .all(|item| !item.configured));
    }

    #[test]
    fn build_snapshot_reports_disabled_channels_as_not_configured() {
        let mut config = configured_config();
        config.enabled_channel.clear();
        let mut http = StubHttp;
        let snapshot = build_snapshot(&config, &mut http, Locale::Zh);
        assert!(snapshot
            .channels
            .iter()
            .take(5)
            .all(|item| !item.configured));
        assert!(snapshot.channels.iter().take(5).all(|item| !item.ok));
    }
}
