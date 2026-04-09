//! kv_store 工具：持久化键值存储，底层为状态根下 JSON 文件（经 `StateFs`）。
//! 提供 get / set / delete / list_keys 四个操作，供 LLM 跨会话记忆用户偏好与状态。
//! kv_store tool: persistent key-value store via `StateFs` JSON file.
//! Supports get / set / delete / list_keys; used for cross-session LLM memory.

use crate::constants::{KV_STORE_MAX_ENTRIES, KV_STORE_MAX_KEY_LEN, KV_STORE_MAX_VALUE_LEN};
use crate::error::{Error, Result};
use crate::tools::{
    parse_tool_args, Tool, ToolCapabilityContract, ToolContext, ToolMetadata, ToolRiskLevel,
    ToolRollbackKind,
};
use crate::StateFs;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

/// 相对状态根路径，与历史 SPIFFS 布局一致。
const KV_STORE_REL_PATH: &str = "memory/kv_store.json";

/// 从状态根读取 KV map；文件不存在或解析失败时返回空 map（容错）。
fn load_map(fs: &(dyn StateFs + Send + Sync)) -> HashMap<String, String> {
    match fs.read(KV_STORE_REL_PATH) {
        Ok(Some(buf)) if buf.len() > 2 => serde_json::from_slice(&buf).unwrap_or_default(),
        _ => HashMap::new(),
    }
}

/// 将 KV map 序列化并写回状态根。
fn save_map(fs: &(dyn StateFs + Send + Sync), map: &HashMap<String, String>) -> Result<()> {
    let json =
        serde_json::to_vec(map).map_err(|e| Error::config("kv_store_save", e.to_string()))?;
    fs.write(KV_STORE_REL_PATH, &json)
}

/// 校验 key：只允许 [a-zA-Z0-9_\-.] 且非空、不超长。
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(Error::config("kv_store", "key must not be empty"));
    }
    if key.len() > KV_STORE_MAX_KEY_LEN {
        return Err(Error::config(
            "kv_store",
            format!("key too long (max {} bytes)", KV_STORE_MAX_KEY_LEN),
        ));
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return Err(Error::config(
            "kv_store",
            "key may only contain [a-zA-Z0-9_\\-.] characters",
        ));
    }
    Ok(())
}

/// 构造时注入与 [`crate::Platform::state_fs`] 同一套实现。
pub struct KvStoreTool {
    state_fs: Arc<dyn StateFs + Send + Sync>,
}

impl KvStoreTool {
    pub(crate) fn new(state_fs: Arc<dyn StateFs + Send + Sync>) -> Self {
        Self { state_fs }
    }
}

impl Tool for KvStoreTool {
    fn name(&self) -> &'static str {
        "kv_store"
    }
    fn description(&self) -> &'static str {
        "Persistent key-value store for cross-session memory (user preferences, state, notes). \
         Operations: \
         'get' — read a value by key; \
         'set' — write a key-value pair (persists across reboots); \
         'delete' — remove a key; \
         'list_keys' — return all stored keys. \
         Keys: alphanumeric, underscore, hyphen, dot; max 64 chars. \
         Values: UTF-8 string, max 512 bytes. Max 64 entries total."
    }
    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","enum":["get","set","delete","list_keys"],"description":"Operation to perform"},"key":{"type":"string","description":"Key name (required for get / set / delete)"},"value":{"type":"string","description":"Value to store (required for set)"}},"required":["op"]}"#
    }
    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let fs = self.state_fs.as_ref();
        let m = parse_tool_args(args, "kv_store")?;

        let op = m
            .get("op")
            .and_then(|o| o.as_str())
            .ok_or_else(|| Error::config("kv_store", "missing op"))?;

        match op {
            "get" => {
                let key = m
                    .get("key")
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| Error::config("kv_store", "missing key"))?;
                validate_key(key)?;
                let map = load_map(fs);
                match map.get(key) {
                    Some(v) => Ok(v.clone()),
                    None => Ok("key not found".to_string()),
                }
            }
            "set" => {
                let key = m
                    .get("key")
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| Error::config("kv_store", "missing key"))?;
                let value = m
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("kv_store", "missing value"))?;
                validate_key(key)?;
                if value.len() > KV_STORE_MAX_VALUE_LEN {
                    return Err(Error::config(
                        "kv_store",
                        format!("value too long (max {} bytes)", KV_STORE_MAX_VALUE_LEN),
                    ));
                }
                let mut map = load_map(fs);
                // 已有 key 更新不占新配额
                if !map.contains_key(key) && map.len() >= KV_STORE_MAX_ENTRIES {
                    return Err(Error::config(
                        "kv_store",
                        format!("store full (max {} entries)", KV_STORE_MAX_ENTRIES),
                    ));
                }
                map.insert(key.to_string(), value.to_string());
                save_map(fs, &map)?;
                Ok("ok".to_string())
            }
            "delete" => {
                let key = m
                    .get("key")
                    .and_then(|k| k.as_str())
                    .ok_or_else(|| Error::config("kv_store", "missing key"))?;
                validate_key(key)?;
                let mut map = load_map(fs);
                if map.remove(key).is_none() {
                    return Ok("key not found".to_string());
                }
                save_map(fs, &map)?;
                Ok("ok".to_string())
            }
            "list_keys" => {
                let map = load_map(fs);
                let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
                keys.sort_unstable();
                Ok(json!(keys).to_string())
            }
            _ => Err(Error::config(
                "kv_store",
                "op must be one of: get, set, delete, list_keys",
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_risk_level(ToolRiskLevel::Medium)
            .with_rollback_kind(ToolRollbackKind::CompensatingWrite)
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[
            crate::orchestrator::RUNTIME_CAPABILITY_STORAGE_STATE_FS,
        ])
    }
}
