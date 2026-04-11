#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinuxWifiStartup {
    Inherit { ip: String, scan_via_iw: bool },
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EffectiveWifiRuntimeState {
    pub connected: bool,
    pub ip: Option<String>,
    pub scan_via_iw: bool,
}

pub(super) fn effective_wifi_runtime_state(
    associated: bool,
    sta_ip: Option<&str>,
    default_route_iface: Option<&str>,
    wifi_iface: &str,
) -> EffectiveWifiRuntimeState {
    if effective_wifi_ready(associated, sta_ip, default_route_iface, wifi_iface) {
        return EffectiveWifiRuntimeState {
            connected: true,
            ip: sta_ip.map(ToString::to_string),
            scan_via_iw: true,
        };
    }
    EffectiveWifiRuntimeState {
        connected: false,
        ip: None,
        scan_via_iw: true,
    }
}

pub(super) fn effective_wifi_ready(
    associated: bool,
    sta_ip: Option<&str>,
    default_route_iface: Option<&str>,
    wifi_iface: &str,
) -> bool {
    associated && sta_ip.is_some_and(usable_wifi_ipv4) && default_route_iface == Some(wifi_iface)
}

fn usable_wifi_ipv4(ip: &str) -> bool {
    let Ok(ipv4) = ip.parse::<Ipv4Addr>() else {
        return false;
    };
    !ipv4.is_unspecified() && !ipv4.is_loopback() && !ipv4.is_link_local()
}

#[cfg(test)]
mod tests {
    use super::{effective_wifi_ready, effective_wifi_runtime_state, EffectiveWifiRuntimeState};

    #[test]
    fn existing_wifi_is_effective_only_when_assoc_ip_and_route_all_match() {
        assert!(effective_wifi_ready(
            true,
            Some("192.168.1.20"),
            Some("wlan0"),
            "wlan0",
        ));
        assert!(!effective_wifi_ready(
            false,
            Some("192.168.1.20"),
            Some("wlan0"),
            "wlan0",
        ));
        assert!(!effective_wifi_ready(
            true,
            Some("169.254.10.2"),
            Some("wlan0"),
            "wlan0",
        ));
        assert!(!effective_wifi_ready(
            true,
            Some("127.0.0.1"),
            Some("wlan0"),
            "wlan0",
        ));
        assert!(!effective_wifi_ready(
            true,
            Some("192.168.1.20"),
            Some("eth0"),
            "wlan0",
        ));
        assert!(!effective_wifi_ready(
            true,
            Some("not-an-ip"),
            Some("wlan0"),
            "wlan0",
        ));
    }

    #[test]
    fn runtime_state_carries_ip_only_for_effective_wifi() {
        let inherited =
            effective_wifi_runtime_state(true, Some("192.168.1.20"), Some("wlan0"), "wlan0");
        assert_eq!(
            inherited,
            EffectiveWifiRuntimeState {
                connected: true,
                ip: Some("192.168.1.20".to_string()),
                scan_via_iw: true,
            }
        );

        let fallback =
            effective_wifi_runtime_state(true, Some("192.168.1.20"), Some("eth0"), "wlan0");
        assert_eq!(
            fallback,
            EffectiveWifiRuntimeState {
                connected: false,
                ip: None,
                scan_via_iw: true,
            }
        );
    }
}
