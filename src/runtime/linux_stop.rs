//! Linux stop helpers that honor the user-facing `beetle stop` contract.

use crate::error::{Error, Result};
use crate::platform::linux_owner::LinuxProcessIdentity;
use std::thread;
use std::time::{Duration, Instant};

const STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_WAIT_POLL: Duration = Duration::from_millis(100);

/// Outcome of a Linux stop request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxStopOutcome {
    /// The stop request was handed to the platform service manager.
    ManagedService,
    /// The stop request was delivered directly to one or more `beetle run` processes.
    DirectProcess { targets: Vec<String> },
}

/// Request that the active Beetle Linux runtime stop.
///
/// The user-facing contract is "stop Beetle", not "only stop a service". We therefore prefer the
/// managed service path when present, but still fall back to the live `beetle run` process when
/// the host is unmanaged.
pub fn request_linux_stop() -> Result<LinuxStopOutcome> {
    let mut managed_service_failure = None;
    match crate::runtime::linux_service::run_beetle_service_action("stop")? {
        Some(status) if status.success() => return Ok(LinuxStopOutcome::ManagedService),
        Some(status) => {
            managed_service_failure = Some(format!(
                "managed beetle stop failed with exit status: {status}"
            ));
        }
        None => {}
    }

    let processes = crate::platform::linux_owner::other_beetle_run_processes()
        .map_err(|error| Error::io("linux_stop_scan", error))?;
    if processes.is_empty() {
        if let Some(message) = managed_service_failure {
            return Err(Error::config("linux_stop", message));
        }
    }
    let targets = request_process_stop_with(
        processes,
        STOP_WAIT_TIMEOUT,
        signal_pid_sigterm,
        pid_is_alive,
        || {
            thread::sleep(STOP_WAIT_POLL);
        },
    )?;
    Ok(LinuxStopOutcome::DirectProcess { targets })
}

fn request_process_stop_with<SignalFn, AliveFn, SleepFn>(
    processes: Vec<LinuxProcessIdentity>,
    timeout: Duration,
    mut signal_pid: SignalFn,
    mut pid_is_alive: AliveFn,
    mut sleep: SleepFn,
) -> Result<Vec<String>>
where
    SignalFn: FnMut(u32) -> Result<()>,
    AliveFn: FnMut(u32) -> bool,
    SleepFn: FnMut(),
{
    if processes.is_empty() {
        return Err(Error::config(
            "linux_stop",
            "no active `beetle run` instance is currently running",
        ));
    }

    let targets = processes
        .iter()
        .map(LinuxProcessIdentity::summary)
        .collect::<Vec<_>>();
    for process in &processes {
        signal_pid(process.pid)?;
    }

    let deadline = Instant::now() + timeout;
    loop {
        let mut alive = Vec::new();
        for process in &processes {
            if pid_is_alive(process.pid) {
                alive.push(process.summary());
            }
        }
        if alive.is_empty() {
            return Ok(targets);
        }
        if Instant::now() >= deadline {
            return Err(Error::config(
                "linux_stop",
                format!(
                    "stop signal sent but `beetle run` is still alive: {}",
                    alive.join(", ")
                ),
            ));
        }
        sleep();
    }
}

fn signal_pid_sigterm(pid: u32) -> Result<()> {
    let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if rc < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(Error::io("linux_stop_signal", error));
    }
    Ok(())
}

fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::{request_process_stop_with, LinuxProcessIdentity};
    use crate::error::Error;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Duration;

    fn process(pid: u32) -> LinuxProcessIdentity {
        LinuxProcessIdentity {
            pid,
            command: "beetle".to_string(),
            exe: Some(PathBuf::from("/opt/beetle/current/beetle")),
            args: vec!["/opt/beetle/current/beetle".to_string(), "run".to_string()],
        }
    }

    #[test]
    fn direct_stop_requires_active_run_process() {
        let err =
            request_process_stop_with(Vec::new(), Duration::ZERO, |_| Ok(()), |_| false, || {})
                .expect_err("empty process list must fail");
        assert_eq!(err.stage(), "linux_stop");
        assert!(matches!(err, Error::Config { .. }));
    }

    #[test]
    fn direct_stop_signals_all_targets_and_returns_summaries() {
        let signaled = Rc::new(RefCell::new(Vec::new()));
        let signaled_for_closure = Rc::clone(&signaled);
        let targets = request_process_stop_with(
            vec![process(11), process(29)],
            Duration::ZERO,
            move |pid| {
                signaled_for_closure.borrow_mut().push(pid);
                Ok(())
            },
            |_| false,
            || {},
        )
        .expect("signal path should succeed");
        assert_eq!(*signaled.borrow(), vec![11, 29]);
        assert_eq!(targets.len(), 2);
        assert!(targets[0].contains("pid=11"));
        assert!(targets[1].contains("pid=29"));
    }

    #[test]
    fn direct_stop_errors_when_process_survives_timeout() {
        let err = request_process_stop_with(
            vec![process(41)],
            Duration::ZERO,
            |_| Ok(()),
            |_| true,
            || {},
        )
        .expect_err("live process should time out");
        assert_eq!(err.stage(), "linux_stop");
        assert!(err.to_string().contains("pid=41"));
    }
}
