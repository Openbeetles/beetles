//! LLM-facing tool catalog authority.
//! LLM 工具目录真源：集中声明各入口可见面，而不是让 ToolMetadata 兼职承担。

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolLlmVisibility {
    pub user_llm: bool,
    pub system_llm: bool,
    pub internal_system_llm: bool,
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
