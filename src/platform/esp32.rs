//! ESP32 平台的 Platform 实现。仅在此目标编译。
//! ESP32 implementation of Platform trait.

#[cfg(all(
    feature = "capability_office",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
use crate::office::{OfficeCredentialStore, OfficeRuntimeStatusStore};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::abstraction::{MemorySnapshot, Platform, StateFs};
#[cfg(all(
    feature = "capability_office",
    any(target_arch = "xtensa", target_arch = "riscv32")
))]
use crate::platform::storage::{StorageOfficeCredentialStore, StorageOfficeRuntimeStatusStore};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::platform::{
    display_driver::{install_display_state, DisplayState},
    heartbeat_file::read_heartbeat_file,
    storage::{
        storage_usage, CachedSkillMetaStore, CachedSkillStorage, StorageActiveWorkStore,
        StorageAutonomyStrategyStore, StorageCalendarStore, StorageContinuityCapsuleStore,
        StorageCoreRevisionLedgerStore, StorageExecutionStateStore, StorageFeltSignificanceStore,
        StorageImportantMessageStore, StorageInnerConflictStore, StorageInnerLifeStore,
        StorageLongTermMemoryExtractionStateStore, StorageLongTermMemoryStore, StorageMemoryStore,
        StorageMentalPrivacyStore, StorageOuterVoiceStore, StoragePendingRetryStore,
        StoragePrivateDocStore, StoragePrivateGardenStore, StorageRelationshipConstitutionStore,
        StorageRelationshipPortfolioStore, StorageRelationshipTopologyStore, StorageRemindAtStore,
        StorageSelfAuthoredCoreStore, StorageSelfContinuityStore, StorageSelfModelStore,
        StorageSessionStore, StorageSessionSummaryStore, StorageSkillMetaStore,
        StorageSkillStorage, StorageTaskArtifactStore, StorageTaskExecutionLedgerStore,
        StorageTaskLearningStore, StorageTaskRunStore, StorageTaskStore,
        StorageTemperamentContinuityStore, StorageTurnContinuityEvidenceStore,
        StorageWorldSenseStore,
    },
    NvsConfigStore,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::runtime::write_back::{
    BufferedActiveWorkStore, BufferedAutonomyStrategyStore, BufferedCoreRevisionLedgerStore,
    BufferedExecutionStateStore, BufferedFeltSignificanceStore, BufferedImportantMessageStore,
    BufferedInnerConflictStore, BufferedInnerLifeStore, BufferedLongTermExtractionStateStore,
    BufferedMentalPrivacyStore, BufferedOuterVoiceStore, BufferedRelationshipConstitutionStore,
    BufferedRelationshipPortfolioStore, BufferedRelationshipTopologyStore,
    BufferedSelfAuthoredCoreStore, BufferedSelfContinuityStore, BufferedSelfModelStore,
    BufferedSessionStore, BufferedSessionSummaryStore, BufferedTemperamentContinuityStore,
    BufferedTurnContinuityEvidenceStore, BufferedWorldSenseStore, ProfiledCoreRevisionLedgerStore,
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use crate::{
    calendar::CalendarStore,
    config::{AppConfig, AudioSegment},
    display::{DisplayCommand, DisplayConfig},
    memory::{
        AutonomyStrategyStore, ContinuityCapsuleStore, CoreRevisionLedgerStore,
        ExecutionStateStore, FeltSignificanceStore, ImportantMessageStore, InnerConflictStore,
        InnerLifeStore, LongTermMemoryExtractionStateStore, LongTermMemoryStore, MemoryStore,
        MentalPrivacyStore, OuterVoiceStore, PendingRetryStore, PrivateDocStore,
        PrivateGardenStore, RelationshipConstitutionStore, RelationshipPortfolioStore,
        RelationshipTopologyStore, RemindAtStore, SelfAuthoredCoreStore, SelfContinuityStore,
        SelfModelStore, SessionStore, SessionSummaryStore, TemperamentContinuityStore,
        TurnContinuityEvidenceStore, TurnLedgerStore, VolatileTurnLedgerStore, WorldSenseStore,
    },
    task::TaskStore,
    task_execution::{
        TaskArtifactStore, TaskExecutionLedgerStore, TaskLearningStore, TaskRunStore,
    },
};
#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
use std::sync::{Arc, Mutex, RwLock};

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
/// ESP32 平台实现。
pub struct Esp32Platform {
    state_fs: Arc<dyn StateFs + Send + Sync>,
    config_store: Arc<NvsConfigStore>,
    skill_storage: Arc<dyn crate::platform::SkillStorage + Send + Sync>,
    skill_meta_store: Arc<dyn crate::platform::SkillMetaStore + Send + Sync>,
    memory_store: Arc<StorageMemoryStore>,
    long_term_memory_store: Arc<StorageLongTermMemoryStore>,
    continuity_capsule_store: Arc<dyn ContinuityCapsuleStore + Send + Sync>,
    long_term_memory_extraction_state_store:
        Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync>,
    session_store: Arc<dyn SessionStore + Send + Sync>,
    pending_retry_store: Arc<StoragePendingRetryStore>,
    calendar_store: Arc<StorageCalendarStore>,
    #[cfg(feature = "capability_office")]
    office_credential_store: Arc<StorageOfficeCredentialStore>,
    #[cfg(feature = "capability_office")]
    office_runtime_status_store: Arc<StorageOfficeRuntimeStatusStore>,
    task_store: Arc<StorageTaskStore>,
    task_run_store: Arc<StorageTaskRunStore>,
    task_artifact_store: Arc<StorageTaskArtifactStore>,
    task_execution_ledger_store: Arc<StorageTaskExecutionLedgerStore>,
    task_learning_store: Arc<StorageTaskLearningStore>,
    active_work_store: Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>,
    detached_work_store: Arc<dyn crate::agent::DetachedWorkStore + Send + Sync>,
    execution_state_store: Arc<dyn ExecutionStateStore + Send + Sync>,
    self_model_store: Arc<dyn SelfModelStore + Send + Sync>,
    self_authored_core_store: Arc<dyn SelfAuthoredCoreStore + Send + Sync>,
    core_revision_ledger_store: Arc<dyn CoreRevisionLedgerStore + Send + Sync>,
    relationship_constitution_store: Arc<dyn RelationshipConstitutionStore + Send + Sync>,
    world_sense_store: Arc<dyn WorldSenseStore + Send + Sync>,
    autonomy_strategy_store: Arc<dyn AutonomyStrategyStore + Send + Sync>,
    outer_voice_store: Arc<dyn OuterVoiceStore + Send + Sync>,
    inner_life_store: Arc<dyn InnerLifeStore + Send + Sync>,
    self_continuity_store: Arc<dyn SelfContinuityStore + Send + Sync>,
    felt_significance_store: Arc<dyn FeltSignificanceStore + Send + Sync>,
    temperament_continuity_store: Arc<dyn TemperamentContinuityStore + Send + Sync>,
    inner_conflict_store: Arc<dyn InnerConflictStore + Send + Sync>,
    relationship_portfolio_store: Arc<dyn RelationshipPortfolioStore + Send + Sync>,
    relationship_topology_store: Arc<dyn RelationshipTopologyStore + Send + Sync>,
    private_doc_store: Arc<StoragePrivateDocStore>,
    private_garden_store: Arc<StoragePrivateGardenStore>,
    mental_privacy_store: Arc<dyn MentalPrivacyStore + Send + Sync>,
    important_message_store: Arc<dyn ImportantMessageStore + Send + Sync>,
    remind_at_store: Arc<StorageRemindAtStore>,
    session_summary_store: Arc<dyn SessionSummaryStore + Send + Sync>,
    turn_continuity_evidence_store: Arc<dyn TurnContinuityEvidenceStore + Send + Sync>,
    turn_ledger_store: Arc<dyn TurnLedgerStore + Send + Sync>,
    wifi_scan_handle: Mutex<Option<Arc<dyn crate::platform::WifiScan + Send + Sync>>>,
    display_state: Mutex<Option<DisplayState>>,
    i2c_state: Mutex<Option<crate::platform::hardware_drivers::I2cBusState>>,
    audio_state: RwLock<Option<Arc<crate::platform::audio_drivers::AudioPipelineState>>>,
    audio_capabilities: RwLock<crate::platform::AudioDuplexCapabilities>,
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Esp32Platform {
    pub fn new() -> Self {
        let state_fs: Arc<dyn StateFs + Send + Sync> =
            Arc::new(crate::platform::state_fs::Esp32StateFs);
        let long_term_memory_extraction_state_store = BufferedLongTermExtractionStateStore::wrap(
            Arc::new(StorageLongTermMemoryExtractionStateStore::new())
                as Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync>,
        );
        let session_store = BufferedSessionStore::wrap(
            Arc::new(StorageSessionStore::new()) as Arc<dyn SessionStore + Send + Sync>
        );
        let execution_state_store =
            BufferedExecutionStateStore::wrap(Arc::new(StorageExecutionStateStore::new())
                as Arc<dyn ExecutionStateStore + Send + Sync>);
        let self_model_store = BufferedSelfModelStore::wrap(
            Arc::new(StorageSelfModelStore::new()) as Arc<dyn SelfModelStore + Send + Sync>
        );
        let self_authored_core_store =
            BufferedSelfAuthoredCoreStore::wrap(Arc::new(StorageSelfAuthoredCoreStore::new())
                as Arc<dyn SelfAuthoredCoreStore + Send + Sync>);
        let core_revision_ledger_store = ProfiledCoreRevisionLedgerStore::wrap(
            BufferedCoreRevisionLedgerStore::wrap(Arc::new(StorageCoreRevisionLedgerStore::new())
                as Arc<dyn CoreRevisionLedgerStore + Send + Sync>),
            crate::memory::MemoryProfile::Embedded,
        );
        let relationship_constitution_store = BufferedRelationshipConstitutionStore::wrap(
            Arc::new(StorageRelationshipConstitutionStore::new())
                as Arc<dyn RelationshipConstitutionStore + Send + Sync>,
        );
        let world_sense_store = BufferedWorldSenseStore::wrap(
            Arc::new(StorageWorldSenseStore::new()) as Arc<dyn WorldSenseStore + Send + Sync>,
        );
        let autonomy_strategy_store =
            BufferedAutonomyStrategyStore::wrap(Arc::new(StorageAutonomyStrategyStore::new())
                as Arc<dyn AutonomyStrategyStore + Send + Sync>);
        let outer_voice_store = BufferedOuterVoiceStore::wrap(
            Arc::new(StorageOuterVoiceStore::new()) as Arc<dyn OuterVoiceStore + Send + Sync>,
        );
        let inner_life_store = BufferedInnerLifeStore::wrap(
            Arc::new(StorageInnerLifeStore::new()) as Arc<dyn InnerLifeStore + Send + Sync>
        );
        let self_continuity_store =
            BufferedSelfContinuityStore::wrap(Arc::new(StorageSelfContinuityStore::new())
                as Arc<dyn SelfContinuityStore + Send + Sync>);
        let felt_significance_store =
            BufferedFeltSignificanceStore::wrap(Arc::new(StorageFeltSignificanceStore::new())
                as Arc<dyn FeltSignificanceStore + Send + Sync>);
        let temperament_continuity_store = BufferedTemperamentContinuityStore::wrap(Arc::new(
            StorageTemperamentContinuityStore::new(),
        )
            as Arc<dyn TemperamentContinuityStore + Send + Sync>);
        let inner_conflict_store =
            BufferedInnerConflictStore::wrap(Arc::new(StorageInnerConflictStore::new())
                as Arc<dyn InnerConflictStore + Send + Sync>);
        let relationship_portfolio_store = BufferedRelationshipPortfolioStore::wrap(Arc::new(
            StorageRelationshipPortfolioStore::new(),
        )
            as Arc<dyn RelationshipPortfolioStore + Send + Sync>);
        let relationship_topology_store = BufferedRelationshipTopologyStore::wrap(Arc::new(
            StorageRelationshipTopologyStore::new(),
        )
            as Arc<dyn RelationshipTopologyStore + Send + Sync>);
        let mental_privacy_store =
            BufferedMentalPrivacyStore::wrap(Arc::new(StorageMentalPrivacyStore::new())
                as Arc<dyn MentalPrivacyStore + Send + Sync>);
        let important_message_store =
            BufferedImportantMessageStore::wrap(Arc::new(StorageImportantMessageStore::new())
                as Arc<dyn ImportantMessageStore + Send + Sync>);
        let session_summary_store =
            BufferedSessionSummaryStore::wrap(Arc::new(StorageSessionSummaryStore::new())
                as Arc<dyn SessionSummaryStore + Send + Sync>);
        let turn_continuity_evidence_store = BufferedTurnContinuityEvidenceStore::wrap(Arc::new(
            StorageTurnContinuityEvidenceStore::new(),
        )
            as Arc<dyn TurnContinuityEvidenceStore + Send + Sync>);
        let turn_ledger_store =
            Arc::new(VolatileTurnLedgerStore::new(4)) as Arc<dyn TurnLedgerStore + Send + Sync>;
        let active_work_store =
            BufferedActiveWorkStore::wrap(Arc::new(StorageActiveWorkStore::new())
                as Arc<dyn crate::agent::ActiveWorkStore + Send + Sync>);
        Self {
            state_fs,
            config_store: Arc::new(NvsConfigStore),
            skill_storage: CachedSkillStorage::wrap(Arc::new(StorageSkillStorage)),
            skill_meta_store: CachedSkillMetaStore::wrap(Arc::new(StorageSkillMetaStore)),
            memory_store: Arc::new(StorageMemoryStore::new()),
            long_term_memory_store: Arc::new(StorageLongTermMemoryStore::new()),
            continuity_capsule_store: Arc::new(StorageContinuityCapsuleStore::new()),
            long_term_memory_extraction_state_store,
            session_store,
            pending_retry_store: Arc::new(StoragePendingRetryStore::new()),
            calendar_store: Arc::new(StorageCalendarStore::new()),
            #[cfg(feature = "capability_office")]
            office_credential_store: Arc::new(StorageOfficeCredentialStore::new()),
            #[cfg(feature = "capability_office")]
            office_runtime_status_store: Arc::new(StorageOfficeRuntimeStatusStore::new()),
            task_store: Arc::new(StorageTaskStore::new()),
            task_run_store: Arc::new(StorageTaskRunStore::new()),
            task_artifact_store: Arc::new(StorageTaskArtifactStore::new()),
            task_execution_ledger_store: Arc::new(StorageTaskExecutionLedgerStore::new()),
            task_learning_store: Arc::new(StorageTaskLearningStore::new()),
            active_work_store,
            detached_work_store: Arc::new(crate::agent::VolatileDetachedWorkStore::new()),
            execution_state_store,
            self_model_store,
            self_authored_core_store,
            core_revision_ledger_store,
            relationship_constitution_store,
            world_sense_store,
            autonomy_strategy_store,
            outer_voice_store,
            inner_life_store,
            self_continuity_store,
            felt_significance_store,
            temperament_continuity_store,
            inner_conflict_store,
            relationship_portfolio_store,
            relationship_topology_store,
            private_doc_store: Arc::new(StoragePrivateDocStore::new()),
            private_garden_store: Arc::new(StoragePrivateGardenStore::new()),
            mental_privacy_store,
            important_message_store,
            remind_at_store: Arc::new(StorageRemindAtStore::new()),
            session_summary_store,
            turn_continuity_evidence_store,
            turn_ledger_store,
            wifi_scan_handle: Mutex::new(None),
            display_state: Mutex::new(None),
            i2c_state: Mutex::new(None),
            audio_state: RwLock::new(None),
            audio_capabilities: RwLock::new(crate::platform::AudioDuplexCapabilities::unavailable()),
        }
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Default for Esp32Platform {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl crate::platform::PlatformCamera for Esp32Platform {}

#[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
impl Platform for Esp32Platform {
    fn state_fs(&self) -> Arc<dyn StateFs + Send + Sync> {
        Arc::clone(&self.state_fs)
    }

    fn memory_snapshot(&self) -> MemorySnapshot {
        use crate::platform::heap::{
            heap_free_internal, heap_free_spiram, heap_largest_free_block_internal,
            heap_largest_free_block_spiram, heap_min_free_internal, heap_min_free_spiram,
            heap_total_spiram,
        };
        MemorySnapshot {
            heap_free_internal: heap_free_internal() as u32,
            heap_min_free_internal: heap_min_free_internal() as u32,
            heap_free_spiram: heap_free_spiram() as u32,
            heap_total_spiram: heap_total_spiram() as u32,
            heap_min_free_spiram: heap_min_free_spiram() as u32,
            heap_largest_block_spiram: heap_largest_free_block_spiram() as u32,
            heap_largest_block: heap_largest_free_block_internal() as u32,
        }
    }

    fn memory_system_kind(&self) -> crate::memory::MemorySystemKind {
        crate::memory::MemorySystemKind::EspCompact
    }

    fn init(&self) -> crate::error::Result<()> {
        esp_idf_svc::sys::link_patches();
        let logger = esp_idf_svc::log::init_from_esp_idf();
        // 尽量压低 esp-idf-svc httpd 的注册日志；若 target filter 失效，vendored esp-idf-svc
        // 里也已把对应日志降到了 debug，这里保留为额外兜底。
        for target in ["esp_idf_svc::http::server", "esp-idf-svc::http::server"] {
            let _ = logger
                .filter()
                .set_target_level(target, log::LevelFilter::Warn);
        }
        self.init_nvs()?;
        self.init_storage()?;
        if let Err(e) = self.remind_at_store.warm_cache() {
            log::warn!("[platform::esp32] warm remind cache failed: {}", e);
        }
        if let Err(e) = self.task_store.warm_cache() {
            log::warn!("[platform::esp32] warm task cache failed: {}", e);
        }
        Ok(())
    }

    fn init_nvs(&self) -> crate::error::Result<()> {
        crate::platform::nvs::init_nvs()
    }

    fn init_storage(&self) -> crate::error::Result<()> {
        crate::platform::storage::init_storage()
    }

    fn config_store(&self) -> Arc<dyn crate::platform::ConfigStore + Send + Sync> {
        Arc::clone(&self.config_store) as Arc<dyn crate::platform::ConfigStore + Send + Sync>
    }

    fn connect_wifi(&self, config: &AppConfig) -> crate::error::Result<()> {
        if let Some(handle) = crate::platform::connect_wifi(config)? {
            let arc_dyn: Arc<dyn crate::platform::WifiScan + Send + Sync> = Arc::new(handle);
            *self
                .wifi_scan_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(arc_dyn);
        }
        Ok(())
    }

    fn wifi_scan(&self) -> Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> {
        let opt: Option<Arc<dyn crate::platform::WifiScan + Send + Sync>> = self
            .wifi_scan_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        opt
    }

    fn wifi_sta_ip(&self) -> Option<String> {
        crate::platform::wifi::wifi_sta_ip()
    }

    fn memory_store(&self) -> Arc<dyn MemoryStore + Send + Sync> {
        Arc::clone(&self.memory_store) as Arc<dyn MemoryStore + Send + Sync>
    }

    fn long_term_memory_store(&self) -> Arc<dyn LongTermMemoryStore + Send + Sync> {
        Arc::clone(&self.long_term_memory_store) as Arc<dyn LongTermMemoryStore + Send + Sync>
    }

    fn continuity_capsule_store(&self) -> Arc<dyn ContinuityCapsuleStore + Send + Sync> {
        Arc::clone(&self.continuity_capsule_store)
    }

    fn long_term_memory_extraction_state_store(
        &self,
    ) -> Arc<dyn LongTermMemoryExtractionStateStore + Send + Sync> {
        Arc::clone(&self.long_term_memory_extraction_state_store)
    }

    fn session_store(&self) -> Arc<dyn SessionStore + Send + Sync> {
        Arc::clone(&self.session_store)
    }

    fn pending_retry_store(&self) -> Arc<dyn PendingRetryStore + Send + Sync> {
        Arc::clone(&self.pending_retry_store) as Arc<dyn PendingRetryStore + Send + Sync>
    }

    fn calendar_store(&self) -> Arc<dyn CalendarStore + Send + Sync> {
        Arc::clone(&self.calendar_store) as Arc<dyn CalendarStore + Send + Sync>
    }

    #[cfg(feature = "capability_office")]
    fn office_credential_store(&self) -> Arc<dyn OfficeCredentialStore + Send + Sync> {
        Arc::clone(&self.office_credential_store) as Arc<dyn OfficeCredentialStore + Send + Sync>
    }

    #[cfg(feature = "capability_office")]
    fn office_runtime_status_store(&self) -> Arc<dyn OfficeRuntimeStatusStore + Send + Sync> {
        Arc::clone(&self.office_runtime_status_store)
            as Arc<dyn OfficeRuntimeStatusStore + Send + Sync>
    }

    fn task_store(&self) -> Arc<dyn TaskStore + Send + Sync> {
        Arc::clone(&self.task_store) as Arc<dyn TaskStore + Send + Sync>
    }

    fn task_run_store(&self) -> Arc<dyn TaskRunStore + Send + Sync> {
        Arc::clone(&self.task_run_store) as Arc<dyn TaskRunStore + Send + Sync>
    }

    fn task_artifact_store(&self) -> Arc<dyn TaskArtifactStore + Send + Sync> {
        Arc::clone(&self.task_artifact_store) as Arc<dyn TaskArtifactStore + Send + Sync>
    }

    fn task_execution_ledger_store(&self) -> Arc<dyn TaskExecutionLedgerStore + Send + Sync> {
        Arc::clone(&self.task_execution_ledger_store)
            as Arc<dyn TaskExecutionLedgerStore + Send + Sync>
    }

    fn task_learning_store(&self) -> Arc<dyn TaskLearningStore + Send + Sync> {
        Arc::clone(&self.task_learning_store) as Arc<dyn TaskLearningStore + Send + Sync>
    }

    fn active_work_store(&self) -> Arc<dyn crate::agent::ActiveWorkStore + Send + Sync> {
        Arc::clone(&self.active_work_store)
    }
    fn detached_work_store(&self) -> Arc<dyn crate::agent::DetachedWorkStore + Send + Sync> {
        Arc::clone(&self.detached_work_store)
    }

    fn execution_state_store(&self) -> Arc<dyn ExecutionStateStore + Send + Sync> {
        Arc::clone(&self.execution_state_store)
    }

    fn self_model_store(&self) -> Arc<dyn SelfModelStore + Send + Sync> {
        Arc::clone(&self.self_model_store)
    }

    fn self_authored_core_store(&self) -> Arc<dyn SelfAuthoredCoreStore + Send + Sync> {
        Arc::clone(&self.self_authored_core_store)
    }

    fn core_revision_ledger_store(&self) -> Arc<dyn CoreRevisionLedgerStore + Send + Sync> {
        Arc::clone(&self.core_revision_ledger_store)
    }

    fn relationship_constitution_store(
        &self,
    ) -> Arc<dyn RelationshipConstitutionStore + Send + Sync> {
        Arc::clone(&self.relationship_constitution_store)
    }

    fn world_sense_store(&self) -> Arc<dyn WorldSenseStore + Send + Sync> {
        Arc::clone(&self.world_sense_store)
    }

    fn autonomy_strategy_store(&self) -> Arc<dyn AutonomyStrategyStore + Send + Sync> {
        Arc::clone(&self.autonomy_strategy_store)
    }

    fn outer_voice_store(&self) -> Arc<dyn OuterVoiceStore + Send + Sync> {
        Arc::clone(&self.outer_voice_store)
    }

    fn inner_life_store(&self) -> Arc<dyn InnerLifeStore + Send + Sync> {
        Arc::clone(&self.inner_life_store)
    }

    fn self_continuity_store(&self) -> Arc<dyn SelfContinuityStore + Send + Sync> {
        Arc::clone(&self.self_continuity_store)
    }

    fn felt_significance_store(&self) -> Arc<dyn FeltSignificanceStore + Send + Sync> {
        Arc::clone(&self.felt_significance_store)
    }

    fn temperament_continuity_store(&self) -> Arc<dyn TemperamentContinuityStore + Send + Sync> {
        Arc::clone(&self.temperament_continuity_store)
    }

    fn inner_conflict_store(&self) -> Arc<dyn InnerConflictStore + Send + Sync> {
        Arc::clone(&self.inner_conflict_store)
    }

    fn relationship_portfolio_store(&self) -> Arc<dyn RelationshipPortfolioStore + Send + Sync> {
        Arc::clone(&self.relationship_portfolio_store)
    }

    fn relationship_topology_store(&self) -> Arc<dyn RelationshipTopologyStore + Send + Sync> {
        Arc::clone(&self.relationship_topology_store)
    }

    fn private_doc_store(&self) -> Arc<dyn PrivateDocStore + Send + Sync> {
        Arc::clone(&self.private_doc_store) as Arc<dyn PrivateDocStore + Send + Sync>
    }

    fn private_garden_store(&self) -> Arc<dyn PrivateGardenStore + Send + Sync> {
        Arc::clone(&self.private_garden_store) as Arc<dyn PrivateGardenStore + Send + Sync>
    }

    fn mental_privacy_store(&self) -> Arc<dyn MentalPrivacyStore + Send + Sync> {
        Arc::clone(&self.mental_privacy_store)
    }

    fn important_message_store(&self) -> Arc<dyn ImportantMessageStore + Send + Sync> {
        Arc::clone(&self.important_message_store)
    }

    fn remind_at_store(&self) -> Arc<dyn RemindAtStore + Send + Sync> {
        Arc::clone(&self.remind_at_store) as Arc<dyn RemindAtStore + Send + Sync>
    }

    fn session_summary_store(&self) -> Arc<dyn SessionSummaryStore + Send + Sync> {
        Arc::clone(&self.session_summary_store)
    }

    fn turn_continuity_evidence_store(&self) -> Arc<dyn TurnContinuityEvidenceStore + Send + Sync> {
        Arc::clone(&self.turn_continuity_evidence_store)
    }

    fn turn_ledger_store(&self) -> Arc<dyn TurnLedgerStore + Send + Sync> {
        Arc::clone(&self.turn_ledger_store)
    }

    fn skill_storage(&self) -> Arc<dyn crate::platform::SkillStorage + Send + Sync> {
        Arc::clone(&self.skill_storage) as Arc<dyn crate::platform::SkillStorage + Send + Sync>
    }

    fn skill_meta_store(&self) -> Arc<dyn crate::platform::SkillMetaStore + Send + Sync> {
        Arc::clone(&self.skill_meta_store)
    }

    fn create_http_client(
        &self,
        config: &AppConfig,
    ) -> crate::error::Result<Box<dyn crate::platform::PlatformHttpClient>> {
        if !config.proxy_url.trim().is_empty() {
            Ok(Box::new(crate::platform::EspHttpClient::new_with_config(
                config,
            )?))
        } else {
            Ok(Box::new(crate::platform::EspHttpClient::new()?))
        }
    }

    fn create_interactive_http_client(
        &self,
        config: &AppConfig,
    ) -> crate::error::Result<Box<dyn crate::platform::PlatformHttpClient>> {
        if !config.proxy_url.trim().is_empty() {
            Ok(Box::new(
                crate::platform::EspHttpClient::new_with_config_and_priority(
                    config,
                    crate::orchestrator::Priority::High,
                )?,
            ))
        } else {
            Ok(Box::new(crate::platform::EspHttpClient::new_with_priority(
                crate::orchestrator::Priority::High,
            )?))
        }
    }

    fn storage_usage(&self) -> Option<(u64, u64)> {
        storage_usage()
    }

    fn read_heartbeat_file(&self) -> crate::error::Result<String> {
        read_heartbeat_file()
    }

    fn request_restart(&self) {
        unsafe { esp_idf_svc::sys::esp_restart() };
    }

    fn init_sntp(&self) {
        crate::platform::sntp::init_sntp();
    }

    fn init_display(&self, config: &DisplayConfig) -> crate::error::Result<()> {
        let mut guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        install_display_state(&mut guard, config)
    }

    fn init_audio(
        &self,
        config: &AudioSegment,
        i2c_bus: Option<&crate::config::I2cBusConfig>,
        i2s_bus: Option<&crate::config::I2sBusConfig>,
    ) -> crate::error::Result<()> {
        if !config.enabled {
            *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) = None;
            *self
                .audio_capabilities
                .write()
                .unwrap_or_else(|e| e.into_inner()) =
                crate::platform::AudioDuplexCapabilities::unavailable();
            return Ok(());
        }
        if !crate::config::audio_runtime_pipeline_enabled(config) {
            *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) = None;
            *self
                .audio_capabilities
                .write()
                .unwrap_or_else(|e| e.into_inner()) =
                crate::platform::AudioDuplexCapabilities::unavailable();
            return Ok(());
        }
        match crate::platform::audio_drivers::AudioPipelineState::from_config(
            config, i2c_bus, i2s_bus,
        ) {
            Ok(state) => {
                let capabilities = state.duplex_capabilities().normalized();
                *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) =
                    Some(Arc::new(state));
                *self
                    .audio_capabilities
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) = capabilities;
                log::info!(
                    "[platform::esp32] audio contract profile={} mic={} speaker={} duplex={} barge_in={} reference={:?} aec={:?}",
                    capabilities.profile().as_str(),
                    capabilities.microphone_input,
                    capabilities.speaker_output,
                    capabilities.concurrent_capture_playback,
                    capabilities.barge_in,
                    capabilities.reference_capture,
                    capabilities.echo_cancellation,
                );
                Ok(())
            }
            Err(e) => {
                *self.audio_state.write().unwrap_or_else(|e| e.into_inner()) = None;
                *self
                    .audio_capabilities
                    .write()
                    .unwrap_or_else(|e| e.into_inner()) =
                    crate::platform::AudioDuplexCapabilities::unavailable();
                Err(e)
            }
        }
    }

    fn audio_duplex_capabilities(&self) -> crate::platform::AudioDuplexCapabilities {
        *self
            .audio_capabilities
            .read()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn read_mic_pcm_i16(&self, out: &mut [i16]) -> crate::error::Result<usize> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_mic", "audio pipeline not initialized")
            })?;
        state.read_mic_pcm_i16(out)
    }

    fn write_speaker_pcm_i16(&self, buf: &[i16]) -> crate::error::Result<()> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_speaker", "audio pipeline not initialized")
            })?;
        state.write_speaker_pcm_i16(buf)
    }

    fn try_write_speaker_pcm_i16(&self, buf: &[i16]) -> crate::error::Result<usize> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_speaker", "audio pipeline not initialized")
            })?;
        state.try_write_speaker_pcm_i16(buf)
    }

    fn read_playback_reference_pcm_i16(&self, out: &mut [i16]) -> crate::error::Result<usize> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_reference", "audio pipeline not initialized")
            })?;
        state.read_playback_reference_pcm_i16(out)
    }

    fn speaker_buffered_samples(&self) -> usize {
        self.audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.speaker_buffered_samples())
            .unwrap_or(0)
    }

    fn clear_speaker_buffer(&self) -> crate::error::Result<()> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_speaker", "audio pipeline not initialized")
            })?;
        state.clear_speaker_buffer()
    }

    fn push_speaker_staging_pcm_i16(&self, buf: &[i16]) -> crate::error::Result<usize> {
        let state = self
            .audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                crate::error::Error::config("audio_speaker", "audio pipeline not initialized")
            })?;
        state.push_staging_pcm_i16(buf)
    }

    fn speaker_staging_samples(&self) -> usize {
        self.audio_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.staging_samples())
            .unwrap_or(0)
    }

    fn display_available(&self) -> bool {
        self.display_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.available)
            .unwrap_or(false)
    }

    fn display_command(&self, cmd: DisplayCommand) -> crate::error::Result<()> {
        let mut guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.execute(cmd),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn set_display_backlight(&self, on: bool) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.set_backlight(on),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn display_backlight_available(&self) -> bool {
        self.display_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.backlight_available())
            .unwrap_or(false)
    }

    fn set_display_backlight_brightness(&self, percent: u8) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.set_brightness(percent),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn fade_display_backlight(
        &self,
        from: u8,
        to: u8,
        duration_ms: u32,
    ) -> crate::error::Result<()> {
        let guard = self.display_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(state) => state.fade_brightness(from, to, duration_ms),
            None => Err(crate::error::Error::config(
                "display",
                "display not initialized",
            )),
        }
    }

    fn drive_gpio_out(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_gpio_out(pins, params)
    }

    fn drive_gpio_in(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_gpio_in(pins, params, options)
    }

    fn drive_pwm_out(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
        ledc_channel: u8,
        ledc_timer_index: u8,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_pwm_out(
            pins,
            params,
            options,
            ledc_channel,
            ledc_timer_index,
        )
    }

    fn drive_adc_in(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_adc_in(pins, params, options)
    }

    fn drive_buzzer(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_buzzer(pins, params)
    }

    fn drive_dht(
        &self,
        pins: &crate::config::PinConfig,
        params: &serde_json::Value,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        crate::platform::hardware_drivers::drive_dht(pins, params, options)
    }

    fn drive_i2c_sensor(
        &self,
        addr: u8,
        model: &str,
        options: &serde_json::Value,
    ) -> crate::error::Result<String> {
        use crate::platform::hardware_drivers::{parse_aht20, parse_sht3x};
        use std::time::Duration;

        match model {
            "sht3x" => {
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, 0x2C, &[0x06])?;
                }
                std::thread::sleep(Duration::from_millis(15));
                let data = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, 6)?
                };
                let (temperature, humidity) = parse_sht3x(&data)?;
                Ok(format!(
                    r#"{{"temperature":{},"humidity":{},"model":"sht3x"}}"#,
                    temperature, humidity
                ))
            }
            "aht20" => {
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, 0xAC, &[0x33, 0x00])?;
                }
                std::thread::sleep(Duration::from_millis(80));
                let data = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, 6)?
                };
                let (temperature, humidity) = parse_aht20(&data)?;
                Ok(format!(
                    r#"{{"temperature":{},"humidity":{},"model":"aht20"}}"#,
                    temperature, humidity
                ))
            }
            "raw" => {
                let arr = options
                    .get("init_cmd")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        crate::error::Error::config(
                            "drive_i2c_sensor",
                            "raw model requires options.init_cmd",
                        )
                    })?;
                let mut cmd: Vec<u8> = Vec::with_capacity(arr.len());
                for el in arr {
                    let b = el.as_u64().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "init_cmd must be u8")
                    })?;
                    if b > 255 {
                        return Err(crate::error::Error::config(
                            "drive_i2c_sensor",
                            "init_cmd byte must be 0-255",
                        ));
                    }
                    cmd.push(b as u8);
                }
                if cmd.is_empty() {
                    return Err(crate::error::Error::config(
                        "drive_i2c_sensor",
                        "init_cmd must not be empty",
                    ));
                }
                let read_len = options
                    .get("read_len")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "raw requires read_len")
                    })? as usize;
                let wait_ms = options
                    .get("conversion_wait_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(15)
                    .min(2000);
                let reg = cmd[0];
                let tail = &cmd[1..];
                {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.write(addr, reg, tail)?;
                }
                std::thread::sleep(Duration::from_millis(wait_ms));
                let buf = {
                    let mut g = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
                    let s = g.as_mut().ok_or_else(|| {
                        crate::error::Error::config("drive_i2c_sensor", "I2C bus not initialized")
                    })?;
                    s.receive(addr, read_len)?
                };
                let hex: String = buf.iter().map(|b| format!("{:02X}", b)).collect();
                Ok(format!(r#"{{"raw":"{}","model":"raw"}}"#, hex))
            }
            other => Err(crate::error::Error::config(
                "drive_i2c_sensor",
                format!("unknown model '{}'", other),
            )),
        }
    }

    fn init_i2c(&self, config: &crate::config::I2cBusConfig) -> crate::error::Result<()> {
        let state = crate::platform::hardware_drivers::I2cBusState::new(
            config.sda_pin,
            config.scl_pin,
            config.freq_hz,
        )?;
        *self.i2c_state.lock().unwrap_or_else(|e| e.into_inner()) = Some(state);
        Ok(())
    }

    fn i2c_ready(&self) -> bool {
        self.i2c_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    fn i2c_read(&self, addr: u8, register: u8, len: usize) -> crate::error::Result<Vec<u8>> {
        let mut guard = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.read(addr, register, len),
            None => Err(crate::error::Error::config(
                "i2c_read",
                "I2C bus not initialized",
            )),
        }
    }

    fn i2c_write(&self, addr: u8, register: u8, data: &[u8]) -> crate::error::Result<()> {
        let mut guard = self.i2c_state.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(state) => state.write(addr, register, data),
            None => Err(crate::error::Error::config(
                "i2c_write",
                "I2C bus not initialized",
            )),
        }
    }
}
