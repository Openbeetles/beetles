//! 从 SkillStorage 加载 skill 描述；加载失败不阻塞启动。
//! Load skill descriptions from SkillStorage; load failure does not block startup.

use crate::error::{Error, Result};
use crate::platform::{SkillMetaStore, SkillStorage};

mod capability_atoms;
mod prompt_cache;
mod runtime;

pub use capability_atoms::{
    build_capability_atom_operator_summary, export_capability_atom_exchange_envelope,
    import_capability_atom_exchange_envelope, is_capability_atom_name,
    sync_capability_atoms_from_runtime_skills, CapabilityAtomComponentKind,
    CapabilityAtomComponentRef, CapabilityAtomExchangeEnvelope, CapabilityAtomImportOutcome,
    CapabilityAtomOperatorRecord, CapabilityAtomOperatorSummary, CapabilityAtomProvenance,
    CapabilityAtomRecord, CapabilityAtomSourceKind, CapabilityAtomSyncOutcome,
    CapabilityAtomTrustLevel,
};
pub(crate) use capability_atoms::{
    capability_atom_lifecycle_event_at, list_capability_atom_records,
};
pub use prompt_cache::SkillPromptCache;
pub use runtime::{
    build_runtime_skill_doctrine_snapshot, build_runtime_skill_genome_snapshot,
    build_runtime_skill_operator_summary, build_runtime_skill_recall_block, govern_runtime_skills,
    is_runtime_skill_name, record_runtime_skill_outcomes, retrieve_runtime_skill_hits,
    touch_runtime_skill_hits, upsert_runtime_skill, write_governed_runtime_skills,
    RuntimeSkillDoctrineClauseRecord, RuntimeSkillDoctrineSnapshot, RuntimeSkillGenomeDisposition,
    RuntimeSkillGenomeLineageRecord, RuntimeSkillGenomeNode, RuntimeSkillGenomeSnapshot,
    RuntimeSkillGovernanceOutcome, RuntimeSkillHit, RuntimeSkillOperatorRecord,
    RuntimeSkillOperatorSummary, RuntimeSkillRecallScoreBreakdown, RuntimeSkillRecord,
    RuntimeSkillReuseOutcome, RuntimeSkillStatus, RuntimeSkillStrategyDiff,
    RuntimeSkillStrategyDiffKind, RuntimeSkillWriteAction, RuntimeSkillWriteItemReport,
    RuntimeSkillWriteOutcome, RuntimeSkillWriteReason, RuntimeSkillWriteSource,
};
pub(crate) use runtime::{
    list_runtime_skill_records, retrieve_runtime_skill_hits_with_backend,
    runtime_skill_doctrine_event_at, runtime_skill_genome_event_at,
};

fn is_skill_name_valid(name: &str) -> bool {
    !name.is_empty() && !name.contains("..") && !name.contains('/') && !name.contains('\\')
}

const TAG: &str = "skills";
/// 单条 skill 内容最大字节数。
pub const MAX_SKILL_CONTENT_LEN: usize = 32 * 1024;
/// Default prompt budget for manually enabled skill docs on non-embedded profiles.
pub const DEFAULT_PROMPT_SKILL_MAX_CHARS: usize = 8192;
/// ESP steady-state prompt budget for manually enabled skill docs.
pub const ESP_PROMPT_SKILL_MAX_CHARS: usize = 2048;
/// ESP prompt budget for skill docs when resource pressure is already cautious.
pub const ESP_PROMPT_SKILL_CAUTION_MAX_CHARS: usize = 1024;
const RUNTIME_SKILL_PREFIX: &str = "runtime_skill__";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSkillWrite {
    pub name: String,
    pub topic: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub citations: Vec<String>,
    pub source_chat_id: Option<String>,
    pub observed_at: u64,
}

/// 返回所有 skill 名称（不含 .md）。失败或目录不存在返回空 vec，打日志不阻塞。
pub fn list_skill_names(storage: &dyn SkillStorage) -> Vec<String> {
    let names = match storage.list_names() {
        Ok(n) => n,
        Err(e) => {
            log::warn!("[{}] list_names failed: {}", TAG, e);
            return vec![];
        }
    };
    let mut out: Vec<String> = names;
    out.sort();
    out
}

#[derive(Default)]
struct SkillMetaSnapshot {
    order: Vec<String>,
    disabled: Vec<String>,
}

fn read_skill_meta_snapshot(meta_store: &dyn SkillMetaStore) -> Result<SkillMetaSnapshot> {
    let (order, disabled) = meta_store.read_meta()?;
    Ok(SkillMetaSnapshot {
        order: order
            .into_iter()
            .filter(|name| is_skill_name_valid(name))
            .collect(),
        disabled: disabled
            .into_iter()
            .filter(|name| is_skill_name_valid(name))
            .collect(),
    })
}

/// 读取指定 skill 的完整内容；name 不含 .md。超过 MAX_SKILL_CONTENT_LEN 截断。失败返回 None，打日志。
pub fn get_skill_content(storage: &dyn SkillStorage, name: &str) -> Option<String> {
    if !is_skill_name_valid(name) {
        log::warn!("[{}] invalid skill name (empty or contains .. / \\)", TAG);
        return None;
    }
    let buf = match storage.read(name) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("[{}] read {} failed: {}", TAG, name, e);
            return None;
        }
    };
    if buf.len() > MAX_SKILL_CONTENT_LEN {
        log::warn!(
            "[{}] skill {} truncated from {} to {}",
            TAG,
            name,
            buf.len(),
            MAX_SKILL_CONTENT_LEN
        );
    }
    let s = String::from_utf8_lossy(&buf[..buf.len().min(MAX_SKILL_CONTENT_LEN)]).into_owned();
    Some(s)
}

/// 从 meta_store 读取禁用列表，过滤非法 name 后返回。
pub fn get_disabled_skills(meta_store: &dyn SkillMetaStore) -> Vec<String> {
    match read_skill_meta_snapshot(meta_store) {
        Ok(meta) => meta.disabled,
        Err(error) => {
            log::warn!(
                "[{}] read_meta failed while loading disabled skills: {}",
                TAG,
                error
            );
            Vec::new()
        }
    }
}

/// 设置某 skill 的启用状态；enabled=false 加入禁用列表，enabled=true 从禁用列表移除。
pub fn set_skill_enabled(meta_store: &dyn SkillMetaStore, name: &str, enabled: bool) -> Result<()> {
    if !is_skill_name_valid(name) {
        return Err(Error::config(
            "set_skill_enabled",
            "skill name empty or contains .. / \\",
        ));
    }
    let (order, mut disabled) = meta_store.read_meta()?;
    if enabled {
        disabled.retain(|n| n != name);
    } else if !disabled.contains(&name.to_string()) {
        disabled.push(name.to_string());
    }
    meta_store.write_meta(&order, &disabled)
}

/// 从 meta_store 读取技能顺序；空或缺失则返回空 vec。
pub fn get_skills_order(meta_store: &dyn SkillMetaStore) -> Vec<String> {
    match read_skill_meta_snapshot(meta_store) {
        Ok(meta) => meta.order,
        Err(error) => {
            log::warn!(
                "[{}] read_meta failed while loading skill order: {}",
                TAG,
                error
            );
            Vec::new()
        }
    }
}

/// 写入技能顺序；order 中仅保留合法 name。
pub fn set_skills_order(meta_store: &dyn SkillMetaStore, order: &[String]) -> Result<()> {
    let (_, disabled) = meta_store.read_meta()?;
    let filtered: Vec<String> = order
        .iter()
        .filter(|s| is_skill_name_valid(s))
        .cloned()
        .collect();
    meta_store.write_meta(&filtered, &disabled)
}

/// 返回已启用且按顺序排列的 skill 名称，供 API 返回 order 字段。
fn try_get_ordered_enabled_skill_names(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
) -> Result<Vec<String>> {
    let meta = read_skill_meta_snapshot(meta_store)?;
    let all = list_skill_names(storage);
    let enabled: Vec<String> = all
        .into_iter()
        .filter(|n| !meta.disabled.contains(n))
        .collect();
    if meta.order.is_empty() {
        return Ok(enabled);
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in &meta.order {
        if enabled.contains(&name.to_string()) && seen.insert(name.as_str()) {
            out.push(name.clone());
        }
    }
    for name in &enabled {
        if !seen.contains(name.as_str()) {
            out.push(name.clone());
        }
    }
    Ok(out)
}

/// 返回已启用且按顺序排列的 skill 名称，供 API 返回 order 字段。
pub fn get_ordered_enabled_skill_names(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
) -> Vec<String> {
    match try_get_ordered_enabled_skill_names(meta_store, storage) {
        Ok(names) => names,
        Err(error) => {
            log::warn!(
                "[{}] read_meta failed; suppressing enabled skill exposure until meta recovers: {}",
                TAG,
                error
            );
            Vec::new()
        }
    }
}

/// 聚合所有**已启用** skill 内容为 system prompt 用字符串，总长不超过 max_chars。
pub(crate) fn try_build_skill_descriptions_for_system_prompt(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
    max_chars: usize,
) -> Result<String> {
    let names = try_get_ordered_enabled_skill_names(meta_store, storage)?;
    if names.is_empty() || max_chars == 0 {
        return Ok(String::new());
    }
    let mut out = String::with_capacity(max_chars.min(4096));
    for name in names {
        if is_runtime_skill_name(&name) || is_capability_atom_name(&name) {
            continue;
        }
        if out.len() >= max_chars {
            break;
        }
        let content = match get_skill_content(storage, &name) {
            Some(c) => c,
            None => continue,
        };
        let block = format!("### {}\n{}\n\n", name, content.trim());
        let remain = max_chars.saturating_sub(out.len());
        if block.len() <= remain {
            out.push_str(&block);
        } else {
            let take: String = block.chars().take(remain).collect();
            out.push_str(&take);
            break;
        }
    }
    out.truncate(max_chars);
    Ok(out)
}

pub fn build_skill_descriptions_for_system_prompt(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
    max_chars: usize,
) -> String {
    match try_build_skill_descriptions_for_system_prompt(meta_store, storage, max_chars) {
        Ok(rendered) => rendered,
        Err(error) => {
            log::warn!(
                "[{}] prompt skill assembly suppressed because skill meta read failed: {}",
                TAG,
                error
            );
            String::new()
        }
    }
}

/// Resolve active skill-doc prompt budget for the current runtime state.
///
/// This gates manually enabled prompt skills only; runtime skills and capability atoms
/// keep using the governed recall paths.
pub fn prompt_skill_budget_for_runtime_mode(
    runtime_mode: crate::runtime::RuntimeMode,
    pressure: crate::orchestrator::PressureLevel,
    configured_max_chars: usize,
    embedded_profile: bool,
) -> usize {
    if configured_max_chars == 0 {
        return 0;
    }
    if !embedded_profile {
        return configured_max_chars;
    }
    match runtime_mode {
        crate::runtime::RuntimeMode::Normal => match pressure {
            crate::orchestrator::PressureLevel::Normal => {
                configured_max_chars.min(ESP_PROMPT_SKILL_MAX_CHARS)
            }
            crate::orchestrator::PressureLevel::Cautious => {
                configured_max_chars.min(ESP_PROMPT_SKILL_CAUTION_MAX_CHARS)
            }
            crate::orchestrator::PressureLevel::Critical => 0,
        },
        crate::runtime::RuntimeMode::Booting
        | crate::runtime::RuntimeMode::Pairing
        | crate::runtime::RuntimeMode::ConfigActive
        | crate::runtime::RuntimeMode::VoiceExclusive
        | crate::runtime::RuntimeMode::Maintenance
        | crate::runtime::RuntimeMode::RecoverySafeMode => 0,
    }
}

/// 写入或覆盖指定 skill 文件。name 校验同 get_skill_content；content 长度 ≤ MAX_SKILL_CONTENT_LEN。
pub fn write_skill(storage: &dyn SkillStorage, name: &str, content: &str) -> Result<()> {
    if !is_skill_name_valid(name) {
        return Err(Error::config(
            "write_skill",
            "skill name empty or contains .. / \\",
        ));
    }
    if content.len() > MAX_SKILL_CONTENT_LEN {
        return Err(Error::config(
            "write_skill",
            format!(
                "content length {} exceeds {}",
                content.len(),
                MAX_SKILL_CONTENT_LEN
            ),
        ));
    }
    storage.write(name, content.as_bytes())
}

pub fn runtime_skill_name_for_topic(topic: &str) -> String {
    let mut slug = topic
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while slug.contains("__") {
        slug = slug.replace("__", "_");
    }
    let slug = slug.trim_matches('_');
    let suffix = if slug.is_empty() { "skill" } else { slug };
    format!(
        "{RUNTIME_SKILL_PREFIX}{}",
        suffix.chars().take(40).collect::<String>()
    )
}

/// 删除指定 skill 文件。
pub fn delete_skill(storage: &dyn SkillStorage, name: &str) -> Result<()> {
    if !is_skill_name_valid(name) {
        return Err(Error::config(
            "delete_skill",
            "skill name empty or contains .. / \\",
        ));
    }
    storage.remove(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestSkillStorage {
        names: Mutex<Vec<String>>,
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl TestSkillStorage {
        fn with_entries(entries: &[(&str, &str)]) -> Self {
            let mut names = Vec::new();
            let mut files = HashMap::new();
            for (name, content) in entries {
                names.push((*name).to_string());
                files.insert((*name).to_string(), content.as_bytes().to_vec());
            }
            Self {
                names: Mutex::new(names),
                files: Mutex::new(files),
            }
        }
    }

    impl SkillStorage for TestSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self.names.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .ok_or_else(|| Error::config("test_skill_storage_read", "missing"))
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            if !self
                .names
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|value| value == name)
            {
                self.names
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(name.to_string());
            }
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.names
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|value| value != name);
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    struct OrderedMetaStore {
        order: Vec<String>,
        disabled: Vec<String>,
    }

    impl SkillMetaStore for OrderedMetaStore {
        fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
            Ok((self.order.clone(), self.disabled.clone()))
        }

        fn write_meta(&self, _order: &[String], _disabled: &[String]) -> Result<()> {
            Ok(())
        }
    }

    struct FailingMetaStore;

    impl SkillMetaStore for FailingMetaStore {
        fn read_meta(&self) -> Result<(Vec<String>, Vec<String>)> {
            Err(Error::config("test_skill_meta_read", "corrupt meta"))
        }

        fn write_meta(&self, _order: &[String], _disabled: &[String]) -> Result<()> {
            Err(Error::config("test_skill_meta_write", "disabled"))
        }
    }

    #[test]
    fn list_skill_names_returns_full_sorted_inventory_without_global_truncation() {
        let storage = TestSkillStorage {
            names: Mutex::new((0..70).rev().map(|idx| format!("skill_{idx:02}")).collect()),
            files: Mutex::new(HashMap::new()),
        };

        let names = list_skill_names(&storage);

        assert_eq!(names.len(), 70);
        assert_eq!(names.first().map(String::as_str), Some("skill_00"));
        assert_eq!(names.last().map(String::as_str), Some("skill_69"));
    }

    #[test]
    fn prompt_skill_assembly_fails_closed_when_skill_meta_read_fails() {
        let storage = TestSkillStorage::with_entries(&[("alpha", "alpha body")]);
        let rendered = build_skill_descriptions_for_system_prompt(&FailingMetaStore, &storage, 512);

        assert!(rendered.is_empty());
        assert!(get_ordered_enabled_skill_names(&FailingMetaStore, &storage).is_empty());
    }

    #[test]
    fn embedded_prompt_skill_budget_follows_runtime_mode_and_pressure() {
        assert_eq!(
            prompt_skill_budget_for_runtime_mode(
                crate::runtime::RuntimeMode::Normal,
                crate::orchestrator::PressureLevel::Normal,
                DEFAULT_PROMPT_SKILL_MAX_CHARS,
                true,
            ),
            ESP_PROMPT_SKILL_MAX_CHARS
        );
        assert_eq!(
            prompt_skill_budget_for_runtime_mode(
                crate::runtime::RuntimeMode::Normal,
                crate::orchestrator::PressureLevel::Cautious,
                DEFAULT_PROMPT_SKILL_MAX_CHARS,
                true,
            ),
            ESP_PROMPT_SKILL_CAUTION_MAX_CHARS
        );
        assert_eq!(
            prompt_skill_budget_for_runtime_mode(
                crate::runtime::RuntimeMode::VoiceExclusive,
                crate::orchestrator::PressureLevel::Normal,
                DEFAULT_PROMPT_SKILL_MAX_CHARS,
                true,
            ),
            0
        );
        assert_eq!(
            prompt_skill_budget_for_runtime_mode(
                crate::runtime::RuntimeMode::Normal,
                crate::orchestrator::PressureLevel::Normal,
                DEFAULT_PROMPT_SKILL_MAX_CHARS,
                false,
            ),
            DEFAULT_PROMPT_SKILL_MAX_CHARS
        );
    }

    #[test]
    fn ordered_enabled_skill_names_follow_meta_snapshot_when_available() {
        let storage = TestSkillStorage::with_entries(&[
            ("gamma", "gamma body"),
            ("alpha", "alpha body"),
            ("beta", "beta body"),
        ]);
        let meta = OrderedMetaStore {
            order: vec!["beta".to_string(), "alpha".to_string()],
            disabled: vec!["gamma".to_string()],
        };

        let names = get_ordered_enabled_skill_names(&meta, &storage);

        assert_eq!(names, vec!["beta".to_string(), "alpha".to_string()]);
    }
}
