use super::*;

pub(super) struct ExecutedTurn {
    pub(super) outcome: WorkerOutcome,
    pub(super) telemetry: WorkerRunTelemetry,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_turn(
    http: &mut dyn PlatformHttpClient,
    worker_llm: &(dyn LlmClient + Send + Sync),
    msg: &crate::bus::PcMsg,
    outbound_tx: &OutboundTx,
    req_id: &str,
    registry: &crate::tools::ToolRegistry,
    config: &AgentLoopConfig,
    tool_call_repeat: &mut HashMap<u64, u8>,
    loc: UiLocale,
) -> Result<ExecutedTurn> {
    let (outcome, telemetry) = super::run_worker_path(
        http,
        worker_llm,
        msg,
        outbound_tx,
        req_id,
        registry,
        config,
        tool_call_repeat,
        loc,
    )?;
    Ok(ExecutedTurn { outcome, telemetry })
}
