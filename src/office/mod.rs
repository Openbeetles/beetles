//! Office capability domain: accounts, bindings, policies, and resolver.

mod account;
mod binding;
mod config_management;
mod credentials;
mod policy;
mod resolver;
mod service;
mod status;

pub use account::{
    OfficeAccount, OfficeAccountIdentityClass, OfficeAccountRegistry, OfficeCapability,
};
pub use binding::OfficeCapabilityBinding;
pub use config_management::{
    OfficeAccountDraftRequest, OfficeConfigManagementService, OfficeConfigSnapshot,
    OfficeCredentialDraftRequest, OfficePolicyPatch, OfficeProbeAdapter, OfficeProbeDisposition,
    OfficeProbeResult,
};
pub use credentials::{
    OfficeCredential, OfficeCredentialStatus, OfficeCredentialStore, OfficeCredentialsSegment,
    OFFICE_METADATA_CALENDAR_ID, REL_PATH_OFFICE_CREDENTIALS,
};
pub use policy::OfficeSelectionPolicy;
pub use resolver::{
    OfficeResolveRequest, OfficeResolveResult, OfficeResolveSelection, OfficeResolver,
};
pub use service::{
    OfficeAccountAuthorityStatus, OfficeAuthoritySummary, OfficeCapabilityDefault, OfficeService,
};
pub use status::{
    OfficeAccountRuntimeStatus, OfficeAccountStatusSummary, OfficeRuntimeStatusStore,
    REL_PATH_OFFICE_RUNTIME_STATUS,
};
