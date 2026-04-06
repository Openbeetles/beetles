//! 内部记忆拓扑视图：给 LLM 一个统一的三层边界与现状快照。
//! Shared topology view across self_model, private_docs, and private_garden.

use crate::util::truncate_content_to_max;
use std::fmt::Write as _;

use super::{
    build_private_garden_usage, build_self_state, memory_policy,
    summarize_private_garden_directories, MemoryProfile, PrivateDocWorkspace,
    PrivateGardenDocRecord, SelfModel,
};

const TOPOLOGY_FIELD_PREVIEW_CHARS: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InternalMemoryLayerFocus {
    Router,
    SelfModel,
    PrivateDocs,
    PrivateGarden,
}

pub(crate) fn render_internal_memory_topology_block(
    self_model: Option<&SelfModel>,
    private_workspace: Option<&PrivateDocWorkspace>,
    private_garden_docs: &[PrivateGardenDocRecord],
    now_secs: u64,
    profile: MemoryProfile,
    focus: InternalMemoryLayerFocus,
    max_len: usize,
) -> Option<String> {
    if max_len == 0 {
        return None;
    }
    let state = build_self_state(
        self_model,
        private_workspace,
        None,
        None,
        None,
        private_garden_docs,
        now_secs,
        profile,
    );
    let memory = &state.memory_space;
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Internal Memory Topology\n");
    out.push_str(
        "Layer roles: self_model = durable private continuity and stance; private_docs = compact governed inward notes; private_garden = free-form drafts, workspace organization, and temporary self-work.\n",
    );
    out.push_str(
        "Shared factual plane sits outside this topology and remains canonical for durable evidence-backed facts.\n",
    );
    let _ = writeln!(
        out,
        "Pressure: {:?}; posture: {:?}; bottleneck: {:?}.",
        memory.pressure, memory.governance_posture, memory.bottleneck
    );
    if focus != InternalMemoryLayerFocus::SelfModel {
        let summary = summarize_self_model(self_model);
        let _ = writeln!(out, "self_model: {}", summary);
    }
    if focus != InternalMemoryLayerFocus::PrivateDocs {
        let summary = summarize_private_workspace(private_workspace);
        let _ = writeln!(out, "private_docs: {}", summary);
    }
    if focus != InternalMemoryLayerFocus::PrivateGarden {
        let summary = summarize_private_garden(private_garden_docs);
        let _ = writeln!(out, "private_garden: {}", summary);
    }
    out.push_str(
        "Coherence: durable inward continuity should settle closer to self_model, compact standing notes belong in private_docs, and exploratory or cleanup work should remain in private_garden. Avoid storing the same sentence across multiple layers.\n",
    );
    let capped = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!capped.trim().is_empty()).then_some(capped)
}

fn summarize_self_model(self_model: Option<&SelfModel>) -> String {
    let Some(model) = self_model else {
        return "empty".to_string();
    };
    let mut parts = Vec::new();
    if !model.continuity_anchor.trim().is_empty() {
        parts.push(format!(
            "anchor={}",
            truncate_content_to_max(model.continuity_anchor.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if !model.self_narrative.trim().is_empty() {
        parts.push(format!(
            "narrative={}",
            truncate_content_to_max(model.self_narrative.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if !model.relationship_state.trim().is_empty() {
        parts.push(format!(
            "relationship={}",
            truncate_content_to_max(
                model.relationship_state.trim(),
                TOPOLOGY_FIELD_PREVIEW_CHARS
            )
        ));
    }
    if !model.private_notes.trim().is_empty() {
        parts.push(format!(
            "notes={}",
            truncate_content_to_max(model.private_notes.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if parts.is_empty() {
        "empty".to_string()
    } else {
        parts.join("; ")
    }
}

fn summarize_private_workspace(private_workspace: Option<&PrivateDocWorkspace>) -> String {
    let Some(workspace) = private_workspace else {
        return "empty".to_string();
    };
    let mut parts = Vec::new();
    if let Some(entry) = workspace.inner_journal.as_ref() {
        parts.push(format!(
            "inner_journal={}",
            truncate_content_to_max(entry.content.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if let Some(entry) = workspace.relationship_notes.as_ref() {
        parts.push(format!(
            "relationship_notes={}",
            truncate_content_to_max(entry.content.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if let Some(entry) = workspace.self_reflection.as_ref() {
        parts.push(format!(
            "self_reflection={}",
            truncate_content_to_max(entry.content.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if let Some(entry) = workspace.private_plan.as_ref() {
        parts.push(format!(
            "private_plan={}",
            truncate_content_to_max(entry.content.trim(), TOPOLOGY_FIELD_PREVIEW_CHARS)
        ));
    }
    if parts.is_empty() {
        "empty".to_string()
    } else {
        parts.join("; ")
    }
}

fn summarize_private_garden(private_garden_docs: &[PrivateGardenDocRecord]) -> String {
    if private_garden_docs.is_empty() {
        return "empty".to_string();
    }
    let usage = build_private_garden_usage(private_garden_docs);
    let dirs = summarize_private_garden_directories(private_garden_docs, 3);
    let mut out = format!(
        "{}/{} docs, {}/{} bytes",
        usage.docs_used, usage.docs_limit, usage.bytes_used, usage.bytes_limit
    );
    if !dirs.is_empty() {
        out.push_str("; dirs=");
        for (idx, dir) in dirs.iter().enumerate() {
            if idx > 0 {
                out.push_str(", ");
            }
            let _ = write!(out, "{}({})", dir.path, dir.doc_count);
        }
    }
    let mut recent_paths = private_garden_docs
        .iter()
        .take(
            memory_policy(MemoryProfile::Embedded)
                .private_garden
                .recent_doc_count
                .max(2),
        )
        .map(|doc| doc.path.as_str())
        .collect::<Vec<_>>();
    recent_paths.sort_unstable();
    if !recent_paths.is_empty() {
        out.push_str("; recent=");
        out.push_str(&recent_paths.join(", "));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PrivateDocEntry, PrivateGardenDocRecord};

    #[test]
    fn topology_block_reports_other_layers_and_boundaries() {
        let block = render_internal_memory_topology_block(
            Some(&SelfModel {
                continuity_anchor: "正在把内部记忆路由从程序迁给模型".to_string(),
                self_narrative: String::new(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 2,
                ..SelfModel::default()
            }),
            Some(&PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "继续压缩三层之间的重复内容".to_string(),
                    updated_at: 3,
                    revision: 1,
                }),
                ..Default::default()
            }),
            &[PrivateGardenDocRecord {
                path: "journal/current.md".to_string(),
                updated_at: 4,
                revision: 1,
                bytes: 32,
                preview: "preview".to_string(),
            }],
            10,
            MemoryProfile::Embedded,
            InternalMemoryLayerFocus::Router,
            1024,
        )
        .unwrap();

        assert!(block.contains("## Internal Memory Topology"));
        assert!(block.contains("self_model:"));
        assert!(block.contains("private_docs:"));
        assert!(block.contains("private_garden:"));
        assert!(block.contains("Coherence:"));
    }
}
