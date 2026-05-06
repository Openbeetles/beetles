//! Channel capability contract and runtime snapshot.
//! 通道能力合同与运行态快照。

use crate::bus::{MessageBodyKind, TextFormat, MAX_CONTENT_LEN};
use crate::channel_catalog;
use crate::config::AppConfig;
use serde::Serialize;
use std::collections::HashMap;

#[cfg(feature = "telegram")]
const TELEGRAM_MAX_TEXT_BYTES: usize = 4096;
#[cfg(feature = "telegram")]
const TELEGRAM_MAX_CAPTION_BYTES: usize = 1024;
#[cfg(feature = "feishu")]
const FEISHU_MAX_TEXT_BYTES: usize = 4096;
#[cfg(feature = "feishu")]
const FEISHU_MAX_CAPTION_BYTES: usize = 0;
#[cfg(feature = "dingtalk")]
const DINGTALK_MAX_TEXT_BYTES: usize = 4096;
#[cfg(feature = "dingtalk")]
const DINGTALK_MAX_CAPTION_BYTES: usize = 0;
#[cfg(feature = "wecom")]
const WECOM_MAX_TEXT_BYTES: usize = 2048;
#[cfg(feature = "wecom")]
const WECOM_MAX_CAPTION_BYTES: usize = 0;
#[cfg(feature = "qq_channel")]
const QQ_CHANNEL_MAX_TEXT_BYTES: usize = 4096;
#[cfg(feature = "qq_channel")]
const QQ_CHANNEL_MAX_CAPTION_BYTES: usize = 0;

const BODY_KINDS_TEXT_ONLY: &[MessageBodyKind] = &[MessageBodyKind::Text];
#[cfg(feature = "telegram")]
const BODY_KINDS_TELEGRAM: &[MessageBodyKind] = &[
    MessageBodyKind::Text,
    MessageBodyKind::Image,
    MessageBodyKind::Audio,
    MessageBodyKind::Video,
    MessageBodyKind::File,
];
#[cfg(feature = "feishu")]
const BODY_KINDS_FEISHU: &[MessageBodyKind] = &[
    MessageBodyKind::Text,
    MessageBodyKind::Image,
    MessageBodyKind::Audio,
    MessageBodyKind::Video,
    MessageBodyKind::File,
    MessageBodyKind::Card,
];
#[cfg(feature = "dingtalk")]
const BODY_KINDS_DINGTALK: &[MessageBodyKind] = &[
    MessageBodyKind::Text,
    MessageBodyKind::Image,
    MessageBodyKind::Audio,
    MessageBodyKind::Video,
    MessageBodyKind::File,
    MessageBodyKind::Card,
];
#[cfg(feature = "wecom")]
const BODY_KINDS_WECOM: &[MessageBodyKind] = &[
    MessageBodyKind::Text,
    MessageBodyKind::Image,
    MessageBodyKind::Audio,
    MessageBodyKind::Video,
    MessageBodyKind::File,
    MessageBodyKind::Card,
];
#[cfg(feature = "qq_channel")]
const BODY_KINDS_QQ: &[MessageBodyKind] = &[
    MessageBodyKind::Text,
    MessageBodyKind::Image,
    MessageBodyKind::Audio,
    MessageBodyKind::Video,
    MessageBodyKind::File,
    MessageBodyKind::Card,
];

const TEXT_FORMATS_PLAIN_ONLY: &[TextFormat] = &[TextFormat::Plain];
#[cfg(feature = "telegram")]
const TEXT_FORMATS_TELEGRAM: &[TextFormat] =
    &[TextFormat::Plain, TextFormat::Markdown, TextFormat::Html];
#[cfg(feature = "feishu")]
const TEXT_FORMATS_FEISHU: &[TextFormat] = &[TextFormat::Plain, TextFormat::RichText];
#[cfg(feature = "dingtalk")]
const TEXT_FORMATS_DINGTALK: &[TextFormat] = &[
    TextFormat::Plain,
    TextFormat::Markdown,
    TextFormat::RichText,
];
#[cfg(feature = "wecom")]
const TEXT_FORMATS_WECOM: &[TextFormat] = &[TextFormat::Plain, TextFormat::Markdown];
#[cfg(feature = "qq_channel")]
const TEXT_FORMATS_QQ: &[TextFormat] = &[TextFormat::Plain];

pub const CHANNEL_TELEGRAM: &str = "telegram";
pub const CHANNEL_FEISHU: &str = "feishu";
pub const CHANNEL_DINGTALK: &str = "dingtalk";
pub const CHANNEL_WECOM: &str = "wecom";
pub const CHANNEL_QQ_CHANNEL: &str = "qq_channel";
pub const CHANNEL_WEBSOCKET: &str = "websocket";
pub const CHANNEL_VOICE: &str = "voice";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelDeliveryOrderingModel {
    AppendOnly,
    EditableSingleMessage,
    StatelessWebhook,
    SessionSocket,
    AudioPlayback,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ChannelCapabilityContract {
    pub supports_primary_reply: bool,
    pub supports_supplemental_reply: bool,
    pub supports_edit: bool,
    pub supports_stream_edit: bool,
    pub supports_explicit_target: bool,
    pub supports_attachment: bool,
    pub supports_typing_or_chat_action: bool,
    pub supported_body_kinds: &'static [MessageBodyKind],
    pub supported_text_formats: &'static [TextFormat],
    pub requires_pre_upload_for_media: bool,
    pub supports_platform_handle_reuse: bool,
    pub supports_http_url_media: bool,
    pub requires_passive_reply_anchor: bool,
    pub max_text_bytes: usize,
    pub max_caption_bytes: usize,
    pub delivery_ordering_model: ChannelDeliveryOrderingModel,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ChannelCapabilityEntry {
    pub id: &'static str,
    pub configured: bool,
    pub enabled: bool,
    pub contract: ChannelCapabilityContract,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelCapabilityRegistry {
    entries: HashMap<&'static str, ChannelCapabilityEntry>,
}

impl ChannelCapabilityRegistry {
    pub fn get(&self, channel: &str) -> Option<ChannelCapabilityEntry> {
        self.entries.get(channel).copied()
    }

    pub fn list(&self) -> Vec<ChannelCapabilityEntry> {
        channel_catalog::compiled_channel_entries()
            .iter()
            .map(|entry| entry.id)
            .filter_map(|channel| self.entries.get(channel).copied())
            .collect()
    }

    fn insert(&mut self, entry: ChannelCapabilityEntry) {
        self.entries.insert(entry.id, entry);
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ChannelCapabilitySnapshot {
    pub id: String,
    pub configured: bool,
    pub enabled: bool,
    pub supports_primary_reply: bool,
    pub supports_supplemental_reply: bool,
    pub supports_edit: bool,
    pub supports_stream_edit: bool,
    pub supports_explicit_target: bool,
    pub supports_attachment: bool,
    pub supports_typing_or_chat_action: bool,
    pub supported_body_kinds: Vec<MessageBodyKind>,
    pub supported_text_formats: Vec<TextFormat>,
    pub requires_pre_upload_for_media: bool,
    pub supports_platform_handle_reuse: bool,
    pub supports_http_url_media: bool,
    pub requires_passive_reply_anchor: bool,
    pub max_text_bytes: usize,
    pub max_caption_bytes: usize,
    pub delivery_ordering_model: ChannelDeliveryOrderingModel,
    pub stream_edit_active: bool,
    pub typing_active: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<String>,
}

pub fn build_channel_capability_registry(
    config: &AppConfig,
    voice_channel_enabled: bool,
) -> ChannelCapabilityRegistry {
    let mut registry = ChannelCapabilityRegistry::default();
    let normalized_enabled =
        channel_catalog::normalize_compiled_enabled_channel(&config.enabled_channel);
    for channel in channel_catalog::compiled_channel_entries()
        .iter()
        .map(|entry| entry.id)
    {
        let Some(contract) = static_channel_capability_contract(channel) else {
            continue;
        };
        registry.insert(ChannelCapabilityEntry {
            id: channel,
            configured: channel_is_configured(channel, config, voice_channel_enabled),
            enabled: channel_is_enabled(channel, normalized_enabled, config, voice_channel_enabled),
            contract,
        });
    }
    registry
}

/// Build operator/runtime snapshots from the current config.
/// 根据当前配置构建通道能力快照。
pub fn build_channel_capability_snapshots(
    config: &AppConfig,
    voice_channel_enabled: bool,
) -> Vec<ChannelCapabilitySnapshot> {
    let registry = build_channel_capability_registry(config, voice_channel_enabled);
    build_channel_capability_snapshots_for_registry(&registry)
}

/// Build operator/runtime snapshots from an already materialized registry.
/// 基于已构建的 registry 生成运行态/运维快照。
pub fn build_channel_capability_snapshots_for_registry(
    registry: &ChannelCapabilityRegistry,
) -> Vec<ChannelCapabilitySnapshot> {
    registry
        .list()
        .into_iter()
        .map(|entry| {
            let stream_edit_active = entry.enabled && entry.contract.supports_stream_edit;
            let typing_active = entry.enabled && entry.contract.supports_typing_or_chat_action;
            ChannelCapabilitySnapshot {
                id: entry.id.to_string(),
                configured: entry.configured,
                enabled: entry.enabled,
                supports_primary_reply: entry.contract.supports_primary_reply,
                supports_supplemental_reply: entry.contract.supports_supplemental_reply,
                supports_edit: entry.contract.supports_edit,
                supports_stream_edit: entry.contract.supports_stream_edit,
                supports_explicit_target: entry.contract.supports_explicit_target,
                supports_attachment: entry.contract.supports_attachment,
                supports_typing_or_chat_action: entry.contract.supports_typing_or_chat_action,
                supported_body_kinds: entry.contract.supported_body_kinds.to_vec(),
                supported_text_formats: entry.contract.supported_text_formats.to_vec(),
                requires_pre_upload_for_media: entry.contract.requires_pre_upload_for_media,
                supports_platform_handle_reuse: entry.contract.supports_platform_handle_reuse,
                supports_http_url_media: entry.contract.supports_http_url_media,
                requires_passive_reply_anchor: entry.contract.requires_passive_reply_anchor,
                max_text_bytes: entry.contract.max_text_bytes,
                max_caption_bytes: entry.contract.max_caption_bytes,
                delivery_ordering_model: entry.contract.delivery_ordering_model,
                stream_edit_active,
                typing_active,
                degraded_reasons: Vec::new(),
            }
        })
        .collect()
}

fn static_channel_capability_contract(channel: &str) -> Option<ChannelCapabilityContract> {
    match channel {
        #[cfg(feature = "telegram")]
        CHANNEL_TELEGRAM => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: true,
            supports_typing_or_chat_action: true,
            supported_body_kinds: BODY_KINDS_TELEGRAM,
            supported_text_formats: TEXT_FORMATS_TELEGRAM,
            requires_pre_upload_for_media: false,
            supports_platform_handle_reuse: true,
            supports_http_url_media: true,
            requires_passive_reply_anchor: false,
            max_text_bytes: TELEGRAM_MAX_TEXT_BYTES,
            max_caption_bytes: TELEGRAM_MAX_CAPTION_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
        }),
        #[cfg(feature = "feishu")]
        CHANNEL_FEISHU => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: true,
            supports_stream_edit: true,
            supports_explicit_target: true,
            supports_attachment: true,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_FEISHU,
            supported_text_formats: TEXT_FORMATS_FEISHU,
            requires_pre_upload_for_media: true,
            supports_platform_handle_reuse: true,
            supports_http_url_media: false,
            requires_passive_reply_anchor: false,
            max_text_bytes: FEISHU_MAX_TEXT_BYTES,
            max_caption_bytes: FEISHU_MAX_CAPTION_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::EditableSingleMessage,
        }),
        #[cfg(feature = "dingtalk")]
        CHANNEL_DINGTALK => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: false,
            supports_attachment: true,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_DINGTALK,
            supported_text_formats: TEXT_FORMATS_DINGTALK,
            requires_pre_upload_for_media: true,
            supports_platform_handle_reuse: true,
            supports_http_url_media: false,
            requires_passive_reply_anchor: false,
            max_text_bytes: DINGTALK_MAX_TEXT_BYTES,
            max_caption_bytes: DINGTALK_MAX_CAPTION_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::StatelessWebhook,
        }),
        #[cfg(feature = "wecom")]
        CHANNEL_WECOM => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: true,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_WECOM,
            supported_text_formats: TEXT_FORMATS_WECOM,
            requires_pre_upload_for_media: true,
            supports_platform_handle_reuse: true,
            supports_http_url_media: false,
            requires_passive_reply_anchor: false,
            max_text_bytes: WECOM_MAX_TEXT_BYTES,
            max_caption_bytes: WECOM_MAX_CAPTION_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
        }),
        #[cfg(feature = "qq_channel")]
        CHANNEL_QQ_CHANNEL => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: true,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_QQ,
            supported_text_formats: TEXT_FORMATS_QQ,
            requires_pre_upload_for_media: true,
            supports_platform_handle_reuse: true,
            supports_http_url_media: false,
            requires_passive_reply_anchor: true,
            max_text_bytes: QQ_CHANNEL_MAX_TEXT_BYTES,
            max_caption_bytes: QQ_CHANNEL_MAX_CAPTION_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
        }),
        #[cfg(feature = "websocket")]
        CHANNEL_WEBSOCKET => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_TEXT_ONLY,
            supported_text_formats: TEXT_FORMATS_PLAIN_ONLY,
            requires_pre_upload_for_media: false,
            supports_platform_handle_reuse: false,
            supports_http_url_media: false,
            requires_passive_reply_anchor: false,
            max_text_bytes: MAX_CONTENT_LEN,
            max_caption_bytes: 0,
            delivery_ordering_model: ChannelDeliveryOrderingModel::SessionSocket,
        }),
        CHANNEL_VOICE => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: false,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: false,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            supported_body_kinds: BODY_KINDS_TEXT_ONLY,
            supported_text_formats: TEXT_FORMATS_PLAIN_ONLY,
            requires_pre_upload_for_media: false,
            supports_platform_handle_reuse: false,
            supports_http_url_media: false,
            requires_passive_reply_anchor: false,
            max_text_bytes: MAX_CONTENT_LEN,
            max_caption_bytes: 0,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AudioPlayback,
        }),
        _ => None,
    }
}

fn channel_is_configured(channel: &str, config: &AppConfig, voice_channel_enabled: bool) -> bool {
    #[cfg(not(any(
        feature = "telegram",
        feature = "feishu",
        feature = "dingtalk",
        feature = "wecom",
        feature = "qq_channel"
    )))]
    let _ = config;
    match channel {
        #[cfg(feature = "telegram")]
        CHANNEL_TELEGRAM => !config.tg_token.trim().is_empty(),
        #[cfg(feature = "feishu")]
        CHANNEL_FEISHU => {
            !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty()
        }
        #[cfg(feature = "dingtalk")]
        CHANNEL_DINGTALK => {
            !config.dingtalk_client_id.trim().is_empty()
                && !config.dingtalk_client_secret.trim().is_empty()
        }
        #[cfg(feature = "wecom")]
        CHANNEL_WECOM => {
            !config.wecom_bot_id.trim().is_empty() && !config.wecom_bot_secret.trim().is_empty()
        }
        #[cfg(feature = "qq_channel")]
        CHANNEL_QQ_CHANNEL => {
            !config.qq_channel_app_id.trim().is_empty()
                && !config.qq_channel_secret.trim().is_empty()
        }
        #[cfg(feature = "websocket")]
        CHANNEL_WEBSOCKET => true,
        CHANNEL_VOICE => voice_channel_enabled,
        _ => false,
    }
}

fn channel_is_enabled(
    channel: &str,
    normalized_enabled_channel: &str,
    config: &AppConfig,
    voice_channel_enabled: bool,
) -> bool {
    match channel {
        #[cfg(feature = "websocket")]
        CHANNEL_WEBSOCKET => true,
        CHANNEL_VOICE => voice_channel_enabled,
        _ => {
            channel_is_configured(channel, config, voice_channel_enabled)
                && normalized_enabled_channel == channel
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "telegram")]
    #[test]
    fn registry_marks_only_enabled_text_channel_as_active() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_TELEGRAM.to_string();
        config.tg_token = "tg-token".to_string();
        #[cfg(feature = "qq_channel")]
        {
            config.qq_channel_app_id = "qq-app".to_string();
            config.qq_channel_secret = "qq-secret".to_string();
        }

        let registry = build_channel_capability_registry(&config, true);

        let telegram = registry.get(CHANNEL_TELEGRAM).expect("telegram capability");
        assert!(telegram.configured);
        assert!(telegram.enabled);
        assert!(!telegram.contract.supports_stream_edit);
        assert_eq!(telegram.contract.supported_body_kinds, BODY_KINDS_TELEGRAM);
        assert_eq!(
            telegram.contract.supported_text_formats,
            TEXT_FORMATS_TELEGRAM
        );

        #[cfg(feature = "qq_channel")]
        {
            let qq = registry
                .get(CHANNEL_QQ_CHANNEL)
                .expect("qq capability should exist");
            assert!(qq.configured);
            assert!(!qq.enabled);
            assert!(qq.contract.supports_explicit_target);
            assert!(qq.contract.requires_passive_reply_anchor);
            assert_eq!(qq.contract.supported_text_formats, TEXT_FORMATS_QQ);
        }

        #[cfg(not(feature = "qq_channel"))]
        assert!(registry.get(CHANNEL_QQ_CHANNEL).is_none());

        let voice = registry.get(CHANNEL_VOICE).expect("voice capability");
        assert!(voice.enabled);

        #[cfg(feature = "websocket")]
        {
            let websocket = registry
                .get(CHANNEL_WEBSOCKET)
                .expect("websocket capability");
            assert!(websocket.enabled);
        }

        #[cfg(not(feature = "websocket"))]
        assert!(registry.get(CHANNEL_WEBSOCKET).is_none());
    }

    #[cfg(not(feature = "telegram"))]
    #[test]
    fn registry_skips_uncompiled_telegram_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_TELEGRAM.to_string();
        config.tg_token = "tg-token".to_string();

        let registry = build_channel_capability_registry(&config, true);

        assert!(registry.get(CHANNEL_TELEGRAM).is_none());
        assert!(!registry
            .list()
            .iter()
            .any(|entry| entry.id == CHANNEL_TELEGRAM));
    }

    #[cfg(feature = "feishu")]
    #[test]
    fn snapshots_activate_stream_edit_for_enabled_editable_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_FEISHU.to_string();
        config.feishu_app_id = "app".to_string();
        config.feishu_app_secret = "secret".to_string();

        let snapshots = build_channel_capability_snapshots(&config, false);
        let feishu = snapshots
            .into_iter()
            .find(|snapshot| snapshot.id == CHANNEL_FEISHU)
            .expect("feishu snapshot");

        assert!(feishu.enabled);
        assert!(feishu.stream_edit_active);
        assert_eq!(feishu.supported_body_kinds, BODY_KINDS_FEISHU);
        assert_eq!(feishu.supported_text_formats, TEXT_FORMATS_FEISHU);
        assert!(feishu.degraded_reasons.is_empty());
    }

    #[cfg(not(feature = "feishu"))]
    #[test]
    fn snapshots_skip_uncompiled_feishu_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_FEISHU.to_string();
        config.feishu_app_id = "app".to_string();
        config.feishu_app_secret = "secret".to_string();

        let snapshots = build_channel_capability_snapshots(&config, false);
        assert!(!snapshots
            .iter()
            .any(|snapshot| snapshot.id == CHANNEL_FEISHU));
    }

    #[cfg(not(feature = "wecom"))]
    #[test]
    fn registry_skips_uncompiled_wecom_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_WECOM.to_string();
        config.wecom_bot_id = "bot-id".to_string();
        config.wecom_bot_secret = "bot-secret".to_string();

        let registry = build_channel_capability_registry(&config, true);

        assert!(registry.get(CHANNEL_WECOM).is_none());
        assert!(!registry
            .list()
            .iter()
            .any(|entry| entry.id == CHANNEL_WECOM));
    }

    #[cfg(not(feature = "dingtalk"))]
    #[test]
    fn registry_skips_uncompiled_dingtalk_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_DINGTALK.to_string();
        config.dingtalk_client_id = "ding-client".to_string();
        config.dingtalk_client_secret = "ding-secret".to_string();

        let registry = build_channel_capability_registry(&config, true);

        assert!(registry.get(CHANNEL_DINGTALK).is_none());
        assert!(!registry
            .list()
            .iter()
            .any(|entry| entry.id == CHANNEL_DINGTALK));
    }

    #[cfg(not(feature = "qq_channel"))]
    #[test]
    fn registry_skips_uncompiled_qq_channel() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_QQ_CHANNEL.to_string();
        config.qq_channel_app_id = "qq-app".to_string();
        config.qq_channel_secret = "qq-secret".to_string();

        let registry = build_channel_capability_registry(&config, true);

        assert!(registry.get(CHANNEL_QQ_CHANNEL).is_none());
        assert!(!registry
            .list()
            .iter()
            .any(|entry| entry.id == CHANNEL_QQ_CHANNEL));
    }

    #[cfg(not(feature = "websocket"))]
    #[test]
    fn registry_skips_uncompiled_websocket_channel() {
        let registry = build_channel_capability_registry(&AppConfig::load_from_env(), true);

        assert!(registry.get(CHANNEL_WEBSOCKET).is_none());
        assert!(!registry
            .list()
            .iter()
            .any(|entry| entry.id == CHANNEL_WEBSOCKET));
    }
}
