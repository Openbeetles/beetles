//! Programmable reasoning runtime contracts and executors.

use crate::error::Result;
use crate::reasoning::execute_lua_query;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const LUA_QUERY_MAX_SCRIPT_BYTES: usize = 16 * 1024;
pub const LUA_QUERY_MAX_INPUT_BYTES: usize = 64 * 1024;
pub const LUA_QUERY_MAX_OUTPUT_BYTES: usize = 16 * 1024;
pub const LUA_QUERY_MAX_TRACE_ITEMS: usize = 16;
pub const LUA_QUERY_MAX_TRACE_CHARS: usize = 240;
pub const LUA_QUERY_DEFAULT_TIMEOUT_MS: u64 = 800;
pub const LUA_QUERY_MIN_TIMEOUT_MS: u64 = 50;
pub const LUA_QUERY_MAX_TIMEOUT_MS: u64 = 2_000;
pub const LUA_QUERY_MEMORY_LIMIT_BYTES: u64 = 64 * 1024 * 1024;
pub const LUA_QUERY_CPU_LIMIT_SECS: u64 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaQueryBudget {
    pub timeout_ms: u64,
    pub max_script_bytes: usize,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_trace_items: usize,
    pub max_trace_chars: usize,
    pub memory_limit_bytes: u64,
    pub cpu_limit_secs: u64,
}

impl Default for LuaQueryBudget {
    fn default() -> Self {
        Self {
            timeout_ms: LUA_QUERY_DEFAULT_TIMEOUT_MS,
            max_script_bytes: LUA_QUERY_MAX_SCRIPT_BYTES,
            max_input_bytes: LUA_QUERY_MAX_INPUT_BYTES,
            max_output_bytes: LUA_QUERY_MAX_OUTPUT_BYTES,
            max_trace_items: LUA_QUERY_MAX_TRACE_ITEMS,
            max_trace_chars: LUA_QUERY_MAX_TRACE_CHARS,
            memory_limit_bytes: LUA_QUERY_MEMORY_LIMIT_BYTES,
            cpu_limit_secs: LUA_QUERY_CPU_LIMIT_SECS,
        }
    }
}

impl LuaQueryBudget {
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms.clamp(LUA_QUERY_MIN_TIMEOUT_MS, LUA_QUERY_MAX_TIMEOUT_MS);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaQueryRequest {
    pub script: String,
    pub input: Value,
    pub budget: LuaQueryBudget,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LuaQueryResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default)]
    pub trace: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub budget: LuaQueryBudget,
}

impl LuaQueryResponse {
    pub fn success(result: Value, trace: Vec<String>, budget: LuaQueryBudget) -> Self {
        Self {
            ok: true,
            result: Some(result),
            trace,
            error_kind: None,
            error_message: None,
            budget,
        }
    }

    pub fn failure(
        error_kind: impl Into<String>,
        error_message: impl Into<String>,
        budget: LuaQueryBudget,
    ) -> Self {
        Self {
            ok: false,
            result: None,
            trace: Vec::new(),
            error_kind: Some(error_kind.into()),
            error_message: Some(error_message.into()),
            budget,
        }
    }
}

pub trait ReasoningExecutor: Send + Sync {
    fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse>;
}

#[derive(Default)]
pub struct DirectLuaSandboxExecutor;

impl ReasoningExecutor for DirectLuaSandboxExecutor {
    fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
        execute_lua_query(request)
    }
}

#[derive(Default)]
pub struct CurrentExecutableLuaSandboxExecutor;

impl ReasoningExecutor for CurrentExecutableLuaSandboxExecutor {
    fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
        let program = std::env::current_exe()
            .map_err(|error| crate::error::Error::io("lua_query_current_exe", error))?;
        SubprocessLuaSandboxExecutor::new(program).execute_query(request)
    }
}

#[derive(Clone, Debug)]
pub struct SubprocessLuaSandboxExecutor {
    program: PathBuf,
}

impl SubprocessLuaSandboxExecutor {
    pub fn new(program: PathBuf) -> Self {
        Self { program }
    }

    pub fn program(&self) -> &PathBuf {
        &self.program
    }
}

impl ReasoningExecutor for SubprocessLuaSandboxExecutor {
    fn execute_query(&self, request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
        let request_json = serde_json::to_vec(request).map_err(|error| {
            crate::error::Error::config("lua_query_request_json", error.to_string())
        })?;
        let mut child = Command::new(&self.program)
            .arg("reasoning-runner")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .spawn()
            .map_err(|error| crate::error::Error::io("lua_query_spawn", error))?;
        {
            use std::io::Write;
            let stdin = child.stdin.as_mut().ok_or_else(|| {
                crate::error::Error::config("lua_query_spawn", "missing child stdin")
            })?;
            stdin
                .write_all(&request_json)
                .map_err(|error| crate::error::Error::io("lua_query_request_write", error))?;
        }
        let _ = child.stdin.take();

        let deadline = Instant::now() + Duration::from_millis(request.budget.timeout_ms);
        loop {
            if child
                .try_wait()
                .map_err(|error| crate::error::Error::io("lua_query_wait", error))?
                .is_some()
            {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(LuaQueryResponse::failure(
                    "timeout",
                    format!("lua query timed out after {} ms", request.budget.timeout_ms),
                    request.budget.clone(),
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut handle) = child.stdout.take() {
            use std::io::Read;
            handle
                .read_to_end(&mut stdout)
                .map_err(|error| crate::error::Error::io("lua_query_stdout_read", error))?;
        }
        if let Some(mut handle) = child.stderr.take() {
            use std::io::Read;
            handle
                .read_to_end(&mut stderr)
                .map_err(|error| crate::error::Error::io("lua_query_stderr_read", error))?;
        }
        Ok(match serde_json::from_slice::<LuaQueryResponse>(&stdout) {
            Ok(response) => response,
            Err(error) => LuaQueryResponse::failure(
                "protocol_error",
                format!(
                    "invalid lua runner response: {}; stderr={}",
                    error,
                    String::from_utf8_lossy(&stderr)
                ),
                request.budget.clone(),
            ),
        })
    }
}

pub fn default_lua_query_capabilities() -> Vec<String> {
    vec![
        "read_input".to_string(),
        "emit_result".to_string(),
        "emit_trace".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_clamps_timeout_into_p1_range() {
        assert_eq!(
            LuaQueryBudget::default().with_timeout_ms(1).timeout_ms,
            LUA_QUERY_MIN_TIMEOUT_MS
        );
        assert_eq!(
            LuaQueryBudget::default().with_timeout_ms(9_999).timeout_ms,
            LUA_QUERY_MAX_TIMEOUT_MS
        );
    }

    #[test]
    fn default_capabilities_match_p1_contract() {
        assert_eq!(
            default_lua_query_capabilities(),
            vec![
                "read_input".to_string(),
                "emit_result".to_string(),
                "emit_trace".to_string()
            ]
        );
    }

    #[test]
    fn subprocess_executor_keeps_program_path() {
        let executor = SubprocessLuaSandboxExecutor::new(PathBuf::from("/tmp/beetle"));
        assert_eq!(executor.program(), &PathBuf::from("/tmp/beetle"));
    }
}
