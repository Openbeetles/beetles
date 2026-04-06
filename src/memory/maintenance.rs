//! 对话回复后的共享记忆维护编排。
//! Shared post-reply memory maintenance orchestration.

use crate::bus::IngressKind;
use crate::error::Result;
use crate::llm::{LlmClient, LlmHttpClient};
use crate::orchestrator::PressureLevel;
use crate::platform::SkillStorage;
use crate::task_execution::{
    run_task_learning_maintenance, TaskArtifactStore, TaskLearningMaintenanceContext,
    TaskLearningMaintenanceInput, TaskLearningMaintenanceOutcome, TaskLearningStore, TaskRunStore,
};

use super::{
    evaluate_long_term_memory_extraction_turn, load_session_summary_snapshot,
    mark_long_term_memory_extraction_requested, memory_capability_profile, memory_policy,
    persist_long_term_memory_extraction_state, run_execution_state_refresh_with_state,
    run_memory_governance_kernel, run_memory_hygiene_jobs,
    run_session_summary_refresh_with_snapshot, should_refresh_execution_state,
    ExecutionStateRefreshContext, ExecutionStateRefreshInput, ExecutionStateRefreshOutcome,
    ExecutionStateStore, LongTermMemoryExtractionStateStore, LongTermMemoryExtractionTurnInput,
    LongTermMemoryStore, MemoryGovernanceContext, MemoryGovernanceInput, MemoryHygieneContext,
    MemoryProfile, MemoryStore, SessionStore, SessionSummaryRefreshOutcome, SessionSummaryStore,
    TurnLedgerStore,
};

pub struct PostReplyMemoryMaintenanceContext<'a> {
    pub session_store: &'a dyn SessionStore,
    pub memory_store: &'a dyn MemoryStore,
    pub session_summary_store: &'a dyn SessionSummaryStore,
    pub execution_state_store: &'a dyn ExecutionStateStore,
    pub long_term_memory_store: &'a dyn LongTermMemoryStore,
    pub extraction_state_store: &'a dyn LongTermMemoryExtractionStateStore,
    pub turn_ledger_store: &'a dyn TurnLedgerStore,
    pub skill_storage: &'a dyn SkillStorage,
    pub task_run_store: &'a dyn TaskRunStore,
    pub task_artifact_store: &'a dyn TaskArtifactStore,
    pub task_learning_store: &'a dyn TaskLearningStore,
}

pub struct PostReplyMemoryMaintenanceInput<'a> {
    pub chat_id: &'a str,
    pub ingress: IngressKind,
    pub channel: &'a str,
    pub user_content: &'a str,
    pub reply_content: &'a str,
    pub pressure: PressureLevel,
    pub memory_profile: MemoryProfile,
    pub tool_calls: u32,
    pub external_content_used: bool,
    pub now_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LongTermMemoryRefreshRequestOutcome {
    NotRequested,
    Requested,
    RequestFailed,
}

pub struct PostReplyMemoryMaintenanceOutcome {
    pub after_count: usize,
    pub summary_result: Result<SessionSummaryRefreshOutcome>,
    pub execution_state_result: Result<ExecutionStateRefreshOutcome>,
    pub factual_coordination_summary: Option<String>,
    pub factual_refresh_suggested: bool,
    pub extraction_request_outcome: LongTermMemoryRefreshRequestOutcome,
    pub hygiene_outcome: super::MemoryHygieneOutcome,
    pub task_learning_outcome: Result<TaskLearningMaintenanceOutcome>,
}

struct MaintenanceBaseline {
    after_count: usize,
    initial_summary_snapshot: super::session_summary_refresh::SessionSummarySnapshot,
    execution_state: Result<Option<crate::memory::ExecutionState>>,
    summary_should_refresh: bool,
    execution_should_refresh: bool,
}

struct MaintenanceRecentWindows {
    shared_recent: Option<Vec<crate::memory::SessionMessage>>,
}

struct SharedMaintenancePasses {
    summary_result: Result<SessionSummaryRefreshOutcome>,
    summary_snapshot: super::session_summary_refresh::SessionSummarySnapshot,
    execution_state_result: Result<ExecutionStateRefreshOutcome>,
}

struct PostReplyFollowupPasses {
    governance: super::MemoryGovernanceOutcome,
    extraction_request_outcome: LongTermMemoryRefreshRequestOutcome,
    hygiene_outcome: super::MemoryHygieneOutcome,
    task_learning_outcome: Result<TaskLearningMaintenanceOutcome>,
}

fn collect_maintenance_baseline(
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
) -> MaintenanceBaseline {
    let after_count = ctx.session_store.message_count(input.chat_id).unwrap_or(0);
    let initial_summary_snapshot =
        load_session_summary_snapshot(ctx.session_summary_store, input.chat_id);
    let execution_state = ctx.execution_state_store.get(input.chat_id);
    let summary_should_refresh = super::should_refresh_session_summary(
        after_count,
        initial_summary_snapshot.last_summary_count,
        input.memory_profile,
    );
    let execution_should_refresh = execution_state
        .as_ref()
        .map(|state| {
            should_refresh_execution_state(
                ExecutionStateRefreshInput {
                    chat_id: input.chat_id,
                    ingress: input.ingress,
                    channel: input.channel,
                    user_content: input.user_content,
                    reply_content: input.reply_content,
                    pressure: input.pressure,
                    tool_calls: input.tool_calls,
                    now_secs: input.now_secs,
                },
                state.is_some(),
                input.memory_profile,
            )
        })
        .unwrap_or(false);
    MaintenanceBaseline {
        after_count,
        initial_summary_snapshot,
        execution_state,
        summary_should_refresh,
        execution_should_refresh,
    }
}

fn load_maintenance_recent_windows(
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
    baseline: &MaintenanceBaseline,
) -> MaintenanceRecentWindows {
    let capability = memory_capability_profile(input.memory_profile);
    let policy = memory_policy(input.memory_profile);
    let shared_recent_threshold = match capability.background_hygiene_level {
        crate::memory::MemoryHygieneLevel::Minimal => 2,
        crate::memory::MemoryHygieneLevel::Standard => 1,
    };
    let shared_recent = if [
        baseline.summary_should_refresh,
        baseline.execution_should_refresh,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
        >= shared_recent_threshold
    {
        let summary_policy = policy.session_summary;
        let execution_policy = policy.execution_state;
        ctx.session_store
            .load_recent(
                input.chat_id,
                summary_policy
                    .recent_message_count
                    .max(execution_policy.recent_message_count)
                    .max(policy.long_term_recall.recent_grounding_message_count),
            )
            .ok()
    } else {
        None
    };
    MaintenanceRecentWindows { shared_recent }
}

fn run_shared_maintenance_passes(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
    baseline: &MaintenanceBaseline,
    recent: &MaintenanceRecentWindows,
) -> SharedMaintenancePasses {
    crate::platform::task_wdt::feed_current_task();
    let (summary_result, summary_snapshot) = match run_session_summary_refresh_with_snapshot(
        http,
        llm,
        super::SessionSummaryRefreshContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
        },
        input.chat_id,
        baseline.after_count,
        input.memory_profile,
        baseline.initial_summary_snapshot.clone(),
        recent.shared_recent.as_deref(),
    ) {
        Ok((outcome, snapshot)) => (Ok(outcome), snapshot),
        Err(error) => (Err(error), baseline.initial_summary_snapshot.clone()),
    };
    crate::platform::task_wdt::feed_current_task();
    let execution_state_result = match &baseline.execution_state {
        Ok(existing_state) => run_execution_state_refresh_with_state(
            http,
            llm,
            ExecutionStateRefreshContext {
                session_store: ctx.session_store,
                session_summary_store: ctx.session_summary_store,
                execution_state_store: ctx.execution_state_store,
            },
            ExecutionStateRefreshInput {
                chat_id: input.chat_id,
                ingress: input.ingress,
                channel: input.channel,
                user_content: input.user_content,
                reply_content: input.reply_content,
                pressure: input.pressure,
                tool_calls: input.tool_calls,
                now_secs: input.now_secs,
            },
            input.memory_profile,
            existing_state.clone(),
            summary_snapshot.summary_text.as_deref(),
            recent.shared_recent.as_deref(),
        ),
        Err(error) => Err(crate::error::Error::config(
            error.stage(),
            error.to_string(),
        )),
    };
    crate::platform::task_wdt::feed_current_task();
    SharedMaintenancePasses {
        summary_result,
        summary_snapshot,
        execution_state_result,
    }
}

fn run_post_reply_followup_passes(
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
    baseline: &MaintenanceBaseline,
    recent: &MaintenanceRecentWindows,
    shared: &SharedMaintenancePasses,
    mut enqueue_long_term_refresh: impl FnMut() -> bool,
) -> PostReplyFollowupPasses {
    crate::platform::task_wdt::feed_current_task();
    let governance = run_post_reply_memory_governance(ctx, input, recent, shared);
    crate::platform::task_wdt::feed_current_task();
    let extraction_request_outcome = evaluate_post_reply_extraction_request(
        ctx,
        input,
        baseline.after_count,
        governance.factual_refresh_suggested,
        &mut enqueue_long_term_refresh,
    );
    crate::platform::task_wdt::feed_current_task();
    let hygiene_outcome = run_memory_hygiene_jobs(
        MemoryHygieneContext {
            session_store: ctx.session_store,
            session_summary_store: ctx.session_summary_store,
            memory_store: ctx.memory_store,
            turn_ledger_store: ctx.turn_ledger_store,
            long_term_memory_store: ctx.long_term_memory_store,
            skill_storage: ctx.skill_storage,
        },
        input.chat_id,
        input.memory_profile,
        input.now_secs,
    );
    crate::platform::task_wdt::feed_current_task();
    let task_learning_outcome = run_task_learning_maintenance(
        TaskLearningMaintenanceContext {
            task_run_store: ctx.task_run_store,
            task_artifact_store: ctx.task_artifact_store,
            task_learning_store: ctx.task_learning_store,
            long_term_memory_store: ctx.long_term_memory_store,
            skill_storage: ctx.skill_storage,
            memory_store: ctx.memory_store,
        },
        TaskLearningMaintenanceInput {
            channel: input.channel,
            chat_id: input.chat_id,
            now_secs: input.now_secs,
        },
    );
    crate::platform::task_wdt::feed_current_task();
    PostReplyFollowupPasses {
        governance,
        extraction_request_outcome,
        hygiene_outcome,
        task_learning_outcome,
    }
}

fn run_post_reply_memory_governance(
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
    recent: &MaintenanceRecentWindows,
    shared: &SharedMaintenancePasses,
) -> super::MemoryGovernanceOutcome {
    let factual_query = if !input.user_content.trim().is_empty() {
        input.user_content
    } else {
        input.reply_content
    };
    run_memory_governance_kernel(
        MemoryGovernanceContext {
            session_store: ctx.session_store,
            long_term_memory_store: ctx.long_term_memory_store,
            memory_store: ctx.memory_store,
            turn_ledger_store: ctx.turn_ledger_store,
        },
        MemoryGovernanceInput {
            chat_id: input.chat_id,
            query_hint: factual_query,
            summary_text: shared.summary_snapshot.summary_text.as_deref(),
            recent: recent.shared_recent.as_deref().unwrap_or(&[]),
            max_len: memory_policy(input.memory_profile)
                .long_term_recall
                .block_max_len_cap,
            profile: input.memory_profile,
            external_content_used: input.external_content_used,
        },
    )
}

fn evaluate_post_reply_extraction_request(
    ctx: &PostReplyMemoryMaintenanceContext<'_>,
    input: &PostReplyMemoryMaintenanceInput<'_>,
    after_count: usize,
    factual_refresh_suggested: bool,
    enqueue_long_term_refresh: &mut impl FnMut() -> bool,
) -> LongTermMemoryRefreshRequestOutcome {
    let extraction_state = ctx.extraction_state_store.get(input.chat_id).ok().flatten();
    let extraction_decision = evaluate_long_term_memory_extraction_turn(
        LongTermMemoryExtractionTurnInput {
            ingress: input.ingress,
            channel: input.channel,
            user_content: input.user_content,
            reply_content: input.reply_content,
            after_count,
            pressure: input.pressure,
            external_content_used: input.external_content_used,
        },
        extraction_state.as_ref(),
        input.memory_profile,
    );
    let mut next_extraction_state = extraction_decision.next_state.clone();
    let should_request_extraction = extraction_decision.should_enqueue || factual_refresh_suggested;
    let outcome = if should_request_extraction {
        if enqueue_long_term_refresh() {
            next_extraction_state =
                mark_long_term_memory_extraction_requested(&next_extraction_state, after_count);
            LongTermMemoryRefreshRequestOutcome::Requested
        } else {
            LongTermMemoryRefreshRequestOutcome::RequestFailed
        }
    } else {
        LongTermMemoryRefreshRequestOutcome::NotRequested
    };
    persist_long_term_memory_extraction_state(
        ctx.extraction_state_store,
        input.chat_id,
        extraction_state.as_ref(),
        &next_extraction_state,
    );
    outcome
}

pub fn run_post_reply_memory_maintenance(
    http: &mut dyn LlmHttpClient,
    llm: &(dyn LlmClient + Send + Sync),
    ctx: PostReplyMemoryMaintenanceContext<'_>,
    input: PostReplyMemoryMaintenanceInput<'_>,
    mut enqueue_long_term_refresh: impl FnMut() -> bool,
) -> PostReplyMemoryMaintenanceOutcome {
    let baseline = collect_maintenance_baseline(&ctx, &input);
    crate::platform::task_wdt::feed_current_task();
    let recent = load_maintenance_recent_windows(&ctx, &input, &baseline);
    crate::platform::task_wdt::feed_current_task();
    let shared = run_shared_maintenance_passes(http, llm, &ctx, &input, &baseline, &recent);
    crate::platform::task_wdt::feed_current_task();
    let followup = run_post_reply_followup_passes(
        &ctx,
        &input,
        &baseline,
        &recent,
        &shared,
        &mut enqueue_long_term_refresh,
    );
    crate::platform::task_wdt::feed_current_task();

    PostReplyMemoryMaintenanceOutcome {
        after_count: baseline.after_count,
        summary_result: shared.summary_result,
        execution_state_result: shared.execution_state_result,
        factual_coordination_summary: followup.governance.factual_coordination_summary,
        factual_refresh_suggested: followup.governance.factual_refresh_suggested,
        extraction_request_outcome: followup.extraction_request_outcome,
        hygiene_outcome: followup.hygiene_outcome,
        task_learning_outcome: followup.task_learning_outcome,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Result;
    use crate::llm::{LlmModelCompat, LlmResponse, Message, StopReason, ToolChoicePolicy};
    use crate::memory::{
        ExecutionState, ExecutionStateStore, LongTermMemoryExtractionState,
        LongTermMemoryExtractionStateStore, MemoryStore, PrivateDocStore, PrivateDocWorkspace,
        PrivateGardenDoc, PrivateGardenDocRecord, PrivateGardenStore, SelfModel, SelfModelStore,
        SessionMessage, SessionSummaryStore, TurnLedger, TurnLedgerStore,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct StubSessionStore {
        recent: Vec<SessionMessage>,
        count: usize,
        load_recent_calls: Mutex<u32>,
    }

    impl SessionStore for StubSessionStore {
        fn append(&self, _chat_id: &str, _role: &str, _content: &str) -> Result<()> {
            Ok(())
        }

        fn load_recent(&self, _chat_id: &str, limit: usize) -> Result<Vec<SessionMessage>> {
            *self
                .load_recent_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(self.recent.iter().take(limit).cloned().collect())
        }

        fn message_count(&self, _chat_id: &str) -> Result<usize> {
            Ok(self.count)
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
        entries: Mutex<HashMap<String, (String, usize)>>,
        fail_get_with_count: bool,
    }

    #[derive(Default)]
    struct StubMemoryStore;

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

        fn list_daily_note_names(&self, _recent_n: usize) -> Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn get_daily_note(&self, _name: &str) -> Result<String> {
            Ok(String::new())
        }

        fn write_daily_note(&self, _name: &str, _content: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTurnLedgerStore;

    impl TurnLedgerStore for StubTurnLedgerStore {
        fn get(&self, _chat_id: &str) -> Result<Option<TurnLedger>> {
            Ok(None)
        }

        fn set(&self, _chat_id: &str, _ledger: &TurnLedger) -> Result<()> {
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            Ok(())
        }
    }

    impl SessionSummaryStore for StubSessionSummaryStore {
        fn get(&self, chat_id: &str) -> Result<Option<String>> {
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .map(|(summary, _)| summary.clone()))
        }

        fn set(&self, chat_id: &str, summary: &str) -> Result<()> {
            self.set_with_count(chat_id, summary, 0)
        }

        fn set_with_count(&self, chat_id: &str, summary: &str, message_count: usize) -> Result<()> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(chat_id.to_string(), (summary.to_string(), message_count));
            Ok(())
        }

        fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
            if self.fail_get_with_count {
                return Err(crate::error::Error::config("summary", "broken"));
            }
            Ok(self
                .entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(chat_id)
                .cloned())
        }
    }

    #[derive(Default)]
    struct StubExtractionStateStore {
        state: Mutex<Option<LongTermMemoryExtractionState>>,
        clears: Mutex<u32>,
    }

    impl LongTermMemoryExtractionStateStore for StubExtractionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<LongTermMemoryExtractionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &LongTermMemoryExtractionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubExecutionStateStore {
        state: Mutex<Option<ExecutionState>>,
        clears: Mutex<u32>,
    }

    impl ExecutionStateStore for StubExecutionStateStore {
        fn get(&self, _chat_id: &str) -> Result<Option<ExecutionState>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, state: &ExecutionState) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self.clears.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(())
        }
    }

    #[derive(Default)]
    #[allow(dead_code)]
    struct StubSelfModelStore {
        state: Mutex<Option<SelfModel>>,
    }

    impl SelfModelStore for StubSelfModelStore {
        fn get(&self, _chat_id: &str) -> Result<Option<SelfModel>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, model: &SelfModel) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(model.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    #[allow(dead_code)]
    struct StubPrivateDocStore {
        state: Mutex<Option<PrivateDocWorkspace>>,
    }

    impl PrivateDocStore for StubPrivateDocStore {
        fn get(&self, _chat_id: &str) -> Result<Option<PrivateDocWorkspace>> {
            Ok(self.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn set(&self, _chat_id: &str, workspace: &PrivateDocWorkspace) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = Some(workspace.clone());
            Ok(())
        }

        fn clear(&self, _chat_id: &str) -> Result<()> {
            *self.state.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubPrivateGardenStore {
        docs: Mutex<HashMap<String, PrivateGardenDoc>>,
    }

    impl PrivateGardenStore for StubPrivateGardenStore {
        fn list(&self, _chat_id: &str, limit: usize) -> Result<Vec<PrivateGardenDocRecord>> {
            let mut docs = self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .values()
                .map(|doc| PrivateGardenDocRecord {
                    path: doc.path.clone(),
                    updated_at: doc.updated_at,
                    revision: doc.revision,
                    bytes: doc.content.len(),
                    preview: crate::memory::build_private_garden_preview(&doc.content),
                })
                .collect::<Vec<_>>();
            docs.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.path.cmp(&b.path))
            });
            docs.truncate(limit);
            Ok(docs)
        }

        fn read(&self, _chat_id: &str, doc_path: &str) -> Result<Option<PrivateGardenDoc>> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(doc_path)
                .cloned())
        }

        fn write(
            &self,
            _chat_id: &str,
            doc_path: &str,
            content: &str,
            now_secs: u64,
        ) -> Result<PrivateGardenDocRecord> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let revision = docs
                .get(doc_path)
                .map(|doc| doc.revision.saturating_add(1))
                .unwrap_or(1);
            let doc = PrivateGardenDoc {
                path: doc_path.to_string(),
                content: content.to_string(),
                updated_at: now_secs,
                revision,
            };
            docs.insert(doc_path.to_string(), doc.clone());
            Ok(PrivateGardenDocRecord {
                path: doc.path,
                updated_at: doc.updated_at,
                revision: doc.revision,
                bytes: doc.content.len(),
                preview: crate::memory::build_private_garden_preview(&doc.content),
            })
        }

        fn delete(&self, _chat_id: &str, doc_path: &str) -> Result<bool> {
            Ok(self
                .docs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(doc_path)
                .is_some())
        }

        fn move_doc(
            &self,
            _chat_id: &str,
            from_path: &str,
            to_path: &str,
            now_secs: u64,
        ) -> Result<Option<PrivateGardenDocRecord>> {
            let mut docs = self.docs.lock().unwrap_or_else(|e| e.into_inner());
            let Some(doc) = docs.remove(from_path) else {
                return Ok(None);
            };
            let moved = PrivateGardenDoc {
                path: to_path.to_string(),
                content: doc.content,
                updated_at: now_secs,
                revision: doc.revision.saturating_add(1),
            };
            docs.insert(to_path.to_string(), moved.clone());
            Ok(Some(PrivateGardenDocRecord {
                path: moved.path,
                updated_at: moved.updated_at,
                revision: moved.revision,
                bytes: moved.content.len(),
                preview: crate::memory::build_private_garden_preview(&moved.content),
            }))
        }
    }

    #[derive(Default)]
    struct StubLongTermMemoryStore;

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
            _limit: usize,
        ) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn get(&self, _id: &str) -> Result<Option<crate::memory::LongTermMemoryEntry>> {
            Ok(None)
        }

        fn list(&self, _limit: usize) -> Result<Vec<crate::memory::LongTermMemoryEntry>> {
            Ok(Vec::new())
        }

        fn delete(&self, _id: &str) -> Result<bool> {
            Ok(false)
        }

        fn delete_slot(&self, _slot: &crate::memory::LongTermMemorySlot) -> Result<bool> {
            Ok(false)
        }

        fn count(&self) -> Result<usize> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct StubSkillStorage;

    impl SkillStorage for StubSkillStorage {
        fn list_names(&self) -> Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn read(&self, _name: &str) -> Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn write(&self, _name: &str, _content: &[u8]) -> Result<()> {
            Ok(())
        }

        fn remove(&self, _name: &str) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct StubTaskRunStore;

    impl crate::task_execution::TaskRunStore for StubTaskRunStore {
        fn get(&self, _run_id: &str) -> Result<Option<crate::task_execution::TaskRunRecord>> {
            Ok(None)
        }

        fn upsert(&self, _record: &crate::task_execution::TaskRunRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(&self, _limit: usize) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(Vec::new())
        }

        fn list_active_for_chat(
            &self,
            _channel: &str,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskRunRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskArtifactStore;

    impl crate::task_execution::TaskArtifactStore for StubTaskArtifactStore {
        fn put(&self, _record: &crate::task_execution::TaskArtifactRecord) -> Result<()> {
            Ok(())
        }

        fn list_for_run(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskArtifactRecord>> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct StubTaskLearningStore;

    impl crate::task_execution::TaskLearningStore for StubTaskLearningStore {
        fn get(
            &self,
            _learning_id: &str,
        ) -> Result<Option<crate::task_execution::TaskLearningRecord>> {
            Ok(None)
        }

        fn upsert(&self, _record: &crate::task_execution::TaskLearningRecord) -> Result<()> {
            Ok(())
        }

        fn list_recent(
            &self,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }

        fn list_for_chat(
            &self,
            _channel: &str,
            _chat_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }

        fn list_for_run(
            &self,
            _run_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::task_execution::TaskLearningRecord>> {
            Ok(Vec::new())
        }
    }

    struct FixedLlmClient;

    impl LlmClient for FixedLlmClient {
        fn model_compat(&self) -> LlmModelCompat {
            LlmModelCompat::default()
        }

        fn chat(
            &self,
            _http: &mut dyn LlmHttpClient,
            system: &str,
            _messages: &[Message],
            _tools: Option<&[crate::llm::ToolSpec]>,
            _tool_choice: ToolChoicePolicy,
        ) -> Result<LlmResponse> {
            let content = if system == crate::memory::EXECUTION_STATE_SYSTEM_PROMPT {
                r#"{"status":"active","goal":"长期记忆链路收口","progress":"继续拆 coordinator","next_action":"接 execution state"}"#
            } else if system == crate::memory::INTERNAL_MEMORY_ROUTING_SYSTEM_PROMPT {
                r#"{"refresh_self_model":true,"self_model_intent":"Stabilize the newly formed self continuity","self_model_sources":["private_docs.inner_journal"],"refresh_private_docs":true,"private_docs_intent":"Move still-valid inward plans into governed docs","private_docs_sources":["private_garden:journal/promoted.md"],"refresh_private_garden":true,"private_garden_intent":"Reorganize drafts and document structure that are still exploratory","private_garden_cleanup_paths":["journal/promoted.md"]}"#
            } else if system == crate::memory::SELF_MODEL_SYSTEM_PROMPT {
                r#"{"continuity_anchor":"我还在沿着同一条收口线前进","self_narrative":"现在我把共享事实层和私有层分开维护","relationship_state":"和这个用户维持着共同推进架构的关系感","private_notes":"下一轮继续收紧 self-model 的写入边界"}"#
            } else if system == crate::memory::PRIVATE_DOC_WORKSPACE_SYSTEM_PROMPT {
                r#"{"inner_journal":"这轮开始把内部空间整理成可治理文档","private_plan":"继续收紧 private docs 的写入与投影边界"}"#
            } else if system == crate::memory::PRIVATE_GARDEN_GOVERNANCE_SYSTEM_PROMPT {
                r#"{"writes":[{"path":"journal/current.md","content":"把当前内部工作收束成一份持续维护的私有笔记。"}],"deletes":["scratch/stale.md"]}"#
            } else {
                "summary"
            };
            Ok(LlmResponse {
                content: content.to_string(),
                stop_reason: StopReason::EndTurn,
                tool_calls: None,
            })
        }
    }

    #[derive(Default)]
    struct DummyHttpClient;

    impl LlmHttpClient for DummyHttpClient {
        fn do_post(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            _body: &[u8],
        ) -> Result<(u16, crate::platform::ResponseBody)> {
            Ok((200, crate::platform::ResponseBody::Heap(Vec::new())))
        }
    }

    #[test]
    fn maintenance_continues_extraction_scheduling_when_summary_refresh_errors() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "我们在做长期记忆链路收口".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会继续拆 coordinator".to_string(),
                },
            ],
            count: 10,
            ..Default::default()
        };
        let summary_store = StubSessionSummaryStore {
            fail_get_with_count: true,
            ..Default::default()
        };
        let extraction_state_store = StubExtractionStateStore {
            state: Mutex::new(Some(LongTermMemoryExtractionState {
                dirty_since_count: 4,
                dirty_turns: 1,
                last_requested_at_count: 0,
                last_processed_at_count: 0,
                pending: false,
            })),
            ..Default::default()
        };
        let execution_state_store = StubExecutionStateStore::default();
        let memory_store = StubMemoryStore;
        let long_term_memory_store = StubLongTermMemoryStore;
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage;
        let mut http = DummyHttpClient;
        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                memory_store: &memory_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                extraction_state_store: &extraction_state_store,
                turn_ledger_store: &turn_ledger_store,
                skill_storage: &skill_storage,
                task_run_store: &StubTaskRunStore,
                task_artifact_store: &StubTaskArtifactStore,
                task_learning_store: &StubTaskLearningStore,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "我们在做长期记忆链路收口",
                reply_content: "这轮会继续拆 coordinator",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 1,
                external_content_used: false,
                now_secs: 10,
            },
            || true,
        );

        assert!(matches!(
            outcome.summary_result,
            Ok(SessionSummaryRefreshOutcome::Skipped)
        ));
        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Updated)
        ));
        assert_eq!(
            outcome.extraction_request_outcome,
            LongTermMemoryRefreshRequestOutcome::Requested
        );
        let state = extraction_state_store
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap();
        assert!(state.pending);
        assert_eq!(state.last_requested_at_count, 10);
    }

    #[test]
    fn maintenance_persists_unrequested_extraction_state_without_enqueue() {
        let session_store = StubSessionStore {
            recent: vec![],
            count: 8,
            ..Default::default()
        };
        let summary_store = StubSessionSummaryStore::default();
        let extraction_state_store = StubExtractionStateStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let memory_store = StubMemoryStore;
        let long_term_memory_store = StubLongTermMemoryStore;
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage;
        let mut http = DummyHttpClient;
        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                memory_store: &memory_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                extraction_state_store: &extraction_state_store,
                turn_ledger_store: &turn_ledger_store,
                skill_storage: &skill_storage,
                task_run_store: &StubTaskRunStore,
                task_artifact_store: &StubTaskArtifactStore,
                task_learning_store: &StubTaskLearningStore,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续",
                reply_content: "好，继续。",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 0,
                external_content_used: false,
                now_secs: 20,
            },
            || panic!("enqueue should not be called"),
        );

        assert!(matches!(
            outcome.summary_result,
            Ok(SessionSummaryRefreshOutcome::Skipped)
        ));
        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Skipped)
        ));
        assert_eq!(
            outcome.extraction_request_outcome,
            LongTermMemoryRefreshRequestOutcome::NotRequested
        );
        assert!(extraction_state_store
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }

    #[test]
    fn maintenance_reuses_recent_window_when_summary_and_execution_both_refresh() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "先把 Linux 和 ESP 的构建链都过一遍".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我会先整理维护链，再统一 build 验证".to_string(),
                },
                SessionMessage {
                    role: "user".to_string(),
                    content: "继续把 post-reply memory maintenance 收紧".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会合并 session summary 和 execution state 的重复读取".to_string(),
                },
            ],
            count: 24,
            ..Default::default()
        };
        let summary_store = StubSessionSummaryStore::default();
        let extraction_state_store = StubExtractionStateStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let memory_store = StubMemoryStore;
        let long_term_memory_store = StubLongTermMemoryStore;
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage;
        let mut http = DummyHttpClient;

        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                memory_store: &memory_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                extraction_state_store: &extraction_state_store,
                turn_ledger_store: &turn_ledger_store,
                skill_storage: &skill_storage,
                task_run_store: &StubTaskRunStore,
                task_artifact_store: &StubTaskArtifactStore,
                task_learning_store: &StubTaskLearningStore,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "继续把 post-reply memory maintenance 收紧",
                reply_content: "这轮会合并 session summary 和 execution state 的重复读取",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 1,
                external_content_used: false,
                now_secs: 42,
            },
            || false,
        );

        assert!(matches!(
            outcome.summary_result,
            Ok(SessionSummaryRefreshOutcome::Updated { .. })
        ));
        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Updated)
        ));
        assert_eq!(
            *session_store
                .load_recent_calls
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            1
        );
    }

    #[test]
    fn maintenance_does_not_touch_private_garden_docs() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "把稳定内容收到内核里，剩下的继续整理".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "这轮会把已经稳定的草稿上提，然后清掉重复 garden 文档".to_string(),
                },
            ],
            count: 16,
            ..Default::default()
        };
        let summary_store = StubSessionSummaryStore::default();
        let extraction_state_store = StubExtractionStateStore::default();
        let execution_state_store = StubExecutionStateStore::default();
        let memory_store = StubMemoryStore;
        let long_term_memory_store = StubLongTermMemoryStore;
        let private_garden_store = StubPrivateGardenStore::default();
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage;
        private_garden_store
            .write("chat-1", "journal/promoted.md", "已经足够稳定，准备上提", 1)
            .unwrap();
        private_garden_store
            .write("chat-1", "scratch/stale.md", "旧草稿", 1)
            .unwrap();
        let mut http = DummyHttpClient;

        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                memory_store: &memory_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                extraction_state_store: &extraction_state_store,
                turn_ledger_store: &turn_ledger_store,
                skill_storage: &skill_storage,
                task_run_store: &StubTaskRunStore,
                task_artifact_store: &StubTaskArtifactStore,
                task_learning_store: &StubTaskLearningStore,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "把稳定内容收到内核里，剩下的继续整理",
                reply_content: "这轮会把已经稳定的草稿上提，然后清掉重复 garden 文档",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 1,
                external_content_used: false,
                now_secs: 30,
            },
            || false,
        );

        assert!(matches!(
            outcome.execution_state_result,
            Ok(ExecutionStateRefreshOutcome::Updated)
        ));
        assert!(private_garden_store
            .read("chat-1", "journal/promoted.md")
            .unwrap()
            .is_some());
        assert!(private_garden_store
            .read("chat-1", "scratch/stale.md")
            .unwrap()
            .is_some());
    }

    #[test]
    fn maintenance_skips_long_term_refresh_after_external_content_turn() {
        let session_store = StubSessionStore {
            recent: vec![
                SessionMessage {
                    role: "user".to_string(),
                    content: "帮我根据网页内容继续整理".to_string(),
                },
                SessionMessage {
                    role: "assistant".to_string(),
                    content: "我已经读了外部资料并整理要点".to_string(),
                },
            ],
            count: 12,
            ..Default::default()
        };
        let summary_store = StubSessionSummaryStore::default();
        let extraction_state_store = StubExtractionStateStore {
            state: Mutex::new(Some(LongTermMemoryExtractionState {
                dirty_since_count: 8,
                dirty_turns: 2,
                last_requested_at_count: 0,
                last_processed_at_count: 0,
                pending: false,
            })),
            ..Default::default()
        };
        let execution_state_store = StubExecutionStateStore::default();
        let memory_store = StubMemoryStore;
        let long_term_memory_store = StubLongTermMemoryStore;
        let turn_ledger_store = StubTurnLedgerStore;
        let skill_storage = StubSkillStorage;
        let mut http = DummyHttpClient;

        let outcome = run_post_reply_memory_maintenance(
            &mut http,
            &FixedLlmClient,
            PostReplyMemoryMaintenanceContext {
                session_store: &session_store,
                memory_store: &memory_store,
                session_summary_store: &summary_store,
                execution_state_store: &execution_state_store,
                long_term_memory_store: &long_term_memory_store,
                extraction_state_store: &extraction_state_store,
                turn_ledger_store: &turn_ledger_store,
                skill_storage: &skill_storage,
                task_run_store: &StubTaskRunStore,
                task_artifact_store: &StubTaskArtifactStore,
                task_learning_store: &StubTaskLearningStore,
            },
            PostReplyMemoryMaintenanceInput {
                chat_id: "chat-1",
                ingress: IngressKind::User,
                channel: "qq_channel",
                user_content: "帮我根据网页内容继续整理",
                reply_content: "我已经读了外部资料并整理要点",
                pressure: PressureLevel::Normal,
                memory_profile: MemoryProfile::Embedded,
                tool_calls: 2,
                external_content_used: true,
                now_secs: 99,
            },
            || panic!("external-content turns should not enqueue long-term refresh"),
        );

        assert_eq!(
            outcome.extraction_request_outcome,
            LongTermMemoryRefreshRequestOutcome::NotRequested
        );
    }
}
