//! Compile-time channel catalog for runtime/config/display surfaces.
//! 编译期通道目录：为 capability / config / display / connectivity 提供单一真源。

#[cfg(all(
    feature = "dingtalk",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::channel_capability::CHANNEL_DINGTALK;
#[cfg(feature = "feishu")]
use crate::channel_capability::CHANNEL_FEISHU;
#[cfg(feature = "qq_channel")]
use crate::channel_capability::CHANNEL_QQ_CHANNEL;
#[cfg(feature = "telegram")]
use crate::channel_capability::CHANNEL_TELEGRAM;
use crate::channel_capability::CHANNEL_VOICE;
#[cfg(feature = "websocket")]
use crate::channel_capability::CHANNEL_WEBSOCKET;
#[cfg(feature = "wecom")]
use crate::channel_capability::CHANNEL_WECOM;

pub const DISPLAY_CHANNEL_CAPACITY: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompiledChannelEntry {
    pub id: &'static str,
    pub display_label: &'static str,
    pub show_in_connectivity: bool,
    pub show_on_display: bool,
}

#[cfg(feature = "telegram")]
const TELEGRAM_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_TELEGRAM,
    display_label: "TG",
    show_in_connectivity: true,
    show_on_display: true,
};

#[cfg(feature = "feishu")]
const FEISHU_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_FEISHU,
    display_label: "FS",
    show_in_connectivity: true,
    show_on_display: true,
};

#[cfg(all(
    feature = "dingtalk",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
const DINGTALK_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_DINGTALK,
    display_label: "DT",
    show_in_connectivity: true,
    show_on_display: true,
};

#[cfg(feature = "wecom")]
const WECOM_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_WECOM,
    display_label: "WC",
    show_in_connectivity: true,
    show_on_display: true,
};

#[cfg(feature = "qq_channel")]
const QQ_CHANNEL_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_QQ_CHANNEL,
    display_label: "QQ",
    show_in_connectivity: true,
    show_on_display: true,
};

#[cfg(feature = "websocket")]
const WEBSOCKET_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_WEBSOCKET,
    display_label: "WS",
    show_in_connectivity: false,
    show_on_display: false,
};

const VOICE_ENTRY: CompiledChannelEntry = CompiledChannelEntry {
    id: CHANNEL_VOICE,
    display_label: "VO",
    show_in_connectivity: false,
    show_on_display: false,
};

fn compiled_channel_entry(channel: &str) -> Option<CompiledChannelEntry> {
    compiled_channel_entries()
        .iter()
        .copied()
        .find(|entry| entry.id == channel)
}

pub fn compiled_channel_entries() -> &'static [CompiledChannelEntry] {
    &[
        #[cfg(feature = "telegram")]
        TELEGRAM_ENTRY,
        #[cfg(feature = "feishu")]
        FEISHU_ENTRY,
        #[cfg(all(
            feature = "dingtalk",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        DINGTALK_ENTRY,
        #[cfg(feature = "wecom")]
        WECOM_ENTRY,
        #[cfg(feature = "qq_channel")]
        QQ_CHANNEL_ENTRY,
        #[cfg(feature = "websocket")]
        WEBSOCKET_ENTRY,
        VOICE_ENTRY,
    ]
}

pub fn compiled_enabled_channel_ids() -> &'static [&'static str] {
    &[
        "",
        #[cfg(feature = "telegram")]
        CHANNEL_TELEGRAM,
        #[cfg(feature = "feishu")]
        CHANNEL_FEISHU,
        #[cfg(all(
            feature = "dingtalk",
            not(any(target_arch = "xtensa", target_arch = "riscv32"))
        ))]
        CHANNEL_DINGTALK,
        #[cfg(feature = "wecom")]
        CHANNEL_WECOM,
        #[cfg(feature = "qq_channel")]
        CHANNEL_QQ_CHANNEL,
    ]
}

pub fn selectable_channel_entries() -> impl Iterator<Item = CompiledChannelEntry> {
    compiled_enabled_channel_ids()
        .iter()
        .copied()
        .filter_map(compiled_channel_entry)
}

pub fn connectivity_channel_entries() -> impl Iterator<Item = CompiledChannelEntry> {
    compiled_channel_entries()
        .iter()
        .copied()
        .filter(|entry| entry.show_in_connectivity)
}

pub fn display_channel_entries() -> impl Iterator<Item = CompiledChannelEntry> {
    compiled_channel_entries()
        .iter()
        .copied()
        .filter(|entry| entry.show_on_display)
}

pub fn channel_is_compiled(channel: &str) -> bool {
    compiled_channel_entry(channel).is_some()
}

pub fn normalize_compiled_enabled_channel(channel: &str) -> &str {
    if compiled_enabled_channel_ids().contains(&channel) {
        channel
    } else {
        ""
    }
}

pub fn channel_display_label(channel: &str) -> Option<&'static str> {
    compiled_channel_entry(channel).map(|entry| entry.display_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_enabled_channel_ids_follow_selectable_catalog_order() {
        let ids = compiled_enabled_channel_ids();
        assert_eq!(ids.first().copied(), Some(""));
        let selectable = selectable_channel_entries()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        assert_eq!(&ids[1..], selectable.as_slice());
        #[cfg(feature = "dingtalk")]
        assert!(ids.contains(&CHANNEL_DINGTALK));
        #[cfg(not(feature = "dingtalk"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_DINGTALK));
        #[cfg(feature = "qq_channel")]
        assert!(ids.contains(&CHANNEL_QQ_CHANNEL));
        #[cfg(not(feature = "qq_channel"))]
        assert!(!ids.contains(&crate::channel_capability::CHANNEL_QQ_CHANNEL));
    }

    #[test]
    fn normalize_compiled_enabled_channel_rejects_unknown_values() {
        assert_eq!(normalize_compiled_enabled_channel("unknown"), "");
    }

    #[test]
    fn display_entries_fit_fixed_capacity() {
        assert!(display_channel_entries().count() <= DISPLAY_CHANNEL_CAPACITY);
    }

    #[cfg(feature = "telegram")]
    #[test]
    fn telegram_appears_when_feature_enabled() {
        assert!(compiled_enabled_channel_ids().contains(&CHANNEL_TELEGRAM));
        assert_eq!(channel_display_label(CHANNEL_TELEGRAM), Some("TG"));
    }

    #[cfg(not(feature = "telegram"))]
    #[test]
    fn telegram_disappears_when_feature_disabled() {
        let telegram = crate::channel_capability::CHANNEL_TELEGRAM;
        assert!(!compiled_enabled_channel_ids().contains(&telegram));
        assert!(!channel_is_compiled(telegram));
    }

    #[cfg(feature = "feishu")]
    #[test]
    fn feishu_appears_when_feature_enabled() {
        let feishu = crate::channel_capability::CHANNEL_FEISHU;
        assert!(compiled_enabled_channel_ids().contains(&feishu));
        assert_eq!(channel_display_label(feishu), Some("FS"));
    }

    #[cfg(not(feature = "feishu"))]
    #[test]
    fn feishu_disappears_when_feature_disabled() {
        let feishu = crate::channel_capability::CHANNEL_FEISHU;
        assert!(!compiled_enabled_channel_ids().contains(&feishu));
        assert!(!channel_is_compiled(feishu));
    }

    #[cfg(feature = "wecom")]
    #[test]
    fn wecom_appears_when_feature_enabled() {
        let wecom = crate::channel_capability::CHANNEL_WECOM;
        assert!(compiled_enabled_channel_ids().contains(&wecom));
        assert_eq!(channel_display_label(wecom), Some("WC"));
    }

    #[cfg(not(feature = "wecom"))]
    #[test]
    fn wecom_disappears_when_feature_disabled() {
        let wecom = crate::channel_capability::CHANNEL_WECOM;
        assert!(!compiled_enabled_channel_ids().contains(&wecom));
        assert!(!channel_is_compiled(wecom));
    }

    #[cfg(feature = "dingtalk")]
    #[test]
    fn dingtalk_appears_when_feature_enabled() {
        let dingtalk = crate::channel_capability::CHANNEL_DINGTALK;
        assert!(compiled_enabled_channel_ids().contains(&dingtalk));
        assert_eq!(channel_display_label(dingtalk), Some("DT"));
    }

    #[cfg(not(feature = "dingtalk"))]
    #[test]
    fn dingtalk_disappears_when_feature_disabled() {
        let dingtalk = crate::channel_capability::CHANNEL_DINGTALK;
        assert!(!compiled_enabled_channel_ids().contains(&dingtalk));
        assert!(!channel_is_compiled(dingtalk));
    }

    #[cfg(feature = "qq_channel")]
    #[test]
    fn qq_channel_appears_when_feature_enabled() {
        let qq_channel = crate::channel_capability::CHANNEL_QQ_CHANNEL;
        assert!(compiled_enabled_channel_ids().contains(&qq_channel));
        assert_eq!(channel_display_label(qq_channel), Some("QQ"));
    }

    #[cfg(not(feature = "qq_channel"))]
    #[test]
    fn qq_channel_disappears_when_feature_disabled() {
        let qq_channel = crate::channel_capability::CHANNEL_QQ_CHANNEL;
        assert!(!compiled_enabled_channel_ids().contains(&qq_channel));
        assert!(!channel_is_compiled(qq_channel));
    }

    #[cfg(feature = "websocket")]
    #[test]
    fn websocket_appears_when_feature_enabled() {
        let websocket = crate::channel_capability::CHANNEL_WEBSOCKET;
        assert!(channel_is_compiled(websocket));
        assert_eq!(channel_display_label(websocket), Some("WS"));
    }

    #[cfg(not(feature = "websocket"))]
    #[test]
    fn websocket_disappears_when_feature_disabled() {
        let websocket = crate::channel_capability::CHANNEL_WEBSOCKET;
        assert!(!channel_is_compiled(websocket));
    }
}
