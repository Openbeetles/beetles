use crate::config::{
    self, validate_office_accounts_candidate, validate_office_credentials_candidate,
    ConfigFileStore, OfficeAccountsSegment,
};
use crate::error::{Error, Result};
use crate::office::{
    assess_office_account, office_provider_schema, office_provider_schemas, OfficeAccount,
    OfficeAccountAssessment, OfficeAccountAuthorityStatus, OfficeAccountIdentityClass,
    OfficeAccountRuntimeStatus, OfficeAuthoritySummary, OfficeCapability, OfficeConfigAssessment,
    OfficeCredential, OfficeCredentialStore, OfficeCredentialsSegment, OfficeProviderFieldLocation,
    OfficeProviderFieldSchema, OfficeProviderFieldValueKind, OfficeProviderSchema,
    OfficeResolveRequest, OfficeResolveResult, OfficeRuntimeStatusStore, OfficeSelectionPolicy,
    OfficeService,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigSnapshot {
    pub accounts: OfficeAccountsSegment,
    pub credentials: OfficeCredentialsSegment,
    pub summary: OfficeAuthoritySummary,
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
pub struct OfficeAccountRecordInput {
    #[serde(default)]
    pub account_key: String,
    pub provider_kind: String,
    #[serde(default)]
    pub external_account_id: String,
    #[serde(default)]
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    #[serde(default)]
    pub enabled_capabilities: Vec<OfficeCapability>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountUpsertRequest {
    pub account: OfficeAccountRecordInput,
    #[serde(default)]
    pub set_defaults: Vec<OfficeCapability>,
    #[serde(default)]
    pub clear_defaults: Vec<OfficeCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_patch: Option<OfficePolicyPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<OfficeAccountConfigSaveRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeCredentialDraftRequest {
    pub credential: OfficeCredential,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigAccountSummary {
    pub account_key: String,
    pub provider_kind: String,
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    #[serde(default)]
    pub enabled_capabilities: Vec<OfficeCapability>,
    #[serde(default)]
    pub selected_for_capabilities: Vec<OfficeCapability>,
    pub readiness: crate::office::OfficeConfigReadiness,
    pub next_action: crate::office::OfficeConfigNextAction,
    pub missing_fields_count: usize,
    pub has_runtime_error: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountConfigSaveRequest {
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub clear_fields: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigFieldState {
    #[serde(flatten)]
    pub schema: OfficeProviderFieldSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_value: Option<String>,
    #[serde(default)]
    pub configured: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigAccountDetail {
    pub account: OfficeAccountAuthorityStatus,
    pub assessment: OfficeAccountAssessment,
    #[serde(default)]
    pub fields: Vec<OfficeConfigFieldState>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigFieldOption {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigCreateFieldSchema {
    pub key: String,
    pub label: String,
    pub description: String,
    pub value_kind: OfficeProviderFieldValueKind,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default)]
    pub default_values: Vec<String>,
    #[serde(default)]
    pub options: Vec<OfficeConfigFieldOption>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigProviderCatalogItem {
    pub provider_kind: String,
    #[serde(default)]
    pub capabilities: Vec<OfficeCapability>,
    #[serde(default)]
    pub account_fields: Vec<OfficeConfigCreateFieldSchema>,
    #[serde(default)]
    pub config_fields: Vec<OfficeProviderFieldSchema>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigCapabilitySelectionStatus {
    Selected,
    Ambiguous,
    Missing,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeConfigCapabilityNextAction {
    CreateAccount,
    SelectDefaultAccount,
    DraftCredentials,
    Probe,
    ReviewRuntimeError,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigCapabilityStatus {
    pub capability: OfficeCapability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_account_key: Option<String>,
    pub selection_status: OfficeConfigCapabilitySelectionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_account_key: Option<String>,
    pub ready: bool,
    pub next_action: OfficeConfigCapabilityNextAction,
    #[serde(default)]
    pub accounts: Vec<OfficeConfigAccountSummary>,
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

    pub fn with_default_probe_adapters(self) -> Self {
        self.with_probe_adapters(
            crate::office::build_default_office_integration_topology().probe_adapters(),
        )
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

    pub fn provider_schemas(
        &self,
        provider_kind: Option<&str>,
        capability: Option<OfficeCapability>,
    ) -> Result<Vec<OfficeProviderSchema>> {
        if let Some(provider_kind) = provider_kind {
            let schema = office_provider_schema(provider_kind).ok_or_else(|| {
                Error::config(
                    "office_config_provider_schema",
                    format!("unknown office provider '{}'", provider_kind),
                )
            })?;
            return Ok(vec![schema]);
        }
        Ok(office_provider_schemas(capability))
    }

    pub fn provider_catalog(
        &self,
        capability: Option<OfficeCapability>,
    ) -> Result<Vec<OfficeConfigProviderCatalogItem>> {
        Ok(office_provider_schemas(capability)
            .into_iter()
            .map(build_provider_catalog_item)
            .collect())
    }

    pub fn account_summaries(
        &self,
        provider_kind: Option<&str>,
        capability: Option<OfficeCapability>,
    ) -> Result<Vec<OfficeConfigAccountSummary>> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        let summary = office.summary()?;
        let assessments = office.assess_all_accounts(|provider_kind| {
            self.probe_supported_for_provider(provider_kind)
        })?;
        let assessment_by_account = assessments
            .into_iter()
            .map(|assessment| (assessment.account_key.clone(), assessment))
            .collect::<BTreeMap<_, _>>();
        let mut items = summary
            .accounts
            .into_iter()
            .filter(|account| {
                provider_kind
                    .map(|value| account.provider_kind == value)
                    .unwrap_or(true)
            })
            .filter(|account| {
                capability
                    .map(|value| account.enabled_capabilities.contains(&value))
                    .unwrap_or(true)
            })
            .filter_map(|account| {
                assessment_by_account
                    .get(&account.account_key)
                    .map(|assessment| build_account_summary(&account, assessment))
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        Ok(items)
    }

    pub fn capability_statuses(
        &self,
        capability: Option<OfficeCapability>,
    ) -> Result<Vec<OfficeConfigCapabilityStatus>> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        let summary = office.summary()?;
        let assessments = office.assess_all_accounts(|provider_kind| {
            self.probe_supported_for_provider(provider_kind)
        })?;
        let assessment_by_account = assessments
            .into_iter()
            .map(|assessment| (assessment.account_key.clone(), assessment))
            .collect::<BTreeMap<_, _>>();
        let account_status_by_key = summary
            .accounts
            .iter()
            .map(|account| (account.account_key.clone(), account))
            .collect::<BTreeMap<_, _>>();
        let default_by_capability = summary
            .defaults
            .iter()
            .map(|item| (item.capability, item.account_key.clone()))
            .collect::<BTreeMap<_, _>>();
        let capabilities = capability
            .map(|value| vec![value])
            .unwrap_or_else(|| OfficeCapability::all().to_vec());
        let mut items = Vec::with_capacity(capabilities.len());
        for capability in capabilities {
            let mut capability_accounts = summary
                .accounts
                .iter()
                .filter(|account| account.enabled_capabilities.contains(&capability))
                .filter_map(|account| {
                    assessment_by_account
                        .get(&account.account_key)
                        .map(|assessment| build_account_summary(account, assessment))
                })
                .collect::<Vec<_>>();
            capability_accounts.sort_by(|left, right| left.account_key.cmp(&right.account_key));
            let resolve_result = office.resolve(&OfficeResolveRequest {
                capability,
                preferred_account_key: None,
                preferred_provider_kind: None,
                preferred_identity_class: None,
                historical_account_key: None,
            });
            let default_account_key = default_by_capability.get(&capability).cloned().flatten();
            let (selection_status, selected_account_key, ready, next_action) = match resolve_result
            {
                OfficeResolveResult::Selected(selection) => {
                    let assessment = assessment_by_account
                        .get(&selection.account_key)
                        .ok_or_else(|| {
                            Error::config(
                                "office_config_capability_status",
                                format!(
                                    "missing assessment for selected office account '{}'",
                                    selection.account_key
                                ),
                            )
                        })?;
                    let has_runtime_error = account_status_by_key
                        .get(&selection.account_key)
                        .and_then(|account| account.runtime_status.as_ref())
                        .is_some_and(runtime_status_indicates_failure);
                    let next_action = if has_runtime_error {
                        OfficeConfigCapabilityNextAction::ReviewRuntimeError
                    } else {
                        map_account_next_action(assessment.next_action)
                    };
                    (
                        OfficeConfigCapabilitySelectionStatus::Selected,
                        Some(selection.account_key),
                        assessment.readiness == crate::office::OfficeConfigReadiness::Ready
                            && !has_runtime_error,
                        next_action,
                    )
                }
                OfficeResolveResult::Ambiguous(_) => (
                    OfficeConfigCapabilitySelectionStatus::Ambiguous,
                    None,
                    false,
                    OfficeConfigCapabilityNextAction::SelectDefaultAccount,
                ),
                OfficeResolveResult::Missing(_) => (
                    OfficeConfigCapabilitySelectionStatus::Missing,
                    None,
                    false,
                    OfficeConfigCapabilityNextAction::CreateAccount,
                ),
            };
            items.push(OfficeConfigCapabilityStatus {
                capability,
                default_account_key,
                selection_status,
                selected_account_key,
                ready,
                next_action,
                accounts: capability_accounts,
            });
        }
        Ok(items)
    }

    pub fn account_detail(&self, account_key: &str) -> Result<OfficeConfigAccountDetail> {
        let accounts = self.load_accounts_segment()?;
        let office = self.build_office_service(&accounts)?;
        let summary = office.summary()?;
        let account = summary
            .accounts
            .into_iter()
            .find(|item| item.account_key == account_key)
            .ok_or_else(|| {
                Error::config(
                    "office_config_account_detail",
                    format!("unknown office account '{}'", account_key),
                )
            })?;
        let assessment = self.build_account_assessment(&office, account_key)?;
        let provider_schema = office_provider_schema(&account.provider_kind).ok_or_else(|| {
            Error::config(
                "office_config_account_detail",
                format!("unknown office provider '{}'", account.provider_kind),
            )
        })?;
        let credential = office.credential(account_key)?;
        let fields = provider_schema
            .fields
            .iter()
            .map(|schema| build_field_state(&account, credential.as_ref(), schema))
            .collect::<Vec<_>>();
        Ok(OfficeConfigAccountDetail {
            account,
            assessment,
            fields,
        })
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
        self.normalize_credentials_segment(&segment)
    }

    pub fn validate_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        validate_office_accounts_candidate(segment)
    }

    pub fn validate_credentials(&self, segment: &OfficeCredentialsSegment) -> Result<()> {
        self.normalize_credentials_segment(segment).map(|_| ())
    }

    pub fn commit_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        self.validate_accounts(segment)?;
        let body = serde_json::to_string(segment)
            .map_err(|error| Error::config("office_config_commit_accounts", error.to_string()))?;
        config::save_office_accounts_segment(self.config_file_store.as_ref(), &body)
    }

    pub fn commit_credentials(&self, segment: &OfficeCredentialsSegment) -> Result<()> {
        let normalized = self.normalize_credentials_segment(segment)?;
        let body = serde_json::to_string(&normalized).map_err(|error| {
            Error::config("office_config_commit_credentials", error.to_string())
        })?;
        config::save_office_credentials_segment(self.credential_store.as_ref(), &body)
    }

    pub fn save_account(
        &self,
        request: &OfficeAccountDraftRequest,
    ) -> Result<OfficeConfigAccountDetail> {
        let segment = self.draft_accounts(request)?;
        self.commit_accounts(&segment)?;
        self.account_detail(&request.account.account_key)
    }

    pub fn save_account_upsert(
        &self,
        request: &OfficeAccountUpsertRequest,
    ) -> Result<OfficeConfigAccountDetail> {
        let accounts = self.load_accounts_segment()?;
        let mut account = materialize_account_record_input(&accounts.registry, &request.account)?;
        let provider_schema = office_provider_schema(&account.provider_kind).ok_or_else(|| {
            Error::config(
                "office_config_save_account",
                format!("unknown office provider '{}'", account.provider_kind),
            )
        })?;
        if account
            .enabled_capabilities
            .iter()
            .any(|capability| !provider_schema.capabilities.contains(capability))
        {
            return Err(Error::config(
                "office_config_save_account",
                format!(
                    "provider '{}' does not support one or more enabled capabilities",
                    request.account.provider_kind
                ),
            ));
        }
        let segment = self.draft_accounts(&OfficeAccountDraftRequest {
            account: account.clone(),
            set_defaults: request.set_defaults.clone(),
            clear_defaults: request.clear_defaults.clone(),
            policy_patch: request.policy_patch.clone(),
        })?;
        let normalized_credential = if let Some(config) = request.config.as_ref() {
            Some(self.prepare_account_config(&mut account, config, false)?)
        } else {
            None
        };
        self.commit_accounts(&segment)?;
        if let Some(credential) = normalized_credential.as_ref() {
            self.credential_store.set(credential)?;
        }
        self.account_detail(&account.account_key)
    }

    pub fn save_account_config(
        &self,
        account_key: &str,
        request: &OfficeAccountConfigSaveRequest,
    ) -> Result<OfficeConfigAccountDetail> {
        let mut accounts = self.load_accounts_segment()?;
        let mut account = accounts.registry.get(account_key).cloned().ok_or_else(|| {
            Error::config(
                "office_config_save_account_config",
                format!("unknown office account '{}'", account_key),
            )
        })?;
        let normalized_credential = self.prepare_account_config(&mut account, request, true)?;
        accounts.registry.insert(account.clone());
        self.validate_accounts(&accounts)?;

        self.commit_accounts(&accounts)?;
        self.credential_store.set(&normalized_credential)?;
        self.account_detail(account_key)
    }

    pub fn delete_account(&self, account_key: &str) -> Result<()> {
        let mut accounts = self.load_accounts_segment()?;
        let removed = accounts.registry.remove(account_key).ok_or_else(|| {
            Error::config(
                "office_config_delete_account",
                format!("unknown office account '{}'", account_key),
            )
        })?;

        for capability in removed.enabled_capabilities {
            if accounts.binding.default_account_for(capability) == Some(account_key) {
                accounts.binding.clear_default_account(capability);
            }
        }
        if accounts.policy.global_default_account_key.trim() == account_key {
            accounts.policy.global_default_account_key.clear();
        }

        self.validate_accounts(&accounts)?;
        self.commit_accounts(&accounts)?;
        self.credential_store.clear(account_key)?;
        self.runtime_status_store.clear(account_key)?;
        Ok(())
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
            let result = OfficeProbeResult {
                account_key: account.account_key,
                provider_kind: account.provider_kind,
                configured: false,
                disposition: OfficeProbeDisposition::MissingCredential,
                reason: "credential_missing".to_string(),
            };
            self.persist_probe_runtime_status(&office, &result)?;
            return Ok(result);
        };
        if let Some(adapter) = self
            .probe_adapters
            .iter()
            .find(|adapter| adapter.provider_kind() == account.provider_kind)
        {
            let result = adapter.probe(&account, &credential)?;
            self.persist_probe_runtime_status(&office, &result)?;
            return Ok(result);
        }
        let result = OfficeProbeResult {
            account_key: account.account_key,
            provider_kind: account.provider_kind,
            configured: !credential.access_token.trim().is_empty(),
            disposition: OfficeProbeDisposition::Unsupported,
            reason: "probe_adapter_unavailable".to_string(),
        };
        self.persist_probe_runtime_status(&office, &result)?;
        Ok(result)
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
        let runtime_status = office.runtime_status(account_key)?;
        let probe_supported = self
            .probe_adapters
            .iter()
            .any(|adapter| adapter.provider_kind() == account.provider_kind);
        Ok(assess_office_account(
            &account,
            credential.as_ref(),
            runtime_status.as_ref(),
            probe_supported,
        ))
    }

    fn probe_supported_for_provider(&self, provider_kind: &str) -> bool {
        self.probe_adapters
            .iter()
            .any(|adapter| adapter.provider_kind() == provider_kind)
    }

    fn prepare_account_config(
        &self,
        account: &mut OfficeAccount,
        request: &OfficeAccountConfigSaveRequest,
        allow_external_account_id: bool,
    ) -> Result<OfficeCredential> {
        let provider_schema = office_provider_schema(&account.provider_kind).ok_or_else(|| {
            Error::config(
                "office_config_save_account_config",
                format!("unknown office provider '{}'", account.provider_kind),
            )
        })?;
        let field_schemas = provider_schema
            .fields
            .into_iter()
            .filter(|field| {
                allow_external_account_id
                    || field.location
                        != crate::office::OfficeProviderFieldLocation::ExternalAccountId
            })
            .map(|field| (field.key.clone(), field))
            .collect::<BTreeMap<_, _>>();
        for key in request.fields.keys() {
            if !field_schemas.contains_key(key) {
                return Err(Error::config(
                    "office_config_save_account_config",
                    format!(
                        "provider '{}' does not accept config field '{}'",
                        account.provider_kind, key
                    ),
                ));
            }
        }
        for key in &request.clear_fields {
            if !field_schemas.contains_key(key) {
                return Err(Error::config(
                    "office_config_save_account_config",
                    format!(
                        "provider '{}' does not accept config field '{}'",
                        account.provider_kind, key
                    ),
                ));
            }
        }

        let mut credential = self
            .credential_store
            .get(&account.account_key)?
            .unwrap_or_else(|| OfficeCredential {
                account_key: account.account_key.clone(),
                ..OfficeCredential::default()
            });
        let clear_fields = request
            .clear_fields
            .iter()
            .map(|field| field.trim().to_string())
            .collect::<BTreeSet<_>>();
        let now = crate::util::current_unix_secs();
        for (key, schema) in &field_schemas {
            if let Some(next_value) = request.fields.get(key) {
                apply_config_field_value(account, &mut credential, schema, next_value);
                continue;
            }
            if clear_fields.contains(key) {
                apply_config_field_value(account, &mut credential, schema, "");
            }
        }
        credential.updated_at = now;
        credential.account_key = account.account_key.clone();
        self.normalize_credential_for_account(account, &credential)
    }

    fn normalize_credentials_segment(
        &self,
        segment: &OfficeCredentialsSegment,
    ) -> Result<OfficeCredentialsSegment> {
        validate_office_credentials_candidate(segment)?;
        let accounts = self.load_accounts_segment()?;
        let mut items = Vec::with_capacity(segment.items.len());
        for credential in &segment.items {
            let account_key = credential.account_key.trim();
            let account = accounts.registry.get(account_key).ok_or_else(|| {
                Error::config(
                    "office_config_credentials",
                    format!(
                        "credential account_key '{}' is not registered in office accounts",
                        account_key
                    ),
                )
            })?;
            items.push(self.normalize_credential_for_account(account, credential)?);
        }
        items.sort_by(|left, right| left.account_key.cmp(&right.account_key));
        let normalized = OfficeCredentialsSegment { items };
        validate_office_credentials_candidate(&normalized)?;
        Ok(normalized)
    }

    fn normalize_credential_for_account(
        &self,
        account: &OfficeAccount,
        credential: &OfficeCredential,
    ) -> Result<OfficeCredential> {
        let schema = office_provider_schema(&account.provider_kind).ok_or_else(|| {
            Error::config(
                "office_config_credentials",
                format!("unknown office provider '{}'", account.provider_kind),
            )
        })?;
        let mut allowed_metadata_keys = BTreeSet::new();
        let mut metadata_defaults = BTreeMap::new();
        for field in schema.fields {
            if field.location != crate::office::OfficeProviderFieldLocation::Metadata {
                continue;
            }
            allowed_metadata_keys.insert(field.key.clone());
            if let Some(default_value) = field.default_value.as_ref() {
                metadata_defaults.insert(field.key.clone(), default_value.clone());
            }
        }

        let mut metadata = BTreeMap::new();
        for (key, value) in &credential.metadata {
            let key = key.trim();
            if key.is_empty() {
                return Err(Error::config(
                    "office_config_credentials",
                    "credential metadata key must not be empty",
                ));
            }
            if !allowed_metadata_keys.contains(key) {
                return Err(Error::config(
                    "office_config_credentials",
                    format!(
                        "provider '{}' does not accept credential metadata key '{}'",
                        account.provider_kind, key
                    ),
                ));
            }
            let value = value.trim();
            if value.is_empty() {
                if let Some(default_value) = metadata_defaults.get(key) {
                    metadata.insert(key.to_string(), default_value.clone());
                }
                continue;
            }
            metadata.insert(key.to_string(), value.to_string());
        }
        for (key, default_value) in metadata_defaults {
            metadata.entry(key).or_insert(default_value);
        }

        let normalized = OfficeCredential {
            account_key: credential.account_key.trim().to_string(),
            access_token: credential.access_token.trim().to_string(),
            refresh_token: credential.refresh_token.trim().to_string(),
            token_endpoint: credential.token_endpoint.trim().to_string(),
            expires_at_unix_secs: credential.expires_at_unix_secs,
            updated_at: credential.updated_at,
            metadata,
        };
        let assessment = assess_office_account(account, Some(&normalized), None, false);
        if !assessment.missing_fields.is_empty() {
            return Err(Error::config(
                "office_config_credentials",
                format!(
                    "credential for account '{}' is missing required fields: {}",
                    account.account_key,
                    assessment.missing_fields.join(", ")
                ),
            ));
        }
        Ok(normalized)
    }

    fn persist_probe_runtime_status(
        &self,
        office: &OfficeService,
        result: &OfficeProbeResult,
    ) -> Result<()> {
        let now = crate::util::current_unix_secs();
        let mut status = office
            .runtime_status(&result.account_key)?
            .unwrap_or_else(|| OfficeAccountRuntimeStatus {
                account_key: result.account_key.clone(),
                ..OfficeAccountRuntimeStatus::default()
            });
        status.account_key = result.account_key.clone();
        status.last_probe_at_unix_secs = now;
        status.updated_at = now;
        status.last_activity_kind.clear();
        status.last_activity_ok = false;
        status.last_activity_at_unix_secs = 0;
        match result.disposition {
            OfficeProbeDisposition::Ready => {
                status.probe_ok = true;
                status.last_error.clear();
            }
            OfficeProbeDisposition::MissingCredential | OfficeProbeDisposition::Unsupported => {
                status.probe_ok = false;
                status.last_error = result.reason.clone();
            }
        }
        office.set_runtime_status(&status)
    }
}

fn build_field_state(
    account: &OfficeAccountAuthorityStatus,
    credential: Option<&OfficeCredential>,
    schema: &OfficeProviderFieldSchema,
) -> OfficeConfigFieldState {
    let raw_value = match schema.location {
        OfficeProviderFieldLocation::AccessToken => credential
            .map(|item| item.access_token.trim().to_string())
            .filter(|value| !value.is_empty()),
        OfficeProviderFieldLocation::RefreshToken => credential
            .map(|item| item.refresh_token.trim().to_string())
            .filter(|value| !value.is_empty()),
        OfficeProviderFieldLocation::TokenEndpoint => credential
            .map(|item| item.token_endpoint.trim().to_string())
            .filter(|value| !value.is_empty()),
        OfficeProviderFieldLocation::ExternalAccountId => {
            let value = account.external_account_id.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        OfficeProviderFieldLocation::Metadata => credential
            .and_then(|item| item.metadata_value(&schema.key))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    };
    let configured = raw_value.is_some()
        || schema
            .default_value
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
    let current_value = if schema.secret {
        None
    } else {
        raw_value.or_else(|| schema.default_value.clone())
    };
    OfficeConfigFieldState {
        schema: schema.clone(),
        current_value,
        configured,
    }
}

fn build_provider_catalog_item(schema: OfficeProviderSchema) -> OfficeConfigProviderCatalogItem {
    let mut account_fields = default_account_create_fields(&schema);
    for field in &schema.fields {
        if field.location == OfficeProviderFieldLocation::ExternalAccountId {
            account_fields.push(build_create_field_from_provider_field(field));
        }
    }
    let config_fields = schema
        .fields
        .into_iter()
        .filter(|field| field.location != OfficeProviderFieldLocation::ExternalAccountId)
        .collect::<Vec<_>>();
    OfficeConfigProviderCatalogItem {
        provider_kind: schema.provider_kind,
        capabilities: schema.capabilities,
        account_fields,
        config_fields,
    }
}

fn default_account_create_fields(
    schema: &OfficeProviderSchema,
) -> Vec<OfficeConfigCreateFieldSchema> {
    vec![
        OfficeConfigCreateFieldSchema {
            key: "account_label".to_string(),
            label: "Account label".to_string(),
            description: "Human-readable account label.".to_string(),
            value_kind: crate::office::OfficeProviderFieldValueKind::Text,
            required: false,
            secret: false,
            multiple: false,
            default_value: Some(schema.display_name.clone()),
            default_values: Vec::new(),
            options: Vec::new(),
        },
        OfficeConfigCreateFieldSchema {
            key: "identity_class".to_string(),
            label: "Identity class".to_string(),
            description: "Identity bucket used during account selection.".to_string(),
            value_kind: crate::office::OfficeProviderFieldValueKind::Identifier,
            required: true,
            secret: false,
            multiple: false,
            default_value: Some("work".to_string()),
            default_values: Vec::new(),
            options: vec![
                build_field_option("work", "Work"),
                build_field_option("personal", "Personal"),
                build_field_option("family", "Family"),
                build_field_option("shared", "Shared"),
                build_field_option("other", "Other"),
            ],
        },
        OfficeConfigCreateFieldSchema {
            key: "enabled_capabilities".to_string(),
            label: "Enabled capabilities".to_string(),
            description: "Capabilities this account can serve.".to_string(),
            value_kind: crate::office::OfficeProviderFieldValueKind::Identifier,
            required: true,
            secret: false,
            multiple: true,
            default_value: None,
            default_values: schema
                .capabilities
                .iter()
                .map(|capability| capability_key(*capability).to_string())
                .collect(),
            options: schema
                .capabilities
                .iter()
                .map(|capability| {
                    build_field_option(
                        capability_key(*capability),
                        capability_display_name(*capability),
                    )
                })
                .collect(),
        },
    ]
}

fn materialize_account_record_input(
    registry: &crate::office::OfficeAccountRegistry,
    input: &OfficeAccountRecordInput,
) -> Result<OfficeAccount> {
    let account_key = if input.account_key.trim().is_empty() {
        generate_account_key(registry, input)?
    } else {
        input.account_key.trim().to_string()
    };
    Ok(OfficeAccount {
        account_key,
        provider_kind: input.provider_kind.trim().to_string(),
        external_account_id: input.external_account_id.trim().to_string(),
        account_label: input.account_label.trim().to_string(),
        identity_class: input.identity_class,
        enabled_capabilities: input.enabled_capabilities.clone(),
    })
}

fn generate_account_key(
    registry: &crate::office::OfficeAccountRegistry,
    input: &OfficeAccountRecordInput,
) -> Result<String> {
    generate_account_key_with_limit(registry, input, crate::config::CONFIG_ACCOUNT_KEY_MAX_LEN)
}

fn generate_account_key_with_limit(
    registry: &crate::office::OfficeAccountRegistry,
    input: &OfficeAccountRecordInput,
    max_len: usize,
) -> Result<String> {
    let provider_slug = slugify_account_key_segment(&input.provider_kind, "provider");
    let identity_slug =
        slugify_account_key_segment(identity_class_slug(input.identity_class), "acct");
    let seed_slug = slugify_account_key_segment(
        if input.external_account_id.trim().is_empty() {
            &input.account_label
        } else {
            &input.external_account_id
        },
        "",
    );
    let mut base = format!("{provider_slug}-{identity_slug}");
    if !seed_slug.is_empty() {
        base.push('-');
        base.push_str(&seed_slug);
    }
    let mut candidate = truncate_account_key_candidate(&base, max_len);
    if registry.get(&candidate).is_none() {
        return Ok(candidate);
    }
    for suffix in 2..=usize::MAX {
        let suffix_text = format!("-{suffix}");
        if suffix_text.len() >= max_len {
            return Err(Error::config(
                "office_account_key_generate",
                format!(
                    "could not generate unique account key within max length {} for provider '{}'",
                    max_len, input.provider_kind
                ),
            ));
        }
        let keep_len = max_len.saturating_sub(suffix_text.len());
        candidate = format!(
            "{}{}",
            truncate_account_key_candidate(&base, keep_len.max(1)),
            suffix_text
        );
        if registry.get(&candidate).is_none() {
            return Ok(candidate);
        }
    }
    Err(Error::config(
        "office_account_key_generate",
        format!(
            "could not generate unique account key within max length {} for provider '{}'",
            max_len, input.provider_kind
        ),
    ))
}

fn truncate_account_key_candidate(value: &str, max_len: usize) -> String {
    value.chars().take(max_len).collect::<String>()
}

fn slugify_account_key_segment(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;
    for ch in value.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !out.is_empty() && !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed
    }
}

fn identity_class_slug(identity_class: OfficeAccountIdentityClass) -> &'static str {
    match identity_class {
        OfficeAccountIdentityClass::Work => "work",
        OfficeAccountIdentityClass::Personal => "personal",
        OfficeAccountIdentityClass::Family => "family",
        OfficeAccountIdentityClass::Shared => "shared",
        OfficeAccountIdentityClass::Other => "other",
    }
}

fn build_create_field_from_provider_field(
    field: &OfficeProviderFieldSchema,
) -> OfficeConfigCreateFieldSchema {
    OfficeConfigCreateFieldSchema {
        key: field.key.clone(),
        label: field.label.clone(),
        description: field.description.clone(),
        value_kind: field.value_kind,
        required: field.required,
        secret: field.secret,
        multiple: false,
        default_value: field.default_value.clone(),
        default_values: Vec::new(),
        options: Vec::new(),
    }
}

fn build_field_option(value: &str, label: &str) -> OfficeConfigFieldOption {
    OfficeConfigFieldOption {
        value: value.to_string(),
        label: label.to_string(),
    }
}

fn build_account_summary(
    account: &OfficeAccountAuthorityStatus,
    assessment: &OfficeAccountAssessment,
) -> OfficeConfigAccountSummary {
    OfficeConfigAccountSummary {
        account_key: account.account_key.clone(),
        provider_kind: account.provider_kind.clone(),
        account_label: account.account_label.clone(),
        identity_class: account.identity_class,
        enabled_capabilities: account.enabled_capabilities.clone(),
        selected_for_capabilities: account.selected_for_capabilities.clone(),
        readiness: assessment.readiness,
        next_action: assessment.next_action,
        missing_fields_count: assessment.missing_fields.len(),
        has_runtime_error: account
            .runtime_status
            .as_ref()
            .is_some_and(runtime_status_indicates_failure),
    }
}

fn runtime_status_indicates_failure(status: &OfficeAccountRuntimeStatus) -> bool {
    !status.last_error.trim().is_empty()
        || (!status.last_activity_kind.trim().is_empty() && !status.last_activity_ok)
}

fn map_account_next_action(
    next_action: crate::office::OfficeConfigNextAction,
) -> OfficeConfigCapabilityNextAction {
    match next_action {
        crate::office::OfficeConfigNextAction::DraftCredentials => {
            OfficeConfigCapabilityNextAction::DraftCredentials
        }
        crate::office::OfficeConfigNextAction::Probe => OfficeConfigCapabilityNextAction::Probe,
        crate::office::OfficeConfigNextAction::None => OfficeConfigCapabilityNextAction::None,
    }
}

fn apply_config_field_value(
    account: &mut OfficeAccount,
    credential: &mut OfficeCredential,
    schema: &OfficeProviderFieldSchema,
    value: &str,
) {
    match schema.location {
        OfficeProviderFieldLocation::AccessToken => credential.access_token = value.to_string(),
        OfficeProviderFieldLocation::RefreshToken => credential.refresh_token = value.to_string(),
        OfficeProviderFieldLocation::TokenEndpoint => credential.token_endpoint = value.to_string(),
        OfficeProviderFieldLocation::ExternalAccountId => {
            account.external_account_id = value.to_string();
        }
        OfficeProviderFieldLocation::Metadata => {
            credential
                .metadata
                .insert(schema.key.clone(), value.to_string());
        }
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

fn capability_display_name(capability: OfficeCapability) -> &'static str {
    match capability {
        OfficeCapability::Mail => "Mail",
        OfficeCapability::Calendar => "Calendar",
        OfficeCapability::Documents => "Documents",
        OfficeCapability::ContactsDirectory => "Contacts Directory",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::{
        OfficeAccountRuntimeStatus, OfficeConfigNextAction, OfficeConfigReadiness,
    };
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
    fn delete_account_removes_registry_defaults_credentials_and_runtime() {
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
                "binding": {
                    "capability_defaults": {
                        "mail": "mail-work"
                    }
                },
                "policy": {
                    "global_default_account_key": "mail-work"
                }
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
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
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
            .expect("seed runtime");
        let service = OfficeConfigManagementService::new(
            config_file_store.clone(),
            credential_store.clone(),
            runtime_status_store.clone(),
        );

        service.delete_account("mail-work").expect("delete account");

        assert!(credential_store.get("mail-work").unwrap().is_none());
        assert!(runtime_status_store.get("mail-work").unwrap().is_none());
        let snapshot = service.inspect().expect("inspect");
        assert!(snapshot.accounts.registry.get("mail-work").is_none());
        assert_eq!(
            snapshot.summary.policy.global_default_account_key, "",
            "global default should be cleared"
        );
        assert_eq!(
            snapshot
                .accounts
                .binding
                .default_account_for(OfficeCapability::Mail),
            None
        );
    }

    #[test]
    fn provider_catalog_exposes_create_fields_and_provider_config_fields() {
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let providers = service.provider_catalog(None).expect("provider catalog");
        let imap = providers
            .iter()
            .find(|item| item.provider_kind == "imap_smtp")
            .expect("imap_smtp provider");

        assert!(
            !imap
                .account_fields
                .iter()
                .any(|field| field.key == "account_key"),
            "provider catalog should not expose internal account_key in account_fields"
        );
        let enabled_capabilities = imap
            .account_fields
            .iter()
            .find(|field| field.key == "enabled_capabilities")
            .expect("enabled_capabilities field");
        assert!(enabled_capabilities.multiple);
        assert_eq!(
            enabled_capabilities.default_values,
            vec!["mail".to_string()]
        );
        assert!(
            enabled_capabilities
                .options
                .iter()
                .any(|option| option.value == "mail"),
            "provider catalog should expose provider capabilities as create options"
        );
        assert!(
            imap.account_fields
                .iter()
                .any(|field| field.key == "external_account_id"),
            "external_account_id should be exposed as account field"
        );
        assert!(
            imap.config_fields
                .iter()
                .any(|field| field.key == "access_token"),
            "provider catalog should expose provider credential fields"
        );
        assert!(
            !imap
                .config_fields
                .iter()
                .any(|field| field.key == "external_account_id"),
            "external_account_id should not be duplicated into provider config fields"
        );
    }

    #[test]
    fn provider_catalog_includes_international_office_providers() {
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let providers = service.provider_catalog(None).expect("provider catalog");
        let provider_kinds = providers
            .into_iter()
            .map(|item| item.provider_kind)
            .collect::<Vec<_>>();
        for provider_kind in [
            "microsoft365_mail",
            "microsoft365_calendar",
            "microsoft365_documents",
            "microsoft365_contacts_directory",
            "google_mail",
            "google_calendar",
            "google_documents",
            "google_contacts_directory",
        ] {
            assert!(
                provider_kinds.iter().any(|item| item == provider_kind),
                "missing microsoft provider: {provider_kind}"
            );
        }
    }

    #[test]
    fn save_account_upsert_generates_account_key_when_missing() {
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let detail = service
            .save_account_upsert(&OfficeAccountUpsertRequest {
                account: OfficeAccountRecordInput {
                    account_key: String::new(),
                    provider_kind: "imap_smtp".to_string(),
                    external_account_id: String::new(),
                    account_label: "Primary mail".to_string(),
                    identity_class: OfficeAccountIdentityClass::Work,
                    enabled_capabilities: vec![OfficeCapability::Mail],
                },
                set_defaults: vec![],
                clear_defaults: vec![],
                policy_patch: None,
                config: None,
            })
            .expect("save account upsert");

        assert_eq!(detail.account.account_key, "imap-smtp-work-primary-mail");
    }

    #[test]
    fn account_summaries_filter_by_provider_kind() {
        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        config::save_office_accounts_segment(
            config_file_store.as_ref(),
            r#"{
                "registry": {
                    "accounts": {
                        "mail-imap": {
                            "account_key": "mail-imap",
                            "provider_kind": "imap_smtp",
                            "external_account_id": "",
                            "account_label": "IMAP",
                            "identity_class": "work",
                            "enabled_capabilities": ["mail"]
                        },
                        "mail-feishu": {
                            "account_key": "mail-feishu",
                            "provider_kind": "feishu_mail",
                            "external_account_id": "",
                            "account_label": "Feishu",
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
        let service = OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let items = service
            .account_summaries(Some("feishu_mail"), Some(OfficeCapability::Mail))
            .expect("account summaries");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].account_key, "mail-feishu");
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
    fn probe_success_persists_runtime_probe_state() {
        #[derive(Clone)]
        struct ReadyProbeAdapter;

        impl OfficeProbeAdapter for ReadyProbeAdapter {
            fn provider_kind(&self) -> &'static str {
                "imap_smtp"
            }

            fn probe(
                &self,
                account: &OfficeAccount,
                _credential: &OfficeCredential,
            ) -> Result<OfficeProbeResult> {
                Ok(OfficeProbeResult {
                    account_key: account.account_key.clone(),
                    provider_kind: account.provider_kind.clone(),
                    configured: true,
                    disposition: OfficeProbeDisposition::Ready,
                    reason: "imap_ok".to_string(),
                })
            }
        }

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
        let runtime_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service = OfficeConfigManagementService::new(
            config_file_store,
            credential_store,
            runtime_store.clone(),
        )
        .with_probe_adapters(vec![Arc::new(ReadyProbeAdapter)]);

        let probe = service.probe("mail-work").expect("probe");
        assert_eq!(probe.disposition, OfficeProbeDisposition::Ready);

        let runtime = runtime_store
            .get("mail-work")
            .expect("load runtime")
            .expect("runtime status");
        assert!(runtime.probe_ok);
        assert!(runtime.last_error.is_empty());
        assert_eq!(runtime.last_activity_kind, "");
    }

    #[test]
    fn probe_missing_credential_persists_runtime_failure_state() {
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
        let runtime_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service = OfficeConfigManagementService::new(
            config_file_store,
            Arc::new(MemoryCredentialStore::default()),
            runtime_store.clone(),
        );

        let probe = service.probe("mail-work").expect("probe");
        assert_eq!(probe.disposition, OfficeProbeDisposition::MissingCredential);

        let runtime = runtime_store
            .get("mail-work")
            .expect("load runtime")
            .expect("runtime status");
        assert!(!runtime.probe_ok);
        assert_eq!(runtime.last_error, "credential_missing");
        assert_eq!(runtime.last_activity_kind, "");
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

    #[test]
    fn generate_account_key_returns_error_when_suffix_cannot_fit_within_max_len() {
        let mut registry = crate::office::OfficeAccountRegistry::default();
        registry.insert(OfficeAccount {
            account_key: "p".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: String::new(),
            account_label: "Work".to_string(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });
        let input = OfficeAccountRecordInput {
            account_key: String::new(),
            provider_kind: "p".to_string(),
            external_account_id: String::new(),
            account_label: String::new(),
            identity_class: OfficeAccountIdentityClass::Work,
            enabled_capabilities: vec![OfficeCapability::Mail],
        };

        let error =
            generate_account_key_with_limit(&registry, &input, 1).expect_err("fit error expected");

        assert_eq!(error.stage(), "office_account_key_generate");
    }
}
