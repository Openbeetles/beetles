//! i2c_device 工具：I2C 总线通信。
//! i2c_device tool: I2C bus communication.

use crate::Platform;
use crate::config::I2cDeviceEntry;
use crate::constants::{
    I2C_MAX_READ_LEN, I2C_MAX_WRITE_LEN, I2C_READ_MIN_INTERVAL_MS, I2C_WRITE_MIN_INTERVAL_MS,
};
use crate::error::{Error, Result};
use crate::tools::{Tool, ToolContext, ToolMetadata, parse_tool_args};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// 每设备运行时状态：上次操作时间 + 操作锁。
struct DeviceState {
    last_op: std::sync::Mutex<Option<Instant>>,
    busy: AtomicBool,
}

/// 守卫：Drop 时若未 disarm 则调用 release_lock。
struct ReleaseLockGuard<'a> {
    tool: &'a I2cDeviceTool,
    idx: usize,
    released: std::cell::Cell<bool>,
}

impl<'a> ReleaseLockGuard<'a> {
    fn new(tool: &'a I2cDeviceTool, idx: usize) -> Self {
        Self {
            tool,
            idx,
            released: std::cell::Cell::new(false),
        }
    }
    fn release(self) {
        self.released.set(true);
        self.tool.release_lock(self.idx);
    }
}

impl Drop for ReleaseLockGuard<'_> {
    fn drop(&mut self) {
        if !self.released.get() {
            self.tool.release_lock(self.idx);
        }
    }
}

/// tool description 拼接总长上限（字节）。
const MAX_TOOL_DESCRIPTION_LEN: usize = 2048;

pub struct I2cDeviceTool {
    platform: Arc<dyn Platform>,
    devices: Vec<I2cDeviceEntry>,
    device_map: HashMap<String, usize>,
    states: Vec<DeviceState>,
    description: String,
    schema: String,
}

impl I2cDeviceTool {
    pub fn new(platform: Arc<dyn Platform>, devices: Vec<I2cDeviceEntry>) -> Self {
        let device_map: HashMap<String, usize> = devices
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.clone(), i))
            .collect();
        let states: Vec<DeviceState> = devices
            .iter()
            .map(|_| DeviceState {
                last_op: std::sync::Mutex::new(None),
                busy: AtomicBool::new(false),
            })
            .collect();
        let description = Self::build_description(&devices);
        let schema = Self::build_schema(&devices);
        Self {
            platform,
            devices,
            device_map,
            states,
            description,
            schema,
        }
    }

    fn build_description(devices: &[I2cDeviceEntry]) -> String {
        let mut desc = String::from(
            "I2C bus communication. Read/write registers on configured I2C devices. Available devices:\n",
        );
        for dev in devices {
            let line = format!(
                "- {} (addr=0x{:02X}): {} — {}\n",
                dev.id, dev.addr, dev.what, dev.how
            );
            if desc.len() + line.len() > MAX_TOOL_DESCRIPTION_LEN {
                desc.push_str("...(truncated)");
                break;
            }
            desc.push_str(&line);
        }
        desc
    }

    fn build_schema(devices: &[I2cDeviceEntry]) -> String {
        let mut schema =
            String::from(r#"{"type":"object","properties":{"device_id":{"type":"string","enum":["#);
        for (idx, device) in devices.iter().enumerate() {
            if idx > 0 {
                schema.push(',');
            }
            crate::util::push_json_string_escaped(&mut schema, &device.id);
        }
        schema.push_str(
            r#"],"description":"Target I2C device ID"},"op":{"type":"string","description":"Operation: read|write"},"register":{"type":"integer","description":"Register address (0-255)"},"len":{"type":"integer","description":"Number of bytes to read (1-32, for read op)"},"data":{"type":"array","items":{"type":"integer"},"description":"Bytes to write (for write op, max 32)"}},"required":["device_id","op","register"]}"#,
        );
        schema
    }

    fn check_rate_limit(&self, idx: usize, is_write: bool) -> Result<()> {
        let interval_ms = if is_write {
            I2C_WRITE_MIN_INTERVAL_MS
        } else {
            I2C_READ_MIN_INTERVAL_MS
        };
        let state = &self.states[idx];
        let last = state.last_op.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = *last {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < interval_ms {
                return Err(Error::config(
                    "i2c_device",
                    format!(
                        "rate limited: please wait {}ms before next operation",
                        interval_ms - elapsed
                    ),
                ));
            }
        }
        Ok(())
    }

    fn acquire_lock(&self, idx: usize) -> Result<()> {
        if self.states[idx]
            .busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(Error::config("i2c_device", "device is busy"));
        }
        Ok(())
    }

    fn release_lock(&self, idx: usize) {
        self.states[idx].busy.store(false, Ordering::Release);
    }
}

impl Tool for I2cDeviceTool {
    fn name(&self) -> &'static str {
        "i2c_device"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> &str {
        &self.schema
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "i2c_device")?;

        let device_id = obj
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("i2c_device", "missing device_id"))?;

        let idx = *self.device_map.get(device_id).ok_or_else(|| {
            Error::config("i2c_device", format!("unknown device_id '{}'", device_id))
        })?;

        let op = obj
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("i2c_device", "missing op"))?;

        let register = obj
            .get("register")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| Error::config("i2c_device", "missing or invalid register (0-255)"))?;
        if register > 255 {
            return Err(Error::config("i2c_device", "register must be 0-255"));
        }

        let dev = &self.devices[idx];
        let is_write = op == "write";

        self.check_rate_limit(idx, is_write)?;
        self.acquire_lock(idx)?;
        let guard = ReleaseLockGuard::new(self, idx);

        let result = match op {
            "read" => {
                let len = obj.get("len").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
                if len == 0 || len > I2C_MAX_READ_LEN {
                    return Err(Error::config(
                        "i2c_device",
                        format!("len must be 1-{}", I2C_MAX_READ_LEN),
                    ));
                }

                let data = self.platform.i2c_read(dev.addr, register as u8, len)?;
                let hex: Vec<String> = data.iter().map(|b| format!("0x{:02X}", b)).collect();
                Ok(json!({
                    "op": "read",
                    "device_id": device_id,
                    "register": register,
                    "len": data.len(),
                    "data": data,
                    "hex": hex
                })
                .to_string())
            }
            "write" => {
                let data_arr = obj
                    .get("data")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| Error::config("i2c_device", "missing data array for write"))?;
                if data_arr.is_empty() || data_arr.len() > I2C_MAX_WRITE_LEN {
                    return Err(Error::config(
                        "i2c_device",
                        format!("data length must be 1-{}", I2C_MAX_WRITE_LEN),
                    ));
                }
                let data: Vec<u8> = data_arr
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .and_then(|n| if n <= 255 { Some(n as u8) } else { None })
                            .ok_or_else(|| Error::config("i2c_device", "data bytes must be 0-255"))
                    })
                    .collect::<Result<Vec<u8>>>()?;

                self.platform.i2c_write(dev.addr, register as u8, &data)?;
                Ok(json!({
                    "op": "write",
                    "device_id": device_id,
                    "register": register,
                    "bytes_written": data.len(),
                    "ok": true
                })
                .to_string())
            }
            _ => Err(Error::config(
                "i2c_device",
                format!("unknown op: {} (use read|write)", op),
            )),
        };

        guard.release();

        // Update last-op time
        let mut last = self.states[idx]
            .last_op
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *last = Some(Instant::now());

        match &result {
            Ok(r) => {
                log::info!(
                    "[i2c_device] device=\"{}\" addr=0x{:02X} op={} reg={} result=ok {}",
                    dev.id,
                    dev.addr,
                    op,
                    register,
                    r
                );
            }
            Err(e) => {
                log::warn!(
                    "[i2c_device] device=\"{}\" addr=0x{:02X} op={} reg={} result=err {}",
                    dev.id,
                    dev.addr,
                    op,
                    register,
                    e
                );
            }
        }
        result
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::debug()
    }
}
