//! Office capability domain: accounts, policies, and resolver.

mod account;
#[cfg(feature = "capability_office")]
mod assessment;
#[cfg(feature = "capability_office")]
mod authority_source;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod capability_runtime;
#[cfg(feature = "capability_office")]
mod config_management;
mod credentials;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod google_api;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod integration_topology;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod microsoft_graph;
mod policy;
#[cfg(feature = "capability_office")]
mod provider_schema;
#[cfg(feature = "capability_office")]
mod public_contract;
#[cfg(feature = "capability_office")]
mod resolver;
#[cfg(feature = "capability_office")]
mod service;
mod status;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod transport;
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
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use capability_runtime::{
    OfficeCapabilityCredentialAccess, OfficeCapabilityRemoteRuntime, OfficeCapabilityRuntime,
    OfficeSelectedRoute,
};
#[cfg(feature = "capability_office")]
pub(crate) use config_management::infer_single_capability_for_provider;
#[cfg(feature = "capability_office")]
pub use config_management::{
    OfficeAccountConfigSaveRequest, OfficeAccountOnboardingDisposition,
    OfficeAccountOnboardingRequest, OfficeAccountOnboardingResult, OfficeAccountRecordInput,
    OfficeConfigAccountDetail, OfficeConfigAccountSummary, OfficeConfigCapabilityNextAction,
    OfficeConfigCapabilitySelectionStatus, OfficeConfigCapabilityStatus,
    OfficeConfigCreateFieldSchema, OfficeConfigFieldOption, OfficeConfigFieldState,
    OfficeConfigManagementService, OfficeConfigProviderCatalogItem, OfficeConfigSnapshot,
    OfficeProbeAdapter, OfficeProbeDisposition, OfficeProbeResult,
};
pub use credentials::{
    OfficeCredential, OfficeCredentialStatus, OfficeCredentialStore, OfficeCredentialsSegment,
    OFFICE_METADATA_CALENDAR_ID, REL_PATH_OFFICE_CREDENTIALS,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use google_api::{
    build_google_api_url, normalize_google_api_base_url, parse_google_api_json,
    request_google_api_empty, request_google_api_empty_ureq, request_google_api_json,
    request_google_api_json_ureq, GoogleApiErrorEnvelope, GoogleApiListEnvelope,
    GOOGLE_CALENDAR_DEFAULT_BASE_URL, GOOGLE_DRIVE_DEFAULT_BASE_URL, GOOGLE_GMAIL_DEFAULT_BASE_URL,
    GOOGLE_PEOPLE_DEFAULT_BASE_URL,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use integration_topology::build_default_office_integration_topology;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use microsoft_graph::{
    build_microsoft_graph_url, normalize_microsoft_graph_base_url, parse_microsoft_graph_json,
    request_microsoft_graph_empty, request_microsoft_graph_empty_ureq,
    request_microsoft_graph_json, request_microsoft_graph_json_ureq, MicrosoftGraphCollection,
    MicrosoftGraphErrorEnvelope, MICROSOFT_GRAPH_DEFAULT_BASE_URL,
};
pub use policy::OfficeSelectionPolicy;
#[cfg(feature = "capability_office")]
pub use provider_schema::{
    office_provider_schema, office_provider_schemas, OfficeProviderFieldLocation,
    OfficeProviderFieldSchema, OfficeProviderFieldValueKind, OfficeProviderSchema,
};
#[cfg(feature = "capability_office")]
pub(crate) use public_contract::parse_public_account_upsert_request_value;
#[cfg(feature = "capability_office")]
pub use resolver::{
    OfficeResolveAmbiguity, OfficeResolveAmbiguityReason, OfficeResolveCandidate,
    OfficeResolveMissing, OfficeResolveMissingReason, OfficeResolveRequest, OfficeResolveResult,
    OfficeResolveSelection, OfficeResolveSelectionReason, OfficeResolver,
};
#[cfg(feature = "capability_office")]
pub use service::{OfficeAccountAuthorityStatus, OfficeAuthoritySummary, OfficeService};
pub use status::{
    OfficeAccountRuntimeStatus, OfficeAccountStatusSummary, OfficeRuntimeStatusStore,
    REL_PATH_OFFICE_RUNTIME_STATUS,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use transport::{
    read_bounded_http_bytes, OfficeBoundedBytes, OfficeHttpClient, OfficeStreamingResponse,
    UnavailableOfficeHttpClient,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use wecom::{
    fetch_wecom_access_token, fetch_wecom_access_token_ureq, request_wecom_json,
    request_wecom_json_ureq, WecomApiEnvelope, WecomAuthCredential, WecomTokenPayload,
    WECOM_DEFAULT_BASE_URL,
};
