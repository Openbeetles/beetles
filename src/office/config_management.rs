use crate::config::{
    self, validate_office_accounts_candidate, validate_office_credentials_candidate,
    ConfigFileStore, OfficeAccountsSegment,
};
use crate::documents::{OFFICE_METADATA_DOCUMENTS_BASE_URL, OFFICE_METADATA_DOCUMENTS_USERNAME};
use crate::error::{Error, Result};
use crate::mail::{
    OFFICE_METADATA_MAIL_IMAP_HOST, OFFICE_METADATA_MAIL_SMTP_HOST, OFFICE_METADATA_MAIL_USERNAME,
};
use crate::office::{
    OfficeAccount, OfficeAccountIdentityClass, OfficeAuthoritySummary, OfficeCapability,
    OfficeCredential, OfficeCredentialStore, OfficeCredentialsSegment, OfficeResolveRequest,
    OfficeResolveResult, OfficeRuntimeStatusStore, OfficeSelectionPolicy, OfficeService,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const OFFICE_METADATA_CALENDAR_USERNAME_FIELD: &str = "calendar_username";
const OFFICE_METADATA_CALENDAR_BASE_URL_FIELD: &str = "calendar_base_url";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigSnapshot {
    pub accounts: OfficeAccountsSegment,
    pub credentials: OfficeCredentialsSegment,
    pub summary: OfficeAuthoritySummary,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigReadiness {
    NeedsCredentialInput,
    ReadyForProbe,
    ProbeUnavailable,
    Ready,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigNextAction {
    DraftCredentials,
    Probe,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountAssessment {
    pub account_key: String,
    pub provider_kind: String,
    pub enabled_capabilities: Vec<OfficeCapability>,
    pub credential_present: bool,
    pub credential_configured: bool,
    pub probe_supported: bool,
    #[serde(default)]
    pub missing_fields: Vec<String>,
    pub readiness: OfficeConfigReadiness,
    pub next_action: OfficeConfigNextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_status: Option<crate::office::OfficeAccountRuntimeStatus>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigAssessment {
    #[serde(default)]
    pub accounts: Vec<OfficeAccountAssessment>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficePolicyPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_default_account_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ask_when_ambiguous: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_identity_class: Option<OfficeAccountIdentityClass>,
    #[serde(default)]
    pub clear_preferred_identity_class: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountDraftRequest {
    pub account: OfficeAccount,
    #[serde(default)]
    pub set_defaults: Vec<OfficeCapability>,
    #[serde(default)]
    pub clear_defaults: Vec<OfficeCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_patch: Option<OfficePolicyPatch>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCredentialDraftRequest {
    pub credential: OfficeCredential,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeProbeDisposition {
    Ready,
    MissingCredential,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeProbeResult {
    pub account_key: String,
    pub provider_kind: String,
    pub configured: bool,
    pub disposition: OfficeProbeDisposition,
    pub reason: String,
}

pub trait OfficeProbeAdapter: Send + Sync {
    fn provider_kind(&self) -> &'static str;
    fn probe(
        &self,
        account: &OfficeAccount,
        credential: &OfficeCredential,
    ) -> Result<OfficeProbeResult>;
}

#[derive(Clone)]
pub struct OfficeConfigManagementService {
    config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
    credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
    runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
    probe_adapters: Vec<Arc<dyn OfficeProbeAdapter + Send + Sync>>,
}

impl OfficeConfigManagementService {
    pub fn new(
        config_file_store: Arc<dyn ConfigFileStore + Send + Sync>,
        credential_store: Arc<dyn OfficeCredentialStore + Send + Sync>,
        runtime_status_store: Arc<dyn OfficeRuntimeStatusStore + Send + Sync>,
    ) -> Self {
        Self {
            config_file_store,
            credential_store,
            runtime_status_store,
            probe_adapters: Vec::new(),
        }
    }

    pub fn with_probe_adapters(
        mut self,
        probe_adapters: Vec<Arc<dyn OfficeProbeAdapter + Send + Sync>>,
    ) -> Self {
        self.probe_adapters = probe_adapters;
        self
    }

    pub fn inspect(&self) -> Result<OfficeConfigSnapshot> {
        let accounts = self.load_accounts_segment()?;
        let credentials = self.load_credentials_segment()?;
        let summary = self.build_office_service(&accounts)?.summary()?;
        Ok(OfficeConfigSnapshot {
            accounts,
            credentials,
            summary,
        })
    }

    pub fn resolve_account(&self, request: &OfficeResolveRequest) -> Result<OfficeResolveResult> {
        let accounts = self.load_accounts_segment()?;
        Ok(self.build_office_service(&accounts)?.resolve(request))
    }

    pub fn assess(&self, account_key: Option<&str>) -> Result<OfficeConfigAssessment> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        let mut items = if let Some(account_key) = account_key {
            vec![self.build_account_assessment(&office, account_key)?]
        } else {
            let mut items = office
                .summary()?
                .accounts
                .into_iter()
                .map(|status| self.build_account_assessment(&office, &status.account_key))
                .collect::<Result<Vec<_>>>()?;
            items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
            items
        };
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(OfficeConfigAssessment { accounts: items })
    }

    pub fn assess_account(&self, account_key: &str) -> Result<OfficeAccountAssessment> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        self.build_account_assessment(&office, account_key)
    }

    pub fn draft_accounts(
        &self,
        request: &OfficeAccountDraftRequest,
    ) -> Result<OfficeAccountsSegment> {
        let mut segment = self.load_accounts_segment()?;
        segment.registry.insert(request.account.clone());
        for capability in &request.set_defaults {
            segment
                .binding
                .set_default_account(*capability, request.account.account_key.clone());
        }
        for capability in &request.clear_defaults {
            if segment.binding.default_account_for(*capability)
                == Some(request.account.account_key.as_str())
            {
                remove_default_binding(&mut segment, *capability)?;
            }
        }
        if let Some(patch) = request.policy_patch.as_ref() {
            apply_policy_patch(&mut segment.policy, patch);
        }
        self.validate_accounts(&segment)?;
        Ok(segment)
    }

    pub fn draft_credentials(
        &self,
        request: &OfficeCredentialDraftRequest,
    ) -> Result<OfficeCredentialsSegment> {
        let mut segment = self.load_credentials_segment()?;
        segment
            .items
            .retain(|item| item.account_key != request.credential.account_key);
        segment.items.push(request.credential.clone());
        segment
            .items
            .sort_by(|left, right| left.account_key.cmp(&right.account_key));
        self.validate_credentials(&segment)?;
        Ok(segment)
    }

    pub fn validate_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        validate_office_accounts_candidate(segment)
    }

    pub fn validate_credentials(&self, segment: &OfficeCredentialsSegment) -> Result<()> {
        validate_office_credentials_candidate(segment)
    }

    pub fn commit_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        self.validate_accounts(segment)?;
        let body = serde_json::to_string(segment)
            .map_err(|error| Error::config("office_config_commit_accounts", error.to_string()))?;
        config::save_office_accounts_segment(self.config_file_store.as_ref(), &body)
    }

    pub fn commit_credentials(&self, segment: &OfficeCredentialsSegment) -> Result<()> {
        self.validate_credentials(segment)?;
        let body = serde_json::to_string(segment).map_err(|error| {
            Error::config("office_config_commit_credentials", error.to_string())
        })?;
        config::save_office_credentials_segment(self.credential_store.as_ref(), &body)
    }

    pub fn revoke(&self, account_key: &str, clear_runtime_status: bool) -> Result<()> {
        self.credential_store.clear(account_key)?;
        if clear_runtime_status {
            self.runtime_status_store.clear(account_key)?;
        }
        Ok(())
    }

    pub fn probe(&self, account_key: &str) -> Result<OfficeProbeResult> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        let account = office.account(account_key).ok_or_else(|| {
            Error::config(
                "office_config_probe",
                format!("unknown office account '{}'", account_key),
            )
        })?;
        let Some(credential) = office.credential(account_key)? else {
            return Ok(OfficeProbeResult {
                account_key: account.account_key,
                provider_kind: account.provider_kind,
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "credential_missing".to_string(),
            });
        };
        if let Some(adapter) = self
            .probe_adapters
            .iter()
            .find(|adapter| adapter.provider_kind() == account.provider_kind)
        {
            return adapter.probe(&account, &credential);
        }
        Ok(OfficeProbeResult {
            account_key: account.account_key,
            provider_kind: account.provider_kind,
            configured: !credential.access_token.trim().is_empty(),
            disposition: OfficeProbeDisposition::Unsupported,
            reason: "probe_adapter_unavailable".to_string(),
        })
    }

    fn build_office_service(&self, accounts: &OfficeAccountsSegment) -> Result<OfficeService> {
        Ok(OfficeService::new(
            accounts.registry.clone(),
            accounts.binding.clone(),
            accounts.policy.clone(),
            Arc::clone(&self.credential_store),
            Arc::clone(&self.runtime_status_store),
        ))
    }

    fn load_accounts_segment(&self) -> Result<OfficeAccountsSegment> {
        let json = config::get_office_accounts_segment(self.config_file_store.as_ref())?;
        serde_json::from_str(&json)
            .map_err(|error| Error::config("office_config_load_accounts", error.to_string()))
    }

    fn load_credentials_segment(&self) -> Result<OfficeCredentialsSegment> {
        let json = config::get_office_credentials_segment(self.credential_store.as_ref())?;
        serde_json::from_str(&json)
            .map_err(|error| Error::config("office_config_load_credentials", error.to_string()))
    }

    fn build_account_assessment(
        &self,
        office: &OfficeService,
        account_key: &str,
    ) -> Result<OfficeAccountAssessment> {
        let account = office.account(account_key).ok_or_else(|| {
            Error::config(
                "office_config_assess",
                format!("unknown office account '{}'", account_key),
            )
        })?;
        let credential = office.credential(account_key)?;
        let missing_fields = collect_missing_fields(&account, credential.as_ref());
        let runtime_status = office.runtime_status(account_key)?;
        let probe_supported = self
            .probe_adapters
            .iter()
            .any(|adapter| adapter.provider_kind() == account.provider_kind);
        let readiness = if missing_fields.is_empty() {
            if runtime_status
                .as_ref()
                .is_some_and(|status| status.probe_ok)
            {
                OfficeConfigReadiness::Ready
            } else if probe_supported {
                OfficeConfigReadiness::ReadyForProbe
            } else {
                OfficeConfigReadiness::ProbeUnavailable
            }
        } else {
            OfficeConfigReadiness::NeedsCredentialInput
        };
        let next_action = match readiness {
            OfficeConfigReadiness::NeedsCredentialInput => OfficeConfigNextAction::DraftCredentials,
            OfficeConfigReadiness::ReadyForProbe => OfficeConfigNextAction::Probe,
            OfficeConfigReadiness::ProbeUnavailable | OfficeConfigReadiness::Ready => {
                OfficeConfigNextAction::None
            }
        };
        Ok(OfficeAccountAssessment {
            account_key: account.account_key,
            provider_kind: account.provider_kind,
            enabled_capabilities: account.enabled_capabilities,
            credential_present: credential.is_some(),
            credential_configured: missing_fields.is_empty(),
            probe_supported,
            missing_fields,
            readiness,
            next_action,
            runtime_status,
        })
    }
}

fn collect_missing_fields(
    account: &OfficeAccount,
    credential: Option<&OfficeCredential>,
) -> Vec<String> {
    let mut missing = std::collections::BTreeSet::new();
    let access_token = credential
        .map(|item| item.access_token.trim())
        .unwrap_or_default();
    let external_account_id = account.external_account_id.trim();
    let metadata_value = |key: &str| {
        credential
            .and_then(|item| item.metadata_value(key))
            .map(str::trim)
            .unwrap_or_default()
    };

    match account.provider_kind.as_str() {
        "imap_smtp" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_MAIL_USERNAME).is_empty()
            {
                missing.insert("mail_username".to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_IMAP_HOST,
                metadata_value(OFFICE_METADATA_MAIL_IMAP_HOST),
            );
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_MAIL_SMTP_HOST,
                metadata_value(OFFICE_METADATA_MAIL_SMTP_HOST),
            );
        }
        "webdav" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_DOCUMENTS_USERNAME).is_empty()
            {
                missing.insert(OFFICE_METADATA_DOCUMENTS_USERNAME.to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_DOCUMENTS_BASE_URL,
                metadata_value(OFFICE_METADATA_DOCUMENTS_BASE_URL),
            );
        }
        "caldav" => {
            push_missing_if_blank(&mut missing, "access_token", access_token);
            if external_account_id.is_empty()
                && metadata_value(OFFICE_METADATA_CALENDAR_USERNAME_FIELD).is_empty()
            {
                missing.insert(OFFICE_METADATA_CALENDAR_USERNAME_FIELD.to_string());
            }
            push_missing_if_blank(
                &mut missing,
                OFFICE_METADATA_CALENDAR_BASE_URL_FIELD,
                metadata_value(OFFICE_METADATA_CALENDAR_BASE_URL_FIELD),
            );
            push_missing_if_blank(
                &mut missing,
                crate::office::OFFICE_METADATA_CALENDAR_ID,
                metadata_value(crate::office::OFFICE_METADATA_CALENDAR_ID),
            );
        }
        _ => {
            if account
                .enabled_capabilities
                .iter()
                .any(|capability| *capability != OfficeCapability::ContactsDirectory)
            {
                push_missing_if_blank(&mut missing, "access_token", access_token);
            }
        }
    }

    missing.into_iter().collect()
}

fn push_missing_if_blank(
    missing: &mut std::collections::BTreeSet<String>,
    field: &str,
    value: &str,
) {
    if value.trim().is_empty() {
        missing.insert(field.to_string());
    }
}

fn apply_policy_patch(policy: &mut OfficeSelectionPolicy, patch: &OfficePolicyPatch) {
    if let Some(global_default_account_key) = patch.global_default_account_key.as_ref() {
        policy.global_default_account_key = global_default_account_key.clone();
    }
    if let Some(ask_when_ambiguous) = patch.ask_when_ambiguous {
        policy.ask_when_ambiguous = ask_when_ambiguous;
    }
    if patch.clear_preferred_identity_class {
        policy.preferred_identity_class = None;
    } else if let Some(preferred_identity_class) = patch.preferred_identity_class {
        policy.preferred_identity_class = Some(preferred_identity_class);
    }
}

fn remove_default_binding(
    segment: &mut OfficeAccountsSegment,
    capability: OfficeCapability,
) -> Result<()> {
    let mut defaults = serde_json::to_value(&segment.binding)
        .map_err(|error| Error::config("office_config_remove_default", error.to_string()))?;
    let Some(capability_defaults) = defaults
        .get_mut("capability_defaults")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return Ok(());
    };
    capability_defaults.remove(capability_key(capability));
    segment.binding = serde_json::from_value(defaults)
        .map_err(|error| Error::config("office_config_remove_default", error.to_string()))?;
    Ok(())
}

fn capability_key(capability: OfficeCapability) -> &'static str {
    match capability {
        OfficeCapability::Mail => "mail",
        OfficeCapability::Calendar => "calendar",
        OfficeCapability::Documents => "documents",
        OfficeCapability::ContactsDirectory => "contacts_directory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::OfficeAccountRuntimeStatus;
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    struct MemoryConfigFileStore {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl MemoryConfigFileStore {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }
    }

    impl ConfigFileStore for MemoryConfigFileStore {
        fn read_config_file(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write_config_file(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove_config_file(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(rel_path);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryCredentialStore {
        items: Mutex<BTreeMap<String, OfficeCredential>>,
    }

    impl OfficeCredentialStore for MemoryCredentialStore {
        fn get(&self, account_key: &str) -> Result<Option<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeCredential>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, credential: &OfficeCredential) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(credential.account_key.clone(), credential.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
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
                .unwrap_or_else(|e| e.into_inner())
                .get(account_key)
                .cloned())
        }

        fn list(&self) -> Result<Vec<OfficeAccountRuntimeStatus>> {
            Ok(self
                .items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .cloned()
                .collect())
        }

        fn set(&self, status: &OfficeAccountRuntimeStatus) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(status.account_key.clone(), status.clone());
            Ok(())
        }

        fn clear(&self, account_key: &str) -> Result<()> {
            self.items
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(account_key);
            Ok(())
        }
    }

    fn account(account_key: &str, capability: OfficeCapability) -> OfficeAccount {
        OfficeAccount {
            account_key: account_key.to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: format!("{account_key}@example.com"),
            account_label: account_key.to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![capability],
        }
    }

    #[test]
    fn draft_accounts_upserts_account_and_sets_default_binding() {
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let draft = service
            .draft_accounts(&OfficeAccountDraftRequest {
                account: account("mail-work", OfficeCapability::Mail),
                set_defaults: vec![OfficeCapability::Mail],
                clear_defaults: Vec::new(),
                policy_patch: None,
            })
            .expect("draft accounts");

        assert!(draft.registry.get("mail-work").is_some());
        assert_eq!(
            draft.binding.default_account_for(OfficeCapability::Mail),
            Some("mail-work")
        );
    }

    #[test]
    fn commit_accounts_refreshes_inspect_snapshot() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        let service = OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );
        let draft = service
            .draft_accounts(&OfficeAccountDraftRequest {
                account: account("calendar-work", OfficeCapability::Calendar),
                set_defaults: vec![OfficeCapability::Calendar],
                clear_defaults: Vec::new(),
                policy_patch: Some(OfficePolicyPatch {
                    global_default_account_key: Some("calendar-work".to_string()),
                    ask_when_ambiguous: Some(true),
                    preferred_identity_class: Some(OfficeAccountIdentityClass::Work),
                    clear_preferred_identity_class: false,
                }),
            })
            .expect("draft accounts");

        service.commit_accounts(&draft).expect("commit accounts");
        let snapshot = service.inspect().expect("inspect");
        assert!(snapshot.accounts.registry.get("calendar-work").is_some());
        assert_eq!(
            snapshot.summary.policy.global_default_account_key,
            "calendar-work"
        );
    }

    #[test]
    fn revoke_clears_credential_and_runtime_status() {
        let credential_store = Arc::new(MemoryCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect("seed credential");
        runtime_status_store
            .set(&OfficeAccountRuntimeStatus {
                account_key: "mail-work".to_string(),
                probe_ok: false,
                last_error: "auth_failed".to_string(),
                last_probe_at_unix_secs: 1,
                last_activity_kind: String::new(),
                last_activity_ok: false,
                last_activity_at_unix_secs: 0,
                updated_at: 1,
            })
            .expect("seed runtime status");
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            credential_store.clone(),
            runtime_status_store.clone(),
        );

        service.revoke("mail-work", true).expect("revoke");
        assert!(credential_store.get("mail-work").unwrap().is_none());
        assert!(runtime_status_store.get("mail-work").unwrap().is_none());
    }

    #[test]
    fn probe_without_adapter_returns_structured_unsupported() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry": {
                    "accounts": {
                        "mail-work": {
                            "account_key": "mail-work",
                            "provider_kind": "imap_smtp",
                            "external_account_id": "work@example.com",
                            "account_label": "Work",
                            "identity_class": "work",
                            "enabled_capabilities": ["mail"]
                        }
                    }
                },
                "binding": {},
                "policy": {}
            }"#,
        )
        .expect("seed accounts");
        let credential_store = Arc::new(MemoryCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: "token".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect("seed credential");
        let service = OfficeConfigManagementService::new(
            config_file_store,
            credential_store,
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let probe = service.probe("mail-work").expect("probe");
        assert_eq!(probe.disposition, OfficeProbeDisposition::Unsupported);
        assert_eq!(probe.reason, "probe_adapter_unavailable");
    }

    #[test]
    fn assess_account_reports_missing_mail_transport_fields() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry": {
                    "accounts": {
                        "mail-work": {
                            "account_key": "mail-work",
                            "provider_kind": "imap_smtp",
                            "external_account_id": "",
                            "account_label": "Work",
                            "identity_class": "work",
                            "enabled_capabilities": ["mail"]
                        }
                    }
                },
                "binding": {},
                "policy": {}
            }"#,
        )
        .expect("seed accounts");
        let credential_store = Arc::new(MemoryCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "mail-work".to_string(),
                access_token: String::new(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: BTreeMap::new(),
            })
            .expect("seed credential");
        let service = OfficeConfigManagementService::new(
            config_file_store,
            credential_store,
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let assessment = service.assess_account("mail-work").expect("assess account");
        assert_eq!(assessment.account_key, "mail-work");
        assert_eq!(
            assessment.readiness,
            OfficeConfigReadiness::NeedsCredentialInput
        );
        assert_eq!(
            assessment.next_action,
            OfficeConfigNextAction::DraftCredentials
        );
        assert!(assessment
            .missing_fields
            .contains(&"access_token".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"mail_username".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"mail_imap_host".to_string()));
        assert!(assessment
            .missing_fields
            .contains(&"mail_smtp_host".to_string()));
    }

    #[test]
    fn assess_account_reports_ready_for_probe_when_transport_shape_is_complete() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry": {
                    "accounts": {
                        "docs-work": {
                            "account_key": "docs-work",
                            "provider_kind": "webdav",
                            "external_account_id": "work@example.com",
                            "account_label": "Docs",
                            "identity_class": "work",
                            "enabled_capabilities": ["documents"]
                        }
                    }
                },
                "binding": {},
                "policy": {}
            }"#,
        )
        .expect("seed accounts");
        let credential_store = Arc::new(MemoryCredentialStore::default());
        credential_store
            .set(&OfficeCredential {
                account_key: "docs-work".to_string(),
                access_token: "secret".to_string(),
                refresh_token: String::new(),
                token_endpoint: String::new(),
                expires_at_unix_secs: 0,
                updated_at: 1,
                metadata: [(
                    crate::documents::OFFICE_METADATA_DOCUMENTS_BASE_URL.to_string(),
                    "https://dav.example.com/root".to_string(),
                )]
                .into_iter()
                .collect(),
            })
            .expect("seed credential");
        let service = OfficeConfigManagementService::new(
            config_file_store,
            credential_store,
            Arc::new(MemoryRuntimeStatusStore::default()),
        )
        .with_probe_adapters(vec![Arc::new(
            crate::documents::providers::webdav::WebDavOfficeProbeAdapter,
        )]);

        let assessment = service.assess_account("docs-work").expect("assess account");
        assert_eq!(assessment.readiness, OfficeConfigReadiness::ReadyForProbe);
        assert_eq!(assessment.next_action, OfficeConfigNextAction::Probe);
        assert!(assessment.missing_fields.is_empty());
        assert!(assessment.probe_supported);
    }
}
