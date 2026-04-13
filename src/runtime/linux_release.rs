//! Linux release / rollout / rollback contract.

use crate::error::{Error, Result};
use crate::platform::{Platform, StateFs};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const REL_PATH_LINUX_RELEASE_STATE: &str = "runtime/linux_release/state.json";
pub const REL_PATH_STATE_SCHEMA_STATUS: &str = "runtime/state_schema.json";
pub const LINUX_RELEASE_STATE_VERSION: u32 = 1;
pub const BEETLE_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LinuxReleaseRolloutState {
    #[default]
    Unmanaged,
    PendingValidation,
    Steady,
    RollbackTriggered,
    RolledBack,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxReleasePointer {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxReleaseState {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<LinuxReleasePointer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<LinuxReleasePointer>,
    #[serde(default)]
    pub rollout_state: LinuxReleaseRolloutState,
    #[serde(default)]
    pub last_updated_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_action: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateSchemaStatus {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgraded_from: Option<u32>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinuxReleaseStatus {
    pub managed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<LinuxReleasePointer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<LinuxReleasePointer>,
    #[serde(default)]
    pub rollout_state: LinuxReleaseRolloutState,
    pub rollback_available: bool,
    pub current_exe: String,
    pub state_schema_version: u32,
    pub state_schema_current: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub systemd_unit_consistent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_script_consistent: Option<bool>,
    #[serde(default)]
    pub last_updated_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_action: String,
}

impl LinuxReleaseRolloutState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unmanaged => "unmanaged",
            Self::PendingValidation => "pending_validation",
            Self::Steady => "steady",
            Self::RollbackTriggered => "rollback_triggered",
            Self::RolledBack => "rolled_back",
        }
    }
}

impl LinuxReleaseStatus {
    pub fn rollout_state_label(&self) -> &'static str {
        self.rollout_state.as_str()
    }
}

pub fn sync_platform_release_state(
    platform: &dyn Platform,
    now_secs: u64,
) -> Result<LinuxReleaseStatus> {
    let schema = ensure_state_schema(platform.state_fs().as_ref(), now_secs)?;
    let current_exe = std::env::current_exe().map_err(|error| Error::io("linux_release", error))?;
    let mut state = load_release_state(platform.state_fs().as_ref())?.unwrap_or_default();
    if state.version == 0 {
        state.version = LINUX_RELEASE_STATE_VERSION;
    }
    let managed_layout = derive_managed_layout_from_binary(current_exe.as_path());
    let current = managed_layout.as_ref().map(|(_, pointer)| pointer.clone());
    let deploy_root = managed_layout
        .as_ref()
        .map(|(root, _)| root.to_string_lossy().into_owned())
        .or_else(|| state.deploy_root.clone());
    let rollback = deploy_root
        .as_deref()
        .and_then(|root| read_release_pointer_symlink(Path::new(root).join("rollback").as_path()));

    state.deploy_root = deploy_root.clone();
    state.current = current.clone();
    state.rollback = rollback.clone();
    if current.is_none() {
        state.rollout_state = LinuxReleaseRolloutState::Unmanaged;
    } else if matches!(
        state.rollout_state,
        LinuxReleaseRolloutState::RollbackTriggered
    ) && current.is_some()
    {
        state.rollout_state = LinuxReleaseRolloutState::RolledBack;
    } else if matches!(state.rollout_state, LinuxReleaseRolloutState::Unmanaged)
        && current.is_some()
    {
        state.rollout_state = LinuxReleaseRolloutState::Steady;
    }
    if state.last_updated_at == 0 {
        state.last_updated_at = now_secs;
    }
    write_release_state(platform.state_fs().as_ref(), &state)?;
    Ok(build_release_status(state, schema, current_exe.as_path()))
}

pub fn inspect_platform_linux_release(
    platform: &dyn Platform,
    _now_secs: u64,
) -> LinuxReleaseStatus {
    let current_exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("unknown"));
    let state = load_release_state(platform.state_fs().as_ref())
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            let managed_layout = derive_managed_layout_from_binary(current_exe.as_path());
            LinuxReleaseState {
                version: LINUX_RELEASE_STATE_VERSION,
                deploy_root: managed_layout
                    .as_ref()
                    .map(|(root, _)| root.to_string_lossy().into_owned()),
                current: managed_layout.as_ref().map(|(_, pointer)| pointer.clone()),
                rollback: managed_layout.as_ref().and_then(|(root, _)| {
                    read_release_pointer_symlink(root.join("rollback").as_path())
                }),
                rollout_state: if managed_layout.is_some() {
                    LinuxReleaseRolloutState::Steady
                } else {
                    LinuxReleaseRolloutState::Unmanaged
                },
                last_updated_at: 0,
                last_action: String::new(),
            }
        });
    let schema = read_state_schema(platform.state_fs().as_ref())
        .unwrap_or_default()
        .unwrap_or(StateSchemaStatus {
            version: BEETLE_STATE_SCHEMA_VERSION,
            upgraded_from: None,
            updated_at: 0,
        });
    build_release_status(state, schema, current_exe.as_path())
}

pub fn mark_current_release_steady(platform: &dyn Platform, now_secs: u64) -> Result<bool> {
    let mut state = load_release_state(platform.state_fs().as_ref())?.unwrap_or_default();
    if state.current.is_none() || state.rollout_state != LinuxReleaseRolloutState::PendingValidation
    {
        return Ok(false);
    }
    state.rollout_state = LinuxReleaseRolloutState::Steady;
    state.last_updated_at = now_secs;
    state.last_action = "validation_passed".to_string();
    write_release_state(platform.state_fs().as_ref(), &state)?;
    Ok(true)
}

pub fn rollback_current_release(
    platform: &dyn Platform,
    reason: &str,
    now_secs: u64,
) -> Result<bool> {
    let mut state = load_release_state(platform.state_fs().as_ref())?.unwrap_or_default();
    let Some(deploy_root) = state.deploy_root.as_deref() else {
        return Ok(false);
    };
    let Some(rollback) = state.rollback.clone() else {
        return Ok(false);
    };
    let current_link = Path::new(deploy_root).join("current");
    let rollback_link = Path::new(deploy_root).join("rollback");
    atomic_symlink(Path::new(&rollback.path), current_link.as_path())?;
    let _ = std::fs::remove_file(rollback_link);
    state.current = Some(rollback);
    state.rollback = None;
    state.rollout_state = LinuxReleaseRolloutState::RollbackTriggered;
    state.last_updated_at = now_secs;
    state.last_action = normalize_release_action(reason);
    write_release_state(platform.state_fs().as_ref(), &state)?;
    Ok(true)
}

pub fn ensure_state_schema(state_fs: &dyn StateFs, now_secs: u64) -> Result<StateSchemaStatus> {
    let current = read_state_schema(state_fs)?.unwrap_or_default();
    if current.version == 0 {
        let status = StateSchemaStatus {
            version: BEETLE_STATE_SCHEMA_VERSION,
            upgraded_from: None,
            updated_at: now_secs,
        };
        write_state_schema(state_fs, &status)?;
        return Ok(status);
    }
    if current.version >= BEETLE_STATE_SCHEMA_VERSION {
        return Ok(current);
    }
    let migrated = StateSchemaStatus {
        version: BEETLE_STATE_SCHEMA_VERSION,
        upgraded_from: Some(current.version),
        updated_at: now_secs,
    };
    write_state_schema(state_fs, &migrated)?;
    Ok(migrated)
}

fn build_release_status(
    state: LinuxReleaseState,
    schema: StateSchemaStatus,
    current_exe: &Path,
) -> LinuxReleaseStatus {
    let deploy_root = state.deploy_root.clone();
    LinuxReleaseStatus {
        managed: state.current.is_some() && deploy_root.is_some(),
        deploy_root,
        current: state.current.clone(),
        rollback: state.rollback.clone(),
        rollout_state: state.rollout_state,
        rollback_available: state.rollback.is_some(),
        current_exe: current_exe.to_string_lossy().into_owned(),
        state_schema_version: schema.version,
        state_schema_current: schema.version == BEETLE_STATE_SCHEMA_VERSION,
        systemd_unit_consistent: inspect_systemd_unit_consistency(),
        init_script_consistent: inspect_init_script_consistency(),
        last_updated_at: state.last_updated_at,
        last_action: state.last_action,
    }
}

fn normalize_release_action(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        "release_action".to_string()
    } else {
        trimmed.to_string()
    }
}

fn load_release_state(state_fs: &dyn StateFs) -> Result<Option<LinuxReleaseState>> {
    let Some(bytes) = state_fs.read(REL_PATH_LINUX_RELEASE_STATE)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| Error::config("linux_release_state", error.to_string()))
}

fn write_release_state(state_fs: &dyn StateFs, state: &LinuxReleaseState) -> Result<()> {
    let payload = serde_json::to_vec_pretty(state)
        .map_err(|error| Error::config("linux_release_state", error.to_string()))?;
    state_fs.write(REL_PATH_LINUX_RELEASE_STATE, &payload)
}

fn read_state_schema(state_fs: &dyn StateFs) -> Result<Option<StateSchemaStatus>> {
    let Some(bytes) = state_fs.read(REL_PATH_STATE_SCHEMA_STATUS)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| Error::config("state_schema", error.to_string()))
}

fn write_state_schema(state_fs: &dyn StateFs, status: &StateSchemaStatus) -> Result<()> {
    let payload = serde_json::to_vec_pretty(status)
        .map_err(|error| Error::config("state_schema", error.to_string()))?;
    state_fs.write(REL_PATH_STATE_SCHEMA_STATUS, &payload)
}

fn derive_managed_layout_from_binary(path: &Path) -> Option<(PathBuf, LinuxReleasePointer)> {
    let canonical = path
        .canonicalize()
        .ok()
        .unwrap_or_else(|| path.to_path_buf());
    let release_dir = canonical.parent()?;
    let releases_dir = release_dir.parent()?;
    if releases_dir.file_name()?.to_str()? != "releases" {
        return None;
    }
    let deploy_root = releases_dir.parent()?.to_path_buf();
    let name = release_dir.file_name()?.to_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some((
        deploy_root,
        LinuxReleasePointer {
            name,
            path: release_dir.to_string_lossy().into_owned(),
        },
    ))
}

fn read_release_pointer_symlink(link_path: &Path) -> Option<LinuxReleasePointer> {
    let target = std::fs::read_link(link_path).ok()?;
    let abs = if target.is_absolute() {
        target
    } else {
        link_path.parent()?.join(target)
    };
    let canonical = abs.canonicalize().ok().unwrap_or(abs);
    let release_dir = canonical;
    let name = release_dir.file_name()?.to_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(LinuxReleasePointer {
        name,
        path: release_dir.to_string_lossy().into_owned(),
    })
}

fn inspect_systemd_unit_consistency() -> Option<bool> {
    crate::runtime::linux_systemd::inspect_beetle_systemd_unit_consistency()
}

fn inspect_init_script_consistency() -> Option<bool> {
    let path = Path::new("/etc/init.d/beetle");
    let content = std::fs::read_to_string(path).ok()?;
    Some(
        content
            .lines()
            .any(|line| line.contains("start-stop-daemon -S") && line.contains("-- supervise")),
    )
}

fn atomic_symlink(target: &Path, link_path: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    let parent = link_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| Error::io("linux_release_symlink", error))?;
    let file_name = link_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("link");
    let temp = parent.join(format!(".{}.tmp.{}", file_name, std::process::id()));
    let _ = std::fs::remove_file(&temp);
    symlink(target, &temp).map_err(|error| Error::io("linux_release_symlink", error))?;
    match std::fs::rename(&temp, link_path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(Error::io("linux_release_symlink", error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        derive_managed_layout_from_binary, read_release_pointer_symlink, LinuxReleasePointer,
        LinuxReleaseRolloutState, LinuxReleaseState, LinuxReleaseStatus, StateSchemaStatus,
        BEETLE_STATE_SCHEMA_VERSION, REL_PATH_LINUX_RELEASE_STATE, REL_PATH_STATE_SCHEMA_STATUS,
    };
    use crate::error::Result;
    use crate::platform::StateFs;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{}/", rel_path.trim_end_matches('/'))
            };
            let files = self.files.lock().unwrap_or_else(|error| error.into_inner());
            let mut names = BTreeSet::new();
            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let tail = &key[prefix.len()..];
                if tail.is_empty() {
                    continue;
                }
                if let Some((dir, _)) = tail.split_once('/') {
                    names.insert(format!("{dir}/"));
                } else {
                    names.insert(tail.to_string());
                }
            }
            Ok(names.into_iter().collect())
        }
    }

    #[test]
    fn derive_managed_release_from_release_binary_path() {
        let result = derive_managed_layout_from_binary(Path::new(
            "/opt/beetle/releases/20260407-aarch64/beetle",
        ))
        .expect("managed release");
        assert_eq!(result.0, Path::new("/opt/beetle"));
        assert_eq!(result.1.name, "20260407-aarch64");
        assert_eq!(result.1.path, "/opt/beetle/releases/20260407-aarch64");
    }

    #[test]
    fn release_and_schema_state_round_trip_json() {
        let fs = MemoryStateFs::default();
        let release = LinuxReleaseState {
            version: 1,
            deploy_root: Some("/opt/beetle".to_string()),
            current: Some(LinuxReleasePointer {
                name: "r1".to_string(),
                path: "/opt/beetle/releases/r1".to_string(),
            }),
            rollback: Some(LinuxReleasePointer {
                name: "r0".to_string(),
                path: "/opt/beetle/releases/r0".to_string(),
            }),
            rollout_state: LinuxReleaseRolloutState::PendingValidation,
            last_updated_at: 10,
            last_action: "deploy".to_string(),
        };
        let schema = StateSchemaStatus {
            version: BEETLE_STATE_SCHEMA_VERSION,
            upgraded_from: None,
            updated_at: 10,
        };
        fs.write(
            REL_PATH_LINUX_RELEASE_STATE,
            &serde_json::to_vec(&release).unwrap(),
        )
        .unwrap();
        fs.write(
            REL_PATH_STATE_SCHEMA_STATUS,
            &serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();

        let stored_release: LinuxReleaseState =
            serde_json::from_slice(&fs.read(REL_PATH_LINUX_RELEASE_STATE).unwrap().unwrap())
                .unwrap();
        let stored_schema: StateSchemaStatus =
            serde_json::from_slice(&fs.read(REL_PATH_STATE_SCHEMA_STATUS).unwrap().unwrap())
                .unwrap();
        assert_eq!(stored_release, release);
        assert_eq!(stored_schema, schema);
    }

    #[test]
    fn release_status_serializes_rollout_state() {
        let status = LinuxReleaseStatus {
            managed: true,
            deploy_root: Some("/opt/beetle".to_string()),
            current: Some(LinuxReleasePointer {
                name: "r2".to_string(),
                path: "/opt/beetle/releases/r2".to_string(),
            }),
            rollback: None,
            rollout_state: LinuxReleaseRolloutState::Steady,
            rollback_available: false,
            current_exe: "/opt/beetle/releases/r2/beetle".to_string(),
            state_schema_version: BEETLE_STATE_SCHEMA_VERSION,
            state_schema_current: true,
            systemd_unit_consistent: Some(true),
            init_script_consistent: Some(true),
            last_updated_at: 20,
            last_action: "validation_passed".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"rollout_state\":\"steady\""));
    }

    #[test]
    fn read_release_pointer_symlink_returns_none_for_missing_link() {
        assert!(read_release_pointer_symlink(Path::new("/no/such/link")).is_none());
    }
}
