//! Device capability planes for audio / camera / sensors.
//! audio / camera / sensors 的统一 capability plane 合同与快照。

use crate::config::AppConfig;
use crate::platform::{HardwareCapability, HardwareDiscoveryBus, HardwareDiscoveryQuery, Platform};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const DEVICE_CAPABILITY_VOICE: &str = "voice";
pub const DEVICE_CAPABILITY_VISION: &str = "vision";
pub const DEVICE_CAPABILITY_SENSOR: &str = "sensor";

const DEVICE_CAPABILITY_ORDER: [&str; 3] = [
    DEVICE_CAPABILITY_VOICE,
    DEVICE_CAPABILITY_VISION,
    DEVICE_CAPABILITY_SENSOR,
];

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCapabilityPlaneMountModel {
    DiscoveryFirst,
    ConfigFeatureGated,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCapabilityObservationMode {
    GovernedRealtime,
    GovernedPeriodic,
    DiscoveryOnly,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct DeviceCapabilityPlaneContract {
    pub mount_model: DeviceCapabilityPlaneMountModel,
    pub observation_mode: DeviceCapabilityObservationMode,
    pub governed_by_runtime_mode: bool,
    pub governed_by_admission: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct DeviceCapabilityPlaneEntry {
    pub id: &'static str,
    pub configured: bool,
    pub discovered: bool,
    pub mounted: bool,
    pub runtime_active: bool,
    pub candidate_count: usize,
    pub contract: DeviceCapabilityPlaneContract,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceCapabilityRegistry {
    entries: HashMap<&'static str, DeviceCapabilityPlaneEntry>,
}

impl DeviceCapabilityRegistry {
    pub fn get(&self, id: &str) -> Option<DeviceCapabilityPlaneEntry> {
        self.entries.get(id).copied()
    }

    pub fn is_mounted(&self, id: &str) -> bool {
        self.get(id).is_some_and(|entry| entry.mounted)
    }

    pub fn list(&self) -> Vec<DeviceCapabilityPlaneEntry> {
        DEVICE_CAPABILITY_ORDER
            .iter()
            .filter_map(|id| self.entries.get(id).copied())
            .collect()
    }

    fn insert(&mut self, entry: DeviceCapabilityPlaneEntry) {
        self.entries.insert(entry.id, entry);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceCapabilityBuildInput {
    pub voice_compiled: bool,
    pub voice_configured: bool,
    pub voice_candidate_count: usize,
    pub voice_runtime_active: bool,
    pub vision_compiled: bool,
    pub vision_configured: bool,
    pub vision_candidate_count: usize,
    pub vision_runtime_active: bool,
    pub sensor_compiled: bool,
    pub sensor_configured: bool,
    pub sensor_candidate_count: usize,
    pub sensor_runtime_active: bool,
    pub linux_discovery_first: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DeviceCapabilityPlaneSnapshot {
    pub id: String,
    pub configured: bool,
    pub discovered: bool,
    pub mounted: bool,
    pub runtime_active: bool,
    pub candidate_count: usize,
    pub mount_model: DeviceCapabilityPlaneMountModel,
    pub observation_mode: DeviceCapabilityObservationMode,
    pub governed_by_runtime_mode: bool,
    pub governed_by_admission: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_reasons: Vec<String>,
}

pub fn build_device_capability_registry_from_input(
    input: DeviceCapabilityBuildInput,
) -> DeviceCapabilityRegistry {
    let mut registry = DeviceCapabilityRegistry::default();

    maybe_insert_entry(
        &mut registry,
        DeviceCapabilityEntrySpec {
            id: DEVICE_CAPABILITY_VOICE,
            compiled: input.voice_compiled,
            configured: input.voice_configured,
            candidate_count: input.voice_candidate_count,
            runtime_active: input.voice_runtime_active,
            mount_model: mount_model_for_voice(input.linux_discovery_first),
            observation_mode: DeviceCapabilityObservationMode::GovernedRealtime,
        },
    );
    maybe_insert_entry(
        &mut registry,
        DeviceCapabilityEntrySpec {
            id: DEVICE_CAPABILITY_VISION,
            compiled: input.vision_compiled,
            configured: input.vision_configured,
            candidate_count: input.vision_candidate_count,
            runtime_active: input.vision_runtime_active,
            mount_model: mount_model_for_vision(input.linux_discovery_first),
            observation_mode: DeviceCapabilityObservationMode::DiscoveryOnly,
        },
    );
    maybe_insert_entry(
        &mut registry,
        DeviceCapabilityEntrySpec {
            id: DEVICE_CAPABILITY_SENSOR,
            compiled: input.sensor_compiled,
            configured: input.sensor_configured,
            candidate_count: input.sensor_candidate_count,
            runtime_active: input.sensor_runtime_active,
            mount_model: DeviceCapabilityPlaneMountModel::ConfigFeatureGated,
            observation_mode: DeviceCapabilityObservationMode::GovernedPeriodic,
        },
    );

    registry
}

pub fn build_device_capability_registry(
    config: &AppConfig,
    platform: &dyn Platform,
) -> DeviceCapabilityRegistry {
    let sensor_candidate_count = configured_sensor_candidate_count(config);
    let voice_candidate_count = voice_candidate_count(platform);
    let build_input = DeviceCapabilityBuildInput {
        voice_compiled: crate::compiled_voice_capability(),
        voice_configured: config.audio.as_ref().is_some_and(|audio| audio.enabled),
        voice_candidate_count,
        voice_runtime_active: platform.audio_duplex_capabilities().profile()
            != crate::platform::AudioDuplexProfile::Unavailable,
        vision_compiled: crate::compiled_vision_capability(),
        vision_configured: false,
        vision_candidate_count: discovery_candidate_count(platform, HardwareCapability::Camera),
        vision_runtime_active: false,
        sensor_compiled: crate::compiled_sensor_capability(),
        sensor_configured: sensor_candidate_count > 0,
        sensor_candidate_count,
        sensor_runtime_active: sensor_candidate_count > 0,
        linux_discovery_first: cfg!(not(any(target_arch = "xtensa", target_arch = "riscv32"))),
    };
    build_device_capability_registry_from_input(build_input)
}

pub fn build_device_capability_snapshots(
    config: &AppConfig,
    platform: &dyn Platform,
) -> Vec<DeviceCapabilityPlaneSnapshot> {
    let registry = build_device_capability_registry(config, platform);
    build_device_capability_snapshots_for_registry(&registry)
}

pub fn build_device_capability_snapshots_for_registry(
    registry: &DeviceCapabilityRegistry,
) -> Vec<DeviceCapabilityPlaneSnapshot> {
    registry
        .list()
        .into_iter()
        .map(|entry| {
            let mut degraded_reasons = Vec::new();
            if entry.configured && entry.mounted && !entry.runtime_active {
                degraded_reasons.push("runtime_inactive".to_string());
            }
            if entry.discovered && !entry.mounted {
                degraded_reasons.push("not_mounted".to_string());
            }
            DeviceCapabilityPlaneSnapshot {
                id: entry.id.to_string(),
                configured: entry.configured,
                discovered: entry.discovered,
                mounted: entry.mounted,
                runtime_active: entry.runtime_active,
                candidate_count: entry.candidate_count,
                mount_model: entry.contract.mount_model,
                observation_mode: entry.contract.observation_mode,
                governed_by_runtime_mode: entry.contract.governed_by_runtime_mode,
                governed_by_admission: entry.contract.governed_by_admission,
                degraded_reasons,
            }
        })
        .collect()
}

fn maybe_insert_entry(registry: &mut DeviceCapabilityRegistry, spec: DeviceCapabilityEntrySpec) {
    if !spec.compiled {
        return;
    }
    let mounted = spec.configured;
    registry.insert(DeviceCapabilityPlaneEntry {
        id: spec.id,
        configured: spec.configured,
        discovered: spec.candidate_count > 0,
        mounted,
        runtime_active: mounted && spec.runtime_active,
        candidate_count: spec.candidate_count,
        contract: DeviceCapabilityPlaneContract {
            mount_model: spec.mount_model,
            observation_mode: spec.observation_mode,
            governed_by_runtime_mode: true,
            governed_by_admission: true,
        },
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeviceCapabilityEntrySpec {
    id: &'static str,
    compiled: bool,
    configured: bool,
    candidate_count: usize,
    runtime_active: bool,
    mount_model: DeviceCapabilityPlaneMountModel,
    observation_mode: DeviceCapabilityObservationMode,
}

fn mount_model_for_voice(linux_discovery_first: bool) -> DeviceCapabilityPlaneMountModel {
    if linux_discovery_first {
        DeviceCapabilityPlaneMountModel::DiscoveryFirst
    } else {
        DeviceCapabilityPlaneMountModel::ConfigFeatureGated
    }
}

fn mount_model_for_vision(linux_discovery_first: bool) -> DeviceCapabilityPlaneMountModel {
    if linux_discovery_first {
        DeviceCapabilityPlaneMountModel::DiscoveryFirst
    } else {
        DeviceCapabilityPlaneMountModel::ConfigFeatureGated
    }
}

fn configured_sensor_candidate_count(config: &AppConfig) -> usize {
    let device_count = config
        .hardware_devices
        .iter()
        .filter(|device| matches!(device.device_type.as_str(), "adc_in" | "gpio_in" | "dht"))
        .count();
    let i2c_sensor_count = config
        .i2c_sensors
        .iter()
        .filter(|sensor| sensor.model != "raw")
        .count();
    device_count + i2c_sensor_count
}

fn voice_candidate_count(platform: &dyn Platform) -> usize {
    let Some(discovery) = platform.hardware_discovery() else {
        return 0;
    };
    let mut device_refs = HashSet::new();
    for capability in [
        HardwareCapability::AudioInput,
        HardwareCapability::AudioOutput,
    ] {
        match discovery.discover(&HardwareDiscoveryQuery {
            bus: HardwareDiscoveryBus::Usb,
            capability,
        }) {
            Ok(response) => {
                for item in response.items {
                    device_refs.insert(item.device_ref);
                }
            }
            Err(error) => {
                log::warn!(
                    "[device_capability] hardware discovery failed for {:?}: {}",
                    capability,
                    error
                );
            }
        }
    }
    device_refs.len()
}

fn discovery_candidate_count(platform: &dyn Platform, capability: HardwareCapability) -> usize {
    let Some(discovery) = platform.hardware_discovery() else {
        return 0;
    };
    match discovery.discover(&HardwareDiscoveryQuery {
        bus: HardwareDiscoveryBus::Usb,
        capability,
    }) {
        Ok(response) => response.items.len(),
        Err(error) => {
            log::warn!(
                "[device_capability] hardware discovery failed for {:?}: {}",
                capability,
                error
            );
            0
        }
    }
}
