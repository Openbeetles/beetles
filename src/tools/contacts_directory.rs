//! contacts_directory tool: local people directory for future mail/calendar composition.

use crate::contacts_directory::{
    ContactEntry, ContactsDirectoryService, ContactsDirectoryStore, ContactsDirectoryUpsertResult,
};
use crate::error::{Error, Result};
use crate::tools::{
    parse_tool_args, serialize_tool_output, Tool, ToolApprovalMode, ToolCapabilityContract,
    ToolContext, ToolEffectClass, ToolExecutionShape, ToolMetadata, ToolRiskLevel,
    ToolRollbackKind,
};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct ContactsDirectoryTool {
    service: ContactsDirectoryService,
}

#[derive(Serialize)]
struct ContactsDirectoryStatusResponse {
    op: &'static str,
    status: crate::contacts_directory::ContactsDirectoryStatus,
}

#[derive(Serialize)]
struct ContactsDirectoryListResponse {
    op: &'static str,
    count: usize,
    items: Vec<ContactEntry>,
}

#[derive(Serialize)]
struct ContactsDirectoryLookupResponse {
    op: &'static str,
    query: String,
    count: usize,
    items: Vec<crate::contacts_directory::ContactsDirectoryLookupHit>,
}

#[derive(Serialize)]
struct ContactsDirectoryUpsertResponse {
    op: &'static str,
    created: bool,
    contact: ContactEntry,
}

#[derive(Serialize)]
struct ContactsDirectoryDeleteResponse {
    op: &'static str,
    id: String,
    deleted: bool,
}

impl ContactsDirectoryTool {
    pub fn new(store: Arc<dyn ContactsDirectoryStore + Send + Sync>) -> Self {
        Self {
            service: ContactsDirectoryService::new(store),
        }
    }
}

impl Tool for ContactsDirectoryTool {
    fn name(&self) -> &'static str {
        "contacts_directory"
    }

    fn description(&self) -> &'static str {
        "Manage a local contacts directory the agent can use for people lookup and future mail/calendar composition. Ops: status, list, lookup, upsert, delete."
    }

    fn schema(&self) -> &str {
        r#"{"type":"object","properties":{"op":{"type":"string","description":"Operation: status|list|lookup|upsert|delete"},"id":{"type":"string","description":"Contact id for upsert or delete. Optional for create-style upsert."},"query":{"type":"string","description":"Lookup phrase, typically a name, alias, organization, or email."},"limit":{"type":"integer","description":"Maximum items to return for list or lookup. Default 10, max 50."},"display_name":{"type":"string","description":"Primary display name for upsert."},"emails":{"type":"array","items":{"type":"string"},"description":"Known email addresses for upsert."},"aliases":{"type":"array","items":{"type":"string"},"description":"Alternative names or nicknames for upsert."},"organization":{"type":"string","description":"Optional organization or team name for upsert."},"notes":{"type":"string","description":"Optional short notes for upsert."}},"required":["op"]}"#
    }

    fn execute(&self, args: &str, _ctx: &mut dyn ToolContext) -> Result<String> {
        let obj = parse_tool_args(args, "tool_contacts_directory")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::config("tool_contacts_directory", "missing op"))?;
        match op {
            "status" => serialize_tool_output(
                "tool_contacts_directory",
                &ContactsDirectoryStatusResponse {
                    op: "status",
                    status: self.service.status()?,
                },
            ),
            "list" => {
                let items = self.service.list(parse_limit(&obj))?;
                serialize_tool_output(
                    "tool_contacts_directory",
                    &ContactsDirectoryListResponse {
                        op: "list",
                        count: items.len(),
                        items,
                    },
                )
            }
            "lookup" => {
                let query = required_str(&obj, "query")?;
                let items = self.service.lookup(query, parse_limit(&obj))?;
                serialize_tool_output(
                    "tool_contacts_directory",
                    &ContactsDirectoryLookupResponse {
                        op: "lookup",
                        query: query.to_string(),
                        count: items.len(),
                        items,
                    },
                )
            }
            "upsert" => {
                let ContactsDirectoryUpsertResult { created, contact } =
                    self.service.upsert(ContactEntry {
                        id: optional_str(&obj, "id").unwrap_or_default(),
                        display_name: optional_str(&obj, "display_name").unwrap_or_default(),
                        emails: parse_string_array(&obj, "emails")?,
                        aliases: parse_string_array(&obj, "aliases")?,
                        organization: optional_str(&obj, "organization").unwrap_or_default(),
                        notes: optional_str(&obj, "notes").unwrap_or_default(),
                        updated_at_unix_secs: 0,
                    })?;
                serialize_tool_output(
                    "tool_contacts_directory",
                    &ContactsDirectoryUpsertResponse {
                        op: "upsert",
                        created,
                        contact,
                    },
                )
            }
            "delete" => {
                let id = required_str(&obj, "id")?;
                let deleted = self.service.delete(id)?;
                serialize_tool_output(
                    "tool_contacts_directory",
                    &ContactsDirectoryDeleteResponse {
                        op: "delete",
                        id: id.to_string(),
                        deleted,
                    },
                )
            }
            _ => Err(Error::config(
                "tool_contacts_directory",
                format!("unknown op '{op}'"),
            )),
        }
    }

    fn metadata(&self) -> ToolMetadata {
        ToolMetadata::stateful()
            .with_risk_level(ToolRiskLevel::Medium)
            .with_rollback_kind(ToolRollbackKind::CompensatingWrite)
    }

    fn execution_shape(&self, args: &str) -> Result<ToolExecutionShape> {
        let obj = parse_tool_args(args, "tool_contacts_directory_governance")?;
        let op = obj
            .get("op")
            .and_then(Value::as_str)
            .unwrap_or("status")
            .trim()
            .to_ascii_lowercase();
        Ok(match op.as_str() {
            "status" | "list" | "lookup" => self
                .metadata()
                .default_execution_shape(op.as_str())
                .with_effect_class(ToolEffectClass::ReadOnly)
                .with_risk_level(ToolRiskLevel::Low)
                .with_approval_mode(ToolApprovalMode::Automatic)
                .with_rollback_kind(ToolRollbackKind::None),
            "upsert" | "delete" => self
                .metadata()
                .default_execution_shape(op.as_str())
                .with_effect_class(ToolEffectClass::PersistentStateWrite)
                .with_approval_mode(ToolApprovalMode::ExplicitIntent)
                .with_approval_granted(true)
                .with_rollback_kind(ToolRollbackKind::CompensatingWrite),
            _ => self.metadata().default_execution_shape(op.as_str()),
        })
    }

    fn capability_contract(&self) -> ToolCapabilityContract {
        ToolCapabilityContract::required(&[
            crate::orchestrator::RUNTIME_CAPABILITY_STORAGE_STATE_FS,
        ])
    }
}

fn parse_limit(obj: &serde_json::Map<String, Value>) -> Option<usize> {
    obj.get("limit")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn required_str<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::config("tool_contacts_directory", format!("missing {key}")))
}

fn optional_str(obj: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_string_array(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Vec<String>> {
    let Some(value) = obj.get(key) else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| {
        Error::config(
            "tool_contacts_directory",
            format!("{key} must be an array of strings"),
        )
    })?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let text = item.as_str().ok_or_else(|| {
            Error::config(
                "tool_contacts_directory",
                format!("{key} must be an array of strings"),
            )
        })?;
        out.push(text.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::ContactsDirectoryTool;
    use crate::contacts_directory::StateFsContactsDirectoryStore;
    use crate::error::Result;
    use crate::i18n::Locale;
    use crate::platform::{ResponseBody, StateFs};
    use crate::tools::{Tool, ToolContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockStateFs {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl StateFs for MockStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.files.lock().unwrap().get(rel_path).cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files.lock().unwrap().remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, _rel_path: &str) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    struct StubToolContext;

    impl ToolContext for StubToolContext {
        fn get(&mut self, _url: &str) -> Result<(u16, ResponseBody)> {
            panic!("unexpected http get")
        }

        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, ResponseBody)> {
            panic!("unexpected http get_with_headers")
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, ResponseBody)> {
            panic!("unexpected http post_with_headers")
        }

        fn user_locale(&self) -> Locale {
            Locale::Zh
        }
    }

    #[test]
    fn contacts_directory_tool_upserts_and_looks_up_contact() {
        let state_fs = Arc::new(MockStateFs::default());
        let tool =
            ContactsDirectoryTool::new(Arc::new(StateFsContactsDirectoryStore::new(state_fs)));
        let mut ctx = StubToolContext;

        let upsert = tool
            .execute(
                r#"{"op":"upsert","display_name":"Alice Zhang","emails":["alice@example.com"],"aliases":["阿丽丝"]}"#,
                &mut ctx,
            )
            .expect("upsert contact");
        let upsert_json: Value = serde_json::from_str(&upsert).expect("parse upsert");
        assert_eq!(upsert_json["created"].as_bool(), Some(true));

        let lookup = tool
            .execute(r#"{"op":"lookup","query":"alice@example.com"}"#, &mut ctx)
            .expect("lookup contact");
        let lookup_json: Value = serde_json::from_str(&lookup).expect("parse lookup");
        assert_eq!(lookup_json["count"].as_u64(), Some(1));
        assert_eq!(
            lookup_json["items"][0]["contact"]["display_name"].as_str(),
            Some("Alice Zhang")
        );
    }
}
