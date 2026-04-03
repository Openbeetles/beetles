//! 从 SkillStorage 加载 skill 描述；加载失败不阻塞启动。
//! Load skill descriptions from SkillStorage; load failure does not block startup.

use crate::error::{Error, Result};
use crate::platform::{SkillMetaStore, SkillStorage};

fn is_skill_name_valid(name: &str) -> bool {
    !name.is_empty() && !name.contains("..") && !name.contains('/') && !name.contains('\\')
}

const TAG: &str = "skills";
/// 单条 skill 内容最大字节数。
pub const MAX_SKILL_CONTENT_LEN: usize = 32 * 1024;
/// 列出 skill 数量上界。
const MAX_SKILL_COUNT: usize = 64;
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
    let mut out: Vec<String> = names.into_iter().take(MAX_SKILL_COUNT).collect();
    out.sort();
    out
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
    let (_, disabled) = match meta_store.read_meta() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    disabled
        .into_iter()
        .filter(|s| is_skill_name_valid(s))
        .collect()
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
    let (order, _) = match meta_store.read_meta() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    order
        .into_iter()
        .filter(|s| is_skill_name_valid(s))
        .collect()
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
pub fn get_ordered_enabled_skill_names(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
) -> Vec<String> {
    let disabled = get_disabled_skills(meta_store);
    let all = list_skill_names(storage);
    let enabled: Vec<String> = all.into_iter().filter(|n| !disabled.contains(n)).collect();
    let order = get_skills_order(meta_store);
    if order.is_empty() {
        return enabled;
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in &order {
        if enabled.contains(&name.to_string()) && seen.insert(name.as_str()) {
            out.push(name.clone());
        }
    }
    for name in &enabled {
        if !seen.contains(name.as_str()) {
            out.push(name.clone());
        }
    }
    out
}

/// 聚合所有**已启用** skill 内容为 system prompt 用字符串，总长不超过 max_chars。
pub fn build_skill_descriptions_for_system_prompt(
    meta_store: &dyn SkillMetaStore,
    storage: &dyn SkillStorage,
    max_chars: usize,
) -> String {
    let names = get_ordered_enabled_skill_names(meta_store, storage);
    if names.is_empty() || max_chars == 0 {
        return String::new();
    }
    let mut out = String::with_capacity(max_chars.min(4096));
    for name in names {
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
    out
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

pub fn upsert_runtime_skill(storage: &dyn SkillStorage, write: &RuntimeSkillWrite) -> Result<bool> {
    let name = if write.name.trim().is_empty() {
        runtime_skill_name_for_topic(&write.topic)
    } else {
        write.name.trim().to_string()
    };
    let rendered = render_runtime_skill_markdown(write);
    let changed = get_skill_content(storage, &name)
        .map(|existing| existing.trim() != rendered.trim())
        .unwrap_or(true);
    if !changed {
        return Ok(false);
    }
    write_skill(storage, &name, &rendered)?;
    Ok(true)
}

fn render_runtime_skill_markdown(write: &RuntimeSkillWrite) -> String {
    let mut out = String::new();
    out.push_str("<!-- beetle:runtime-skill -->\n");
    out.push_str("# ");
    out.push_str(if write.title.trim().is_empty() {
        write.topic.trim()
    } else {
        write.title.trim()
    });
    out.push_str("\n\n");
    out.push_str("Type: procedural_runtime_skill\n");
    out.push_str("Topic: ");
    out.push_str(write.topic.trim());
    out.push('\n');
    if let Some(chat_id) = write
        .source_chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        out.push_str("Source chat: ");
        out.push_str(chat_id);
        out.push('\n');
    }
    if write.observed_at > 0 {
        out.push_str("Observed at: ");
        out.push_str(&write.observed_at.to_string());
        out.push('\n');
    }
    out.push_str("\n## Summary\n");
    out.push_str(write.summary.trim());
    out.push_str("\n\n## Procedure\n");
    out.push_str(write.content.trim());
    if !write.citations.is_empty() {
        out.push_str("\n\n## Provenance\n");
        for citation in &write.citations {
            out.push_str("- ");
            out.push_str(citation.trim());
            out.push('\n');
        }
    }
    out
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
