//! Build/package matrix contract for ESP/Linux packaging.
//! ESP/Linux 打包矩阵合同：默认保持全功能包，瘦身包仅通过显式 profile 选择。

use serde::Serialize;

pub const BUILD_PACKAGE_PROFILE_CORE_ONLY: &str = "core-only";
pub const BUILD_PACKAGE_PROFILE_VOICE: &str = "voice";
pub const BUILD_PACKAGE_PROFILE_VISION: &str = "vision";
pub const BUILD_PACKAGE_PROFILE_SENSOR: &str = "sensor";
pub const BUILD_PACKAGE_PROFILE_VOICE_VISION: &str = "voice+vision";
pub const BUILD_PACKAGE_PROFILE_VOICE_SENSOR: &str = "voice+sensor";
pub const BUILD_PACKAGE_PROFILE_VISION_SENSOR: &str = "vision+sensor";
pub const BUILD_PACKAGE_PROFILE_ESP_FULL: &str = "voice+vision+sensor";
pub const BUILD_PACKAGE_PROFILE_LINUX_FULL: &str = "linux-full";
pub const BUILD_PACKAGE_PROFILE_HOST_FULL: &str = "host-full";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct BuildPackageCapabilities {
    pub voice: bool,
    pub vision: bool,
    pub sensor: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BuildPackageTargetFamily {
    Esp,
    Linux,
    Host,
}

impl BuildPackageTargetFamily {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Esp => "esp",
            Self::Linux => "linux",
            Self::Host => "host",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct BuildPackageSnapshot {
    pub target_family: BuildPackageTargetFamily,
    pub profile: &'static str,
    pub default_full_package: bool,
    pub capabilities: BuildPackageCapabilities,
}

/// 当前构建是否包含 voice capability。
pub fn compiled_voice_capability() -> bool {
    cfg!(feature = "capability_voice")
}

/// 当前构建是否包含 vision capability。
pub fn compiled_vision_capability() -> bool {
    cfg!(feature = "capability_vision")
}

/// 当前构建是否包含 sensor capability。
pub fn compiled_sensor_capability() -> bool {
    cfg!(feature = "capability_sensor")
}

/// 当前构建的 capability 编译矩阵。
pub fn compiled_build_package_capabilities() -> BuildPackageCapabilities {
    BuildPackageCapabilities {
        voice: compiled_voice_capability(),
        vision: compiled_vision_capability(),
        sensor: compiled_sensor_capability(),
    }
}

/// 当前构建的正式 package/profile 契约。
pub fn current_build_package() -> BuildPackageSnapshot {
    let target_family = current_target_family();
    let capabilities = compiled_build_package_capabilities();
    let profile = option_env!("BEETLE_PACKAGE_PROFILE")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| infer_build_package_profile(target_family, capabilities));
    BuildPackageSnapshot {
        target_family,
        profile,
        default_full_package: profile == default_full_profile(target_family)
            && capabilities == full_capabilities(),
        capabilities,
    }
}

fn full_capabilities() -> BuildPackageCapabilities {
    BuildPackageCapabilities {
        voice: true,
        vision: true,
        sensor: true,
    }
}

fn current_target_family() -> BuildPackageTargetFamily {
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        BuildPackageTargetFamily::Esp
    } else if cfg!(target_os = "linux") {
        BuildPackageTargetFamily::Linux
    } else {
        BuildPackageTargetFamily::Host
    }
}

fn default_full_profile(target_family: BuildPackageTargetFamily) -> &'static str {
    match target_family {
        BuildPackageTargetFamily::Esp => BUILD_PACKAGE_PROFILE_ESP_FULL,
        BuildPackageTargetFamily::Linux => BUILD_PACKAGE_PROFILE_LINUX_FULL,
        BuildPackageTargetFamily::Host => BUILD_PACKAGE_PROFILE_HOST_FULL,
    }
}

fn infer_build_package_profile(
    target_family: BuildPackageTargetFamily,
    capabilities: BuildPackageCapabilities,
) -> &'static str {
    match (capabilities.voice, capabilities.vision, capabilities.sensor) {
        (false, false, false) => BUILD_PACKAGE_PROFILE_CORE_ONLY,
        (true, false, false) => BUILD_PACKAGE_PROFILE_VOICE,
        (false, true, false) => BUILD_PACKAGE_PROFILE_VISION,
        (false, false, true) => BUILD_PACKAGE_PROFILE_SENSOR,
        (true, true, false) => BUILD_PACKAGE_PROFILE_VOICE_VISION,
        (true, false, true) => BUILD_PACKAGE_PROFILE_VOICE_SENSOR,
        (false, true, true) => BUILD_PACKAGE_PROFILE_VISION_SENSOR,
        (true, true, true) => default_full_profile(target_family),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_full_profile, infer_build_package_profile, BuildPackageCapabilities,
        BuildPackageTargetFamily, BUILD_PACKAGE_PROFILE_CORE_ONLY, BUILD_PACKAGE_PROFILE_ESP_FULL,
        BUILD_PACKAGE_PROFILE_HOST_FULL, BUILD_PACKAGE_PROFILE_LINUX_FULL,
        BUILD_PACKAGE_PROFILE_SENSOR, BUILD_PACKAGE_PROFILE_VISION,
        BUILD_PACKAGE_PROFILE_VISION_SENSOR, BUILD_PACKAGE_PROFILE_VOICE,
        BUILD_PACKAGE_PROFILE_VOICE_SENSOR, BUILD_PACKAGE_PROFILE_VOICE_VISION,
    };

    #[test]
    fn infer_profile_covers_all_capability_combinations() {
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: false,
                    vision: false,
                    sensor: false,
                },
            ),
            BUILD_PACKAGE_PROFILE_CORE_ONLY
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: true,
                    vision: false,
                    sensor: false,
                },
            ),
            BUILD_PACKAGE_PROFILE_VOICE
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: false,
                    vision: true,
                    sensor: false,
                },
            ),
            BUILD_PACKAGE_PROFILE_VISION
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: false,
                    vision: false,
                    sensor: true,
                },
            ),
            BUILD_PACKAGE_PROFILE_SENSOR
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: true,
                    vision: true,
                    sensor: false,
                },
            ),
            BUILD_PACKAGE_PROFILE_VOICE_VISION
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: true,
                    vision: false,
                    sensor: true,
                },
            ),
            BUILD_PACKAGE_PROFILE_VOICE_SENSOR
        );
        assert_eq!(
            infer_build_package_profile(
                BuildPackageTargetFamily::Esp,
                BuildPackageCapabilities {
                    voice: false,
                    vision: true,
                    sensor: true,
                },
            ),
            BUILD_PACKAGE_PROFILE_VISION_SENSOR
        );
    }

    #[test]
    fn full_profile_name_depends_on_target_family() {
        let full = BuildPackageCapabilities {
            voice: true,
            vision: true,
            sensor: true,
        };
        assert_eq!(
            infer_build_package_profile(BuildPackageTargetFamily::Esp, full),
            BUILD_PACKAGE_PROFILE_ESP_FULL
        );
        assert_eq!(
            infer_build_package_profile(BuildPackageTargetFamily::Linux, full),
            BUILD_PACKAGE_PROFILE_LINUX_FULL
        );
        assert_eq!(
            infer_build_package_profile(BuildPackageTargetFamily::Host, full),
            BUILD_PACKAGE_PROFILE_HOST_FULL
        );
        assert_eq!(
            default_full_profile(BuildPackageTargetFamily::Esp),
            BUILD_PACKAGE_PROFILE_ESP_FULL
        );
        assert_eq!(
            default_full_profile(BuildPackageTargetFamily::Linux),
            BUILD_PACKAGE_PROFILE_LINUX_FULL
        );
        assert_eq!(
            default_full_profile(BuildPackageTargetFamily::Host),
            BUILD_PACKAGE_PROFILE_HOST_FULL
        );
    }
}
