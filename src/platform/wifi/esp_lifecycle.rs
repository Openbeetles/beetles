//! ESP WiFi lifecycle policy.
//! ESP WiFi 生命周期策略：配置/恢复入口不能伪装成常驻运行态。

/// ESP WiFi 启动模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EspWifiStartupMode {
    /// 纯 SoftAP，用于尚未配置 STA 的配网页。
    SoftApOnly,
    /// 纯 STA，用于已配置 WiFi 后的正常启动稳态。
    StaOnly,
}

/// 根据是否存在 STA 配置决定 WiFi 启动模式。
pub(crate) fn startup_mode_for(has_sta_credentials: bool) -> EspWifiStartupMode {
    if has_sta_credentials {
        EspWifiStartupMode::StaOnly
    } else {
        EspWifiStartupMode::SoftApOnly
    }
}

/// STA 已拿到 IP 后，恢复 SoftAP 是否继续保留。
pub(crate) fn keep_recovery_softap_after_sta_ip() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{startup_mode_for, EspWifiStartupMode};

    #[test]
    fn configured_sta_boots_without_recovery_softap_as_steady_state() {
        assert_eq!(startup_mode_for(true), EspWifiStartupMode::StaOnly);
    }

    #[test]
    fn recovery_softap_is_not_retained_after_sta_ip() {
        assert!(!super::keep_recovery_softap_after_sta_ip());
    }
}
