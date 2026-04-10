use super::*;

pub(super) type PreparedTurn = PreparedWorkerConversation;

#[inline(never)]
pub(super) fn prepare_turn<'a>(
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &'a crate::bus::PcMsg,
    request_plan: &AgentRequestPlan<'a>,
    config: &AgentLoopConfig,
    tool_ctx: &mut HttpClientToolContext<'_>,
    latency: &mut WorkerLatency,
) -> Result<PreparedTurn> {
    let mut session = Box::new(super::worker_context_stages::WorkerPrepareSession::new(
        Instant::now(),
    ));
    super::worker_context_stages::compute_prepare_runtime(&mut session, msg, config, request_plan);
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
        request_plan,
        session,
        latency,
    )
}
