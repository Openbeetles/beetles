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
#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
pub const CONNECTIVITY_TOKEN_INVALID_KEY: &str = "network.connectivity_token_invalid";
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
#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
pub(crate) enum ProbeStatus {
    Ok,
    InvalidToken,
    CheckFailed,
}

/// Builds a connectivity item from a common "configured -> probe -> normalized message" flow.
/// 统一处理“已配置 -> 探测 -> 归一化消息”的通道连通性骨架。
#[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
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
    #[cfg(not(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel"
    )))]
    let _ = http;
    match crate::normalize_compiled_enabled_channel(&config.enabled_channel) {
        #[cfg(feature = "telegram")]
        "telegram" => Some(crate::channels::telegram::check_connectivity(config, http)),
        #[cfg(feature = "feishu")]
        "feishu" => Some(crate::channels::feishu::check_connectivity(config, http)),
        #[cfg(feature = "dingtalk")]
        "dingtalk" => Some(crate::channels::dingtalk::check_connectivity(config, http)),
        #[cfg(feature = "wecom")]
        "wecom" => Some(crate::channels::wecom::check_connectivity(config, http)),
        #[cfg(feature = "qq_channel")]
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

fn active_channel_configured(config: &AppConfig) -> bool {
    let active_id = crate::normalize_compiled_enabled_channel(&config.enabled_channel);
    crate::build_channel_capability_registry(config, false)
        .get(active_id)
        .map(|entry| entry.configured)
        .unwrap_or(false)
}

pub fn build_unavailable_snapshot(config: &AppConfig, _loc: Locale) -> ChannelConnectivitySnapshot {
    let active_id = crate::normalize_compiled_enabled_channel(&config.enabled_channel);
    let active_configured = active_channel_configured(config);
    let unavailable = Some(CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY);
    ChannelConnectivitySnapshot {
        channels: crate::connectivity_channel_entries()
            .map(|entry| {
                if active_id == entry.id {
                    item(entry.id, active_configured, false, unavailable)
                } else {
                    disabled_item(entry.id)
                }
            })
            .chain(std::iter::once(webhook_item(config)))
            .collect(),
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
    let active_id = crate::normalize_compiled_enabled_channel(&config.enabled_channel);
    ChannelConnectivitySnapshot {
        channels: crate::connectivity_channel_entries()
            .map(|entry| {
                active
                    .clone()
                    .filter(|item| item.id == entry.id)
                    .unwrap_or_else(|| {
                        if active_id == entry.id {
                            item(
                                entry.id,
                                active.as_ref().map(|item| item.configured).unwrap_or(false),
                                false,
                                Some(CONNECTIVITY_CHECK_FAILED_KEY),
                            )
                        } else {
                            disabled_item(entry.id)
                        }
                    })
            })
            .chain(std::iter::once(webhook_item(config)))
            .collect(),
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

    #[cfg(feature = "telegram")]
    #[test]
    fn build_snapshot_checks_only_enabled_channel() {
        let mut http = StubHttp;
        let snapshot = build_snapshot(&configured_config(), &mut http, Locale::Zh);
        assert!(!snapshot.stale);
        let telegram = snapshot
            .channels
            .iter()
            .find(|item| item.id == "telegram")
            .expect("telegram connectivity item");
        assert!(telegram.configured);
        assert!(snapshot
            .channels
            .iter()
            .filter(|item| item.id != "telegram" && item.id != "webhook")
            .all(|item| !item.configured));
    }

    #[cfg(not(feature = "telegram"))]
    #[test]
    fn build_snapshot_skips_uncompiled_telegram_channel() {
        let mut http = StubHttp;
        let snapshot = build_snapshot(&configured_config(), &mut http, Locale::Zh);
        assert!(!snapshot.channels.iter().any(|item| item.id == "telegram"));
    }

    #[cfg(not(feature = "wecom"))]
    #[test]
    fn build_snapshot_skips_uncompiled_wecom_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = "wecom".to_string();
        config.wecom_bot_id = "bot-id".to_string();
        config.wecom_bot_secret = "bot-secret".to_string();

        let mut http = StubHttp;
        let snapshot = build_snapshot(&config, &mut http, Locale::Zh);
        assert!(!snapshot.channels.iter().any(|item| item.id == "wecom"));
    }

    #[cfg(not(feature = "dingtalk"))]
    #[test]
    fn build_snapshot_skips_uncompiled_dingtalk_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = "dingtalk".to_string();
        config.dingtalk_client_id = "ding-client".to_string();
        config.dingtalk_client_secret = "ding-secret".to_string();

        let mut http = StubHttp;
        let snapshot = build_snapshot(&config, &mut http, Locale::Zh);
        assert!(!snapshot.channels.iter().any(|item| item.id == "dingtalk"));
    }

    #[cfg(not(feature = "qq_channel"))]
    #[test]
    fn build_snapshot_skips_uncompiled_qq_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = "qq_channel".to_string();
        config.qq_channel_app_id = "qq-app".to_string();
        config.qq_channel_secret = "qq-secret".to_string();

        let mut http = StubHttp;
        let snapshot = build_snapshot(&config, &mut http, Locale::Zh);
        assert!(!snapshot.channels.iter().any(|item| item.id == "qq_channel"));
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
            .filter(|item| item.id != "webhook")
            .all(|item| !item.configured));
        assert!(snapshot
            .channels
            .iter()
            .filter(|item| item.id != "webhook")
            .all(|item| !item.ok));
    }
}
