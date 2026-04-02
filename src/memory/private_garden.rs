//! 私有花园：LLM 自由组织的内部工作区，程序只做边界与轻量治理。
//! Free-form internal garden for LLM-owned continuity work.

use crate::error::{Error, Result};
use crate::util::{normalize_state_rel_path, truncate_content_to_max};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

const MAX_PRIVATE_GARDEN_PATH_LEN: usize = 96;
const MAX_PRIVATE_GARDEN_PREVIEW_CHARS: usize = 160;

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

pub fn render_private_garden_block(
    docs: &[PrivateGardenDocRecord],
    max_len: usize,
) -> Option<String> {
    let docs: Vec<&PrivateGardenDocRecord> = docs
        .iter()
        .filter(|doc| !doc.path.trim().is_empty())
        .filter(|doc| !doc.preview.trim().is_empty())
        .collect();
    if docs.is_empty() || max_len == 0 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Private Garden\n");
    out.push_str(
        "Free private workspace. Use `private_garden` when you want to inspect or reorganize it.\n",
    );
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
            512,
        )
        .unwrap();

        assert!(block.contains("## Private Garden"));
        assert!(block.contains("journal/tonight.md"));
        assert!(block.contains("自由空间和内核区分开来"));
    }
}
