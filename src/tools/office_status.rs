use crate::error::{Error, Result};
use crate::office::{
    OfficeAuthoritySource, OfficeAuthoritySummary, OfficeCapability, OfficeService,
    SnapshotOfficeAuthoritySource,
};
use crate::tools::{parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub struct OfficeStatusTool {
    authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
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
        Self::with_authority(Arc::new(SnapshotOfficeAuthoritySource::new(office)))
    }

    pub fn with_authority(authority: Arc<dyn OfficeAuthoritySource + Send + Sync>) -> Self {
        Self { authority }
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
        let capability = obj.get("capability").map(parse_capability).transpose()?;
        let mut summary = self.authority.load()?.summary()?;
        if let Some(capability) = capability {
            summary
                .defaults
                .retain(|item| item.capability == capability);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{save_office_accounts_segment, ConfigFileStore};
    use crate::office::{
        OfficeCredential, OfficeCredentialStore, OfficeRuntimeStatusStore,
        ReloadingOfficeAuthoritySource,
    };
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MemoryConfigFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl ConfigFileStore for MemoryConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubOfficeCredentialStore;

    impl OfficeCredentialStore for StubOfficeCredentialStore {
        fn get(&self, _account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(Vec::new())
        }

        fn set(&self, _credential: &OfficeCredential) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubRuntimeStatusStore;

    impl OfficeRuntimeStatusStore for StubRuntimeStatusStore {
        fn get(
            &self,
            _account_key: &str,
        ) -> Result<Option<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(None)
        }

        fn list(&self) -> Result<Vec<crate::office::OfficeAccountRuntimeStatus>> {
            Ok(Vec::new())
        }

        fn set(&self, _status: &crate::office::OfficeAccountRuntimeStatus) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _account_key: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct DummyCtx;

    impl ToolContext for DummyCtx {
        fn get_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("tool_office_status_test", "network unused"))
        }

        fn post_with_headers(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Err(Error::config("tool_office_status_test", "network unused"))
        }

        fn user_locale(&self) -> crate::i18n::Locale {
            crate::i18n::Locale::Zh
        }
    }

    fn save_mail_accounts(
        config_file_store: &dyn ConfigFileStore,
        default_account_key: &str,
    ) -> Result<()> {
        save_office_accounts_segment(
            config_file_store,
            &format!(
                r#"{{
                    "registry": {{
                        "accounts": {{
                            "mail-work": {{
                                "account_key": "mail-work",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "work@example.com",
                                "account_label": "Work",
                                "identity_class": "work",
                                "enabled_capabilities": ["mail"]
                            }},
                            "mail-personal": {{
                                "account_key": "mail-personal",
                                "provider_kind": "imap_smtp",
                                "external_account_id": "personal@example.com",
                                "account_label": "Personal",
                                "identity_class": "personal",
                                "enabled_capabilities": ["mail"]
                            }}
                        }}
                    }},
                    "binding": {{
                        "capability_defaults": {{
                            "mail": "{default_account_key}"
                        }}
                    }},
                    "policy": {{}}
                }}"#
            ),
        )
    }

    #[test]
    fn office_status_tool_reloads_accounts_after_commit() {
        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        save_mail_accounts(config_file_store.as_ref(), "mail-work").expect("seed accounts");
        let tool = OfficeStatusTool::with_authority(Arc::new(ReloadingOfficeAuthoritySource::new(
            config_file_store.clone(),
            Arc::new(StubOfficeCredentialStore),
            Arc::new(StubRuntimeStatusStore),
        )));
        let mut ctx = DummyCtx;

        let first = tool.execute("{}", &mut ctx).expect("first status");
        let first: Value = serde_json::from_str(&first).expect("valid first status");
        assert_eq!(first["summary"]["defaults"][0]["account_key"], "mail-work");

        save_mail_accounts(config_file_store.as_ref(), "mail-personal").expect("update accounts");

        let second = tool.execute("{}", &mut ctx).expect("second status");
        let second: Value = serde_json::from_str(&second).expect("valid second status");
        assert_eq!(
            second["summary"]["defaults"][0]["account_key"],
            "mail-personal"
        );
    }
}
