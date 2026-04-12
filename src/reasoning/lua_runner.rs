//! Linux lua runner entry and pure execution helpers.

use crate::error::Result;
use crate::reasoning::runtime::{LuaQueryRequest, LuaQueryResponse, LUA_QUERY_MAX_TRACE_CHARS};
use mlua::{Lua, LuaSerdeExt, Value as LuaValue};
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub fn execute_lua_query(request: &LuaQueryRequest) -> Result<LuaQueryResponse> {
    if request.script.len() > request.budget.max_script_bytes {
        return Ok(LuaQueryResponse::failure(
            "budget_exceeded",
            format!(
                "script length exceeds {} bytes",
                request.budget.max_script_bytes
            ),
            request.budget.clone(),
        ));
    }
    let input_bytes = serde_json::to_vec(&request.input).unwrap_or_default();
    if input_bytes.len() > request.budget.max_input_bytes {
        return Ok(LuaQueryResponse::failure(
            "budget_exceeded",
            format!(
                "input length exceeds {} bytes",
                request.budget.max_input_bytes
            ),
            request.budget.clone(),
        ));
    }

    let trace = Arc::new(Mutex::new(Vec::new()));
    let lua = match build_lua_runtime(Arc::clone(&trace), request.budget.max_trace_items) {
        Ok(lua) => lua,
        Err(error) => {
            return Ok(LuaQueryResponse::failure(
                "runtime_init_error",
                error.to_string(),
                request.budget.clone(),
            ))
        }
    };

    let globals = lua.globals();
    let input = match lua.to_value(&request.input) {
        Ok(input) => input,
        Err(error) => {
            return Ok(LuaQueryResponse::failure(
                "input_encode_error",
                error.to_string(),
                request.budget.clone(),
            ))
        }
    };
    if let Err(error) = globals.set("input", input) {
        return Ok(LuaQueryResponse::failure(
            "input_bind_error",
            error.to_string(),
            request.budget.clone(),
        ));
    }

    let evaluated = match lua.load(&request.script).set_name("lua_query").eval::<LuaValue>() {
        Ok(value) => value,
        Err(error) => {
            return Ok(LuaQueryResponse::failure(
                "runtime_error",
                error.to_string(),
                request.budget.clone(),
            ))
        }
    };
    let result: Value = match lua.from_value(evaluated) {
        Ok(value) => value,
        Err(error) => {
            return Ok(LuaQueryResponse::failure(
                "result_decode_error",
                error.to_string(),
                request.budget.clone(),
            ))
        }
    };
    let output_bytes = serde_json::to_vec(&result).unwrap_or_default();
    if output_bytes.len() > request.budget.max_output_bytes {
        return Ok(LuaQueryResponse::failure(
            "budget_exceeded",
            format!(
                "result length exceeds {} bytes",
                request.budget.max_output_bytes
            ),
            request.budget.clone(),
        ));
    }

    let trace = trace
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    Ok(LuaQueryResponse::success(result, trace, request.budget.clone()))
}

fn build_lua_runtime(trace: Arc<Mutex<Vec<String>>>, max_trace_items: usize) -> Result<Lua> {
    let lua = Lua::new();
    let globals = lua.globals();
    for name in [
        "collectgarbage",
        "dofile",
        "load",
        "loadfile",
        "require",
        "package",
        "os",
        "io",
        "debug",
    ] {
        globals
            .set(name, LuaValue::Nil)
            .map_err(|error| crate::error::Error::config("lua_runtime_globals", error.to_string()))?;
    }

    let trace_fn = lua
        .create_function_mut(move |_, message: String| {
            let mut trace = trace.lock().unwrap_or_else(|error| error.into_inner());
            if trace.len() < max_trace_items {
                trace.push(sanitize_trace_item(&message));
            }
            Ok(())
        })
        .map_err(|error| crate::error::Error::config("lua_runtime_trace_fn", error.to_string()))?;
    globals
        .set("trace", trace_fn)
        .map_err(|error| crate::error::Error::config("lua_runtime_trace_set", error.to_string()))?;

    let encode_fn = lua
        .create_function(|lua, value: LuaValue| {
            let json: Value = lua.from_value(value)?;
            serde_json::to_string(&json).map_err(mlua::Error::external)
        })
        .map_err(|error| crate::error::Error::config("lua_runtime_json_encode", error.to_string()))?;
    globals
        .set("json_encode", encode_fn)
        .map_err(|error| crate::error::Error::config("lua_runtime_json_encode_set", error.to_string()))?;

    let decode_fn = lua
        .create_function(|lua, text: String| {
            let value: Value = serde_json::from_str(&text).map_err(mlua::Error::external)?;
            lua.to_value(&value)
        })
        .map_err(|error| crate::error::Error::config("lua_runtime_json_decode", error.to_string()))?;
    globals
        .set("json_decode", decode_fn)
        .map_err(|error| crate::error::Error::config("lua_runtime_json_decode_set", error.to_string()))?;

    Ok(lua)
}

fn sanitize_trace_item(value: &str) -> String {
    value.chars()
        .take(LUA_QUERY_MAX_TRACE_CHARS)
        .collect::<String>()
}

pub fn run_reasoning_runner_stdio() -> Result<()> {
    use std::io::{Read, Write};

    let mut request_bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut request_bytes)
        .map_err(|error| crate::error::Error::io("lua_runner_read_stdin", error))?;
    let request = match serde_json::from_slice::<LuaQueryRequest>(&request_bytes) {
        Ok(request) => request,
        Err(error) => {
            let response = LuaQueryResponse::failure(
                "protocol_error",
                error.to_string(),
                crate::reasoning::LuaQueryBudget::default(),
            );
            let encoded = serde_json::to_vec(&response).map_err(|encode_error| {
                crate::error::Error::config(
                    "lua_runner_protocol_encode",
                    encode_error.to_string(),
                )
            })?;
            std::io::stdout()
                .write_all(&encoded)
                .map_err(|write_error| crate::error::Error::io("lua_runner_write_stdout", write_error))?;
            return Ok(());
        }
    };

    #[cfg(target_os = "linux")]
    if let Err(error) = apply_process_limits(&request) {
        let response =
            LuaQueryResponse::failure("sandbox_error", error.to_string(), request.budget.clone());
        let encoded = serde_json::to_vec(&response).map_err(|encode_error| {
            crate::error::Error::config("lua_runner_sandbox_encode", encode_error.to_string())
        })?;
        std::io::stdout()
            .write_all(&encoded)
            .map_err(|write_error| crate::error::Error::io("lua_runner_write_stdout", write_error))?;
        return Ok(());
    }

    let response = execute_lua_query(&request)?;
    let encoded = serde_json::to_vec(&response)
        .map_err(|error| crate::error::Error::config("lua_runner_response_encode", error.to_string()))?;
    std::io::stdout()
        .write_all(&encoded)
        .map_err(|error| crate::error::Error::io("lua_runner_write_stdout", error))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_process_limits(request: &LuaQueryRequest) -> Result<()> {
    let address_limit = libc::rlimit {
        rlim_cur: request.budget.memory_limit_bytes as libc::rlim_t,
        rlim_max: request.budget.memory_limit_bytes as libc::rlim_t,
    };
    let cpu_limit = libc::rlimit {
        rlim_cur: request.budget.cpu_limit_secs as libc::rlim_t,
        rlim_max: request.budget.cpu_limit_secs as libc::rlim_t,
    };
    let file_limit = libc::rlimit {
        rlim_cur: request.budget.max_output_bytes as libc::rlim_t,
        rlim_max: request.budget.max_output_bytes as libc::rlim_t,
    };

    set_limit(libc::RLIMIT_AS, &address_limit, "lua_runner_rlimit_as")?;
    set_limit(libc::RLIMIT_CPU, &cpu_limit, "lua_runner_rlimit_cpu")?;
    set_limit(libc::RLIMIT_FSIZE, &file_limit, "lua_runner_rlimit_fsize")?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_limit(
    resource: libc::__rlimit_resource_t,
    limit: &libc::rlimit,
    stage: &'static str,
) -> Result<()> {
    let status = unsafe { libc::setrlimit(resource, limit) };
    if status == 0 {
        Ok(())
    } else {
        Err(crate::error::Error::io(stage, std::io::Error::last_os_error()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning::runtime::{default_lua_query_capabilities, LuaQueryBudget};
    use serde_json::json;

    #[test]
    fn lua_runner_executes_simple_query_program() {
        let response = execute_lua_query(&LuaQueryRequest {
            script: "return {sum = input.a + input.b}".to_string(),
            input: json!({"a": 2, "b": 3}),
            budget: LuaQueryBudget::default(),
            capabilities: default_lua_query_capabilities(),
        })
        .expect("lua execution response");
        assert!(response.ok);
        assert_eq!(response.result, Some(json!({"sum": 5})));
    }

    #[test]
    fn lua_runner_does_not_expose_dangerous_stdlibs() {
        let response = execute_lua_query(&LuaQueryRequest {
            script: "return {has_os = os ~= nil, has_io = io ~= nil, has_package = package ~= nil}"
                .to_string(),
            input: json!({}),
            budget: LuaQueryBudget::default(),
            capabilities: default_lua_query_capabilities(),
        })
        .expect("lua execution response");
        assert!(response.ok);
        assert_eq!(
            response.result,
            Some(json!({"has_os": false, "has_io": false, "has_package": false}))
        );
    }
}
