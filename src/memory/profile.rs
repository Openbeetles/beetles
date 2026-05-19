//! 记忆策略档位与共享参数。
//! Shared memory strategy profiles and policy values.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryProfile {
    Embedded,
    Standard,
}

impl MemoryProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Standard => "standard",
        }
    }

    pub const fn memory_system_kind(self) -> MemorySystemKind {
        match self {
            Self::Embedded => MemorySystemKind::EspCompact,
            Self::Standard => MemorySystemKind::LinuxFull,
        }
    }
}

impl From<MemoryProfile> for MemorySystemKind {
    fn from(value: MemoryProfile) -> Self {
        value.memory_system_kind()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySystemKind {
    EspCompact,
    LinuxFull,
}

impl MemorySystemKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EspCompact => "esp_compact",
            Self::LinuxFull => "linux_full",
        }
    }

    pub const fn memory_profile(self) -> MemoryProfile {
        match self {
            Self::EspCompact => MemoryProfile::Embedded,
            Self::LinuxFull => MemoryProfile::Standard,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryCapabilityClass {
    ConstrainedDevice,
    ExpandedDevice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryHygieneLevel {
    Minimal,
    Standard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptParticipationPlan {
    pub load_l1_constitutional: bool,
    pub load_l1_session: bool,
    pub load_l2_governed_recall: bool,
    pub load_l2_background_governance: bool,
    pub load_l3_private_depth: bool,
}

impl Default for PromptParticipationPlan {
    fn default() -> Self {
        Self::full()
    }
}

impl PromptParticipationPlan {
    pub const fn full() -> Self {
        Self {
            load_l1_constitutional: true,
            load_l1_session: true,
            load_l2_governed_recall: true,
            load_l2_background_governance: true,
            load_l3_private_depth: true,
        }
    }

    pub const fn embedded_first_turn_default() -> Self {
        Self {
            load_l1_constitutional: true,
            load_l1_session: true,
            load_l2_governed_recall: false,
            load_l2_background_governance: false,
            load_l3_private_depth: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptAssemblyPlan {
    pub memory_system_kind: MemorySystemKind,
    pub participation_plan: PromptParticipationPlan,
    pub include_capability_package_text: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfRuntimeAuthorityPlan {
    pub allow_direct_inner_life: bool,
    pub allow_direct_private_docs: bool,
    pub allow_direct_private_garden: bool,
    pub allow_direct_self_model: bool,
    pub allow_direct_self_authored_core: bool,
    pub allow_direct_self_continuity: bool,
    pub allow_direct_boundary_persona: bool,
    pub allow_direct_outer_voice: bool,
    pub allow_factual_refresh_request: bool,
    pub allow_method_distillation: bool,
}

impl SelfRuntimeAuthorityPlan {
    pub const fn allows_relationship_governance(self) -> bool {
        self.allow_direct_self_authored_core
            || self.allow_direct_boundary_persona
            || self.allow_direct_outer_voice
    }

    pub fn allows_source_id(self, source_id: &str) -> bool {
        match source_id {
            "inner_life" => self.allow_direct_inner_life,
            "private_docs" => self.allow_direct_private_docs,
            "private_garden" => self.allow_direct_private_garden,
            "self_model" => self.allow_direct_self_model,
            "self_authored_core" => self.allow_direct_self_authored_core,
            "self_continuity" => self.allow_direct_self_continuity,
            "boundary_persona" => self.allow_direct_boundary_persona,
            "outer_voice" => self.allow_direct_outer_voice,
            _ => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PromptParticipationPolicy {
    pub first_user_turn_l2_enabled: bool,
    pub first_user_turn_background_enabled: bool,
    pub non_user_turn_private_projection_enabled: bool,
    pub tool_round_recall_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PromptContextNormalizationBudget {
    pub summary_max_len: usize,
    pub constitutional_stack_max_len: usize,
    pub active_task_context_max_len: usize,
    pub governed_memory_evidence_max_len: usize,
    pub background_governance_max_len: usize,
    pub inward_growth_max_len: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryCapabilityProfile {
    pub class: MemoryCapabilityClass,
    pub archive_prompt_max_items: usize,
    pub archive_prompt_max_chars: usize,
    pub shared_factual_archive_hits: usize,
    pub exact_slot_lookup_enabled: bool,
    pub prompt_exact_lookup_enabled: bool,
    pub slot_query_max_results: usize,
    pub background_hygiene_level: MemoryHygieneLevel,
    pub runtime_max_jobs_per_tick: usize,
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
pub(crate) struct AutonomyStrategyPolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_strategy_max_len: usize,
    pub grounding_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
    pub min_idle_interval_secs: u64,
    pub max_idle_interval_secs: u64,
    pub refresh_interval_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WorldSensePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_world_sense_max_len: usize,
    pub grounding_max_len: usize,
    pub snapshot_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
    pub refresh_interval_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OuterVoicePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_outer_voice_max_len: usize,
    pub grounding_max_len: usize,
    pub snapshot_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
    pub refresh_interval_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InnerLifePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_inner_life_max_len: usize,
    pub grounding_max_len: usize,
    pub render_max_len: usize,
    pub substantive_user_chars: usize,
    pub substantive_reply_chars: usize,
    pub substantive_combined_chars: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelfContinuityPolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub existing_continuity_max_len: usize,
    pub grounding_max_len: usize,
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
pub(crate) struct SelfRuntimePolicy {
    pub recent_message_count: usize,
    pub transcript_preview_chars: usize,
    pub grounding_max_len: usize,
    pub idle_tick_interval_secs: u64,
    pub active_chat_window_secs: u64,
    pub max_jobs_per_tick: usize,
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
    pub world_sense: WorldSensePolicy,
    pub outer_voice: OuterVoicePolicy,
    pub autonomy_strategy: AutonomyStrategyPolicy,
    pub inner_life: InnerLifePolicy,
    pub self_continuity: SelfContinuityPolicy,
    pub private_docs: PrivateDocsPolicy,
    pub private_garden: PrivateGardenPolicy,
    pub private_garden_governance: PrivateGardenGovernancePolicy,
    pub self_runtime: SelfRuntimePolicy,
    pub self_state: SelfStatePolicy,
}

const EMBEDDED_PROMPT_PARTICIPATION_POLICY: PromptParticipationPolicy = PromptParticipationPolicy {
    first_user_turn_l2_enabled: true,
    first_user_turn_background_enabled: false,
    non_user_turn_private_projection_enabled: false,
    tool_round_recall_enabled: true,
};

const STANDARD_PROMPT_PARTICIPATION_POLICY: PromptParticipationPolicy = PromptParticipationPolicy {
    first_user_turn_l2_enabled: true,
    first_user_turn_background_enabled: true,
    non_user_turn_private_projection_enabled: true,
    tool_round_recall_enabled: true,
};

const EMBEDDED_MEMORY_POLICY: MemoryPolicy = MemoryPolicy {
    session_summary: SessionSummaryPolicy {
        refresh_min_messages: 40,
        refresh_delta_messages: 20,
        recent_message_count: 12,
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
    world_sense: WorldSensePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_world_sense_max_len: 480,
        grounding_max_len: 220,
        snapshot_max_len: 320,
        render_max_len: 360,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
        refresh_interval_secs: 4 * 60 * 60,
    },
    outer_voice: OuterVoicePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_outer_voice_max_len: 360,
        grounding_max_len: 220,
        snapshot_max_len: 320,
        render_max_len: 320,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
        refresh_interval_secs: 3 * 60 * 60,
    },
    autonomy_strategy: AutonomyStrategyPolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_strategy_max_len: 540,
        grounding_max_len: 220,
        render_max_len: 420,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
        min_idle_interval_secs: 8 * 60,
        max_idle_interval_secs: 40 * 60,
        refresh_interval_secs: 6 * 60 * 60,
    },
    inner_life: InnerLifePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_inner_life_max_len: 480,
        grounding_max_len: 220,
        render_max_len: 420,
        substantive_user_chars: 8,
        substantive_reply_chars: 20,
        substantive_combined_chars: 48,
    },
    self_continuity: SelfContinuityPolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        existing_continuity_max_len: 420,
        grounding_max_len: 220,
        render_max_len: 360,
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
    self_runtime: SelfRuntimePolicy {
        recent_message_count: 6,
        transcript_preview_chars: 140,
        grounding_max_len: 220,
        idle_tick_interval_secs: 20 * 60,
        active_chat_window_secs: 24 * 60 * 60,
        max_jobs_per_tick: 2,
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
    world_sense: WorldSensePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_world_sense_max_len: 640,
        grounding_max_len: 320,
        snapshot_max_len: 420,
        render_max_len: 560,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
        refresh_interval_secs: 3 * 60 * 60,
    },
    outer_voice: OuterVoicePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_outer_voice_max_len: 512,
        grounding_max_len: 320,
        snapshot_max_len: 420,
        render_max_len: 420,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
        refresh_interval_secs: 2 * 60 * 60,
    },
    autonomy_strategy: AutonomyStrategyPolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_strategy_max_len: 768,
        grounding_max_len: 320,
        render_max_len: 640,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
        min_idle_interval_secs: 5 * 60,
        max_idle_interval_secs: 30 * 60,
        refresh_interval_secs: 4 * 60 * 60,
    },
    inner_life: InnerLifePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_inner_life_max_len: 768,
        grounding_max_len: 320,
        render_max_len: 640,
        substantive_user_chars: 6,
        substantive_reply_chars: 18,
        substantive_combined_chars: 40,
    },
    self_continuity: SelfContinuityPolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        existing_continuity_max_len: 640,
        grounding_max_len: 320,
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
    self_runtime: SelfRuntimePolicy {
        recent_message_count: 10,
        transcript_preview_chars: 220,
        grounding_max_len: 320,
        idle_tick_interval_secs: 10 * 60,
        active_chat_window_secs: 48 * 60 * 60,
        max_jobs_per_tick: 4,
    },
    self_state: SelfStatePolicy {
        render_max_len: 360,
        cautious_usage_percent: 65,
        tight_usage_percent: 85,
        recent_activity_window_secs: 12 * 60 * 60,
    },
};

const EMBEDDED_MEMORY_CAPABILITY_PROFILE: MemoryCapabilityProfile = MemoryCapabilityProfile {
    class: MemoryCapabilityClass::ConstrainedDevice,
    archive_prompt_max_items: 3,
    archive_prompt_max_chars: 512,
    shared_factual_archive_hits: 2,
    exact_slot_lookup_enabled: true,
    prompt_exact_lookup_enabled: true,
    slot_query_max_results: 4,
    background_hygiene_level: MemoryHygieneLevel::Minimal,
    runtime_max_jobs_per_tick: 2,
};

const STANDARD_MEMORY_CAPABILITY_PROFILE: MemoryCapabilityProfile = MemoryCapabilityProfile {
    class: MemoryCapabilityClass::ExpandedDevice,
    archive_prompt_max_items: 4,
    archive_prompt_max_chars: 768,
    shared_factual_archive_hits: 3,
    exact_slot_lookup_enabled: true,
    prompt_exact_lookup_enabled: true,
    slot_query_max_results: 8,
    background_hygiene_level: MemoryHygieneLevel::Standard,
    runtime_max_jobs_per_tick: 4,
};

pub(crate) fn memory_policy(
    memory_system_kind: impl Into<MemorySystemKind>,
) -> &'static MemoryPolicy {
    match memory_system_kind.into() {
        MemorySystemKind::EspCompact => &EMBEDDED_MEMORY_POLICY,
        MemorySystemKind::LinuxFull => &STANDARD_MEMORY_POLICY,
    }
}

pub(crate) fn memory_capability_profile(
    memory_system_kind: impl Into<MemorySystemKind>,
) -> &'static MemoryCapabilityProfile {
    match memory_system_kind.into() {
        MemorySystemKind::EspCompact => &EMBEDDED_MEMORY_CAPABILITY_PROFILE,
        MemorySystemKind::LinuxFull => &STANDARD_MEMORY_CAPABILITY_PROFILE,
    }
}

pub(crate) fn prompt_participation_policy(
    memory_system_kind: impl Into<MemorySystemKind>,
) -> PromptParticipationPolicy {
    match memory_system_kind.into() {
        MemorySystemKind::EspCompact => EMBEDDED_PROMPT_PARTICIPATION_POLICY,
        MemorySystemKind::LinuxFull => STANDARD_PROMPT_PARTICIPATION_POLICY,
    }
}

fn scaled_prompt_budget(
    system_budget: usize,
    numerator: usize,
    denominator: usize,
    floor: usize,
    cap: usize,
) -> usize {
    if system_budget == 0 || denominator == 0 {
        return 0;
    }
    let cap = cap.min(system_budget);
    let floor = floor.min(cap);
    system_budget
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(0)
        .max(floor)
        .min(cap)
}

pub(crate) fn prompt_context_normalization_budget(
    memory_system_kind: MemorySystemKind,
    system_budget: usize,
) -> PromptContextNormalizationBudget {
    let policy = memory_policy(memory_system_kind);
    match memory_system_kind {
        MemorySystemKind::EspCompact => PromptContextNormalizationBudget {
            summary_max_len: policy
                .long_term_recall
                .summary_grounding_max_len
                .min(system_budget),
            constitutional_stack_max_len: scaled_prompt_budget(system_budget, 1, 4, 160, 640),
            active_task_context_max_len: scaled_prompt_budget(system_budget, 1, 4, 160, 640),
            governed_memory_evidence_max_len: scaled_prompt_budget(
                system_budget,
                3,
                8,
                policy.long_term_recall.block_min_len,
                policy.long_term_recall.block_max_len_cap,
            ),
            background_governance_max_len: scaled_prompt_budget(system_budget, 1, 5, 64, 512),
            inward_growth_max_len: scaled_prompt_budget(system_budget, 1, 5, 64, 512),
        },
        MemorySystemKind::LinuxFull => PromptContextNormalizationBudget {
            summary_max_len: policy
                .long_term_recall
                .summary_grounding_max_len
                .min(system_budget),
            constitutional_stack_max_len: scaled_prompt_budget(system_budget, 1, 3, 240, 1024),
            active_task_context_max_len: scaled_prompt_budget(system_budget, 1, 3, 240, 1280),
            governed_memory_evidence_max_len: scaled_prompt_budget(system_budget, 1, 2, 240, 2048),
            background_governance_max_len: scaled_prompt_budget(system_budget, 1, 3, 240, 1536),
            inward_growth_max_len: scaled_prompt_budget(system_budget, 1, 3, 240, 1536),
        },
    }
}

pub(crate) fn decide_self_runtime_authority(
    memory_system_kind: MemorySystemKind,
) -> SelfRuntimeAuthorityPlan {
    match memory_system_kind {
        MemorySystemKind::LinuxFull => SelfRuntimeAuthorityPlan {
            allow_direct_inner_life: true,
            allow_direct_private_docs: true,
            allow_direct_private_garden: true,
            allow_direct_self_model: true,
            allow_direct_self_authored_core: true,
            allow_direct_self_continuity: true,
            allow_direct_boundary_persona: true,
            allow_direct_outer_voice: true,
            allow_factual_refresh_request: true,
            allow_method_distillation: true,
        },
        MemorySystemKind::EspCompact => SelfRuntimeAuthorityPlan {
            allow_direct_inner_life: true,
            allow_direct_private_docs: false,
            allow_direct_private_garden: false,
            allow_direct_self_model: true,
            allow_direct_self_authored_core: false,
            allow_direct_self_continuity: true,
            allow_direct_boundary_persona: false,
            allow_direct_outer_voice: false,
            allow_factual_refresh_request: false,
            allow_method_distillation: true,
        },
    }
}

pub(crate) fn decide_prompt_assembly(
    memory_system_kind: MemorySystemKind,
    ingress: crate::bus::IngressKind,
    has_tools: bool,
    runtime_mode: crate::runtime::RuntimeModeSnapshot,
    pressure: crate::orchestrator::PressureLevel,
    system_budget: usize,
) -> PromptAssemblyPlan {
    let policy = prompt_participation_policy(memory_system_kind);
    let budget_allows_governed = system_budget
        >= memory_policy(memory_system_kind)
            .long_term_recall
            .block_min_len;
    let mode_allows_governed = runtime_mode.allows_prompt_governed_recall(pressure);
    let mode_allows_background = runtime_mode.allows_prompt_background_governance(pressure);
    let mode_allows_private_depth = runtime_mode.allows_prompt_private_depth(pressure);

    match memory_system_kind {
        MemorySystemKind::LinuxFull => PromptAssemblyPlan {
            memory_system_kind,
            participation_plan: match ingress {
                crate::bus::IngressKind::User => PromptParticipationPlan {
                    load_l1_constitutional: true,
                    load_l1_session: true,
                    load_l2_governed_recall: policy.first_user_turn_l2_enabled
                        && budget_allows_governed
                        && mode_allows_governed
                        && !has_tools,
                    load_l2_background_governance: policy.first_user_turn_background_enabled
                        && mode_allows_background,
                    load_l3_private_depth: false,
                },
                _ => PromptParticipationPlan {
                    load_l1_constitutional: true,
                    load_l1_session: true,
                    load_l2_governed_recall: budget_allows_governed && mode_allows_governed,
                    load_l2_background_governance: mode_allows_background,
                    load_l3_private_depth: policy.non_user_turn_private_projection_enabled
                        && mode_allows_private_depth,
                },
            },
            include_capability_package_text: true,
        },
        MemorySystemKind::EspCompact => PromptAssemblyPlan {
            memory_system_kind,
            participation_plan: match ingress {
                crate::bus::IngressKind::User => PromptParticipationPlan {
                    load_l1_constitutional: true,
                    load_l1_session: true,
                    load_l2_governed_recall: false,
                    load_l2_background_governance: false,
                    load_l3_private_depth: false,
                },
                _ => PromptParticipationPlan {
                    load_l1_constitutional: true,
                    load_l1_session: true,
                    load_l2_governed_recall: budget_allows_governed && mode_allows_governed,
                    load_l2_background_governance: mode_allows_background,
                    load_l3_private_depth: false,
                },
            },
            include_capability_package_text: !matches!(ingress, crate::bus::IngressKind::User),
        },
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
    fn memory_system_kind_maps_to_expected_profile() {
        assert_eq!(
            MemorySystemKind::EspCompact.memory_profile(),
            MemoryProfile::Embedded
        );
        assert_eq!(
            MemorySystemKind::LinuxFull.memory_profile(),
            MemoryProfile::Standard
        );
    }

    #[test]
    fn memory_profile_maps_to_expected_memory_system_kind() {
        assert_eq!(
            MemoryProfile::Embedded.memory_system_kind(),
            MemorySystemKind::EspCompact
        );
        assert_eq!(
            MemoryProfile::Standard.memory_system_kind(),
            MemorySystemKind::LinuxFull
        );
    }

    #[test]
    fn esp_compact_self_runtime_authority_is_limited_to_growth_continuity_and_methods() {
        let plan = decide_self_runtime_authority(MemorySystemKind::EspCompact);

        assert!(plan.allow_direct_inner_life);
        assert!(plan.allow_direct_self_model);
        assert!(plan.allow_direct_self_continuity);
        assert!(!plan.allow_direct_self_authored_core);
        assert!(!plan.allow_direct_boundary_persona);
        assert!(!plan.allow_direct_outer_voice);
        assert!(!plan.allow_direct_private_docs);
        assert!(!plan.allow_direct_private_garden);
        assert!(!plan.allow_factual_refresh_request);
        assert!(plan.allow_method_distillation);
    }

    #[test]
    fn linux_full_self_runtime_authority_keeps_full_direct_authority() {
        let plan = decide_self_runtime_authority(MemorySystemKind::LinuxFull);

        assert!(plan.allow_direct_inner_life);
        assert!(plan.allow_direct_private_docs);
        assert!(plan.allow_direct_private_garden);
        assert!(plan.allow_direct_self_model);
        assert!(plan.allow_direct_self_authored_core);
        assert!(plan.allow_direct_self_continuity);
        assert!(plan.allow_direct_boundary_persona);
        assert!(plan.allow_direct_outer_voice);
        assert!(plan.allow_factual_refresh_request);
        assert!(plan.allow_method_distillation);
    }

    #[test]
    fn standard_profile_keeps_larger_memory_windows() {
        let embedded = memory_policy(MemorySystemKind::EspCompact);
        let standard = memory_policy(MemorySystemKind::LinuxFull);
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
        assert!(standard.world_sense.render_max_len > embedded.world_sense.render_max_len);
        assert!(standard.outer_voice.render_max_len > embedded.outer_voice.render_max_len);
        assert!(
            standard.autonomy_strategy.render_max_len > embedded.autonomy_strategy.render_max_len
        );
        assert!(standard.inner_life.render_max_len > embedded.inner_life.render_max_len);
        assert!(standard.self_continuity.render_max_len > embedded.self_continuity.render_max_len);
        assert!(standard.private_docs.render_max_len > embedded.private_docs.render_max_len);
        assert!(standard.private_garden.render_max_len > embedded.private_garden.render_max_len);
        assert!(
            standard.private_garden_governance.existing_docs_max_chars
                > embedded.private_garden_governance.existing_docs_max_chars
        );
        assert!(standard.self_runtime.max_jobs_per_tick > embedded.self_runtime.max_jobs_per_tick);
        assert!(standard.self_state.render_max_len > embedded.self_state.render_max_len);
    }

    #[test]
    fn standard_capability_profile_supports_stronger_memory_sidecar() {
        let embedded = memory_capability_profile(MemorySystemKind::EspCompact);
        let standard = memory_capability_profile(MemorySystemKind::LinuxFull);
        assert!(standard.archive_prompt_max_items > embedded.archive_prompt_max_items);
        assert!(standard.archive_prompt_max_chars > embedded.archive_prompt_max_chars);
        assert!(standard.shared_factual_archive_hits > embedded.shared_factual_archive_hits);
        assert!(standard.slot_query_max_results > embedded.slot_query_max_results);
        assert!(matches!(
            standard.background_hygiene_level,
            MemoryHygieneLevel::Standard
        ));
        assert!(standard.runtime_max_jobs_per_tick > embedded.runtime_max_jobs_per_tick);
    }

    #[test]
    fn embedded_prompt_participation_policy_keeps_private_depth_out_of_first_turn() {
        let embedded = prompt_participation_policy(MemorySystemKind::EspCompact);

        assert!(embedded.first_user_turn_l2_enabled);
        assert!(!embedded.first_user_turn_background_enabled);
        assert!(!embedded.non_user_turn_private_projection_enabled);
        assert!(embedded.tool_round_recall_enabled);
    }

    #[test]
    fn standard_prompt_participation_policy_keeps_wider_sync_participation() {
        let standard = prompt_participation_policy(MemorySystemKind::LinuxFull);

        assert!(standard.first_user_turn_l2_enabled);
        assert!(standard.first_user_turn_background_enabled);
        assert!(standard.non_user_turn_private_projection_enabled);
        assert!(standard.tool_round_recall_enabled);
    }

    #[test]
    fn embedded_prompt_context_normalization_budget_stays_inside_prompt_budget() {
        let budget = prompt_context_normalization_budget(MemorySystemKind::EspCompact, 2048);

        assert_eq!(
            budget.summary_max_len,
            memory_policy(MemorySystemKind::EspCompact)
                .long_term_recall
                .summary_grounding_max_len
        );
        assert!(budget.constitutional_stack_max_len <= 640);
        assert!(budget.active_task_context_max_len <= 640);
        assert!(
            budget.governed_memory_evidence_max_len
                <= memory_policy(MemorySystemKind::EspCompact)
                    .long_term_recall
                    .block_max_len_cap
        );
        assert!(budget.background_governance_max_len <= 512);
        assert!(budget.inward_growth_max_len <= 512);
    }

    #[test]
    fn esp_compact_user_prompt_assembly_uses_compact_carry_without_governed_recall() {
        let plan = decide_prompt_assembly(
            MemorySystemKind::EspCompact,
            crate::bus::IngressKind::User,
            false,
            crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: true,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: true,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            crate::orchestrator::PressureLevel::Normal,
            2048,
        );

        assert_eq!(plan.memory_system_kind, MemorySystemKind::EspCompact);
        assert!(plan.participation_plan.load_l1_constitutional);
        assert!(plan.participation_plan.load_l1_session);
        assert!(!plan.participation_plan.load_l2_governed_recall);
        assert!(!plan.participation_plan.load_l2_background_governance);
        assert!(!plan.participation_plan.load_l3_private_depth);
        assert!(!plan.include_capability_package_text);
    }

    #[test]
    fn linux_full_prompt_assembly_keeps_background_and_capability_package_on_first_user_turn() {
        let plan = decide_prompt_assembly(
            MemorySystemKind::LinuxFull,
            crate::bus::IngressKind::User,
            false,
            crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::Normal,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: true,
                voice_exclusive_active: false,
                background_maintenance_active: false,
                config_plane_alive: true,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: false,
                agent_plane_alive: true,
                external_wss_managed_present: false,
                external_wss_suspend_requested: false,
                external_wss_suspended: false,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: true,
                    allow_due_user_timers: true,
                    allow_heartbeat_injection: true,
                    allow_best_effort_delayed_tasks: true,
                    allow_idle_self_runtime: true,
                    allow_non_voice_outbound: true,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: true,
                    require_external_wss_suspended: false,
                },
            },
            crate::orchestrator::PressureLevel::Normal,
            2048,
        );

        assert_eq!(plan.memory_system_kind, MemorySystemKind::LinuxFull);
        assert!(plan.participation_plan.load_l2_governed_recall);
        assert!(plan.participation_plan.load_l2_background_governance);
        assert!(plan.include_capability_package_text);
    }

    #[test]
    fn voice_exclusive_blocks_nonessential_embedded_participation() {
        let plan = decide_prompt_assembly(
            MemorySystemKind::EspCompact,
            crate::bus::IngressKind::User,
            false,
            crate::runtime::RuntimeModeSnapshot {
                current_mode: crate::runtime::RuntimeMode::VoiceExclusive,
                wifi_sta_connected: true,
                boot_phase_active: false,
                pairing_required: false,
                pairing_state_known: true,
                voice_exclusive_active: true,
                background_maintenance_active: false,
                config_plane_alive: false,
                config_active: false,
                config_activity_phase: crate::runtime::ConfigActivityPhase::Idle,
                channel_plane_alive: true,
                voice_plane_alive: true,
                agent_plane_alive: true,
                external_wss_managed_present: true,
                external_wss_suspend_requested: true,
                external_wss_suspended: true,
                recovery_safe_mode_active: false,
                runtime_foreground: crate::runtime::RuntimeForegroundOverlay::default(),
                action_budget: crate::runtime::RuntimeModeActionBudget {
                    allow_periodic_maintenance: false,
                    allow_due_user_timers: false,
                    allow_heartbeat_injection: false,
                    allow_best_effort_delayed_tasks: false,
                    allow_idle_self_runtime: false,
                    allow_non_voice_outbound: false,
                    allow_realtime_voice_connect: true,
                    allow_external_wss_connect: false,
                    require_external_wss_suspended: true,
                },
            },
            crate::orchestrator::PressureLevel::Normal,
            2048,
        )
        .participation_plan;

        assert!(plan.load_l1_constitutional);
        assert!(plan.load_l1_session);
        assert!(!plan.load_l2_governed_recall);
        assert!(!plan.load_l2_background_governance);
        assert!(!plan.load_l3_private_depth);
    }
}
