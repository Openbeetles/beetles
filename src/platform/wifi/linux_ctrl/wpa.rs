//! STA 连接/扫描：经 [`super::wpa_ctrl`] Unix 套接字控制 `wpa_supplicant`（热路径无 `wpa_cli`）。
//! STA connect/scan via [`super::wpa_ctrl`] Unix socket to `wpa_supplicant` (no `wpa_cli` on hot path).

use crate::constants::WIFI_CONNECT_TIMEOUT_SECS;
use crate::error::{Error, Result};
use crate::platform::state_mount_path;
use crate::platform::wifi::linux_ctrl::net;
use crate::platform::wifi::linux_ctrl::process::{
    is_pid_alive, read_pid_file, run_checked, signal_pid as signal_process, write_secure_atomic,
};
use crate::platform::wifi::linux_ctrl::wpa_ctrl;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const CMD_TIMEOUT: Duration = Duration::from_secs(8);

fn conf_dir() -> PathBuf {
    state_mount_path().join("wifi/linux")
}

fn wpa_conf_path(iface: &str) -> PathBuf {
    conf_dir().join(format!("wpa_supplicant-{}.conf", iface))
}

/// `wpa_supplicant -P` 写入的 PID 路径（与 [`ensure_daemon`] 一致）。
pub fn supplicant_pid_path(iface: &str) -> PathBuf {
    conf_dir().join(format!("wpa_supplicant-{}.pid", iface))
}

fn ctrl_socket_path(iface: &str) -> PathBuf {
    PathBuf::from(super::WPA_CTRL_INTERFACE_DIR).join(iface)
}

fn cleanup_stale_daemon(iface: &str) {
    let pid_path = supplicant_pid_path(iface);
    if let Some(pid) = read_pid_file(pid_path.as_path()) {
        if is_pid_alive(pid) {
            let _ = signal_process(pid, true); // SIGTERM
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                if !is_pid_alive(pid) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if is_pid_alive(pid) {
                let _ = signal_process(pid, false); // SIGKILL
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    let _ = std::fs::remove_file(ctrl_socket_path(iface));
}

pub fn ensure_daemon(iface: &str) -> Result<()> {
    if wpa_ctrl::request(iface, "PING", Duration::from_secs(3), "wifi_wpa_cmd")
        .map(|r| r.contains("PONG"))
        .unwrap_or(false)
    {
        return Ok(());
    }

    // If ctrl ping failed, the PID/socket may be stale from a prior crash or failed boot.
    cleanup_stale_daemon(iface);

    let conf = format!(
        "ctrl_interface={}\nupdate_config=1\n",
        super::WPA_CTRL_INTERFACE_DIR
    );
    let conf_path = wpa_conf_path(iface);
    let pid_path = supplicant_pid_path(iface);
    write_secure_atomic(&conf_path, conf.as_bytes(), "wifi_wpa_config")?;
    let pid_s = pid_path.to_string_lossy().into_owned();
    let conf_s = conf_path.to_string_lossy().into_owned();
    run_checked(
        "wpa_supplicant",
        &[
            "-B",
            "-i",
            iface,
            "-P",
            pid_s.as_str(),
            "-c",
            conf_s.as_str(),
        ],
        Duration::from_secs(10),
        "wifi_wpa_start",
    )?;
    Ok(())
}

fn request_dhcp_lease(iface: &str, timeout: Duration) -> Result<()> {
    if let Some(ip) = net::read_sta_ip(iface)? {
        log::info!("[wifi_wpa] '{}' already has IPv4 {}", iface, ip);
        return Ok(());
    }
    if timeout.is_zero() {
        return Err(Error::config("wifi_dhcp", "no time left for DHCP"));
    }

    // `udhcpc` is the standard DHCP client on embedded Linux / BusyBox systems.
    // `-f`: foreground so we can wait; `-q`: quit after lease; `-n`: fail if no lease.
    run_checked(
        "udhcpc",
        &["-i", iface, "-f", "-q", "-n"],
        timeout,
        "wifi_dhcp",
    )?;

    if net::read_sta_ip(iface)?.is_some() {
        return Ok(());
    }
    Err(Error::config(
        "wifi_dhcp",
        "udhcpc exited but no IPv4 assigned",
    ))
}

pub fn connect_sta(iface: &str, ssid: &str, pass: &str) -> Result<Option<String>> {
    validate_ssid(ssid)?;
    validate_pass(pass)?;
    ensure_daemon(iface)?;

    wpa_ctrl::request(iface, "REMOVE_NETWORK all", CMD_TIMEOUT, "wifi_wpa_cmd")?;
    let add = wpa_ctrl::request(iface, "ADD_NETWORK", CMD_TIMEOUT, "wifi_wpa_cmd")?;
    let net_id = add.trim();
    if net_id.is_empty() {
        return Err(Error::config(
            "wifi_wpa_cmd",
            "add_network returned empty id",
        ));
    }
    wpa_ctrl::request(
        iface,
        &format!("SET_NETWORK {} ssid {}", net_id, quote_wpa(ssid)),
        CMD_TIMEOUT,
        "wifi_wpa_cmd",
    )?;
    if pass.is_empty() {
        wpa_ctrl::request(
            iface,
            &format!("SET_NETWORK {net_id} key_mgmt NONE"),
            CMD_TIMEOUT,
            "wifi_wpa_cmd",
        )?;
    } else {
        wpa_ctrl::request(
            iface,
            &format!("SET_NETWORK {net_id} psk {}", quote_wpa(pass)),
            CMD_TIMEOUT,
            "wifi_wpa_cmd",
        )?;
    }
    wpa_ctrl::request(
        iface,
        &format!("ENABLE_NETWORK {net_id}"),
        CMD_TIMEOUT,
        "wifi_wpa_cmd",
    )?;
    wpa_ctrl::request(iface, "RECONNECT", CMD_TIMEOUT, "wifi_wpa_cmd")?;

    let deadline = std::time::Instant::now() + Duration::from_secs(WIFI_CONNECT_TIMEOUT_SECS);
    loop {
        let status = wpa_ctrl::request(iface, "STATUS", Duration::from_secs(3), "wifi_wpa_cmd")?;
        if status.contains("wpa_state=COMPLETED") {
            if let Some(ip) = net::read_sta_ip(iface)? {
                return Ok(Some(ip));
            }
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            request_dhcp_lease(iface, remain)?;
            return net::read_sta_ip(iface);
        }
        if std::time::Instant::now() >= deadline {
            log::warn!(
                "[wifi_wpa] STA timeout on '{}', last status: {}",
                iface,
                status.replace('\n', " | ")
            );
            return Err(Error::config(
                "wifi_connect",
                format!("STA timeout after {}s", WIFI_CONNECT_TIMEOUT_SECS),
            ));
        }
        // 与 probe 线程的退避 [`WIFI_RETRY_BACKOFF_SECS`] 解耦：此处仅轮询 wpa_state，用较短间隔更快收敛。
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 在 `deadline` 前完成一次扫描；各子步骤超时不超过剩余时间，避免总墙钟超过 `WIFI_SCAN_TIMEOUT_SECS`。
pub fn scan_bounded(iface: &str, deadline: Instant) -> Result<Vec<crate::platform::WifiApEntry>> {
    fn remain(deadline: Instant) -> Duration {
        deadline.saturating_duration_since(Instant::now())
    }
    ensure_daemon(iface)?;
    if remain(deadline).is_zero() {
        return Err(Error::config("wifi_scan", "scan timeout"));
    }
    let t_scan = remain(deadline).min(CMD_TIMEOUT);
    wpa_ctrl::request(iface, "SCAN", t_scan, "wifi_scan")?;
    let sleep_dur = remain(deadline).min(Duration::from_millis(800));
    if !sleep_dur.is_zero() {
        std::thread::sleep(sleep_dur);
    }
    if remain(deadline).is_zero() {
        return Err(Error::config("wifi_scan", "scan timeout"));
    }
    let t_res = remain(deadline).min(Duration::from_secs(5));
    let out = wpa_ctrl::request(iface, "SCAN_RESULTS", t_res, "wifi_scan")?;
    parse_scan_results(&out)
}

fn parse_scan_results(raw: &str) -> Result<Vec<crate::platform::WifiApEntry>> {
    let mut out = Vec::new();
    for line in raw.lines().skip(1) {
        // bssid / freq / signal / flags / ssid
        let mut parts = line.split('\t');
        let _bssid = parts.next();
        let _freq = parts.next();
        let Some(sig) = parts.next() else {
            continue;
        };
        let _flags = parts.next();
        let Some(ssid) = parts.next() else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let Ok(rssi) = sig.parse::<i16>() else {
            continue;
        };
        let rssi_i8 = rssi.clamp(i8::MIN as i16, i8::MAX as i16) as i8;
        out.push(crate::platform::WifiApEntry {
            ssid: ssid.to_string(),
            rssi: rssi_i8,
        });
    }
    out.sort_by(|a, b| b.rssi.cmp(&a.rssi));
    Ok(out)
}

fn validate_ssid(ssid: &str) -> Result<()> {
    if ssid.is_empty() || ssid.len() > 64 || ssid.contains('\n') || ssid.contains('\r') {
        return Err(Error::config("wifi_connect", "invalid wifi_ssid"));
    }
    Ok(())
}

fn validate_pass(pass: &str) -> Result<()> {
    if pass.len() > 64 || pass.contains('\n') || pass.contains('\r') {
        return Err(Error::config("wifi_connect", "invalid wifi_pass"));
    }
    Ok(())
}

fn quote_wpa(v: &str) -> String {
    let escaped = v.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}
