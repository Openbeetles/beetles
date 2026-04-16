use std::sync::Arc;

/// Shared runtime dependency envelope collected from `Platform` once and then
/// threaded through runtime assembly, registry construction, and agent setup.
#[derive(Clone)]
pub struct RuntimeServices {
    pub platform: Arc<dyn crate::Platform>,
    pub config_store: Arc<dyn crate::platform::ConfigStore + Send + Sync>,
    pub memory_system_kind: crate::memory::MemorySystemKind,
    pub skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync>,
    pub skill_meta_store: Arc<dyn crate::platform::SkillMetaStore + Send + Sync>,
    pub memory_store: Arc<dyn crate::memory::MemoryStore + Send + Sync>,
    pub long_term_memory_store: Arc<dyn crate::memory::LongTermMemoryStore + Send + Sync>,
    pub continuity_capsule_store: Arc<dyn crate::memory::ContinuityCapsuleStore + Send + Sync>,
    pub long_term_memory_extraction_state_store:
        Arc<dyn crate::memory::LongTermMemoryExtractionStateStore + Send + Sync>,
    pub session_store: Arc<dyn crate::memory::SessionStore + Send + Sync>,
    pub pending_retry_store: Arc<dyn crate::memory::PendingRetryStore + Send + Sync>,
    pub calendar_store: Arc<dyn crate::calendar::CalendarStore + Send + Sync>,
    pub office_credential_store: Arc<dyn crate::office::OfficeCredentialStore + Send + Sync>,
    pub office_runtime_status_store: Arc<dyn crate::office::OfficeRuntimeStatusStore + Send + Sync>,
    pub task_store: Arc<dyn crate::task::TaskStore + Send + Sync>,
    pub task_run_store: Arc<dyn crate::task_execution::TaskRunStore + Send + Sync>,
    pub task_artifact_store: Arc<dyn crate::task_execution::TaskArtifactStore + Send + Sync>,
    pub task_execution_ledger_store:
        Arc<dyn crate::task_execution::TaskExecutionLedgerStore + Send + Sync>,
    pub task_learning_store: Arc<dyn crate::task_execution::TaskLearningStore + Send + Sync>,
    pub active_work_store: Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>,
    pub execution_state_store: Arc<dyn crate::memory::ExecutionStateStore + Send + Sync>,
    pub self_model_store: Arc<dyn crate::memory::SelfModelStore + Send + Sync>,
    pub self_authored_core_store: Arc<dyn crate::memory::SelfAuthoredCoreStore + Send + Sync>,
    pub core_revision_ledger_store: Arc<dyn crate::memory::CoreRevisionLedgerStore + Send + Sync>,
    pub relationship_constitution_store:
        Arc<dyn crate::memory::RelationshipConstitutionStore + Send + Sync>,
    pub relationship_portfolio_store:
        Arc<dyn crate::memory::RelationshipPortfolioStore + Send + Sync>,
    pub world_sense_store: Arc<dyn crate::memory::WorldSenseStore + Send + Sync>,
    pub autonomy_strategy_store: Arc<dyn crate::memory::AutonomyStrategyStore + Send + Sync>,
    pub outer_voice_store: Arc<dyn crate::memory::OuterVoiceStore + Send + Sync>,
    pub inner_life_store: Arc<dyn crate::memory::InnerLifeStore + Send + Sync>,
    pub self_continuity_store: Arc<dyn crate::memory::SelfContinuityStore + Send + Sync>,
    pub relationship_topology_store:
        Arc<dyn crate::memory::RelationshipTopologyStore + Send + Sync>,
    pub private_doc_store: Arc<dyn crate::memory::PrivateDocStore + Send + Sync>,
    pub private_garden_store: Arc<dyn crate::memory::PrivateGardenStore + Send + Sync>,
    pub mental_privacy_store: Arc<dyn crate::memory::MentalPrivacyStore + Send + Sync>,
    pub important_message_store: Arc<dyn crate::memory::ImportantMessageStore + Send + Sync>,
    pub remind_at_store: Arc<dyn crate::memory::RemindAtStore + Send + Sync>,
    pub session_summary_store: Arc<dyn crate::memory::SessionSummaryStore + Send + Sync>,
    pub turn_ledger_store: Arc<dyn crate::memory::TurnLedgerStore + Send + Sync>,
    pub emotion_signal_store: Arc<crate::memory::MemoryEmotionSignalStore>,
}

impl RuntimeServices {
    pub fn from_platform(platform: Arc<dyn crate::Platform>) -> Self {
        Self {
            config_store: platform.config_store(),
            memory_system_kind: platform.memory_system_kind(),
            skill_storage: platform.skill_storage(),
            skill_meta_store: platform.skill_meta_store(),
            memory_store: platform.memory_store(),
            long_term_memory_store: platform.long_term_memory_store(),
            continuity_capsule_store: platform.continuity_capsule_store(),
            long_term_memory_extraction_state_store: platform
                .long_term_memory_extraction_state_store(),
            session_store: platform.session_store(),
            pending_retry_store: platform.pending_retry_store(),
            calendar_store: platform.calendar_store(),
            office_credential_store: platform.office_credential_store(),
            office_runtime_status_store: platform.office_runtime_status_store(),
            task_store: platform.task_store(),
            task_run_store: platform.task_run_store(),
            task_artifact_store: platform.task_artifact_store(),
            task_execution_ledger_store: platform.task_execution_ledger_store(),
            task_learning_store: platform.task_learning_store(),
            active_work_store: platform.active_work_store(),
            execution_state_store: platform.execution_state_store(),
            self_model_store: platform.self_model_store(),
            self_authored_core_store: platform.self_authored_core_store(),
            core_revision_ledger_store: platform.core_revision_ledger_store(),
            relationship_constitution_store: platform.relationship_constitution_store(),
            relationship_portfolio_store: platform.relationship_portfolio_store(),
            world_sense_store: platform.world_sense_store(),
            autonomy_strategy_store: platform.autonomy_strategy_store(),
            outer_voice_store: platform.outer_voice_store(),
            inner_life_store: platform.inner_life_store(),
            self_continuity_store: platform.self_continuity_store(),
            relationship_topology_store: platform.relationship_topology_store(),
            private_doc_store: platform.private_doc_store(),
            private_garden_store: platform.private_garden_store(),
            mental_privacy_store: platform.mental_privacy_store(),
            important_message_store: platform.important_message_store(),
            remind_at_store: platform.remind_at_store(),
            session_summary_store: platform.session_summary_store(),
            turn_ledger_store: platform.turn_ledger_store(),
            emotion_signal_store: Arc::new(crate::memory::MemoryEmotionSignalStore::new()),
            platform,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeServices;
    use crate::Platform;
    use std::sync::Arc;

    #[test]
    fn cloned_runtime_services_preserve_store_identity() {
        let platform: Arc<dyn Platform> = Arc::new(crate::platform::LinuxPlatform::new());
        let services = RuntimeServices::from_platform(Arc::clone(&platform));
        let clone = services.clone();

        assert!(Arc::ptr_eq(&services.platform, &clone.platform));
        assert!(Arc::ptr_eq(&services.memory_store, &clone.memory_store));
        assert!(Arc::ptr_eq(&services.session_store, &clone.session_store));
        assert!(Arc::ptr_eq(&services.config_store, &clone.config_store));
    }
}
