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
    pub runtime_status: ChannelRuntimeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_reason: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelRuntimeStatus {
    Disabled,
    Configured,
    WorkerStarted,
    WaitingWallClock,
    SuspendedByMode,
    Connecting,
    Connected,
    CoolingDown,
    Failed,
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
    let (runtime_status, runtime_reason) =
        runtime_status_for_channel_item(id, configured, ok, message_key);
    ChannelConnectivityItem {
        id: id.to_string(),
        configured,
        ok,
        message_key,
        runtime_status,
        runtime_reason,
    }
}

fn runtime_status_for_channel_item(
    id: &'static str,
    configured: bool,
    ok: bool,
    message_key: Option<&'static str>,
) -> (ChannelRuntimeStatus, Option<&'static str>) {
    if !configured {
        return (ChannelRuntimeStatus::Disabled, message_key);
    }
    if let Some(status) = wss_runtime_status(id) {
        return status;
    }
    if let Some(health) = crate::orchestrator::snapshot().channels.get(id) {
        if health.consecutive_failures > 0 {
            return (
                ChannelRuntimeStatus::CoolingDown,
                Some("channel_health_cooling_down"),
            );
        }
        if !health.healthy {
            return (ChannelRuntimeStatus::Failed, Some("channel_health_failed"));
        }
    }
    if ok {
        (ChannelRuntimeStatus::Connected, None)
    } else if message_key_is_failure(message_key) {
        (ChannelRuntimeStatus::Failed, message_key)
    } else {
        (ChannelRuntimeStatus::Configured, message_key)
    }
}

fn message_key_is_failure(message_key: Option<&'static str>) -> bool {
    if message_key == Some(CONNECTIVITY_CHECK_FAILED_KEY) {
        return true;
    }
    #[cfg(any(feature = "telegram", feature = "feishu", feature = "qq_channel"))]
    if message_key == Some(CONNECTIVITY_TOKEN_INVALID_KEY) {
        return true;
    }
    false
}

fn wss_runtime_status(id: &'static str) -> Option<(ChannelRuntimeStatus, Option<&'static str>)> {
    #[cfg(feature = "qq_channel")]
    if id == crate::CHANNEL_QQ_CHANNEL && crate::channels::is_ws_online() {
        return Some((ChannelRuntimeStatus::Connected, None));
    }
    let owner = wss_lifecycle_owner(id)?;
    let record = crate::runtime::plane_lifecycle::snapshot()
        .records
        .into_iter()
        .find(|record| {
            record.plane == crate::runtime::PlaneId::ChannelWss && record.owner == owner
        })?;
    let status = match record.state {
        crate::runtime::PlaneLifecycleState::Registered => ChannelRuntimeStatus::Configured,
        crate::runtime::PlaneLifecycleState::Starting => ChannelRuntimeStatus::Connecting,
        crate::runtime::PlaneLifecycleState::Active => ChannelRuntimeStatus::WorkerStarted,
        crate::runtime::PlaneLifecycleState::Suspended => {
            if record.last_reason == "wall_clock_untrusted" {
                ChannelRuntimeStatus::WaitingWallClock
            } else {
                ChannelRuntimeStatus::SuspendedByMode
            }
        }
        crate::runtime::PlaneLifecycleState::Draining
        | crate::runtime::PlaneLifecycleState::Stopping => ChannelRuntimeStatus::SuspendedByMode,
        crate::runtime::PlaneLifecycleState::Disabled
        | crate::runtime::PlaneLifecycleState::Unloaded => ChannelRuntimeStatus::Configured,
        crate::runtime::PlaneLifecycleState::Failed => ChannelRuntimeStatus::Failed,
    };
    Some((status, Some(record.last_reason)))
}

fn wss_lifecycle_owner(id: &'static str) -> Option<&'static str> {
    #[cfg(feature = "qq_channel")]
    if id == crate::CHANNEL_QQ_CHANNEL {
        return Some("qq_ws");
    }
    #[cfg(feature = "feishu")]
    if id == crate::CHANNEL_FEISHU {
        return Some("feishu_ws");
    }
    #[cfg(feature = "dingtalk")]
    if id == crate::CHANNEL_DINGTALK {
        return Some("dingtalk_stream");
    }
    #[cfg(feature = "wecom")]
    if id == crate::CHANNEL_WECOM {
        return Some("wecom_aibot");
    }
    let _ = id;
    None
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

    #[test]
    fn channel_runtime_status_reports_configured_and_failure_states() {
        let configured = item(
            "webhook",
            true,
            false,
            Some(CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY),
        );
        assert_eq!(configured.runtime_status, ChannelRuntimeStatus::Configured);
        assert_eq!(
            configured.runtime_reason,
            Some(CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY)
        );

        let failed = item("webhook", true, false, Some(CONNECTIVITY_CHECK_FAILED_KEY));
        assert_eq!(failed.runtime_status, ChannelRuntimeStatus::Failed);

        let connected = item("webhook", true, true, None);
        assert_eq!(connected.runtime_status, ChannelRuntimeStatus::Connected);

        let disabled = item(
            "webhook",
            false,
            false,
            Some(CONNECTIVITY_NOT_CONFIGURED_KEY),
        );
        assert_eq!(disabled.runtime_status, ChannelRuntimeStatus::Disabled);
    }

    #[cfg(feature = "qq_channel")]
    #[test]
    fn channel_runtime_status_consumes_wss_lifecycle_for_api_snapshot() {
        let _lifecycle_guard = crate::runtime::plane_lifecycle::plane_lifecycle_test_guard();
        crate::runtime::plane_lifecycle::mark(
            crate::runtime::PlaneId::ChannelWss,
            "qq_ws",
            crate::runtime::PlaneLifecycleState::Suspended,
            "wall_clock_untrusted",
        );

        let item = item(
            crate::CHANNEL_QQ_CHANNEL,
            true,
            false,
            Some(CHANNEL_CONNECTIVITY_UNAVAILABLE_KEY),
        );

        assert_eq!(item.runtime_status, ChannelRuntimeStatus::WaitingWallClock);
        assert_eq!(item.runtime_reason, Some("wall_clock_untrusted"));
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
