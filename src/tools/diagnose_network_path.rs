use crate::diagnosis::build_network_path_diagnosis_from_runtime;
use crate::i18n::locale_from_store;
use crate::tools::{
    serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct DiagnoseNetworkPathTool {
    platform: Arc<dyn crate::Platform>,
    config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
}

impl DiagnoseNetworkPathTool {
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

impl Tool for DiagnoseNetworkPathTool {
    fn name(&self) -> &'static str {
        "diagnose_network_path"
    }

    fn description(&self) -> &'static str {
        "Diagnose Beetle network-path health across WiFi, DNS, route, active channel connectivity, proxy path, and outbound TLS/runtime readiness. Use this when the user asks why network requests or message delivery cannot reach the upstream path."
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
            "diagnose_network_path",
            &build_network_path_diagnosis_from_runtime(
                self.platform.as_ref(),
                &config,
                locale_from_store(self.config_store.as_ref()),
            ),
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}
