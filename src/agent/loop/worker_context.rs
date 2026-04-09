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
pub(super) fn prepare_worker_conversation<'a>(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &'a crate::bus::PcMsg,
    request_plan: &AgentRequestPlan<'a>,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> Result<PreparedWorkerConversation> {
    match config.memory_system_kind {
        crate::memory::MemorySystemKind::LinuxFull
        | crate::memory::MemorySystemKind::EspCompact => prepare_worker_conversation_impl(
            worker_llm,
            msg,
            request_plan,
            config,
            tool_ctx,
            latency,
        ),
    }
}

#[inline(never)]
fn prepare_worker_conversation_impl<'a>(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &'a crate::bus::PcMsg,
    request_plan: &AgentRequestPlan<'a>,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> Result<PreparedWorkerConversation> {
    let context_start = Instant::now();
    let runtime_stage =
        super::worker_context_stages::compute_prepare_runtime(msg, config, request_plan);
    let primer = super::worker_context_stages::run_prepare_mental_privacy(
        worker_llm,
        msg,
        config,
        tool_ctx,
        &runtime_stage,
    );
    let mut prompt_stage =
        super::worker_context_stages::load_prepare_prompt_memory(msg, config, &runtime_stage);
    let governance_stage = super::worker_context_stages::enrich_prepare_governance(
        worker_llm,
        msg,
        config,
        tool_ctx,
        &runtime_stage,
        primer,
        &mut prompt_stage,
    );
    super::worker_context_stages::finalize_prepare_context(
        msg,
        config,
        request_plan,
        runtime_stage,
        prompt_stage,
        governance_stage,
        latency,
        context_start,
    )
}
