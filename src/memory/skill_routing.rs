//! Route durable writes between canonical factual memory and procedural runtime skills.

use crate::skills::{runtime_skill_name_for_topic, RuntimeSkillWrite};
use crate::util::{looks_like_raw_payload_text, procedural_text_signal_count};

use super::{LongTermMemoryDraft, LongTermMemoryKind, LongTermMemorySourceType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryPlane {
    Factual,
    Skill,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedMemoryDraft {
    pub plane: MemoryPlane,
    pub factual_draft: Option<LongTermMemoryDraft>,
    pub skill_write: Option<RuntimeSkillWrite>,
    pub reason: &'static str,
}

pub fn route_long_term_draft(draft: &LongTermMemoryDraft) -> RoutedMemoryDraft {
    let Some(normalized) = draft.normalized() else {
        return RoutedMemoryDraft {
            plane: MemoryPlane::Reject,
            factual_draft: None,
            skill_write: None,
            reason: "empty_or_invalid",
        };
    };
    if looks_like_raw_payload_text(&normalized.content) {
        return RoutedMemoryDraft {
            plane: MemoryPlane::Reject,
            factual_draft: None,
            skill_write: None,
            reason: "raw_payload_or_log",
        };
    }
    if is_procedural_experience(&normalized) {
        let name = runtime_skill_name_for_topic(&normalized.topic);
        let summary = build_skill_summary(&normalized.content);
        return RoutedMemoryDraft {
            plane: MemoryPlane::Skill,
            factual_draft: None,
            skill_write: Some(RuntimeSkillWrite {
                name,
                topic: normalized.topic.clone(),
                title: normalized.topic.replace('_', " "),
                summary,
                content: normalized.content.clone(),
                citations: normalized.supporting_citations.clone(),
                source_chat_id: normalized.source_chat_id.clone(),
                observed_at: normalized.observed_at.unwrap_or(0),
            }),
            reason: "procedural_experience",
        };
    }
    RoutedMemoryDraft {
        plane: MemoryPlane::Factual,
        factual_draft: Some(normalized),
        skill_write: None,
        reason: "durable_fact",
    }
}

fn is_procedural_experience(draft: &LongTermMemoryDraft) -> bool {
    let content = draft.content.trim();
    if content.is_empty() {
        return false;
    }
    let content_signal = procedural_text_signal_count(content) as usize;
    let source_bias = usize::from(matches!(
        draft
            .source_type
            .unwrap_or(LongTermMemorySourceType::Conversation),
        LongTermMemorySourceType::ManualTool | LongTermMemorySourceType::SystemRuntime
    ));
    let kind_bias = usize::from(matches!(
        draft.kind,
        LongTermMemoryKind::Project | LongTermMemoryKind::Task | LongTermMemoryKind::Fact
    ));
    let score = content_signal + source_bias + kind_bias;
    score >= 4
}

fn build_skill_summary(content: &str) -> String {
    content
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| content.trim().chars().take(96).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{LongTermMemoryKind, LongTermMemorySourceType};

    fn draft(content: &str) -> LongTermMemoryDraft {
        LongTermMemoryDraft {
            kind: LongTermMemoryKind::Fact,
            topic: "network_setup".to_string(),
            content: content.to_string(),
            keywords: Vec::new(),
            source_chat_id: Some("chat-1".to_string()),
            source_type: Some(LongTermMemorySourceType::ManualTool),
            source_scope: None,
            confidence: None,
            freshness: None,
            stale_hint: None,
            supporting_citations: Vec::new(),
            evidence_count: None,
            observed_at: Some(10),
            last_confirmed_at: None,
            source_revision: None,
        }
    }

    #[test]
    fn routes_bulleted_procedure_to_skill_plane() {
        let routed = route_long_term_draft(&draft(
            "- open device\n- run setup --fast\n- verify /tmp/log",
        ));
        assert_eq!(routed.plane, MemoryPlane::Skill);
        assert!(routed.skill_write.is_some());
    }

    #[test]
    fn keeps_plain_fact_in_factual_plane() {
        let routed = route_long_term_draft(&LongTermMemoryDraft {
            kind: LongTermMemoryKind::Profile,
            topic: "user_timezone".to_string(),
            content: "User timezone is Asia/Shanghai.".to_string(),
            keywords: vec!["timezone".to_string()],
            source_chat_id: Some("chat-1".to_string()),
            source_type: Some(LongTermMemorySourceType::Conversation),
            source_scope: None,
            confidence: None,
            freshness: None,
            stale_hint: None,
            supporting_citations: Vec::new(),
            evidence_count: None,
            observed_at: Some(10),
            last_confirmed_at: None,
            source_revision: None,
        });
        assert_eq!(routed.plane, MemoryPlane::Factual);
        assert!(routed.factual_draft.is_some());
    }

    #[test]
    fn rejects_raw_payload_shape() {
        let routed = route_long_term_draft(&draft(
            "[2026-04-03] level=info key=value\n[2026-04-03] payload={\"a\":1,\"b\":2}\n[2026-04-03] more={\"c\":3}",
        ));
        assert_eq!(routed.plane, MemoryPlane::Reject);
    }
}
