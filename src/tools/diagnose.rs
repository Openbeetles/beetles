use crate::diagnosis::{
    build_delivery_diagnosis_from_runtime, build_memory_runtime_diagnosis_from_runtime,
    build_network_path_diagnosis_from_runtime, build_system_diagnosis_from_runtime,
    build_voice_path_diagnosis_from_runtime,
};
use crate::error::{Error, Result};
use crate::i18n::locale_from_store;
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata,
    ToolRiskLevel,
};
use std::sync::Arc;

pub struct DiagnoseTool {
    platform: Arc<dyn crate::Platform>,
    config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
    enabled_channel: String,
    continuity_snapshot_supported: bool,
}

impl DiagnoseTool {
    pub fn new(
        platform: Arc<dyn crate::Platform>,
        config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
        enabled_channel: String,
        continuity_snapshot_supported: bool,
    ) -> Self {
        Self {
            platform,
            config_store,
            enabled_channel,
            continuity_snapshot_supported,
        }
    }
}

impl Tool for DiagnoseTool {
    fn name(&self) -> &'static str {
        "diagnose"
    }

    fn description(&self) -> &'static str {
        "Diagnose Beetle runtime health across one specific plane. `op` selects the plane: `system`, `network`, `memory`, `delivery`, or `voice`. Use this when the user asks why the system is unhealthy, network paths fail, memory/runtime behavior drifts, delivery is unstable, or voice feels broken."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Diagnosis plane: system | network | memory | delivery | voice"}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "diagnose")?;
        let op = obj
            .get("op")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::config("diagnose", "missing op"))?;

        match op {
            "system" => serialize_tool_output(
                "diagnose",
                &build_system_diagnosis_from_runtime(
                    self.platform.as_ref(),
                    Some(self.enabled_channel.as_str()),
                ),
            ),
            "network" => {
                let config = crate::config::AppConfig::load(
                    self.config_store.as_ref(),
                    Some(&crate::config::PlatformConfigFileStore(Arc::clone(
                        &self.platform,
                    ))),
                );
                serialize_tool_output(
                    "diagnose",
                    &build_network_path_diagnosis_from_runtime(
                        self.platform.as_ref(),
                        &config,
                        locale_from_store(self.config_store.as_ref()),
                    ),
                )
            }
            "memory" => serialize_tool_output(
                "diagnose",
                &build_memory_runtime_diagnosis_from_runtime(
                    self.platform.as_ref(),
                    self.continuity_snapshot_supported,
                )?,
            ),
            "delivery" => serialize_tool_output(
                "diagnose",
                &build_delivery_diagnosis_from_runtime(Some(self.enabled_channel.as_str())),
            ),
            "voice" => {
                let config = crate::config::AppConfig::load(
                    self.config_store.as_ref(),
                    Some(&crate::config::PlatformConfigFileStore(Arc::clone(
                        &self.platform,
                    ))),
                );
                serialize_tool_output(
                    "diagnose",
                    &build_voice_path_diagnosis_from_runtime(self.platform.as_ref(), &config),
                )
            }
            other => Err(Error::config(
                "diagnose",
                format!("unsupported op '{other}'"),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}
