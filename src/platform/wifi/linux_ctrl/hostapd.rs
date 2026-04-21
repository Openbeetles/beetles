//! Start/stop hostapd and dnsmasq for AP mode.

use crate::error::Result;
use crate::platform::linux_owner::{self, LinuxSocketProtocol};
use crate::platform::state_mount_path;
use crate::platform::wifi::linux_ctrl::hostapd_ctrl;
use crate::platform::wifi::linux_ctrl::net;
use crate::platform::wifi::linux_ctrl::process::{
    self, is_pid_alive, run_checked, signal_pid as signal_process, write_secure_atomic,
};
use crate::platform::wifi::linux_ctrl::HOSTAPD_CTRL_INTERFACE_DIR;
use std::collections::HashSet;
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

fn hostapd_socket_path(iface: &str) -> PathBuf {
    Path::new(HOSTAPD_CTRL_INTERFACE_DIR).join(iface)
}

fn process_summary(identity: &process::ProcessIdentity) -> String {
    format!(
        "pid={} comm={} cmdline={}",
        identity.pid,
        identity.comm,
        identity.cmdline()
    )
}

fn hostapd_process_matches_owned(identity: &process::ProcessIdentity, conf_path: &Path) -> bool {
    identity.comm.trim() == "hostapd"
        && identity
            .args
            .iter()
            .any(|arg| arg == conf_path.to_string_lossy().as_ref())
}

/// Parse dnsmasq argv and confirm it points at beetle-owned config or pidfile.
#[cfg(test)]
fn dnsmasq_cmdline_matches_owned_paths(
    cmdline: &[u8],
    conf_path: &Path,
    pidfile_path: &Path,
) -> bool {
    let expected_conf = conf_path.to_string_lossy();
    let expected_pidfile = pidfile_path.to_string_lossy();
    let args = cmdline
        .split(|byte| *byte == 0)
        .filter(|segment| !segment.is_empty())
        .map(String::from_utf8_lossy)
        .collect::<Vec<_>>();
    for (index, arg) in args.iter().enumerate() {
        if arg.as_ref() == format!("--conf-file={}", expected_conf) {
            return true;
        }
        if arg.as_ref() == format!("--pid-file={}", expected_pidfile) {
            return true;
        }
        if arg.as_ref() == "--conf-file"
            && args
                .get(index + 1)
                .is_some_and(|next| next.as_ref() == expected_conf.as_ref())
        {
            return true;
        }
        if arg.as_ref() == "--pid-file"
            && args
                .get(index + 1)
                .is_some_and(|next| next.as_ref() == expected_pidfile.as_ref())
        {
            return true;
        }
    }
    false
}

fn dnsmasq_process_matches_owned(
    identity: &process::ProcessIdentity,
    conf_path: &Path,
    pidfile_path: &Path,
) -> bool {
    if identity.comm.trim() != "dnsmasq" {
        return false;
    }
    let expected_conf = conf_path.to_string_lossy();
    let expected_pidfile = pidfile_path.to_string_lossy();
    for (index, arg) in identity.args.iter().enumerate() {
        if arg.as_str() == format!("--conf-file={}", expected_conf) {
            return true;
        }
        if arg.as_str() == format!("--pid-file={}", expected_pidfile) {
            return true;
        }
        if arg.as_str() == "--conf-file"
            && identity
                .args
                .get(index + 1)
                .is_some_and(|next| next.as_str() == expected_conf.as_ref())
        {
            return true;
        }
        if arg.as_str() == "--pid-file"
            && identity
                .args
                .get(index + 1)
                .is_some_and(|next| next.as_str() == expected_pidfile.as_ref())
        {
            return true;
        }
    }
    false
}

fn pidfile_identity(path: &Path) -> Option<process::ProcessIdentity> {
    let pid = std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()?;
    process::read_process_identity(pid)
}

fn pidfile_matches_owned_hostapd(
    pid_path: &Path,
    conf_path: &Path,
) -> Option<process::ProcessIdentity> {
    let identity = pidfile_identity(pid_path)?;
    hostapd_process_matches_owned(&identity, conf_path).then_some(identity)
}

fn owned_hostapd_owners(iface: &str, conf_path: &Path) -> Vec<process::ProcessIdentity> {
    process::unix_socket_owners(&hostapd_socket_path(iface))
        .into_iter()
        .filter(|identity| hostapd_process_matches_owned(identity, conf_path))
        .collect()
}

fn summarize_owners(owners: &[process::ProcessIdentity]) -> String {
    if owners.is_empty() {
        return "none".to_string();
    }
    owners
        .iter()
        .map(process_summary)
        .collect::<Vec<_>>()
        .join(", ")
}

fn summarize_string_owners(hostapd_owners: &[String], dnsmasq_owners: &[String]) -> String {
    let mut parts = Vec::new();
    if !hostapd_owners.is_empty() {
        parts.push(format!("hostapd=[{}]", hostapd_owners.join(", ")));
    }
    if !dnsmasq_owners.is_empty() {
        parts.push(format!("dnsmasq=[{}]", dnsmasq_owners.join(", ")));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join("; ")
    }
}

fn partition_dnsmasq_owners(
    owners: &[linux_owner::LinuxSocketOwner],
    conf_path: &Path,
    pidfile_path: &Path,
) -> (Vec<process::ProcessIdentity>, Vec<String>) {
    let mut owned = Vec::new();
    let mut external = Vec::new();
    for owner in owners {
        match process::read_process_identity(owner.pid) {
            Some(proc) if dnsmasq_process_matches_owned(&proc, conf_path, pidfile_path) => {
                owned.push(proc);
            }
            Some(_) | None => external.push(owner.summary()),
        }
    }
    (owned, external)
}

fn own_pidfile_residue(
    iface: &str,
    hostapd_conf: &Path,
    dnsmasq_conf: &Path,
    hostapd_pid: &Path,
    dnsmasq_pid: &Path,
) -> Vec<String> {
    let mut residue = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(hostapd_pid) {
        let pid = raw.trim().parse::<u32>().ok();
        match pid.and_then(process::read_process_identity) {
            Some(identity) if hostapd_process_matches_owned(&identity, hostapd_conf) => {
                residue.push(format!("hostapd pidfile -> {}", process_summary(&identity)));
            }
            Some(identity) => {
                residue.push(format!(
                    "hostapd pidfile foreign -> {}",
                    process_summary(&identity)
                ));
            }
            None => {
                residue.push(format!(
                    "hostapd pidfile stale -> {}",
                    hostapd_pid.display()
                ));
            }
        }
    }
    if let Ok(raw) = std::fs::read_to_string(dnsmasq_pid) {
        let pid = raw.trim().parse::<u32>().ok();
        match pid.and_then(process::read_process_identity) {
            Some(identity)
                if dnsmasq_process_matches_owned(&identity, dnsmasq_conf, dnsmasq_pid) =>
            {
                residue.push(format!("dnsmasq pidfile -> {}", process_summary(&identity)));
            }
            Some(identity) => {
                residue.push(format!(
                    "dnsmasq pidfile foreign -> {}",
                    process_summary(&identity)
                ));
            }
            None => {
                residue.push(format!(
                    "dnsmasq pidfile stale -> {}",
                    dnsmasq_pid.display()
                ));
            }
        }
    }
    if !residue.is_empty() {
        residue.push(format!("iface={}", iface));
    }
    residue
}

fn other_beetle_runtime_summaries() -> Result<Vec<String>> {
    Ok(linux_owner::other_beetle_run_processes()
        .map_err(|e| crate::error::Error::io("wifi_ap_owner", e))?
        .into_iter()
        .map(|identity| identity.summary())
        .collect())
}

/// 启动前 owner preflight：
/// - 外部 hostapd/dnsmasq 占用时直接报 owner，避免误杀系统服务。
/// - Beetle 自己的残留/重复启动尝试会先清理，再继续启动链。
pub fn preflight_ap_start(iface: &str) -> Result<()> {
    let hostapd_conf_file = hostapd_conf_path();
    let dnsmasq_conf_file = dnsmasq_conf_path();
    let hostapd_pid_file = pidfile("hostapd");
    let dnsmasq_pid_file = pidfile("dnsmasq");

    let hostapd_owners = process::unix_socket_owners(&hostapd_socket_path(iface));
    let dnsmasq_owners = linux_owner::resolve_listener_owners("67", LinuxSocketProtocol::Udp)
        .map_err(|e| crate::error::Error::io("wifi_ap_owner", e))?;
    let owned_hostapd = hostapd_owners
        .iter()
        .filter(|identity| hostapd_process_matches_owned(identity, &hostapd_conf_file))
        .cloned()
        .collect::<Vec<_>>();
    let (owned_dnsmasq, external_dnsmasq) =
        partition_dnsmasq_owners(&dnsmasq_owners, &dnsmasq_conf_file, &dnsmasq_pid_file);
    let external_hostapd = hostapd_owners
        .iter()
        .filter(|identity| !hostapd_process_matches_owned(identity, &hostapd_conf_file))
        .map(process_summary)
        .collect::<Vec<_>>();

    let owned_pidfile_residue = own_pidfile_residue(
        iface,
        &hostapd_conf_file,
        &dnsmasq_conf_file,
        &hostapd_pid_file,
        &dnsmasq_pid_file,
    );

    if !external_hostapd.is_empty() || !external_dnsmasq.is_empty() {
        return Err(crate::error::Error::config(
            "wifi_ap_owner",
            format!(
                "external AP/DHCP owner on iface '{}': {}",
                iface,
                summarize_string_owners(&external_hostapd, &external_dnsmasq)
            ),
        ));
    }

    if !owned_hostapd.is_empty() || !owned_dnsmasq.is_empty() || !owned_pidfile_residue.is_empty() {
        let other_beetle_runs = other_beetle_runtime_summaries()?;
        if !other_beetle_runs.is_empty() {
            return Err(crate::error::Error::config(
                "wifi_ap_owner",
                format!(
                    "another `beetle run` instance is already active; refusing to reclaim Beetle-owned AP/DHCP on '{}': runtimes=[{}], hostapd=[{}], dnsmasq=[{}], pidfiles=[{}]",
                    iface,
                    other_beetle_runs.join(", "),
                    summarize_owners(&owned_hostapd),
                    summarize_owners(&owned_dnsmasq),
                    owned_pidfile_residue.join(", ")
                ),
            ));
        }
        log::warn!(
            "[hostapd] Beetle-owned AP/DHCP residue on '{}'; cleaning before start: hostapd=[{}], dnsmasq=[{}], pidfiles=[{}]",
            iface,
            summarize_owners(&owned_hostapd),
            summarize_owners(&owned_dnsmasq),
            owned_pidfile_residue.join(", ")
        );
        stop_ap(iface);

        let remaining_owned_hostapd = owned_hostapd_owners(iface, &hostapd_conf_file);
        let remaining_dnsmasq_owners =
            linux_owner::resolve_listener_owners("67", LinuxSocketProtocol::Udp)
                .map_err(|e| crate::error::Error::io("wifi_ap_owner", e))?;
        let (remaining_owned_dnsmasq, remaining_external_dnsmasq) = partition_dnsmasq_owners(
            &remaining_dnsmasq_owners,
            &dnsmasq_conf_file,
            &dnsmasq_pid_file,
        );
        if !remaining_owned_hostapd.is_empty() || !remaining_owned_dnsmasq.is_empty() {
            return Err(crate::error::Error::config(
                "wifi_ap_owner",
                format!(
                    "Beetle-owned AP/DHCP residue still present after cleanup on '{}': hostapd=[{}], dnsmasq=[{}]",
                    iface,
                    summarize_owners(&remaining_owned_hostapd),
                    summarize_owners(&remaining_owned_dnsmasq)
                ),
            ));
        }
        if !remaining_external_dnsmasq.is_empty() {
            return Err(crate::error::Error::config(
                "wifi_ap_owner",
                format!(
                    "external DHCP owner still present after Beetle cleanup on '{}': dnsmasq=[{}]",
                    iface,
                    remaining_external_dnsmasq.join(", ")
                ),
            ));
        }
    }
    Ok(())
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
    preflight_ap_start(iface)?;
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
/// 3. Scan /proc for any remaining beetle-owned dnsmasq not covered by PID files
///    (e.g. prior crash without cleanup) and kill them so port 67 is free.
pub fn stop_ap(iface: &str) {
    let dnsmasq_conf = dnsmasq_conf_path();
    let dnsmasq_pid_path = pidfile("dnsmasq");
    let hostapd_conf = hostapd_conf_path();
    let hostapd_pid_path = pidfile("hostapd");
    let hostapd_socket = hostapd_socket_path(iface);
    let mut killed_pids = HashSet::new();

    let hostapd_socket_owners = process::unix_socket_owners(&hostapd_socket);
    let owned_hostapd_socket = hostapd_socket_owners
        .iter()
        .filter(|identity| hostapd_process_matches_owned(identity, &hostapd_conf))
        .cloned()
        .collect::<Vec<_>>();
    let owned_hostapd_pidfile = pidfile_matches_owned_hostapd(&hostapd_pid_path, &hostapd_conf);
    if !owned_hostapd_socket.is_empty() || owned_hostapd_pidfile.is_some() {
        hostapd_ctrl::try_terminate(iface, Duration::from_secs(3));
    } else if !hostapd_socket_owners.is_empty() {
        log::warn!(
            "[hostapd] skip terminate on '{}' because ctrl socket is owned by external process(es): {}",
            iface,
            summarize_owners(&hostapd_socket_owners)
        );
    }

    // Kill PID-file-tracked processes; wait for each to release its sockets.
    for name in ["dnsmasq", "hostapd"] {
        let pid_path = pidfile(name);
        if let Ok(raw) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = raw.trim().parse::<u32>() {
                if pid > 0 {
                    let expected_owned = if name == "dnsmasq" {
                        process::read_process_identity(pid)
                            .map(|identity| {
                                dnsmasq_process_matches_owned(
                                    &identity,
                                    &dnsmasq_conf,
                                    &dnsmasq_pid_path,
                                )
                            })
                            .unwrap_or(false)
                    } else {
                        process::read_process_identity(pid)
                            .map(|identity| hostapd_process_matches_owned(&identity, &hostapd_conf))
                            .unwrap_or(false)
                    };
                    if expected_owned {
                        killed_pids.insert(pid);
                        kill_and_wait(pid);
                    } else {
                        log::warn!(
                            "[hostapd] refuse to kill non-Beetle pidfile owner '{}' -> pid={} (stale pidfile removed)",
                            name,
                            pid
                        );
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    // Kill any remaining Beetle-owned hostapd residue not covered by the PID file.
    for identity in owned_hostapd_owners(iface, &hostapd_conf) {
        if killed_pids.contains(&identity.pid) {
            continue;
        }
        log::debug!(
            "[hostapd] killing owned untracked hostapd process {}",
            process_summary(&identity)
        );
        kill_and_wait(identity.pid);
    }

    // Kill any remaining beetle-owned dnsmasq not tracked by our PID file.
    for owner in linux_owner::resolve_listener_owners("67", LinuxSocketProtocol::Udp)
        .map(|owners| {
            owners
                .into_iter()
                .filter(|identity| {
                    process::read_process_identity(identity.pid).is_some_and(|proc| {
                        dnsmasq_process_matches_owned(&proc, &dnsmasq_conf, &dnsmasq_pid_path)
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    {
        if killed_pids.contains(&owner.pid) {
            continue;
        }
        log::debug!(
            "[hostapd] killing owned untracked dnsmasq process {}",
            owner.summary()
        );
        kill_and_wait(owner.pid);
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn owned_dnsmasq_match_rejects_unrelated_instances() {
        let conf = Path::new("/tmp/beetle/wifi/linux/dnsmasq.conf");
        let pidfile = Path::new("/tmp/beetle/wifi/linux/dnsmasq.pid");

        assert!(!super::dnsmasq_cmdline_matches_owned_paths(
            b"dnsmasq\0--conf-file=/etc/dnsmasq.conf\0--pid-file=/run/dnsmasq.pid\0",
            conf,
            pidfile,
        ));
        assert!(!super::dnsmasq_cmdline_matches_owned_paths(
            b"dnsmasq\0--dhcp-range=192.168.1.2,192.168.1.10\0",
            conf,
            pidfile,
        ));
    }

    #[test]
    fn owned_dnsmasq_match_accepts_owned_conf_or_pidfile() {
        let conf = Path::new("/tmp/beetle/wifi/linux/dnsmasq.conf");
        let pidfile = Path::new("/tmp/beetle/wifi/linux/dnsmasq.pid");

        assert!(super::dnsmasq_cmdline_matches_owned_paths(
            b"dnsmasq\0--conf-file=/tmp/beetle/wifi/linux/dnsmasq.conf\0",
            conf,
            pidfile,
        ));
        assert!(super::dnsmasq_cmdline_matches_owned_paths(
            b"dnsmasq\0--pid-file\0/tmp/beetle/wifi/linux/dnsmasq.pid\0",
            conf,
            pidfile,
        ));
    }

    #[test]
    fn ownership_helpers_recognize_owned_process_shapes() {
        let hostapd_conf = Path::new("/tmp/beetle/wifi/linux/hostapd.conf");
        let dnsmasq_conf = Path::new("/tmp/beetle/wifi/linux/dnsmasq.conf");
        let dnsmasq_pidfile = Path::new("/tmp/beetle/wifi/linux/dnsmasq.pid");
        let hostapd = crate::platform::wifi::linux_ctrl::process::ProcessIdentity {
            pid: 101,
            comm: "hostapd".to_string(),
            args: vec![
                "-B".to_string(),
                "-P".to_string(),
                "/tmp/beetle/wifi/linux/hostapd.pid".to_string(),
                "/tmp/beetle/wifi/linux/hostapd.conf".to_string(),
            ],
        };
        let dnsmasq = crate::platform::wifi::linux_ctrl::process::ProcessIdentity {
            pid: 102,
            comm: "dnsmasq".to_string(),
            args: vec![
                "dnsmasq".to_string(),
                "--conf-file=/tmp/beetle/wifi/linux/dnsmasq.conf".to_string(),
                "--pid-file=/tmp/beetle/wifi/linux/dnsmasq.pid".to_string(),
            ],
        };
        assert!(super::hostapd_process_matches_owned(&hostapd, hostapd_conf));
        assert!(super::dnsmasq_process_matches_owned(
            &dnsmasq,
            dnsmasq_conf,
            dnsmasq_pidfile
        ));
    }
}
