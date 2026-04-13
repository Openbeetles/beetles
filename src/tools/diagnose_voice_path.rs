use crate::diagnosis::build_voice_path_diagnosis_from_runtime;
use crate::tools::{
    serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct DiagnoseVoicePathTool {
    platform: Arc<dyn crate::Platform>,
    config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
}

impl DiagnoseVoicePathTool {
    pub fn new(
        platform: Arc<dyn crate::Platform>,
        config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
    ) -> Self {
        Self {
            platform,
            config_store,
        }
    }
}

impl Tool for DiagnoseVoicePathTool {
    fn name(&self) -> &'static str {
        "diagnose_voice_path"
    }

    fn description(&self) -> &'static str {
        "Diagnose Beetle voice-path health across audio configuration, duplex contract, realtime eligibility, capture/playback runtime state, and recent voice metrics. Use this when the user asks why wake word, voice input, or voice output feels broken or unstable."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }

    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> crate::Result<String> {
        let config = crate::config::AppConfig::load(
            self.config_store.as_ref(),
            Some(&crate::config::PlatformConfigFileStore(Arc::clone(
                &self.platform,
            ))),
        );
        serialize_tool_output(
            "diagnose_voice_path",
            &build_voice_path_diagnosis_from_runtime(self.platform.as_ref(), &config),
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}
