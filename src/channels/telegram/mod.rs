//! Telegram 通道：入站 long poll 推 bus，出站经 MessageSink 队列后由 main 用 HTTP 发送。

mod poll;
pub(crate) mod send;

pub use poll::{poll_telegram_once, run_telegram_poll_loop, TelegramCommandCtx};
pub use send::{
    check_connectivity, edit_message_text, flush_telegram_sends, get_bot_username,
    run_telegram_sender_loop, send_and_get_id as tg_send_and_get_id, send_chat_action,
};
