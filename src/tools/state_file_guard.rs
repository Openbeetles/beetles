use crate::error::{Error, Result};
use crate::util::normalize_state_rel_path;
use std::borrow::Cow;

const PROTECTED_STATE_PATHS: &[&str] = &[
    "config/llm.json",
    "config/channels.json",
    "config/wifi.json",
    "memory/MEMORY.md",
];
const RETIRED_STATE_PATHS: &[&str] = &["config/SOUL.md", "config/USER.md"];

const SENSITIVE_READ_PATHS: &[&str] = &[
    "config/llm.json",
    "config/channels.json",
    "config/wifi.json",
];

pub(crate) fn normalize_state_tool_path(path_arg: &str, stage: &'static str) -> Result<String> {
    normalize_state_rel_path(path_arg).map_err(|_| Error::config(stage, "invalid path"))
}

pub(crate) fn ensure_state_path_mutable(rel_path: &str, stage: &'static str) -> Result<()> {
    if PROTECTED_STATE_PATHS.contains(&rel_path) {
        return Err(Error::config(stage, "path is protected"));
    }
    if RETIRED_STATE_PATHS.contains(&rel_path) {
        return Err(Error::config(stage, "path is retired"));
    }
    Ok(())
}

pub(crate) fn sanitize_state_file_read<'a>(
    rel_path: &str,
    raw: &'a [u8],
    stage: &'static str,
) -> Result<Cow<'a, [u8]>> {
    if RETIRED_STATE_PATHS.contains(&rel_path) {
        return Err(Error::config(stage, "path is retired"));
    }
    if !SENSITIVE_READ_PATHS.contains(&rel_path) {
        return Ok(Cow::Borrowed(raw));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| Error::config(stage, "sensitive config file is not valid UTF-8"))?;
    Ok(Cow::Owned(
        crate::util::redact_sensitive_config_text(text).into_bytes(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{ensure_state_path_mutable, sanitize_state_file_read};

    #[test]
    fn retired_state_paths_are_not_mutable() {
        let err = ensure_state_path_mutable("config/SOUL.md", "test").unwrap_err();
        assert!(format!("{err}").contains("path is retired"));
    }

    #[test]
    fn retired_state_paths_are_not_readable() {
        let err = sanitize_state_file_read("config/USER.md", b"legacy", "test").unwrap_err();
        assert!(format!("{err}").contains("path is retired"));
    }
}
