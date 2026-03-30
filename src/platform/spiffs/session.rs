//! SPIFFS 实现的 SessionStore。会话文件为 {sessions_dir}/{short_id}.jsonl，有界 ring。
//! 短 id：chat_id 若不超过 MAX_CHAT_ID_FILENAME_LEN 则直接用，否则用 8 字符哈希，文件首行存 "# chat_id: <真实 id>"。

use crate::error::{Error, Result};
use crate::memory::{
    SessionMessage, SessionStore, MAX_SESSION_ENTRIES, MAX_SESSION_MESSAGE_LEN,
    REL_PATH_SESSIONS_DIR,
};
use serde_json;
use std::path::PathBuf;

use crate::platform::state_root::state_mount_path;

use super::{list_dir, read_file, write_file};

const TAG: &str = "platform::spiffs::session";

/// 文件名中 chat_id 部分最大长度（不含 .jsonl），超出则用 hash 短名，满足 ESP-IDF 路径/文件名限制。
const MAX_CHAT_ID_FILENAME_LEN: usize = 20;
const SESSION_FILE_EXT: &str = ".jsonl";
const CHAT_ID_HEADER_PREFIX: &str = "# chat_id: ";

fn fnv1a_hash(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 返回 (路径, 是否在文件中写入 chat_id 首行)。长 chat_id 用 8 位 hex 哈希作文件名，首行存真实 id。
fn session_path(chat_id: &str) -> Result<(PathBuf, bool)> {
    if chat_id.is_empty() {
        return Err(Error::config("session_path", "chat_id empty"));
    }
    if !chat_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':')
    {
        return Err(Error::config(
            "session_path",
            "chat_id contains invalid chars",
        ));
    }
    let mut p = state_mount_path();
    p.push(REL_PATH_SESSIONS_DIR);
    let (filename, write_header) = if chat_id.len() <= MAX_CHAT_ID_FILENAME_LEN {
        (format!("{}{}", chat_id, SESSION_FILE_EXT), false)
    } else {
        let h = fnv1a_hash(chat_id);
        (format!("{:08x}{}", h, SESSION_FILE_EXT), true)
    };
    p.push(&filename);
    if p.as_os_str().len() > 56 {
        return Err(Error::config(
            "session_path",
            format!("path too long ({})", p.as_os_str().len()),
        ));
    }
    Ok((p, write_header))
}

fn parse_jsonl_line(line: &str) -> Option<SessionMessage> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    match serde_json::from_str::<SessionMessage>(line) {
        Ok(m) => Some(m),
        Err(e_strict) => {
            let mut iter = serde_json::Deserializer::from_str(line).into_iter::<SessionMessage>();
            match iter.next() {
                Some(Ok(m)) => {
                    log::warn!(
                        "[{}] strict parse failed ({}); using first message JSON only",
                        TAG,
                        e_strict
                    );
                    Some(m)
                }
                Some(Err(e)) => {
                    log::warn!("[{}] skip bad line: {}", TAG, e);
                    None
                }
                None => None,
            }
        }
    }
}

/// 从文件首行解析 "# chat_id: <id>"，非该格式返回 None。
fn parse_chat_id_header(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(stripped) = line.strip_prefix(CHAT_ID_HEADER_PREFIX) {
        let id = stripped.trim();
        if !id.is_empty() {
            return Some(id.to_string());
        }
    }
    None
}

/// 统计 JSONL 中会话消息行数（与 `append` 慢路径解析规则一致：可选首行 `# chat_id:`，其余为 `serde_json` 行）。
/// 不反序列化 JSON，仅以 `trim` 后以 `{` 开头作为消息行，与 `SessionMessage` 序列化形态一致。
fn count_session_message_lines(buf: &[u8]) -> usize {
    let mut first = true;
    let mut n = 0usize;
    for line in buf.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(s) = std::str::from_utf8(line) {
            let t = s.trim();
            if t.is_empty() {
                continue;
            }
            if first && parse_chat_id_header(t).is_some() {
                first = false;
                continue;
            }
            first = false;
            if t.starts_with('{') {
                n += 1;
            }
        }
    }
    n
}

/// 列举 chat_id 数量上界（与 MAX_SESSION_ENTRIES 同量级）。
const MAX_LIST_CHAT_IDS: usize = 128;

/// SessionStore 的 SPIFFS 实现；单会话最多 MAX_SESSION_ENTRIES 条，超限淘汰最旧。
pub struct SpiffsSessionStore;

impl Default for SpiffsSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SpiffsSessionStore {
    pub fn new() -> Self {
        SpiffsSessionStore
    }
}

impl SessionStore for SpiffsSessionStore {
    fn append(&self, chat_id: &str, role: &str, content: &str) -> Result<()> {
        let msg = SessionMessage {
            role: role.to_string(),
            content: content.to_string(),
        };
        let line = serde_json::to_string(&msg)
            .map_err(|e| Error::config("session_append", e.to_string()))?;
        if line.len() > MAX_SESSION_MESSAGE_LEN {
            return Err(Error::config(
                "session_append",
                format!(
                    "message serialized len {} exceeds {}",
                    line.len(),
                    MAX_SESSION_MESSAGE_LEN
                ),
            ));
        }

        let (path, write_header) = session_path(chat_id)?;

        // Fast path: 未满环时直接追加一行，避免整文件 JSON 解析/重序列化。
        let existing_buf = read_file(&path).unwrap_or_default();
        let msg_count = count_session_message_lines(&existing_buf);
        if msg_count < MAX_SESSION_ENTRIES {
            let out_cap = existing_buf.len() + line.len() + 2;
            let mut out = if out_cap >= super::PSRAM_FILE_THRESHOLD {
                super::psram_vec_with_capacity(out_cap)
            } else {
                Vec::with_capacity(out_cap)
            };
            if existing_buf.is_empty() {
                if write_header {
                    out.extend_from_slice(CHAT_ID_HEADER_PREFIX.as_bytes());
                    out.extend_from_slice(chat_id.as_bytes());
                    out.push(b'\n');
                }
                out.extend_from_slice(line.as_bytes());
                return write_file(&path, &out);
            }
            out.extend_from_slice(&existing_buf);
            if !existing_buf.ends_with(b"\n") {
                out.push(b'\n');
            }
            out.extend_from_slice(line.as_bytes());
            return write_file(&path, &out);
        }

        // Slow path: 已满，需解析、淘汰最旧、整文件重写。
        let mut messages: Vec<SessionMessage> = Vec::with_capacity(MAX_SESSION_ENTRIES);
        let mut first = true;
        for raw_line in existing_buf.split(|&b| b == b'\n') {
            if raw_line.is_empty() {
                continue;
            }
            if let Ok(s) = std::str::from_utf8(raw_line) {
                if first && parse_chat_id_header(s).is_some() {
                    first = false;
                    continue;
                }
                first = false;
                if let Some(m) = parse_jsonl_line(s) {
                    messages.push(m);
                }
            }
        }

        messages.push(msg);
        if messages.len() > MAX_SESSION_ENTRIES {
            messages.drain(0..(messages.len() - MAX_SESSION_ENTRIES));
        }

        let cap = messages
            .len()
            .saturating_mul(MAX_SESSION_MESSAGE_LEN.saturating_add(1))
            .saturating_add(if write_header {
                CHAT_ID_HEADER_PREFIX.len() + chat_id.len() + 2
            } else {
                0
            });
        let mut body = String::with_capacity(cap);
        if write_header {
            body.push_str(CHAT_ID_HEADER_PREFIX);
            body.push_str(chat_id);
            body.push('\n');
        }
        for (i, m) in messages.iter().enumerate() {
            if i > 0 {
                body.push('\n');
            }
            let json_line = serde_json::to_string(m).unwrap_or_default();
            body.push_str(&json_line);
        }
        write_file(&path, body.as_bytes())
    }

    fn load_recent(&self, chat_id: &str, n: usize) -> Result<Vec<SessionMessage>> {
        let (path, _) = session_path(chat_id)?;
        let cap = n.min(MAX_SESSION_ENTRIES);
        let mut all: Vec<SessionMessage> = Vec::with_capacity(MAX_SESSION_ENTRIES);
        if let Ok(buf) = read_file(&path) {
            for raw_line in buf.split(|&b| b == b'\n') {
                if raw_line.is_empty() {
                    continue;
                }
                if let Ok(s) = std::str::from_utf8(raw_line) {
                    if parse_chat_id_header(s).is_some() {
                        continue;
                    }
                    if let Some(m) = parse_jsonl_line(s) {
                        all.push(m);
                    }
                }
            }
        }
        let skip = all.len().saturating_sub(cap);
        Ok(all.split_off(skip))
    }

    fn message_count(&self, chat_id: &str) -> Result<usize> {
        let (path, _) = session_path(chat_id)?;
        let buf = match read_file(&path) {
            Ok(buf) => buf,
            Err(_) => return Ok(0),
        };
        Ok(count_session_message_lines(&buf))
    }

    fn clear(&self, chat_id: &str) -> Result<()> {
        let (path, write_header) = session_path(chat_id)?;
        if write_header {
            let mut empty = String::from(CHAT_ID_HEADER_PREFIX);
            empty.push_str(chat_id);
            empty.push('\n');
            write_file(&path, empty.as_bytes())
        } else {
            write_file(&path, b"")
        }
    }

    fn list_chat_ids(&self) -> Result<Vec<String>> {
        let mut p = state_mount_path();
        p.push(REL_PATH_SESSIONS_DIR);
        let names = match list_dir(&p) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("[{}] list_dir {:?} failed: {}", TAG, p, e);
                return Ok(Vec::new());
            }
        };
        let mut out: Vec<String> = Vec::with_capacity(MAX_LIST_CHAT_IDS.min(names.len()));
        for name in names {
            if !name.ends_with(SESSION_FILE_EXT) {
                continue;
            }
            let stem = name.trim_end_matches(SESSION_FILE_EXT);
            // 短 chat_id 直接作文件名，无首行 header，无需读文件；仅 8 位 hex 哈希名需读首行取真实 chat_id
            let chat_id = if stem.len() == 8 && stem.chars().all(|c| c.is_ascii_hexdigit()) {
                p.push(&name);
                let id = if let Ok(buf) = read_file(&p) {
                    let first_line = buf
                        .split(|&b| b == b'\n')
                        .next()
                        .and_then(|line| std::str::from_utf8(line).ok());
                    first_line
                        .and_then(parse_chat_id_header)
                        .unwrap_or_else(|| stem.to_string())
                } else {
                    stem.to_string()
                };
                p.pop();
                id
            } else {
                stem.to_string()
            };
            if !chat_id.is_empty() {
                out.push(chat_id);
                if out.len() >= MAX_LIST_CHAT_IDS {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn gc_stale(&self, max_age_secs: u64) -> Result<usize> {
        let mut p = state_mount_path();
        p.push(REL_PATH_SESSIONS_DIR);
        let names = match list_dir(&p) {
            Ok(n) => n,
            Err(_) => return Ok(0),
        };
        let now = std::time::SystemTime::now();
        let mut removed = 0usize;
        for name in &names {
            if !name.ends_with(SESSION_FILE_EXT) {
                continue;
            }
            p.push(name);
            let stale = match std::fs::metadata(p.as_path()) {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => now
                        .duration_since(mtime)
                        .map(|d| d.as_secs() > max_age_secs)
                        .unwrap_or(false),
                    Err(_) => false,
                },
                Err(_) => false,
            };
            if stale && super::remove_file(&p).is_ok() {
                removed += 1;
                log::info!("[{}] gc: removed stale session file {:?}", TAG, name);
            }
            p.pop();
        }
        if removed > 0 {
            log::info!("[{}] gc: cleaned {} stale session files", TAG, removed);
        }
        Ok(removed)
    }

    fn delete(&self, chat_id: &str) -> Result<()> {
        let (path, _) = session_path(chat_id)?;
        if path.exists() {
            super::remove_file(&path)?;
        }
        Ok(())
    }
}
