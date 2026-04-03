//! 通道连通性检查：按配置逐通道单次 HTTP 探测，供 GET /api/channel_connectivity 使用。
//! 不依赖 Platform，仅依赖 ChannelHttpClient 与 AppConfig。
//!
//! 各通道实现 check_connectivity，本模块仅按固定顺序收集并返回列表。
//! 调用方（前端或网关）应设置合理 HTTP 超时。

use crate::config::AppConfig;
use crate::i18n::{tr, Locale, Message};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

#[derive(Default)]
pub struct ChannelConnectivityCache {
    inner: Mutex<Option<ChannelConnectivitySnapshot>>,
    refresh_in_flight: AtomicBool,
}

impl ChannelConnectivityCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            refresh_in_flight: AtomicBool::new(false),
        }
    }

    pub fn publish_success(
        &self,
        channels: Vec<ChannelConnectivityItem>,
        checked_at_unix_secs: u64,
    ) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ChannelConnectivitySnapshot {
            channels,
            checked_at_unix_secs: Some(checked_at_unix_secs),
            stale: false,
        });
    }

    pub fn try_start_refresh(&self) -> bool {
        self.refresh_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub fn finish_refresh(&self) {
        self.refresh_in_flight.store(false, Ordering::Release);
    }

    pub fn snapshot_or_fallback(
        &self,
        config: &AppConfig,
        loc: Locale,
        now_unix_secs: u64,
        max_age_secs: u64,
    ) -> ChannelConnectivitySnapshot {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .map(|snapshot| mark_snapshot_staleness(snapshot, now_unix_secs, max_age_secs))
            .unwrap_or_else(|| unavailable_snapshot(config, loc))
    }
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

/// 按固定顺序检查各通道，返回列表；未配置的通道也列入，configured=false。
pub fn check_all<H: crate::channels::ChannelHttpClient + ?Sized>(
    config: &AppConfig,
    http: &mut H,
    loc: Locale,
) -> Vec<ChannelConnectivityItem> {
    let mut out = Vec::with_capacity(6);
    out.push(crate::channels::telegram::check_connectivity(
        config, http, loc,
    ));
    out.push(crate::channels::feishu::check_connectivity(
        config, http, loc,
    ));
    out.push(crate::channels::dingtalk::check_connectivity(
        config, http, loc,
    ));
    out.push(crate::channels::wecom::check_connectivity(
        config, http, loc,
    ));
    out.push(crate::channels::qq::check_connectivity(config, http, loc));
    let configured = webhook_configured(config);
    let msg = if configured {
        None
    } else {
        Some(tr(Message::ConnectivityNotConfigured, loc))
    };
    out.push(item("webhook", configured, configured, msg));
    out
}

pub fn refresh_cached_connectivity<F>(
    cache: &ChannelConnectivityCache,
    config: &AppConfig,
    loc: Locale,
    create_http: F,
) -> crate::Result<()>
where
    F: FnOnce() -> crate::Result<Box<dyn crate::PlatformHttpClient>>,
{
    let mut http = create_http()?;
    let channels = check_all(config, &mut *http, loc);
    cache.publish_success(channels, crate::util::current_unix_secs());
    Ok(())
}

fn mark_snapshot_staleness(
    mut snapshot: ChannelConnectivitySnapshot,
    now_unix_secs: u64,
    max_age_secs: u64,
) -> ChannelConnectivitySnapshot {
    snapshot.stale = snapshot
        .checked_at_unix_secs
        .is_none_or(|checked_at| now_unix_secs.saturating_sub(checked_at) > max_age_secs);
    snapshot
}

fn unavailable_snapshot(config: &AppConfig, loc: Locale) -> ChannelConnectivitySnapshot {
    let unavailable = tr(Message::ChannelConnectivityUnavailable, loc);
    ChannelConnectivitySnapshot {
        channels: vec![
            unavailable_item("telegram", telegram_configured(config), loc, &unavailable),
            unavailable_item("feishu", feishu_configured(config), loc, &unavailable),
            unavailable_item("dingtalk", dingtalk_configured(config), loc, &unavailable),
            unavailable_item("wecom", wecom_configured(config), loc, &unavailable),
            unavailable_item(
                "qq_channel",
                qq_channel_configured(config),
                loc,
                &unavailable,
            ),
            unavailable_item("webhook", webhook_configured(config), loc, &unavailable),
        ],
        checked_at_unix_secs: None,
        stale: true,
    }
}

fn unavailable_item(
    id: &'static str,
    configured: bool,
    loc: Locale,
    unavailable: &str,
) -> ChannelConnectivityItem {
    if configured {
        item(id, true, false, Some(unavailable.to_string()))
    } else {
        item(
            id,
            false,
            false,
            Some(tr(Message::ConnectivityNotConfigured, loc)),
        )
    }
}

fn telegram_configured(config: &AppConfig) -> bool {
    !config.tg_token.trim().is_empty()
}

fn feishu_configured(config: &AppConfig) -> bool {
    !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty()
}

fn dingtalk_configured(config: &AppConfig) -> bool {
    !config.dingtalk_webhook_url.trim().is_empty()
}

fn wecom_configured(config: &AppConfig) -> bool {
    !config.wecom_corp_id.trim().is_empty()
        && !config.wecom_corp_secret.trim().is_empty()
        && config.wecom_agent_id.trim().parse::<u32>().is_ok()
}

fn qq_channel_configured(config: &AppConfig) -> bool {
    !config.qq_channel_app_id.trim().is_empty() && !config.qq_channel_secret.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured_config() -> AppConfig {
        let mut config = AppConfig::load_from_env();
        config.tg_token = "tg-token".to_string();
        config
    }

    #[test]
    fn snapshot_without_cache_is_stale_fallback() {
        let cache = ChannelConnectivityCache::new();
        let snapshot = cache.snapshot_or_fallback(&configured_config(), Locale::Zh, 100, 60);
        assert!(snapshot.stale);
        assert_eq!(snapshot.channels[0].id, "telegram");
        assert!(!snapshot.channels[0].ok);
    }

    #[test]
    fn snapshot_marks_old_success_as_stale() {
        let cache = ChannelConnectivityCache::new();
        cache.publish_success(vec![item("telegram", true, true, None)], 10);

        let fresh = cache.snapshot_or_fallback(&configured_config(), Locale::Zh, 50, 60);
        assert!(!fresh.stale);

        let stale = cache.snapshot_or_fallback(&configured_config(), Locale::Zh, 100, 60);
        assert!(stale.stale);
        assert!(stale.channels[0].ok);
    }

    #[test]
    fn refresh_claim_is_single_flight() {
        let cache = ChannelConnectivityCache::new();
        assert!(cache.try_start_refresh());
        assert!(!cache.try_start_refresh());
        cache.finish_refresh();
        assert!(cache.try_start_refresh());
    }
}
