//! 记忆策略档位与共享参数。
//! Shared memory strategy profiles and policy values.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryProfile {
    Embedded,
    Standard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SessionSummaryPolicy {
    pub refresh_min_messages: usize,
    pub refresh_delta_messages: usize,
    pub recent_message_count: usize,
    pub fallback_recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub fallback_preview_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LongTermRecallPolicy {
    pub direct_recall_multiplier: usize,
    pub fallback_list_multiplier: usize,
    pub summary_grounding_max_len: usize,
    pub recent_grounding_message_count: usize,
    pub recent_grounding_max_len: usize,
    pub weak_query_short_chars: usize,
    pub weak_query_max_chars: usize,
    pub weak_query_max_words: usize,
    pub block_max_len_cap: usize,
    pub block_min_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LongTermExtractionPolicy {
    pub first_process_min_messages: usize,
    pub min_messages_between_requests: usize,
    pub force_process_after_messages: usize,
    pub low_signal_user_chars: usize,
    pub low_signal_user_words: usize,
    pub low_signal_reply_chars: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_memory_max_len: usize,
    pub batch_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExecutionStatePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_state_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelfModelPolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_model_max_len: usize,
    pub factual_grounding_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivateDocsPolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_workspace_max_len: usize,
    pub factual_grounding_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivateGardenPolicy {
    pub recent_doc_count: usize,
    pub render_max_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivateGardenGovernancePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub grounding_max_len: usize,
    pub existing_doc_count: usize,
    pub existing_doc_max_chars: usize,
    pub existing_docs_max_chars: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
    pub max_writes: usize,
    pub max_moves: usize,
    pub max_deletes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InternalMemoryRoutingPolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub grounding_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelfStatePolicy {
    pub render_max_len: usize,
    pub cautious_usage_percent: u8,
    pub tight_usage_percent: u8,
    pub recent_activity_window_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MemoryPolicy {
    pub session_summary: SessionSummaryPolicy,
    pub long_term_recall: LongTermRecallPolicy,
    pub long_term_extraction: LongTermExtractionPolicy,
    pub execution_state: ExecutionStatePolicy,
    pub self_model: SelfModelPolicy,
    pub private_docs: PrivateDocsPolicy,
    pub private_garden: PrivateGardenPolicy,
    pub private_garden_governance: PrivateGardenGovernancePolicy,
    pub internal_memory_routing: InternalMemoryRoutingPolicy,
    pub self_state: SelfStatePolicy,
}

const EMBEDDED_MEMORY_POLICY: MemoryPolicy = MemoryPolicy {
    session_summary: SessionSummaryPolicy {
        refresh_min_messages: 20,
        refresh_delta_messages: 10,
        recent_message_count: 16,
        fallback_recent_message_count: 4,
        transcript_preview_chars: 160,
        fallback_preview_chars: 80,
    },
    long_term_recall: LongTermRecallPolicy {
        direct_recall_multiplier: 2,
        fallback_list_multiplier: 3,
        summary_grounding_max_len: 160,
        recent_grounding_message_count: 2,
        recent_grounding_max_len: 160,
        weak_query_short_chars: 6,
        weak_query_max_chars: 12,
        weak_query_max_words: 2,
        block_max_len_cap: 768,
        block_min_len: 160,
    },
    long_term_extraction: LongTermExtractionPolicy {
        first_process_min_messages: 10,
        min_messages_between_requests: 8,
        force_process_after_messages: 14,
        low_signal_user_chars: 6,
        low_signal_user_words: 2,
        low_signal_reply_chars: 48,
        substantive_user_chars: 12,
        substantive_reply_chars: 36,
        substantive_combined_chars: 84,
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_memory_max_len: 512,
        batch_size: 3,
    },
    execution_state: ExecutionStatePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_state_max_len: 320,
        render_max_len: 320,
        substantive_user_chars: 10,
        substantive_reply_chars: 24,
        substantive_combined_chars: 56,
    },
    self_model: SelfModelPolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_model_max_len: 320,
        factual_grounding_max_len: 220,
        render_max_len: 320,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
    },
    private_docs: PrivateDocsPolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_workspace_max_len: 480,
        factual_grounding_max_len: 220,
        render_max_len: 420,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
    },
    private_garden: PrivateGardenPolicy {
        recent_doc_count: 2,
        render_max_len: 320,
    },
    private_garden_governance: PrivateGardenGovernancePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        grounding_max_len: 220,
        existing_doc_count: 4,
        existing_doc_max_chars: 180,
        existing_docs_max_chars: 640,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
        max_writes: 2,
        max_moves: 2,
        max_deletes: 2,
    },
    internal_memory_routing: InternalMemoryRoutingPolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        grounding_max_len: 240,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
    },
    self_state: SelfStatePolicy {
        render_max_len: 280,
        cautious_usage_percent: 65,
        tight_usage_percent: 85,
        recent_activity_window_secs: 6 * 60 * 60,
    },
};

const STANDARD_MEMORY_POLICY: MemoryPolicy = MemoryPolicy {
    session_summary: SessionSummaryPolicy {
        refresh_min_messages: 16,
        refresh_delta_messages: 8,
        recent_message_count: 24,
        fallback_recent_message_count: 6,
        transcript_preview_chars: 240,
        fallback_preview_chars: 120,
    },
    long_term_recall: LongTermRecallPolicy {
        direct_recall_multiplier: 3,
        fallback_list_multiplier: 4,
        summary_grounding_max_len: 320,
        recent_grounding_message_count: 3,
        recent_grounding_max_len: 280,
        weak_query_short_chars: 8,
        weak_query_max_chars: 16,
        weak_query_max_words: 3,
        block_max_len_cap: 1024,
        block_min_len: 192,
    },
    long_term_extraction: LongTermExtractionPolicy {
        first_process_min_messages: 6,
        min_messages_between_requests: 4,
        force_process_after_messages: 8,
        low_signal_user_chars: 6,
        low_signal_user_words: 2,
        low_signal_reply_chars: 48,
        substantive_user_chars: 8,
        substantive_reply_chars: 24,
        substantive_combined_chars: 56,
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_memory_max_len: 1024,
        batch_size: 4,
    },
    execution_state: ExecutionStatePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_state_max_len: 512,
        render_max_len: 512,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
    },
    self_model: SelfModelPolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_model_max_len: 512,
        factual_grounding_max_len: 320,
        render_max_len: 512,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
    },
    private_docs: PrivateDocsPolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_workspace_max_len: 768,
        factual_grounding_max_len: 320,
        render_max_len: 640,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
    },
    private_garden: PrivateGardenPolicy {
        recent_doc_count: 4,
        render_max_len: 512,
    },
    private_garden_governance: PrivateGardenGovernancePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        grounding_max_len: 320,
        existing_doc_count: 6,
        existing_doc_max_chars: 260,
        existing_docs_max_chars: 1280,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
        max_writes: 3,
        max_moves: 3,
        max_deletes: 3,
    },
    internal_memory_routing: InternalMemoryRoutingPolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        grounding_max_len: 320,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
    },
    self_state: SelfStatePolicy {
        render_max_len: 360,
        cautious_usage_percent: 65,
        tight_usage_percent: 85,
        recent_activity_window_secs: 12 * 60 * 60,
    },
};

pub(crate) fn memory_policy(profile: MemoryProfile) -> &'static MemoryPolicy {
    match profile {
        MemoryProfile::Embedded => &EMBEDDED_MEMORY_POLICY,
        MemoryProfile::Standard => &STANDARD_MEMORY_POLICY,
    }
}

/// 长期记忆持久化治理（TTL / kind budget）当前在两端平台保持统一，
/// 避免 ESP / Linux 对同一状态文件裁剪出不同结果。
/// Prompt 注入窗口与提取节奏按 MemoryProfile 分档，但持久化治理口径先共享。
pub(crate) fn shared_long_term_governance_policy() -> LongTermRecallPolicy {
    STANDARD_MEMORY_POLICY.long_term_recall
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_profile_keeps_larger_memory_windows() {
        let embedded = memory_policy(MemoryProfile::Embedded);
        let standard = memory_policy(MemoryProfile::Standard);
        assert!(
            standard.session_summary.recent_message_count
                > embedded.session_summary.recent_message_count
        );
        assert!(
            standard.long_term_recall.block_max_len_cap
                > embedded.long_term_recall.block_max_len_cap
        );
        assert!(
            standard.long_term_extraction.recent_message_count
                > embedded.long_term_extraction.recent_message_count
        );
        assert!(standard.execution_state.render_max_len > embedded.execution_state.render_max_len);
        assert!(standard.self_model.render_max_len > embedded.self_model.render_max_len);
        assert!(standard.private_docs.render_max_len > embedded.private_docs.render_max_len);
        assert!(standard.private_garden.render_max_len > embedded.private_garden.render_max_len);
        assert!(
            standard.private_garden_governance.existing_docs_max_chars
                > embedded.private_garden_governance.existing_docs_max_chars
        );
        assert!(
            standard.internal_memory_routing.recent_message_count
                > embedded.internal_memory_routing.recent_message_count
        );
        assert!(standard.self_state.render_max_len > embedded.self_state.render_max_len);
    }
}
