//! LLM-facing tool catalog authority.
//! LLM 工具目录真源：集中声明各入口可见面，而不是让 ToolMetadata 兼职承担。

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolLlmVisibility {
    pub user_llm: bool,
    pub system_llm: bool,
    pub internal_system_llm: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolInputProtocolKind {
    StructuredObject,
    OperationEnvelope,
}

impl ToolInputProtocolKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::StructuredObject => "structured_object",
            Self::OperationEnvelope => "operation_envelope",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOutputProtocolKind {
    PlainText,
    StructuredJson,
    StructuredJsonWithOutbound,
}

impl ToolOutputProtocolKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PlainText => "plain_text",
            Self::StructuredJson => "structured_json",
            Self::StructuredJsonWithOutbound => "structured_json_with_outbound",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolProtocolContract {
    pub input_kind: ToolInputProtocolKind,
    pub output_kind: ToolOutputProtocolKind,
    pub supports_rich_blockers: bool,
}

impl ToolProtocolContract {
    pub const fn structured_object_json() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::StructuredObject,
            output_kind: ToolOutputProtocolKind::StructuredJson,
            supports_rich_blockers: false,
        }
    }

    pub const fn structured_object_json_with_rich_blockers() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::StructuredObject,
            output_kind: ToolOutputProtocolKind::StructuredJson,
            supports_rich_blockers: true,
        }
    }

    pub const fn structured_object_plain_text() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::StructuredObject,
            output_kind: ToolOutputProtocolKind::PlainText,
            supports_rich_blockers: false,
        }
    }

    pub const fn structured_object_json_with_outbound() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::StructuredObject,
            output_kind: ToolOutputProtocolKind::StructuredJsonWithOutbound,
            supports_rich_blockers: false,
        }
    }

    pub const fn structured_object_json_with_outbound_and_rich_blockers() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::StructuredObject,
            output_kind: ToolOutputProtocolKind::StructuredJsonWithOutbound,
            supports_rich_blockers: true,
        }
    }

    pub const fn operation_envelope_json() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::OperationEnvelope,
            output_kind: ToolOutputProtocolKind::StructuredJson,
            supports_rich_blockers: false,
        }
    }

    pub const fn operation_envelope_json_with_rich_blockers() -> Self {
        Self {
            input_kind: ToolInputProtocolKind::OperationEnvelope,
            output_kind: ToolOutputProtocolKind::StructuredJson,
            supports_rich_blockers: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolProtocolAuthority {
    entries: BTreeMap<String, ToolProtocolContract>,
}

impl ToolProtocolAuthority {
    pub fn with_entry(mut self, tool_name: &str, contract: ToolProtocolContract) -> Self {
        self.insert(tool_name, contract);
        self
    }

    pub fn insert(&mut self, tool_name: &str, contract: ToolProtocolContract) {
        self.entries.insert(tool_name.to_string(), contract);
    }

    pub fn get(&self, tool_name: &str) -> Option<ToolProtocolContract> {
        self.entries.get(tool_name).copied()
    }

    pub fn has_entry(&self, tool_name: &str) -> bool {
        self.entries.contains_key(tool_name)
    }

    pub fn missing_entries<'a>(
        &self,
        tool_names: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let mut missing = tool_names
            .into_iter()
            .filter(|name| !self.has_entry(name))
            .map(str::to_string)
            .collect::<Vec<_>>();
        missing.sort();
        missing
    }
}

impl ToolLlmVisibility {
    pub const fn new(user_llm: bool, system_llm: bool, internal_system_llm: bool) -> Self {
        Self {
            user_llm,
            system_llm,
            internal_system_llm,
        }
    }

    pub const fn hidden() -> Self {
        Self::new(false, false, false)
    }

    pub const fn user_only() -> Self {
        Self::new(true, false, false)
    }

    pub const fn user_and_system() -> Self {
        Self::new(true, true, false)
    }

    pub const fn system_and_internal() -> Self {
        Self::new(false, true, true)
    }

    pub const fn internal_only() -> Self {
        Self::new(false, false, true)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolCatalogAuthority {
    entries: BTreeMap<String, ToolLlmVisibility>,
}

impl ToolCatalogAuthority {
    pub fn with_entry(mut self, tool_name: &str, visibility: ToolLlmVisibility) -> Self {
        self.insert(tool_name, visibility);
        self
    }

    pub fn insert(&mut self, tool_name: &str, visibility: ToolLlmVisibility) {
        self.entries.insert(tool_name.to_string(), visibility);
    }

    pub fn get(&self, tool_name: &str) -> Option<ToolLlmVisibility> {
        self.entries.get(tool_name).copied()
    }

    pub fn has_entry(&self, tool_name: &str) -> bool {
        self.entries.contains_key(tool_name)
    }

    pub fn missing_entries<'a>(
        &self,
        tool_names: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let mut missing = tool_names
            .into_iter()
            .filter(|name| !self.has_entry(name))
            .map(str::to_string)
            .collect::<Vec<_>>();
        missing.sort();
        missing
    }
}

fn insert_many(
    authority: &mut ToolCatalogAuthority,
    visibility: ToolLlmVisibility,
    tool_names: &[&str],
) {
    for name in tool_names {
        authority.insert(name, visibility);
    }
}

fn insert_many_protocol(
    authority: &mut ToolProtocolAuthority,
    contract: ToolProtocolContract,
    tool_names: &[&str],
) {
    for name in tool_names {
        authority.insert(name, contract);
    }
}

fn populate_core_tool_catalog(authority: &mut ToolCatalogAuthority) {
    insert_many(
        authority,
        ToolLlmVisibility::user_and_system(),
        &[
            "get_time",
            "document_search",
            "document_read",
            "document_extract",
            "web_search",
            "analyze_image",
            "board_info",
            "diagnose_delivery",
            "diagnose_system",
            "diagnose_network_path",
            "factual_memory",
            "memory_search",
            "memory_get",
            "diagnose_memory_runtime",
            "diagnose_voice_path",
        ],
    );
    insert_many(
        authority,
        ToolLlmVisibility::user_only(),
        &[
            "message",
            "task",
            "remind_at",
            "remind_list",
            "device_control",
        ],
    );
    authority.insert("private_garden", ToolLlmVisibility::system_and_internal());
    insert_many(
        authority,
        ToolLlmVisibility::hidden(),
        &[
            "env",
            "files",
            "file_edit",
            "web_fetch",
            "pdf_read",
            "kv_store",
            "continuity_snapshot",
        ],
    );
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn populate_office_tool_catalog(authority: &mut ToolCatalogAuthority) {
    insert_many(
        authority,
        ToolLlmVisibility::user_only(),
        &[
            "calendar",
            "mail",
            "contacts_directory",
            "documents",
            "office_config",
        ],
    );
    authority.insert("office_status", ToolLlmVisibility::user_and_system());
}

fn populate_extended_runtime_tool_catalog(authority: &mut ToolCatalogAuthority) {
    authority.insert("network_scan", ToolLlmVisibility::user_only());
    insert_many(
        authority,
        ToolLlmVisibility::user_only(),
        &["sensor_watch", "i2c_sensor"],
    );
    insert_many(
        authority,
        ToolLlmVisibility::hidden(),
        &[
            "memory_manage",
            "http_request",
            "session_manage",
            "file_write",
            "system_control",
            "cron_manage",
            "proxy_config",
            "model_config",
            "i2c_device",
        ],
    );
}

fn populate_audio_tool_catalog(authority: &mut ToolCatalogAuthority) {
    insert_many(
        authority,
        ToolLlmVisibility::user_only(),
        &["voice_input", "voice_output"],
    );
}

#[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
fn populate_host_only_tool_catalog(authority: &mut ToolCatalogAuthority) {
    insert_many(
        authority,
        ToolLlmVisibility::hidden(),
        &[
            "shell",
            "process",
            "network",
            "lua_query",
            "lua_datasheet_distill",
            "lua_protocol_frame_helper",
            "lua_register_table_helper",
            "lua_state_machine_checker",
            "lua_memory_query",
            "lua_tool_bridge",
            "capability_atoms_exchange",
            "capability_atoms_inspect",
        ],
    );
}

pub fn build_default_llm_catalog_authority() -> ToolCatalogAuthority {
    let mut authority = ToolCatalogAuthority::default();
    populate_core_tool_catalog(&mut authority);
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    populate_office_tool_catalog(&mut authority);
    populate_extended_runtime_tool_catalog(&mut authority);
    populate_audio_tool_catalog(&mut authority);
    #[cfg(not(any(target_arch = "xtensa", target_arch = "riscv32")))]
    populate_host_only_tool_catalog(&mut authority);
    authority
}

fn populate_core_tool_protocol_authority(authority: &mut ToolProtocolAuthority) {
    insert_many_protocol(
        authority,
        ToolProtocolContract::structured_object_json(),
        &[
            "board_info",
            "diagnose_delivery",
            "diagnose_system",
            "diagnose_network_path",
            "factual_memory",
            "memory_search",
            "memory_get",
            "diagnose_memory_runtime",
            "diagnose_voice_path",
            "document_extract",
            "analyze_image",
            "device_control",
            "office_status",
            "voice_output",
            "sensor_watch",
            "i2c_sensor",
            "lua_query",
            "lua_memory_query",
            "lua_tool_bridge",
            "lua_datasheet_distill",
            "lua_register_table_helper",
            "lua_protocol_frame_helper",
            "lua_state_machine_checker",
        ],
    );
    insert_many_protocol(
        authority,
        ToolProtocolContract::structured_object_json_with_rich_blockers(),
        &[
            "web_search",
            "document_search",
            "document_read",
            "web_fetch",
        ],
    );
    insert_many_protocol(
        authority,
        ToolProtocolContract::structured_object_plain_text(),
        &["get_time", "voice_input", "shell"],
    );
    authority.insert(
        "message",
        ToolProtocolContract::structured_object_json_with_outbound_and_rich_blockers(),
    );
    insert_many_protocol(
        authority,
        ToolProtocolContract::operation_envelope_json(),
        &[
            "env",
            "files",
            "private_garden",
            "process",
            "network",
            "network_scan",
            "cron_manage",
            "memory_manage",
            "session_manage",
            "system_control",
            "proxy_config",
            "model_config",
            "kv_store",
            "continuity_snapshot",
            "capability_atoms_exchange",
            "capability_atoms_inspect",
        ],
    );
    insert_many_protocol(
        authority,
        ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
        &["task", "remind_at"],
    );
    insert_many_protocol(
        authority,
        ToolProtocolContract::structured_object_json(),
        &[
            "file_edit",
            "file_write",
            "pdf_read",
            "http_request",
            "remind_list",
            "i2c_device",
        ],
    );
}

#[cfg(all(
    feature = "capability_office",
    not(any(target_arch = "xtensa", target_arch = "riscv32"))
))]
fn populate_office_tool_protocol_authority(authority: &mut ToolProtocolAuthority) {
    insert_many_protocol(
        authority,
        ToolProtocolContract::operation_envelope_json_with_rich_blockers(),
        &[
            "office_config",
            "mail",
            "calendar",
            "documents",
            "contacts_directory",
        ],
    );
}

pub fn build_default_tool_protocol_authority() -> ToolProtocolAuthority {
    let mut authority = ToolProtocolAuthority::default();
    populate_core_tool_protocol_authority(&mut authority);
    #[cfg(all(
        feature = "capability_office",
        not(any(target_arch = "xtensa", target_arch = "riscv32"))
    ))]
    populate_office_tool_protocol_authority(&mut authority);
    authority
}

#[cfg(test)]
mod tests {
    use super::{
        build_default_tool_protocol_authority, ToolInputProtocolKind, ToolOutputProtocolKind,
    };

    #[test]
    fn first_wave_reading_tools_advertise_rich_blockers() {
        let authority = build_default_tool_protocol_authority();
        for tool_name in [
            "web_search",
            "web_fetch",
            "document_search",
            "document_read",
        ] {
            let contract = authority.get(tool_name).expect("tool protocol contract");
            assert_eq!(contract.input_kind, ToolInputProtocolKind::StructuredObject);
            assert_eq!(contract.output_kind, ToolOutputProtocolKind::StructuredJson);
            assert!(
                contract.supports_rich_blockers,
                "expected {tool_name} to advertise rich blocker support"
            );
        }
    }
}
