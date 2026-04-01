use crate::error::{Error, Result};
use crate::util::normalize_state_rel_path;

const PROTECTED_STATE_PATHS: &[&str] = &[
    "config/llm.json",
    "config/channels.json",
    "config/wifi.json",
    "config/SOUL.md",
    "config/USER.md",
    "memory/MEMORY.md",
];

pub(crate) fn normalize_state_tool_path(path_arg: &str, stage: &'static str) -> Result<String> {
    normalize_state_rel_path(path_arg).map_err(|_| Error::config(stage, "invalid path"))
}

pub(crate) fn ensure_state_path_mutable(rel_path: &str, stage: &'static str) -> Result<()> {
    if PROTECTED_STATE_PATHS.contains(&rel_path) {
        return Err(Error::config(stage, "path is protected"));
    }
    Ok(())
}
