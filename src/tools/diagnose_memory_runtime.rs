use crate::diagnosis::build_memory_runtime_diagnosis_from_runtime;
use crate::tools::{
    serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct DiagnoseMemoryRuntimeTool {
    platform: Arc<dyn crate::Platform>,
    continuity_snapshot_supported: bool,
}

impl DiagnoseMemoryRuntimeTool {
    pub fn new(
        platform: Arc<dyn crate::Platform>,
        continuity_snapshot_supported: bool,
    ) -> Self {
        Self {
            platform,
            continuity_snapshot_supported,
        }
    }
}

impl Tool for DiagnoseMemoryRuntimeTool {
    fn name(&self) -> &'static str {
        "diagnose_memory_runtime"
    }

    fn description(&self) -> &'static str {
        "Diagnose Beetle memory runtime health, continuity governance, repair posture, and learning drift. Use this when the user asks why memory feels unstable, recall is weak, or learning/personality runtime looks off."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }

    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> crate::Result<String> {
        serialize_tool_output(
            "diagnose_memory_runtime",
            &build_memory_runtime_diagnosis_from_runtime(
                self.platform.as_ref(),
                self.continuity_snapshot_supported,
            )?,
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}
