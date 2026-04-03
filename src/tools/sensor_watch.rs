//! sensor_watch 工具：传感器持续监控与阈值告警。
//! sensor_watch tool: sensor monitoring with threshold alerts.

use crate::config::{DeviceEntry, I2cSensorEntry};
use crate::constants::{
    SENSOR_WATCH_MAX_ALERT_LEN, SENSOR_WATCH_MAX_ENTRIES, SENSOR_WATCH_MIN_INTERVAL_SECS,
};
use crate::error::{Error, Result};
use crate::i18n::{tr, Message as UiMessage, SensorWatchThresholdKind};
use crate::memory::MemoryStore;
use crate::tools::{parse_tool_args, Tool, ToolContext, ToolMetadata};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

/// 持久化文件路径。
const SENSOR_WATCHES_REL_PATH: &str = "memory/sensor_watches.json";

/// 阈值类型。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThresholdType {
    Above,
    Below,
    Change,
}

/// 单条传感器监控。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SensorWatch {
    pub id: String,
    pub device_id: String,
    pub interval_secs: u64,
    pub threshold_type: ThresholdType,
    pub threshold_value: f64,
    pub alert_message: String,
    pub channel: String,
    pub chat_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub last_value: Option<f64>,
    #[serde(default)]
    pub last_check_secs: u64,
}

pub struct SensorWatchTool {
    store: Arc<dyn MemoryStore + Send + Sync>,
    devices: Vec<DeviceEntry>,
    i2c_sensors: Vec<I2cSensorEntry>,
    schema: String,
}

impl SensorWatchTool {
    pub fn new(
        store: Arc<dyn MemoryStore + Send + Sync>,
        devices: Vec<DeviceEntry>,
        i2c_sensors: Vec<I2cSensorEntry>,
    ) -> Self {
        let schema = Self::build_schema(&devices, &i2c_sensors);
        Self {
            store,
            devices,
            i2c_sensors,
            schema,
        }
    }

    fn build_schema(devices: &[DeviceEntry], i2c_sensors: &[I2cSensorEntry]) -> String {
        let mut schema = String::from(
            r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: add|list|remove|update"},"id":{"type":"string","description":"Watch ID (for remove/update)"},"device_id":{"type":"string","enum":["#,
        );
        let mut wrote_any = false;
        for device in devices.iter().filter(|d| {
            d.device_type == "adc_in" || d.device_type == "gpio_in" || d.device_type == "dht"
        }) {
            if wrote_any {
                schema.push(',');
            }
            crate::util::push_json_string_escaped(&mut schema, &device.id);
            wrote_any = true;
        }
        for sensor in i2c_sensors.iter().filter(|sensor| sensor.model != "raw") {
            if wrote_any {
                schema.push(',');
            }
            crate::util::push_json_string_escaped(&mut schema, &sensor.id);
            wrote_any = true;
        }
        schema.push_str(
            r#"],"description":"Sensor device ID (adc_in, gpio_in, dht, or i2c_sensors id)"},"interval_secs":{"type":"integer","description":"Check interval in seconds (min 60)"},"threshold_type":{"type":"string","description":"Threshold type: above|below|change"},"threshold_value":{"type":"number","description":"Threshold value"},"alert_message":{"type":"string","description":"Alert message (max 512 bytes)"},"enabled":{"type":"boolean","description":"Enable/disable watch (for update)"}},"required":["op"]}"#,
        );
        schema
    }

    fn load_watches(&self) -> Result<Vec<SensorWatch>> {
        let content = self.store.get_daily_note(SENSOR_WATCHES_REL_PATH);
        match content {
            Ok(s) if !s.is_empty() => serde_json::from_str(&s)
                .map_err(|e| Error::config("tool_sensor_watch", e.to_string())),
            _ => Ok(Vec::new()),
        }
    }

    fn save_watches(&self, watches: &[SensorWatch]) -> Result<()> {
        let data = serde_json::to_string_pretty(watches)
            .map_err(|e| Error::config("tool_sensor_watch", e.to_string()))?;
        self.store.write_daily_note(SENSOR_WATCHES_REL_PATH, &data)
    }

    fn is_valid_sensor_device(&self, device_id: &str) -> bool {
        if self
            .i2c_sensors
            .iter()
            .any(|s| s.id == device_id && s.model != "raw")
        {
            return true;
        }
        self.devices.iter().any(|d| {
            d.id == device_id
                && (d.device_type == "adc_in"
                    || d.device_type == "gpio_in"
                    || d.device_type == "dht")
        })
    }
}

impl Tool for SensorWatchTool {
    fn name(&self) -> &'static str {
        "sensor_watch"
    }

    fn description(&self) -> &'static str {
        "Manage sensor monitoring watches with threshold alerts. Op: add (create watch on adc_in/gpio_in/dht or configured i2c_sensors except raw), list, remove (by id), update (toggle enabled or change threshold). Max 8 watches. Watches are checked by cron loop."
    }

    fn schema(&self) -> &str {
        &self.schema
    }

    fn execute(&self, args: &str, ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_sensor_watch")?;
        let op = obj
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::config("tool_sensor_watch", "missing op"))?;

        match op {
            "add" => {
                let device_id = obj
                    .get("device_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing device_id"))?;
                if !self.is_valid_sensor_device(device_id) {
                    return Err(Error::config(
                        "tool_sensor_watch",
                        format!(
                            "device_id '{}' not found or not a monitorable sensor (adc_in/gpio_in/dht or i2c_sensors non-raw)",
                            device_id
                        ),
                    ));
                }

                let interval_secs = obj
                    .get("interval_secs")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing interval_secs"))?;
                if interval_secs < SENSOR_WATCH_MIN_INTERVAL_SECS {
                    return Err(Error::config(
                        "tool_sensor_watch",
                        format!(
                            "interval_secs must be >= {}",
                            SENSOR_WATCH_MIN_INTERVAL_SECS
                        ),
                    ));
                }

                let threshold_type_str = obj
                    .get("threshold_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing threshold_type"))?;
                let threshold_type = match threshold_type_str {
                    "above" => ThresholdType::Above,
                    "below" => ThresholdType::Below,
                    "change" => ThresholdType::Change,
                    _ => {
                        return Err(Error::config(
                            "tool_sensor_watch",
                            "threshold_type must be above|below|change",
                        ));
                    }
                };

                let threshold_value = obj
                    .get("threshold_value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing threshold_value"))?;

                let alert_message = obj
                    .get("alert_message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing alert_message"))?;
                if alert_message.len() > SENSOR_WATCH_MAX_ALERT_LEN {
                    return Err(Error::config(
                        "tool_sensor_watch",
                        format!("alert_message exceeds {} bytes", SENSOR_WATCH_MAX_ALERT_LEN),
                    ));
                }

                let mut watches = self.load_watches()?;
                if watches.len() >= SENSOR_WATCH_MAX_ENTRIES {
                    return Err(Error::config(
                        "tool_sensor_watch",
                        format!("max {} watches reached", SENSOR_WATCH_MAX_ENTRIES),
                    ));
                }

                let channel = ctx.current_channel().unwrap_or("cron").to_string();
                let chat_id = ctx.current_chat_id().unwrap_or("cron").to_string();
                let id = format!("sw_{}", crate::util::current_unix_secs());

                let watch = SensorWatch {
                    id: id.clone(),
                    device_id: device_id.to_string(),
                    interval_secs,
                    threshold_type,
                    threshold_value,
                    alert_message: alert_message.to_string(),
                    channel,
                    chat_id,
                    enabled: true,
                    last_value: None,
                    last_check_secs: 0,
                };
                watches.push(watch);
                self.save_watches(&watches)?;

                Ok(json!({"op": "add", "id": id, "ok": true}).to_string())
            }
            "list" => {
                let watches = self.load_watches()?;
                let list: Vec<Value> = watches
                    .iter()
                    .map(|w| {
                        json!({
                            "id": w.id,
                            "device_id": w.device_id,
                            "interval_secs": w.interval_secs,
                            "threshold_type": w.threshold_type,
                            "threshold_value": w.threshold_value,
                            "alert_message": w.alert_message,
                            "enabled": w.enabled,
                            "last_value": w.last_value,
                        })
                    })
                    .collect();
                Ok(json!({"op": "list", "watches": list, "count": list.len()}).to_string())
            }
            "remove" => {
                let id = obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing id"))?;
                let mut watches = self.load_watches()?;
                let before = watches.len();
                watches.retain(|w| w.id != id);
                if watches.len() == before {
                    return Ok(
                        json!({"op": "remove", "ok": false, "error": "watch not found"})
                            .to_string(),
                    );
                }
                self.save_watches(&watches)?;
                Ok(json!({"op": "remove", "id": id, "ok": true}).to_string())
            }
            "update" => {
                let id = obj
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::config("tool_sensor_watch", "missing id"))?;
                let mut watches = self.load_watches()?;
                let watch = watches
                    .iter_mut()
                    .find(|w| w.id == id)
                    .ok_or_else(|| Error::config("tool_sensor_watch", "watch not found"))?;

                let mut updated = Vec::new();
                if let Some(enabled) = obj.get("enabled").and_then(|v| v.as_bool()) {
                    watch.enabled = enabled;
                    updated.push("enabled");
                }
                if let Some(interval) = obj.get("interval_secs").and_then(|v| v.as_u64()) {
                    if interval < SENSOR_WATCH_MIN_INTERVAL_SECS {
                        return Err(Error::config(
                            "tool_sensor_watch",
                            format!(
                                "interval_secs must be >= {}",
                                SENSOR_WATCH_MIN_INTERVAL_SECS
                            ),
                        ));
                    }
                    watch.interval_secs = interval;
                    updated.push("interval_secs");
                }
                if let Some(tv) = obj.get("threshold_value").and_then(|v| v.as_f64()) {
                    watch.threshold_value = tv;
                    updated.push("threshold_value");
                }
                if let Some(tt) = obj.get("threshold_type").and_then(|v| v.as_str()) {
                    watch.threshold_type = match tt {
                        "above" => ThresholdType::Above,
                        "below" => ThresholdType::Below,
                        "change" => ThresholdType::Change,
                        _ => {
                            return Err(Error::config(
                                "tool_sensor_watch",
                                "threshold_type must be above|below|change",
                            ));
                        }
                    };
                    updated.push("threshold_type");
                }
                if let Some(msg) = obj.get("alert_message").and_then(|v| v.as_str()) {
                    if msg.len() > SENSOR_WATCH_MAX_ALERT_LEN {
                        return Err(Error::config(
                            "tool_sensor_watch",
                            format!("alert_message exceeds {} bytes", SENSOR_WATCH_MAX_ALERT_LEN),
                        ));
                    }
                    watch.alert_message = msg.to_string();
                    updated.push("alert_message");
                }

                if updated.is_empty() {
                    return Ok(json!({
                        "op": "update",
                        "ok": false,
                        "error": "no fields to update"
                    })
                    .to_string());
                }

                self.save_watches(&watches)?;
                Ok(json!({"op": "update", "id": id, "updated": updated, "ok": true}).to_string())
            }
            _ => Err(Error::config(
                "tool_sensor_watch",
                format!("unknown op: {}", op),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
    }
}

/// 从 store 加载 sensor watches（供 cron 循环使用）。
pub fn load_sensor_watches(store: &dyn MemoryStore) -> Vec<SensorWatch> {
    match store.get_daily_note(SENSOR_WATCHES_REL_PATH) {
        Ok(s) if !s.is_empty() => serde_json::from_str(&s).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// 保存 sensor watches（供 cron 循环更新 last_value/last_check_secs）。
pub fn save_sensor_watches(store: &dyn MemoryStore, watches: &[SensorWatch]) {
    if let Ok(data) = serde_json::to_string_pretty(watches) {
        if let Err(e) = store.write_daily_note(SENSOR_WATCHES_REL_PATH, &data) {
            log::warn!("[sensor_watch] failed to save watches: {}", e);
        }
    }
}

/// 由 cron 循环调用：检查到期的 sensor watch，读取传感器值，判断阈值，触发时注入消息。
pub(crate) fn check_sensor_watches(
    store: &dyn MemoryStore,
    inbound_tx: &crate::bus::InboundTx,
    platform: &dyn crate::Platform,
    devices: &[DeviceEntry],
    i2c_sensors: &[I2cSensorEntry],
    loc: crate::i18n::Locale,
) {
    let mut watches = load_sensor_watches(store);
    if watches.is_empty() {
        return;
    }

    let now_secs = crate::util::current_unix_secs();
    let mut changed = false;

    for watch in watches.iter_mut() {
        if !watch.enabled {
            continue;
        }
        if now_secs.saturating_sub(watch.last_check_secs) < watch.interval_secs {
            continue;
        }

        let read_result = read_sensor_value(&watch.device_id, platform, devices, i2c_sensors);
        let value = match read_result {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "[sensor_watch] failed to read device '{}': {}",
                    watch.device_id,
                    e
                );
                watch.last_check_secs = now_secs;
                changed = true;
                continue;
            }
        };

        // Check threshold
        let triggered = match watch.threshold_type {
            ThresholdType::Above => value > watch.threshold_value,
            ThresholdType::Below => value < watch.threshold_value,
            ThresholdType::Change => {
                if let Some(last) = watch.last_value {
                    (value - last).abs() >= watch.threshold_value
                } else {
                    false // First reading, no change to compare
                }
            }
        };

        if triggered {
            let threshold_kind = match watch.threshold_type {
                ThresholdType::Above => SensorWatchThresholdKind::Above,
                ThresholdType::Below => SensorWatchThresholdKind::Below,
                ThresholdType::Change => SensorWatchThresholdKind::Change,
            };
            let content = tr(
                UiMessage::SensorWatchAlert {
                    id: watch.id.clone(),
                    label: watch.alert_message.clone(),
                    value,
                    threshold: watch.threshold_value,
                    threshold_kind,
                },
                loc,
            );
            match crate::bus::PcMsg::new(&watch.channel, &watch.chat_id, content) {
                Ok(msg) => {
                    if let Err(e) = inbound_tx.send(msg) {
                        log::warn!(
                            "[sensor_watch] failed to send alert for {}: {}",
                            watch.id,
                            e
                        );
                    } else {
                        log::info!("[sensor_watch] alert fired for {}", watch.id);
                    }
                }
                Err(e) => {
                    log::warn!("[sensor_watch] PcMsg::new for {} failed: {}", watch.id, e);
                }
            }
        }

        watch.last_value = Some(value);
        watch.last_check_secs = now_secs;
        changed = true;
    }

    if changed {
        save_sensor_watches(store, &watches);
    }
}

/// 读取传感器值：按 `device_id` 查 `i2c_sensors` 或 `hardware_devices`，经 [`Platform`] 委托。
fn read_sensor_value(
    device_id: &str,
    platform: &dyn crate::Platform,
    devices: &[DeviceEntry],
    i2c_sensors: &[I2cSensorEntry],
) -> Result<f64> {
    if let Some(e) = i2c_sensors.iter().find(|s| s.id == device_id) {
        if e.model == "raw" {
            return Err(Error::config(
                "sensor_watch",
                "i2c_sensor raw model has no numeric temperature/humidity for threshold watches",
            ));
        }
        let s = platform.drive_i2c_sensor(
            e.addr,
            e.model.as_str(),
            e.watch_field.as_str(),
            &e.options,
        )?;
        let v: Value = serde_json::from_str(&s)
            .map_err(|er| Error::config("sensor_watch", format!("i2c_sensor JSON: {}", er)))?;
        let field = e.watch_field.as_str();
        if field != "temperature" && field != "humidity" {
            return Err(Error::config(
                "sensor_watch",
                format!(
                    "i2c watch_field must be 'temperature' or 'humidity', got '{}'",
                    field
                ),
            ));
        }
        let val = v.get(field).and_then(|x| x.as_f64()).ok_or_else(|| {
            Error::config(
                "sensor_watch",
                format!(
                    "i2c_sensor response missing or non-numeric field '{}'",
                    field
                ),
            )
        })?;
        return Ok(val);
    }

    let dev = devices.iter().find(|d| d.id == device_id).ok_or_else(|| {
        Error::config("sensor_watch", format!("unknown device_id '{}'", device_id))
    })?;
    let empty = json!({});
    match dev.device_type.as_str() {
        "gpio_in" => {
            let s = platform.drive_gpio_in(&dev.pins, &empty, &dev.options)?;
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| Error::config("sensor_watch", format!("gpio_in JSON: {}", e)))?;
            let n = v
                .get("value")
                .and_then(|x| {
                    x.as_f64()
                        .or_else(|| x.as_i64().map(|i| i as f64))
                        .or_else(|| x.as_u64().map(|u| u as f64))
                })
                .ok_or_else(|| {
                    Error::config("sensor_watch", "gpio_in response missing numeric 'value'")
                })?;
            Ok(n)
        }
        "adc_in" => {
            let s = platform.drive_adc_in(&dev.pins, &empty, &dev.options)?;
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| Error::config("sensor_watch", format!("adc_in JSON: {}", e)))?;
            let raw = v
                .get("raw")
                .and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|i| i as f64)))
                .ok_or_else(|| {
                    Error::config("sensor_watch", "adc_in response missing numeric 'raw'")
                })?;
            Ok(raw)
        }
        "dht" => {
            let s = platform.drive_dht(&dev.pins, &empty, &dev.options)?;
            let v: Value = serde_json::from_str(&s)
                .map_err(|e| Error::config("sensor_watch", format!("dht JSON: {}", e)))?;
            let field = dev
                .options
                .get("watch_field")
                .and_then(|f| f.as_str())
                .unwrap_or("temperature");
            if field != "temperature" && field != "humidity" {
                return Err(Error::config(
                    "sensor_watch",
                    format!(
                        "dht watch_field must be 'temperature' or 'humidity', got '{}'",
                        field
                    ),
                ));
            }
            let val = v.get(field).and_then(|x| x.as_f64()).ok_or_else(|| {
                Error::config(
                    "sensor_watch",
                    format!("dht response missing or non-numeric field '{}'", field),
                )
            })?;
            Ok(val)
        }
        _ => Err(Error::config(
            "sensor_watch",
            format!(
                "device '{}' type '{}' not supported for monitoring",
                device_id, dev.device_type
            ),
        )),
    }
}
