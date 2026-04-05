//! 通道连通性检查：按当前启用通道做一次现场 HTTP 探测，供 GET /api/channel_connectivity 使用。
//! 不依赖 Platform，仅依赖 ChannelHttpClient 与 AppConfig。
//!
//! 配置面一次只启用一个 outbound channel，因此这里只对 `enabled_channel`
//! 做真实探测；其余通道返回“当前未启用”的占位结果，避免无意义的串行外网请求。

use crate::config::AppConfig;
use crate::i18n::{Locale, Message, tr};
use serde::Serialize;

/// 单通道连通性结果；与前端约定字段名。
#[derive(Debug, Clone, Serialize)]
pub struct ChannelConnectivityItem {
    pub id: String,
    pub configured: bool,
    pub ok: bool,
    pub message: Option<String>,
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
    message: Option<String>,
) -> ChannelConnectivityItem {
    ChannelConnectivityItem {
        id: id.to_string(),
        configured,
        ok,
        message,
    }
}

fn webhook_configured(c: &AppConfig) -> bool {
    c.webhook_enabled && !c.webhook_token.trim().is_empty()
}

fn disabled_item(id: &'static str, loc: Locale) -> ChannelConnectivityItem {
    item(
        id,
        false,
        false,
        Some(tr(Message::ConnectivityNotConfigured, loc)),
    )
}

fn active_channel_item<H: crate::channels::ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: Locale,
) -> Option<ChannelConnectivityItem> {
    match config.enabled_channel.as_str() {
        "telegram" => Some(crate::channels::telegram::check_connectivity(
            config, http, loc,
        )),
        "feishu" => Some(crate::channels::feishu::check_connectivity(
            config, http, loc,
        )),
        "dingtalk" => Some(crate::channels::dingtalk::check_connectivity(
            config, http, loc,
        )),
        "wecom" => Some(crate::channels::wecom::check_connectivity(
            config, http, loc,
        )),
        "qq_channel" => Some(crate::channels::qq::check_connectivity(config, http, loc)),
        _ => None,
    }
}

fn webhook_item(config: &AppConfig, loc: Locale) -> ChannelConnectivityItem {
    let configured = webhook_configured(config);
    let message = if configured {
        None
    } else {
        Some(tr(Message::ConnectivityNotConfigured, loc))
    };
    item("webhook", configured, configured, message)
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
fn active_channel_configured(config: &AppConfig) -> bool {
    match config.enabled_channel.as_str() {
        "telegram" => !config.tg_token.trim().is_empty(),
        "feishu" => {
            !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty()
        }
        "dingtalk" => !config.dingtalk_webhook_url.trim().is_empty(),
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
pub fn build_unavailable_snapshot(config: &AppConfig, loc: Locale) -> ChannelConnectivitySnapshot {
    let active_id = config.enabled_channel.as_str();
    let active_configured = active_channel_configured(config);
    let unavailable = Some(tr(Message::ChannelConnectivityUnavailable, loc));
    ChannelConnectivitySnapshot {
        channels: vec![
            if active_id == "telegram" {
                item("telegram", active_configured, false, unavailable.clone())
            } else {
                disabled_item("telegram", loc)
            },
            if active_id == "feishu" {
                item("feishu", active_configured, false, unavailable.clone())
            } else {
                disabled_item("feishu", loc)
            },
            if active_id == "dingtalk" {
                item("dingtalk", active_configured, false, unavailable.clone())
            } else {
                disabled_item("dingtalk", loc)
            },
            if active_id == "wecom" {
                item("wecom", active_configured, false, unavailable.clone())
            } else {
                disabled_item("wecom", loc)
            },
            if active_id == "qq_channel" {
                item("qq_channel", active_configured, false, unavailable)
            } else {
                disabled_item("qq_channel", loc)
            },
            webhook_item(config, loc),
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
                .unwrap_or_else(|| disabled_item("telegram", loc)),
            active
                .clone()
                .filter(|item| item.id == "feishu")
                .unwrap_or_else(|| disabled_item("feishu", loc)),
            active
                .clone()
                .filter(|item| item.id == "dingtalk")
                .unwrap_or_else(|| disabled_item("dingtalk", loc)),
            active
                .clone()
                .filter(|item| item.id == "wecom")
                .unwrap_or_else(|| disabled_item("wecom", loc)),
            active
                .filter(|item| item.id == "qq_channel")
                .unwrap_or_else(|| disabled_item("qq_channel", loc)),
            webhook_item(config, loc),
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
        assert!(
            snapshot
                .channels
                .iter()
                .skip(1)
                .take(4)
                .all(|item| !item.configured)
        );
    }

    #[test]
    fn build_snapshot_reports_disabled_channels_as_not_configured() {
        let mut config = configured_config();
        config.enabled_channel.clear();
        let mut http = StubHttp;
        let snapshot = build_snapshot(&config, &mut http, Locale::Zh);
        assert!(
            snapshot
                .channels
                .iter()
                .take(5)
                .all(|item| !item.configured)
        );
        assert!(snapshot.channels.iter().take(5).all(|item| !item.ok));
    }
}
