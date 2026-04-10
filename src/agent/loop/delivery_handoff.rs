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
) -> DeliveryHandoff {
    if finalized.skip_delivery {
        return DeliveryHandoff::default();
    }

    let outbound_start = Instant::now();
    let delivered = if finalized.reply_already_delivered {
        crate::platform::task_wdt::feed_current_task();
        true
    } else if !finalized.streamed {
        let out = match PcMsg::new_outbound_reply_to(msg, finalized.reply_content.clone()) {
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
