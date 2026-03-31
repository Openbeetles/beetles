//! Single controlled command entry for Linux WiFi operations.

use crate::error::{Error, Result};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

use std::ffi::OsString;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

/// Minimal command output used by WiFi controllers.
#[derive(Debug, Clone)]
pub struct CmdOutput {
    // stdout 仅在守护进程启动失败时用于诊断日志，正常路径不读取
    #[allow(dead_code)]
    pub stdout: String,
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
