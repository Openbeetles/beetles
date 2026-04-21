//! Channel capability contract and runtime snapshot.
//! 通道能力合同与运行态快照。

use crate::bus::MAX_CONTENT_LEN;
use crate::config::AppConfig;
use serde::Serialize;
use std::collections::HashMap;

const TELEGRAM_MAX_TEXT_BYTES: usize = 4096;
const FEISHU_MAX_TEXT_BYTES: usize = 4096;
const DINGTALK_MAX_TEXT_BYTES: usize = 4096;
const WECOM_MAX_TEXT_BYTES: usize = 2048;
const QQ_CHANNEL_MAX_TEXT_BYTES: usize = 4096;

pub const CHANNEL_TELEGRAM: &str = "telegram";
pub const CHANNEL_FEISHU: &str = "feishu";
pub const CHANNEL_DINGTALK: &str = "dingtalk";
pub const CHANNEL_WECOM: &str = "wecom";
pub const CHANNEL_QQ_CHANNEL: &str = "qq_channel";
pub const CHANNEL_WEBSOCKET: &str = "websocket";
pub const CHANNEL_VOICE: &str = "voice";

const CHANNEL_CAPABILITY_ORDER: [&str; 7] = [
    CHANNEL_TELEGRAM,
    CHANNEL_FEISHU,
    CHANNEL_DINGTALK,
    CHANNEL_WECOM,
    CHANNEL_QQ_CHANNEL,
    CHANNEL_WEBSOCKET,
    CHANNEL_VOICE,
];

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
    pub max_text_bytes: usize,
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
        CHANNEL_CAPABILITY_ORDER
            .iter()
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
    pub max_text_bytes: usize,
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
    for channel in CHANNEL_CAPABILITY_ORDER {
        let Some(contract) = static_channel_capability_contract(channel) else {
            continue;
        };
        registry.insert(ChannelCapabilityEntry {
            id: channel,
            configured: channel_is_configured(channel, config, voice_channel_enabled),
            enabled: channel_is_enabled(channel, config, voice_channel_enabled),
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
                max_text_bytes: entry.contract.max_text_bytes,
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
        CHANNEL_TELEGRAM => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: true,
            supports_stream_edit: true,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: true,
            max_text_bytes: TELEGRAM_MAX_TEXT_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::EditableSingleMessage,
        }),
        CHANNEL_FEISHU => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: true,
            supports_stream_edit: true,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            max_text_bytes: FEISHU_MAX_TEXT_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::EditableSingleMessage,
        }),
        CHANNEL_DINGTALK => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: false,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            max_text_bytes: DINGTALK_MAX_TEXT_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::StatelessWebhook,
        }),
        CHANNEL_WECOM => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            max_text_bytes: WECOM_MAX_TEXT_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
        }),
        CHANNEL_QQ_CHANNEL => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            max_text_bytes: QQ_CHANNEL_MAX_TEXT_BYTES,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AppendOnly,
        }),
        CHANNEL_WEBSOCKET => Some(ChannelCapabilityContract {
            supports_primary_reply: true,
            supports_supplemental_reply: true,
            supports_edit: false,
            supports_stream_edit: false,
            supports_explicit_target: true,
            supports_attachment: false,
            supports_typing_or_chat_action: false,
            max_text_bytes: MAX_CONTENT_LEN,
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
            max_text_bytes: MAX_CONTENT_LEN,
            delivery_ordering_model: ChannelDeliveryOrderingModel::AudioPlayback,
        }),
        _ => None,
    }
}

fn channel_is_configured(channel: &str, config: &AppConfig, voice_channel_enabled: bool) -> bool {
    match channel {
        CHANNEL_TELEGRAM => !config.tg_token.trim().is_empty(),
        CHANNEL_FEISHU => {
            !config.feishu_app_id.trim().is_empty() && !config.feishu_app_secret.trim().is_empty()
        }
        CHANNEL_DINGTALK => !config.dingtalk_webhook_url.trim().is_empty(),
        CHANNEL_WECOM => {
            !config.wecom_corp_id.trim().is_empty()
                && !config.wecom_corp_secret.trim().is_empty()
                && config.wecom_agent_id.trim().parse::<u32>().is_ok()
        }
        CHANNEL_QQ_CHANNEL => {
            !config.qq_channel_app_id.trim().is_empty()
                && !config.qq_channel_secret.trim().is_empty()
        }
        CHANNEL_WEBSOCKET => true,
        CHANNEL_VOICE => voice_channel_enabled,
        _ => false,
    }
}

fn channel_is_enabled(channel: &str, config: &AppConfig, voice_channel_enabled: bool) -> bool {
    match channel {
        CHANNEL_WEBSOCKET => true,
        CHANNEL_VOICE => voice_channel_enabled,
        _ => {
            channel_is_configured(channel, config, voice_channel_enabled)
                && config.enabled_channel == channel
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_marks_only_enabled_text_channel_as_active() {
        let mut config = AppConfig::load_from_env();
        config.enabled_channel = CHANNEL_TELEGRAM.to_string();
        config.tg_token = "tg-token".to_string();
        config.qq_channel_app_id = "qq-app".to_string();
        config.qq_channel_secret = "qq-secret".to_string();

        let registry = build_channel_capability_registry(&config, true);

        let telegram = registry.get(CHANNEL_TELEGRAM).expect("telegram capability");
        assert!(telegram.configured);
        assert!(telegram.enabled);
        assert!(telegram.contract.supports_stream_edit);

        let qq = registry
            .get(CHANNEL_QQ_CHANNEL)
            .expect("qq capability should exist");
        assert!(qq.configured);
        assert!(!qq.enabled);
        assert!(qq.contract.supports_explicit_target);

        let voice = registry.get(CHANNEL_VOICE).expect("voice capability");
        assert!(voice.enabled);

        let websocket = registry
            .get(CHANNEL_WEBSOCKET)
            .expect("websocket capability");
        assert!(websocket.enabled);
    }

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
        assert!(feishu.degraded_reasons.is_empty());
    }
}
