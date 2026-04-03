//! 私有花园：LLM 自由组织的内部工作区，程序只做边界与轻量治理。
//! Free-form internal garden for LLM-owned continuity work.

use crate::error::{Error, Result};
use crate::util::{normalize_state_rel_path, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const MAX_PRIVATE_GARDEN_PATH_LEN: usize = 96;
const MAX_PRIVATE_GARDEN_PREVIEW_CHARS: usize = 160;
pub const PRIVATE_GARDEN_MAX_DOCS_PER_CHAT: usize = 16;
pub const PRIVATE_GARDEN_MAX_DOC_BYTES: usize = 8 * 1024;
pub const PRIVATE_GARDEN_TOTAL_BYTE_LIMIT: usize =
    PRIVATE_GARDEN_MAX_DOCS_PER_CHAT * PRIVATE_GARDEN_MAX_DOC_BYTES;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivateGardenDocRole {
    Workspace,
    Diary,
    Relational,
    Sealed,
}

impl PrivateGardenDocRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Diary => "diary",
            Self::Relational => "relational",
            Self::Sealed => "sealed",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateGardenDocRecord {
    pub path: String,
    pub updated_at: u64,
    pub revision: u32,
    pub bytes: usize,
    #[serde(default)]
    pub preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateGardenDoc {
    pub path: String,
    pub content: String,
    pub updated_at: u64,
    pub revision: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateGardenUsage {
    pub docs_used: usize,
    pub docs_limit: usize,
    pub docs_free: usize,
    pub bytes_used: usize,
    pub bytes_limit: usize,
    pub bytes_free: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrivateGardenDirectorySummary {
    pub path: String,
    pub doc_count: usize,
    pub bytes: usize,
    pub latest_updated_at: u64,
}

pub fn normalize_private_garden_doc_path(doc_path: &str) -> Result<String> {
    let normalized = normalize_state_rel_path(doc_path)?;
    if normalized.is_empty() {
        return Err(Error::config(
            "private_garden_path",
            "document path must not be empty",
        ));
    }
    if normalized.len() > MAX_PRIVATE_GARDEN_PATH_LEN {
        return Err(Error::config(
            "private_garden_path",
            format!(
                "document path exceeds {} bytes",
                MAX_PRIVATE_GARDEN_PATH_LEN
            ),
        ));
    }
    if normalized.ends_with('/') {
        return Err(Error::config(
            "private_garden_path",
            "document path must point to a file",
        ));
    }
    if normalized
        .split('/')
        .any(|segment| segment.trim().is_empty())
    {
        return Err(Error::config(
            "private_garden_path",
            "document path contains an empty segment",
        ));
    }
    Ok(normalized)
}

pub fn classify_private_garden_doc_path(doc_path: &str) -> PrivateGardenDocRole {
    let normalized = doc_path.trim_start_matches('/');
    if normalized.starts_with("sealed/") || normalized.starts_with("sealed_inner/") {
        PrivateGardenDocRole::Sealed
    } else if normalized.starts_with("diary/")
        || normalized.starts_with("journal/")
        || normalized.starts_with("private_diary/")
    {
        PrivateGardenDocRole::Diary
    } else if normalized.starts_with("relationship/") || normalized.starts_with("relational/") {
        PrivateGardenDocRole::Relational
    } else {
        PrivateGardenDocRole::Workspace
    }
}

pub(crate) fn build_private_garden_preview(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len().min(MAX_PRIVATE_GARDEN_PREVIEW_CHARS));
    let mut last_was_space = true;
    for ch in content.trim().chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                normalized.push(' ');
                last_was_space = true;
            }
            continue;
        }
        normalized.push(ch);
        last_was_space = false;
    }
    truncate_content_to_max(normalized.trim(), MAX_PRIVATE_GARDEN_PREVIEW_CHARS).into_owned()
}

pub fn build_private_garden_usage(docs: &[PrivateGardenDocRecord]) -> PrivateGardenUsage {
    let docs_used = docs.len();
    let bytes_used = docs.iter().map(|doc| doc.bytes).sum::<usize>();
    PrivateGardenUsage {
        docs_used,
        docs_limit: PRIVATE_GARDEN_MAX_DOCS_PER_CHAT,
        docs_free: PRIVATE_GARDEN_MAX_DOCS_PER_CHAT.saturating_sub(docs_used),
        bytes_used,
        bytes_limit: PRIVATE_GARDEN_TOTAL_BYTE_LIMIT,
        bytes_free: PRIVATE_GARDEN_TOTAL_BYTE_LIMIT.saturating_sub(bytes_used),
    }
}

pub fn summarize_private_garden_directories(
    docs: &[PrivateGardenDocRecord],
    limit: usize,
) -> Vec<PrivateGardenDirectorySummary> {
    let mut dirs = BTreeMap::<String, PrivateGardenDirectorySummary>::new();
    for doc in docs {
        let dir_path = doc
            .path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or(".");
        let entry =
            dirs.entry(dir_path.to_string())
                .or_insert_with(|| PrivateGardenDirectorySummary {
                    path: dir_path.to_string(),
                    doc_count: 0,
                    bytes: 0,
                    latest_updated_at: 0,
                });
        entry.doc_count = entry.doc_count.saturating_add(1);
        entry.bytes = entry.bytes.saturating_add(doc.bytes);
        entry.latest_updated_at = entry.latest_updated_at.max(doc.updated_at);
    }
    let mut summaries = dirs.into_values().collect::<Vec<_>>();
    summaries.sort_by(|a, b| {
        b.doc_count
            .cmp(&a.doc_count)
            .then_with(|| b.latest_updated_at.cmp(&a.latest_updated_at))
            .then_with(|| b.bytes.cmp(&a.bytes))
            .then_with(|| a.path.cmp(&b.path))
    });
    summaries.truncate(limit);
    summaries
}

pub fn render_private_garden_block(
    docs: &[PrivateGardenDocRecord],
    recent_doc_count: usize,
    max_len: usize,
) -> Option<String> {
    let mut all_docs = docs
        .iter()
        .filter(|doc| !doc.path.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    all_docs.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    if all_docs.is_empty() || max_len == 0 {
        return None;
    }
    let usage = build_private_garden_usage(&all_docs);
    let directories = summarize_private_garden_directories(&all_docs, 4);
    let mut role_counts = BTreeMap::<&'static str, usize>::new();
    for doc in &all_docs {
        let label = classify_private_garden_doc_path(&doc.path).label();
        *role_counts.entry(label).or_insert(0) += 1;
    }
    let docs: Vec<&PrivateGardenDocRecord> = all_docs
        .iter()
        .filter(|doc| !doc.preview.trim().is_empty())
        .take(recent_doc_count.max(1))
        .collect();
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Private Garden\n");
    out.push_str(
        "Free private space. `workspace/*` is for active working material, `diary/*` for inward journal traces, `relationship/*` for relationship-side private notes, and `sealed/*` for the most private inner material. Keep docs current by rewriting in place instead of appending a history trail.\n",
    );
    let _ = writeln!(
        out,
        "Capacity: {}/{} docs used ({} free), {}/{} bytes used ({} free).",
        usage.docs_used,
        usage.docs_limit,
        usage.docs_free,
        usage.bytes_used,
        usage.bytes_limit,
        usage.bytes_free
    );
    if !role_counts.is_empty() {
        out.push_str("Roles: ");
        for (idx, (label, count)) in role_counts.iter().enumerate() {
            if idx > 0 {
                out.push_str("; ");
            }
            let _ = write!(out, "{}={}", label, count);
        }
        out.push('\n');
    }
    if !directories.is_empty() {
        out.push_str("Folders: ");
        for (idx, dir) in directories.iter().enumerate() {
            if idx > 0 {
                out.push_str("; ");
            }
            let _ = write!(
                out,
                "{} ({} docs, {} bytes)",
                dir.path, dir.doc_count, dir.bytes
            );
        }
        out.push('\n');
    }
    for doc in docs {
        let _ = writeln!(
            out,
            "- {} (rev {}, updated={}): {}",
            doc.path, doc.revision, doc.updated_at, doc.preview
        );
    }
    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    let capped = truncate_content_to_max(trimmed, max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_private_garden_doc_path() {
        assert_eq!(
            normalize_private_garden_doc_path("/notes/self/idea.md").unwrap(),
            "notes/self/idea.md"
        );
        assert!(normalize_private_garden_doc_path("../escape").is_err());
        assert!(normalize_private_garden_doc_path("notes//bad").is_err());
        assert!(normalize_private_garden_doc_path("notes/").is_err());
    }

    #[test]
    fn classifies_private_garden_doc_roles() {
        assert_eq!(
            classify_private_garden_doc_path("workspace/notes.md"),
            PrivateGardenDocRole::Workspace
        );
        assert_eq!(
            classify_private_garden_doc_path("journal/tonight.md"),
            PrivateGardenDocRole::Diary
        );
        assert_eq!(
            classify_private_garden_doc_path("relationship/owner.md"),
            PrivateGardenDocRole::Relational
        );
        assert_eq!(
            classify_private_garden_doc_path("sealed/core.md"),
            PrivateGardenDocRole::Sealed
        );
    }

    #[test]
    fn renders_private_garden_block_with_recent_preview() {
        let block = render_private_garden_block(
            &[PrivateGardenDocRecord {
                path: "journal/tonight.md".to_string(),
                updated_at: 42,
                revision: 3,
                bytes: 128,
                preview: build_private_garden_preview(
                    "  想把自由空间和内核区分开来\n但又不能撕裂记忆 ",
                ),
            }],
            2,
            512,
        )
        .unwrap();

        assert!(block.contains("## Private Garden"));
        assert!(block.contains("Capacity: 1/16 docs used"));
        assert!(block.contains("Roles: diary=1"));
        assert!(block.contains("Folders: journal"));
        assert!(block.contains("journal/tonight.md"));
        assert!(block.contains("自由空间和内核区分开来"));
    }

    #[test]
    fn summarizes_private_garden_directories_by_density() {
        let summaries = summarize_private_garden_directories(
            &[
                PrivateGardenDocRecord {
                    path: "journal/a.md".to_string(),
                    updated_at: 5,
                    revision: 1,
                    bytes: 100,
                    preview: String::new(),
                },
                PrivateGardenDocRecord {
                    path: "journal/b.md".to_string(),
                    updated_at: 8,
                    revision: 1,
                    bytes: 80,
                    preview: String::new(),
                },
                PrivateGardenDocRecord {
                    path: "scratch/raw.md".to_string(),
                    updated_at: 9,
                    revision: 1,
                    bytes: 40,
                    preview: String::new(),
                },
            ],
            8,
        );

        assert_eq!(summaries[0].path, "journal");
        assert_eq!(summaries[0].doc_count, 2);
        assert_eq!(summaries[1].path, "scratch");
    }
}
