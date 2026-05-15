use super::*;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct DeliveryHandoff {
    pub(super) delivered: bool,
    pub(super) outbound_enqueue_ms: u128,
    pub(super) reply_handoff_ms: u128,
}

pub(super) fn deliver_turn(
    outbound_tx: &OutboundTx,
    msg: &PcMsg,
    finalized: &super::reply_finalize::FinalizedTurn,
    config: &AgentLoopConfig,
) -> DeliveryHandoff {
    if finalized.skip_delivery {
        return DeliveryHandoff::default();
    }

    let outbound_start = Instant::now();
    let delivered = if finalized.reply_already_delivered {
        crate::platform::task_wdt::feed_current_task();
        true
    } else if crate::chat_stream::is_configure_ui_stream_turn(msg) {
        metrics::record_message_out();
        crate::platform::task_wdt::feed_current_task();
        true
    } else if !finalized.streamed {
        let out_result = match finalized.artifact_bundle.as_ref() {
            Some(bundle) => PcMsg::new_outbound_reply_to_with_body_projection(
                msg,
                bundle.current_chat_primary_body.clone(),
                finalized.reply.visible_text.clone(),
            ),
            None => PcMsg::new_outbound_reply_to(msg, finalized.reply.visible_text.clone()),
        };
        let out = match out_result {
            Ok(out) => out,
            Err(error) => {
                metrics::record_error_by_stage(error.metrics_stage());
                log::error!(
                    "[agent_delivery] failed to build outbound reply channel={} chat_id={}: {}",
                    msg.channel,
                    msg.chat_id,
                    error
                );
                crate::platform::task_wdt::feed_current_task();
                return DeliveryHandoff::default();
            }
        };
        crate::platform::task_wdt::feed_current_task();
        super::try_send_outbound(outbound_tx, out, "reply")
    } else {
        metrics::record_message_out();
        crate::platform::task_wdt::feed_current_task();
        true
    };
    let outbound_enqueue_ms = outbound_start.elapsed().as_millis();
    if delivered {
        let req_id = msg.req_id.as_deref().unwrap_or("reply");
        crate::agent::delivery::send_terminal_reaction_if_enabled(
            msg,
            req_id,
            outbound_tx,
            config.channel_capability_registry.get(msg.channel.as_ref()),
            true,
        );
    }
    let reply_handoff_ms = if delivered {
        finalized.msg_start.elapsed().as_millis()
    } else {
        0
    };
    DeliveryHandoff {
        delivered,
        outbound_enqueue_ms,
        reply_handoff_ms,
    }
}
