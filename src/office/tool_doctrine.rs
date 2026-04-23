//! Shared office tool doctrine truth for LLM-facing routing and operation priority.

use super::account::OfficeCapability;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeToolRole {
    StatusEntry,
    ManagementEntry,
    CapabilityEntry(OfficeCapability),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeToolLlmSurface {
    UserOnly,
    UserAndSystem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeToolProtocolProfile {
    StructuredObjectRich,
    OperationEnvelopeRich,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeToolDoctrine {
    pub tool_name: &'static str,
    pub role: OfficeToolRole,
    pub llm_surface: OfficeToolLlmSurface,
    pub protocol_profile: OfficeToolProtocolProfile,
    pub description: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeConfigOpTier {
    Mainline,
    Repair,
    Advanced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeConfigOpDoctrine {
    pub op: &'static str,
    pub tier: OfficeConfigOpTier,
    pub llm_primary: bool,
    pub description: &'static str,
}

const OFFICE_TOOL_DOCTRINES: &[OfficeToolDoctrine] = &[
    OfficeToolDoctrine {
        tool_name: "office_status",
        role: OfficeToolRole::StatusEntry,
        llm_surface: OfficeToolLlmSurface::UserAndSystem,
        protocol_profile: OfficeToolProtocolProfile::StructuredObjectRich,
        description:
            "Inspect office account status, readiness, diagnostics, and routing. Use this first when you need to understand whether configured office accounts are ready or need repair.",
    },
    OfficeToolDoctrine {
        tool_name: "office_config",
        role: OfficeToolRole::ManagementEntry,
        llm_surface: OfficeToolLlmSurface::UserOnly,
        protocol_profile: OfficeToolProtocolProfile::OperationEnvelopeRich,
        description:
            "Configure, reconfigure, and repair shared office accounts for mail, calendar, documents, and contacts. Mainline: use provider_schema first, then apply_account, and use resolve_account when account routing is ambiguous. Advanced and repair paths include probe, revoke, inspect, and assess.",
    },
    OfficeToolDoctrine {
        tool_name: "mail",
        role: OfficeToolRole::CapabilityEntry(OfficeCapability::Mail),
        llm_surface: OfficeToolLlmSurface::UserOnly,
        protocol_profile: OfficeToolProtocolProfile::OperationEnvelopeRich,
        description:
            "Use configured office mail accounts for list, search, read, send, and draft operations once routing is known.",
    },
    OfficeToolDoctrine {
        tool_name: "calendar",
        role: OfficeToolRole::CapabilityEntry(OfficeCapability::Calendar),
        llm_surface: OfficeToolLlmSurface::UserOnly,
        protocol_profile: OfficeToolProtocolProfile::OperationEnvelopeRich,
        description:
            "Use local or configured office calendar accounts for list, read, create, update, and delete operations once routing is known.",
    },
    OfficeToolDoctrine {
        tool_name: "documents",
        role: OfficeToolRole::CapabilityEntry(OfficeCapability::Documents),
        llm_surface: OfficeToolLlmSurface::UserOnly,
        protocol_profile: OfficeToolProtocolProfile::OperationEnvelopeRich,
        description:
            "Use configured office document libraries for list, read, summarize, and search operations once routing is known.",
    },
    OfficeToolDoctrine {
        tool_name: "contacts_directory",
        role: OfficeToolRole::CapabilityEntry(OfficeCapability::ContactsDirectory),
        llm_surface: OfficeToolLlmSurface::UserOnly,
        protocol_profile: OfficeToolProtocolProfile::OperationEnvelopeRich,
        description:
            "Use local or configured office contacts sources for lookup and directory operations that support people-aware routing.",
    },
];

const OFFICE_CONFIG_OP_DOCTRINES: &[OfficeConfigOpDoctrine] = &[
    OfficeConfigOpDoctrine {
        op: "provider_schema",
        tier: OfficeConfigOpTier::Mainline,
        llm_primary: true,
        description: "Inspect provider-family onboarding requirements before configuring an account.",
    },
    OfficeConfigOpDoctrine {
        op: "apply_account",
        tier: OfficeConfigOpTier::Mainline,
        llm_primary: true,
        description: "Apply onboarding or reconfiguration through the atomic office account path.",
    },
    OfficeConfigOpDoctrine {
        op: "resolve_account",
        tier: OfficeConfigOpTier::Mainline,
        llm_primary: true,
        description: "Resolve configured-account ambiguity when a capability call cannot choose an account.",
    },
    OfficeConfigOpDoctrine {
        op: "probe",
        tier: OfficeConfigOpTier::Repair,
        llm_primary: false,
        description: "Probe an existing office account when repair or deep verification is explicitly required.",
    },
    OfficeConfigOpDoctrine {
        op: "revoke",
        tier: OfficeConfigOpTier::Repair,
        llm_primary: false,
        description: "Revoke an existing office account when the user explicitly wants to disconnect it.",
    },
    OfficeConfigOpDoctrine {
        op: "inspect",
        tier: OfficeConfigOpTier::Advanced,
        llm_primary: false,
        description: "Inspect the full persisted office configuration snapshot for advanced/operator paths.",
    },
    OfficeConfigOpDoctrine {
        op: "assess",
        tier: OfficeConfigOpTier::Advanced,
        llm_primary: false,
        description: "Assess stored accounts and readiness in advanced/operator troubleshooting paths.",
    },
];

pub fn office_tool_doctrines() -> &'static [OfficeToolDoctrine] {
    OFFICE_TOOL_DOCTRINES
}

pub fn office_tool_doctrine(tool_name: &str) -> Option<&'static OfficeToolDoctrine> {
    OFFICE_TOOL_DOCTRINES
        .iter()
        .find(|doctrine| doctrine.tool_name == tool_name)
}

pub fn office_config_op_doctrines() -> &'static [OfficeConfigOpDoctrine] {
    OFFICE_CONFIG_OP_DOCTRINES
}

pub fn office_config_op_doctrine(op: &str) -> Option<&'static OfficeConfigOpDoctrine> {
    OFFICE_CONFIG_OP_DOCTRINES
        .iter()
        .find(|doctrine| doctrine.op == op)
}
