use crate::error::{Error, Result};
use crate::office::{OfficeAuthoritySummary, OfficeCapability, OfficeService};
use crate::tools::{parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata};
use serde::Serialize;
use serde_json::Value;

pub struct OfficeStatusTool {
    office: OfficeService,
}

#[derive(Serialize)]
struct OfficeStatusResponse {
    op: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<OfficeCapability>,
    summary: OfficeAuthoritySummary,
}

impl OfficeStatusTool {
    pub fn new(office: OfficeService) -> Self {
        Self { office }
    }
}

impl Tool for OfficeStatusTool {
    fn name(&self) -> &'static str {
        "office_status"
    }

    fn description(&self) -> &'static str {
        "Inspect office account authority, credential presence, runtime probe state, and capability defaults."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"capability":{"type":"string","description":"Optional capability filter: mail|calendar|documents|contacts_directory"}}}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_office_status")?;
        let capability = obj
            .get("capability")
            .map(parse_capability)
            .transpose()?;
        let mut summary = self.office.summary()?;
        if let Some(capability) = capability {
            summary.defaults.retain(|item| item.capability == capability);
            summary.accounts.retain(|account| {
                account.enabled_capabilities.contains(&capability)
                    || account.selected_for_capabilities.contains(&capability)
            });
        }
        serialize_tool_output(
            "tool_office_status",
            &OfficeStatusResponse {
                op: "status",
                capability,
                summary,
            },
        )
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::task()
    }
}

fn parse_capability(value: &Value) -> Result<OfficeCapability> {
    let raw = value
        .as_str()
        .ok_or_else(|| Error::config("tool_office_status", "capability must be a string"))?;
    match raw {
        "mail" => Ok(OfficeCapability::Mail),
        "calendar" => Ok(OfficeCapability::Calendar),
        "documents" => Ok(OfficeCapability::Documents),
        "contacts_directory" => Ok(OfficeCapability::ContactsDirectory),
        _ => Err(Error::config(
            "tool_office_status",
            format!("unsupported capability '{}'", raw),
        )),
    }
}
