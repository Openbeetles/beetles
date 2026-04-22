//! Capability package contract and runtime overlays.
//! 能力包合同与运行时覆盖层。

use crate::channel_capability::ChannelCapabilityRegistry;
use crate::error::{Error, Result};
use crate::tools::ToolPolicyContext;
use crate::StateFs;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

pub const REL_DIR_CAPABILITY_PACKAGES: &str = "packages/capability_packages";
pub const REL_PATH_CAPABILITY_PACKAGE_REGISTRY: &str = "config/capability_packages_registry.json";
pub const REL_DIR_CAPABILITY_PACKAGE_ROLLBACK: &str = "packages/capability_package_rollback";
pub const MAX_CAPABILITY_PACKAGE_HTTP_BODY_LEN: usize = 256 * 1024;

const MAX_CAPABILITY_PACKAGE_COUNT: usize = 24;
const MAX_CAPABILITY_PACKAGE_ID_LEN: usize = 48;
const MAX_CAPABILITY_PACKAGE_VERSION_LEN: usize = 32;
const MAX_CAPABILITY_PACKAGE_TITLE_LEN: usize = 96;
const MAX_CAPABILITY_PACKAGE_SUMMARY_LEN: usize = 240;
const MAX_CAPABILITY_PACKAGE_TEXT_FILE_LEN: usize = 32 * 1024;
const MAX_CAPABILITY_PACKAGE_ASSET_BYTES: usize = 64 * 1024;
const MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION: usize = 16;
const MAX_CAPABILITY_PACKAGE_RUNTIME_ITEMS: usize = 8;
const MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP: usize = 31;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPackageWorkflowKind {
    #[default]
    WorkflowTemplate,
    TaskRecipe,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPackageRollbackStrategy {
    #[default]
    RestorePreviousBundle,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPackageVisibilityOverride {
    #[default]
    Inherit,
    ForceVisible,
    ForceHidden,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityPackageOperationKind {
    #[default]
    Install,
    Enable,
    Disable,
    Uninstall,
    Rollback,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageFileRef {
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageSkillFragmentManifest {
    pub fragment_id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageWorkflowManifest {
    pub workflow_id: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub kind: CapabilityPackageWorkflowKind,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageToolPolicyOverlay {
    pub tool_name: String,
    #[serde(default)]
    pub user_llm: CapabilityPackageVisibilityOverride,
    #[serde(default)]
    pub system_llm: CapabilityPackageVisibilityOverride,
    #[serde(default)]
    pub internal_system_llm: CapabilityPackageVisibilityOverride,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackagePolicyManifest {
    pub policy_id: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackagePolicyDocument {
    pub policy_id: String,
    #[serde(default)]
    pub tool_policy_overlays: Vec<CapabilityPackageToolPolicyOverlay>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageRollbackContract {
    #[serde(default)]
    pub strategy: CapabilityPackageRollbackStrategy,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageManifest {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub channel_compatibility: Vec<String>,
    #[serde(default)]
    pub skill_fragments: Vec<CapabilityPackageSkillFragmentManifest>,
    #[serde(default)]
    pub workflows: Vec<CapabilityPackageWorkflowManifest>,
    #[serde(default)]
    pub policies: Vec<CapabilityPackagePolicyManifest>,
    #[serde(default)]
    pub assets: Vec<CapabilityPackageFileRef>,
    #[serde(default)]
    pub rollback: CapabilityPackageRollbackContract,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageTextFile {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageAssetFile {
    pub path: String,
    pub content_base64: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageInstallPayload {
    pub manifest: CapabilityPackageManifest,
    #[serde(default)]
    pub skills: Vec<CapabilityPackageTextFile>,
    #[serde(default)]
    pub workflows: Vec<CapabilityPackageTextFile>,
    #[serde(default)]
    pub policies: Vec<CapabilityPackageTextFile>,
    #[serde(default)]
    pub assets: Vec<CapabilityPackageAssetFile>,
    #[serde(default = "default_enable_on_install")]
    pub enable_on_install: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageRegistryEntry {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub installed_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub channel_compatibility: Vec<String>,
    #[serde(default)]
    pub skill_fragment_count: usize,
    #[serde(default)]
    pub workflow_count: usize,
    #[serde(default)]
    pub policy_count: usize,
    #[serde(default)]
    pub asset_count: usize,
    #[serde(default)]
    pub rollback_available: bool,
    #[serde(default)]
    pub rollback_updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageRegistry {
    #[serde(default)]
    pub packages: Vec<CapabilityPackageRegistryEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageRollbackRecord {
    pub package_id: String,
    #[serde(default)]
    pub operation: CapabilityPackageOperationKind,
    #[serde(default)]
    pub captured_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_entry: Option<CapabilityPackageRegistryEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_bundle: Option<CapabilityPackageInstallPayload>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageRuntimeCapabilities {
    #[serde(default)]
    pub available: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageOperatorEntry {
    pub package_id: String,
    pub version: String,
    pub display_name: String,
    pub enabled: bool,
    pub compatible_now: bool,
    pub requirements_satisfied: bool,
    pub workflow_count: usize,
    pub skill_fragment_count: usize,
    pub policy_count: usize,
    pub asset_count: usize,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub missing_capabilities: Vec<String>,
    #[serde(default)]
    pub channel_compatibility: Vec<String>,
    #[serde(default)]
    pub rollback_available: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageOperatorSnapshot {
    #[serde(default)]
    pub installed: usize,
    #[serde(default)]
    pub enabled: usize,
    #[serde(default)]
    pub active_now: usize,
    #[serde(default)]
    pub workflow_count: usize,
    #[serde(default)]
    pub skill_fragment_count: usize,
    #[serde(default)]
    pub policy_overlay_count: usize,
    #[serde(default)]
    pub asset_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packages: Vec<CapabilityPackageOperatorEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityPackageRuntimePromptBundle {
    pub text: String,
    pub active_packages: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityPackageResolvedToolPolicyOverlay {
    pub tool_name: String,
    pub user_llm: CapabilityPackageVisibilityOverride,
    pub system_llm: CapabilityPackageVisibilityOverride,
    pub internal_system_llm: CapabilityPackageVisibilityOverride,
    pub source_packages: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilityPackageToolPolicySet {
    overlays: BTreeMap<String, CapabilityPackageResolvedToolPolicyOverlay>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityPackageOperationOutcome {
    pub package_id: String,
    pub operation: CapabilityPackageOperationKind,
    pub version: String,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CapabilityPackageStorageLayout {
    Standard,
    EspCompact,
}

fn default_enable_on_install() -> bool {
    true
}

fn current_storage_layout() -> CapabilityPackageStorageLayout {
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) {
        CapabilityPackageStorageLayout::EspCompact
    } else {
        CapabilityPackageStorageLayout::Standard
    }
}

fn fnv1a64_hash(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn fnv1a64_hash_pair(left: &str, right: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in left.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    h ^= u64::from(0xff_u8);
    h = h.wrapping_mul(1099511628211);
    for b in right.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(1099511628211);
    }
    h
}

fn package_file_alias(package_id: &str, path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .take(6)
                .collect::<String>()
        })
        .filter(|value| !value.is_empty());
    match ext {
        Some(ext) => format!("{:016x}.{}", fnv1a64_hash_pair(package_id, path), ext),
        None => format!("{:016x}.dat", fnv1a64_hash_pair(package_id, path)),
    }
}

fn is_safe_rel_path_on_esp(rel_path: &str) -> bool {
    rel_path.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP
}

impl CapabilityPackageRuntimeCapabilities {
    pub fn contains(&self, value: &str) -> bool {
        self.available.iter().any(|candidate| candidate == value)
    }
}

impl CapabilityPackageToolPolicySet {
    pub fn llm_visibility_for(
        &self,
        tool_name: &str,
        ctx: &ToolPolicyContext<'_>,
        base: bool,
    ) -> bool {
        let Some(overlay) = self.overlays.get(tool_name) else {
            return base;
        };
        let decision = if ctx.is_internal_system_channel() {
            overlay.internal_system_llm
        } else if ctx.ingress == crate::bus::IngressKind::System {
            overlay.system_llm
        } else {
            overlay.user_llm
        };
        match decision {
            CapabilityPackageVisibilityOverride::Inherit => base,
            CapabilityPackageVisibilityOverride::ForceVisible => true,
            CapabilityPackageVisibilityOverride::ForceHidden => false,
        }
    }

    pub fn get(&self, tool_name: &str) -> Option<&CapabilityPackageResolvedToolPolicyOverlay> {
        self.overlays.get(tool_name)
    }
}

pub fn build_capability_package_runtime_capabilities(
    channel_registry: &ChannelCapabilityRegistry,
) -> CapabilityPackageRuntimeCapabilities {
    let mut available = BTreeSet::from([
        "skills".to_string(),
        "task_execution".to_string(),
        "runtime_skills".to_string(),
        "tool_governance".to_string(),
        "channel_capability_contract".to_string(),
    ]);
    for entry in channel_registry.list() {
        if entry.configured || entry.enabled {
            available.insert(format!("channel:{}", entry.id));
        }
    }
    CapabilityPackageRuntimeCapabilities {
        available: available.into_iter().collect(),
    }
}

pub fn install_capability_package(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    payload: &CapabilityPackageInstallPayload,
    now_secs: u64,
) -> Result<CapabilityPackageOperationOutcome> {
    validate_install_payload(payload)?;
    let candidate_policy_documents = parse_policy_documents(&payload.policies)?;
    let mut registry = read_capability_package_registry(fs)?;
    let previous_entry = registry
        .packages
        .iter()
        .find(|entry| entry.package_id == payload.manifest.package_id)
        .cloned();
    let previous_bundle = load_capability_package_bundle(fs, &payload.manifest.package_id)?;
    if payload.enable_on_install {
        validate_runtime_requirements(&payload.manifest, runtime_capabilities)?;
        validate_overlay_conflicts(
            fs,
            &registry,
            &payload.manifest,
            &candidate_policy_documents,
            payload.enable_on_install,
            Some(payload.manifest.package_id.as_str()),
        )?;
    }
    write_capability_package_rollback(
        fs,
        &payload.manifest.package_id,
        &CapabilityPackageRollbackRecord {
            package_id: payload.manifest.package_id.clone(),
            operation: CapabilityPackageOperationKind::Install,
            captured_at: now_secs,
            previous_entry: previous_entry.clone(),
            previous_bundle,
        },
    )?;
    clear_capability_package_dir(fs, &payload.manifest.package_id)?;
    write_capability_package_bundle(fs, payload)?;
    upsert_registry_entry(
        &mut registry,
        build_registry_entry(
            &payload.manifest,
            payload.enable_on_install,
            previous_entry
                .as_ref()
                .map_or(now_secs, |entry| entry.installed_at),
            now_secs,
            true,
        ),
    );
    write_capability_package_registry(fs, &registry)?;
    Ok(CapabilityPackageOperationOutcome {
        package_id: payload.manifest.package_id.clone(),
        operation: CapabilityPackageOperationKind::Install,
        version: payload.manifest.version.clone(),
        enabled: payload.enable_on_install,
    })
}

pub fn set_capability_package_enabled(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    package_id: &str,
    enabled: bool,
    now_secs: u64,
) -> Result<CapabilityPackageOperationOutcome> {
    let package_id = normalize_identifier(package_id, MAX_CAPABILITY_PACKAGE_ID_LEN, "package_id")?;
    let mut registry = read_capability_package_registry(fs)?;
    let index = registry
        .packages
        .iter()
        .position(|entry| entry.package_id == package_id)
        .ok_or_else(|| Error::config("capability_package", "package is not installed"))?;
    let mut entry = registry.packages[index].clone();
    if entry.enabled == enabled {
        return Ok(CapabilityPackageOperationOutcome {
            package_id: entry.package_id,
            operation: if enabled {
                CapabilityPackageOperationKind::Enable
            } else {
                CapabilityPackageOperationKind::Disable
            },
            version: entry.version,
            enabled,
        });
    }
    let bundle = load_capability_package_bundle(fs, &entry.package_id)?.ok_or_else(|| {
        Error::config("capability_package", "installed package bundle is missing")
    })?;
    let candidate_policy_documents = parse_policy_documents(&bundle.policies)?;
    if enabled {
        validate_runtime_requirements(&bundle.manifest, runtime_capabilities)?;
        validate_overlay_conflicts(
            fs,
            &registry,
            &bundle.manifest,
            &candidate_policy_documents,
            true,
            Some(entry.package_id.as_str()),
        )?;
    }
    write_capability_package_rollback(
        fs,
        &entry.package_id,
        &CapabilityPackageRollbackRecord {
            package_id: entry.package_id.clone(),
            operation: if enabled {
                CapabilityPackageOperationKind::Enable
            } else {
                CapabilityPackageOperationKind::Disable
            },
            captured_at: now_secs,
            previous_entry: Some(entry.clone()),
            previous_bundle: Some(bundle.clone()),
        },
    )?;
    entry.enabled = enabled;
    entry.updated_at = now_secs;
    entry.rollback_available = true;
    entry.rollback_updated_at = now_secs;
    registry.packages[index] = entry.clone();
    write_capability_package_registry(fs, &registry)?;
    Ok(CapabilityPackageOperationOutcome {
        package_id: entry.package_id,
        operation: if enabled {
            CapabilityPackageOperationKind::Enable
        } else {
            CapabilityPackageOperationKind::Disable
        },
        version: entry.version,
        enabled,
    })
}

pub fn uninstall_capability_package(
    fs: &dyn StateFs,
    package_id: &str,
    now_secs: u64,
) -> Result<CapabilityPackageOperationOutcome> {
    let package_id = normalize_identifier(package_id, MAX_CAPABILITY_PACKAGE_ID_LEN, "package_id")?;
    let mut registry = read_capability_package_registry(fs)?;
    let index = registry
        .packages
        .iter()
        .position(|entry| entry.package_id == package_id)
        .ok_or_else(|| Error::config("capability_package", "package is not installed"))?;
    let entry = registry.packages[index].clone();
    let previous_bundle = load_capability_package_bundle(fs, &package_id)?;
    write_capability_package_rollback(
        fs,
        &package_id,
        &CapabilityPackageRollbackRecord {
            package_id: package_id.clone(),
            operation: CapabilityPackageOperationKind::Uninstall,
            captured_at: now_secs,
            previous_entry: Some(entry.clone()),
            previous_bundle,
        },
    )?;
    clear_capability_package_dir(fs, &package_id)?;
    registry.packages.remove(index);
    write_capability_package_registry(fs, &registry)?;
    Ok(CapabilityPackageOperationOutcome {
        package_id,
        operation: CapabilityPackageOperationKind::Uninstall,
        version: entry.version,
        enabled: false,
    })
}

pub fn rollback_capability_package(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    package_id: &str,
    now_secs: u64,
) -> Result<CapabilityPackageOperationOutcome> {
    let package_id = normalize_identifier(package_id, MAX_CAPABILITY_PACKAGE_ID_LEN, "package_id")?;
    let rollback = read_capability_package_rollback(fs, &package_id)?
        .ok_or_else(|| Error::config("capability_package", "no rollback snapshot available"))?;
    let current_entry = read_capability_package_registry(fs)?
        .packages
        .into_iter()
        .find(|entry| entry.package_id == package_id);
    let current_bundle = load_capability_package_bundle(fs, &package_id)?;
    write_capability_package_rollback(
        fs,
        &package_id,
        &CapabilityPackageRollbackRecord {
            package_id: package_id.clone(),
            operation: CapabilityPackageOperationKind::Rollback,
            captured_at: now_secs,
            previous_entry: current_entry.clone(),
            previous_bundle: current_bundle.clone(),
        },
    )?;
    let mut registry = read_capability_package_registry(fs)?;
    registry
        .packages
        .retain(|entry| entry.package_id != package_id);
    clear_capability_package_dir(fs, &package_id)?;
    if let Some(bundle) = rollback.previous_bundle.as_ref() {
        validate_install_payload(bundle)?;
        let candidate_policy_documents = parse_policy_documents(&bundle.policies)?;
        if rollback
            .previous_entry
            .as_ref()
            .is_some_and(|entry| entry.enabled)
        {
            validate_runtime_requirements(&bundle.manifest, runtime_capabilities)?;
            validate_overlay_conflicts(
                fs,
                &registry,
                &bundle.manifest,
                &candidate_policy_documents,
                true,
                Some(package_id.as_str()),
            )?;
        }
        write_capability_package_bundle(fs, bundle)?;
    }
    if let Some(mut entry) = rollback.previous_entry.clone() {
        entry.rollback_available = true;
        entry.rollback_updated_at = now_secs;
        upsert_registry_entry(&mut registry, entry.clone());
        write_capability_package_registry(fs, &registry)?;
        return Ok(CapabilityPackageOperationOutcome {
            package_id: entry.package_id,
            operation: CapabilityPackageOperationKind::Rollback,
            version: entry.version,
            enabled: entry.enabled,
        });
    }
    write_capability_package_registry(fs, &registry)?;
    Ok(CapabilityPackageOperationOutcome {
        package_id,
        operation: CapabilityPackageOperationKind::Rollback,
        version: String::new(),
        enabled: false,
    })
}

pub fn build_capability_package_runtime_prompt_bundle(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    channel: &str,
    max_chars: usize,
) -> Result<CapabilityPackageRuntimePromptBundle> {
    if max_chars == 0 {
        return Ok(CapabilityPackageRuntimePromptBundle::default());
    }
    let active = load_active_package_bundles(fs, runtime_capabilities, channel)?;
    if active.is_empty() {
        return Ok(CapabilityPackageRuntimePromptBundle::default());
    }
    let mut text = String::from(
        "## Capability Packages\nInstalled capability bundles that extend skills, workflow templates, and tool exposure overlays. Reuse them when they match the current task.\n",
    );
    let mut active_packages = Vec::new();
    for bundle in active
        .into_iter()
        .take(MAX_CAPABILITY_PACKAGE_RUNTIME_ITEMS)
    {
        let manifest = &bundle.manifest;
        active_packages.push(manifest.package_id.clone());
        let header = format!(
            "\n### {} v{}\n{}\n",
            manifest.display_name,
            manifest.version,
            truncate_text(&manifest.description, MAX_CAPABILITY_PACKAGE_SUMMARY_LEN),
        );
        if !push_with_limit(&mut text, &header, max_chars) {
            break;
        }
        for fragment in &manifest.skill_fragments {
            let Some(file) = bundle.skill_map.get(&fragment.path) else {
                continue;
            };
            let block = format!(
                "- [skill:{}] {}: {}\n{}\n",
                fragment.fragment_id,
                fragment.title,
                truncate_text(&fragment.summary, MAX_CAPABILITY_PACKAGE_SUMMARY_LEN),
                truncate_text(&file.content, 640),
            );
            if !push_with_limit(&mut text, &block, max_chars) {
                break;
            }
        }
        for workflow in &manifest.workflows {
            let Some(file) = bundle.workflow_map.get(&workflow.path) else {
                continue;
            };
            let block = format!(
                "- [workflow:{}:{}] {}: {}\n{}\n",
                workflow.kind_label(),
                workflow.workflow_id,
                workflow.title,
                truncate_text(&workflow.summary, MAX_CAPABILITY_PACKAGE_SUMMARY_LEN),
                truncate_text(&file.content, 720),
            );
            if !push_with_limit(&mut text, &block, max_chars) {
                break;
            }
        }
        let policy_set =
            merge_policy_documents(&bundle.manifest.package_id, &bundle.policy_documents, None)?;
        for overlay in policy_set.overlays.values() {
            let line = format!(
                "- [tool_policy:{}] user={:?} system={:?} internal={:?}\n",
                overlay.tool_name,
                overlay.user_llm,
                overlay.system_llm,
                overlay.internal_system_llm
            );
            if !push_with_limit(&mut text, &line, max_chars) {
                break;
            }
        }
    }
    Ok(CapabilityPackageRuntimePromptBundle {
        text: text.trim_end().to_string(),
        active_packages,
    })
}

pub fn build_capability_package_tool_policy_set(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    channel: &str,
) -> Result<CapabilityPackageToolPolicySet> {
    let active = load_active_package_bundles(fs, runtime_capabilities, channel)?;
    let mut merged = CapabilityPackageToolPolicySet::default();
    for bundle in active {
        merged = merge_policy_documents(
            &bundle.manifest.package_id,
            &bundle.policy_documents,
            Some(&merged),
        )?;
    }
    Ok(merged)
}

pub fn build_capability_package_operator_snapshot(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    current_channel: &str,
) -> Result<CapabilityPackageOperatorSnapshot> {
    let registry = read_capability_package_registry(fs)?;
    let mut snapshot = CapabilityPackageOperatorSnapshot {
        installed: registry.packages.len(),
        ..CapabilityPackageOperatorSnapshot::default()
    };
    for entry in registry.packages {
        let bundle = load_capability_package_bundle(fs, &entry.package_id)?;
        let manifest = bundle.as_ref().map(|payload| &payload.manifest);
        let compatible_now = manifest
            .map(|manifest| manifest_supports_channel(manifest, current_channel))
            .unwrap_or(false);
        let missing_capabilities = manifest
            .map(|manifest| missing_runtime_capabilities(manifest, runtime_capabilities))
            .unwrap_or_default();
        let requirements_satisfied = missing_capabilities.is_empty();
        if entry.enabled {
            snapshot.enabled = snapshot.enabled.saturating_add(1);
        }
        if entry.enabled && compatible_now && requirements_satisfied {
            snapshot.active_now = snapshot.active_now.saturating_add(1);
        }
        snapshot.workflow_count = snapshot.workflow_count.saturating_add(entry.workflow_count);
        snapshot.skill_fragment_count = snapshot
            .skill_fragment_count
            .saturating_add(entry.skill_fragment_count);
        snapshot.policy_overlay_count = snapshot
            .policy_overlay_count
            .saturating_add(count_policy_overlays(bundle.as_ref()));
        snapshot.asset_count = snapshot.asset_count.saturating_add(entry.asset_count);
        snapshot.packages.push(CapabilityPackageOperatorEntry {
            package_id: entry.package_id,
            version: entry.version,
            display_name: entry.display_name,
            enabled: entry.enabled,
            compatible_now,
            requirements_satisfied,
            workflow_count: entry.workflow_count,
            skill_fragment_count: entry.skill_fragment_count,
            policy_count: entry.policy_count,
            asset_count: entry.asset_count,
            required_capabilities: entry.required_capabilities,
            missing_capabilities,
            channel_compatibility: entry.channel_compatibility,
            rollback_available: entry.rollback_available,
        });
    }
    Ok(snapshot)
}

pub fn render_capability_package_operator_text(
    snapshot: &CapabilityPackageOperatorSnapshot,
) -> String {
    let mut out = format!(
        "  capability_packages: installed={} enabled={} active_now={} workflows={} skill_fragments={} policy_overlays={} assets={}\n",
        snapshot.installed,
        snapshot.enabled,
        snapshot.active_now,
        snapshot.workflow_count,
        snapshot.skill_fragment_count,
        snapshot.policy_overlay_count,
        snapshot.asset_count,
    );
    for package in snapshot
        .packages
        .iter()
        .take(MAX_CAPABILITY_PACKAGE_RUNTIME_ITEMS)
    {
        out.push_str(&format!(
            "    - {} v{} | enabled={} compatible_now={} requirements_satisfied={} workflows={} skills={} policies={} rollback={}\n",
            package.package_id,
            package.version,
            package.enabled,
            package.compatible_now,
            package.requirements_satisfied,
            package.workflow_count,
            package.skill_fragment_count,
            package.policy_count,
            package.rollback_available,
        ));
    }
    if snapshot.packages.len() > MAX_CAPABILITY_PACKAGE_RUNTIME_ITEMS {
        out.push_str(&format!(
            "    - ... {} more capability packages\n",
            snapshot.packages.len() - MAX_CAPABILITY_PACKAGE_RUNTIME_ITEMS
        ));
    }
    out
}

fn validate_install_payload(payload: &CapabilityPackageInstallPayload) -> Result<()> {
    validate_manifest(&payload.manifest)?;
    validate_text_file_map(&payload.skills, "skills")?;
    validate_text_file_map(&payload.workflows, "workflows")?;
    validate_text_file_map(&payload.policies, "policies")?;
    validate_asset_file_map(&payload.assets)?;
    validate_manifest_paths(payload)?;
    for policy in &payload.manifest.policies {
        let source = payload
            .policies
            .iter()
            .find(|file| file.path == policy.path)
            .ok_or_else(|| Error::config("capability_package", "policy file is missing"))?;
        let document: CapabilityPackagePolicyDocument = serde_json::from_str(&source.content)
            .map_err(|error| Error::config("capability_package", error.to_string()))?;
        if document.policy_id != policy.policy_id {
            return Err(Error::config(
                "capability_package",
                "policy_id does not match policy document",
            ));
        }
        validate_policy_document(&document)?;
    }
    Ok(())
}

fn validate_manifest(manifest: &CapabilityPackageManifest) -> Result<()> {
    normalize_identifier(
        &manifest.package_id,
        MAX_CAPABILITY_PACKAGE_ID_LEN,
        "package_id",
    )?;
    normalize_identifier(
        &manifest.version,
        MAX_CAPABILITY_PACKAGE_VERSION_LEN,
        "version",
    )?;
    if manifest.display_name.trim().is_empty()
        || manifest.display_name.len() > MAX_CAPABILITY_PACKAGE_TITLE_LEN
    {
        return Err(Error::config(
            "capability_package",
            "display_name is empty or too long",
        ));
    }
    if manifest.description.len() > MAX_CAPABILITY_PACKAGE_SUMMARY_LEN {
        return Err(Error::config(
            "capability_package",
            "description exceeds limit",
        ));
    }
    if manifest.skill_fragments.len() > MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION
        || manifest.workflows.len() > MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION
        || manifest.policies.len() > MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION
        || manifest.assets.len() > MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION
    {
        return Err(Error::config(
            "capability_package",
            "package section exceeds file limit",
        ));
    }
    if manifest.channel_compatibility.len() > MAX_CAPABILITY_PACKAGE_FILES_PER_SECTION {
        return Err(Error::config(
            "capability_package",
            "too many channel compatibility entries",
        ));
    }
    let mut skill_ids = BTreeSet::new();
    let mut workflow_ids = BTreeSet::new();
    let mut policy_ids = BTreeSet::new();
    let mut asset_paths = BTreeSet::new();
    for fragment in &manifest.skill_fragments {
        normalize_identifier(
            &fragment.fragment_id,
            MAX_CAPABILITY_PACKAGE_ID_LEN,
            "skill_fragment.fragment_id",
        )?;
        validate_title_and_summary(&fragment.title, &fragment.summary)?;
        validate_package_rel_path(&fragment.path, "skills/")?;
        if !skill_ids.insert(fragment.fragment_id.as_str()) {
            return Err(Error::config(
                "capability_package",
                "duplicate skill fragment id",
            ));
        }
    }
    for workflow in &manifest.workflows {
        normalize_identifier(
            &workflow.workflow_id,
            MAX_CAPABILITY_PACKAGE_ID_LEN,
            "workflow.workflow_id",
        )?;
        validate_title_and_summary(&workflow.title, &workflow.summary)?;
        validate_package_rel_path(&workflow.path, "workflows/")?;
        if !workflow_ids.insert(workflow.workflow_id.as_str()) {
            return Err(Error::config("capability_package", "duplicate workflow id"));
        }
    }
    for policy in &manifest.policies {
        normalize_identifier(
            &policy.policy_id,
            MAX_CAPABILITY_PACKAGE_ID_LEN,
            "policy.policy_id",
        )?;
        validate_package_rel_path(&policy.path, "policies/")?;
        if !policy_ids.insert(policy.policy_id.as_str()) {
            return Err(Error::config("capability_package", "duplicate policy id"));
        }
    }
    for asset in &manifest.assets {
        validate_package_rel_path(&asset.path, "assets/")?;
        if !asset_paths.insert(asset.path.as_str()) {
            return Err(Error::config("capability_package", "duplicate asset path"));
        }
    }
    for channel in &manifest.channel_compatibility {
        if channel != "*" && !crate::channel_catalog::channel_is_compiled(channel) {
            return Err(Error::config(
                "capability_package",
                "unknown channel compatibility value",
            ));
        }
    }
    Ok(())
}

fn validate_title_and_summary(title: &str, summary: &str) -> Result<()> {
    if title.trim().is_empty() || title.len() > MAX_CAPABILITY_PACKAGE_TITLE_LEN {
        return Err(Error::config(
            "capability_package",
            "title is empty or too long",
        ));
    }
    if summary.len() > MAX_CAPABILITY_PACKAGE_SUMMARY_LEN {
        return Err(Error::config("capability_package", "summary exceeds limit"));
    }
    Ok(())
}

fn validate_text_file_map(files: &[CapabilityPackageTextFile], prefix: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for file in files {
        validate_package_rel_path(&file.path, &format!("{prefix}/"))?;
        if !seen.insert(file.path.as_str()) {
            return Err(Error::config(
                "capability_package",
                "duplicate package text file path",
            ));
        }
        if file.content.is_empty() || file.content.len() > MAX_CAPABILITY_PACKAGE_TEXT_FILE_LEN {
            return Err(Error::config(
                "capability_package",
                "package text file is empty or too large",
            ));
        }
    }
    Ok(())
}

fn validate_asset_file_map(files: &[CapabilityPackageAssetFile]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for file in files {
        validate_package_rel_path(&file.path, "assets/")?;
        if !seen.insert(file.path.as_str()) {
            return Err(Error::config(
                "capability_package",
                "duplicate asset file path",
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.content_base64.trim())
            .map_err(|error| Error::config("capability_package", error.to_string()))?;
        if bytes.is_empty() || bytes.len() > MAX_CAPABILITY_PACKAGE_ASSET_BYTES {
            return Err(Error::config(
                "capability_package",
                "asset file is empty or too large",
            ));
        }
    }
    Ok(())
}

fn validate_manifest_paths(payload: &CapabilityPackageInstallPayload) -> Result<()> {
    for fragment in &payload.manifest.skill_fragments {
        if !payload.skills.iter().any(|file| file.path == fragment.path) {
            return Err(Error::config(
                "capability_package",
                "manifest skill fragment path is missing from payload",
            ));
        }
    }
    for workflow in &payload.manifest.workflows {
        if !payload
            .workflows
            .iter()
            .any(|file| file.path == workflow.path)
        {
            return Err(Error::config(
                "capability_package",
                "manifest workflow path is missing from payload",
            ));
        }
    }
    for policy in &payload.manifest.policies {
        if !payload.policies.iter().any(|file| file.path == policy.path) {
            return Err(Error::config(
                "capability_package",
                "manifest policy path is missing from payload",
            ));
        }
    }
    for asset in &payload.manifest.assets {
        if !payload.assets.iter().any(|file| file.path == asset.path) {
            return Err(Error::config(
                "capability_package",
                "manifest asset path is missing from payload",
            ));
        }
    }
    Ok(())
}

fn validate_policy_document(document: &CapabilityPackagePolicyDocument) -> Result<()> {
    let mut seen = BTreeSet::new();
    for overlay in &document.tool_policy_overlays {
        normalize_identifier(
            &overlay.tool_name,
            MAX_CAPABILITY_PACKAGE_ID_LEN,
            "tool_policy_overlay.tool_name",
        )?;
        if !seen.insert(overlay.tool_name.as_str()) {
            return Err(Error::config(
                "capability_package",
                "duplicate tool policy overlay for tool",
            ));
        }
    }
    Ok(())
}

fn validate_runtime_requirements(
    manifest: &CapabilityPackageManifest,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
) -> Result<()> {
    let missing = missing_runtime_capabilities(manifest, runtime_capabilities);
    if missing.is_empty() {
        return Ok(());
    }
    Err(Error::config(
        "capability_package",
        format!("missing required capabilities: {}", missing.join(", ")),
    ))
}

fn missing_runtime_capabilities(
    manifest: &CapabilityPackageManifest,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
) -> Vec<String> {
    manifest
        .required_capabilities
        .iter()
        .filter(|capability| !runtime_capabilities.contains(capability))
        .cloned()
        .collect()
}

fn validate_overlay_conflicts(
    fs: &dyn StateFs,
    registry: &CapabilityPackageRegistry,
    manifest: &CapabilityPackageManifest,
    candidate_policy_documents: &[CapabilityPackagePolicyDocument],
    enabled_after_op: bool,
    exclude_package_id: Option<&str>,
) -> Result<()> {
    if !enabled_after_op {
        return Ok(());
    }
    let candidate_workflow_ids = manifest
        .workflows
        .iter()
        .map(|workflow| workflow.workflow_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut active_policy_set = CapabilityPackageToolPolicySet::default();
    for entry in &registry.packages {
        if !entry.enabled {
            continue;
        }
        if exclude_package_id.is_some_and(|value| value == entry.package_id) {
            continue;
        }
        let Some(bundle) = load_capability_package_bundle(fs, &entry.package_id)? else {
            continue;
        };
        if !channels_overlap(
            &manifest.channel_compatibility,
            &bundle.manifest.channel_compatibility,
        ) {
            continue;
        }
        for workflow in &bundle.manifest.workflows {
            if candidate_workflow_ids.contains(workflow.workflow_id.as_str()) {
                return Err(Error::config(
                    "capability_package",
                    "workflow id conflicts with another enabled package",
                ));
            }
        }
        active_policy_set = merge_policy_documents(
            &bundle.manifest.package_id,
            &parse_policy_documents(&bundle.policies)?,
            Some(&active_policy_set),
        )?;
    }
    let _ = merge_policy_documents(
        &manifest.package_id,
        candidate_policy_documents,
        Some(&active_policy_set),
    )?;
    Ok(())
}

fn merge_policy_documents(
    package_id: &str,
    documents: &[CapabilityPackagePolicyDocument],
    base: Option<&CapabilityPackageToolPolicySet>,
) -> Result<CapabilityPackageToolPolicySet> {
    let mut merged = base.cloned().unwrap_or_default();
    for document in documents {
        for overlay in &document.tool_policy_overlays {
            let existing = merged.overlays.get(&overlay.tool_name);
            let mut resolved = CapabilityPackageResolvedToolPolicyOverlay {
                tool_name: overlay.tool_name.clone(),
                user_llm: overlay.user_llm,
                system_llm: overlay.system_llm,
                internal_system_llm: overlay.internal_system_llm,
                source_packages: vec![package_id.to_string()],
            };
            if let Some(existing) = existing {
                resolved.user_llm = merge_visibility_override(
                    existing.user_llm,
                    overlay.user_llm,
                    overlay.tool_name.as_str(),
                )?;
                resolved.system_llm = merge_visibility_override(
                    existing.system_llm,
                    overlay.system_llm,
                    overlay.tool_name.as_str(),
                )?;
                resolved.internal_system_llm = merge_visibility_override(
                    existing.internal_system_llm,
                    overlay.internal_system_llm,
                    overlay.tool_name.as_str(),
                )?;
                resolved.source_packages = existing.source_packages.clone();
                if !resolved
                    .source_packages
                    .iter()
                    .any(|source| source == package_id)
                {
                    resolved.source_packages.push(package_id.to_string());
                }
            }
            merged.overlays.insert(overlay.tool_name.clone(), resolved);
        }
    }
    Ok(merged)
}

fn merge_visibility_override(
    current: CapabilityPackageVisibilityOverride,
    incoming: CapabilityPackageVisibilityOverride,
    tool_name: &str,
) -> Result<CapabilityPackageVisibilityOverride> {
    match (current, incoming) {
        (CapabilityPackageVisibilityOverride::Inherit, value)
        | (value, CapabilityPackageVisibilityOverride::Inherit) => Ok(value),
        (left, right) if left == right => Ok(left),
        _ => Err(Error::config(
            "capability_package",
            format!("tool policy conflict on tool {tool_name}"),
        )),
    }
}

fn read_capability_package_registry(fs: &dyn StateFs) -> Result<CapabilityPackageRegistry> {
    let Some(bytes) = fs.read(&capability_package_registry_path())? else {
        return Ok(CapabilityPackageRegistry::default());
    };
    serde_json::from_slice(&bytes)
        .map_err(|error| Error::config("capability_package", error.to_string()))
}

fn write_capability_package_registry(
    fs: &dyn StateFs,
    registry: &CapabilityPackageRegistry,
) -> Result<()> {
    if registry.packages.len() > MAX_CAPABILITY_PACKAGE_COUNT {
        return Err(Error::config(
            "capability_package",
            "capability package registry exceeds package limit",
        ));
    }
    let bytes = serde_json::to_vec(registry)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    fs.write(&capability_package_registry_path(), &bytes)
}

fn write_capability_package_rollback(
    fs: &dyn StateFs,
    package_id: &str,
    rollback: &CapabilityPackageRollbackRecord,
) -> Result<()> {
    let bytes = serde_json::to_vec(rollback)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    fs.write(&rollback_path(package_id), &bytes)
}

fn read_capability_package_rollback(
    fs: &dyn StateFs,
    package_id: &str,
) -> Result<Option<CapabilityPackageRollbackRecord>> {
    let Some((bytes, _)) = read_existing_package_file(
        fs,
        &rollback_path(package_id),
        legacy_rollback_path(package_id).as_deref(),
    )?
    else {
        return Ok(None);
    };
    let rollback = serde_json::from_slice(&bytes)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    Ok(Some(rollback))
}

fn load_capability_package_bundle(
    fs: &dyn StateFs,
    package_id: &str,
) -> Result<Option<CapabilityPackageInstallPayload>> {
    let Some((manifest_bytes, layout)) = read_existing_package_file(
        fs,
        &manifest_path(package_id),
        legacy_manifest_path(package_id).as_deref(),
    )?
    else {
        return Ok(None);
    };
    let manifest: CapabilityPackageManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32"))
        && layout == CapabilityPackageStorageLayout::Standard
    {
        ensure_legacy_bundle_paths_safe_on_esp(package_id, &manifest)?;
    }
    let skills = manifest
        .skill_fragments
        .iter()
        .map(|fragment| {
            read_text_file(
                fs,
                package_rel_path_for_layout(layout, package_id, &fragment.path),
                &fragment.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let workflows = manifest
        .workflows
        .iter()
        .map(|workflow| {
            read_text_file(
                fs,
                package_rel_path_for_layout(layout, package_id, &workflow.path),
                &workflow.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let policies = manifest
        .policies
        .iter()
        .map(|policy| {
            read_text_file(
                fs,
                package_rel_path_for_layout(layout, package_id, &policy.path),
                &policy.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let assets = manifest
        .assets
        .iter()
        .map(|asset| {
            read_asset_file(
                fs,
                package_rel_path_for_layout(layout, package_id, &asset.path),
                &asset.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(CapabilityPackageInstallPayload {
        manifest,
        skills,
        workflows,
        policies,
        assets,
        enable_on_install: false,
    }))
}

fn write_capability_package_bundle(
    fs: &dyn StateFs,
    payload: &CapabilityPackageInstallPayload,
) -> Result<()> {
    let package_id = payload.manifest.package_id.as_str();
    let manifest_bytes = serde_json::to_vec_pretty(&payload.manifest)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    fs.write(&manifest_path(package_id), &manifest_bytes)?;
    for file in &payload.skills {
        fs.write(
            &package_rel_path(package_id, &file.path),
            file.content.as_bytes(),
        )?;
    }
    for file in &payload.workflows {
        fs.write(
            &package_rel_path(package_id, &file.path),
            file.content.as_bytes(),
        )?;
    }
    for file in &payload.policies {
        fs.write(
            &package_rel_path(package_id, &file.path),
            file.content.as_bytes(),
        )?;
    }
    for file in &payload.assets {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file.content_base64.trim())
            .map_err(|error| Error::config("capability_package", error.to_string()))?;
        fs.write(&package_rel_path(package_id, &file.path), &bytes)?;
    }
    Ok(())
}

fn clear_capability_package_dir(fs: &dyn StateFs, package_id: &str) -> Result<()> {
    let layout = current_storage_layout();
    clear_bundle_files_for_layout(fs, package_id, layout)?;
    if layout != CapabilityPackageStorageLayout::Standard {
        clear_bundle_files_for_layout(fs, package_id, CapabilityPackageStorageLayout::Standard)?;
    }
    Ok(())
}

fn clear_bundle_files_for_layout(
    fs: &dyn StateFs,
    package_id: &str,
    layout: CapabilityPackageStorageLayout,
) -> Result<bool> {
    let manifest_rel_path = match layout {
        CapabilityPackageStorageLayout::Standard => {
            let Some(path) = legacy_manifest_path(package_id) else {
                return Ok(false);
            };
            path
        }
        CapabilityPackageStorageLayout::EspCompact => manifest_path_for_layout(layout, package_id),
    };
    let Some(manifest_bytes) = fs.read(&manifest_rel_path)? else {
        return Ok(false);
    };
    if let Ok(manifest) = serde_json::from_slice::<CapabilityPackageManifest>(&manifest_bytes) {
        for rel_path in bundle_file_rel_paths_for_layout(layout, package_id, &manifest) {
            fs.remove(&rel_path)?;
        }
    }
    fs.remove(&manifest_rel_path)?;
    if layout == CapabilityPackageStorageLayout::Standard {
        remove_dir_recursive(fs, &legacy_package_dir(package_id))?;
    }
    Ok(true)
}

fn bundle_file_rel_paths_for_layout(
    layout: CapabilityPackageStorageLayout,
    package_id: &str,
    manifest: &CapabilityPackageManifest,
) -> Vec<String> {
    let mut rel_paths = Vec::with_capacity(
        manifest.skill_fragments.len()
            + manifest.workflows.len()
            + manifest.policies.len()
            + manifest.assets.len(),
    );
    rel_paths.extend(
        manifest
            .skill_fragments
            .iter()
            .map(|fragment| package_rel_path_for_layout(layout, package_id, &fragment.path)),
    );
    rel_paths.extend(
        manifest
            .workflows
            .iter()
            .map(|workflow| package_rel_path_for_layout(layout, package_id, &workflow.path)),
    );
    rel_paths.extend(
        manifest
            .policies
            .iter()
            .map(|policy| package_rel_path_for_layout(layout, package_id, &policy.path)),
    );
    rel_paths.extend(
        manifest
            .assets
            .iter()
            .map(|asset| package_rel_path_for_layout(layout, package_id, &asset.path)),
    );
    rel_paths
}

fn remove_dir_recursive(fs: &dyn StateFs, rel_dir: &str) -> Result<()> {
    let entries = fs.list_dir(rel_dir)?;
    for entry in entries {
        let child = format!("{rel_dir}/{}", entry.trim_end_matches('/'));
        if entry.ends_with('/') {
            remove_dir_recursive(fs, &child)?;
        } else {
            fs.remove(&child)?;
        }
    }
    Ok(())
}

fn upsert_registry_entry(
    registry: &mut CapabilityPackageRegistry,
    entry: CapabilityPackageRegistryEntry,
) {
    if let Some(existing) = registry
        .packages
        .iter_mut()
        .find(|candidate| candidate.package_id == entry.package_id)
    {
        *existing = entry;
    } else {
        registry.packages.push(entry);
    }
    registry
        .packages
        .sort_by(|a, b| a.package_id.cmp(&b.package_id));
}

fn build_registry_entry(
    manifest: &CapabilityPackageManifest,
    enabled: bool,
    installed_at: u64,
    updated_at: u64,
    rollback_available: bool,
) -> CapabilityPackageRegistryEntry {
    CapabilityPackageRegistryEntry {
        package_id: manifest.package_id.clone(),
        version: manifest.version.clone(),
        display_name: manifest.display_name.clone(),
        description: manifest.description.clone(),
        enabled,
        installed_at,
        updated_at,
        required_capabilities: manifest.required_capabilities.clone(),
        channel_compatibility: manifest.channel_compatibility.clone(),
        skill_fragment_count: manifest.skill_fragments.len(),
        workflow_count: manifest.workflows.len(),
        policy_count: manifest.policies.len(),
        asset_count: manifest.assets.len(),
        rollback_available,
        rollback_updated_at: updated_at,
    }
}

fn load_active_package_bundles(
    fs: &dyn StateFs,
    runtime_capabilities: &CapabilityPackageRuntimeCapabilities,
    channel: &str,
) -> Result<Vec<LoadedCapabilityPackageBundle>> {
    let registry = read_capability_package_registry(fs)?;
    let mut bundles = Vec::new();
    for entry in registry.packages {
        if !entry.enabled {
            continue;
        }
        let Some(payload) = load_capability_package_bundle(fs, &entry.package_id)? else {
            continue;
        };
        if !manifest_supports_channel(&payload.manifest, channel) {
            continue;
        }
        if !missing_runtime_capabilities(&payload.manifest, runtime_capabilities).is_empty() {
            continue;
        }
        bundles.push(LoadedCapabilityPackageBundle::from_payload(payload)?);
    }
    Ok(bundles)
}

fn count_policy_overlays(bundle: Option<&CapabilityPackageInstallPayload>) -> usize {
    bundle
        .map(|bundle| {
            bundle
                .policies
                .iter()
                .filter_map(|file| {
                    serde_json::from_str::<CapabilityPackagePolicyDocument>(&file.content).ok()
                })
                .map(|document| document.tool_policy_overlays.len())
                .sum()
        })
        .unwrap_or(0)
}

fn manifest_supports_channel(manifest: &CapabilityPackageManifest, channel: &str) -> bool {
    manifest.channel_compatibility.is_empty()
        || manifest
            .channel_compatibility
            .iter()
            .any(|value| value == "*" || value == channel)
}

fn channels_overlap(left: &[String], right: &[String]) -> bool {
    if left.is_empty() || right.is_empty() {
        return true;
    }
    if left.iter().any(|value| value == "*") || right.iter().any(|value| value == "*") {
        return true;
    }
    left.iter()
        .any(|channel| right.iter().any(|candidate| candidate == channel))
}

fn manifest_path(package_id: &str) -> String {
    manifest_path_for_layout(current_storage_layout(), package_id)
}

fn rollback_path(package_id: &str) -> String {
    rollback_path_for_layout(current_storage_layout(), package_id)
}

fn package_rel_path(package_id: &str, path: &str) -> String {
    package_rel_path_for_layout(current_storage_layout(), package_id, path)
}

fn capability_package_registry_path() -> String {
    capability_package_registry_path_for_layout(current_storage_layout())
}

fn capability_package_registry_path_for_layout(layout: CapabilityPackageStorageLayout) -> String {
    match layout {
        CapabilityPackageStorageLayout::Standard => {
            REL_PATH_CAPABILITY_PACKAGE_REGISTRY.to_string()
        }
        CapabilityPackageStorageLayout::EspCompact => "cfg/cpkg.j".to_string(),
    }
}

fn manifest_path_for_layout(layout: CapabilityPackageStorageLayout, package_id: &str) -> String {
    match layout {
        CapabilityPackageStorageLayout::Standard => {
            format!("{}/manifest.json", legacy_package_dir(package_id))
        }
        CapabilityPackageStorageLayout::EspCompact => {
            format!("cpm/{:016x}.j", fnv1a64_hash(package_id))
        }
    }
}

fn rollback_path_for_layout(layout: CapabilityPackageStorageLayout, package_id: &str) -> String {
    match layout {
        CapabilityPackageStorageLayout::Standard => {
            format!("{REL_DIR_CAPABILITY_PACKAGE_ROLLBACK}/{package_id}.json")
        }
        CapabilityPackageStorageLayout::EspCompact => {
            format!("cpr/{:016x}.j", fnv1a64_hash(package_id))
        }
    }
}

fn package_rel_path_for_layout(
    layout: CapabilityPackageStorageLayout,
    package_id: &str,
    path: &str,
) -> String {
    match layout {
        CapabilityPackageStorageLayout::Standard => {
            format!("{}/{}", legacy_package_dir(package_id), path)
        }
        CapabilityPackageStorageLayout::EspCompact => {
            format!("cpf/{}", package_file_alias(package_id, path))
        }
    }
}

fn legacy_package_dir(package_id: &str) -> String {
    format!("{REL_DIR_CAPABILITY_PACKAGES}/{package_id}")
}

fn legacy_manifest_path(package_id: &str) -> Option<String> {
    let path = manifest_path_for_layout(CapabilityPackageStorageLayout::Standard, package_id);
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) && !is_safe_rel_path_on_esp(&path)
    {
        None
    } else {
        Some(path)
    }
}

fn legacy_rollback_path(package_id: &str) -> Option<String> {
    let path = rollback_path_for_layout(CapabilityPackageStorageLayout::Standard, package_id);
    if cfg!(any(target_arch = "xtensa", target_arch = "riscv32")) && !is_safe_rel_path_on_esp(&path)
    {
        None
    } else {
        Some(path)
    }
}

fn read_existing_package_file(
    fs: &dyn StateFs,
    current_rel_path: &str,
    legacy_rel_path: Option<&str>,
) -> Result<Option<(Vec<u8>, CapabilityPackageStorageLayout)>> {
    if let Some(bytes) = fs.read(current_rel_path)? {
        return Ok(Some((bytes, current_storage_layout())));
    }
    let Some(legacy_rel_path) = legacy_rel_path else {
        return Ok(None);
    };
    let Some(bytes) = fs.read(legacy_rel_path)? else {
        return Ok(None);
    };
    Ok(Some((bytes, CapabilityPackageStorageLayout::Standard)))
}

fn ensure_legacy_bundle_paths_safe_on_esp(
    package_id: &str,
    manifest: &CapabilityPackageManifest,
) -> Result<()> {
    let mut all_paths = Vec::with_capacity(
        1 + manifest.skill_fragments.len()
            + manifest.workflows.len()
            + manifest.policies.len()
            + manifest.assets.len(),
    );
    all_paths.push(manifest_path_for_layout(
        CapabilityPackageStorageLayout::Standard,
        package_id,
    ));
    all_paths.extend(bundle_file_rel_paths_for_layout(
        CapabilityPackageStorageLayout::Standard,
        package_id,
        manifest,
    ));
    if let Some(path) = all_paths
        .into_iter()
        .find(|path| !is_safe_rel_path_on_esp(path))
    {
        return Err(Error::config(
            "capability_package",
            format!("legacy capability package path exceeds ESP limit: {path}"),
        ));
    }
    Ok(())
}

fn parse_policy_documents(
    files: &[CapabilityPackageTextFile],
) -> Result<Vec<CapabilityPackagePolicyDocument>> {
    files
        .iter()
        .map(|file| {
            serde_json::from_str::<CapabilityPackagePolicyDocument>(&file.content)
                .map_err(|error| Error::config("capability_package", error.to_string()))
        })
        .collect()
}

fn read_text_file(
    fs: &dyn StateFs,
    rel_path: String,
    logical_path: &str,
) -> Result<CapabilityPackageTextFile> {
    let Some(bytes) = fs.read(&rel_path)? else {
        return Err(Error::config(
            "capability_package",
            "package text file is missing",
        ));
    };
    let content = String::from_utf8(bytes)
        .map_err(|error| Error::config("capability_package", error.to_string()))?;
    Ok(CapabilityPackageTextFile {
        path: logical_path.to_string(),
        content,
    })
}

fn read_asset_file(
    fs: &dyn StateFs,
    rel_path: String,
    original_path: &str,
) -> Result<CapabilityPackageAssetFile> {
    let Some(bytes) = fs.read(&rel_path)? else {
        return Err(Error::config(
            "capability_package",
            "package asset file is missing",
        ));
    };
    Ok(CapabilityPackageAssetFile {
        path: original_path.to_string(),
        content_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

fn validate_package_rel_path(path: &str, prefix: &str) -> Result<()> {
    if !path.starts_with(prefix)
        || path.contains("..")
        || path.starts_with('/')
        || path.ends_with('/')
        || path.trim().is_empty()
    {
        return Err(Error::config(
            "capability_package",
            "package file path is invalid",
        ));
    }
    Ok(())
}

fn normalize_identifier(value: &str, max_len: usize, field: &'static str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > max_len {
        return Err(Error::config(
            "capability_package",
            format!("{field} is empty or too long"),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(Error::config(
            "capability_package",
            format!("{field} contains invalid characters"),
        ));
    }
    Ok(trimmed.to_string())
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    crate::util::truncate_content_to_max(value.trim(), max_chars)
        .trim()
        .to_string()
}

fn push_with_limit(out: &mut String, addition: &str, max_chars: usize) -> bool {
    if out.len().saturating_add(addition.len()) <= max_chars {
        out.push_str(addition);
        return true;
    }
    let remaining = max_chars.saturating_sub(out.len());
    if remaining < 64 {
        return false;
    }
    out.push_str(&crate::util::truncate_content_to_max(addition, remaining));
    false
}

#[derive(Clone, Debug)]
struct LoadedCapabilityPackageBundle {
    manifest: CapabilityPackageManifest,
    skill_map: HashMap<String, CapabilityPackageTextFile>,
    workflow_map: HashMap<String, CapabilityPackageTextFile>,
    policy_documents: Vec<CapabilityPackagePolicyDocument>,
}

impl LoadedCapabilityPackageBundle {
    fn from_payload(payload: CapabilityPackageInstallPayload) -> Result<Self> {
        let skill_map = payload
            .skills
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect::<HashMap<_, _>>();
        let workflow_map = payload
            .workflows
            .into_iter()
            .map(|file| (file.path.clone(), file))
            .collect::<HashMap<_, _>>();
        let policy_documents = payload
            .policies
            .into_iter()
            .map(|file| {
                serde_json::from_str::<CapabilityPackagePolicyDocument>(&file.content)
                    .map_err(|error| Error::config("capability_package", error.to_string()))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            manifest: payload.manifest,
            skill_map,
            workflow_map,
            policy_documents,
        })
    }
}

impl CapabilityPackageWorkflowManifest {
    fn kind_label(&self) -> &'static str {
        match self.kind {
            CapabilityPackageWorkflowKind::WorkflowTemplate => "workflow",
            CapabilityPackageWorkflowKind::TaskRecipe => "task_recipe",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStateFs {
        files: Mutex<BTreeMap<String, Vec<u8>>>,
    }

    impl StateFs for MemoryStateFs {
        fn read(&self, rel_path: &str) -> Result<Option<Vec<u8>>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(rel_path)
                .cloned())
        }

        fn write(&self, rel_path: &str, data: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(rel_path.to_string(), data.to_vec());
            Ok(())
        }

        fn remove(&self, rel_path: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(rel_path);
            Ok(())
        }

        fn list_dir(&self, rel_path: &str) -> Result<Vec<String>> {
            let prefix = if rel_path.is_empty() {
                String::new()
            } else {
                format!("{}/", rel_path.trim_end_matches('/'))
            };
            let files = self.files.lock().unwrap_or_else(|error| error.into_inner());
            let mut names = BTreeSet::new();
            for key in files.keys() {
                if !key.starts_with(&prefix) {
                    continue;
                }
                let tail = &key[prefix.len()..];
                if tail.is_empty() {
                    continue;
                }
                if let Some((dir, _)) = tail.split_once('/') {
                    names.insert(format!("{dir}/"));
                } else {
                    names.insert(tail.to_string());
                }
            }
            Ok(names.into_iter().collect())
        }
    }

    fn sample_payload() -> CapabilityPackageInstallPayload {
        CapabilityPackageInstallPayload {
            manifest: CapabilityPackageManifest {
                package_id: "desk_flow".to_string(),
                version: "1.0.0".to_string(),
                display_name: "Desk Flow".to_string(),
                description: "Task recipes for desk workflows".to_string(),
                required_capabilities: vec!["task_execution".to_string()],
                channel_compatibility: vec!["telegram".to_string()],
                skill_fragments: vec![CapabilityPackageSkillFragmentManifest {
                    fragment_id: "desk_brief".to_string(),
                    title: "Desk Brief".to_string(),
                    summary: "Keep responses concise and operational".to_string(),
                    path: "skills/desk_brief.md".to_string(),
                }],
                workflows: vec![CapabilityPackageWorkflowManifest {
                    workflow_id: "triage_mail".to_string(),
                    title: "Mail Triage".to_string(),
                    summary: "Stepwise mail triage recipe".to_string(),
                    kind: CapabilityPackageWorkflowKind::TaskRecipe,
                    path: "workflows/triage_mail.md".to_string(),
                }],
                policies: vec![CapabilityPackagePolicyManifest {
                    policy_id: "tool_policy".to_string(),
                    path: "policies/tool_policy.json".to_string(),
                }],
                assets: vec![CapabilityPackageFileRef {
                    path: "assets/brief.txt".to_string(),
                }],
                rollback: CapabilityPackageRollbackContract::default(),
            },
            skills: vec![CapabilityPackageTextFile {
                path: "skills/desk_brief.md".to_string(),
                content: "Keep action plans short and operational.".to_string(),
            }],
            workflows: vec![CapabilityPackageTextFile {
                path: "workflows/triage_mail.md".to_string(),
                content: "1. scan mailbox\n2. group by urgency\n3. draft short replies".to_string(),
            }],
            policies: vec![CapabilityPackageTextFile {
                path: "policies/tool_policy.json".to_string(),
                content: serde_json::to_string(&CapabilityPackagePolicyDocument {
                    policy_id: "tool_policy".to_string(),
                    tool_policy_overlays: vec![CapabilityPackageToolPolicyOverlay {
                        tool_name: "web_search".to_string(),
                        user_llm: CapabilityPackageVisibilityOverride::ForceVisible,
                        system_llm: CapabilityPackageVisibilityOverride::Inherit,
                        internal_system_llm: CapabilityPackageVisibilityOverride::ForceHidden,
                        note: "workflow needs web search".to_string(),
                    }],
                })
                .unwrap(),
            }],
            assets: vec![CapabilityPackageAssetFile {
                path: "assets/brief.txt".to_string(),
                content_base64: base64::engine::general_purpose::STANDARD
                    .encode("asset-bytes".as_bytes()),
            }],
            enable_on_install: true,
        }
    }

    fn sample_capabilities() -> CapabilityPackageRuntimeCapabilities {
        CapabilityPackageRuntimeCapabilities {
            available: vec![
                "task_execution".to_string(),
                "runtime_skills".to_string(),
                "skills".to_string(),
                "tool_governance".to_string(),
                "channel:telegram".to_string(),
            ],
        }
    }

    #[test]
    fn esp_compact_paths_keep_long_package_entries_short() {
        let package_id = "qq_capability_overlay_package_with_a_really_long_identifier";
        let registry =
            capability_package_registry_path_for_layout(CapabilityPackageStorageLayout::EspCompact);
        let manifest =
            manifest_path_for_layout(CapabilityPackageStorageLayout::EspCompact, package_id);
        let rollback =
            rollback_path_for_layout(CapabilityPackageStorageLayout::EspCompact, package_id);
        let policy = package_rel_path_for_layout(
            CapabilityPackageStorageLayout::EspCompact,
            package_id,
            "policies/tool_policy_for_qq_c2c_runtime_overlay_with_deep_nested_name.json",
        );

        assert!(registry.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
        assert!(manifest.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
        assert!(rollback.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
        assert!(policy.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
        assert!(registry.starts_with("cfg/"));
        assert!(manifest.starts_with("cpm/"));
        assert!(rollback.starts_with("cpr/"));
        assert!(policy.starts_with("cpf/"));
    }

    #[test]
    fn read_existing_package_file_falls_back_to_legacy_layout() {
        let fs = MemoryStateFs::default();
        let package_id = "desk_flow";
        let legacy_manifest =
            manifest_path_for_layout(CapabilityPackageStorageLayout::Standard, package_id);
        fs.write(&legacy_manifest, br#"{"package_id":"desk_flow"}"#)
            .expect("legacy manifest write");

        let (bytes, layout) = read_existing_package_file(
            &fs,
            &manifest_path_for_layout(CapabilityPackageStorageLayout::EspCompact, package_id),
            Some(&legacy_manifest),
        )
        .expect("read existing")
        .expect("fallback payload");

        assert_eq!(layout, CapabilityPackageStorageLayout::Standard);
        assert_eq!(bytes, br#"{"package_id":"desk_flow"}"#);
    }

    #[test]
    fn legacy_bundle_safety_check_rejects_overlong_esp_paths() {
        let mut payload = sample_payload();
        payload.manifest.package_id =
            "qq_capability_overlay_package_with_a_really_long_identifier".to_string();
        payload.manifest.policies[0].path =
            "policies/tool_policy_for_qq_c2c_runtime_overlay_with_deep_nested_name.json"
                .to_string();

        let err =
            ensure_legacy_bundle_paths_safe_on_esp(&payload.manifest.package_id, &payload.manifest)
                .expect_err("legacy ESP path should be rejected");
        assert_eq!(err.stage(), "capability_package");
    }

    #[test]
    fn esp_compact_file_alias_is_package_scoped() {
        let left = package_rel_path_for_layout(
            CapabilityPackageStorageLayout::EspCompact,
            "desk_flow",
            "policies/tool_policy.json",
        );
        let right = package_rel_path_for_layout(
            CapabilityPackageStorageLayout::EspCompact,
            "desk_flow_alt",
            "policies/tool_policy.json",
        );

        assert_ne!(left, right);
        assert!(left.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
        assert!(right.len() <= MAX_CAPABILITY_PACKAGE_REL_PATH_LEN_ESP);
    }

    #[test]
    fn clear_bundle_files_for_esp_compact_layout_removes_manifest_and_payload_files() {
        let fs = MemoryStateFs::default();
        let payload = sample_payload();
        let package_id = payload.manifest.package_id.clone();
        let manifest_path =
            manifest_path_for_layout(CapabilityPackageStorageLayout::EspCompact, &package_id);
        fs.write(
            &manifest_path,
            &serde_json::to_vec(&payload.manifest).expect("serialize manifest"),
        )
        .expect("write compact manifest");
        for file in &payload.skills {
            fs.write(
                &package_rel_path_for_layout(
                    CapabilityPackageStorageLayout::EspCompact,
                    &package_id,
                    &file.path,
                ),
                file.content.as_bytes(),
            )
            .expect("write skill");
        }
        for file in &payload.workflows {
            fs.write(
                &package_rel_path_for_layout(
                    CapabilityPackageStorageLayout::EspCompact,
                    &package_id,
                    &file.path,
                ),
                file.content.as_bytes(),
            )
            .expect("write workflow");
        }
        for file in &payload.policies {
            fs.write(
                &package_rel_path_for_layout(
                    CapabilityPackageStorageLayout::EspCompact,
                    &package_id,
                    &file.path,
                ),
                file.content.as_bytes(),
            )
            .expect("write policy");
        }
        for file in &payload.assets {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(file.content_base64.trim())
                .expect("decode asset");
            fs.write(
                &package_rel_path_for_layout(
                    CapabilityPackageStorageLayout::EspCompact,
                    &package_id,
                    &file.path,
                ),
                &bytes,
            )
            .expect("write asset");
        }

        assert!(clear_bundle_files_for_layout(
            &fs,
            &package_id,
            CapabilityPackageStorageLayout::EspCompact,
        )
        .expect("clear compact layout"));
        assert!(fs.read(&manifest_path).expect("read manifest").is_none());
        for rel_path in bundle_file_rel_paths_for_layout(
            CapabilityPackageStorageLayout::EspCompact,
            &package_id,
            &payload.manifest,
        ) {
            assert!(fs.read(&rel_path).expect("read payload").is_none());
        }
    }

    #[test]
    fn install_rejects_missing_manifest_file_path() {
        let fs = MemoryStateFs::default();
        let mut payload = sample_payload();
        payload.skills.clear();
        let err = install_capability_package(&fs, &sample_capabilities(), &payload, 10)
            .expect_err("missing skill file should fail");
        assert_eq!(err.stage(), "capability_package");
    }

    #[test]
    fn enable_rejects_missing_runtime_capability() {
        let fs = MemoryStateFs::default();
        let mut payload = sample_payload();
        payload.enable_on_install = false;
        install_capability_package(&fs, &sample_capabilities(), &payload, 10)
            .expect("install disabled package");
        let err = set_capability_package_enabled(
            &fs,
            &CapabilityPackageRuntimeCapabilities {
                available: vec!["skills".to_string()],
            },
            "desk_flow",
            true,
            11,
        )
        .expect_err("enable without capabilities should fail");
        assert_eq!(err.stage(), "capability_package");
    }

    #[test]
    fn install_rejects_policy_conflict_with_enabled_package() {
        let fs = MemoryStateFs::default();
        let payload = sample_payload();
        install_capability_package(&fs, &sample_capabilities(), &payload, 10)
            .expect("install first package");
        let mut second = sample_payload();
        second.manifest.package_id = "desk_flow_alt".to_string();
        second.manifest.version = "1.0.1".to_string();
        second.manifest.workflows[0].workflow_id = "triage_mail_alt".to_string();
        second.workflows[0].content = "alternative".to_string();
        second.policies[0].content = serde_json::to_string(&CapabilityPackagePolicyDocument {
            policy_id: "tool_policy".to_string(),
            tool_policy_overlays: vec![CapabilityPackageToolPolicyOverlay {
                tool_name: "web_search".to_string(),
                user_llm: CapabilityPackageVisibilityOverride::ForceHidden,
                system_llm: CapabilityPackageVisibilityOverride::Inherit,
                internal_system_llm: CapabilityPackageVisibilityOverride::ForceHidden,
                note: String::new(),
            }],
        })
        .unwrap();
        let err = install_capability_package(&fs, &sample_capabilities(), &second, 11)
            .expect_err("policy conflict should fail");
        assert_eq!(err.stage(), "capability_package");
    }

    #[test]
    fn rollback_restores_previous_package_version() {
        let fs = MemoryStateFs::default();
        let payload = sample_payload();
        install_capability_package(&fs, &sample_capabilities(), &payload, 10).expect("install v1");
        let mut updated = sample_payload();
        updated.manifest.version = "2.0.0".to_string();
        updated.skills[0].content = "updated package body".to_string();
        install_capability_package(&fs, &sample_capabilities(), &updated, 20).expect("install v2");

        let outcome = rollback_capability_package(&fs, &sample_capabilities(), "desk_flow", 30)
            .expect("rollback");
        assert_eq!(outcome.operation, CapabilityPackageOperationKind::Rollback);
        assert_eq!(outcome.version, "1.0.0");
        let bundle = load_capability_package_bundle(&fs, "desk_flow")
            .expect("bundle load")
            .expect("bundle should exist");
        assert_eq!(bundle.manifest.version, "1.0.0");
        assert_eq!(
            bundle.skills[0].content,
            "Keep action plans short and operational."
        );
    }

    #[test]
    fn bundle_round_trip_preserves_logical_package_paths() {
        let fs = MemoryStateFs::default();
        let payload = sample_payload();
        install_capability_package(&fs, &sample_capabilities(), &payload, 10)
            .expect("install package");
        let bundle = load_capability_package_bundle(&fs, "desk_flow")
            .expect("bundle load")
            .expect("bundle should exist");
        assert_eq!(bundle.skills[0].path, "skills/desk_brief.md");
        assert_eq!(bundle.workflows[0].path, "workflows/triage_mail.md");
        assert_eq!(bundle.policies[0].path, "policies/tool_policy.json");
    }

    #[test]
    fn install_allows_overlay_inherit_to_compose_with_enabled_package() {
        let fs = MemoryStateFs::default();
        install_capability_package(&fs, &sample_capabilities(), &sample_payload(), 10)
            .expect("install first package");

        let mut second = sample_payload();
        second.manifest.package_id = "desk_flow_bridge".to_string();
        second.manifest.version = "1.0.1".to_string();
        second.manifest.workflows[0].workflow_id = "triage_mail_bridge".to_string();
        second.workflows[0].content = "bridge workflow".to_string();
        second.policies[0].content = serde_json::to_string(&CapabilityPackagePolicyDocument {
            policy_id: "tool_policy".to_string(),
            tool_policy_overlays: vec![CapabilityPackageToolPolicyOverlay {
                tool_name: "web_search".to_string(),
                user_llm: CapabilityPackageVisibilityOverride::Inherit,
                system_llm: CapabilityPackageVisibilityOverride::Inherit,
                internal_system_llm: CapabilityPackageVisibilityOverride::ForceHidden,
                note: String::new(),
            }],
        })
        .unwrap();

        install_capability_package(&fs, &sample_capabilities(), &second, 11)
            .expect("inherit overlay should compose");
        let merged =
            build_capability_package_tool_policy_set(&fs, &sample_capabilities(), "telegram")
                .expect("merged overlay set");
        let overlay = merged.get("web_search").expect("overlay exists");
        assert_eq!(
            overlay.user_llm,
            CapabilityPackageVisibilityOverride::ForceVisible
        );
        assert_eq!(
            overlay.internal_system_llm,
            CapabilityPackageVisibilityOverride::ForceHidden
        );
    }
}
