use crate::error::{Error, Result};
use crate::office::{
    OfficeAccountAssessment, OfficeAuthoritySource, OfficeAuthoritySummary, OfficeCapability,
    OfficeProbeAdapter, OfficeService, SnapshotOfficeAuthoritySource,
};
use crate::tools::{
    office_diagnostics::{build_account_diagnostics, OfficeAccountDiagnostic},
    parse_tool_args, serialize_tool_output, Tool, ToolContext, ToolMetadata,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct OfficeStatusTool {
    authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
    probe_supported_provider_kinds: BTreeSet<String>,
}

#[derive(Serialize)]
struct OfficeStatusResponse {
    op: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    capability: Option<OfficeCapability>,
    summary: OfficeAuthoritySummary,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_assessments: Vec<OfficeAccountAssessment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    account_diagnostics: Vec<OfficeAccountDiagnostic>,
}

impl OfficeStatusTool {
    pub fn new(office: OfficeService) -> Self {
        Self::with_authority(Arc::new(SnapshotOfficeAuthoritySource::new(office)))
    }

    pub fn with_authority(authority: Arc<dyn OfficeAuthoritySource + Send + Sync>) -> Self {
        Self::with_probe_supported_provider_kinds(authority, Vec::<String>::new())
    }

    pub fn with_probe_supported_provider_kinds(
        authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
        provider_kinds: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            authority,
            probe_supported_provider_kinds: provider_kinds.into_iter().collect(),
        }
    }

    pub fn with_probe_adapters(
        authority: Arc<dyn OfficeAuthoritySource + Send + Sync>,
        probe_adapters: impl IntoIterator<Item = Arc<dyn OfficeProbeAdapter + Send + Sync>>,
    ) -> Self {
        Self::with_probe_supported_provider_kinds(
            authority,
            probe_adapters
                .into_iter()
                .map(|adapter| adapter.provider_kind().to_string()),
        )
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
        let service = self.authority.load()?;
        let mut summary = service.summary()?;
        let account_assessments = if let Some(capability) = capability {
            service.assess_capability_accounts(capability, |provider_kind| {
                self.probe_supported_provider_kinds.contains(provider_kind)
            })?
        } else {
            service.assess_all_accounts(|provider_kind| {
                self.probe_supported_provider_kinds.contains(provider_kind)
            })?
        };
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
                account_diagnostics: build_account_diagnostics(&account_assessments),
                account_assessments,
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
    use crate::documents::OFFICE_METADATA_DOCUMENTS_BASE_URL;
    use crate::mail::{OFFICE_METADATA_MAIL_IMAP_HOST, OFFICE_METADATA_MAIL_SMTP_HOST};
    use crate::office::{
        OfficeAccountRuntimeStatus, OfficeCredential, OfficeCredentialStore,
        OfficeRuntimeStatusStore, ReloadingOfficeAuthoritySource,
    };
    use std::collections::{BTreeMap, HashMap};
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
    struct MemoryOfficeCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for MemoryOfficeCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(account_key);
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
    struct MemoryRuntimeStatusStore {
        items: Mutex<BTreeMap<String, OfficeAccountRuntimeStatus>>,
    }

    impl OfficeRuntimeStatusStore for MemoryRuntimeStatusStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(status.account_key.clone(), status.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(account_key);
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

    fn save_documents_accounts(
        config_file_store: &dyn ConfigFileStore,
        default_account_key: &str,
    ) -> Result<()> {
        save_office_accounts_segment(
            config_file_store,
            &format!(
                r#"{{
                    "registry": {{
                        "accounts": {{
                            "docs-work": {{
                                "account_key": "docs-work",
                                "provider_kind": "webdav",
                                "external_account_id": "work@example.com",
                                "account_label": "Work Docs",
                                "identity_class": "work",
                                "enabled_capabilities": ["documents"]
                            }}
                        }}
                    }},
                    "binding": {{
                        "capability_defaults": {{
                            "documents": "{default_account_key}"
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

    #[test]
    fn office_status_tool_reports_account_assessments() {
        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        save_mail_accounts(config_file_store.as_ref(), "mail-work").expect("seed accounts");
        let tool = OfficeStatusTool::with_authority(Arc::new(ReloadingOfficeAuthoritySource::new(
            config_file_store,
            Arc::new(StubOfficeCredentialStore),
            Arc::new(StubRuntimeStatusStore),
        )));
        let mut ctx = DummyCtx;

        let payload = tool.execute("{}", &mut ctx).expect("office status");
        let payload: Value = serde_json::from_str(&payload).expect("valid office status");
        assert_eq!(
            payload["account_assessments"][0]["account_key"],
            "mail-personal"
        );
        assert_eq!(
            payload["account_assessments"][0]["readiness"],
            "needs_credential_input"
        );
        assert_eq!(
            payload["account_assessments"][0]["next_action"],
            "draft_credentials"
        );
        assert!(payload["account_assessments"][0]["missing_fields"]
            .as_array()
            .expect("missing fields array")
            .iter()
            .any(|item| item == "access_token"));
        assert!(payload["account_assessments"][0]["missing_field_details"]
            .as_array()
            .expect("missing field details array")
            .iter()
            .any(|item| {
                item["key"] == "access_token"
                    && item["secret"] == true
                    && item["label"] == "Access token / app secret"
            }));
        assert_eq!(
            payload["account_diagnostics"][0]["diagnosis_kind"],
            "needs_credential_input"
        );
        assert!(payload["account_diagnostics"][0]["summary"]
            .as_str()
            .expect("diagnostic summary")
            .contains("access_token"));
    }

    #[test]
    fn office_status_tool_reports_runtime_failure_diagnostics() {
        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        let credential_store = Arc::new(MemoryOfficeCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        save_documents_accounts(config_file_store.as_ref(), "docs-work")
            .expect("seed document accounts");
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 0,
                metadata: vec![(
                    OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                    "https://dav.example.com/root".to_string(),
                )]
                .into_iter()
                .collect(),
            })
            .expect("seed office credential");
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "docs-work".to_string(),
                probe_ok: false,
                last_error: "remote read failed: 403 forbidden".to_string(),
                last_probe_at_unix_secs: 0,
                last_activity_kind: "documents_read".to_string(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 1_710_000_123,
                updated_at: 1_710_000_123,
            })
            .expect("seed runtime status");
        let tool = OfficeStatusTool::with_probe_supported_provider_kinds(
            Arc::new(ReloadingOfficeAuthoritySource::new(
                config_file_store,
                credential_store,
                runtime_status_store,
            )),
            vec!["webdav".to_string()],
        );
        let mut ctx = DummyCtx;

        let payload = tool
            .execute(r#"{"capability":"documents"}"#, &mut ctx)
            .expect("office status");
        let payload: Value = serde_json::from_str(&payload).expect("valid office status");
        assert_eq!(
            payload["account_diagnostics"][0]["account_key"],
            "docs-work"
        );
        assert_eq!(
            payload["account_diagnostics"][0]["diagnosis_kind"],
            "runtime_failure"
        );
        assert_eq!(
            payload["account_diagnostics"][0]["recommended_action"],
            "review_runtime_error"
        );
        assert_eq!(
            payload["account_diagnostics"][0]["last_activity_kind"],
            "documents_read"
        );
        assert!(payload["account_diagnostics"][0]["summary"]
            .as_str()
            .expect("diagnostic summary")
            .contains("403 forbidden"));
        assert_eq!(
            payload["account_assessments"][0]["readiness"],
            "ready_for_probe"
        );
    }

    #[test]
    fn office_status_tool_reports_ready_diagnostics_for_healthy_account() {
        let config_file_store = Arc::new(MemoryConfigFileStore::default());
        let credential_store = Arc::new(MemoryOfficeCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        save_mail_accounts(config_file_store.as_ref(), "mail-work").expect("seed accounts");
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 0,
                metadata: vec![
                    (
                        OFFICE_METADATA_MAIL_IMAP_HOST.to_string(),
                        "imap.example.com".to_string(),
                    ),
                    (
                        OFFICE_METADATA_MAIL_SMTP_HOST.to_string(),
                        "smtp.example.com".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            })
            .expect("seed mail credential");
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "mail-work".to_string(),
                probe_ok: true,
                last_error: String::new(),
                last_probe_at_unix_secs: 1_710_000_999,
                last_activity_kind: "mail_list".to_string(),
                last_activity_ok: true,
                last_activity_at_unix_secs: 1_710_001_000,
                updated_at: 1_710_001_000,
            })
            .expect("seed runtime status");
        let tool = OfficeStatusTool::with_probe_supported_provider_kinds(
            Arc::new(ReloadingOfficeAuthoritySource::new(
                config_file_store,
                credential_store,
                runtime_status_store,
            )),
            vec!["imap_smtp".to_string()],
        );
        let mut ctx = DummyCtx;

        let payload = tool
            .execute(r#"{"capability":"mail"}"#, &mut ctx)
            .expect("office status");
        let payload: Value = serde_json::from_str(&payload).expect("valid office status");
        let diagnosis = payload["account_diagnostics"]
            .as_array()
            .expect("diagnostics array")
            .iter()
            .find(|item| item["account_key"] == "mail-work")
            .expect("mail-work diagnosis");
        assert_eq!(diagnosis["diagnosis_kind"], "ready");
        assert_eq!(diagnosis["recommended_action"], "none");
    }
}
