//! 系统消息/后台作业分类。
//! Classification for system messages and background maintenance jobs.

use crate::bus::IngressKind;

pub const CHANNEL_HEARTBEAT: &str = "heartbeat";
pub const CHANNEL_CRON: &str = "cron";
pub const CHANNEL_LONG_TERM_MEMORY_REFRESH: &str = "_memory_refresh";
pub const CHANNEL_OPERATOR_MAINTENANCE: &str = "_operator_maintenance";
pub const CHANNEL_POST_REPLY_MAINTENANCE: &str = "_post_reply_maintenance";
pub const CHANNEL_SELF_RUNTIME: &str = "_self_runtime";

pub fn post_reply_quiet_window_remaining_ms(
    now_secs: u64,
    last_active_epoch_secs: u64,
) -> Option<u64> {
    if last_active_epoch_secs == 0 {
        return None;
    }
    let elapsed_secs = now_secs.saturating_sub(last_active_epoch_secs);
    if elapsed_secs >= crate::constants::POST_REPLY_BACKGROUND_QUIET_WINDOW_SECS {
        None
    } else {
        Some(
            crate::constants::POST_REPLY_BACKGROUND_QUIET_WINDOW_SECS
                .saturating_sub(elapsed_secs)
                .saturating_mul(1000),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemWorkClass {
    UserInteractive,
    SystemInteractive,
    Maintenance,
    BackgroundLowPriority,
}

impl SystemWorkClass {
    pub fn is_background_job(self) -> bool {
        matches!(self, Self::Maintenance | Self::BackgroundLowPriority)
    }
}

pub fn classify_system_work(channel: &str, ingress: IngressKind) -> SystemWorkClass {
    if ingress != IngressKind::System {
        return SystemWorkClass::UserInteractive;
    }
    match channel {
        CHANNEL_CRON | CHANNEL_HEARTBEAT | CHANNEL_LONG_TERM_MEMORY_REFRESH => {
            SystemWorkClass::BackgroundLowPriority
        }
        CHANNEL_POST_REPLY_MAINTENANCE | CHANNEL_SELF_RUNTIME | CHANNEL_OPERATOR_MAINTENANCE => {
            SystemWorkClass::Maintenance
        }
        _ => SystemWorkClass::SystemInteractive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_user_message_as_interactive() {
        assert_eq!(
            classify_system_work("qq_channel", IngressKind::User),
            SystemWorkClass::UserInteractive
        );
    }

    #[test]
    fn classify_background_channels() {
        assert_eq!(
            classify_system_work(CHANNEL_CRON, IngressKind::System),
            SystemWorkClass::BackgroundLowPriority
        );
        assert_eq!(
            classify_system_work(CHANNEL_HEARTBEAT, IngressKind::System),
            SystemWorkClass::BackgroundLowPriority
        );
        assert_eq!(
            classify_system_work(CHANNEL_LONG_TERM_MEMORY_REFRESH, IngressKind::System),
            SystemWorkClass::BackgroundLowPriority
        );
    }

    #[test]
    fn classify_maintenance_channels() {
        assert_eq!(
            classify_system_work(CHANNEL_SELF_RUNTIME, IngressKind::System),
            SystemWorkClass::Maintenance
        );
        assert_eq!(
            classify_system_work(CHANNEL_POST_REPLY_MAINTENANCE, IngressKind::System),
            SystemWorkClass::Maintenance
        );
        assert_eq!(
            classify_system_work(CHANNEL_OPERATOR_MAINTENANCE, IngressKind::System),
            SystemWorkClass::Maintenance
        );
    }

    #[test]
    fn post_reply_quiet_window_requires_recent_silence() {
        assert_eq!(
            post_reply_quiet_window_remaining_ms(1_000, 995),
            Some(
                crate::constants::POST_REPLY_BACKGROUND_QUIET_WINDOW_SECS.saturating_sub(5) * 1000
            )
        );
        assert_eq!(post_reply_quiet_window_remaining_ms(1_000, 0), None);
        assert_eq!(
            post_reply_quiet_window_remaining_ms(
                1_000,
                1_000u64.saturating_sub(crate::constants::POST_REPLY_BACKGROUND_QUIET_WINDOW_SECS)
            ),
            None
        );
    }
}
