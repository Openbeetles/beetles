use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreReplyGovernanceMode {
    LinuxFull,
    EspCompact,
}

impl PreReplyGovernanceMode {
    pub(super) fn for_turn(
        memory_system_kind: crate::memory::MemorySystemKind,
        ingress: IngressKind,
    ) -> Option<Self> {
        if ingress != IngressKind::User {
            return None;
        }
        Some(match memory_system_kind {
            crate::memory::MemorySystemKind::LinuxFull => Self::LinuxFull,
            crate::memory::MemorySystemKind::EspCompact => Self::EspCompact,
        })
    }

    pub(super) fn allow_sync_disclosure_adjudication(self) -> bool {
        matches!(self, Self::LinuxFull)
    }

    pub(super) fn allow_dynamic_persona_adjudication(self) -> bool {
        matches!(self, Self::LinuxFull)
    }

    pub(super) fn allow_sync_relationship_constitution(self) -> bool {
        matches!(self, Self::LinuxFull)
    }
}

#[inline(never)]
pub(super) fn prepare_turn(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    has_tools: bool,
    request_semantics: crate::agent::request_semantics::RequestSemantics,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> Result<PreparedWorkerConversation> {
    let mut session = Box::new(super::worker_context_stages::WorkerPrepareSession::new(
        Instant::now(),
    ));
    super::worker_context_stages::compute_prepare_runtime(&mut session, msg, config, has_tools);
    super::worker_context_stages::run_prepare_mental_privacy(
        &mut session,
        worker_llm,
        msg,
        config,
        tool_ctx,
    );
    super::worker_context_stages::load_prepare_prompt_memory(&mut session, msg, config);
    super::worker_context_stages::enrich_prepare_governance(
        &mut session,
        worker_llm,
        msg,
        config,
        tool_ctx,
    );
    super::worker_context_stages::finalize_prepare_context(
        msg,
        config,
        request_semantics,
        session,
        latency,
    )
}
