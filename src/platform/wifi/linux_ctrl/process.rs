//! Single controlled command entry for Linux WiFi operations.

use crate::error::{Error, Result};
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
};

/// Minimal command output used by WiFi controllers.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    // stdout 仅在守护进程启动失败时用于诊断日志，正常路径不读取
    #[allow(dead_code)]
    pub stdout: String,
}

/// Linux `/proc` 进程快照，用于 owner/preflight 收口。
#[derive(Debug, Clone)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub comm: String,
    pub args: Vec<String>,
}

impl ProcessIdentity {
    /// 便于日志打印的命令行。
    pub fn cmdline(&self) -> String {
        if self.args.is_empty() {
            return self.comm.clone();
        }
        self.args.join(" ")
    }
}

fn is_allowed_bin(bin: &str) -> bool {
    // "kill" and "iw" are intentionally absent: we use libc::kill(2) and GENL nl80211 directly.
    matches!(bin, "wpa_supplicant" | "hostapd" | "dnsmasq" | "udhcpc")
}

/// Prefer tools shipped under `/opt/beetle/bin` (e.g. deploy script + bundled static
/// `hostapd`/`dnsmasq` on distros without opkg). Fall back to `PATH`.
fn resolve_tool_executable(bin: &'static str) -> OsString {
    let bundled = Path::new("/opt/beetle/bin").join(bin);
    if bundled.is_file() {
        return bundled.into_os_string();
    }
    OsString::from(bin)
}

/// 读取 PID 文件（十进制）；无效或缺失返回 `None`。
pub fn read_pid_file(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// 读取进程的 `comm` 与命令行参数；进程消失或不可读时返回 `None`。
pub fn read_process_identity(pid: u32) -> Option<ProcessIdentity> {
    if pid == 0 {
        return None;
    }
    let proc_dir = Path::new("/proc").join(pid.to_string());
    let comm = std::fs::read_to_string(proc_dir.join("comm")).ok()?;
    let args = read_process_cmdline_args(proc_dir.join("cmdline").as_path())?;
    Some(ProcessIdentity {
        pid,
        comm: comm.trim().to_string(),
        args,
    })
}

fn read_process_cmdline_args(path: &Path) -> Option<Vec<String>> {
    let raw = std::fs::read(path).ok()?;
    let args = raw
        .split(|byte| *byte == 0)
        .filter(|segment| !segment.is_empty())
        .map(|segment| String::from_utf8_lossy(segment).into_owned())
        .collect::<Vec<_>>();
    Some(args)
}

/// 发送信号给进程：`sigterm=true` → SIGTERM，`false` → SIGKILL。
/// ESRCH（进程已不存在）视为成功；其它 errno 作为 IO 错误返回。
pub fn signal_pid(pid: u32, sigterm: bool) -> Result<()> {
    if pid == 0 {
        return Ok(());
    }
    let sig = if sigterm {
        libc::SIGTERM
    } else {
        libc::SIGKILL
    };
    let rc = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if rc < 0 {
        let e = std::io::Error::last_os_error();
        // ESRCH = process already gone — treat as success for cleanup paths
        if e.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(Error::io("wifi_signal_pid", e));
    }
    Ok(())
}

/// `kill(pid, 0)` 探测进程是否存在（不发送实际信号）。
pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    let e = std::io::Error::last_os_error();
    // EPERM = process exists but we lack permission to signal it → still alive
    e.raw_os_error() == Some(libc::EPERM)
}

fn socket_inode_from_link(target: &Path) -> Option<u64> {
    let target = target.to_string_lossy();
    let inode = target
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse::<u64>()
        .ok()?;
    Some(inode)
}

fn pids_for_socket_inodes(inodes: &HashSet<u64>) -> HashSet<u32> {
    let mut pids = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = std::fs::read_dir(fd_dir) else {
            continue;
        };
        let mut matched = false;
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            if let Some(inode) = socket_inode_from_link(target.as_path()) {
                if inodes.contains(&inode) {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            pids.insert(pid);
        }
    }
    pids
}

fn identities_for_socket_inodes(inodes: HashSet<u64>) -> Vec<ProcessIdentity> {
    let mut out = Vec::new();
    for pid in pids_for_socket_inodes(&inodes) {
        if let Some(identity) = read_process_identity(pid) {
            out.push(identity);
        }
    }
    out.sort_by_key(|identity| identity.pid);
    out
}

fn unix_socket_inodes(socket_path: &Path) -> HashSet<u64> {
    let mut inodes = HashSet::new();
    let Ok(contents) = std::fs::read_to_string("/proc/net/unix") else {
        return inodes;
    };
    let target = socket_path.to_string_lossy();
    for line in contents.lines().skip(1) {
        let cols = line.split_whitespace().collect::<Vec<_>>();
        if cols.len() < 8 {
            continue;
        }
        let Some(path) = cols.get(7) else {
            continue;
        };
        if *path != target.as_ref() {
            continue;
        }
        if let Ok(inode) = cols[6].parse::<u64>() {
            inodes.insert(inode);
        }
    }
    inodes
}

/// 读取占用指定 unix socket 的进程 owner 列表。
pub fn unix_socket_owners(socket_path: &Path) -> Vec<ProcessIdentity> {
    identities_for_socket_inodes(unix_socket_inodes(socket_path))
}

pub fn run_checked(
    bin: &'static str,
    args: &[&str],
    timeout: Duration,
    stage: &'static str,
) -> Result<CmdOutput> {
    if !is_allowed_bin(bin) {
        return Err(Error::config(
            stage,
            format!("command not allowed: {}", bin),
        ));
    }
    for a in args {
        if a.contains('\0') || a.contains('\n') || a.contains('\r') {
            return Err(Error::config(stage, "invalid command argument"));
        }
    }

    let exe = resolve_tool_executable(bin);
    let mut child = Command::new(&exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::io(stage, e))?;

    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| Error::io(stage, e))? {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut out) = child.stdout.take() {
                let _ = out.read_to_string(&mut stdout);
            }
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut stderr);
            }
            if status.success() {
                return Ok(CmdOutput { stdout });
            }
            return Err(Error::config(
                stage,
                format!("{} failed: {}", exe.to_string_lossy(), stderr.trim()),
            ));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::config(
                stage,
                format!("{} timeout", exe.to_string_lossy()),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn write_secure_atomic(path: &Path, data: &[u8], stage: &'static str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::config(stage, "missing parent"))?;
    std::fs::create_dir_all(parent).map_err(|e| Error::io(stage, e))?;
    let fname = path
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or_else(|| Error::config(stage, "invalid file name"))?;
    let tmp = parent.join(format!(".{}.tmp.{}", fname, std::process::id()));
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| Error::io(stage, e))?;
    f.write_all(data).map_err(|e| Error::io(stage, e))?;
    f.sync_all().map_err(|e| Error::io(stage, e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::io(stage, e)
    })?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| Error::io(stage, e))?;
    Ok(())
}
