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
        let out = PcMsg {
            channel: msg.channel.clone(),
            chat_id: msg.chat_id.clone(),
            content: finalized.reply_content.clone(),
            req_id: Some(msg.req_id.as_deref().unwrap_or_default().to_owned()),
            ingress: IngressKind::User,
            enqueue_ts_ms: super::now_unix_ms(),
            is_group: false,
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
