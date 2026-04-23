//! Office capability domain: accounts, policies, and resolver.

mod account;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod assessment;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod authority_source;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod capability_runtime;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
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
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) mod office_refactor_helpers;
mod policy;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod provider_schema;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod public_contract;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod resolver;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
mod service;
mod status;
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
mod tool_doctrine;
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
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use assessment::{
    assess_office_account, OfficeAccountAssessment, OfficeConfigAssessment, OfficeConfigNextAction,
    OfficeConfigReadiness,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use authority_source::{
    OfficeAuthoritySource, ReloadingOfficeAuthoritySource, SnapshotOfficeAuthoritySource,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use capability_runtime::{
    office_authority_from_service, OfficeAuthorityBackedCredentialStoreCore,
    OfficeCapabilityCredentialAccess, OfficeCapabilityRemoteRuntime, OfficeCapabilityRuntime,
    OfficeCapabilityServiceCore, OfficeSelectedRoute,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use config_management::infer_single_capability_for_provider;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
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
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use provider_schema::{
    office_provider_display_name_key, office_provider_schema, office_provider_schemas,
    OfficeProviderFieldLocation, OfficeProviderFieldSchema, OfficeProviderFieldValueKind,
    OfficeProviderSchema,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use public_contract::parse_public_account_upsert_request_value;
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use resolver::{
    OfficeResolveAmbiguity, OfficeResolveAmbiguityReason, OfficeResolveCandidate,
    OfficeResolveMissing, OfficeResolveMissingReason, OfficeResolveRequest, OfficeResolveResult,
    OfficeResolveSelection, OfficeResolveSelectionReason, OfficeResolver,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub use service::{OfficeAccountAuthorityStatus, OfficeAuthoritySummary, OfficeService};
pub use status::{
    OfficeAccountRuntimeStatus, OfficeAccountStatusSummary, OfficeRuntimeStatusStore,
    REL_PATH_OFFICE_RUNTIME_STATUS,
};
#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
pub use tool_doctrine::{
    office_config_op_doctrine, office_config_op_doctrines, office_tool_doctrine,
    office_tool_doctrines, OfficeConfigOpDoctrine, OfficeConfigOpTier, OfficeToolDoctrine,
    OfficeToolLlmSurface, OfficeToolProtocolProfile, OfficeToolRole,
};
#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
pub(crate) use transport::as_office_http_client;
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

#[cfg(all(test, not(any(target_arch = "xtensa", target_arch = "riscv32"))))]
mod tool_doctrine_tests {
    use super::{
        office_config_op_doctrine, office_tool_doctrine, office_tool_doctrines, OfficeCapability,
        OfficeConfigOpTier, OfficeToolLlmSurface, OfficeToolProtocolProfile, OfficeToolRole,
    };

    #[test]
    fn office_status_is_the_unique_status_entrypoint() {
        let office_status =
            office_tool_doctrine("office_status").expect("office_status doctrine must exist");
        let office_config =
            office_tool_doctrine("office_config").expect("office_config doctrine must exist");

        assert_eq!(office_status.role, OfficeToolRole::StatusEntry);
        assert_eq!(office_config.role, OfficeToolRole::ManagementEntry);
        assert_eq!(
            office_status.llm_surface,
            OfficeToolLlmSurface::UserAndSystem
        );
        assert_eq!(
            office_status.protocol_profile,
            OfficeToolProtocolProfile::StructuredObjectRich
        );
        assert_eq!(office_config.llm_surface, OfficeToolLlmSurface::UserOnly);
        assert_eq!(
            office_config.protocol_profile,
            OfficeToolProtocolProfile::OperationEnvelopeRich
        );
    }

    #[test]
    fn office_capability_tools_share_one_surface_authority() {
        let capability_tools = office_tool_doctrines()
            .iter()
            .filter_map(|doctrine| match doctrine.role {
                OfficeToolRole::CapabilityEntry(capability) => Some((
                    doctrine.tool_name,
                    capability,
                    doctrine.llm_surface,
                    doctrine.protocol_profile,
                )),
                OfficeToolRole::StatusEntry | OfficeToolRole::ManagementEntry => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            capability_tools,
            vec![
                (
                    "mail",
                    OfficeCapability::Mail,
                    OfficeToolLlmSurface::UserOnly,
                    OfficeToolProtocolProfile::OperationEnvelopeRich,
                ),
                (
                    "calendar",
                    OfficeCapability::Calendar,
                    OfficeToolLlmSurface::UserOnly,
                    OfficeToolProtocolProfile::OperationEnvelopeRich,
                ),
                (
                    "documents",
                    OfficeCapability::Documents,
                    OfficeToolLlmSurface::UserOnly,
                    OfficeToolProtocolProfile::OperationEnvelopeRich,
                ),
                (
                    "contacts_directory",
                    OfficeCapability::ContactsDirectory,
                    OfficeToolLlmSurface::UserOnly,
                    OfficeToolProtocolProfile::OperationEnvelopeRich,
                ),
            ]
        );
    }

    #[test]
    fn office_config_keeps_full_management_ops_but_marks_mainline_priority() {
        let provider_schema = office_config_op_doctrine("provider_schema")
            .expect("provider_schema doctrine must exist");
        let apply_account =
            office_config_op_doctrine("apply_account").expect("apply_account doctrine must exist");
        let resolve_account = office_config_op_doctrine("resolve_account")
            .expect("resolve_account doctrine must exist");
        let inspect = office_config_op_doctrine("inspect").expect("inspect doctrine must exist");
        let assess = office_config_op_doctrine("assess").expect("assess doctrine must exist");
        let probe = office_config_op_doctrine("probe").expect("probe doctrine must exist");
        let revoke = office_config_op_doctrine("revoke").expect("revoke doctrine must exist");

        assert_eq!(provider_schema.tier, OfficeConfigOpTier::Mainline);
        assert!(provider_schema.llm_primary);
        assert_eq!(apply_account.tier, OfficeConfigOpTier::Mainline);
        assert!(apply_account.llm_primary);
        assert_eq!(resolve_account.tier, OfficeConfigOpTier::Mainline);
        assert!(resolve_account.llm_primary);

        assert_eq!(inspect.tier, OfficeConfigOpTier::Advanced);
        assert!(!inspect.llm_primary);
        assert_eq!(assess.tier, OfficeConfigOpTier::Advanced);
        assert!(!assess.llm_primary);
        assert_eq!(probe.tier, OfficeConfigOpTier::Repair);
        assert!(!probe.llm_primary);
        assert_eq!(revoke.tier, OfficeConfigOpTier::Repair);
        assert!(!revoke.llm_primary);
    }
}
