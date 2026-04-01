//! 记忆与会话抽象。仅定义 trait 与类型，不依赖 platform。
//! Memory and session abstraction. Traits only; no platform dependency.
use crate::bus::PcMsg;
use crate::error::Result;
use serde::{Deserialize, Serialize};

mod context_window;
mod execution_state;
mod long_term;
mod long_term_extraction;
mod maintenance;
mod profile;
mod prompt_context;
mod session_summary_refresh;

pub use context_window::build_context_messages;
pub use execution_state::{
    render_execution_state_block, run_execution_state_refresh, ExecutionState,
    ExecutionStateRefreshContext, ExecutionStateRefreshInput, ExecutionStateRefreshOutcome,
    ExecutionStateStore, ExecutionStatus, EXECUTION_STATE_SYSTEM_PROMPT, REL_PATH_EXECUTION_STATES,
};
pub(crate) use long_term::{
    canonicalize_long_term_memory_entry, govern_long_term_memory_entries,
    merge_long_term_memory_entry, score_long_term_memory_recall,
};
pub use long_term::{
    recall_long_term_memory_block, render_long_term_memory_block, LongTermMemoryDraft,
    LongTermMemoryEntry, LongTermMemoryKind, LongTermMemorySlot, LongTermMemoryStore,
    MAX_LONG_TERM_MEMORY_BLOCK_LEN, MAX_LONG_TERM_MEMORY_CONTENT_LEN, MAX_LONG_TERM_MEMORY_ITEMS,
    MAX_LONG_TERM_MEMORY_KEYWORDS, MAX_LONG_TERM_MEMORY_KEYWORD_LEN, REL_PATH_LONG_TERM_MEMORIES,
};
pub use long_term_extraction::{
    apply_long_term_memory_extraction, build_long_term_memory_extraction_input,
    evaluate_long_term_memory_extraction_turn, mark_long_term_memory_extraction_deferred,
    mark_long_term_memory_extraction_processed, mark_long_term_memory_extraction_requested,
    parse_long_term_memory_extraction_response, persist_long_term_memory_extraction_state,
    run_long_term_memory_refresh, LongTermMemoryExtractionState,
    LongTermMemoryExtractionStateStore, LongTermMemoryExtractionTurnDecision,
    LongTermMemoryExtractionTurnInput, LongTermMemoryRefreshContext, LongTermMemoryRefreshOutcome,
    ParsedLongTermMemoryExtraction, LONG_TERM_MEMORY_EXTRACTION_BATCH,
    LONG_TERM_MEMORY_EXTRACTION_RECENT_N, LONG_TERM_MEMORY_EXTRACTION_SYSTEM_PROMPT,
    REL_PATH_LONG_TERM_EXTRACTION_STATES,
};
pub use maintenance::{
    run_post_reply_memory_maintenance, LongTermMemoryRefreshRequestOutcome,
    PostReplyMemoryMaintenanceContext, PostReplyMemoryMaintenanceInput,
    PostReplyMemoryMaintenanceOutcome,
};
pub use profile::MemoryProfile;
pub(crate) use profile::{
    memory_policy, shared_long_term_governance_policy, ExecutionStatePolicy,
    LongTermExtractionPolicy, LongTermRecallPolicy, SessionSummaryPolicy,
};
pub use prompt_context::{
    load_prompt_memory_context, PromptMemoryContext, PromptMemoryContextParams,
};
pub use session_summary_refresh::{
    fallback_session_summary, run_session_summary_refresh, should_refresh_session_summary,
    SessionSummaryRefreshContext, SessionSummaryRefreshOutcome,
};

/// 单次写入内容最大字节数（与 platform::spiffs 上界一致）。实现应拒绝超长写入。
pub const MAX_MEMORY_CONTENT_LEN: usize = 256 * 1024;
/// SOUL/USER 单次写入上限（实现应拒绝超长）。
pub const MAX_SOUL_USER_LEN: usize = 32 * 1024;

/// 单条会话消息最大长度（role + content 序列化后）。实现应拒绝超长单条。
pub const MAX_SESSION_MESSAGE_LEN: usize = 4 * 1024;
/// 单会话最大条数（ring 上界）。超过时实现应淘汰最旧再追加。
pub const MAX_SESSION_ENTRIES: usize = 128;

/// 相对路径（实现需拼接 SPIFFS_BASE）：MEMORY 文件。
pub const REL_PATH_MEMORY: &str = "memory/MEMORY.md";
/// 相对路径：SOUL 配置。
pub const REL_PATH_SOUL: &str = "config/SOUL.md";
/// 相对路径：USER 配置。
pub const REL_PATH_USER: &str = "config/USER.md";
/// 相对路径：每日笔记目录。
pub const REL_PATH_DAILY_DIR: &str = "memory/daily";
/// 相对路径：会话文件所在目录（文件名为 {chat_id}.jsonl）。短路径以满足 ESP-IDF VFS 路径长度上限（约 64 字符）。
pub const REL_PATH_SESSIONS_DIR: &str = "s";
/// 相对路径：HEARTBEAT 待办文件（与 memory 目录约定一致）。
pub const REL_PATH_HEARTBEAT: &str = "memory/HEARTBEAT.md";
/// 相对路径：待重试消息（低内存且队列满时落盘，单条 PcMsg JSON）。
pub const REL_PATH_PENDING_RETRY: &str = "memory/pending_retry.json";
/// 相对路径：重要消息偏移（截断时优先保留）；单 chat 单 offset。
pub const REL_PATH_IMPORTANT_MESSAGE: &str = "memory/important_message.json";
/// 相对路径：会话摘要（单文件 JSON，chat_id -> { summary, last_summary_at_count }）。
pub const REL_PATH_SESSION_SUMMARIES: &str = "memory/session_summaries.json";

/// 会话摘要存储。由 agent 程序性摘要写入；build_context 将 get 到的摘要注入 messages 首条。实现方按 SESSION_SUMMARY_MAX_LEN 截断。
pub trait SessionSummaryStore: Send + Sync {
    fn get(&self, chat_id: &str) -> Result<Option<String>>;
    fn set(&self, chat_id: &str, summary: &str) -> Result<()>;
    /// 带 message_count 的 set；实现方同时记录当时的会话消息条数。
    fn set_with_count(&self, chat_id: &str, summary: &str, _message_count: usize) -> Result<()> {
        self.set(chat_id, summary)
    }
    /// 获取摘要及其对应的 message_count；返回 (summary, last_message_count)。
    fn get_with_count(&self, chat_id: &str) -> Result<Option<(String, usize)>> {
        self.get(chat_id).map(|opt| opt.map(|s| (s, 0)))
    }
}

/// 重要消息存储。offset_from_end=1 表示最后一条 user 消息。供 build_context 截断时优先保留。
pub trait ImportantMessageStore: Send + Sync {
    fn set_important_offset_from_end(&self, chat_id: &str, offset_from_end: u32) -> Result<()>;
    fn get_important_offset(&self, chat_id: &str) -> Result<Option<u32>>;
    fn clear_important(&self, chat_id: &str) -> Result<()>;
}

/// 到点提醒存储。add 写入 (channel, chat_id, at_unix_secs, context)；pop_due(now) 移除并返回一条 at<=now 的条目。
/// 条目数/context 长度上界见 constants::REMIND_AT_*。
pub trait RemindAtStore: Send + Sync {
    fn add(&self, channel: &str, chat_id: &str, at_unix_secs: u64, context: &str) -> Result<()>;
    /// 移除并返回一条 at <= now 的条目（任选其一）；无到点项返回 Ok(None)。
    fn pop_due(&self, now_unix_secs: u64) -> Result<Option<(String, String, String)>>;
    /// 查询当前会话未到点提醒，按 at 升序返回，limit 由调用方控制。
    fn list_upcoming(
        &self,
        channel: &str,
        chat_id: &str,
        now_unix_secs: u64,
        limit: usize,
    ) -> Result<Vec<(u64, String)>>;
}

/// 情绪信号存储。本轮模型输出带 [SIGNAL:comfort] 时 set，下一轮 build_context 时 get_then_clear 注入 system 后清除。
pub trait EmotionSignalStore: Send + Sync {
    fn set(&self, chat_id: &str, signal: &str) -> Result<()>;
    fn get_then_clear(&self, chat_id: &str) -> Result<Option<String>>;
}

/// 内存实现的 EmotionSignalStore；无持久化。
pub struct MemoryEmotionSignalStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl MemoryEmotionSignalStore {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(std::collections::HashMap::new()))
    }
}

impl Default for MemoryEmotionSignalStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EmotionSignalStore for MemoryEmotionSignalStore {
    fn set(&self, chat_id: &str, signal: &str) -> Result<()> {
        self.0
            .lock()
            .map_err(|e| crate::error::Error::Other {
                source: Box::new(std::io::Error::other(e.to_string())),
                stage: "emotion_signal_set",
            })?
            .insert(chat_id.to_string(), signal.to_string());
        Ok(())
    }

    fn get_then_clear(&self, chat_id: &str) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| crate::error::Error::Other {
                source: Box::new(std::io::Error::other(e.to_string())),
                stage: "emotion_signal_get",
            })?
            .remove(chat_id))
    }
}

/// 待重试消息存储。实现由 platform 注入（如 SpiffsPendingRetryStore）。低内存且入队满时落盘，启动或循环前取回重试。
pub trait PendingRetryStore: Send + Sync {
    fn save_pending_retry(&self, msg: &PcMsg) -> Result<()>;
    fn load_pending_retry(&self) -> Result<Option<PcMsg>>;
    fn clear_pending_retry(&self) -> Result<()>;
}

/// 长期记忆与每日笔记存储。实现由 platform 注入（如 SpiffsMemoryStore）。
pub trait MemoryStore: Send + Sync {
    fn get_memory(&self) -> Result<String>;
    fn set_memory(&self, content: &str) -> Result<()>;
    fn get_soul(&self) -> Result<String>;
    /// 写入 SOUL 配置（config/SOUL.md）。实现应拒绝 content.len() > MAX_SOUL_USER_LEN。
    fn set_soul(&self, content: &str) -> Result<()>;
    fn get_user(&self) -> Result<String>;
    /// 写入 USER 配置（config/USER.md）。实现应拒绝 content.len() > MAX_SOUL_USER_LEN。
    fn set_user(&self, content: &str) -> Result<()>;
    /// 最近 N 条每日笔记的文件名（如 YYYY-MM-DD.md），按名称降序（最新在前）。
    fn list_daily_note_names(&self, recent_n: usize) -> Result<Vec<String>>;
    fn get_daily_note(&self, name: &str) -> Result<String>;
    fn write_daily_note(&self, name: &str, content: &str) -> Result<()>;
}

/// 会话单条消息，JSONL 行格式（role + content）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
}

/// 按 chat_id 的会话存储。实现由 platform 注入（如 SpiffsSessionStore）。
pub trait SessionStore: Send + Sync {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()>;
    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>>;
    /// 返回当前会话消息条数（不含可选头注释）。默认实现回退到 `load_recent(MAX_SESSION_ENTRIES)`。
    /// Implementations should override with an O(file-scan) fast path when possible.
    fn message_count(&self, chat_id: &str) -> Result<usize> {
        self.load_recent(chat_id, MAX_SESSION_ENTRIES)
            .map(|v| v.len())
    }
    fn clear(&self, chat_id: &str) -> Result<()>;
    /// 列举所有会话的 chat_id（如 sessions 目录下 *.jsonl 文件名去掉后缀）。用于 GET /api/sessions。
    fn list_chat_ids(&self) -> Result<Vec<String>>;
    /// 清理超过 max_age_secs 未修改的会话文件，返回清理数量。默认 no-op。
    fn gc_stale(&self, _max_age_secs: u64) -> Result<usize> {
        Ok(0)
    }
    /// 删除指定 chat_id 的会话文件。默认调用 clear。
    fn delete(&self, chat_id: &str) -> Result<()> {
        self.clear(chat_id)
    }
}

/// 系统提示聚合：SOUL + USER + MEMORY + 近期每日笔记，总长度不超过 max_len。
/// 截断策略：从「每日笔记」部分从旧到新丢弃，再若仍超则从尾部截断整体。
/// 纯函数，供 agent::context 使用；可 host 单测。
pub fn build_system_prompt(
    soul: &str,
    user: &str,
    memory: &str,
    daily_notes: &[String],
    max_len: usize,
) -> String {
    const SEP: &str = "\n\n";
    let mut out = String::with_capacity(max_len.min(soul.len() + user.len() + memory.len() + 512));
    out.push_str(soul.trim());
    out.push_str(SEP);
    out.push_str(user.trim());
    out.push_str(SEP);
    out.push_str(memory.trim());
    for note in daily_notes {
        if out.len() >= max_len {
            break;
        }
        out.push_str(SEP);
        let note = note.trim();
        let remaining = max_len.saturating_sub(out.len());
        if note.len() <= remaining {
            out.push_str(note);
        } else {
            let mut end = remaining;
            while end > 0 && !note.is_char_boundary(end) {
                end -= 1;
            }
            if end > 0 {
                out.push_str(&note[..end]);
            }
            break;
        }
    }
    if out.len() > max_len {
        out.truncate(max_len);
    }
    out
}

/// 单次 remind tick：检查到点提醒并注入 inbound。
/// 由 bg_timer 每 60s 调用一次。
pub(crate) fn remind_tick(
    remind_store: &dyn RemindAtStore,
    inbound_tx: &crate::bus::SystemInboundTx,
    resolve_locale: &std::sync::Arc<dyn Fn() -> crate::i18n::Locale + Send + Sync>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    while let Ok(Some((channel, chat_id, context))) = remind_store.pop_due(now) {
        let loc = resolve_locale();
        let prefix = crate::i18n::tr(crate::i18n::Message::RemindPrefix, loc);
        let content = format!("{}{}", prefix, context);
        if let Ok(msg) = PcMsg::new_inbound_with_ingress(
            channel,
            chat_id,
            content,
            false,
            crate::bus::IngressKind::System,
        ) {
            let _ = inbound_tx.send(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_system_prompt;

    #[test]
    fn build_system_prompt_respects_max_len() {
        let soul = "Soul";
        let user = "User";
        let memory = "Memory";
        let notes = vec!["Note1".to_string(), "Note2".to_string()];
        let out = build_system_prompt(soul, user, memory, &notes, 20);
        assert!(out.len() <= 20);
    }

    #[test]
    fn build_system_prompt_order() {
        let out = build_system_prompt("A", "B", "C", &[], 100);
        assert!(out.starts_with("A"));
        assert!(out.contains("B"));
        assert!(out.contains("C"));
    }
}
