//! Linux service helpers that unify systemd and init-script managed beetle installs.

use crate::error::{Error, Result};
use std::path::Path;
use std::process::{Command, ExitStatus};

pub const BEETLE_INIT_SCRIPT_PATH: &str = "/etc/init.d/beetle";
const EXPECTED_INIT_DAEMON_LINE: &str = "DAEMON=\"/opt/beetle/current/beetle\"";
const EXPECTED_INIT_START_FRAGMENT: &str = "start-stop-daemon -S";
const EXPECTED_INIT_RUN_FRAGMENT: &str = "-- run";

/// Run a beetle service action through the installed Linux service manager.
///
/// This keeps the public CLI surface stable (`beetle restart`, `beetle stop`)
/// while allowing systemd and SysV/init.d hosts to share one command contract.
pub fn run_beetle_service_action(action: &str) -> Result<Option<ExitStatus>> {
    if let Some(status) = crate::runtime::linux_systemd::run_beetle_systemd_action(action)? {
        return Ok(Some(status));
    }
    if !Path::new(BEETLE_INIT_SCRIPT_PATH).is_file() {
        return Ok(None);
    }
    let status = Command::new(BEETLE_INIT_SCRIPT_PATH)
        .arg(action)
        .status()
        .map_err(|error| Error::io("linux_service_action", error))?;
    Ok(Some(status))
}

/// Inspect whether the installed beetle init script points at the managed `/opt/beetle/current`
/// entrypoint and launches the unified `run` command.
pub fn inspect_beetle_init_script_consistency() -> Option<bool> {
    let content = std::fs::read_to_string(Path::new(BEETLE_INIT_SCRIPT_PATH)).ok()?;
    Some(init_script_has_expected_entrypoint(&content))
}

fn init_script_has_expected_entrypoint(content: &str) -> bool {
    let has_expected_daemon = content
        .lines()
        .any(|line| line.trim() == EXPECTED_INIT_DAEMON_LINE);
    let has_expected_start = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.contains(EXPECTED_INIT_START_FRAGMENT)
            && trimmed.contains(EXPECTED_INIT_RUN_FRAGMENT)
            && !trimmed.contains("-- supervise")
    });
    has_expected_daemon && has_expected_start
}

#[cfg(test)]
mod tests {
    use super::init_script_has_expected_entrypoint;

    #[test]
    fn init_script_consistency_accepts_run_entrypoint() {
        let script = r#"
DAEMON="/opt/beetle/current/beetle"
start-stop-daemon -S -b -m -p "$PIDFILE" -x "$DAEMON" -- run || exit 1
"#;
        assert!(init_script_has_expected_entrypoint(script));
    }

    #[test]
    fn init_script_consistency_rejects_legacy_supervise_entrypoint() {
        let script = r#"
DAEMON="/opt/beetle/current/beetle"
start-stop-daemon -S -b -m -p "$PIDFILE" -x "$DAEMON" -- supervise || exit 1
"#;
        assert!(
            !init_script_has_expected_entrypoint(script),
            "legacy supervise entrypoint must no longer validate"
        );
    }

    #[test]
    fn init_script_consistency_requires_managed_current_symlink_path() {
        let script = r#"
DAEMON="/opt/beetle/releases/r2/beetle"
start-stop-daemon -S -b -m -p "$PIDFILE" -x "$DAEMON" -- run || exit 1
"#;
        assert!(!init_script_has_expected_entrypoint(script));
    }
}
