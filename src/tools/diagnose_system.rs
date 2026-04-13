use crate::diagnosis::build_system_diagnosis_from_runtime;
use crate::tools::{
    serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};
use std::sync::Arc;

pub struct DiagnoseSystemTool {
    platform: Arc<dyn crate::Platform>,
    enabled_channel: String,
}

impl DiagnoseSystemTool {
    pub fn new(platform: Arc<dyn crate::Platform>, enabled_channel: String) -> Self {
        Self {
            platform,
            enabled_channel,
        }
    }
}

impl Tool for DiagnoseSystemTool {
    fn name(&self) -> &'static str {
        "diagnose_system"
    }

    fn description(&self) -> &'static str {
        "Diagnose overall Beetle system health, resource pressure, runtime mode, and capability degradation. Use this when the user asks why the system is slow, unstable, degraded, or generally unhealthy."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }

    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> crate::Result<String> {
        serialize_tool_output(
            "diagnose_system",
            &build_system_diagnosis_from_runtime(
                self.platform.as_ref(),
                Some(self.enabled_channel.as_str()),
            ),
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
            .with_effect_class(ToolEffectClass::HostInspection)
            .with_risk_level(ToolRiskLevel::Medium)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    struct StubToolContext;

    impl crate::tools::ToolContext for StubToolContext {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> crate::Result<(u16, crate::platform::ResponseBody)> {
            unreachable!()
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> crate::Result<(u16, crate::platform::ResponseBody)> {
            unreachable!()
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    #[test]
    fn diagnose_system_tool_returns_structured_diagnosis_json() {
        let platform: Arc<dyn crate::Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let tool = DiagnoseSystemTool::new(platform, "qq_channel".to_string());
        let mut ctx = StubToolContext;

        let result = tool.execute("{}", &mut ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["kind"], "system");
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("suspected_root_causes").is_some());
    }

    #[test]
    fn default_registry_registers_diagnose_system_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("diagnose_system").is_some());
    }
}
