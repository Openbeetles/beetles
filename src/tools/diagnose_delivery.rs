use crate::diagnosis::build_delivery_diagnosis_from_runtime;
use crate::tools::{
    serialize_tool_output, Tool, ToolContext, ToolEffectClass, ToolMetadata, ToolRiskLevel,
};

pub struct DiagnoseDeliveryTool {
    enabled_channel: String,
}

impl DiagnoseDeliveryTool {
    pub fn new(enabled_channel: String) -> Self {
        Self { enabled_channel }
    }
}

impl Tool for DiagnoseDeliveryTool {
    fn name(&self) -> &'static str {
        "diagnose_delivery"
    }

    fn description(&self) -> &'static str {
        "Diagnose recent message delivery failures, send instability, and reply handoff issues. Use this when the user asks why messages failed, were delayed, or channels look unhealthy."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{}}"#
    }

    fn execute(&self, _args: &str, _ctx: &mut dyn ToolContext) -> crate::Result<String> {
        serialize_tool_output(
            "diagnose_delivery",
            &build_delivery_diagnosis_from_runtime(Some(self.enabled_channel.as_str())),
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
    fn diagnose_delivery_tool_returns_structured_diagnosis_json() {
        let tool = DiagnoseDeliveryTool::new("qq_channel".to_string());
        let mut ctx = StubToolContext;

        let result = tool.execute("{}", &mut ctx).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["kind"], "delivery");
        assert!(parsed.get("summary").is_some());
        assert!(parsed.get("suspected_root_causes").is_some());
    }

    #[test]
    fn default_registry_registers_diagnose_delivery_tool() {
        let ctx = crate::platform::http_server::handlers::build_default_test_handler_context();
        assert!(ctx.tool_registry.get("diagnose_delivery").is_some());
    }
}
