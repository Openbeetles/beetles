//! Office capability domain: accounts, bindings, policies, and resolver.

mod account;
#[cfg(feature = "capability_office")]
mod assessment;
#[cfg(feature = "capability_office")]
mod authority_source;
mod binding;
#[cfg(feature = "capability_office")]
mod config_management;
mod credentials;
mod policy;
#[cfg(feature = "capability_office")]
mod provider_schema;
#[cfg(feature = "capability_office")]
mod resolver;
#[cfg(feature = "capability_office")]
mod service;
mod status;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod wecom;

pub use account::{
    OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
};
#[cfg(feature = "capability_office")]
pub use assessment::{
    assess_office_account, OfficeAccountAssessment, OfficeConfigAssessment, OfficeConfigNextAction,
    OfficeConfigReadiness,
};
#[cfg(feature = "capability_office")]
pub use authority_source::{
    OfficeAuthoritySource, ReloadingOfficeAuthoritySource, SnapshotOfficeAuthoritySource,
};
pub use binding::OfficeCapabilityBinding;
#[cfg(feature = "capability_office")]
pub use config_management::{
    OfficeAccountConfigSaveRequest, OfficeAccountDraftRequest, OfficeConfigAccountDetail,
    OfficeConfigAccountSummary, OfficeConfigCapabilityNextAction,
    OfficeConfigCapabilitySelectionStatus, OfficeConfigCapabilityStatus, OfficeConfigFieldState,
    OfficeConfigManagementService, OfficeConfigSnapshot, OfficeCredentialDraftRequest,
    OfficePolicyPatch, OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
pub use credentials::{
    OfficeCredential, OfficeCredentialStatus, OfficeCredentialStore, OfficeCredentialsSegment,
    OFFICE_METADATA_CALENDAR_ID, REL_PATH_OFFICE_CREDENTIALS,
};
pub use policy::OfficeSelectionPolicy;
#[cfg(feature = "capability_office")]
pub use provider_schema::{
    office_provider_schema, office_provider_schemas, OfficeProviderFieldLocation,
    OfficeProviderFieldSchema, OfficeProviderFieldValueKind, OfficeProviderSchema,
};
#[cfg(feature = "capability_office")]
pub use resolver::{
    OfficeResolveRequest, OfficeResolveResult, OfficeResolveSelection, OfficeResolver,
};
#[cfg(feature = "capability_office")]
pub use service::{
    OfficeAccountAuthorityStatus, OfficeAuthoritySummary, OfficeCapabilityDefault, OfficeService,
};
pub use status::{
    OfficeAccountRuntimeStatus, OfficeAccountStatusSummary, OfficeRuntimeStatusStore,
    REL_PATH_OFFICE_RUNTIME_STATUS,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use wecom::{
    fetch_wecom_access_token_ureq, request_wecom_json_ureq, WecomApiEnvelope, WecomAuthCredential,
    WecomTokenPayload, WECOM_DEFAULT_BASE_URL,
};
