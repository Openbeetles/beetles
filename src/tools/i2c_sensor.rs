//! i2c_sensor 工具：配置的 I2C 温湿度传感器单次读取（SHT3x / AHT20 / raw）。
//! i2c_sensor tool: one-shot read for configured I2C env sensors.

use crate::config::I2cSensorEntry;
use crate::constants::I2C_SENSOR_RATE_LIMIT_MS;
use crate::error::{Error, Result};
use crate::tools::{parse_tool_args, Tool, ToolCapabilityContract, ToolContext, ToolMetadata};
use crate::Platform;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct DeviceState {
    last_op: std::sync::Mutex<Option<Instant>>,
    busy: AtomicBool,
}

struct ReleaseLockGuard<'a> {
    tool: &'a I2cSensorTool,
    idx: usize,
    released: std::cell::Cell<bool>,
}

impl<'a> ReleaseLockGuard<'a> {
    fn new(tool: &'a I2cSensorTool, idx: usize) -> Self {
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

const MAX_TOOL_DESCRIPTION_LEN: usize = 2048;

pub struct I2cSensorTool {
    platform: Arc<dyn Platform>,
    sensors: Vec<I2cSensorEntry>,
    device_map: HashMap<String, usize>,
    states: Vec<DeviceState>,
    description: String,
    schema: String,
}

impl I2cSensorTool {
    pub fn new(platform: Arc<dyn Platform>, sensors: Vec<I2cSensorEntry>) -> Self {
        let device_map: HashMap<String, usize> = sensors
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.clone(), i))
            .collect();
        let states: Vec<DeviceState> = sensors
            .iter()
            .map(|_| DeviceState {
                last_op: std::sync::Mutex::new(None),
                busy: AtomicBool::new(false),
            })
            .collect();
        let description = Self::build_description(&sensors);
        let schema = Self::build_schema(&sensors);
        Self {
            platform,
            sensors,
            device_map,
            states,
            description,
            schema,
        }
    }

    fn build_description(sensors: &[I2cSensorEntry]) -> String {
        let mut desc = String::from(
            "Read configured I2C environmental sensors (SHT3x, AHT20, or raw). Op: read. Available sensors:\n",
        );
        for s in sensors {
            let line = format!(
                "- {} (addr=0x{:02X}, model={}): {} — {}\n",
                s.id, s.addr, s.model, s.what, s.how
            );
            if desc.len() + line.len() > MAX_TOOL_DESCRIPTION_LEN {
                desc.push_str("...(truncated)");
                break;
            }
            desc.push_str(&line);
        }
        desc
    }

    fn build_schema(sensors: &[I2cSensorEntry]) -> String {
        let mut schema =
            String::from(r#"{"type":"object","properties":{"device_id":{"type":"string","enum":["#);
        for (idx, sensor) in sensors.iter().enumerate() {
            if idx > 0 {
                schema.push(',');
            }
            crate::util::push_json_string_escaped(&mut schema, &sensor.id);
        }
        schema.push_str(
            r#"],"description":"Configured I2C sensor ID"},"op":{"type":"string","description":"Operation: read"}},"required":["device_id","op"]}"#,
        );
        schema
    }

    fn check_rate_limit(&self, idx: usize) -> Result<()> {
        let state = &self.states[idx];
        let last = state.last_op.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = *last {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < I2C_SENSOR_RATE_LIMIT_MS {
                return Err(Error::config(
                    "i2c_sensor",
                    format!(
                        "rate limited: please wait {}ms before next read",
                        I2C_SENSOR_RATE_LIMIT_MS.saturating_sub(elapsed)
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
            return Err(Error::config("i2c_sensor", "sensor is busy"));
        }
        Ok(())
    }

    fn release_lock(&self, idx: usize) {
        self.states[idx].busy.store(false, Ordering::Release);
    }
}

impl Tool for I2cSensorTool {
    fn name(&self) -> &'static str {
        "i2c_sensor"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> &str {
        &self.schema
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "i2c_sensor")?;

        let device_id = obj
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("i2c_sensor", "missing device_id"))?;

        let idx = *self.device_map.get(device_id).ok_or_else(|| {
            Error::config("i2c_sensor", format!("unknown device_id '{}'", device_id))
        })?;

        let op = obj
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("i2c_sensor", "missing op"))?;

        if op != "read" {
            return Err(Error::config(
                "i2c_sensor",
                format!("unknown op: '{}' (use read)", op),
            ));
        }

        self.check_rate_limit(idx)?;
        self.acquire_lock(idx)?;
        let guard = ReleaseLockGuard::new(self, idx);

        let s = &self.sensors[idx];
        let result = self
            .platform
            .drive_i2c_sensor(s.addr, s.model.as_str(), &s.options);

        guard.release();

        let mut last = self.states[idx]
            .last_op
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *last = Some(Instant::now());

        match &result {
            Ok(r) => log::info!("[i2c_sensor] device_id={} result=ok {}", device_id, r),
            Err(e) => log::warn!("[i2c_sensor] device_id={} result=err {}", device_id, e),
        }
        result
    }

    fn requires_network(&self) -> bool {
        false
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[
            crate::orchestrator::RUNTIME_CAPABILITY_HARDWARE_I2C,
            crate::orchestrator::RUNTIME_CAPABILITY_SENSOR,
        ])
    }
}
