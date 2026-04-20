//! Shared office tool doctrine truth for LLM-facing routing and operation priority.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficeToolRole {
    StatusEntry,
    ManagementEntry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficeToolDoctrine {
    pub tool_name: &'static str,
    pub role: OfficeToolRole,
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
        description:
            "Inspect office account status, readiness, diagnostics, and routing. Use this first when you need to understand whether configured office accounts are ready or need repair.",
    },
    OfficeToolDoctrine {
        tool_name: "office_config",
        role: OfficeToolRole::ManagementEntry,
        description:
            "Configure, reconfigure, and repair shared office accounts for mail, calendar, documents, and contacts. Mainline: use provider_schema first, then apply_account, and use resolve_account when account routing is ambiguous. Advanced and repair paths include probe, revoke, inspect, and assess.",
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
