//! Linux supervisor loop and status contract for the dual-entry runtime.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::Arc;
use std::time::{Duration, Instant};

const REL_PATH_LINUX_SUPERVISOR_STATE: &str = "runtime/linux_supervisor/status.json";
const REL_PATH_LINUX_SUPERVISOR_LOCK: &str = "runtime/linux_supervisor/supervisor.lock";
const REL_PATH_LINUX_SUPERVISOR_RESTART_REQUEST: &str =
    "runtime/linux_supervisor/control/restart.json";
const REL_PATH_LINUX_SUPERVISOR_STOP_REQUEST: &str = "runtime/linux_supervisor/control/stop.json";
const REL_PATH_LINUX_SUPERVISOR_ROLLBACK_REQUEST: &str =
    "runtime/linux_supervisor/control/rollback.json";
const SUPERVISOR_POLL_INTERVAL_MS: u64 = 200;
const SUPERVISOR_STOP_TIMEOUT_SECS: u64 = 5;
const SUPERVISOR_BACKOFF_MAX_SECS: u64 = 30;
const SUPERVISOR_QUICK_FAILURE_SECS: u64 = 20;
const SUPERVISOR_FAILURE_BURST_WINDOW_SECS: u64 = 120;
const SUPERVISOR_SAFE_MODE_THRESHOLD: u32 = 3;

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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct SupervisorControlRequest {
    requested_at: u64,
    requester_pid: u32,
    reason: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupervisorControlAction {
    Restart,
    Stop,
    Rollback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChildExit {
    code: Option<i32>,
    signal: Option<i32>,
}

#[derive(Debug)]
struct SupervisorLockGuard {
    path: PathBuf,
}

impl Drop for SupervisorLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl LinuxSupervisorState {
    fn new(config_path: Option<String>, now_secs: u64) -> Self {
        Self {
            supervisor_pid: std::process::id(),
            current_state: "starting".to_string(),
            started_at: now_secs,
            last_start_at: now_secs,
            restart_count: 0,
            failure_burst_count: 0,
            failure_burst_started_at: None,
            safe_mode_entered_at: None,
            safe_mode_reason: None,
            config_path,
            last_event: "supervisor_boot".to_string(),
            last_error: None,
            agent: LinuxSupervisorAgentState {
                pid: None,
                state: "not_started".to_string(),
                last_started_at: None,
                last_exited_at: None,
                last_exit_code: None,
                last_exit_signal: None,
                last_exit_reason: String::new(),
            },
        }
    }
}

fn append_supervisor_workflow_audit(
    disposition: crate::runtime::WorkflowDisposition,
    rationale: &str,
    effect: crate::runtime::WorkflowEffect,
    recovery_policy: crate::runtime::WorkflowRecoveryPolicy,
    happened_at: u64,
) {
    crate::runtime::append_workflow_audit(crate::runtime::WorkflowAuditRecord::new(
        crate::runtime::WorkflowKind::RebootRecovery,
        crate::runtime::WorkflowTrigger::ModeTransition,
        disposition,
        effect,
        recovery_policy,
        format!("linux_supervisor:{}", rationale.trim()),
        happened_at,
    ));
}

fn append_supervisor_backoff_workflow_audit(rationale: &str, happened_at: u64, backoff_secs: u64) {
    crate::runtime::append_workflow_audit(
        crate::runtime::WorkflowAuditRecord::new(
            crate::runtime::WorkflowKind::RebootRecovery,
            crate::runtime::WorkflowTrigger::ModeTransition,
            crate::runtime::WorkflowDisposition::DeferUntil,
            crate::runtime::WorkflowEffect::RequestRestart,
            crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
            format!("linux_supervisor:{}", rationale.trim()),
            happened_at,
        )
        .with_next_allowed_at(Some(happened_at.saturating_add(backoff_secs))),
    );
}

pub fn run_supervisor(
    platform: Arc<dyn crate::platform::Platform>,
    config_path: Option<String>,
) -> Result<()> {
    crate::state::set_boot_phase_active(true);
    crate::runtime::set_recovery_safe_mode_active(false);
    let _lock = try_acquire_supervisor_lock()?;

    clear_control_requests()?;

    let now_secs = crate::util::current_unix_secs();
    let mut state = LinuxSupervisorState::new(config_path.clone(), now_secs);
    let release_status = crate::runtime::sync_platform_release_state(platform.as_ref(), now_secs)?;
    if release_status.managed {
        state.last_event = format!("release_synced:{}", release_status.rollout_state_label());
    }
    write_state(&state)?;
    let _control_plane = crate::runtime::linux_control_plane::spawn(Arc::clone(&platform))?;
    crate::state::set_boot_phase_active(false);

    let current_exe = std::env::current_exe().map_err(|e| Error::io("linux_supervisor", e))?;
    let mut child: Option<Child> = None;
    let mut child_started_at: Option<Instant> = None;
    let mut release_validation_recorded = false;

    loop {
        if let Some(action) = consume_control_request()? {
            match action {
                SupervisorControlAction::Restart => {
                    let now_secs = crate::util::current_unix_secs();
                    state.last_event = "restart_requested".to_string();
                    state.last_error = None;
                    clear_failure_burst(&mut state);
                    clear_safe_mode(&mut state);
                    if let Some(mut running_child) = child.take() {
                        let exit = terminate_child(&mut running_child)?;
                        record_child_exit(&mut state, exit, now_secs, "restart_requested");
                    }
                    child_started_at = None;
                    release_validation_recorded = false;
                    state.restart_count = state.restart_count.saturating_add(1);
                    state.current_state = "backoff".to_string();
                    let backoff_secs = restart_backoff_secs(state.restart_count);
                    append_supervisor_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteNow,
                        "restart_requested",
                        crate::runtime::WorkflowEffect::RequestRestart,
                        crate::runtime::WorkflowRecoveryPolicy::ReplayAfterBoot,
                        now_secs,
                    );
                    append_supervisor_backoff_workflow_audit(
                        "restart_backoff",
                        now_secs,
                        backoff_secs,
                    );
                    write_state(&state)?;
                    std::thread::sleep(Duration::from_secs(backoff_secs));
                    continue;
                }
                SupervisorControlAction::Stop => {
                    let now_secs = crate::util::current_unix_secs();
                    state.last_event = "stop_requested".to_string();
                    state.current_state = "stopping".to_string();
                    clear_failure_burst(&mut state);
                    clear_safe_mode(&mut state);
                    if let Some(mut running_child) = child.take() {
                        let exit = terminate_child(&mut running_child)?;
                        record_child_exit(&mut state, exit, now_secs, "stop_requested");
                    }
                    append_supervisor_workflow_audit(
                        crate::runtime::WorkflowDisposition::Cancel,
                        "stop_requested",
                        crate::runtime::WorkflowEffect::Noop,
                        crate::runtime::WorkflowRecoveryPolicy::DropOnModeExit,
                        now_secs,
                    );
                    state.current_state = "stopped".to_string();
                    state.agent.pid = None;
                    state.agent.state = "stopped".to_string();
                    state.last_error = None;
                    write_state(&state)?;
                    return Ok(());
                }
                SupervisorControlAction::Rollback => {
                    if trigger_release_rollback(
                        platform.as_ref(),
                        &mut state,
                        &mut child,
                        &mut child_started_at,
                        "manual_release_rollback",
                    )? {
                        return Err(Error::config(
                            "linux_supervisor",
                            "release rollback applied; restart beetle from /opt/beetle/current",
                        ));
                    }
                    state.last_event = "release_rollback_unavailable".to_string();
                    state.last_error =
                        Some("rollback pointer unavailable for current Linux release".to_string());
                    append_supervisor_workflow_audit(
                        crate::runtime::WorkflowDisposition::ExecuteFailed,
                        "release_rollback_unavailable",
                        crate::runtime::WorkflowEffect::Noop,
                        crate::runtime::WorkflowRecoveryPolicy::OperatorAckRequired,
                        crate::util::current_unix_secs(),
                    );
                    if child.is_some() {
                        state.current_state = "running".to_string();
                    }
                    write_state(&state)?;
                    continue;
                }
            }
        }

        if child.is_none() {
            if safe_mode_active(&state) {
                if state.current_state != "safe_mode" {
                    state.current_state = "safe_mode".to_string();
                    write_state(&state)?;
                }
                std::thread::sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
                continue;
            }
            state.current_state = "starting".to_string();
            state.last_event = "spawning_agent".to_string();
            state.last_error = None;
            write_state(&state)?;

            match spawn_agent_process(&current_exe, config_path.as_deref()) {
                Ok(new_child) => {
                    let now_secs = crate::util::current_unix_secs();
                    state.current_state = "running".to_string();
                    state.last_start_at = now_secs;
                    state.last_event = "agent_started".to_string();
                    state.last_error = None;
                    state.agent.pid = Some(new_child.id());
                    state.agent.state = "running".to_string();
                    state.agent.last_started_at = Some(now_secs);
                    write_state(&state)?;
                    child_started_at = Some(Instant::now());
                    release_validation_recorded = false;
                    child = Some(new_child);
                }
                Err(error) => {
                    state.last_event = "agent_spawn_failed".to_string();
                    state.last_error = Some(error.to_string());
                    state.agent.pid = None;
                    state.agent.state = "spawn_failed".to_string();
                    child_started_at = None;
                    if record_quick_failure(
                        &mut state,
                        crate::util::current_unix_secs(),
                        format!("agent_spawn_failed: {}", error).as_str(),
                    ) {
                        state.current_state = "safe_mode".to_string();
                        state.last_event = "entered_safe_mode".to_string();
                        append_supervisor_workflow_audit(
                            crate::runtime::WorkflowDisposition::ExecuteFailed,
                            "entered_safe_mode",
                            crate::runtime::WorkflowEffect::Noop,
                            crate::runtime::WorkflowRecoveryPolicy::OperatorAckRequired,
                            crate::util::current_unix_secs(),
                        );
                        write_state(&state)?;
                        continue;
                    }
                    state.restart_count = state.restart_count.saturating_add(1);
                    state.current_state = "backoff".to_string();
                    let backoff_secs = restart_backoff_secs(state.restart_count);
                    append_supervisor_backoff_workflow_audit(
                        "spawn_backoff",
                        crate::util::current_unix_secs(),
                        backoff_secs,
                    );
                    write_state(&state)?;
                    std::thread::sleep(Duration::from_secs(backoff_secs));
                    continue;
                }
            }
        }

        let mut restart_needed = false;
        if let Some(running_child) = child.as_mut() {
            if let Some(exit_status) = running_child
                .try_wait()
                .map_err(|e| Error::io("linux_supervisor_wait", e))?
            {
                let now_secs = crate::util::current_unix_secs();
                let exit = parse_exit_status(exit_status);
                let reason = if exit.code == Some(42) {
                    "agent_requested_restart"
                } else {
                    "agent_exit"
                };
                let quick_failure = child_started_at
                    .take()
                    .map(|started| {
                        started.elapsed() <= Duration::from_secs(SUPERVISOR_QUICK_FAILURE_SECS)
                    })
                    .unwrap_or(true);
                record_child_exit(&mut state, exit, now_secs, reason);
                state.last_error = None;
                child = None;
                if reason == "agent_requested_restart" {
                    clear_failure_burst(&mut state);
                } else if quick_failure {
                    let safe_mode_reason = format!("{} ({})", reason, format_child_exit(exit));
                    if record_quick_failure(&mut state, now_secs, safe_mode_reason.as_str()) {
                        if auto_release_rollback_eligible(platform.as_ref(), now_secs)
                            && trigger_release_rollback(
                                platform.as_ref(),
                                &mut state,
                                &mut child,
                                &mut child_started_at,
                                "automatic_release_rollback_after_quick_failures",
                            )?
                        {
                            return Err(Error::config(
                                "linux_supervisor",
                                "pending Linux release rolled back after quick failures; restart beetle from /opt/beetle/current",
                            ));
                        }
                        state.current_state = "safe_mode".to_string();
                        state.last_event = "entered_safe_mode".to_string();
                        append_supervisor_workflow_audit(
                            crate::runtime::WorkflowDisposition::ExecuteFailed,
                            "entered_safe_mode",
                            crate::runtime::WorkflowEffect::Noop,
                            crate::runtime::WorkflowRecoveryPolicy::OperatorAckRequired,
                            now_secs,
                        );
                        write_state(&state)?;
                        continue;
                    }
                } else {
                    clear_failure_burst(&mut state);
                }
                state.restart_count = state.restart_count.saturating_add(1);
                state.current_state = "backoff".to_string();
                let backoff_secs = restart_backoff_secs(state.restart_count);
                append_supervisor_backoff_workflow_audit(
                    "child_exit_backoff",
                    now_secs,
                    backoff_secs,
                );
                write_state(&state)?;
                restart_needed = true;
            }
        }
        if restart_needed {
            std::thread::sleep(Duration::from_secs(restart_backoff_secs(
                state.restart_count,
            )));
            continue;
        }

        if child.is_some() && !release_validation_recorded {
            if let Some(started_at) = child_started_at.as_ref() {
                if started_at.elapsed() > Duration::from_secs(SUPERVISOR_QUICK_FAILURE_SECS) {
                    let now_secs = crate::util::current_unix_secs();
                    if crate::runtime::mark_current_release_steady(platform.as_ref(), now_secs)? {
                        state.last_event = "release_validation_passed".to_string();
                        state.last_error = None;
                        write_state(&state)?;
                    }
                    release_validation_recorded = true;
                }
            }
        }

        std::thread::sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
    }
}

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

pub fn request_restart() -> Result<bool> {
    request_control_action(SupervisorControlAction::Restart, "cli_restart")
}

pub fn request_stop() -> Result<bool> {
    request_control_action(SupervisorControlAction::Stop, "cli_stop")
}

pub fn request_rollback() -> Result<bool> {
    request_control_action(SupervisorControlAction::Rollback, "cli_release_rollback")
}

fn request_control_action(action: SupervisorControlAction, reason: &str) -> Result<bool> {
    let Some(snapshot) = read_status_snapshot()? else {
        return Ok(false);
    };
    if !snapshot.supervisor_alive {
        return Ok(false);
    }
    let request = SupervisorControlRequest {
        requested_at: crate::util::current_unix_secs(),
        requester_pid: std::process::id(),
        reason: reason.trim().to_string(),
    };
    let payload = serde_json::to_vec_pretty(&request)
        .map_err(|e| Error::config("linux_supervisor_control", e.to_string()))?;
    crate::platform::fs_atomic::atomic_write(control_request_path(action).as_path(), &payload)?;
    Ok(true)
}

fn spawn_agent_process(current_exe: &Path, config_path: Option<&str>) -> Result<Child> {
    let mut command = Command::new(current_exe);
    command.arg("agent");
    if let Some(path) = config_path {
        command.arg("--config").arg(path);
    }
    command
        .spawn()
        .map_err(|e| Error::io("linux_supervisor_spawn", e))
}

fn terminate_child(child: &mut Child) -> Result<ChildExit> {
    signal_pid(child.id(), libc::SIGTERM)?;
    let deadline = Instant::now() + Duration::from_secs(SUPERVISOR_STOP_TIMEOUT_SECS);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| Error::io("linux_supervisor_wait", e))?
        {
            return Ok(parse_exit_status(status));
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
    }
    signal_pid(child.id(), libc::SIGKILL)?;
    let status = child
        .wait()
        .map_err(|e| Error::io("linux_supervisor_wait", e))?;
    Ok(parse_exit_status(status))
}

fn record_child_exit(
    state: &mut LinuxSupervisorState,
    exit: ChildExit,
    now_secs: u64,
    reason: &str,
) {
    state.last_event = reason.trim().to_string();
    state.agent.pid = None;
    state.agent.state = "exited".to_string();
    state.agent.last_exited_at = Some(now_secs);
    state.agent.last_exit_code = exit.code;
    state.agent.last_exit_signal = exit.signal;
    state.agent.last_exit_reason = reason.trim().to_string();
}

fn format_child_exit(exit: ChildExit) -> String {
    match (exit.code, exit.signal) {
        (Some(code), _) => format!("exit_code={}", code),
        (_, Some(signal)) => format!("signal={}", signal),
        _ => "unknown_exit".to_string(),
    }
}

fn safe_mode_active(state: &LinuxSupervisorState) -> bool {
    state.safe_mode_reason.is_some()
}

fn clear_failure_burst(state: &mut LinuxSupervisorState) {
    state.failure_burst_count = 0;
    state.failure_burst_started_at = None;
}

fn clear_safe_mode(state: &mut LinuxSupervisorState) {
    state.safe_mode_entered_at = None;
    state.safe_mode_reason = None;
    crate::runtime::set_recovery_safe_mode_active(false);
}

fn auto_release_rollback_eligible(platform: &dyn crate::platform::Platform, now_secs: u64) -> bool {
    let status = crate::runtime::inspect_platform_linux_release(platform, now_secs);
    status.managed
        && status.rollback_available
        && status.rollout_state == crate::runtime::LinuxReleaseRolloutState::PendingValidation
}

fn trigger_release_rollback(
    platform: &dyn crate::platform::Platform,
    state: &mut LinuxSupervisorState,
    child: &mut Option<Child>,
    child_started_at: &mut Option<Instant>,
    reason: &str,
) -> Result<bool> {
    let now_secs = crate::util::current_unix_secs();
    clear_failure_burst(state);
    clear_safe_mode(state);
    state.last_error = None;
    state.last_event = "release_rollback_requested".to_string();
    state.current_state = "rollback".to_string();
    if let Some(mut running_child) = child.take() {
        let exit = terminate_child(&mut running_child)?;
        record_child_exit(state, exit, now_secs, reason);
    }
    *child_started_at = None;
    if !crate::runtime::rollback_current_release(platform, reason, now_secs)? {
        return Ok(false);
    }
    append_supervisor_workflow_audit(
        crate::runtime::WorkflowDisposition::ExecuteNow,
        reason,
        crate::runtime::WorkflowEffect::RollbackRelease,
        crate::runtime::WorkflowRecoveryPolicy::OperatorAckRequired,
        now_secs,
    );
    state.current_state = "rollback_triggered".to_string();
    state.last_event = "release_rollback_triggered".to_string();
    state.agent.pid = None;
    state.agent.state = "rolled_back".to_string();
    write_state(state)?;
    Ok(true)
}

fn record_quick_failure(state: &mut LinuxSupervisorState, now_secs: u64, reason: &str) -> bool {
    let reset_window = match state.failure_burst_started_at {
        Some(started_at) => {
            now_secs >= started_at
                && now_secs.saturating_sub(started_at) > SUPERVISOR_FAILURE_BURST_WINDOW_SECS
        }
        None => true,
    };
    if reset_window {
        state.failure_burst_started_at = Some(now_secs);
        state.failure_burst_count = 0;
    }
    state.failure_burst_count = state.failure_burst_count.saturating_add(1);
    if state.failure_burst_count >= SUPERVISOR_SAFE_MODE_THRESHOLD {
        state.safe_mode_entered_at = Some(now_secs);
        state.safe_mode_reason = Some(reason.trim().to_string());
        crate::runtime::set_recovery_safe_mode_active(true);
        state.agent.state = "safe_mode".to_string();
        return true;
    }
    false
}

fn parse_exit_status(status: ExitStatus) -> ChildExit {
    ChildExit {
        code: status.code(),
        signal: status.signal(),
    }
}

fn restart_backoff_secs(restart_count: u32) -> u64 {
    match restart_count {
        0 | 1 => 1,
        2 => 2,
        3 => 5,
        4 => 10,
        _ => SUPERVISOR_BACKOFF_MAX_SECS,
    }
}

fn clear_control_requests() -> Result<()> {
    remove_if_exists(control_request_path(SupervisorControlAction::Restart).as_path())?;
    remove_if_exists(control_request_path(SupervisorControlAction::Stop).as_path())?;
    remove_if_exists(control_request_path(SupervisorControlAction::Rollback).as_path())
}

fn consume_control_request() -> Result<Option<SupervisorControlAction>> {
    let restart_path = control_request_path(SupervisorControlAction::Restart);
    if restart_path.is_file() {
        remove_if_exists(restart_path.as_path())?;
        return Ok(Some(SupervisorControlAction::Restart));
    }
    let stop_path = control_request_path(SupervisorControlAction::Stop);
    if stop_path.is_file() {
        remove_if_exists(stop_path.as_path())?;
        return Ok(Some(SupervisorControlAction::Stop));
    }
    let rollback_path = control_request_path(SupervisorControlAction::Rollback);
    if rollback_path.is_file() {
        remove_if_exists(rollback_path.as_path())?;
        return Ok(Some(SupervisorControlAction::Rollback));
    }
    Ok(None)
}

fn write_state(state: &LinuxSupervisorState) -> Result<()> {
    let payload = serde_json::to_vec_pretty(state)
        .map_err(|e| Error::config("linux_supervisor_state", e.to_string()))?;
    crate::platform::fs_atomic::atomic_write(supervisor_state_path().as_path(), &payload)
}

fn try_acquire_supervisor_lock() -> Result<SupervisorLockGuard> {
    try_acquire_supervisor_lock_at(&supervisor_lock_path(), std::process::id(), is_pid_alive)
}

fn try_acquire_supervisor_lock_at(
    path: &Path,
    current_pid: u32,
    is_alive: impl Fn(u32) -> bool,
) -> Result<SupervisorLockGuard> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::io("linux_supervisor_lock", error))?;
    }
    for _ in 0..2 {
        match OpenOptions::new().create_new(true).write(true).open(path) {
            Ok(mut file) => {
                writeln!(file, "{}", current_pid)
                    .map_err(|error| Error::io("linux_supervisor_lock", error))?;
                file.sync_all()
                    .map_err(|error| Error::io("linux_supervisor_lock", error))?;
                return Ok(SupervisorLockGuard {
                    path: path.to_path_buf(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing_pid = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|raw| raw.trim().parse::<u32>().ok());
                if existing_pid.is_some_and(&is_alive) {
                    return Err(Error::config(
                        "linux_supervisor",
                        format!(
                            "another beetle supervisor is already running (pid={})",
                            existing_pid.unwrap_or_default()
                        ),
                    ));
                }
                remove_if_exists(path)?;
            }
            Err(error) => return Err(Error::io("linux_supervisor_lock", error)),
        }
    }
    Err(Error::config(
        "linux_supervisor",
        "failed to acquire supervisor lock after stale-lock cleanup",
    ))
}

fn read_optional_file(path: PathBuf) -> Result<Option<Vec<u8>>> {
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io("linux_supervisor_state", error)),
    }
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io("linux_supervisor_control", error)),
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

fn supervisor_lock_path() -> PathBuf {
    linux_supervisor_state_root().join(REL_PATH_LINUX_SUPERVISOR_LOCK)
}

fn control_request_path(action: SupervisorControlAction) -> PathBuf {
    linux_supervisor_state_root().join(match action {
        SupervisorControlAction::Restart => REL_PATH_LINUX_SUPERVISOR_RESTART_REQUEST,
        SupervisorControlAction::Stop => REL_PATH_LINUX_SUPERVISOR_STOP_REQUEST,
        SupervisorControlAction::Rollback => REL_PATH_LINUX_SUPERVISOR_ROLLBACK_REQUEST,
    })
}

fn signal_pid(pid: u32, signal: libc::c_int) -> Result<()> {
    if pid == 0 {
        return Ok(());
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if rc < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(Error::io("linux_supervisor_signal", error));
    }
    Ok(())
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
    use super::{
        append_supervisor_backoff_workflow_audit, append_supervisor_workflow_audit,
        clear_failure_burst, clear_safe_mode, linux_supervisor_state_root, parse_exit_status,
        record_quick_failure, restart_backoff_secs, safe_mode_active,
        try_acquire_supervisor_lock_at, LinuxSupervisorState, SUPERVISOR_FAILURE_BURST_WINDOW_SECS,
    };
    use crate::runtime::workflow::{reset_workflow_audit_for_tests, workflow_audit_snapshot};
    use crate::runtime::{
        WorkflowDisposition, WorkflowEffect, WorkflowKind, WorkflowRecoveryPolicy, WorkflowTrigger,
    };
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;

    #[test]
    fn restart_backoff_caps_at_thirty_seconds() {
        assert_eq!(restart_backoff_secs(0), 1);
        assert_eq!(restart_backoff_secs(1), 1);
        assert_eq!(restart_backoff_secs(2), 2);
        assert_eq!(restart_backoff_secs(3), 5);
        assert_eq!(restart_backoff_secs(4), 10);
        assert_eq!(restart_backoff_secs(9), 30);
    }

    #[test]
    fn parse_exit_status_keeps_exit_code() {
        let exit = parse_exit_status(std::process::ExitStatus::from_raw(42 << 8));
        assert_eq!(exit.code, Some(42));
        assert_eq!(exit.signal, None);
    }

    #[test]
    fn parse_exit_status_keeps_signal() {
        let exit = parse_exit_status(std::process::ExitStatus::from_raw(libc::SIGTERM));
        assert_eq!(exit.code, None);
        assert_eq!(exit.signal, Some(libc::SIGTERM));
    }

    #[test]
    fn repeated_quick_failures_enter_safe_mode() {
        let mut state = LinuxSupervisorState::new(None, 1_000);
        assert!(!record_quick_failure(&mut state, 1_001, "spawn_failed"));
        assert!(!record_quick_failure(&mut state, 1_010, "spawn_failed"));
        assert!(record_quick_failure(&mut state, 1_020, "spawn_failed"));
        assert!(safe_mode_active(&state));
        assert_eq!(state.failure_burst_count, 3);
        assert_eq!(state.safe_mode_reason.as_deref(), Some("spawn_failed"));
        assert_eq!(state.agent.state, "safe_mode");
    }

    #[test]
    fn failure_burst_resets_after_window_and_manual_clear() {
        let mut state = LinuxSupervisorState::new(None, 1_000);
        assert!(!record_quick_failure(&mut state, 1_001, "exit"));
        assert_eq!(state.failure_burst_count, 1);
        assert!(!record_quick_failure(
            &mut state,
            1_001 + SUPERVISOR_FAILURE_BURST_WINDOW_SECS + 1,
            "exit"
        ));
        assert_eq!(state.failure_burst_count, 1);
        assert!(!safe_mode_active(&state));
        clear_failure_burst(&mut state);
        clear_safe_mode(&mut state);
        assert_eq!(state.failure_burst_count, 0);
        assert!(state.failure_burst_started_at.is_none());
        assert!(state.safe_mode_reason.is_none());
    }

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
            root == PathBuf::from("/var/lib/beetle") || root == PathBuf::from("/data/beetle"),
            "unexpected state root: {}",
            root.display()
        );
    }

    #[test]
    fn supervisor_restart_audit_uses_reboot_recovery_contract() {
        let _guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();

        append_supervisor_workflow_audit(
            WorkflowDisposition::ExecuteNow,
            "restart_requested",
            WorkflowEffect::RequestRestart,
            WorkflowRecoveryPolicy::ReplayAfterBoot,
            1_234,
        );

        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.executed, 1);
        assert_eq!(audit.recent_records.len(), 1);
        assert_eq!(
            audit.recent_records[0].workflow,
            WorkflowKind::RebootRecovery
        );
        assert_eq!(
            audit.recent_records[0].trigger,
            WorkflowTrigger::ModeTransition
        );
        assert_eq!(
            audit.recent_records[0].rationale,
            "linux_supervisor:restart_requested"
        );
        assert_eq!(
            audit.recent_records[0].recovery_policy,
            WorkflowRecoveryPolicy::ReplayAfterBoot
        );
    }

    #[test]
    fn supervisor_backoff_audit_records_next_allowed_at() {
        let _guard = crate::runtime::workflow_audit_test_guard();
        reset_workflow_audit_for_tests();

        append_supervisor_backoff_workflow_audit("child_exit_backoff", 2_000, 7);

        let audit = workflow_audit_snapshot(4);
        assert_eq!(audit.summary.deferred, 1);
        assert_eq!(audit.recent_records.len(), 1);
        assert_eq!(
            audit.recent_records[0].disposition,
            WorkflowDisposition::DeferUntil
        );
        assert_eq!(
            audit.recent_records[0].workflow,
            WorkflowKind::RebootRecovery
        );
        assert_eq!(audit.recent_records[0].next_allowed_at, Some(2_007));
        assert_eq!(
            audit.recent_records[0].rationale,
            "linux_supervisor:child_exit_backoff"
        );
    }

    #[test]
    fn supervisor_lock_is_exclusive_until_released() {
        let path = std::env::temp_dir().join(format!(
            "beetle-supervisor-lock-{}.lock",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let _guard = try_acquire_supervisor_lock_at(&path, 111, |_| false).unwrap();
        let error = try_acquire_supervisor_lock_at(&path, 222, |_| true).unwrap_err();
        assert!(error.to_string().contains("already running"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn supervisor_lock_reclaims_stale_pid_file() {
        let path = std::env::temp_dir().join(format!(
            "beetle-supervisor-stale-lock-{}.lock",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"333\n").unwrap();

        let _guard = try_acquire_supervisor_lock_at(&path, 444, |pid| pid == 444).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.trim(), "444");

        let _ = std::fs::remove_file(path);
    }
}
