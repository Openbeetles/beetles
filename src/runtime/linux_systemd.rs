//! Linux systemd helpers for beetle service detection and control.

use crate::error::{Error, Result};
use std::path::Path;
use std::process::{Command, ExitStatus};

const BEETLE_SYSTEMD_UNIT: &str = "beetle.service";
const EXPECTED_EXEC_START: &str = "ExecStart=/opt/beetle/current/beetle supervise";

/// Run `systemctl <action> beetle` when beetle is managed by systemd.
/// Returns `Ok(None)` when systemd does not know the beetle unit in the current environment.
pub fn run_beetle_systemd_action(action: &str) -> Result<Option<ExitStatus>> {
    if !beetle_systemd_unit_managed()? {
        return Ok(None);
    }
    let status = Command::new("systemctl")
        .args([action, "beetle"])
        .status()
        .map_err(|error| Error::io("linux_systemd_action", error))?;
    Ok(Some(status))
}

/// Inspect whether the loaded beetle systemd unit points at the current release entry.
/// Returns `None` when the environment is not systemd-managed or the fragment path is unavailable.
pub fn inspect_beetle_systemd_unit_consistency() -> Option<bool> {
    let fragment_path = query_systemctl_show_value("FragmentPath").ok().flatten()?;
    let content = std::fs::read_to_string(Path::new(&fragment_path)).ok()?;
    Some(unit_content_has_expected_exec_start(&content))
}

fn beetle_systemd_unit_managed() -> Result<bool> {
    Ok(query_systemctl_show_value("LoadState")?
        .as_deref()
        .is_some_and(load_state_indicates_managed_service))
}

fn query_systemctl_show_value(property: &str) -> Result<Option<String>> {
    let output = Command::new("systemctl")
        .args(["show", "--property", property, "--value", BEETLE_SYSTEMD_UNIT])
        .output()
        .map_err(|error| Error::io("linux_systemd_show", error))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(normalized_show_value(&String::from_utf8_lossy(&output.stdout)).map(str::to_string))
}

fn normalized_show_value(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn load_state_indicates_managed_service(load_state: &str) -> bool {
    !matches!(load_state.trim(), "" | "not-found")
}

fn unit_content_has_expected_exec_start(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim() == EXPECTED_EXEC_START)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_state_loaded_counts_as_managed_service() {
        assert!(load_state_indicates_managed_service("loaded"));
        assert!(load_state_indicates_managed_service("masked"));
    }

    #[test]
    fn load_state_not_found_does_not_count_as_managed_service() {
        assert!(!load_state_indicates_managed_service("not-found"));
        assert!(!load_state_indicates_managed_service(""));
    }

    #[test]
    fn normalized_show_value_trims_whitespace() {
        assert_eq!(
            normalized_show_value("  /lib/systemd/system/beetle.service \n"),
            Some("/lib/systemd/system/beetle.service")
        );
        assert_eq!(normalized_show_value(" \n "), None);
    }

    #[test]
    fn unit_consistency_requires_expected_exec_start() {
        assert!(unit_content_has_expected_exec_start(
            "[Service]\nExecStart=/opt/beetle/current/beetle supervise\n"
        ));
        assert!(!unit_content_has_expected_exec_start(
            "[Service]\nExecStart=/opt/beetle/releases/r2/beetle supervise\n"
        ));
    }
}
