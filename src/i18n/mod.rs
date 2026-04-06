//! 固件用户可见文案国际化（仅 `zh` / `en`）。
//!
//! **单一出口**：业务代码通过 [`Locale`] + [`catalog::Message`] + [`tr`] / [`tr_error`] 生成展示字符串，禁止将任意 `Error::to_string()` 或散落字面量直接作为 API/通道用户文案。
//!
//! **刻意不国际化**：送入 LLM 的 system / 路由 / ReAct 续写脚手架（如「上一轮产出…」）保持单一中文，避免双份 prompt 与模型行为漂移；用户最终看到的自然语言主要由模型输出决定。
//!
//! **与日志分离**：`Error` 的 `Display` 可继续偏技术英文供日志；HTTP 响应体仅使用本模块。

mod catalog;
mod error;

pub use catalog::{tr, Message, SensorWatchThresholdKind};
pub use error::tr_error;

use crate::platform::ConfigStore;

/// UI / API 语言；与 NVS `locale` 一致，仅 `zh` 与 `en`。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Locale {
    #[default]
    Zh,
    En,
}

impl Locale {
    /// 与 [`crate::config::get_locale`] 语义一致：`en` 为英文，其余为中文。
    pub fn from_storage(s: &str) -> Self {
        if s == "en" {
            Locale::En
        } else {
            Locale::Zh
        }
    }
}

/// 从配置存储读取当前语言。
pub fn locale_from_store(store: &dyn ConfigStore) -> Locale {
    Locale::from_storage(&crate::config::get_locale(store))
}

#[cfg(test)]
mod tests {
    use super::catalog::{Message, SensorWatchThresholdKind};
    use super::{tr, Locale};

    fn assert_nonempty_both(msg: Message) {
        let zh = tr(msg.clone(), Locale::Zh);
        let en = tr(msg, Locale::En);
        assert!(!zh.is_empty(), "zh empty");
        assert!(!en.is_empty(), "en empty");
    }

    #[test]
    fn core_api_messages_non_empty() {
        assert_nonempty_both(Message::PairingRequired);
        assert_nonempty_both(Message::OperationFailed);
        assert_nonempty_both(Message::InvalidJson);
        assert_nonempty_both(Message::OtaDownload);
        assert_nonempty_both(Message::ErrorProxyUnsupported);
    }

    #[test]
    fn cron_heartbeat_llm_messages_distinct_locales() {
        let cron = Message::CronTaskFired {
            id: "t1".to_string(),
            action: "ping".to_string(),
        };
        let zh_c = tr(cron.clone(), Locale::Zh);
        let en_c = tr(cron, Locale::En);
        assert_ne!(zh_c, en_c);

        assert_nonempty_both(Message::HeartbeatPendingTasksReminder);
        assert_nonempty_both(Message::LlmNotConfigured);

        let alert = Message::SensorWatchAlert {
            id: "s1".to_string(),
            label: "temp".to_string(),
            value: 1.0,
            threshold: 0.5,
            threshold_kind: SensorWatchThresholdKind::Above,
        };
        assert_nonempty_both(alert);
    }
}
