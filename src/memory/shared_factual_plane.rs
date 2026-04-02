//! Shared factual plane helpers for personality and private-memory layers.

use crate::util::truncate_content_to_max;

use super::{recall_long_term_memory_block, LongTermMemoryStore, MemoryProfile, SessionMessage};

const SHARED_FACTUAL_HEADER_LEN: usize = 128;

fn build_shared_factual_query(recent: &[SessionMessage]) -> String {
    recent
        .iter()
        .rev()
        .find_map(|message| {
            let content = message.content.trim();
            (!content.is_empty()).then(|| truncate_content_to_max(content, 160).into_owned())
        })
        .unwrap_or_default()
}

pub(crate) fn render_shared_factual_plane_block(
    store: &dyn LongTermMemoryStore,
    chat_id: &str,
    summary_text: Option<&str>,
    recent: &[SessionMessage],
    max_len: usize,
    profile: MemoryProfile,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let query = build_shared_factual_query(recent);
    let recall_budget = max_len.saturating_sub(SHARED_FACTUAL_HEADER_LEN).max(96);
    let recalled = recall_long_term_memory_block(
        store,
        chat_id,
        &query,
        summary_text,
        recent,
        recall_budget,
        profile,
    );
    let mut out = String::with_capacity(max_len.min(768));
    out.push_str("## Shared Factual Plane\n");
    out.push_str(
        "Canonical shared record for evidence-backed durable user/world facts. Private layers may rely on it, but they do not own it.\n",
    );
    if let Some(recalled) = recalled {
        out.push_str(recalled.trim());
    } else {
        out.push_str("No recalled canonical facts for this turn.");
    }
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

pub(crate) fn render_private_memory_boundary_block(
    layer_name: &str,
    layer_role: &str,
    max_len: usize,
) -> Option<String> {
    if max_len < 96 {
        return None;
    }
    let mut out = String::with_capacity(max_len.min(384));
    out.push_str("## Shared/Private Boundary\n");
    out.push_str(
        "- Shared factual plane is canonical for durable, evidence-backed objective facts.\n",
    );
    out.push_str(&format!(
        "- {} may use shared facts as grounding, but must not rewrite, restate, or compete with them.\n",
        layer_name
    ));
    out.push_str(&format!("- Use {} only for {}.\n", layer_name, layer_role));
    out.push_str("- If something is objective and durable, leave it in shared facts; keep only subjective meaning, continuity, or governance here.\n");
    let rendered = truncate_content_to_max(out.trim_end(), max_len).into_owned();
    (!rendered.trim().is_empty()).then_some(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{LongTermMemoryEntry, LongTermMemoryStore};

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Vec<LongTermMemoryEntry>,
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            Ok(0)
        }

        fn recall(
            &self,
            _query: &str,
            _source_chat_id: Option<&str>,
            limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(self.entries.len())
        }
    }

    #[test]
    fn shared_factual_plane_block_wraps_recalled_memory() {
        let store = StubLongTermMemoryStore {
            entries: vec![LongTermMemoryEntry {
                id: "ltm-1".to_string(),
                kind: crate::memory::LongTermMemoryKind::Fact,
                topic: "primary_llm".to_string(),
                content: "当前主模型是 OpenAI。".to_string(),
                keywords: vec!["openai".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::World,
                confidence: crate::memory::LongTermMemoryConfidence::Medium,
                freshness: crate::memory::LongTermMemoryFreshness::Dynamic,
                stale_hint: crate::memory::LongTermMemoryStaleHint::ReviewBeforeUse,
                supporting_citations: vec!["transcript:chat-1#message=1".to_string()],
                evidence_count: 1,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 0,
                last_used_at: 0,
            }],
        };

        let block = render_shared_factual_plane_block(
            &store,
            "chat-1",
            Some("summary"),
            &[SessionMessage {
                role: "user".to_string(),
                content: "主模型现在是什么".to_string(),
            }],
            512,
            MemoryProfile::Embedded,
        )
        .unwrap();

        assert!(block.contains("## Shared Factual Plane"));
        assert!(block.contains("Long-term memory"));
        assert!(block.contains("primary_llm"));
    }

    #[test]
    fn private_memory_boundary_block_mentions_shared_facts() {
        let block = render_private_memory_boundary_block(
            "self_model",
            "durable private continuity and stance",
            256,
        )
        .unwrap();

        assert!(block.contains("Shared factual plane is canonical"));
        assert!(block.contains("self_model"));
    }
}
