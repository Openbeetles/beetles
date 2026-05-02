//! 统一硬件设备控制工具。根据配置生成单个 `device_control` tool，LLM 按 device_id 操作。
//! Unified hardware device control tool. Generates a single `device_control` tool from config.

use crate::config::DeviceEntry;
use crate::error::{Error, Result};
use crate::tools::{
    Tool, ToolApprovalMode, ToolCapabilityContract, ToolContext, ToolEffectClass,
    ToolExecutionShape, ToolMetadata, ToolRiskLevel, ToolRollbackKind,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// 输出类设备最小操作间隔（ms）。
const DEVICE_MIN_INTERVAL_MS: u64 = 2000;
/// 输入类设备最小读取间隔（ms）。
const DEVICE_READ_MIN_INTERVAL_MS: u64 = 500;
/// tool description 拼接总长上限（字节）。
const MAX_TOOL_DESCRIPTION_LEN: usize = 2048;

fn is_input_type(device_type: &str) -> bool {
    matches!(device_type, "gpio_in" | "adc_in" | "dht")
}

/// 每设备运行时状态：上次操作时间 + 操作锁。
struct DeviceState {
    last_op: std::sync::Mutex<Option<Instant>>,
    busy: AtomicBool,
}

/// 守卫：Drop 时若未 disarm 则调用 release_lock，避免 dispatch panic 后锁永不释放。
struct ReleaseLockGuard<'a> {
    tool: &'a DeviceControlTool,
    idx: usize,
    released: std::cell::Cell<bool>,
}

impl<'a> ReleaseLockGuard<'a> {
    fn new(tool: &'a DeviceControlTool, idx: usize) -> Self {
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

/// 统一硬件设备控制工具；持有设备配置与运行时状态。
pub struct DeviceControlTool {
    devices: Vec<DeviceEntry>,
    device_map: HashMap<String, usize>,
    states: Vec<DeviceState>,
    /// pwm_out 设备 ID → (LEDC channel 0–7, LEDC timer index 0–3)；每设备独立定时器以支持不同 frequency_hz。
    pwm_channels: HashMap<String, (u8, u8)>,
    description: String,
    schema: String,
    platform: Arc<dyn crate::Platform>,
}

impl DeviceControlTool {
    /// 从配置构造。`devices` 为空时不应调用（调用方在注册前检查）。
    pub fn new(devices: Vec<DeviceEntry>, platform: Arc<dyn crate::Platform>) -> Self {
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
        // Allocate (channel, timer) per pwm_out so each device can have its own frequency.
        let mut pwm_channels = HashMap::new();
        let mut next_ch: u8 = 0;
        for dev in &devices {
            if dev.device_type == "pwm_out" {
                pwm_channels.insert(dev.id.clone(), (next_ch, next_ch));
                next_ch += 1;
            }
        }
        let description = Self::build_description(&devices);
        let schema = Self::build_schema(&devices);
        Self {
            devices,
            device_map,
            states,
            pwm_channels,
            description,
            schema,
            platform,
        }
    }

    fn build_description(devices: &[DeviceEntry]) -> String {
        let mut desc = String::from("Control or read hardware devices. Available devices:\n");
        for dev in devices {
            let line = format!(
                "- {} ({}): {} — {}\n",
                dev.id, dev.device_type, dev.what, dev.how
            );
            if desc.len() + line.len() > MAX_TOOL_DESCRIPTION_LEN {
                desc.push_str("...(truncated)");
                break;
            }
            desc.push_str(&line);
        }
        desc
    }

    fn build_schema(devices: &[DeviceEntry]) -> String {
        let mut schema =
            String::from(r#"{"type":"object","properties":{"device_id":{"type":"string","enum":["#);
        for (idx, device) in devices.iter().enumerate() {
            if idx > 0 {
                schema.push(',');
            }
            crate::util::push_json_string_escaped(&mut schema, &device.id);
        }
        schema.push_str(
            r#"],"description":"Target device ID"},"params":{"type":"object","description":"Device-specific params, e.g. {\"value\": 1} or {\"duty\": 50}; read-only devices need no params"}},"required":["device_id"]}"#,
        );
        schema
    }

    fn check_rate_limit(&self, idx: usize, device_type: &str) -> Result<()> {
        let interval_ms = if is_input_type(device_type) {
            DEVICE_READ_MIN_INTERVAL_MS
        } else {
            DEVICE_MIN_INTERVAL_MS
        };
        let state = &self.states[idx];
        let last = state.last_op.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = *last {
            let elapsed = t.elapsed().as_millis() as u64;
            if elapsed < interval_ms {
                return Err(Error::config(
                    "device_control",
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
            return Err(Error::config("device_control", "device is busy"));
        }
        Ok(())
    }

    fn release_lock(&self, idx: usize) {
        self.states[idx].busy.store(false, Ordering::Release);
    }

    fn dispatch(&self, dev: &DeviceEntry, params: &Value) -> Result<String> {
        match dev.device_type.as_str() {
            "gpio_out" => self.platform.drive_gpio_out(&dev.pins, params),
            "gpio_in" => self.platform.drive_gpio_in(&dev.pins, params, &dev.options),
            "pwm_out" => {
                let (ch, timer_idx) = *self.pwm_channels.get(&dev.id).ok_or_else(|| {
                    Error::config(
                        "device_control",
                        "no LEDC channel allocated for this pwm_out device",
                    )
                })?;
                self.platform
                    .drive_pwm_out(&dev.pins, params, &dev.options, ch, timer_idx)
            }
            "adc_in" => self.platform.drive_adc_in(&dev.pins, params, &dev.options),
            "buzzer" => self.platform.drive_buzzer(&dev.pins, params),
            "dht" => self.platform.drive_dht(&dev.pins, params, &dev.options),
            other => Err(Error::config(
                "device_control",
                format!(
                    "unsupported device_type '{}' (driver not yet implemented)",
                    other
                ),
            )),
        }
    }
}

impl Tool for DeviceControlTool {
    fn name(&self) -> &'static str {
        "device_control"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> &str {
        &self.schema
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let m = crate::tools::parse_tool_args(args, "device_control")?;
        let device_id = m
            .get("device_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("device_control", "missing or invalid device_id"))?;
        let idx = *self.device_map.get(device_id).ok_or_else(|| {
            Error::config(
                "device_control",
                format!("unknown device_id '{}'", device_id),
            )
        })?;
        let dev = &self.devices[idx];
        let params = m
            .get("params")
            .cloned()
            .unwrap_or(Value::Object(serde_json::Map::new()));

        // rate limit
        self.check_rate_limit(idx, &dev.device_type)?;

        // concurrency lock; guard ensures release on panic
        self.acquire_lock(idx)?;
        let guard = ReleaseLockGuard::new(self, idx);
        let result = self.dispatch(dev, &params);
        guard.release();

        // Update last-op time after completion (success or failure) for rate limit.
        let mut last = self.states[idx]
            .last_op
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *last = Some(Instant::now());

        match &result {
            Ok(r) => {
                log::info!(
                    "[device_control] device=\"{}\" type={} params={} result=ok {}",
                    dev.id,
                    dev.device_type,
                    params,
                    r
                );
            }
            Err(e) => {
                log::warn!(
                    "[device_control] device=\"{}\" type={} params={} result=err {}",
                    dev.id,
                    dev.device_type,
                    params,
                    e
                );
            }
        }
        result
    }

    fn requires_network(&self) -> bool {
        false
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[crate::orchestrator::RUNTIME_CAPABILITY_HARDWARE_GPIO])
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HardwareActuation)
            .with_risk_level(ToolRiskLevel::Critical)
            .with_approval_mode(ToolApprovalMode::ExplicitIntent)
            .with_rollback_kind(ToolRollbackKind::Irreversible)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = crate::tools::parse_tool_args(args, "device_control_governance")?;
        let device_id = obj
            .get("device_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| Error::config("device_control_governance", "missing device_id"))?;
        let idx = *self.device_map.get(device_id).ok_or_else(|| {
            Error::config(
                "device_control_governance",
                format!("unknown device_id '{}'", device_id),
            )
        })?;
        let device_type = self.devices[idx].device_type.as_str();
        let output_device = !is_input_type(device_type);
        Ok(self
            .metadata()
            .default_execution_shape(device_type)
            .with_effect_class(if output_device {
                ToolEffectClass::HardwareActuation
            } else {
                ToolEffectClass::HardwareRead
            })
            .with_risk_level(if output_device {
                ToolRiskLevel::Critical
            } else {
                ToolRiskLevel::Medium
            })
            .with_approval_mode(if output_device {
                ToolApprovalMode::ExplicitIntent
            } else {
                ToolApprovalMode::Automatic
            })
            .with_approval_granted(true)
            .with_rollback_kind(if output_device {
                ToolRollbackKind::Irreversible
            } else {
                ToolRollbackKind::None
            }))
    }

    fn catalog_execution_shape(&self) -> ToolExecutionShape {
        self.metadata().default_execution_shape(self.name())
    }
}
