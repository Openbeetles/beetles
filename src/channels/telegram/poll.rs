//! Telegram 入站 long poll：getUpdates，解析消息入队，命令处理。

use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::bus::{InboundTx, OutboundTx, PcMsg, MAX_CONTENT_LEN};
use crate::channels::ChannelHttpClient;
use crate::error::{Error, Result};
use crate::i18n::{tr, Locale as UiLocale, Message as UiMessage};
use crate::memory::{PendingRetryStore, SessionStore};

use super::send::set_message_reaction;

const TAG_POLL: &str = "telegram";
/// Telegram 控制命令（/activation、/session clear、/status）执行所需的上下文，由 main 传入轮询线程。
/// NOTE: 该结构体持有多种回调与共享状态，短期保留 type_complexity 以维持调用侧显式依赖注入。
#[allow(clippy::type_complexity)]
pub struct TelegramCommandCtx {
    pub outbound_tx: OutboundTx,
    pub session_store: Arc<dyn SessionStore + Send + Sync>,
    pub inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    pub outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    pub set_group_activation: Box<dyn Fn(&str) -> Result<()> + Send>,
}

fn map_stage(e: Error, stage: &'static str) -> Error {
    match e {
        Error::Http { status_code, .. } => Error::Http { status_code, stage },
        other => Error::Other {
            source: Box::new(other),
            stage,
        },
    }
}

#[derive(serde::Deserialize)]
struct TelegramUpdates {
    result: Option<Vec<TelegramUpdate>>,
}

#[derive(serde::Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessage>,
}

#[derive(serde::Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    #[serde(default)]
    message_id: i64,
    text: Option<String>,
    entities: Option<Vec<MessageEntity>>,
}

#[derive(serde::Deserialize)]
struct TelegramChat {
    id: i64,
    #[serde(rename = "type")]
    type_: Option<String>,
}

#[derive(serde::Deserialize)]
struct MessageEntity {
    #[serde(rename = "type")]
    type_: String,
    offset: Option<i32>,
    length: Option<i32>,
}

fn message_mentions_bot(
    text: &str,
    entities: Option<&[MessageEntity]>,
    bot_username: &str,
) -> bool {
    if bot_username.is_empty() {
        return false;
    }
    let mention = format!("@{}", bot_username);
    if text.contains(&mention) {
        return true;
    }
    let Some(entities) = entities else {
        return false;
    };
    for e in entities {
        if e.type_ != "mention" {
            continue;
        }
        let (off, len) = match (e.offset, e.length) {
            (Some(o), Some(l)) if o >= 0 && l > 0 => (o as usize, l as usize),
            _ => continue,
        };
        if let Some(slice) = text.get(off..off.saturating_add(len)) {
            if slice.eq_ignore_ascii_case(&mention) {
                return true;
            }
        }
    }
    false
}

const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// 轮询一次 getUpdates，解析消息并推入 inbound_tx；失败返回 Err 带 stage，调用方退避。
/// NOTE: 保留参数显式传递，避免把状态收敛到全局可变对象；待后续仅提取参数对象时再移除 allow。
#[allow(clippy::too_many_arguments)]
pub fn poll_telegram_once<H: ChannelHttpClient>(
    http: &mut H,
    token: &str,
    offset: Option<i64>,
    inbound_tx: &InboundTx,
    pending_retry: &dyn PendingRetryStore,
    allowed_chat_ids: &[String],
    group_activation: &str,
    bot_username: Option<&str>,
    cmd_ctx: Option<&TelegramCommandCtx>,
    resolve_locale: &std::sync::Arc<dyn Fn() -> UiLocale + Send + Sync>,
) -> Result<Option<i64>> {
    let loc = resolve_locale();
    let url = format!(
        "{}{}/getUpdates?timeout=5{}",
        TELEGRAM_API_BASE,
        token,
        offset.map(|o| format!("&offset={}", o)).unwrap_or_default()
    );
    let (status, body) = http
        .http_get(&url)
        .map_err(|e| map_stage(e, "telegram_poll"))?;
    if status >= 400 {
        return Err(Error::Http {
            status_code: status,
            stage: "telegram_poll",
        });
    }
    let updates: TelegramUpdates =
        serde_json::from_slice(body.as_ref()).map_err(|e| Error::Other {
            source: Box::new(e),
            stage: "telegram_parse",
        })?;
    let mut next_offset = offset;
    for u in updates.result.unwrap_or_default() {
        next_offset = Some(u.update_id + 1);
        if let Some(msg) = u.message {
            let chat_id = msg.chat.id.to_string();
            if allowed_chat_ids.is_empty() {
                log::warn!(
                    "[{}] rejected chat_id={} (allowlist empty). {}",
                    TAG_POLL,
                    chat_id,
                    tr(UiMessage::BindHintEmpty, loc)
                );
                continue;
            }
            if !allowed_chat_ids.iter().any(|id| id == &chat_id) {
                log::warn!(
                    "[{}] rejected chat_id={} (not in allowlist). {} Example: ...{}",
                    TAG_POLL,
                    chat_id,
                    tr(UiMessage::BindHintNotInList, loc),
                    chat_id
                );
                continue;
            }
            let text = msg.text.unwrap_or_default();
            if text.is_empty() {
                continue;
            }
            let is_group = msg
                .chat
                .type_
                .as_deref()
                .is_some_and(|t| t == "group" || t == "supergroup");
            if is_group && group_activation == "mention" {
                let mentioned = message_mentions_bot(
                    &text,
                    msg.entities.as_deref(),
                    bot_username.unwrap_or(""),
                );
                if !mentioned {
                    continue;
                }
            }
            if let Some(ctx) = cmd_ctx {
                let loc_cmd = resolve_locale();
                if text.starts_with('/') {
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    let handled = match parts.as_slice() {
                        ["/activation", "mention"] => {
                            if let Err(e) = (ctx.set_group_activation)("mention") {
                                log::warn!("[{}] set_group_activation: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgActivationMention, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/activation", "always"] => {
                            if let Err(e) = (ctx.set_group_activation)("always") {
                                log::warn!("[{}] set_group_activation: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgActivationAlways, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/session", "clear"] => {
                            if let Err(e) = ctx.session_store.clear(&chat_id) {
                                log::warn!("[{}] session clear: {}", TAG_POLL, e);
                            }
                            let _ = PcMsg::new(
                                "telegram",
                                &chat_id,
                                tr(UiMessage::TgSessionCleared, loc_cmd),
                            )
                            .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        ["/status"] => {
                            let inc = ctx.inbound_depth.load(Ordering::Relaxed);
                            let out = ctx.outbound_depth.load(Ordering::Relaxed);
                            let status = tr(
                                UiMessage::TelegramStatus {
                                    wifi_connected: crate::state::wifi_sta_connected(),
                                    inbound: inc,
                                    outbound: out,
                                },
                                loc_cmd,
                            );
                            let _ = PcMsg::new("telegram", &chat_id, status)
                                .map(|m| ctx.outbound_tx.send(m));
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        continue;
                    }
                }
            }
            let content = if text.len() > MAX_CONTENT_LEN {
                text.chars().take(MAX_CONTENT_LEN).collect::<String>()
            } else {
                text
            };
            let _ = set_message_reaction(http, token, &chat_id, msg.message_id, "👍");
            let pc = PcMsg {
                channel: Arc::from("telegram"),
                chat_id: Arc::from(chat_id.as_str()),
                content,
                req_id: None,
                ingress: crate::bus::IngressKind::User,
                enqueue_ts_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis().min(u64::MAX as u128) as u64)
                    .unwrap_or(0),
                is_group,
            };
            let mut enqueued = false;
            for _ in 0..3 {
                match inbound_tx.try_send(pc.clone()) {
                    Ok(()) => {
                        enqueued = true;
                        break;
                    }
                    Err(std::sync::mpsc::TrySendError::Full(_)) => {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }
                    Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                        log::warn!(
                            "[{}] inbound_tx closed while enqueueing telegram msg",
                            TAG_POLL
                        );
                        break;
                    }
                }
            }
            if !enqueued {
                log::warn!(
                    "[{}] inbound queue full, drop telegram msg chat_id={}",
                    TAG_POLL,
                    chat_id
                );
                let _ = pending_retry.save_pending_retry(&pc);
            }
        }
    }
    Ok(next_offset)
}

/// 启动 Telegram 长轮询循环（阻塞，应在独立线程调用）。
/// 内部通过 create_http 工厂创建 HTTP 客户端，执行轮询循环。
#[allow(clippy::too_many_arguments)]
pub fn run_telegram_poll_loop<H, F>(
    token: String,
    allowed_chat_ids: Vec<String>,
    group_activation: String,
    inbound_tx: InboundTx,
    pending_retry: Arc<dyn PendingRetryStore + Send + Sync>,
    outbound_tx: OutboundTx,
    session_store: Arc<dyn SessionStore + Send + Sync>,
    inbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    outbound_depth: Arc<std::sync::atomic::AtomicUsize>,
    config_store: Arc<dyn crate::platform::ConfigStore>,
    resolve_locale: Arc<dyn Fn() -> UiLocale + Send + Sync>,
    mut create_http: F,
) where
    H: ChannelHttpClient,
    F: FnMut() -> Result<H>,
{
    const TAG_TG: &str = "telegram_poll";

    #[cfg(any(target_arch = "xtensa", target_arch = "riscv32"))]
    crate::platform::task_wdt::register_current_task_to_task_wdt();

    let cmd_ctx = TelegramCommandCtx {
        outbound_tx,
        session_store,
        inbound_depth,
        outbound_depth,
        set_group_activation: Box::new(move |v| {
            crate::config::write_tg_group_activation(config_store.as_ref(), v)
        }),
    };

    let mut http = match create_http() {
        Ok(h) => h,
        Err(e) => {
            log::warn!("[{}] create_http failed: {}", TAG_TG, e);
            return;
        }
    };

    let bot_username = match super::send::get_bot_username(&mut http, &token) {
        Ok(Some(u)) => Some(u),
        _ => None,
    };

    let mut offset: Option<i64> = None;
    const POLL_INTERVAL_SECS: u64 = 5;
    const BACKOFF_SECS: u64 = 30;

    loop {
        match poll_telegram_once(
            &mut http,
            &token,
            offset,
            &inbound_tx,
            pending_retry.as_ref(),
            &allowed_chat_ids,
            &group_activation,
            bot_username.as_deref(),
            Some(&cmd_ctx),
            &resolve_locale,
        ) {
            Ok(next) => offset = next,
            Err(e) => {
                log::warn!("[{}] poll failed: {}, backoff {}s", TAG_TG, e, BACKOFF_SECS);
                ChannelHttpClient::reset_connection_for_retry(&mut http);
                std::thread::sleep(std::time::Duration::from_secs(BACKOFF_SECS));
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
    }
}
