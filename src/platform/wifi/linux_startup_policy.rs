#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::net::Ipv4Addr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LinuxWifiStartup {
    Inherit { ip: String, scan_via_iw: bool },
    Fallback,
}

pub(super) fn classify_linux_wifi_startup(
    associated: bool,
    sta_ip: Option<&str>,
    default_route_iface: Option<&str>,
    wifi_iface: &str,
) -> LinuxWifiStartup {
    if effective_wifi_ready(associated, sta_ip, default_route_iface, wifi_iface) {
        return LinuxWifiStartup::Inherit {
            ip: sta_ip.unwrap_or_default().to_string(),
            scan_via_iw: true,
        };
    }
    LinuxWifiStartup::Fallback
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
    use super::{classify_linux_wifi_startup, effective_wifi_ready, LinuxWifiStartup};

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
    fn startup_inherits_only_when_existing_wifi_is_effective() {
        let inherited =
            classify_linux_wifi_startup(true, Some("192.168.1.20"), Some("wlan0"), "wlan0");
        assert!(matches!(
            inherited,
            LinuxWifiStartup::Inherit {
                ref ip,
                scan_via_iw: true,
            } if ip == "192.168.1.20"
        ));

        let fallback =
            classify_linux_wifi_startup(true, Some("192.168.1.20"), Some("eth0"), "wlan0");
        assert!(matches!(fallback, LinuxWifiStartup::Fallback));
    }
}
