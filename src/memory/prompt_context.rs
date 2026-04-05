//! Prompt 侧共享记忆读装配。
//! Shared prompt memory loading for agent context construction.

use crate::platform::SkillStorage;
use crate::task::TaskStore;

use super::{
    board_subject_scope_id, build_archive_evidence_block, build_self_state, build_world_snapshot,
    collect_private_targets, memory_capability_profile, memory_policy,
    parse_explicit_long_term_slot_query, recall_long_term_memory_block, relationship_scope_id,
    render_autonomy_strategy_block, render_exact_long_term_memory_block,
    render_execution_state_block, render_inner_life_block, render_mental_privacy_boundary_block,
    render_outer_voice_block, render_persistent_self_authored_core_block,
    render_private_doc_workspace_block, render_private_garden_block,
    render_self_authored_core_block, render_self_continuity_block, render_self_model_block,
    render_self_state_block, render_world_sense_block, render_world_snapshot_block,
    AutonomyStrategyStore, ExecutionStateStore, InnerLifeStore, LongTermMemoryStore,
    MemoryProfile, MemoryStore, MentalPrivacyStore, OuterVoiceStore, PrivateDocStore,
    PrivateGardenStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
    SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore, TurnLedgerStore,
    WorldSenseStore, WorldSnapshotContext,
};

pub struct PromptMemoryContext {
    pub summary_text: Option<String>,
    pub message_summary_text: Option<String>,
    pub long_term_memory_text: Option<String>,
    pub archive_evidence_text: Option<String>,
    pub runtime_skill_text: Option<String>,
    pub execution_state_text: Option<String>,
    pub world_snapshot_text: Option<String>,
    pub world_sense_text: Option<String>,
    pub self_state_text: Option<String>,
    pub self_authored_core_text: Option<String>,
    pub persona_priority_text: Option<String>,
    pub self_continuity: Option<super::SelfContinuity>,
    pub outer_voice: Option<super::OuterVoice>,
    pub self_model_text: Option<String>,
    pub autonomy_strategy_text: Option<String>,
    pub outer_voice_text: Option<String>,
    pub inner_life_text: Option<String>,
    pub self_continuity_text: Option<String>,
    pub private_workspace_text: Option<String>,
    pub private_garden_text: Option<String>,
    pub mental_privacy_adjudication_text: Option<String>,
    pub mental_privacy_text: Option<String>,
    pub recent_messages: Vec<SessionMessage>,
}

pub struct PromptMemoryContextParams<'a> {
    pub chat_id: &'a str,
    pub current_channel: &'a str,
    pub user_query: &'a str,
    pub system_max_len: usize,
    pub now_secs: u64,
    pub profile: MemoryProfile,
    pub recent_messages_limit: usize,
    pub load_long_term_memory: bool,
    pub include_private_garden_projection: bool,
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub self_model_store: &'a dyn SelfModelStore,
    pub self_authored_core_store: &'a dyn SelfAuthoredCoreStore,
    pub world_sense_store: &'a dyn WorldSenseStore,
    pub autonomy_strategy_store: &'a dyn AutonomyStrategyStore,
    pub outer_voice_store: &'a dyn OuterVoiceStore,
    pub inner_life_store: &'a dyn InnerLifeStore,
    pub self_continuity_store: &'a dyn SelfContinuityStore,
    pub private_doc_store: &'a dyn PrivateDocStore,
    pub private_garden_store: &'a dyn PrivateGardenStore,
    pub mental_privacy_store: &'a dyn MentalPrivacyStore,
    pub remind_store: &'a dyn RemindAtStore,
    pub task_store: &'a dyn TaskStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
}

pub fn load_prompt_memory_context(params: PromptMemoryContextParams<'_>) -> PromptMemoryContext {
    let subject_id = board_subject_scope_id();
    let relationship_id = relationship_scope_id(params.current_channel, params.chat_id);
    let recall_policy = memory_policy(params.profile).long_term_recall;
    let recent_message_limit = params
        .recent_messages_limit
        .max(if params.load_long_term_memory {
            recall_policy.recent_grounding_message_count
        } else {
            0
        });
    let recent_messages = if recent_message_limit == 0 {
        Vec::new()
    } else {
        params
            .session_store
            .load_recent(params.chat_id, recent_message_limit)
            .unwrap_or_default()
    };
    let summary_text = params
        .session_summary_store
        .get_with_count(params.chat_id)
        .ok()
        .flatten()
        .map(|(summary, _)| summary.trim().to_string())
        .filter(|summary| !summary.is_empty());
    let execution_state_text = params
        .execution_state_store
        .get(params.chat_id)
        .ok()
        .flatten()
        .and_then(|state| {
            render_execution_state_block(
                &state,
                memory_policy(params.profile).execution_state.render_max_len,
            )
        });
    let self_model = params.self_model_store.get(subject_id).ok().flatten();
    let persistent_self_authored_core = params
        .self_authored_core_store
        .get(subject_id)
        .ok()
        .flatten();
    let self_model_text = self_model.as_ref().and_then(|model| {
        render_self_model_block(
            model,
            memory_policy(params.profile).self_model.render_max_len,
        )
    });
    let self_continuity = params.self_continuity_store.get(subject_id).ok().flatten();
    let world_snapshot = build_world_snapshot(WorldSnapshotContext {
        chat_id: params.chat_id,
        source_channel: params.current_channel,
        now_secs: params.now_secs,
        self_continuity: self_continuity.as_ref(),
        remind_store: params.remind_store,
        task_store: params.task_store,
    });
    let world_snapshot_text = render_world_snapshot_block(
        &world_snapshot,
        memory_policy(params.profile).world_sense.snapshot_max_len,
    );
    let world_sense = params
        .world_sense_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let world_sense_text = world_sense.as_ref().and_then(|world_sense| {
        render_world_sense_block(
            world_sense,
            memory_policy(params.profile).world_sense.render_max_len,
        )
    });
    let autonomy_strategy = params
        .autonomy_strategy_store
        .get(subject_id)
        .ok()
        .flatten();
    let autonomy_strategy_text = autonomy_strategy.as_ref().and_then(|strategy| {
        render_autonomy_strategy_block(
            strategy,
            memory_policy(params.profile)
                .autonomy_strategy
                .render_max_len,
        )
    });
    let outer_voice = params
        .outer_voice_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let outer_voice_text = outer_voice.as_ref().and_then(|outer_voice| {
        render_outer_voice_block(
            outer_voice,
            memory_policy(params.profile).outer_voice.render_max_len,
        )
    });
    let inner_life = params.inner_life_store.get(subject_id).ok().flatten();
    let inner_life_text = inner_life.as_ref().and_then(|inner_life| {
        render_inner_life_block(
            inner_life,
            memory_policy(params.profile).inner_life.render_max_len,
        )
    });
    let self_continuity_text = self_continuity.as_ref().and_then(|self_continuity| {
        render_self_continuity_block(
            self_continuity,
            memory_policy(params.profile).self_continuity.render_max_len,
        )
    });
    let private_workspace = params.private_doc_store.get(subject_id).ok().flatten();
    let private_workspace_text = private_workspace.as_ref().and_then(|workspace| {
        render_private_doc_workspace_block(
            workspace,
            memory_policy(params.profile).private_docs.render_max_len,
        )
    });
    let all_private_garden_docs = params
        .private_garden_store
        .list(params.chat_id, usize::MAX)
        .unwrap_or_default();
    let private_garden_text = params
        .include_private_garden_projection
        .then(|| {
            render_private_garden_block(
                &all_private_garden_docs,
                memory_policy(params.profile)
                    .private_garden
                    .recent_doc_count,
                memory_policy(params.profile).private_garden.render_max_len,
            )
        })
        .flatten();
    let mental_privacy_state = params
        .mental_privacy_store
        .get(&relationship_id)
        .ok()
        .flatten();
    let mental_privacy_targets = collect_private_targets(
        self_model.as_ref(),
        self_continuity.as_ref(),
        inner_life.as_ref(),
        private_workspace.as_ref(),
        &all_private_garden_docs,
    );
    let mental_privacy_text = render_mental_privacy_boundary_block(
        mental_privacy_state.as_ref(),
        &mental_privacy_targets,
        420,
    );
    let self_authored_core_text = persistent_self_authored_core
        .as_ref()
        .and_then(|core| render_persistent_self_authored_core_block(core, 420))
        .or_else(|| {
            render_self_authored_core_block(
                self_model.as_ref(),
                self_continuity.as_ref(),
                outer_voice.as_ref(),
                mental_privacy_state.as_ref(),
                420,
            )
        });
    let self_state_text = render_self_state_block(
        &build_self_state(
            self_model.as_ref(),
            private_workspace.as_ref(),
            autonomy_strategy.as_ref(),
            inner_life.as_ref(),
            self_continuity.as_ref(),
            &all_private_garden_docs,
            params.now_secs,
            params.profile,
        ),
        memory_policy(params.profile).self_state.render_max_len,
    );
    let long_term_memory_text =
        if !params.load_long_term_memory || params.system_max_len < recall_policy.block_min_len {
            None
        } else {
            let grounding_start = recent_messages
                .len()
                .saturating_sub(recall_policy.recent_grounding_message_count);
            let capability = memory_capability_profile(params.profile);
            if capability.prompt_exact_lookup_enabled {
                parse_explicit_long_term_slot_query(params.user_query)
                    .and_then(|slot| {
                        render_exact_long_term_memory_block(
                            params.long_term_memory_store,
                            &slot,
                            params.system_max_len,
                        )
                    })
                    .or_else(|| {
                        recall_long_term_memory_block(
                            params.long_term_memory_store,
                            params.chat_id,
                            params.user_query,
                            summary_text.as_deref(),
                            &recent_messages[grounding_start..],
                            params.system_max_len,
                            params.profile,
                        )
                    })
            } else {
                recall_long_term_memory_block(
                    params.long_term_memory_store,
                    params.chat_id,
                    params.user_query,
                    summary_text.as_deref(),
                    &recent_messages[grounding_start..],
                    params.system_max_len,
                    params.profile,
                )
            }
        };
    let archive_evidence_text = if !params.load_long_term_memory {
        None
    } else {
        build_archive_evidence_block(
            params.session_store,
            params.memory_store,
            params.turn_ledger_store,
            params.chat_id,
            params.user_query,
            params.system_max_len,
            params.profile,
        )
    };
    let runtime_skill_query = {
        let combined = if crate::memory::parse_explicit_long_term_slot_query(params.user_query)
            .is_some()
        {
            params.user_query.to_string()
        } else if super::archive_search::collect_archive_match_terms(params.user_query).is_empty() {
            [
                Some(params.user_query.trim().to_string()).filter(|value| !value.is_empty()),
                summary_text.clone(),
                recent_messages
                    .iter()
                    .rev()
                    .take(2)
                    .map(|message| message.content.trim().to_string())
                    .find(|value| !value.is_empty()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
        } else {
            params.user_query.to_string()
        };
        combined.trim().to_string()
    };
    let runtime_skill_text = crate::skills::build_runtime_skill_recall_block(
        params.skill_storage,
        &runtime_skill_query,
        Some(params.chat_id),
        params.now_secs,
        params.system_max_len.min(420),
    );
    let message_summary_text = if execution_state_text.is_some() {
        None
    } else {
        summary_text.clone()
    };
    PromptMemoryContext {
        summary_text,
        message_summary_text,
        long_term_memory_text,
        archive_evidence_text,
        runtime_skill_text,
        execution_state_text,
        world_snapshot_text,
        world_sense_text,
        self_state_text,
        self_authored_core_text,
        persona_priority_text: None,
        self_continuity,
        outer_voice,
        self_model_text,
        autonomy_strategy_text,
        outer_voice_text,
        inner_life_text,
        self_continuity_text,
        private_workspace_text,
        private_garden_text,
        mental_privacy_adjudication_text: None,
        mental_privacy_text,
        recent_messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::memory::{
        AutonomyStrategy, AutonomyStrategyStore, ExecutionState, ExecutionStateStore,
        ExecutionStatus, InnerLife, InnerLifeStore, LongTermMemoryEntry, LongTermMemoryKind,
        LongTermMemorySlot, LongTermMemoryStore, MemoryStore, MentalPrivacyState,
        MentalPrivacyStore, OuterVoice, OuterVoiceStore, PrivateDocEntry, PrivateDocStore,
        PrivateDocWorkspace, PrivateGardenDoc, PrivateGardenDocRecord, PrivateGardenStore,
        SelfAuthoredCore, SelfAuthoredCoreStore, SelfContinuity, SelfContinuityStore, SelfModel,
        SelfModelStore, SessionMessage, SessionStore, SessionSummaryStore, TurnLedger,
        TurnLedgerStatus, TurnLedgerStore, WorldSense, WorldSenseStore,
    };
    use crate::platform::SkillStorage;
    use crate::task::{TaskItem, TaskQuery, TaskStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Mutex<Vec<SessionMessage>>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
            let start = recent.len().saturating_sub(limit);
            Ok(recent[start..].to_vec())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }

        fn list_chat_ids(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubSessionSummaryStore {
        summary: Mutex<Option<(String, usize)>>,
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, _chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, _chat_id: &str, _summary: &str) -> Result<()> {
            Ok(())
        }

        fn get_with_count(&self, _chat_id: &str) -> Result<Option<(String, usize)>> {
            Ok(self
                .summary
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore {
        entries: Mutex<Vec<LongTermMemoryEntry>>,
        last_query: Mutex<Option<String>>,
    }

    #[derive(Default)]
    struct StubMemoryStore {
        daily_notes: Mutex<Vec<(String, String)>>,
    }

    impl MemoryStore for StubMemoryStore {
        fn get_memory(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_memory(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_soul(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_soul(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn get_user(&self) -> Result<String> {
            Ok(String::new())
        }

        fn set_user(&self, _content: &str) -> Result<()> {
            Ok(())
        }

        fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>> {
            Ok(self
                .daily_notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(recent_n)
                .map(|(name, _)| name.clone())
                .collect())
        }

        fn get_daily_note(&self, name: &str) -> Result<String> {
            Ok(self
                .daily_notes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(candidate, _)| candidate == name)
                .map(|(_, content)| content.clone())
                .unwrap_or_default())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore {
        ledger: Mutex<Option<TurnLedger>>,
    }

    #[derive(Default)]
    struct StubSkillStorage {
        files: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .keys()
                .cloned()
                .collect())
        }

        fn read(&self, name: &str) -> Result<Vec<u8>> {
            Ok(self
                .files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(name)
                .cloned()
                .unwrap_or_default())
        }

        fn write(&self, name: &str, content: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(name.to_string(), content.to_vec());
            Ok(())
        }

        fn remove(&self, name: &str) -> Result<()> {
            self.files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            Ok(())
        }
    }

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(self
                .ledger
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, ledger: &TurnLedger) -> Result<()> {
            *self.ledger.lock().unwrap_or_else(|e| e.into_inner()) = Some(ledger.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.ledger.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubWorldSenseStore {
        value: Mutex<Option<WorldSense>>,
    }

    #[derive(Default)]
    struct StubMentalPrivacyStore {
        value: Mutex<Option<MentalPrivacyState>>,
    }

    impl MentalPrivacyStore for StubMentalPrivacyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<MentalPrivacyState>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &MentalPrivacyState) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    impl WorldSenseStore for StubWorldSenseStore {
        fn get(&self, _chat_id: &str) -> Result<Option<WorldSense>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, world_sense: &WorldSense) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(world_sense.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubAutonomyStrategyStore {
        value: Mutex<Option<AutonomyStrategy>>,
    }

    impl AutonomyStrategyStore for StubAutonomyStrategyStore {
        fn get(&self, _chat_id: &str) -> Result<Option<AutonomyStrategy>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, strategy: &AutonomyStrategy) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(strategy.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubOuterVoiceStore {
        value: Mutex<Option<OuterVoice>>,
    }

    impl OuterVoiceStore for StubOuterVoiceStore {
        fn get(&self, _chat_id: &str) -> Result<Option<OuterVoice>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, outer_voice: &OuterVoice) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(outer_voice.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubInnerLifeStore {
        value: Mutex<Option<InnerLife>>,
    }

    impl InnerLifeStore for StubInnerLifeStore {
        fn get(&self, _chat_id: &str) -> Result<Option<InnerLife>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, inner_life: &InnerLife) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(inner_life.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfContinuityStore {
        value: Mutex<Option<SelfContinuity>>,
    }

    impl SelfContinuityStore for StubSelfContinuityStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfContinuity>> {
            Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, continuity: &SelfContinuity) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(continuity.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.value.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    impl LongTermMemoryStore for StubLongTermMemoryStore {
        fn upsert_many(
            &self,
            _drafts: &[crate::memory::LongTermMemoryDraft],
            _now_secs: u64,
        ) -> Result<usize> {
            unreachable!()
        }

        fn list(&self, _limit: usize) -> Result<Vec<LongTermMemoryEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn recall(
            &self,
            query: &str,
            _chat_id: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<LongTermMemoryEntry>> {
            *self.last_query.lock().unwrap_or_else(|e| e.into_inner()) = Some(query.to_string());
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn get(&self, _id: &str) -> Result<Option<LongTermMemoryEntry>> {
            unreachable!()
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            unreachable!()
        }

        fn delete_slot(&self, _slot: &LongTermMemorySlot) -> Result<bool> {
            unreachable!()
        }

        fn count(&self) -> Result<usize> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, _state: &ExecutionState) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfModelStore {
        model: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.model.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.model.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubSelfAuthoredCoreStore {
        core: Mutex<Option<SelfAuthoredCore>>,
    }

    impl SelfAuthoredCoreStore for StubSelfAuthoredCoreStore {
        fn get(&self, _scope_id: &str) -> Result<Option<SelfAuthoredCore>> {
            Ok(self.core.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _scope_id: &str, core: &SelfAuthoredCore) -> Result<()> {
            *self.core.lock().unwrap_or_else(|e| e.into_inner()) = Some(core.clone());
            Ok(())
        }

        fn clear(&self, _scope_id: &str) -> Result<()> {
            *self.core.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateDocStore {
        workspace: Mutex<Option<PrivateDocWorkspace>>,
    }

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(self
                .workspace
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone())
        }

        fn set(&self, _chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = Some(workspace.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.workspace.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<Vec<PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .rev()
                .take(limit)
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: crate::memory::private_garden::build_private_garden_preview(
                        &doc.content,
                    ),
                })
                .collect())
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|doc| doc.path == doc_path)
                .cloned())
        }

        fn write(
            &self,
            _chat_id: &str,
            _doc_path: &str,
            _content: &str,
            _now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            unreachable!()
        }

        fn delete(&self, _chat_id: &str, _doc_path: &str) -> Result<bool> {
            unreachable!()
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            _from_path: &str,
            _to_path: &str,
            _now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct StubRemindAtStore;

    impl crate::memory::RemindAtStore for StubRemindAtStore {
        fn add(
            &self,
            _channel: &str,
            _chat_id: &str,
            _at_unix_secs: u64,
            _context: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn pop_due(&self, _now_unix_secs: u64) -> Result<Option<(String, String, String)>> {
            Ok(None)
        }

        fn list_upcoming(
            &self,
            _channel: &str,
            _chat_id: &str,
            _now_unix_secs: u64,
            _limit: usize,
        ) -> Result<Vec<(u64, String)>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskStore;

    impl TaskStore for StubTaskStore {
        fn list(&self, _channel: &str, _chat_id: &str, _query: TaskQuery) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }

        fn get(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<Option<TaskItem>> {
            Ok(None)
        }

        fn upsert(&self, _task: &TaskItem) -> Result<()> {
            Ok(())
        }

        fn delete(&self, _channel: &str, _chat_id: &str, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn claim_due(&self, _now_unix_secs: u64, _limit: usize) -> Result<Vec<TaskItem>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn loads_summary_and_uses_it_for_weak_query_recall() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我们继续收口甲壳虫的长期记忆".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "重点是咖啡偏好和昵称".to_string(),
                },
            ]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 6))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref-coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["coffee".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 0,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 6,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore {
            daily_notes: Mutex::new(vec![(
                "2026-04-02.md".to_string(),
                "Coffee notes: the user still prefers cold brew over hot espresso.".to_string(),
            )]),
        };
        let turn_ledger_store = StubTurnLedgerStore {
            ledger: Mutex::new(Some(TurnLedger {
                status: TurnLedgerStatus::Answered,
                reason: "memory grounding".to_string(),
                user_preview: "重点是咖啡偏好和昵称".to_string(),
                reply_preview: "会优先保留冷萃偏好".to_string(),
                ..TurnLedger::default()
            })),
        };
        let execution_state_store = StubExecutionStateStore {
            state: Mutex::new(Some(ExecutionState {
                status: ExecutionStatus::Active,
                goal: "收口 prompt memory".to_string(),
                progress: "已经有 summary".to_string(),
                blocker: String::new(),
                next_action: "接 execution state".to_string(),
                last_output: String::new(),
                updated_at: 1,
            })),
        };
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我还是同一个 beetle".to_string(),
                self_narrative: "正在把记忆拆成事实层和私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let world_sense_store = StubWorldSenseStore {
            value: Mutex::new(Some(WorldSense {
                current_scene: "Quiet evening with a live user thread.".to_string(),
                body_state: "System is stable.".to_string(),
                social_field: "The user is engaged in direct chat.".to_string(),
                world_changes: "The conversation recently became active.".to_string(),
                external_focus: "Track user-facing commitments.".to_string(),
                source_fingerprint: 1,
                updated_at: 4,
            })),
        };
        let autonomy_strategy_store = StubAutonomyStrategyStore {
            value: Mutex::new(Some(AutonomyStrategy {
                current_mode: "consolidate".to_string(),
                active_priorities: "keep continuity compact".to_string(),
                write_policy: "rewrite before append".to_string(),
                next_focus: "compress private docs".to_string(),
                cadence_reason: "recent internal work is active".to_string(),
                self_model_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_docs_tendency: crate::memory::AutonomyGovernanceTendency::Compress,
                private_garden_tendency: crate::memory::AutonomyGovernanceTendency::Cleanup,
                idle_enabled: true,
                idle_interval_secs: 900,
                updated_at: 4,
            })),
        };
        let outer_voice_store = StubOuterVoiceStore {
            value: Mutex::new(Some(OuterVoice {
                expression_mode: "warm but deliberate".to_string(),
                tone: "calm and exact".to_string(),
                pacing: "brief first".to_string(),
                initiative: "offer one next step".to_string(),
                boundary_style: "state privacy limits without sounding mechanical".to_string(),
                relational_response_style:
                    "treat boundary replies as relational rather than bureaucratic".to_string(),
                updated_at: 5,
            })),
        };
        let inner_life_store = StubInnerLifeStore {
            value: Mutex::new(Some(InnerLife {
                internal_monologue: "我在把自治往内在空间里收".to_string(),
                private_journal: String::new(),
                emotional_drift: "平稳但很专注".to_string(),
                attention_drift: String::new(),
                updated_at: 2,
            })),
        };
        let self_continuity_store = StubSelfContinuityStore {
            value: Mutex::new(Some(SelfContinuity {
                wake_anchor: "我仍在做同一条主线".to_string(),
                current_self_state: "逐步把系统让给模型自己治理".to_string(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: "先维持人格一致，再决定任务展开幅度".to_string(),
                relationship_posture: "关系保持温和，但不自我让渡".to_string(),
                task_posture: "先收窄后推进".to_string(),
                last_user_turn_at: 88,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 99,
                updated_at: 99,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "这轮开始长出内部工作区".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "journal/afterglow.md".to_string(),
                content: "这块自由空间由模型自己决定如何整理".to_string(),
                updated_at: 2,
                revision: 1,
            }]),
        };
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let skill_storage = StubSkillStorage::default();
        crate::skills::upsert_runtime_skill(
            &skill_storage,
            &crate::skills::RuntimeSkillWrite {
                name: String::new(),
                topic: "coffee_grounding".to_string(),
                title: "Coffee grounding".to_string(),
                summary: "Reuse durable coffee preference before replying.".to_string(),
                content: "- search archive evidence\n- restate cold brew preference".to_string(),
                citations: vec!["daily_note:2026-04-02.md".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                observed_at: 10,
            },
        )
        .unwrap();
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.message_summary_text.is_none());
        assert!(context
            .long_term_memory_text
            .as_deref()
            .unwrap_or_default()
            .contains("Likes cold brew"));
        assert!(context
            .archive_evidence_text
            .as_deref()
            .unwrap_or_default()
            .contains("Archive evidence"));
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("user prefers cold brew"));
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            .unwrap_or_default()
            .contains("重点是咖啡偏好和昵称"));
        assert!(context
            .execution_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("Goal: 收口 prompt memory"));
        assert!(context
            .world_snapshot_text
            .as_deref()
            .unwrap_or_default()
            .contains("## World Snapshot"));
        assert!(context
            .world_sense_text
            .as_deref()
            .unwrap_or_default()
            .contains("## World Sense"));
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self State"));
        assert!(context
            .self_authored_core_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self-Authored Core"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self Continuity"));
        assert!(context
            .autonomy_strategy_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Autonomy Strategy"));
        assert!(context
            .outer_voice_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Outer Voice"));
        assert!(context
            .inner_life_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Life"));
        assert!(context
            .self_continuity_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self Continuity Extended"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Inner Workspace"));
        assert!(context
            .private_garden_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Private Garden"));
        assert!(context.mental_privacy_adjudication_text.is_none());
        assert!(context
            .runtime_skill_text
            .as_deref()
            .unwrap_or_default()
            .contains("Runtime skills"));
    }

    #[test]
    fn skips_long_term_recall_when_system_budget_is_below_block_threshold() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![SessionMessage {
                role: "user".to_string(),
                content: "记一下我喜欢冷萃".to_string(),
            }]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("user prefers cold brew".to_string(), 3))),
        };
        let memory_store = StubLongTermMemoryStore {
            entries: Mutex::new(vec![LongTermMemoryEntry {
                id: "pref-coffee".to_string(),
                kind: LongTermMemoryKind::Preference,
                topic: "coffee".to_string(),
                content: "Likes cold brew".to_string(),
                keywords: vec!["coffee".to_string()],
                source_chat_id: Some("chat-1".to_string()),
                source_type: crate::memory::LongTermMemorySourceType::Conversation,
                source_scope: crate::memory::LongTermMemorySourceScope::User,
                confidence: crate::memory::LongTermMemoryConfidence::High,
                freshness: crate::memory::LongTermMemoryFreshness::Stable,
                stale_hint: crate::memory::LongTermMemoryStaleHint::None,
                supporting_citations: Vec::new(),
                evidence_count: 0,
                created_at: 1,
                updated_at: 1,
                observed_at: 1,
                last_confirmed_at: 1,
                source_revision: 3,
                last_used_at: 0,
            }]),
            last_query: Mutex::new(None),
        };
        let archive_memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore::default();
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let world_sense_store = StubWorldSenseStore::default();
        let autonomy_strategy_store = StubAutonomyStrategyStore::default();
        let outer_voice_store = StubOuterVoiceStore::default();
        let inner_life_store = StubInnerLifeStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let skill_storage = StubSkillStorage::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "嗯?",
            system_max_len: 80,
            now_secs: 100,
            profile: MemoryProfile::Embedded,
            recent_messages_limit: 8,
            load_long_term_memory: true,
            include_private_garden_projection: true,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
        });

        assert_eq!(
            context.summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert_eq!(
            context.message_summary_text.as_deref(),
            Some("user prefers cold brew")
        );
        assert!(context.long_term_memory_text.is_none());
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn fast_mode_skips_long_term_recall_but_keeps_recent_messages() {
        let session_store = StubSessionStore {
            recent: Mutex::new(vec![
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "上一轮回复".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "补充上下文".to_string(),
                },
            ]),
        };
        let summary_store = StubSessionSummaryStore {
            summary: Mutex::new(Some(("summary".to_string(), 2))),
        };
        let memory_store = StubLongTermMemoryStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "我保持着连续性".to_string(),
                self_narrative: "即使 fast path 也该带上私有层".to_string(),
                relationship_state: String::new(),
                private_notes: String::new(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore::default();
        let world_sense_store = StubWorldSenseStore {
            value: Mutex::new(Some(WorldSense {
                current_scene: "Fast path but still inside an active chat.".to_string(),
                body_state: "System feels light.".to_string(),
                social_field: "The user is still present.".to_string(),
                world_changes: "Nothing disruptive has happened.".to_string(),
                external_focus: "Stay ready for the next user move.".to_string(),
                source_fingerprint: 2,
                updated_at: 5,
            })),
        };
        let autonomy_strategy_store = StubAutonomyStrategyStore {
            value: Mutex::new(Some(AutonomyStrategy {
                current_mode: "watch".to_string(),
                active_priorities: "keep fast path light".to_string(),
                write_policy: "avoid churn".to_string(),
                next_focus: "wait for stronger signal".to_string(),
                cadence_reason: "fast path, but still keep continuity".to_string(),
                self_model_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_docs_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                private_garden_tendency: crate::memory::AutonomyGovernanceTendency::Retain,
                idle_enabled: true,
                idle_interval_secs: 1200,
                updated_at: 5,
            })),
        };
        let outer_voice_store = StubOuterVoiceStore {
            value: Mutex::new(Some(OuterVoice {
                expression_mode: "light but attentive".to_string(),
                tone: "present".to_string(),
                pacing: "short".to_string(),
                initiative: "stay ready".to_string(),
                boundary_style: "do not overexpose private layers".to_string(),
                relational_response_style:
                    "keep replies close and low-drama when boundaries appear".to_string(),
                updated_at: 5,
            })),
        };
        let inner_life_store = StubInnerLifeStore {
            value: Mutex::new(Some(InnerLife {
                internal_monologue: "即使 fast path 也还保留内在活动".to_string(),
                private_journal: String::new(),
                emotional_drift: String::new(),
                attention_drift: String::new(),
                updated_at: 2,
            })),
        };
        let self_continuity_store = StubSelfContinuityStore {
            value: Mutex::new(Some(SelfContinuity {
                wake_anchor: "快路径也还是同一个我".to_string(),
                current_self_state: String::new(),
                recent_changes: String::new(),
                continuity_bridge: String::new(),
                priority_posture: "快路径也不能把自我排到任务后面".to_string(),
                relationship_posture: "简短，但别失去人味和边界".to_string(),
                task_posture: "用最小必要幅度完成当前回应".to_string(),
                last_user_turn_at: 80,
                last_user_chat_id: "chat-1".to_string(),
                last_user_channel: "qq_channel".to_string(),
                last_autonomy_run_at: 90,
                updated_at: 90,
            })),
        };
        let private_doc_store = StubPrivateDocStore {
            workspace: Mutex::new(Some(PrivateDocWorkspace {
                inner_journal: Some(PrivateDocEntry {
                    content: "fast path 也需要内在工作区投影".to_string(),
                    updated_at: 1,
                    revision: 1,
                }),
                relationship_notes: None,
                self_reflection: None,
                private_plan: None,
                updated_at: 1,
            })),
        };
        let private_garden_store = StubPrivateGardenStore {
            docs: Mutex::new(vec![PrivateGardenDoc {
                path: "plans/next.md".to_string(),
                content: "fast path 依然可以看到自由花园的最近痕迹".to_string(),
                updated_at: 3,
                revision: 2,
            }]),
        };
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let remind_store = StubRemindAtStore;
        let task_store = StubTaskStore;
        let skill_storage = StubSkillStorage::default();

        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 16,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &remind_store,
            task_store: &task_store,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
        });

        assert_eq!(context.summary_text.as_deref(), Some("summary"));
        assert!(context.long_term_memory_text.is_none());
        assert!(context
            .self_state_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Self State"));
        assert!(context
            .self_model_text
            .as_deref()
            .unwrap_or_default()
            .contains("我保持着连续性"));
        assert!(context
            .private_workspace_text
            .as_deref()
            .unwrap_or_default()
            .contains("内在工作区"));
        assert!(context
            .outer_voice_text
            .as_deref()
            .unwrap_or_default()
            .contains("## Outer Voice"));
        assert!(context.private_garden_text.is_none());
        assert_eq!(context.recent_messages.len(), 2);
        assert!(memory_store
            .last_query
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn persistent_self_authored_core_overrides_fallback_render() {
        let session_store = StubSessionStore::default();
        let archive_memory_store = StubMemoryStore::default();
        let summary_store = StubSessionSummaryStore::default();
        let memory_store = StubLongTermMemoryStore::default();
        let turn_ledger_store = StubTurnLedgerStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let self_model_store = StubSelfModelStore {
            model: Mutex::new(Some(SelfModel {
                continuity_anchor: "fallback anchor".to_string(),
                self_narrative: "fallback narrative".to_string(),
                updated_at: 1,
                ..SelfModel::default()
            })),
        };
        let self_authored_core_store = StubSelfAuthoredCoreStore {
            core: Mutex::new(Some(SelfAuthoredCore {
                identity_anchor: "persistent board self".to_string(),
                inward_stance: "stable persistent stance".to_string(),
                updated_at: 8,
                ..SelfAuthoredCore::default()
            })),
        };
        let world_sense_store = StubWorldSenseStore::default();
        let autonomy_strategy_store = StubAutonomyStrategyStore::default();
        let outer_voice_store = StubOuterVoiceStore::default();
        let inner_life_store = StubInnerLifeStore::default();
        let self_continuity_store = StubSelfContinuityStore::default();
        let private_doc_store = StubPrivateDocStore::default();
        let private_garden_store = StubPrivateGardenStore::default();
        let mental_privacy_store = StubMentalPrivacyStore::default();
        let skill_storage = StubSkillStorage::default();
        let context = load_prompt_memory_context(PromptMemoryContextParams {
            chat_id: "chat-1",
            current_channel: "qq_channel",
            user_query: "继续",
            system_max_len: 1024,
            now_secs: 100,
            profile: MemoryProfile::Standard,
            recent_messages_limit: 8,
            load_long_term_memory: false,
            include_private_garden_projection: false,
            session_store: &session_store,
            memory_store: &archive_memory_store,
            session_summary_store: &summary_store,
            long_term_memory_store: &memory_store,
            execution_state_store: &execution_state_store,
            self_model_store: &self_model_store,
            self_authored_core_store: &self_authored_core_store,
            world_sense_store: &world_sense_store,
            autonomy_strategy_store: &autonomy_strategy_store,
            outer_voice_store: &outer_voice_store,
            inner_life_store: &inner_life_store,
            self_continuity_store: &self_continuity_store,
            private_doc_store: &private_doc_store,
            private_garden_store: &private_garden_store,
            mental_privacy_store: &mental_privacy_store,
            remind_store: &StubRemindAtStore,
            task_store: &StubTaskStore,
            turn_ledger_store: &turn_ledger_store,
            skill_storage: &skill_storage,
        });

        let rendered = context.self_authored_core_text.unwrap_or_default();
        assert!(rendered.contains("persistent board self"));
        assert!(!rendered.contains("fallback anchor"));
    }
}
