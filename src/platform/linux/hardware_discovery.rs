//! Linux hardware discovery via udev + sysfs, with USB audio capability classification.
//! 基于 udev + sysfs 的 Linux 硬件发现，当前用于 USB 音频设备归类。

use crate::error::{Error, Result};
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const STAGE: &str = "hardware_discovery";

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct ResolvedUsbAudioDevice {
    pub device_ref: String,
    pub label: String,
    pub playback_pcm: Option<String>,
    pub capture_pcm: Option<String>,
    pub vendor_id: String,
    pub product_id: String,
    pub serial: Option<String>,
    pub physical_path: String,
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{Error, ResolvedUsbAudioDevice, Result, STAGE};
    use crate::platform::{
        HardwareCapability, HardwareDiscovery, HardwareDiscoveryItem, HardwareDiscoveryQuery,
        HardwareDiscoveryResponse,
    };
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs;

    #[derive(Default)]
    struct UsbAudioDeviceBuilder {
        label: String,
        vendor_id: String,
        product_id: String,
        serial: Option<String>,
        physical_path: String,
        playback_pcm: Option<String>,
        capture_pcm: Option<String>,
        has_output: bool,
        has_input: bool,
    }

    impl UsbAudioDeviceBuilder {
        fn finish(self) -> ResolvedUsbAudioDevice {
            ResolvedUsbAudioDevice {
                device_ref: build_device_ref(
                    self.vendor_id.as_str(),
                    self.product_id.as_str(),
                    self.serial.as_deref(),
                    self.physical_path.as_str(),
                ),
                label: self.label,
                playback_pcm: self.playback_pcm,
                capture_pcm: self.capture_pcm,
                vendor_id: self.vendor_id,
                product_id: self.product_id,
                serial: self.serial,
                physical_path: self.physical_path,
            }
        }
    }

    #[derive(Default)]
    pub struct LinuxHardwareDiscovery;

    impl LinuxHardwareDiscovery {
        pub fn new() -> Self {
            Self
        }
    }

    impl HardwareDiscovery for LinuxHardwareDiscovery {
        fn discover(&self, query: &HardwareDiscoveryQuery) -> Result<HardwareDiscoveryResponse> {
            let mut items = Vec::new();
            for device in enumerate_usb_audio_devices()? {
                let caps = capabilities_for_resolved(&device);
                if !caps.iter().any(|cap| *cap == query.capability) {
                    continue;
                }
                items.push(HardwareDiscoveryItem {
                    device_ref: device.device_ref.clone(),
                    label: device.label.clone(),
                    kind: "usb_device".to_string(),
                    capabilities: caps,
                    is_default: false,
                    metadata: json!({
                        "backend": "alsa",
                        "alsa_playback_hint": device.playback_pcm,
                        "alsa_capture_hint": device.capture_pcm,
                        "vendor_id": device.vendor_id,
                        "product_id": device.product_id,
                        "serial": device.serial,
                        "physical_path": device.physical_path,
                    }),
                });
            }
            items.sort_by(|a, b| {
                a.label
                    .cmp(&b.label)
                    .then_with(|| a.device_ref.cmp(&b.device_ref))
            });
            Ok(HardwareDiscoveryResponse {
                bus: query.bus,
                capability: query.capability,
                items,
            })
        }
    }

    pub(crate) fn resolve_usb_audio_output_device(
        device_ref: Option<&str>,
    ) -> Result<ResolvedUsbAudioDevice> {
        let mut output_devices: Vec<ResolvedUsbAudioDevice> = enumerate_usb_audio_devices()?
            .into_iter()
            .filter(|device| device.playback_pcm.is_some())
            .collect();
        output_devices.sort_by(|a, b| {
            a.label
                .cmp(&b.label)
                .then_with(|| a.device_ref.cmp(&b.device_ref))
        });

        if let Some(device_ref) = device_ref {
            for device in output_devices {
                if device.device_ref == device_ref {
                    return Ok(device);
                }
            }
            return Err(Error::config(
                STAGE,
                format!("selected USB device not found: {}", device_ref),
            ));
        }

        match output_devices.len() {
            0 => Err(Error::config(STAGE, "no USB audio output device found")),
            1 => Ok(output_devices.remove(0)),
            _ => Err(Error::config(
                STAGE,
                "multiple USB audio output devices found; speaker.device_ref is required",
            )),
        }
    }

    fn enumerate_usb_audio_devices() -> Result<Vec<ResolvedUsbAudioDevice>> {
        let mut enumerator = udev::Enumerator::new().map_err(map_other)?;
        enumerator.match_subsystem("sound").map_err(map_other)?;

        let mut merged: BTreeMap<String, UsbAudioDeviceBuilder> = BTreeMap::new();
        for device in enumerator.scan_devices().map_err(map_other)? {
            let sysname = os_to_string(device.sysname());
            if !sysname.starts_with("card") {
                continue;
            }
            let Some(card_index) = parse_card_index(sysname.as_str()) else {
                continue;
            };
            let Some(usb_parent) = find_usb_parent(&device) else {
                continue;
            };
            let vendor_id = attr_str(&usb_parent, "idVendor").unwrap_or_default();
            let product_id = attr_str(&usb_parent, "idProduct").unwrap_or_default();
            if vendor_id.is_empty() || product_id.is_empty() {
                continue;
            }
            let serial = attr_str(&usb_parent, "serial");
            let physical_path = os_to_string(usb_parent.sysname());
            let key = build_device_ref(
                vendor_id.as_str(),
                product_id.as_str(),
                serial.as_deref(),
                physical_path.as_str(),
            );

            let (playback_devs, capture_devs) = sound_capabilities(card_index)?;
            if playback_devs.is_empty() && capture_devs.is_empty() {
                continue;
            }

            let entry = merged.entry(key).or_default();
            if entry.label.is_empty() {
                entry.label = build_label(&usb_parent, sysname.as_str());
                entry.vendor_id = vendor_id.clone();
                entry.product_id = product_id.clone();
                entry.serial = serial.clone();
                entry.physical_path = physical_path.clone();
            }
            if let Some(dev_idx) = playback_devs.first().copied() {
                entry.has_output = true;
                if entry.playback_pcm.is_none() {
                    entry.playback_pcm = Some(format!("plughw:{},{}", card_index, dev_idx));
                }
            }
            if let Some(dev_idx) = capture_devs.first().copied() {
                entry.has_input = true;
                if entry.capture_pcm.is_none() {
                    entry.capture_pcm = Some(format!("plughw:{},{}", card_index, dev_idx));
                }
            }
        }

        Ok(merged
            .into_values()
            .map(UsbAudioDeviceBuilder::finish)
            .collect())
    }

    fn sound_capabilities(card_index: usize) -> Result<(Vec<usize>, Vec<usize>)> {
        let mut playback = Vec::new();
        let mut capture = Vec::new();
        let entries = fs::read_dir("/sys/class/sound").map_err(|e| Error::io(STAGE, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(STAGE, e))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some((entry_card, dev_idx, kind)) = parse_pcm_sysname(name.as_ref()) else {
                continue;
            };
            if entry_card != card_index {
                continue;
            }
            match kind {
                'p' => playback.push(dev_idx),
                'c' => capture.push(dev_idx),
                _ => {}
            }
        }
        playback.sort_unstable();
        playback.dedup();
        capture.sort_unstable();
        capture.dedup();
        Ok((playback, capture))
    }

    fn parse_card_index(sysname: &str) -> Option<usize> {
        sysname.strip_prefix("card")?.parse::<usize>().ok()
    }

    fn parse_pcm_sysname(name: &str) -> Option<(usize, usize, char)> {
        let rest = name.strip_prefix("pcmC")?;
        let d_pos = rest.find('D')?;
        let card = rest[..d_pos].parse::<usize>().ok()?;
        let after_d = &rest[d_pos + 1..];
        let kind = after_d.chars().last()?;
        if kind != 'p' && kind != 'c' {
            return None;
        }
        let dev = after_d[..after_d.len().checked_sub(kind.len_utf8())?]
            .parse::<usize>()
            .ok()?;
        Some((card, dev, kind))
    }

    fn capabilities_for_resolved(device: &ResolvedUsbAudioDevice) -> Vec<HardwareCapability> {
        let mut caps = Vec::with_capacity(2);
        if device.playback_pcm.is_some() {
            caps.push(HardwareCapability::AudioOutput);
        }
        if device.capture_pcm.is_some() {
            caps.push(HardwareCapability::AudioInput);
        }
        caps
    }

    fn build_device_ref(
        vendor_id: &str,
        product_id: &str,
        serial: Option<&str>,
        physical_path: &str,
    ) -> String {
        let mut parts = vec![
            format!("vid={}", urlencoding::encode(vendor_id)),
            format!("pid={}", urlencoding::encode(product_id)),
        ];
        if let Some(serial) = serial.filter(|value| !value.trim().is_empty()) {
            parts.push(format!("serial={}", urlencoding::encode(serial.trim())));
        } else {
            parts.push(format!("path={}", urlencoding::encode(physical_path)));
        }
        format!("usb:{}", parts.join(":"))
    }

    fn build_label(device: &udev::Device, fallback: &str) -> String {
        let manufacturer = attr_str(device, "manufacturer");
        let product = attr_str(device, "product")
            .or_else(|| property_str(device, "ID_MODEL_FROM_DATABASE"))
            .or_else(|| property_str(device, "ID_MODEL"));
        match (manufacturer, product) {
            (Some(m), Some(p)) if !m.is_empty() && !p.is_empty() => {
                format!("{} {}", m.trim(), p.trim())
            }
            (None, Some(p)) | (Some(p), None) if !p.is_empty() => p.trim().to_string(),
            _ => fallback.to_string(),
        }
    }

    fn find_usb_parent(device: &udev::Device) -> Option<udev::Device> {
        let mut current = Some(device.clone());
        while let Some(dev) = current {
            if dev
                .subsystem()
                .and_then(OsStr::to_str)
                .is_some_and(|value| value == "usb")
                && dev
                    .devtype()
                    .and_then(OsStr::to_str)
                    .is_some_and(|value| value == "usb_device")
            {
                return Some(dev);
            }
            current = dev.parent();
        }
        None
    }

    fn attr_str(device: &udev::Device, key: &str) -> Option<String> {
        device
            .attribute_value(key)
            .map(os_to_string)
            .filter(|value| !value.trim().is_empty())
    }

    fn property_str(device: &udev::Device, key: &str) -> Option<String> {
        device
            .property_value(key)
            .map(os_to_string)
            .filter(|value| !value.trim().is_empty())
    }

    fn os_to_string(value: impl AsRef<OsStr>) -> String {
        value.as_ref().to_string_lossy().into_owned()
    }

    fn map_other<E>(error: E) -> Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Error::Other {
            source: Box::new(error),
            stage: STAGE,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{build_device_ref, parse_card_index, parse_pcm_sysname};

        #[test]
        fn parses_card_index() {
            assert_eq!(parse_card_index("card0"), Some(0));
            assert_eq!(parse_card_index("card12"), Some(12));
            assert_eq!(parse_card_index("pcmC0D0p"), None);
        }

        #[test]
        fn parses_pcm_sysname() {
            assert_eq!(parse_pcm_sysname("pcmC2D0p"), Some((2, 0, 'p')));
            assert_eq!(parse_pcm_sysname("pcmC7D3c"), Some((7, 3, 'c')));
            assert_eq!(parse_pcm_sysname("controlC0"), None);
        }

        #[test]
        fn device_ref_prefers_serial() {
            let device_ref = build_device_ref("0d8c", "0014", Some("ABC 123"), "1-1.3");
            assert!(device_ref.contains("serial=ABC%20123"));
            assert!(!device_ref.contains("path="));
        }

        #[test]
        fn device_ref_falls_back_to_path() {
            let device_ref = build_device_ref("0d8c", "0014", None, "1-1.3");
            assert!(device_ref.contains("path=1-1.3"));
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::{Error, ResolvedUsbAudioDevice, Result, STAGE};
    use crate::platform::{HardwareDiscovery, HardwareDiscoveryQuery, HardwareDiscoveryResponse};

    #[derive(Default)]
    pub struct LinuxHardwareDiscovery;

    impl LinuxHardwareDiscovery {
        pub fn new() -> Self {
            Self
        }
    }

    impl HardwareDiscovery for LinuxHardwareDiscovery {
        fn discover(&self, query: &HardwareDiscoveryQuery) -> Result<HardwareDiscoveryResponse> {
            Ok(HardwareDiscoveryResponse {
                bus: query.bus,
                capability: query.capability,
                items: Vec::new(),
            })
        }
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn resolve_usb_audio_output_device(
        _device_ref: Option<&str>,
    ) -> Result<ResolvedUsbAudioDevice> {
        Err(Error::config(
            STAGE,
            "USB hardware discovery is only available on target_os=linux",
        ))
    }
}

pub use imp::LinuxHardwareDiscovery;
#[cfg_attr(not(target_os = "linux"), allow(unused_imports))]
pub(crate) use imp::resolve_usb_audio_output_device;
