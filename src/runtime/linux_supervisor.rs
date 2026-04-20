//! Linux supervisor status contract for the single-process `beetle run` runtime.
//!
//! Process-level restart, stop, and rollback are now owned by systemd (or the
//! packaging init wrapper). This module only keeps the shared status snapshot
//! types and the read path used by presence / operator status / acceptance
//! inspection. There is no in-process supervisor loop anymore.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const REL_PATH_LINUX_SUPERVISOR_STATE: &str = "runtime/linux_supervisor/status.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxSupervisorAgentState {
    pub pid: Option<u32>,
    pub state: String,
    pub last_started_at: Option<u64>,
    pub last_exited_at: Option<u64>,
    pub last_exit_code: Option<i32>,
    pub last_exit_signal: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_exit_reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxSupervisorState {
    pub supervisor_pid: u32,
    pub current_state: String,
    pub started_at: u64,
    pub last_start_at: u64,
    pub restart_count: u32,
    pub failure_burst_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_burst_started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_mode_entered_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_mode_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub agent: LinuxSupervisorAgentState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxSupervisorStatusSnapshot {
    pub supervisor_alive: bool,
    pub agent_alive: bool,
    pub state: LinuxSupervisorState,
}

/// Read a legacy-format supervisor status snapshot if one is present on disk.
///
/// Under the single-process `beetle run` runtime this normally returns `None`:
/// there is no in-process supervisor writing the state file. The reader is kept
/// so that presence / operator_status / acceptance inspections can still
/// surface a snapshot when running on a host that retained a status file from
/// an earlier dual-process deployment.
pub fn read_status_snapshot() -> Result<Option<LinuxSupervisorStatusSnapshot>> {
    let Some(bytes) = read_optional_file(supervisor_state_path())? else {
        return Ok(None);
    };
    let state = serde_json::from_slice::<LinuxSupervisorState>(&bytes)
        .map_err(|e| Error::config("linux_supervisor_state", e.to_string()))?;
    let supervisor_alive = is_pid_alive(state.supervisor_pid);
    let agent_alive = state.agent.pid.map(is_pid_alive).unwrap_or(false);
    Ok(Some(LinuxSupervisorStatusSnapshot {
        supervisor_alive,
        agent_alive,
        state,
    }))
}

fn read_optional_file(path: PathBuf) -> Result<Option<Vec<u8>>> {
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io("linux_supervisor_state", error)),
    }
}

fn linux_supervisor_state_root() -> PathBuf {
    if let Ok(path) = std::env::var("BEETLE_STATE_ROOT") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    for candidate in ["/var/lib/beetle", "/data/beetle"] {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return path;
        }
    }

    PathBuf::from("/var/lib/beetle")
}

fn supervisor_state_path() -> PathBuf {
    linux_supervisor_state_root().join(REL_PATH_LINUX_SUPERVISOR_STATE)
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    let error = std::io::Error::last_os_error();
    error.raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::linux_supervisor_state_root;
    use std::path::PathBuf;

    #[test]
    fn linux_supervisor_state_root_defaults_without_platform_init() {
        let previous = std::env::var_os("BEETLE_STATE_ROOT");
        unsafe {
            std::env::remove_var("BEETLE_STATE_ROOT");
        }
        let root = linux_supervisor_state_root();
        match previous {
            Some(value) => unsafe { std::env::set_var("BEETLE_STATE_ROOT", value) },
            None => unsafe { std::env::remove_var("BEETLE_STATE_ROOT") },
        }
        assert_ne!(root, PathBuf::from("/tmp/beetle"));
        assert!(
            root.as_path() == std::path::Path::new("/var/lib/beetle")
                || root.as_path() == std::path::Path::new("/data/beetle"),
            "unexpected state root: {}",
            root.display()
        );
    }
}
