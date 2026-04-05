//! Start/stop hostapd and dnsmasq for AP mode.

use crate::error::Result;
use crate::platform::state_mount_path;
use crate::platform::wifi::linux_ctrl::HOSTAPD_CTRL_INTERFACE_DIR;
use crate::platform::wifi::linux_ctrl::hostapd_ctrl;
use crate::platform::wifi::linux_ctrl::net;
use crate::platform::wifi::linux_ctrl::process::{
    is_pid_alive, run_checked, signal_pid as signal_process, write_secure_atomic,
};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const CMD_TIMEOUT: Duration = Duration::from_secs(10);
/// `hostapd -B` 返回后控制套接字可能尚未创建；在启动 dnsmasq 前必须确认 AP 守护进程可响应。
const HOSTAPD_CTRL_READY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SOFTAP_CHANNEL: u8 = 1;

/// 发 TERM，等待进程退出（最多 2s），超时后发 KILL。
fn kill_and_wait(pid: u32) {
    let _ = signal_process(pid, true); // SIGTERM
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if !is_pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = signal_process(pid, false); // SIGKILL
    // 给内核最多 500ms 回收 socket
    std::thread::sleep(Duration::from_millis(500));
}

/// 通过 /proc/*/comm 扫描所有名为 `comm` 的进程 PID。
/// 用于清理无 PID 文件的残留进程（如系统 dnsmasq 或崩溃遗留）。
fn find_pids_by_comm(comm: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let comm_path = entry.path().join("comm");
        if let Ok(c) = std::fs::read_to_string(comm_path) {
            if c.trim() == comm {
                pids.push(pid);
            }
        }
    }
    pids
}

fn config_dir() -> PathBuf {
    state_mount_path().join("wifi/linux")
}

fn hostapd_conf_path() -> PathBuf {
    config_dir().join("hostapd.conf")
}

fn dnsmasq_conf_path() -> PathBuf {
    config_dir().join("dnsmasq.conf")
}

fn pidfile(name: &str) -> PathBuf {
    config_dir().join(format!("{}.pid", name))
}

/// 与 [`start_ap`] 写入位置一致的 PID 路径，供守护线程检查。
pub fn daemon_pid_path(name: &str) -> PathBuf {
    pidfile(name)
}

/// 轮询 hostapd 控制口直至 `PING` 返回 `PONG` 或超时，避免「进程已 fork 但 AP 未就绪」时立刻启动 dnsmasq / 返回给调用方。
fn wait_hostapd_ctrl_ready(iface: &str) -> Result<()> {
    let deadline = Instant::now() + HOSTAPD_CTRL_READY_TIMEOUT;
    loop {
        match hostapd_ctrl::request(
            iface,
            "PING",
            Duration::from_millis(400),
            "wifi_hostapd_ready",
        ) {
            Ok(r) if r.contains("PONG") => return Ok(()),
            Ok(_) | Err(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(crate::error::Error::config(
                "wifi_hostapd_ready",
                format!(
                    "hostapd ctrl iface not ready on '{}' within {:?}",
                    iface, HOSTAPD_CTRL_READY_TIMEOUT
                ),
            ));
        }
        thread::sleep(Duration::from_millis(100));
    }
}

/// Start AP with an explicit channel. Invalid channel falls back to default channel 1.
pub fn start_ap_on_channel(iface: &str, ssid: &str, ip: &str, channel: u8) -> Result<()> {
    let safe_channel = if (1..=13).contains(&channel) {
        channel
    } else {
        log::warn!(
            "[hostapd] invalid channel {}, fallback to {}",
            channel,
            DEFAULT_SOFTAP_CHANNEL
        );
        DEFAULT_SOFTAP_CHANNEL
    };
    net::wait_iface_kernel_ready(iface, "wifi_ap_iface_ready")?;
    net::setup_ap_address(iface, &format!("{}/24", ip))?;

    let hostapd_conf_body = format!(
        "interface={iface}\ndriver=nl80211\nssid={ssid}\nhw_mode=g\nchannel={safe_channel}\nauth_algs=1\nwpa=0\nctrl_interface={HOSTAPD_CTRL_INTERFACE_DIR}\n",
    );
    let hostapd_conf_file = hostapd_conf_path();
    write_secure_atomic(
        &hostapd_conf_file,
        hostapd_conf_body.as_bytes(),
        "wifi_ap_config",
    )?;

    let dnsmasq_conf_body = format!(
        "interface={iface}\nbind-interfaces\ndhcp-range={net_start},{net_end},255.255.255.0,12h\n",
        net_start = ap_pool_start(ip),
        net_end = ap_pool_end(ip),
    );
    let dnsmasq_conf_file = dnsmasq_conf_path();
    write_secure_atomic(
        &dnsmasq_conf_file,
        dnsmasq_conf_body.as_bytes(),
        "wifi_ap_config",
    )?;

    let hostapd_pid = pidfile("hostapd");
    let dnsmasq_pid = pidfile("dnsmasq");
    let _ = std::fs::remove_file(&hostapd_pid);
    let _ = std::fs::remove_file(&dnsmasq_pid);

    // Use owned `String` argv fragments (not `path().to_string_lossy().as_ref()` on temporaries):
    // dnsmasq 2.90 is strict about argv; unstable pointers produced "junk found in command line".
    let hostapd_pid_s = hostapd_pid.to_string_lossy().into_owned();
    let hostapd_conf_s = hostapd_conf_file.to_string_lossy().into_owned();
    run_checked(
        "hostapd",
        &["-B", "-P", hostapd_pid_s.as_str(), hostapd_conf_s.as_str()],
        CMD_TIMEOUT,
        "wifi_hostapd_start",
    )?;
    if let Err(e) = wait_hostapd_ctrl_ready(iface) {
        log::warn!(
            "[hostapd] control interface not ready on '{}', tearing down partial AP stack: {}",
            iface,
            e
        );
        stop_ap(iface);
        return Err(e);
    }
    net::wait_iface_kernel_ready(iface, "wifi_dnsmasq_iface_ready")?;
    // Single-token `--opt=path` avoids any ambiguity with multi-arg parsing on embedded dnsmasq.
    let dnsmasq_cf = format!("--conf-file={}", dnsmasq_conf_file.display());
    let dnsmasq_pf = format!("--pid-file={}", dnsmasq_pid.display());
    if let Err(e) = run_checked(
        "dnsmasq",
        &[dnsmasq_cf.as_str(), dnsmasq_pf.as_str()],
        CMD_TIMEOUT,
        "wifi_dnsmasq_start",
    ) {
        // Without DHCP, clients often associate (L2) but never get a routable IPv4 — provisioning URL appears "down".
        stop_ap(iface);
        return Err(e);
    }
    Ok(())
}

/// 停止 AP 相关进程并清理 PID；`iface` 用于尝试删除 hostapd 控制 socket。
///
/// Stop strategy:
/// 1. Graceful terminate via hostapd ctrl socket.
/// 2. TERM → wait-for-exit (≤2 s) → KILL for each PID-file-tracked process.
/// 3. Scan /proc for any remaining dnsmasq not covered by PID files
///    (system dnsmasq, prior crash without cleanup) and kill them so port 67 is free.
pub fn stop_ap(iface: &str) {
    hostapd_ctrl::try_terminate(iface, Duration::from_secs(3));

    // Kill PID-file-tracked processes; wait for each to release its sockets.
    for name in ["dnsmasq", "hostapd"] {
        let pid_path = pidfile(name);
        if let Ok(raw) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                if pid > 0 {
                    kill_and_wait(pid);
                }
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    // Kill any remaining dnsmasq not tracked by our PID file (e.g. system service,
    // or previous instance that crashed before writing the file).
    // Port 67/udp must be free before we start our own dnsmasq.
    for pid in find_pids_by_comm("dnsmasq") {
        log::debug!("[hostapd] killing untracked dnsmasq pid={}", pid);
        kill_and_wait(pid);
    }

    let sock = Path::new(HOSTAPD_CTRL_INTERFACE_DIR).join(iface);
    let _ = std::fs::remove_file(sock);
}

fn ap_pool_start(ip: &str) -> String {
    let mut parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 {
        parts[3] = "20";
        return parts.join(".");
    }
    "192.168.1.20".to_string()
}

fn ap_pool_end(ip: &str) -> String {
    let mut parts: Vec<&str> = ip.split('.').collect();
    if parts.len() == 4 {
        parts[3] = "180";
        return parts.join(".");
    }
    "192.168.1.180".to_string()
}
