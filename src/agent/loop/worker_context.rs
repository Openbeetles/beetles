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
    super::turn_prepare::prepare_turn(worker_llm, msg, request_plan, config, tool_ctx, latency)
}
