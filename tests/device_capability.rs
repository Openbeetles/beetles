use beetle::{
    build_device_capability_registry_from_input, DeviceCapabilityBuildInput,
    DeviceCapabilityPlaneEntry, DeviceCapabilityPlaneMountModel, DEVICE_CAPABILITY_SENSOR,
    DEVICE_CAPABILITY_VISION, DEVICE_CAPABILITY_VOICE,
};

fn entry(registry: &beetle::DeviceCapabilityRegistry, id: &str) -> DeviceCapabilityPlaneEntry {
    registry.get(id).expect("device capability entry")
}

#[test]
fn linux_discovery_first_keeps_voice_candidate_unmounted_until_selected() {
    let registry = build_device_capability_registry_from_input(DeviceCapabilityBuildInput {
        voice_compiled: true,
        voice_configured: false,
        voice_candidate_count: 2,
        voice_runtime_active: false,
        vision_compiled: true,
        vision_configured: false,
        vision_candidate_count: 1,
        vision_runtime_active: false,
        sensor_compiled: true,
        sensor_configured: false,
        sensor_candidate_count: 0,
        sensor_runtime_active: false,
        linux_discovery_first: true,
    });

    let voice = entry(&registry, DEVICE_CAPABILITY_VOICE);
    assert_eq!(
        voice.contract.mount_model,
        DeviceCapabilityPlaneMountModel::DiscoveryFirst
    );
    assert!(voice.discovered);
    assert_eq!(voice.candidate_count, 2);
    assert!(!voice.mounted);
    assert!(!voice.runtime_active);

    let vision = entry(&registry, DEVICE_CAPABILITY_VISION);
    assert!(vision.discovered);
    assert_eq!(vision.candidate_count, 1);
    assert!(!vision.mounted);
}

#[test]
fn esp_config_gated_keeps_unconfigured_planes_zero_side_effect() {
    let registry = build_device_capability_registry_from_input(DeviceCapabilityBuildInput {
        voice_compiled: true,
        voice_configured: false,
        voice_candidate_count: 0,
        voice_runtime_active: false,
        vision_compiled: false,
        vision_configured: false,
        vision_candidate_count: 0,
        vision_runtime_active: false,
        sensor_compiled: true,
        sensor_configured: false,
        sensor_candidate_count: 0,
        sensor_runtime_active: false,
        linux_discovery_first: false,
    });

    let voice = entry(&registry, DEVICE_CAPABILITY_VOICE);
    assert_eq!(
        voice.contract.mount_model,
        DeviceCapabilityPlaneMountModel::ConfigFeatureGated
    );
    assert!(!voice.discovered);
    assert!(!voice.mounted);
    assert!(!voice.runtime_active);

    let sensor = entry(&registry, DEVICE_CAPABILITY_SENSOR);
    assert_eq!(
        sensor.contract.mount_model,
        DeviceCapabilityPlaneMountModel::ConfigFeatureGated
    );
    assert!(!sensor.discovered);
    assert!(!sensor.mounted);

    assert!(registry.get(DEVICE_CAPABILITY_VISION).is_none());
}

#[test]
fn configured_voice_plane_mounts_and_reports_runtime_ready() {
    let registry = build_device_capability_registry_from_input(DeviceCapabilityBuildInput {
        voice_compiled: true,
        voice_configured: true,
        voice_candidate_count: 1,
        voice_runtime_active: true,
        vision_compiled: true,
        vision_configured: false,
        vision_candidate_count: 0,
        vision_runtime_active: false,
        sensor_compiled: true,
        sensor_configured: false,
        sensor_candidate_count: 0,
        sensor_runtime_active: false,
        linux_discovery_first: true,
    });

    let voice = entry(&registry, DEVICE_CAPABILITY_VOICE);
    assert!(voice.configured);
    assert!(voice.mounted);
    assert!(voice.runtime_active);
}

#[test]
fn configured_sensor_plane_mounts_without_promoting_vision_plane() {
    let registry = build_device_capability_registry_from_input(DeviceCapabilityBuildInput {
        voice_compiled: true,
        voice_configured: false,
        voice_candidate_count: 0,
        voice_runtime_active: false,
        vision_compiled: true,
        vision_configured: false,
        vision_candidate_count: 0,
        vision_runtime_active: false,
        sensor_compiled: true,
        sensor_configured: true,
        sensor_candidate_count: 3,
        sensor_runtime_active: true,
        linux_discovery_first: false,
    });

    let sensor = entry(&registry, DEVICE_CAPABILITY_SENSOR);
    assert!(sensor.discovered);
    assert_eq!(sensor.candidate_count, 3);
    assert!(sensor.mounted);
    assert!(sensor.runtime_active);

    let voice = entry(&registry, DEVICE_CAPABILITY_VOICE);
    assert!(!voice.mounted);
    assert!(!voice.runtime_active);
}
