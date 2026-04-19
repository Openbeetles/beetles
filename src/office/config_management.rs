use crate::config::{
    self, validate_office_accounts_candidate, ConfigFileStore, OfficeAccountsSegment,
};
use crate::error::{Error, Result};
use crate::office::{
    assess_office_account, office_provider_schema, office_provider_schemas, OfficeAccount,
    OfficeAccountAssessment, OfficeAccountAuthorityStatus, OfficeAccountIdentityClass,
    OfficeAccountRuntimeStatus, OfficeAuthoritySummary, OfficeCapability, OfficeConfigAssessment,
    OfficeCredential, OfficeCredentialStore, OfficeCredentialsSegment, OfficeProviderFieldLocation,
    OfficeProviderFieldSchema, OfficeProviderFieldValueKind, OfficeProviderSchema,
    OfficeResolveRequest, OfficeResolveResult, OfficeRuntimeStatusStore, OfficeService,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
use crate::office::{OfficeHttpClient, UnavailableOfficeHttpClient};
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountOnboardingRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<OfficeCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_class: Option<OfficeAccountIdentityClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<OfficeAccountConfigSaveRequest>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigAccountSummary {
    pub account_key: String,
    pub provider_kind: String,
    pub account_label: String,
    pub identity_class: OfficeAccountIdentityClass,
    #[serde(default)]
    pub enabled_capabilities: Vec<OfficeCapability>,
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OfficeAccountOnboardingDisposition {
    Applied,
    NeedsUserFacts,
    ProbeFailed,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeAccountOnboardingResult {
    pub disposition: OfficeAccountOnboardingDisposition,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability: Option<OfficeCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_field_details: Vec<OfficeConfigCreateFieldSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<OfficeConfigAccountDetail>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe: Option<OfficeProbeResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
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
    SelectAccount,
    ConfigureAccount,
    Probe,
    ReviewRuntimeError,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficeConfigCapabilityStatus {
    pub capability: OfficeCapability,
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
        http: &mut dyn OfficeHttpClient,
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
                    OfficeConfigCapabilityNextAction::SelectAccount,
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

    fn validate_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        validate_office_accounts_candidate(segment)
    }

    fn persist_accounts(&self, segment: &OfficeAccountsSegment) -> Result<()> {
        self.validate_accounts(segment)?;
        let body = serde_json::to_string(segment)
            .map_err(|error| Error::config("office_config_persist_accounts", error.to_string()))?;
        config::save_office_accounts_segment(self.config_file_store.as_ref(), &body)
    }

    pub fn apply_account(
        &self,
        request: &OfficeAccountOnboardingRequest,
    ) -> Result<OfficeAccountOnboardingResult> {
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.apply_account_with_http(&mut unavailable_http, request)
    }

    pub fn apply_account_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        request: &OfficeAccountOnboardingRequest,
    ) -> Result<OfficeAccountOnboardingResult> {
        let accounts = self.load_accounts_segment()?;
        let mut missing_fields = Vec::new();
        let mut missing_field_details = Vec::new();

        let provider_kind = request
            .provider_kind
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if provider_kind.is_none() {
            push_missing_field(
                &mut missing_fields,
                &mut missing_field_details,
                "provider",
                provider_selector_field_schema(),
            );
        }
        if request.identity_class.is_none() {
            push_missing_field(
                &mut missing_fields,
                &mut missing_field_details,
                "identity_class",
                identity_class_field_schema(),
            );
        }
        let Some(provider_kind) = provider_kind else {
            return Ok(OfficeAccountOnboardingResult {
                disposition: OfficeAccountOnboardingDisposition::NeedsUserFacts,
                reason: "missing_user_facts".to_string(),
                provider_kind: None,
                capability: request.capability,
                missing_fields,
                missing_field_details,
                account: None,
                probe: None,
                error_stage: None,
                error_message: None,
            });
        };

        let provider_schema = match office_provider_schema(&provider_kind) {
            Some(schema) => schema,
            None => {
                return Ok(OfficeAccountOnboardingResult {
                    disposition: OfficeAccountOnboardingDisposition::Unsupported,
                    reason: "unknown_provider".to_string(),
                    provider_kind: Some(provider_kind),
                    capability: request.capability,
                    missing_fields: Vec::new(),
                    missing_field_details: Vec::new(),
                    account: None,
                    probe: None,
                    error_stage: None,
                    error_message: None,
                })
            }
        };
        let capability = request
            .capability
            .or_else(|| infer_single_capability_for_provider(&provider_kind));
        if capability.is_none() {
            push_missing_field(
                &mut missing_fields,
                &mut missing_field_details,
                "capability",
                capability_field_schema(),
            );
        }

        let account_label = request
            .account_label
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| request.external_account_id.clone())
            .unwrap_or_else(|| provider_schema.display_name.clone());
        let mut provisional_account = OfficeAccount {
            account_key: request
                .external_account_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|external_account_id| {
                    existing_account_key_for_provider_identity(
                        &accounts.registry,
                        &provider_kind,
                        external_account_id,
                    )
                })
                .unwrap_or_default(),
            provider_kind: provider_kind.clone(),
            external_account_id: request.external_account_id.clone().unwrap_or_default(),
            account_label,
            identity_class: request
                .identity_class
                .unwrap_or(OfficeAccountIdentityClass::Other),
            enabled_capabilities: capability.into_iter().collect(),
        };
        let provisional_credential = match request.config.as_ref() {
            Some(config) => Some(self.prepare_candidate_account_config(
                &mut provisional_account,
                config,
                false,
            )?),
            None if !provisional_account.account_key.trim().is_empty() => self
                .credential_store
                .get(&provisional_account.account_key)?
                .map(|credential| {
                    self.normalize_credential_structure_for_account(
                        &provisional_account,
                        &credential,
                    )
                })
                .transpose()?,
            None => None,
        };
        let assessment = assess_office_account(
            &provisional_account,
            provisional_credential.as_ref(),
            None,
            self.probe_supported_for_provider(&provider_kind),
        );
        for field in &assessment.missing_field_details {
            push_missing_field(
                &mut missing_fields,
                &mut missing_field_details,
                &field.key,
                build_create_field_from_provider_field(field),
            );
        }
        for field in &assessment.missing_fields {
            if !missing_fields.iter().any(|existing| existing == field) {
                missing_fields.push(field.clone());
            }
        }
        if !missing_fields.is_empty() {
            return Ok(OfficeAccountOnboardingResult {
                disposition: OfficeAccountOnboardingDisposition::NeedsUserFacts,
                reason: "missing_user_facts".to_string(),
                provider_kind: Some(provider_kind),
                capability,
                missing_fields,
                missing_field_details,
                account: None,
                probe: None,
                error_stage: None,
                error_message: None,
            });
        }

        let Some(identity_class) = request.identity_class else {
            return Err(Error::config(
                "office_config_apply_account",
                "identity_class must be present after onboarding assessment",
            ));
        };
        let Some(capability) = capability else {
            return Err(Error::config(
                "office_config_apply_account",
                "capability must be present after onboarding assessment",
            ));
        };
        let mut account = materialize_account_record_input(
            &accounts.registry,
            &OfficeAccountRecordInput {
                account_key: String::new(),
                provider_kind: provider_kind.clone(),
                external_account_id: provisional_account.external_account_id.clone(),
                account_label: provisional_account.account_label.clone(),
                identity_class,
                enabled_capabilities: vec![capability],
            },
        )?;
        let credential = match request.config.as_ref() {
            Some(config) => {
                Some(self.prepare_candidate_account_config(&mut account, config, false)?)
            }
            None if !account.account_key.trim().is_empty() => self
                .credential_store
                .get(&account.account_key)?
                .map(|credential| {
                    self.normalize_credential_structure_for_account(&account, &credential)
                })
                .transpose()?,
            None => None,
        };
        let Some(credential) = credential else {
            return Ok(OfficeAccountOnboardingResult {
                disposition: OfficeAccountOnboardingDisposition::NeedsUserFacts,
                reason: "missing_user_facts".to_string(),
                provider_kind: Some(provider_kind),
                capability: Some(capability),
                missing_fields: assessment.missing_fields,
                missing_field_details: assessment
                    .missing_field_details
                    .iter()
                    .map(build_create_field_from_provider_field)
                    .collect(),
                account: None,
                probe: None,
                error_stage: None,
                error_message: None,
            });
        };

        let probe = match self
            .probe_adapters
            .iter()
            .find(|adapter| adapter.provider_kind() == account.provider_kind)
        {
            Some(adapter) => match adapter.probe(http, &account, &credential) {
                Ok(result) => result,
                Err(error) => {
                    return Ok(OfficeAccountOnboardingResult {
                        disposition: OfficeAccountOnboardingDisposition::ProbeFailed,
                        reason: "probe_error".to_string(),
                        provider_kind: Some(account.provider_kind.clone()),
                        capability: Some(capability),
                        missing_fields: Vec::new(),
                        missing_field_details: Vec::new(),
                        account: None,
                        probe: None,
                        error_stage: Some(error.stage().to_string()),
                        error_message: Some(error.to_string()),
                    })
                }
            },
            None => {
                return Ok(OfficeAccountOnboardingResult {
                    disposition: OfficeAccountOnboardingDisposition::Unsupported,
                    reason: "probe_adapter_unavailable".to_string(),
                    provider_kind: Some(account.provider_kind.clone()),
                    capability: Some(capability),
                    missing_fields: Vec::new(),
                    missing_field_details: Vec::new(),
                    account: None,
                    probe: Some(OfficeProbeResult {
                        account_key: account.account_key.clone(),
                        provider_kind: account.provider_kind.clone(),
                        configured: !credential.access_token.trim().is_empty(),
                        disposition: OfficeProbeDisposition::Unsupported,
                        reason: "probe_adapter_unavailable".to_string(),
                    }),
                    error_stage: None,
                    error_message: None,
                })
            }
        };

        match probe.disposition {
            OfficeProbeDisposition::Ready => {
                let detail =
                    self.commit_onboarded_account(&accounts, account, &credential, &probe)?;
                Ok(OfficeAccountOnboardingResult {
                    disposition: OfficeAccountOnboardingDisposition::Applied,
                    reason: "applied".to_string(),
                    provider_kind: Some(provider_kind),
                    capability: Some(capability),
                    missing_fields: Vec::new(),
                    missing_field_details: Vec::new(),
                    account: Some(detail),
                    probe: Some(probe),
                    error_stage: None,
                    error_message: None,
                })
            }
            OfficeProbeDisposition::MissingCredential => {
                let assessment = assess_office_account(
                    &account,
                    Some(&credential),
                    None,
                    self.probe_supported_for_provider(&account.provider_kind),
                );
                let missing_field_details = assessment
                    .missing_field_details
                    .iter()
                    .map(build_create_field_from_provider_field)
                    .collect::<Vec<_>>();
                Ok(OfficeAccountOnboardingResult {
                    disposition: OfficeAccountOnboardingDisposition::NeedsUserFacts,
                    reason: probe.reason.clone(),
                    provider_kind: Some(provider_kind),
                    capability: Some(capability),
                    missing_fields: assessment.missing_fields,
                    missing_field_details,
                    account: None,
                    probe: Some(probe),
                    error_stage: None,
                    error_message: None,
                })
            }
            OfficeProbeDisposition::Unsupported => Ok(OfficeAccountOnboardingResult {
                disposition: OfficeAccountOnboardingDisposition::Unsupported,
                reason: probe.reason.clone(),
                provider_kind: Some(provider_kind),
                capability: Some(capability),
                missing_fields: Vec::new(),
                missing_field_details: Vec::new(),
                account: None,
                probe: Some(probe),
                error_stage: None,
                error_message: None,
            }),
        }
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
        self.persist_accounts(&accounts)?;
        self.credential_store.set(&normalized_credential)?;
        self.account_detail(account_key)
    }

    pub fn delete_account(&self, account_key: &str) -> Result<()> {
        let mut accounts = self.load_accounts_segment()?;
        accounts.registry.remove(account_key).ok_or_else(|| {
            Error::config(
                "office_config_delete_account",
                format!("unknown office account '{}'", account_key),
            )
        })?;

        self.persist_accounts(&accounts)?;
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
        let mut unavailable_http = UnavailableOfficeHttpClient;
        self.probe_with_http(&mut unavailable_http, account_key)
    }

    pub fn probe_with_http(
        &self,
        http: &mut dyn OfficeHttpClient,
        account_key: &str,
    ) -> Result<OfficeProbeResult> {
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
            let result = adapter.probe(http, &account, &credential)?;
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
        let normalized =
            self.prepare_candidate_account_config(account, request, allow_external_account_id)?;
        self.ensure_credential_ready_for_account(account, &normalized)?;
        Ok(normalized)
    }

    fn prepare_candidate_account_config(
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
        self.normalize_credential_structure_for_account(account, &credential)
    }

    fn normalize_credential_structure_for_account(
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
        Ok(normalized)
    }

    fn ensure_credential_ready_for_account(
        &self,
        account: &OfficeAccount,
        credential: &OfficeCredential,
    ) -> Result<()> {
        let assessment = assess_office_account(account, Some(credential), None, false);
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
        Ok(())
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

    fn commit_onboarded_account(
        &self,
        previous_accounts: &OfficeAccountsSegment,
        account: OfficeAccount,
        credential: &OfficeCredential,
        probe: &OfficeProbeResult,
    ) -> Result<OfficeConfigAccountDetail> {
        let previous_credential = self.credential_store.get(&account.account_key)?;
        let previous_runtime_status = self.runtime_status_store.get(&account.account_key)?;
        let mut next_accounts = previous_accounts.clone();
        upsert_account_segment(&mut next_accounts, account.clone())?;
        self.persist_accounts(&next_accounts)?;
        if let Err(error) = self.credential_store.set(credential) {
            return Err(self.rollback_onboarding_commit(
                previous_accounts,
                &account.account_key,
                previous_credential.as_ref(),
                previous_runtime_status.as_ref(),
                error,
            ));
        }
        let office = match self.build_office_service(&next_accounts) {
            Ok(office) => office,
            Err(error) => {
                return Err(self.rollback_onboarding_commit(
                    previous_accounts,
                    &account.account_key,
                    previous_credential.as_ref(),
                    previous_runtime_status.as_ref(),
                    error,
                ));
            }
        };
        if let Err(error) = self.persist_probe_runtime_status(&office, probe) {
            return Err(self.rollback_onboarding_commit(
                previous_accounts,
                &account.account_key,
                previous_credential.as_ref(),
                previous_runtime_status.as_ref(),
                error,
            ));
        }
        if let Err(error) = self.account_detail(&account.account_key) {
            return Err(self.rollback_onboarding_commit(
                previous_accounts,
                &account.account_key,
                previous_credential.as_ref(),
                previous_runtime_status.as_ref(),
                error,
            ));
        }
        self.account_detail(&account.account_key)
    }

    fn rollback_onboarding_commit(
        &self,
        previous_accounts: &OfficeAccountsSegment,
        account_key: &str,
        previous_credential: Option<&OfficeCredential>,
        previous_runtime_status: Option<&OfficeAccountRuntimeStatus>,
        source_error: Error,
    ) -> Error {
        if let Err(rollback_error) = self.restore_onboarding_state(
            previous_accounts,
            account_key,
            previous_credential,
            previous_runtime_status,
        ) {
            return Error::config(
                "office_config_apply_account_commit",
                format!("{}; rollback_failed={}", source_error, rollback_error),
            );
        }
        Error::config(
            "office_config_apply_account_commit",
            source_error.to_string(),
        )
    }

    fn restore_onboarding_state(
        &self,
        previous_accounts: &OfficeAccountsSegment,
        account_key: &str,
        previous_credential: Option<&OfficeCredential>,
        previous_runtime_status: Option<&OfficeAccountRuntimeStatus>,
    ) -> Result<()> {
        self.persist_accounts(previous_accounts)?;
        match previous_credential {
            Some(credential) => self.credential_store.set(credential)?,
            None => self.credential_store.clear(account_key)?,
        }
        match previous_runtime_status {
            Some(status) => self.runtime_status_store.set(status)?,
            None => self.runtime_status_store.clear(account_key)?,
        }
        Ok(())
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
    let mut account_fields = shared_account_create_fields(&schema);
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

fn shared_account_create_fields(
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
            description: "Account identity class used for office routing and account selection."
                .to_string(),
            value_kind: crate::office::OfficeProviderFieldValueKind::Text,
            required: true,
            secret: false,
            multiple: false,
            default_value: None,
            default_values: Vec::new(),
            options: vec![
                OfficeConfigFieldOption {
                    value: "work".to_string(),
                    label: "Work".to_string(),
                },
                OfficeConfigFieldOption {
                    value: "personal".to_string(),
                    label: "Personal".to_string(),
                },
                OfficeConfigFieldOption {
                    value: "family".to_string(),
                    label: "Family".to_string(),
                },
                OfficeConfigFieldOption {
                    value: "shared".to_string(),
                    label: "Shared".to_string(),
                },
                OfficeConfigFieldOption {
                    value: "other".to_string(),
                    label: "Other".to_string(),
                },
            ],
        },
    ]
}

fn materialize_account_record_input(
    registry: &crate::office::OfficeAccountRegistry,
    input: &OfficeAccountRecordInput,
) -> Result<OfficeAccount> {
    let provider_kind = input.provider_kind.trim().to_string();
    let external_account_id = input.external_account_id.trim().to_string();
    let account_label = input.account_label.trim().to_string();
    let normalized_input = OfficeAccountRecordInput {
        account_key: input.account_key.trim().to_string(),
        provider_kind: provider_kind.clone(),
        external_account_id: external_account_id.clone(),
        account_label: account_label.clone(),
        identity_class: input.identity_class,
        enabled_capabilities: input.enabled_capabilities.clone(),
    };
    let account_key = if !normalized_input.account_key.is_empty() {
        normalized_input.account_key.clone()
    } else if let Some(existing_account_key) =
        existing_account_key_for_natural_identity(registry, &normalized_input)
    {
        existing_account_key
    } else {
        generate_account_key(registry, &normalized_input)?
    };
    Ok(OfficeAccount {
        account_key,
        provider_kind,
        external_account_id,
        account_label,
        identity_class: normalized_input.identity_class,
        enabled_capabilities: normalized_input.enabled_capabilities,
    })
}

fn existing_account_key_for_natural_identity(
    registry: &crate::office::OfficeAccountRegistry,
    input: &OfficeAccountRecordInput,
) -> Option<String> {
    existing_account_key_for_provider_identity(
        registry,
        &input.provider_kind,
        input.external_account_id.trim(),
    )
}

fn existing_account_key_for_provider_identity(
    registry: &crate::office::OfficeAccountRegistry,
    provider_kind: &str,
    external_account_id: &str,
) -> Option<String> {
    if external_account_id.is_empty() {
        return None;
    }
    registry
        .all_accounts()
        .into_iter()
        .find(|account| {
            account.provider_kind == provider_kind
                && account.external_account_id.trim() == external_account_id
        })
        .map(|account| account.account_key.clone())
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

fn provider_selector_field_schema() -> OfficeConfigCreateFieldSchema {
    OfficeConfigCreateFieldSchema {
        key: "provider".to_string(),
        label: "Provider".to_string(),
        description: "Provider family for this office account.".to_string(),
        value_kind: OfficeProviderFieldValueKind::Identifier,
        required: true,
        secret: false,
        multiple: false,
        default_value: None,
        default_values: Vec::new(),
        options: Vec::new(),
    }
}

fn capability_field_schema() -> OfficeConfigCreateFieldSchema {
    OfficeConfigCreateFieldSchema {
        key: "capability".to_string(),
        label: "Capability".to_string(),
        description: "Office capability family for this account.".to_string(),
        value_kind: OfficeProviderFieldValueKind::Identifier,
        required: true,
        secret: false,
        multiple: false,
        default_value: None,
        default_values: Vec::new(),
        options: OfficeCapability::all()
            .into_iter()
            .map(|capability| OfficeConfigFieldOption {
                value: office_capability_key(capability).to_string(),
                label: office_capability_key(capability).to_string(),
            })
            .collect(),
    }
}

fn identity_class_field_schema() -> OfficeConfigCreateFieldSchema {
    shared_account_create_fields(&OfficeProviderSchema {
        provider_kind: String::new(),
        display_name: String::new(),
        capabilities: Vec::new(),
        fields: Vec::new(),
    })
    .into_iter()
    .find(|field| field.key == "identity_class")
    .expect("identity_class field")
}

fn push_missing_field(
    missing_fields: &mut Vec<String>,
    missing_field_details: &mut Vec<OfficeConfigCreateFieldSchema>,
    key: &str,
    field: OfficeConfigCreateFieldSchema,
) {
    if !missing_fields.iter().any(|existing| existing == key) {
        missing_fields.push(key.to_string());
    }
    if !missing_field_details
        .iter()
        .any(|existing| existing.key == key)
    {
        missing_field_details.push(field);
    }
}

fn office_capability_key(capability: OfficeCapability) -> &'static str {
    match capability {
        OfficeCapability::Mail => "mail",
        OfficeCapability::Calendar => "calendar",
        OfficeCapability::Documents => "documents",
        OfficeCapability::ContactsDirectory => "contacts_directory",
    }
}

pub(crate) fn infer_single_capability_for_provider(
    provider_kind: &str,
) -> Option<OfficeCapability> {
    let schema = office_provider_schema(provider_kind)?;
    match schema.capabilities.as_slice() {
        [capability] => Some(*capability),
        _ => None,
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
        crate::office::OfficeConfigNextAction::ConfigureAccount => {
            OfficeConfigCapabilityNextAction::ConfigureAccount
        }
        crate::office::OfficeConfigNextAction::Probe => OfficeConfigCapabilityNextAction::Probe,
        crate::office::OfficeConfigNextAction::None => OfficeConfigCapabilityNextAction::None,
    }
}

fn upsert_account_segment(
    segment: &mut OfficeAccountsSegment,
    account: OfficeAccount,
) -> Result<()> {
    segment.registry.insert(account);
    validate_office_accounts_candidate(segment)
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
    fn delete_account_removes_registry_credentials_and_runtime() {
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
        assert!(snapshot.summary.accounts.is_empty());
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
        let identity_class = imap
            .account_fields
            .iter()
            .find(|field| field.key == "identity_class")
            .expect("provider catalog should expose identity_class in account_fields");
        assert!(
            identity_class.required,
            "identity_class should be required in account_fields"
        );
        assert_eq!(
            identity_class
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            vec!["work", "personal", "family", "shared", "other"],
            "identity_class should expose the shared identity options"
        );
        assert!(
            !imap
                .account_fields
                .iter()
                .any(|field| field.key == "enabled_capabilities"),
            "provider catalog should not expose internal enabled_capabilities in account_fields"
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
    fn materialize_account_record_input_generates_account_key_when_missing() {
        let registry = crate::office::OfficeAccountRegistry::new();
        let account = materialize_account_record_input(
            &registry,
            &OfficeAccountRecordInput {
                account_key: String::new(),
                provider_kind: "imap_smtp".to_string(),
                external_account_id: String::new(),
                account_label: "Primary mail".to_string(),
                identity_class: OfficeAccountIdentityClass::Work,
                enabled_capabilities: vec![OfficeCapability::Mail],
            },
        )
        .expect("materialize account");

        assert_eq!(account.account_key, "imap-smtp-work-primary-mail");
    }

    #[test]
    fn materialize_account_record_input_reuses_existing_account_key_for_natural_identity() {
        let mut registry = crate::office::OfficeAccountRegistry::new();
        registry.insert(OfficeAccount {
            account_key: "imap-smtp-other-675778650-qq-com".to_string(),
            provider_kind: "imap_smtp".to_string(),
            external_account_id: "675778650@qq.com".to_string(),
            account_label: "QQ邮箱".to_string(),
            identity_class: OfficeAccountIdentityClass::Other,
            enabled_capabilities: vec![OfficeCapability::Mail],
        });

        let account = materialize_account_record_input(
            &registry,
            &OfficeAccountRecordInput {
                account_key: String::new(),
                provider_kind: "imap_smtp".to_string(),
                external_account_id: "675778650@qq.com".to_string(),
                account_label: "QQ邮箱（更新）".to_string(),
                identity_class: OfficeAccountIdentityClass::Other,
                enabled_capabilities: vec![OfficeCapability::Mail],
            },
        )
        .expect("materialize account");

        assert_eq!(account.account_key, "imap-smtp-other-675778650-qq-com");
    }

    #[test]
    fn apply_account_returns_missing_user_facts_without_persisting() {
        let service = OfficeConfigManagementService::new(
            Arc::new(MemoryConfigFileStore::new()),
            Arc::new(MemoryCredentialStore::default()),
            Arc::new(MemoryRuntimeStatusStore::default()),
        );

        let result = service
            .apply_account(&OfficeAccountOnboardingRequest {
                provider_kind: Some("imap_smtp".to_string()),
                capability: None,
                external_account_id: Some("work@example.com".to_string()),
                account_label: Some("Work".to_string()),
                identity_class: None,
                config: Some(OfficeAccountConfigSaveRequest {
                    fields: BTreeMap::from([(
                        "access_token".to_string(),
                        "secret-token".to_string(),
                    )]),
                    clear_fields: Vec::new(),
                }),
            })
            .expect("atomic apply should return structured blocker");

        assert_eq!(
            result.disposition,
            OfficeAccountOnboardingDisposition::NeedsUserFacts
        );
        assert_eq!(result.reason, "missing_user_facts");
        assert!(result
            .missing_fields
            .contains(&"identity_class".to_string()));
        assert!(result
            .missing_fields
            .contains(&"mail_imap_host".to_string()));
        assert!(result
            .missing_fields
            .contains(&"mail_smtp_host".to_string()));
        let snapshot = service.inspect().expect("inspect");
        assert!(snapshot.accounts.registry.all_accounts().is_empty());
        assert!(snapshot.summary.accounts.is_empty());
        assert!(service
            .load_credentials_segment()
            .expect("credentials")
            .items
            .is_empty());
        assert!(service
            .runtime_status_store
            .list()
            .expect("runtime status list")
            .is_empty());
    }

    #[test]
    fn apply_account_probe_failure_does_not_persist_partial_state() {
        #[derive(Clone)]
        struct FailingProbeAdapter;

        impl OfficeProbeAdapter for FailingProbeAdapter {
            fn provider_kind(&self) -> &'static str {
                "imap_smtp"
            }

            fn probe(
                &self,
                _http: &mut dyn OfficeHttpClient,
                _account: &OfficeAccount,
                _credential: &OfficeCredential,
            ) -> Result<OfficeProbeResult> {
                Err(Error::config("office_probe_test", "imap login failed"))
            }
        }

        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        let credential_store = Arc::new(MemoryCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service = OfficeConfigManagementService::new(
            config_file_store.clone(),
            credential_store.clone(),
            runtime_status_store.clone(),
        )
        .with_probe_adapters(vec![Arc::new(FailingProbeAdapter)]);

        let result = service
            .apply_account(&OfficeAccountOnboardingRequest {
                provider_kind: Some("imap_smtp".to_string()),
                capability: None,
                external_account_id: Some("work@example.com".to_string()),
                account_label: Some("Work".to_string()),
                identity_class: Some(OfficeAccountIdentityClass::Work),
                config: Some(OfficeAccountConfigSaveRequest {
                    fields: BTreeMap::from([
                        ("access_token".to_string(), "secret-token".to_string()),
                        ("mail_imap_host".to_string(), "imap.example.com".to_string()),
                        ("mail_smtp_host".to_string(), "smtp.example.com".to_string()),
                    ]),
                    clear_fields: Vec::new(),
                }),
            })
            .expect("atomic apply should return structured probe failure");

        assert_eq!(
            result.disposition,
            OfficeAccountOnboardingDisposition::ProbeFailed
        );
        assert_eq!(result.reason, "probe_error");
        assert_eq!(result.error_stage.as_deref(), Some("office_probe_test"));
        assert_eq!(
            result.error_message.as_deref(),
            Some("config: imap login failed (stage: office_probe_test)")
        );
        let snapshot = service.inspect().expect("inspect");
        assert!(snapshot.accounts.registry.all_accounts().is_empty());
        assert!(credential_store.list().expect("credentials").is_empty());
        assert!(runtime_status_store
            .list()
            .expect("runtime status")
            .is_empty());
    }

    #[test]
    fn apply_account_probe_success_commits_account_credential_and_runtime_status() {
        #[derive(Clone)]
        struct ReadyProbeAdapter;

        impl OfficeProbeAdapter for ReadyProbeAdapter {
            fn provider_kind(&self) -> &'static str {
                "imap_smtp"
            }

            fn probe(
                &self,
                _http: &mut dyn OfficeHttpClient,
                account: &OfficeAccount,
                _credential: &OfficeCredential,
            ) -> Result<OfficeProbeResult> {
                Ok(OfficeProbeResult {
                    account_key: account.account_key.clone(),
                    provider_kind: account.provider_kind.clone(),
                    configured: true,
                    disposition: OfficeProbeDisposition::Ready,
                    reason: "imap_login_ok".to_string(),
                })
            }
        }

        let config_file_store = Arc::new(MemoryConfigFileStore::new());
        let credential_store = Arc::new(MemoryCredentialStore::default());
        let runtime_status_store = Arc::new(MemoryRuntimeStatusStore::default());
        let service = OfficeConfigManagementService::new(
            config_file_store.clone(),
            credential_store.clone(),
            runtime_status_store.clone(),
        )
        .with_probe_adapters(vec![Arc::new(ReadyProbeAdapter)]);

        let result = service
            .apply_account(&OfficeAccountOnboardingRequest {
                provider_kind: Some("imap_smtp".to_string()),
                capability: None,
                external_account_id: Some("work@example.com".to_string()),
                account_label: Some("Work".to_string()),
                identity_class: Some(OfficeAccountIdentityClass::Work),
                config: Some(OfficeAccountConfigSaveRequest {
                    fields: BTreeMap::from([
                        ("access_token".to_string(), "secret-token".to_string()),
                        ("mail_imap_host".to_string(), "imap.example.com".to_string()),
                        ("mail_smtp_host".to_string(), "smtp.example.com".to_string()),
                    ]),
                    clear_fields: Vec::new(),
                }),
            })
            .expect("atomic apply should succeed");

        assert_eq!(
            result.disposition,
            OfficeAccountOnboardingDisposition::Applied
        );
        assert_eq!(result.reason, "applied");
        let detail = result.account.as_ref().expect("applied detail");
        let account_key = detail.account.account_key.as_str();
        let credential = credential_store
            .get(account_key)
            .expect("load credential")
            .expect("credential exists");
        assert_eq!(credential.access_token, "secret-token");
        assert_eq!(
            credential.metadata_value("mail_imap_host"),
            Some("imap.example.com")
        );
        assert_eq!(
            credential.metadata_value("mail_smtp_host"),
            Some("smtp.example.com")
        );
        let runtime = runtime_status_store
            .get(account_key)
            .expect("load runtime")
            .expect("runtime exists");
        assert!(runtime.probe_ok);
        assert!(runtime.last_error.is_empty());
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
                _http: &mut dyn OfficeHttpClient,
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
            OfficeConfigReadiness::NeedsConfiguration
        );
        assert_eq!(
            assessment.next_action,
            OfficeConfigNextAction::ConfigureAccount
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
